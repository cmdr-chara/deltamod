mod fixtures;

use deltamod_lifecycle_runtime::*;
use deltamod_product_contracts::{
    AcquireOutcome, GameHealthState, InstallationClaimsLedger, InstallationClaimsLedgerPayload,
    JournalDisposition, LifecycleOperationKind, OperationIntent, OperationRequest, OperationState,
    OperationStore, ProductErrorCode, RecoveryAction, ValidatedRelativePath, VerificationState,
};
use fixtures::{state_hash, ModelWorkspace};
use std::sync::Arc;

const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const H3: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const H4: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const ARCHIVE_V1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ARCHIVE_V2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Clone, Default)]
struct Catalog {
    cached: Option<CachedArchiveSource>,
    provider: Option<ProviderArtifactSource>,
}

impl RepairSourceCatalog for Catalog {
    fn cached_archive(&self, _expected_archive_sha256: &str) -> Option<CachedArchiveSource> {
        self.cached.clone()
    }

    fn exact_provider_artifact(
        &self,
        _expected_provider: &deltamod_product_contracts::ProviderRef,
    ) -> Option<ProviderArtifactSource> {
        self.provider.clone()
    }
}

// Keeping both file identities explicit makes the transactional before/after
// assertions below readable without introducing a test-only builder.
#[allow(clippy::too_many_arguments)]
fn install_plan(
    operation_id: &str,
    key: &str,
    kind: LifecycleOperationKind,
    version: &str,
    archive_sha256: &str,
    first_source: &str,
    first_hash: &str,
    second_source: &str,
    second_hash: &str,
) -> ValidatedInstallPlan {
    let files = vec![
        InstallFilePlan {
            path: ValidatedRelativePath::parse("mods/a.dat").unwrap(),
            path_identity_key: "mods/a.dat".into(),
            sha256: first_hash.into(),
            size_bytes: 4,
            expected_previous_sha256: None,
            source: StagingSource::Artifact {
                source_id: first_source.into(),
            },
        },
        InstallFilePlan {
            path: ValidatedRelativePath::parse("mods/b.dat").unwrap(),
            path_identity_key: "mods/b.dat".into(),
            sha256: second_hash.into(),
            size_bytes: 4,
            expected_previous_sha256: None,
            source: StagingSource::Artifact {
                source_id: second_source.into(),
            },
        },
    ];
    let provider = deltamod_product_contracts::fixtures::provider_ref();
    let request = OperationRequest::new(
        operation_id,
        key,
        OperationIntent {
            installation_id: "game".into(),
            kind,
            mod_instance_id: Some("mod-a".into()),
            provider: Some(provider.clone()),
            archive_sha256: Some(archive_sha256.into()),
            file_plan_fingerprint: Some(file_plan_fingerprint(&files)),
            profile_id: None,
        },
    )
    .unwrap();
    ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: "mod-a".into(),
            mod_id: "mod".into(),
            display_name: "Mod".into(),
            version: Some(version.into()),
            provider,
            archive_sha256: Some(archive_sha256.into()),
        },
        files,
    )
    .unwrap()
}

fn repair_request(runtime: &ReleaseARuntime, operation_id: &str, key: &str) -> OperationRequest {
    let manifest = runtime.store().manifest("game").unwrap().unwrap();
    let installed = manifest
        .records
        .iter()
        .find(|record| record.instance_id == "mod-a")
        .unwrap();
    OperationRequest::new(
        operation_id,
        key,
        OperationIntent {
            installation_id: "game".into(),
            kind: LifecycleOperationKind::Repair,
            mod_instance_id: Some(installed.instance_id.clone()),
            provider: Some(installed.provider.clone()),
            archive_sha256: installed.archive_sha256.clone(),
            file_plan_fingerprint: Some(installed.file_plan_fingerprint.clone()),
            profile_id: None,
        },
    )
    .unwrap()
}

fn restore_request(operation_id: &str, key: &str) -> OperationRequest {
    OperationRequest::new(
        operation_id,
        key,
        OperationIntent {
            installation_id: "game".into(),
            kind: LifecycleOperationKind::Recover,
            mod_instance_id: None,
            provider: None,
            archive_sha256: None,
            file_plan_fingerprint: None,
            profile_id: None,
        },
    )
    .unwrap()
}

