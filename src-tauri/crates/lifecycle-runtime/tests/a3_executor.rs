#[allow(dead_code)]
mod fixtures;

use deltamod_lifecycle_runtime::{
    file_plan_fingerprint, DurableLifecycleStore, ExecutionIdentity, FailOnce, FaultPoint,
    InstallFilePlan, InstallMetadata, JournalCheckpointKind, LifecycleOutcome, LockedProfileMod,
    ManualClock, ProfileDefinition, ProfileLockfile, ProfileModDefinition, ProfileSwitchPlan,
    ReleaseARuntime, StagingSource, StartupRecoveryOutcome, ValidatedInstallPlan,
};
use deltamod_product_contracts::{
    JournalDisposition, LifecycleOperationKind, ManifestCommitState, MutationCheckpoint,
    OperationIntent, OperationPhase, OperationRequest, OperationState, ProviderArtifactKind,
    ProviderId, ProviderItemKind, ProviderRef, ProviderResourceId, ValidatedRelativePath,
};
use fixtures::{state_hash, ModelWorkspace};
use std::sync::Arc;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Clone)]
struct Spec {
    instance_id: &'static str,
    mod_id: &'static str,
    version: &'static str,
    path: &'static str,
    hash: &'static str,
}

fn provider(instance_id: &str, version: &str) -> ProviderRef {
    ProviderRef::new(
        ProviderId::parse("local").unwrap(),
        ProviderItemKind::LocalArchive,
        ProviderResourceId::parse(instance_id).unwrap(),
        None,
        Some(ProviderResourceId::parse(instance_id).unwrap()),
        ProviderArtifactKind::Archive,
        Some(ProviderResourceId::parse(version).unwrap()),
        None,
    )
    .unwrap()
}

fn source_id(spec: &Spec) -> String {
    format!("source-{}-{}", spec.instance_id, spec.version)
}

fn files(spec: &Spec) -> Vec<InstallFilePlan> {
    vec![InstallFilePlan {
        path: ValidatedRelativePath::parse(spec.path).unwrap(),
        path_identity_key: spec.path.to_owned(),
        sha256: spec.hash.to_owned(),
        size_bytes: 1,
        expected_previous_sha256: None,
        source: StagingSource::Artifact {
            source_id: source_id(spec),
        },
    }]
}

fn lock(profile_id: &str, specs: &[Spec]) -> ProfileLockfile {
    let definition = ProfileDefinition::new(
        profile_id,
        "game",
        "installation",
        specs
            .iter()
            .enumerate()
            .map(|(order, spec)| ProfileModDefinition {
                order: order as u32,
                instance_id: spec.instance_id.into(),
                mod_id: spec.mod_id.into(),
                display_name: format!("Mod {}", spec.mod_id),
                provider: provider(spec.instance_id, spec.version),
                configuration_fingerprint: None,
            })
            .collect(),
    )
    .unwrap();
    ProfileLockfile::new(
        &definition,
        specs
            .iter()
            .enumerate()
            .map(|(order, spec)| LockedProfileMod {
                order: order as u32,
                instance_id: spec.instance_id.into(),
                mod_id: spec.mod_id.into(),
                display_name: format!("Mod {}", spec.mod_id),
                version: Some(spec.version.into()),
                provider: provider(spec.instance_id, spec.version),
                archive_sha256: spec.hash.into(),
                file_plan_fingerprint: file_plan_fingerprint(&files(spec)),
                configuration_fingerprint: None,
            })
            .collect(),
    )
    .unwrap()
}