fn identity(lease_id: &str, generation_id: &str) -> ExecutionIdentity {
    ExecutionIdentity {
        owner_instance_id: "a2-runtime".into(),
        lease_id: lease_id.into(),
        recovery_generation_id: generation_id.into(),
        now_ms: 1,
        lease_ttl_ms: 100,
    }
}

fn assert_succeeded(outcome: LifecycleOutcome) {
    assert!(
        matches!(outcome, LifecycleOutcome::Succeeded { .. }),
        "expected successful lifecycle outcome, received {outcome:#?}"
    );
}

fn setup() -> (tempfile::TempDir, ReleaseARuntime, ModelWorkspace) {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(1);
    let store = DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock)).unwrap();
    let mut workspace = ModelWorkspace::new();
    for (source, hash) in [
        ("v1-a", H1),
        ("v1-b", H2),
        ("v2-a", H3),
        ("v2-b", H4),
        ("repair-cache", H1),
        ("repair-provider", H1),
    ] {
        workspace.add_source(source, hash);
    }
    (directory, ReleaseARuntime::new(store), workspace)
}

#[test]
fn failed_transactional_update_rolls_back_and_replays_without_touching_v1() {
    let (_directory, mut runtime, mut workspace) = setup();
    assert_succeeded(runtime.install(
        install_plan(
            "install-v1",
            "install-v1-key",
            LifecycleOperationKind::Install,
            "1",
            ARCHIVE_V1,
            "v1-a",
            H1,
            "v1-b",
            H2,
        ),
        identity("install-v1-lease", "install-v1-generation"),
        &mut workspace,
    ));
    workspace.fail_publish_once(1);
    let outcome = runtime.update(
        install_plan(
            "update-v2",
            "update-v2-key",
            LifecycleOperationKind::Update,
            "2",
            ARCHIVE_V2,
            "v2-a",
            H3,
            "v2-b",
            H4,
        ),
        identity("update-v2-lease", "update-v2-generation"),
        &mut workspace,
    );
    assert!(matches!(
        outcome,
        LifecycleOutcome::Rejected {
            operation: Some(operation),
            ..
        } if operation.state == OperationState::Failed
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/a.dat")),
        Some(H1)
    );
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/b.dat")),
        Some(H2)
    );
    let manifest = runtime.store().manifest("game").unwrap().unwrap();
    assert_eq!(manifest.records[0].version.as_deref(), Some("1"));
    let status = runtime.operation_status("update-v2").unwrap().unwrap();
    assert_eq!(status.operation.state, OperationState::Failed);
    assert_eq!(status.progress.state, OperationState::Failed);
    assert_eq!(
        status.recovery_disposition,
        Some(JournalDisposition::Complete)
    );

    let replay = runtime.update(
        install_plan(
            "update-v2",
            "update-v2-key",
            LifecycleOperationKind::Update,
            "2",
            ARCHIVE_V2,
            "v2-a",
            H3,
            "v2-b",
            H4,
        ),
        identity("replay-lease", "replay-generation"),
        &mut workspace,
    );
    assert!(matches!(
        replay,
        LifecycleOutcome::Existing { operation } if operation.state == OperationState::Failed
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/a.dat")),
        Some(H1)
    );
}

#[test]
fn verification_and_repair_enforce_exact_source_priority_and_external_change_policy() {
    let (_directory, mut runtime, mut workspace) = setup();
    assert_succeeded(runtime.install(
        install_plan(
            "install-repair",
            "install-repair-key",
            LifecycleOperationKind::Install,
            "1",
            ARCHIVE_V1,
            "v1-a",
            H1,
            "v1-b",
            H2,
        ),
        identity("install-repair-lease", "install-repair-generation"),
        &mut workspace,
    ));
    let provider = deltamod_product_contracts::fixtures::provider_ref();
    let catalog = Catalog {
        cached: Some(CachedArchiveSource {
            source_id: "repair-cache".into(),
            archive_sha256: ARCHIVE_V1.into(),
        }),
        provider: Some(ProviderArtifactSource {
            source_id: "repair-provider".into(),
            provider: provider.clone(),
            archive_sha256: Some(ARCHIVE_V1.into()),
        }),
    };
    workspace.remove_transaction("mods/a.dat");
    let scope = VerificationScope::new("game", Some("mod-a".into())).unwrap();
    let report = runtime
        .verify_installation_with_sources(&scope, &workspace, &catalog)
        .unwrap();
    assert_eq!(report.verification.state, VerificationState::MissingFiles);
    assert_eq!(report.health.state, GameHealthState::MissingFiles);
    assert_eq!(
        report.archive_identities[0].state,
        ArchiveIdentityState::ExactCachedArchive
    );
    let repair = runtime
        .plan_repair(
            repair_request(&runtime, "repair-missing", "repair-missing-key"),
            &workspace,
            &catalog,
        )
        .unwrap();
    let RepairPlanDisposition::Ready(plan) = repair else {
        panic!("expected repair plan");
    };
    assert!(matches!(plan.source(), RepairSource::CachedArchive(_)));
    assert_eq!(plan.report().health.state, GameHealthState::RepairAvailable);
    assert_succeeded(runtime.repair(
        *plan,
        identity("repair-missing-lease", "repair-missing-generation"),
        &mut workspace,
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/a.dat")),
        Some(H1)
    );
    assert_eq!(
        runtime
            .verify_installation(&scope, &workspace)
            .unwrap()
            .verification
            .state,
        VerificationState::Healthy
    );

    workspace.replace_transaction("mods/b.dat", H3);
    let changed = runtime.verify_installation(&scope, &workspace).unwrap();
    assert_eq!(changed.verification.state, VerificationState::HashMismatch);
    assert_eq!(
        changed.health.state,
        GameHealthState::ExternalChangesDetected
    );
    let blocked = runtime
        .plan_repair(
            repair_request(&runtime, "repair-changed", "repair-changed-key"),
            &workspace,
            &catalog,
        )
        .unwrap_err();
    assert_eq!(blocked.error.code, ProductErrorCode::ExternalModification);
    assert_eq!(
        blocked.error.recovery_action,
        RecoveryAction::ResolveConflict
    );

    workspace.replace_transaction("mods/b.dat", H2);
    workspace.remove_transaction("mods/a.dat");
    let source_required = runtime
        .plan_repair(
            repair_request(&runtime, "repair-source", "repair-source-key"),
            &workspace,
            &NoRepairSources,
        )
        .unwrap_err();
    assert_eq!(
        source_required.error.code,
        ProductErrorCode::RecoveryUnavailable
    );
    assert_eq!(
        source_required.error.recovery_action,
        RecoveryAction::SelectExactSource
    );
    let substituted_provider = deltamod_product_contracts::ProviderRef::new(
        provider.provider_id().clone(),
        provider.item_kind(),
        provider.resource_id().clone(),
        provider.scope().cloned(),
        provider.artifact_id().cloned(),
        provider.artifact_kind(),
        Some(
            deltamod_product_contracts::ProviderResourceId::parse("2.0.0")
                .expect("substitute version identity"),
        ),
        provider.canonical_url().map(str::to_owned),
    )
    .expect("substitute provider reference");
    let substitution = runtime
        .plan_repair(
            repair_request(&runtime, "repair-substitute", "repair-substitute-key"),
            &workspace,
            &Catalog {
                cached: None,
                provider: Some(ProviderArtifactSource {
                    source_id: "repair-substitute".into(),
                    provider: substituted_provider,
                    archive_sha256: Some(ARCHIVE_V1.into()),
                }),
            },
        )
        .unwrap_err();
    assert_eq!(
        substitution.error.code,
        ProductErrorCode::RecoveryUnavailable
    );
    assert_eq!(
        substitution.error.recovery_action,
        RecoveryAction::SelectExactSource
    );
    let provider_only = Catalog {
        cached: None,
        provider: Some(ProviderArtifactSource {
            source_id: "repair-provider".into(),
            provider,
            archive_sha256: Some(ARCHIVE_V1.into()),
        }),
    };
    let fallback = runtime
        .plan_repair(
            repair_request(&runtime, "repair-provider", "repair-provider-key"),
            &workspace,
            &provider_only,
        )
        .unwrap();
    let RepairPlanDisposition::Ready(plan) = fallback else {
        panic!("expected provider repair plan");
    };
    assert!(matches!(plan.source(), RepairSource::ProviderArtifact(_)));
}

#[test]
fn restore_source_is_durably_protected_and_duplicate_or_parallel_mutations_do_not_run() {
    let (_directory, mut runtime, mut workspace) = setup();
    assert_succeeded(runtime.install(
        install_plan(
            "restore-install",
            "restore-install-key",
            LifecycleOperationKind::Install,
            "1",
            ARCHIVE_V1,
            "v1-a",
            H1,
            "v1-b",
            H2,
        ),
        identity("restore-install-lease", "restore-install-generation"),
        &mut workspace,
    ));
    assert_succeeded(runtime.update(
        install_plan(
            "restore-update",
            "restore-update-key",
            LifecycleOperationKind::Update,
            "2",
            ARCHIVE_V2,
            "v2-a",
            H3,
            "v2-b",
            H4,
        ),
        identity("restore-update-lease", "restore-update-generation"),
        &mut workspace,
    ));
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(FaultPoint::BeforeStagingEffect { index: 0 }));
    let request = restore_request("restore-last", "restore-last-key");
    let outcome = runtime.restore_last_working_state(
        request.clone(),
        identity("restore-last-lease", "restore-last-generation"),
        &mut workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::RecoveryRequired { .. }));
    let source = runtime
        .store()
        .latest_recovery_generation("game")
        .unwrap()
        .unwrap();
    assert_eq!(source.generation_id, "restore-update-generation");
    assert!(source.protected_by_operations.contains("restore-last"));
    let status = runtime.operation_status("restore-last").unwrap().unwrap();
    assert_eq!(
        status.recovery_disposition,
        Some(JournalDisposition::RollBack)
    );

    runtime.store_mut().clear_fault_injector();
    let duplicate = runtime.restore_last_working_state(
        request,
        identity("restore-duplicate-lease", "restore-duplicate-generation"),
        &mut workspace,
    );
    assert!(matches!(duplicate, LifecycleOutcome::Existing { .. }));
    let parallel = runtime.update(
        install_plan(
            "parallel-update",
            "parallel-update-key",
            LifecycleOperationKind::Update,
            "3",
            ARCHIVE_V2,
            "v2-a",
            H3,
            "v2-b",
            H4,
        ),
        identity("parallel-update-lease", "parallel-update-generation"),
        &mut workspace,
    );
    assert!(matches!(parallel, LifecycleOutcome::Busy { .. }));
}