fn resolved(
    plan: &ProfileSwitchPlan,
    target: &ProfileLockfile,
    spec: &Spec,
) -> ValidatedInstallPlan {
    let operation = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == spec.instance_id)
        .unwrap();
    let file_plan = files(spec);
    let fingerprint = file_plan_fingerprint(&file_plan);
    let provider = provider(spec.instance_id, spec.version);
    let request = OperationRequest::new(
        operation.operation_id(),
        operation.idempotency_key(),
        OperationIntent {
            installation_id: "installation".into(),
            kind: operation.kind(),
            mod_instance_id: Some(spec.instance_id.into()),
            provider: Some(provider.clone()),
            archive_sha256: Some(spec.hash.into()),
            file_plan_fingerprint: Some(fingerprint),
            profile_id: Some(target.profile_id.clone()),
        },
    )
    .unwrap();
    ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: spec.instance_id.into(),
            mod_id: spec.mod_id.into(),
            display_name: format!("Mod {}", spec.mod_id),
            version: Some(spec.version.into()),
            provider,
            archive_sha256: Some(spec.hash.into()),
        },
        file_plan,
    )
    .unwrap()
}

fn outer_request(
    operation_id: &str,
    idempotency_key: &str,
    plan: &ProfileSwitchPlan,
) -> OperationRequest {
    OperationRequest::new(
        operation_id,
        idempotency_key,
        OperationIntent {
            installation_id: "installation".into(),
            kind: LifecycleOperationKind::ProfileSwitch,
            mod_instance_id: None,
            provider: None,
            archive_sha256: None,
            file_plan_fingerprint: Some(plan.fingerprint().into()),
            profile_id: Some(plan.target_profile_id().into()),
        },
    )
    .unwrap()
}

fn identity(lease_id: &str, generation_id: &str, ttl_ms: u64) -> ExecutionIdentity {
    ExecutionIdentity {
        owner_instance_id: "profile-executor".into(),
        lease_id: lease_id.into(),
        recovery_generation_id: generation_id.into(),
        now_ms: 100,
        lease_ttl_ms: ttl_ms,
    }
}

fn add_sources(workspace: &mut ModelWorkspace, specs: &[Spec]) {
    for spec in specs {
        workspace.add_source(&source_id(spec), spec.hash);
    }
}

fn bootstrap(
    runtime: &mut ReleaseARuntime,
    workspace: &mut ModelWorkspace,
    spec: &Spec,
) -> ProfileLockfile {
    let target = lock("profile-old", std::slice::from_ref(spec));
    let plan = ProfileSwitchPlan::build(None, None, &target).unwrap();
    add_sources(workspace, std::slice::from_ref(spec));
    let outcome = runtime.switch_profile(
        outer_request("switch-bootstrap", "switch-bootstrap-key", &plan),
        plan.clone(),
        target.clone(),
        vec![resolved(&plan, &target, spec)],
        identity("lease-bootstrap", "generation-bootstrap", 10_000),
        workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::Succeeded { .. }));
    target
}

#[test]
fn switch_uses_one_outer_journal_commits_pointer_and_replays_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
    let mut workspace = ModelWorkspace::new();
    let specs = [
        Spec {
            instance_id: "first",
            mod_id: "mod-first",
            version: "v1",
            path: "mods/first.dat",
            hash: A,
        },
        Spec {
            instance_id: "second",
            mod_id: "mod-second",
            version: "v1",
            path: "mods/second.dat",
            hash: B,
        },
    ];
    let target = lock("profile-target", &specs);
    let plan = ProfileSwitchPlan::build(None, None, &target).unwrap();
    let request = outer_request("switch-outer", "switch-outer-key", &plan);
    let resolved_plans = specs
        .iter()
        .map(|spec| resolved(&plan, &target, spec))
        .collect::<Vec<_>>();
    add_sources(&mut workspace, &specs);

    let outcome = runtime.switch_profile(
        request.clone(),
        plan.clone(),
        target.clone(),
        resolved_plans.clone(),
        identity("lease-outer", "generation-outer", 60_000),
        &mut workspace,
    );
    let LifecycleOutcome::Succeeded {
        operation,
        journal: Some(journal),
        preflight,
    } = outcome
    else {
        panic!("profile switch did not succeed");
    };
    assert_eq!(operation.request.operation_id(), "switch-outer");
    assert_eq!(
        operation.request.intent().kind,
        LifecycleOperationKind::ProfileSwitch
    );
    assert_eq!(journal.operation, LifecycleOperationKind::ProfileSwitch);
    assert_eq!(journal.mutations.len(), 2);
    assert_eq!(preflight.kind, LifecycleOperationKind::ProfileSwitch);
    assert_eq!(preflight.files.len(), 2);

    let manifest = runtime.store().manifest("installation").unwrap().unwrap();
    let pointer = runtime
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    assert_eq!(pointer.profile_id(), "profile-target");
    assert_eq!(pointer.lock_fingerprint(), target.fingerprint().unwrap());
    assert_eq!(pointer.manifest_generation(), manifest.generation());
    assert_eq!(pointer.manifest_fingerprint(), manifest.fingerprint());
    let operations = runtime.store().operations().unwrap();
    assert_eq!(operations.len(), 1);
    assert!(plan.operations().iter().all(|child| operations
        .iter()
        .all(|record| record.request.operation_id() != child.operation_id())));
    assert_eq!(runtime.store().journals().unwrap().len(), 1);

    let replay = runtime.switch_profile(
        request,
        plan,
        target,
        resolved_plans,
        identity("lease-replay", "generation-replay", 60_000),
        &mut workspace,
    );
    assert!(matches!(replay, LifecycleOutcome::Existing { .. }));
    assert_eq!(runtime.store().operations().unwrap().len(), 1);
    assert_eq!(runtime.store().journals().unwrap().len(), 1);
}

#[test]
fn child_failure_rolls_back_the_complete_batch_and_keeps_previous_profile() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
    let mut workspace = ModelWorkspace::new();
    let old = Spec {
        instance_id: "old",
        mod_id: "mod-old",
        version: "v1",
        path: "mods/old.dat",
        hash: A,
    };
    let previous_lock = bootstrap(&mut runtime, &mut workspace, &old);
    let previous_manifest = runtime.store().manifest("installation").unwrap().unwrap();
    let previous_pointer = runtime
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    let new = Spec {
        instance_id: "new",
        mod_id: "mod-new",
        version: "v1",
        path: "mods/new.dat",
        hash: B,
    };
    let target = lock("profile-new", std::slice::from_ref(&new));
    let plan =
        ProfileSwitchPlan::build(Some(&previous_manifest), Some(&previous_lock), &target).unwrap();
    let child = resolved(&plan, &target, &new);
    add_sources(&mut workspace, std::slice::from_ref(&new));
    workspace.fail_publish_once(1);

    let outcome = runtime.switch_profile(
        outer_request("switch-fail", "switch-fail-key", &plan),
        plan,
        target,
        vec![child],
        identity("lease-fail", "generation-fail", 60_000),
        &mut workspace,
    );
    let LifecycleOutcome::Rejected {
        operation: Some(operation),
        ..
    } = outcome
    else {
        panic!("failed child did not roll the batch back");
    };
    assert_eq!(operation.state, OperationState::Failed);
    assert_eq!(
        runtime.store().manifest("installation").unwrap(),
        Some(previous_manifest)
    );
    assert_eq!(
        runtime.store().active_profile("installation").unwrap(),
        Some(previous_pointer)
    );
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/old.dat")),
        Some(A)
    );
    assert!(state_hash(&workspace.transaction_state("mods/new.dat")).is_none());
    let journal = runtime
        .store()
        .journal_by_operation("switch-fail")
        .unwrap()
        .unwrap();
    assert_eq!(journal.phase, OperationPhase::Complete);
    assert_eq!(
        journal.manifest_commit_state,
        ManifestCommitState::NotStarted
    );
    assert!(journal.mutations.iter().all(|mutation| matches!(
        mutation.checkpoint,
        MutationCheckpoint::NoEffect | MutationCheckpoint::RolledBack
    )));
}