#[test]
fn restore_falls_back_when_the_newest_completed_generation_has_no_required_recovery_source() {
    let (_directory, mut runtime, mut workspace) = setup();
    assert_succeeded(runtime.install(
        install_plan(
            "fallback-install",
            "fallback-install-key",
            LifecycleOperationKind::Install,
            "1",
            ARCHIVE_V1,
            "v1-a",
            H1,
            "v1-b",
            H2,
        ),
        identity("fallback-install-lease", "fallback-install-generation"),
        &mut workspace,
    ));
    assert_succeeded(runtime.update(
        install_plan(
            "fallback-update",
            "fallback-update-key",
            LifecycleOperationKind::Update,
            "2",
            ARCHIVE_V2,
            "v2-a",
            H3,
            "v2-b",
            H4,
        ),
        identity("fallback-update-lease", "fallback-update-generation"),
        &mut workspace,
    ));

    // Model a completed generation whose manifest was published but whose
    // required recovery journal is incomplete. The durable store accepts the
    // generation, but restore preflight cannot source its previous bytes.
    let incomplete_plan = install_plan(
        "fallback-incomplete",
        "fallback-incomplete-key",
        LifecycleOperationKind::Update,
        "3",
        ARCHIVE_V2,
        "v1-b",
        H2,
        "v1-a",
        H1,
    );
    let current = runtime.store().manifest("game").unwrap().unwrap();
    let next_generation = current.generation().checked_add(1).unwrap();
    let mut records = current.records.clone();
    for installed in &mut records {
        installed
            .try_update(|payload| {
                payload.version = Some("3".into());
                payload.file_plan_fingerprint = incomplete_plan.fingerprint().into();
                payload.manifest_generation = next_generation;
                for file in &mut payload.files {
                    file.expected_sha256 = match file.path.as_str() {
                        "mods/a.dat" => H2,
                        "mods/b.dat" => H1,
                        path => panic!("unexpected installed path: {path}"),
                    }
                    .into();
                }
            })
            .unwrap();
    }
    let mut claims = current.ledger.claims.clone();
    for claim in &mut claims {
        claim.sha256 = match claim.path.as_str() {
            "mods/a.dat" => H2,
            "mods/b.dat" => H1,
            path => panic!("unexpected claim path: {path}"),
        }
        .into();
    }
    let target = InstallationManifest::new(
        records,
        InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
            installation_id: "game".into(),
            manifest_generation: next_generation,
            updated_at_ms: current.ledger.updated_at_ms,
            claims,
        })
        .unwrap(),
    )
    .unwrap();
    let acquired = runtime
        .store_mut()
        .acquire_or_replay(
            incomplete_plan.request(),
            "fallback-incomplete-writer",
            "fallback-incomplete-lease",
            1,
            100,
        )
        .unwrap();
    let (record, lease) = match acquired {
        AcquireOutcome::Acquired { record, lease } => (record, lease),
        other => panic!("expected incomplete generation lease: {other:?}"),
    };
    workspace.replace_transaction("mods/a.dat", H2);
    workspace.replace_transaction("mods/b.dat", H1);
    runtime
        .store_mut()
        .complete_manifest_only(
            &record,
            &lease,
            "fallback-incomplete-generation",
            &target,
            H1,
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        runtime
            .store()
            .latest_recovery_generation("game")
            .unwrap()
            .unwrap()
            .generation_id,
        "fallback-incomplete-generation"
    );

    assert_succeeded(runtime.restore_last_working_state(
        restore_request("fallback-restore", "fallback-restore-key"),
        identity("fallback-restore-lease", "fallback-restore-generation"),
        &mut workspace,
    ));
    let restored = runtime.store().manifest("game").unwrap().unwrap();
    assert_eq!(restored.records[0].version.as_deref(), Some("1"));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/a.dat")),
        Some(H1)
    );
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/b.dat")),
        Some(H2)
    );
}