#[test]
fn pointer_only_switch_verifies_retained_files_and_commits_without_a_crash_window() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
    let mut workspace = ModelWorkspace::new();
    let retained = Spec {
        instance_id: "retained",
        mod_id: "mod-retained",
        version: "v1",
        path: "mods/retained.dat",
        hash: A,
    };
    let previous_lock = bootstrap(&mut runtime, &mut workspace, &retained);
    let previous_manifest = runtime.store().manifest("installation").unwrap().unwrap();
    let previous_pointer = runtime
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    let target = lock("profile-alias", std::slice::from_ref(&retained));
    let plan =
        ProfileSwitchPlan::build(Some(&previous_manifest), Some(&previous_lock), &target).unwrap();
    assert!(plan.operations().is_empty());

    workspace.replace_transaction("mods/retained.dat", C);
    let rejected = runtime.switch_profile(
        outer_request("switch-retained-stale", "switch-retained-stale-key", &plan),
        plan.clone(),
        target.clone(),
        Vec::new(),
        identity("lease-retained-stale", "generation-retained-stale", 60_000),
        &mut workspace,
    );
    assert!(matches!(rejected, LifecycleOutcome::Rejected { .. }));
    assert_eq!(
        runtime.store().manifest("installation").unwrap(),
        Some(previous_manifest.clone())
    );
    assert_eq!(
        runtime.store().active_profile("installation").unwrap(),
        Some(previous_pointer)
    );

    workspace.replace_transaction("mods/retained.dat", A);
    let outcome = runtime.switch_profile(
        outer_request("switch-retained", "switch-retained-key", &plan),
        plan,
        target.clone(),
        Vec::new(),
        identity("lease-retained", "generation-retained", 60_000),
        &mut workspace,
    );
    let LifecycleOutcome::Succeeded { journal, .. } = outcome else {
        panic!("pointer-only switch did not succeed");
    };
    assert!(journal.is_none());
    let manifest = runtime.store().manifest("installation").unwrap().unwrap();
    let pointer = runtime
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    assert_eq!(manifest.generation(), previous_manifest.generation() + 1);
    assert_eq!(pointer.profile_id(), "profile-alias");
    assert_eq!(pointer.lock_fingerprint(), target.fingerprint().unwrap());
    assert_eq!(pointer.manifest_generation(), manifest.generation());
    assert_eq!(pointer.manifest_fingerprint(), manifest.fingerprint());
    assert_eq!(runtime.store().journals().unwrap().len(), 1);

    let exact_plan = ProfileSwitchPlan::build(Some(&manifest), Some(&target), &target).unwrap();
    let exact = runtime.switch_profile(
        outer_request("switch-exact-noop", "switch-exact-noop-key", &exact_plan),
        exact_plan,
        target,
        Vec::new(),
        identity("lease-exact-noop", "generation-exact-noop", 60_000),
        &mut workspace,
    );
    assert!(matches!(
        exact,
        LifecycleOutcome::Succeeded { journal: None, .. }
    ));
    assert_eq!(
        runtime.store().manifest("installation").unwrap(),
        Some(manifest.clone())
    );
    assert_eq!(
        runtime.store().active_profile("installation").unwrap(),
        Some(pointer)
    );
    assert_eq!(runtime.store().journals().unwrap().len(), 1);
}

#[test]
fn crash_after_applied_effect_recovers_by_rolling_back_the_outer_journal() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(100);
    let store =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap();
    let mut runtime = ReleaseARuntime::new(store);
    let mut workspace = ModelWorkspace::new();
    let old = Spec {
        instance_id: "mod",
        mod_id: "mod",
        version: "v1",
        path: "mods/mod.dat",
        hash: A,
    };
    let previous_lock = bootstrap(&mut runtime, &mut workspace, &old);
    let previous_manifest = runtime.store().manifest("installation").unwrap().unwrap();
    let previous_pointer = runtime
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    let updated = Spec {
        version: "v2",
        hash: B,
        ..old.clone()
    };
    let target = lock("profile-updated", std::slice::from_ref(&updated));
    let plan =
        ProfileSwitchPlan::build(Some(&previous_manifest), Some(&previous_lock), &target).unwrap();
    let child = resolved(&plan, &target, &updated);
    add_sources(&mut workspace, std::slice::from_ref(&updated));
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(FaultPoint::AfterJournalCas(
            JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            },
        )));

    let outcome = runtime.switch_profile(
        outer_request("switch-crash-rollback", "switch-crash-rollback-key", &plan),
        plan,
        target,
        vec![child],
        identity("lease-crash-rollback", "generation-crash-rollback", 10),
        &mut workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::RecoveryRequired { .. }));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/mod.dat")),
        Some(B)
    );
    assert_eq!(
        runtime.store().manifest("installation").unwrap(),
        Some(previous_manifest.clone())
    );
    assert_eq!(
        runtime.store().active_profile("installation").unwrap(),
        Some(previous_pointer.clone())
    );

    clock.advance(11).unwrap();
    let reopened =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock)).unwrap();
    let mut recovered = ReleaseARuntime::new(reopened);
    let outcomes = recovered.recover_startup(
        "recovery-owner",
        111,
        10,
        |_| "lease-recovery-rollback".into(),
        &mut workspace,
    );
    assert!(matches!(
        outcomes.as_slice(),
        [StartupRecoveryOutcome::Recovered {
            disposition: JournalDisposition::RollBack,
            ..
        }]
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/mod.dat")),
        Some(A)
    );
    assert_eq!(
        recovered.store().manifest("installation").unwrap(),
        Some(previous_manifest)
    );
    assert_eq!(
        recovered.store().active_profile("installation").unwrap(),
        Some(previous_pointer)
    );
}

#[test]
fn crash_after_full_verification_recovers_manifest_and_pointer_together() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(100);
    let store =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap();
    let mut runtime = ReleaseARuntime::new(store);
    let mut workspace = ModelWorkspace::new();
    let old = Spec {
        instance_id: "mod",
        mod_id: "mod",
        version: "v1",
        path: "mods/mod.dat",
        hash: A,
    };
    let previous_lock = bootstrap(&mut runtime, &mut workspace, &old);
    let previous_manifest = runtime.store().manifest("installation").unwrap().unwrap();
    let previous_pointer = runtime
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    let updated = Spec {
        version: "v3",
        hash: C,
        ..old
    };
    let target = lock("profile-verified", std::slice::from_ref(&updated));
    let target_fingerprint = target.fingerprint().unwrap();
    let plan =
        ProfileSwitchPlan::build(Some(&previous_manifest), Some(&previous_lock), &target).unwrap();
    let child = resolved(&plan, &target, &updated);
    add_sources(&mut workspace, std::slice::from_ref(&updated));
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(FaultPoint::BeforeJournalCas(
            JournalCheckpointKind::ManifestTemporary,
        )));

    let outcome = runtime.switch_profile(
        outer_request("switch-crash-commit", "switch-crash-commit-key", &plan),
        plan,
        target,
        vec![child],
        identity("lease-crash-commit", "generation-crash-commit", 10),
        &mut workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::RecoveryRequired { .. }));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/mod.dat")),
        Some(C)
    );
    assert_eq!(
        runtime.store().manifest("installation").unwrap(),
        Some(previous_manifest.clone())
    );
    assert_eq!(
        runtime.store().active_profile("installation").unwrap(),
        Some(previous_pointer)
    );

    clock.advance(11).unwrap();
    let reopened =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock)).unwrap();
    let mut recovered = ReleaseARuntime::new(reopened);
    let outcomes = recovered.recover_startup(
        "recovery-owner",
        111,
        10,
        |_| "lease-recovery-commit".into(),
        &mut workspace,
    );
    assert!(matches!(
        outcomes.as_slice(),
        [StartupRecoveryOutcome::Recovered {
            disposition: JournalDisposition::FinalizeVerifiedCommit,
            ..
        }]
    ));
    let manifest = recovered.store().manifest("installation").unwrap().unwrap();
    let pointer = recovered
        .store()
        .active_profile("installation")
        .unwrap()
        .unwrap();
    assert_eq!(manifest.generation(), previous_manifest.generation() + 1);
    assert_eq!(manifest.records[0].version.as_deref(), Some("v3"));
    assert_eq!(pointer.profile_id(), "profile-verified");
    assert_eq!(pointer.lock_fingerprint(), target_fingerprint);
    assert_eq!(pointer.manifest_generation(), manifest.generation());
    assert_eq!(pointer.manifest_fingerprint(), manifest.fingerprint());
}