#[test]
fn restore_preserves_an_external_destination_even_when_it_matches_an_older_generation() {
    let (_directory, mut runtime, mut workspace) = setup();
    assert_succeeded(runtime.install(
        install_plan(
            "external-install",
            "external-install-key",
            LifecycleOperationKind::Install,
            "1",
            ARCHIVE_V1,
            "v1-a",
            H1,
            "v1-b",
            H2,
        ),
        identity("external-install-lease", "external-install-generation"),
        &mut workspace,
    ));
    assert_succeeded(runtime.update(
        install_plan(
            "external-update",
            "external-update-key",
            LifecycleOperationKind::Update,
            "2",
            ARCHIVE_V2,
            "v2-a",
            H3,
            "v2-b",
            H4,
        ),
        identity("external-update-lease", "external-update-generation"),
        &mut workspace,
    ));

    workspace.replace_transaction("mods/a.dat", H1);
    let external = workspace.transaction_state("mods/a.dat");
    let outcome = runtime.restore_last_working_state(
        restore_request("external-restore", "external-restore-key"),
        identity("external-restore-lease", "external-restore-generation"),
        &mut workspace,
    );
    let LifecycleOutcome::Rejected { error, .. } = outcome else {
        panic!("expected external destination rejection: {outcome:?}");
    };
    assert_eq!(error.code, ProductErrorCode::ExternalModification);
    assert_eq!(workspace.transaction_state("mods/a.dat"), external);
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/b.dat")),
        Some(H4)
    );
    let unchanged = runtime.store().manifest("game").unwrap().unwrap();
    assert_eq!(unchanged.records[0].version.as_deref(), Some("2"));
}
