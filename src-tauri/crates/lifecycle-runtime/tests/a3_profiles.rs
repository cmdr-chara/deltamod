use deltamod_lifecycle_runtime::*;
use deltamod_product_contracts::{
    FileClaim, InstallationClaimsLedger, InstallationClaimsLedgerPayload, InstalledFileRef,
    InstalledModPayload, InstalledModRecord, LifecycleOperationKind, OperationIntent,
    OperationRequest, ProviderArtifactKind, ProviderId, ProviderItemKind, ProviderRef,
    ProviderResourceId, ValidatedRelativePath,
};
use std::collections::{BTreeMap, BTreeSet};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

#[derive(Clone)]
struct Spec {
    locked: LockedProfileMod,
    files: Vec<InstallFilePlan>,
}

fn provider(resource: &str, version: &str) -> ProviderRef {
    ProviderRef::new(
        ProviderId::parse("gamebanana").unwrap(),
        ProviderItemKind::Mod,
        ProviderResourceId::parse(resource).unwrap(),
        None,
        Some(ProviderResourceId::parse(&format!("artifact-{version}")).unwrap()),
        ProviderArtifactKind::File,
        Some(ProviderResourceId::parse(version).unwrap()),
        Some(format!("https://gamebanana.com/mods/{resource}")),
    )
    .unwrap()
}

fn files(path: &str, hash: &str, source: &str) -> Vec<InstallFilePlan> {
    vec![InstallFilePlan {
        path: ValidatedRelativePath::parse(path).unwrap(),
        path_identity_key: path.into(),
        sha256: hash.into(),
        size_bytes: 4,
        expected_previous_sha256: None,
        source: StagingSource::Artifact {
            source_id: source.into(),
        },
    }]
}

#[allow(clippy::too_many_arguments)]
fn spec(
    order: u32,
    instance_id: &str,
    resource: &str,
    version: &str,
    archive: &str,
    path: &str,
    hash: &str,
) -> Spec {
    let planned = files(path, hash, &format!("source-{instance_id}-{version}"));
    Spec {
        locked: LockedProfileMod {
            order,
            instance_id: instance_id.into(),
            mod_id: format!("mod-{instance_id}"),
            display_name: format!("Mod {instance_id}"),
            version: Some(version.into()),
            provider: provider(resource, version),
            archive_sha256: archive.into(),
            file_plan_fingerprint: file_plan_fingerprint(&planned),
            configuration_fingerprint: None,
        },
        files: planned,
    }
}

fn lock(profile_id: &str, specs: &[Spec]) -> (ProfileDefinition, ProfileLockfile) {
    let definition = ProfileDefinition::new(
        profile_id,
        "deltarune",
        "game",
        specs
            .iter()
            .map(|item| ProfileModDefinition {
                order: item.locked.order,
                instance_id: item.locked.instance_id.clone(),
                mod_id: item.locked.mod_id.clone(),
                display_name: item.locked.display_name.clone(),
                provider: item.locked.provider.clone(),
                configuration_fingerprint: item.locked.configuration_fingerprint.clone(),
            })
            .collect(),
    )
    .unwrap();
    let lockfile = ProfileLockfile::new(
        &definition,
        specs.iter().map(|item| item.locked.clone()).collect(),
    )
    .unwrap();
    (definition, lockfile)
}

fn manifest(specs: &[Spec]) -> InstallationManifest {
    manifest_for_installation(specs, "game")
}

fn manifest_for_installation(specs: &[Spec], installation_id: &str) -> InstallationManifest {
    let generation = 7;
    let records = specs
        .iter()
        .map(|item| {
            InstalledModRecord::new(InstalledModPayload {
                instance_id: item.locked.instance_id.clone(),
                mod_id: item.locked.mod_id.clone(),
                installation_id: installation_id.into(),
                display_name: item.locked.display_name.clone(),
                version: item.locked.version.clone(),
                provider: item.locked.provider.clone(),
                archive_sha256: Some(item.locked.archive_sha256.clone()),
                file_plan_fingerprint: item.locked.file_plan_fingerprint.clone(),
                manifest_generation: generation,
                installed_at_ms: 1,
                updated_at_ms: 1,
                files: item
                    .files
                    .iter()
                    .map(|file| InstalledFileRef {
                        path: file.path.clone(),
                        path_identity_key: file.path_identity_key.clone(),
                        expected_sha256: file.sha256.clone(),
                    })
                    .collect(),
            })
            .unwrap()
        })
        .collect();
    let mut claims: BTreeMap<String, FileClaim> = BTreeMap::new();
    for item in specs {
        for file in &item.files {
            claims
                .entry(file.path_identity_key.clone())
                .and_modify(|claim| {
                    assert_eq!(claim.sha256, file.sha256);
                    claim.owners.insert(item.locked.instance_id.clone());
                })
                .or_insert_with(|| FileClaim {
                    path: file.path.clone(),
                    path_identity_key: file.path_identity_key.clone(),
                    sha256: file.sha256.clone(),
                    owners: BTreeSet::from([item.locked.instance_id.clone()]),
                });
        }
    }
    InstallationManifest::new(
        records,
        InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
            installation_id: installation_id.into(),
            manifest_generation: generation,
            updated_at_ms: 1,
            claims: claims.into_values().collect(),
        })
        .unwrap(),
    )
    .unwrap()
}

fn resolved(
    expected: &Spec,
    operation: &PlannedProfileOperation,
    provider_override: Option<ProviderRef>,
    archive_override: Option<&str>,
) -> ValidatedInstallPlan {
    resolved_with_binding(
        expected,
        operation.kind(),
        operation.operation_id(),
        operation.idempotency_key(),
        provider_override,
        archive_override,
    )
}

fn resolved_with_binding(
    expected: &Spec,
    kind: LifecycleOperationKind,
    operation_id: &str,
    idempotency_key: &str,
    provider_override: Option<ProviderRef>,
    archive_override: Option<&str>,
) -> ValidatedInstallPlan {
    let provider = provider_override.unwrap_or_else(|| expected.locked.provider.clone());
    let archive = archive_override.unwrap_or(&expected.locked.archive_sha256);
    let request = OperationRequest::new(
        operation_id,
        idempotency_key,
        OperationIntent {
            installation_id: "game".into(),
            kind,
            mod_instance_id: Some(expected.locked.instance_id.clone()),
            provider: Some(provider.clone()),
            archive_sha256: Some(archive.into()),
            file_plan_fingerprint: Some(file_plan_fingerprint(&expected.files)),
            profile_id: Some("target".into()),
        },
    )
    .unwrap();
    ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: expected.locked.instance_id.clone(),
            mod_id: expected.locked.mod_id.clone(),
            display_name: expected.locked.display_name.clone(),
            version: expected.locked.version.clone(),
            provider,
            archive_sha256: Some(archive.into()),
        },
        expected.files.clone(),
    )
    .unwrap()
}

#[test]
fn canonical_definition_and_lockfile_round_trip_strictly() {
    let specs = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let (definition, lockfile) = lock("target", &specs);
    let definition_json = definition.to_canonical_json().unwrap();
    let lock_json = lockfile.to_canonical_json().unwrap();
    assert_eq!(
        ProfileDefinition::from_canonical_json(&definition_json).unwrap(),
        definition
    );
    assert_eq!(
        ProfileLockfile::from_canonical_json(&lock_json).unwrap(),
        lockfile
    );
    assert_eq!(
        ProfileLockfile::from_canonical_json(&format!("{lock_json}\n")),
        Err(ProfileError::NonCanonical)
    );
}

#[test]
fn future_schema_versions_fail_closed() {
    let specs = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let (_, lockfile) = lock("target", &specs);
    let future = lockfile
        .to_canonical_json()
        .unwrap()
        .replace("\"schemaVersion\":1", "\"schemaVersion\":2");
    assert_eq!(
        ProfileLockfile::from_canonical_json(&future),
        Err(ProfileError::UnsupportedSchema {
            found: 2,
            supported: 1
        })
    );
}

#[test]
fn lock_and_plan_fingerprints_change_when_exact_content_changes() {
    let first = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let second = vec![spec(0, "a", "101", "1", C, "mods/a.dat", B)];
    let (_, first_lock) = lock("target", &first);
    let (_, second_lock) = lock("target", &second);
    assert_ne!(
        first_lock.fingerprint().unwrap(),
        second_lock.fingerprint().unwrap()
    );
    assert_ne!(
        ProfileSwitchPlan::build(None, None, &first_lock)
            .unwrap()
            .fingerprint(),
        ProfileSwitchPlan::build(None, None, &second_lock)
            .unwrap()
            .fingerprint()
    );
}

#[test]
fn duplicate_identity_and_divergent_target_paths_are_rejected() {
    let duplicate = [
        spec(0, "same", "101", "1", A, "mods/a.dat", A),
        spec(1, "same", "102", "1", B, "mods/b.dat", B),
    ];
    assert!(matches!(
        ProfileDefinition::new(
            "target",
            "deltarune",
            "game",
            duplicate
                .iter()
                .map(|item| ProfileModDefinition {
                    order: item.locked.order,
                    instance_id: item.locked.instance_id.clone(),
                    mod_id: item.locked.mod_id.clone(),
                    display_name: item.locked.display_name.clone(),
                    provider: item.locked.provider.clone(),
                    configuration_fingerprint: None,
                })
                .collect()
        ),
        Err(ProfileError::DuplicateInstance(_))
    ));

    let specs = vec![
        spec(0, "a", "101", "1", A, "mods/shared.dat", A),
        spec(1, "b", "102", "1", B, "mods/shared.dat", B),
    ];
    let (_, target) = lock("target", &specs);
    let plan = ProfileSwitchPlan::build(None, None, &target).unwrap();
    let operation_a = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == "a")
        .unwrap();
    let operation_b = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == "b")
        .unwrap();
    let report = plan.preflight(
        None,
        &[
            resolved(&specs[0], operation_a, None, None),
            resolved(&specs[1], operation_b, None, None),
        ],
    );
    assert_eq!(report.disposition(), ProfilePreflightDisposition::Blocked);
    assert_eq!(report.path_conflicts().len(), 1);
    assert_eq!(
        report.path_conflicts()[0].reason,
        ProfilePathConflictReason::DifferentContent
    );
}

#[test]
fn preflight_enforces_exact_provider_version_archive_and_file_plan() {
    let specs = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let (_, target) = lock("target", &specs);
    let plan = ProfileSwitchPlan::build(None, None, &target).unwrap();
    let operation = &plan.operations()[0];
    let wrong = resolved(&specs[0], operation, Some(provider("101", "2")), Some(C));
    let report = plan.preflight(None, &[wrong]);
    assert_eq!(report.disposition(), ProfilePreflightDisposition::Blocked);
    assert!(report.errors().iter().any(|error| matches!(
        error,
        ProfilePreflightError::ExactIdentityMismatch { instance_id } if instance_id == "a"
    )));
}

#[test]
fn retry_operation_identity_is_stable_and_arbitrary_binding_is_rejected() {
    let specs = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let (_, target) = lock("target", &specs);
    let first = ProfileSwitchPlan::build(None, None, &target).unwrap();
    let second = ProfileSwitchPlan::build(None, None, &target).unwrap();
    assert_eq!(
        first.operations()[0].operation_id(),
        second.operations()[0].operation_id()
    );
    assert_eq!(
        first.operations()[0].idempotency_key(),
        second.operations()[0].idempotency_key()
    );

    let correct = resolved(&specs[0], &first.operations()[0], None, None);
    assert_eq!(
        first.preflight(None, &[correct]).disposition(),
        ProfilePreflightDisposition::Ready
    );

    let arbitrary_operation_id = resolved_with_binding(
        &specs[0],
        LifecycleOperationKind::Install,
        "arbitrary-operation",
        first.operations()[0].idempotency_key(),
        None,
        None,
    );
    let rejected_id = first.preflight(None, &[arbitrary_operation_id]);
    assert_eq!(
        rejected_id.disposition(),
        ProfilePreflightDisposition::Blocked
    );
    assert!(rejected_id.errors().iter().any(|error| matches!(
        error,
        ProfilePreflightError::OperationBindingMismatch { instance_id }
            if instance_id == "a"
    )));

    let arbitrary_idempotency_key = resolved_with_binding(
        &specs[0],
        LifecycleOperationKind::Install,
        first.operations()[0].operation_id(),
        "arbitrary-key",
        None,
        None,
    );
    let rejected_key = first.preflight(None, &[arbitrary_idempotency_key]);
    assert_eq!(
        rejected_key.disposition(),
        ProfilePreflightDisposition::Blocked
    );
    assert!(rejected_key.errors().iter().any(|error| matches!(
        error,
        ProfilePreflightError::OperationBindingMismatch { instance_id }
            if instance_id == "a"
    )));
}

#[test]
fn duplicate_resolved_plans_are_rejected_even_when_one_diverges() {
    let specs = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let (_, target) = lock("target", &specs);
    let plan = ProfileSwitchPlan::build(None, None, &target).unwrap();
    let operation = &plan.operations()[0];
    let exact = resolved(&specs[0], operation, None, None);
    let divergent = resolved(&specs[0], operation, Some(provider("101", "2")), Some(C));
    let report = plan.preflight(None, &[exact, divergent]);
    assert_eq!(report.disposition(), ProfilePreflightDisposition::Blocked);
    assert!(report.errors().iter().any(|error| matches!(
        error,
        ProfilePreflightError::DuplicateResolvedPlan { instance_id }
            if instance_id == "a"
    )));
}

#[test]
fn preflight_binds_exact_source_manifest_not_only_generation() {
    let specs = vec![spec(0, "a", "101", "1", A, "mods/a.dat", B)];
    let (_, active) = lock("target", &specs);
    let current = manifest(&specs);
    let plan = ProfileSwitchPlan::build(Some(&current), Some(&active), &active).unwrap();

    let wrong_installation = manifest_for_installation(&specs, "other-game");
    let wrong_installation_report = plan.preflight(Some(&wrong_installation), &[]);
    assert_eq!(
        wrong_installation_report.disposition(),
        ProfilePreflightDisposition::Blocked
    );
    assert!(wrong_installation_report
        .errors()
        .contains(&ProfilePreflightError::StaleManifest));

    let changed_specs = vec![spec(0, "a", "101", "2", C, "mods/a.dat", D)];
    let changed_content = manifest(&changed_specs);
    let changed_content_report = plan.preflight(Some(&changed_content), &[]);
    assert_eq!(
        changed_content_report.disposition(),
        ProfilePreflightDisposition::Blocked
    );
    assert!(changed_content_report
        .errors()
        .contains(&ProfilePreflightError::StaleManifest));
}

#[test]
fn ownership_release_precedes_dependent_claim_acquisition() {
    let current_specs = vec![spec(0, "release", "101", "1", A, "mods/shared.dat", A)];
    let target_specs = vec![
        spec(0, "acquire", "102", "1", B, "mods/shared.dat", B),
        spec(1, "release", "101", "2", C, "mods/release-new.dat", C),
    ];
    let (_, active) = lock("active", &current_specs);
    let (_, target) = lock("target", &target_specs);
    let current = manifest(&current_specs);
    let plan = ProfileSwitchPlan::build(Some(&current), Some(&active), &target).unwrap();
    let release_operation = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == "release")
        .unwrap();
    let acquire_operation = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == "acquire")
        .unwrap();
    let report = plan.preflight(
        Some(&current),
        &[
            resolved(&target_specs[0], acquire_operation, None, None),
            resolved(&target_specs[1], release_operation, None, None),
        ],
    );
    assert_eq!(report.disposition(), ProfilePreflightDisposition::Ready);
    assert_eq!(
        report
            .operations()
            .iter()
            .map(|operation| (operation.kind(), operation.instance_id()))
            .collect::<Vec<_>>(),
        vec![
            (LifecycleOperationKind::Update, "release"),
            (LifecycleOperationKind::Install, "acquire"),
        ]
    );
}

#[test]
fn cyclic_co_owner_replacement_fails_closed() {
    let current_specs = vec![
        spec(0, "a", "101", "1", A, "mods/shared.dat", A),
        spec(1, "b", "102", "1", B, "mods/shared.dat", A),
    ];
    let target_specs = vec![
        spec(0, "a", "101", "2", C, "mods/shared.dat", B),
        spec(1, "b", "102", "2", D, "mods/shared.dat", B),
    ];
    let (_, active) = lock("active", &current_specs);
    let (_, target) = lock("target", &target_specs);
    let current = manifest(&current_specs);
    let plan = ProfileSwitchPlan::build(Some(&current), Some(&active), &target).unwrap();
    let operation_a = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == "a")
        .unwrap();
    let operation_b = plan
        .operations()
        .iter()
        .find(|operation| operation.instance_id() == "b")
        .unwrap();
    let report = plan.preflight(
        Some(&current),
        &[
            resolved(&target_specs[0], operation_a, None, None),
            resolved(&target_specs[1], operation_b, None, None),
        ],
    );
    assert_eq!(report.disposition(), ProfilePreflightDisposition::Blocked);
    assert!(report
        .errors()
        .contains(&ProfilePreflightError::UnsafeOwnershipTransition));
    assert!(report.operations().is_empty());
}

#[test]
fn diff_and_lifecycle_operation_order_are_deterministic() {
    let current_specs = vec![
        spec(0, "z-remove", "109", "1", A, "mods/z.dat", A),
        spec(1, "b-update", "102", "1", B, "mods/b.dat", B),
        spec(2, "a-keep", "101", "1", C, "mods/a.dat", C),
    ];
    let target_specs = vec![
        spec(0, "b-update", "102", "2", D, "mods/b.dat", D),
        spec(1, "c-install", "103", "1", A, "mods/c.dat", A),
        spec(2, "a-keep", "101", "1", C, "mods/a.dat", C),
    ];
    let (_, active) = lock("active", &current_specs);
    let (_, target) = lock("target", &target_specs);
    let current = manifest(&current_specs);
    let first = ProfileSwitchPlan::build(Some(&current), Some(&active), &target).unwrap();
    let second = ProfileSwitchPlan::build(Some(&current), Some(&active), &target).unwrap();
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(first.removals()[0].instance_id(), "z-remove");
    assert_eq!(first.updates()[0].target().instance_id, "b-update");
    assert_eq!(first.installs()[0].target().instance_id, "c-install");
    assert_eq!(first.retained()[0].instance_id, "a-keep");
    assert_eq!(
        first
            .operations()
            .iter()
            .map(|operation| (operation.kind(), operation.instance_id()))
            .collect::<Vec<_>>(),
        vec![
            (LifecycleOperationKind::Uninstall, "z-remove"),
            (LifecycleOperationKind::Update, "b-update"),
            (LifecycleOperationKind::Install, "c-install"),
        ]
    );
}

#[test]
fn exact_active_profile_is_a_no_op() {
    let specs = vec![
        spec(0, "a", "101", "1", A, "mods/a.dat", A),
        spec(1, "b", "102", "1", B, "mods/b.dat", B),
    ];
    let (_, target) = lock("target", &specs);
    let current = manifest(&specs);
    let plan = ProfileSwitchPlan::build(Some(&current), Some(&target), &target).unwrap();
    assert!(plan.is_noop());
    assert_eq!(
        plan.preflight(Some(&current), &[]).disposition(),
        ProfilePreflightDisposition::NoOp
    );
}

#[test]
fn commit_boundary_keeps_previous_profile_until_every_item_verifies() {
    let current_specs = vec![spec(0, "old", "101", "1", A, "mods/old.dat", A)];
    let target_specs = vec![spec(0, "new", "102", "1", B, "mods/new.dat", B)];
    let (_, active) = lock("active", &current_specs);
    let (_, target) = lock("target", &target_specs);
    let current = manifest(&current_specs);
    let plan = ProfileSwitchPlan::build(Some(&current), Some(&active), &target).unwrap();
    assert_eq!(
        plan.lease_contract(),
        ProfileLeaseContract::OneInstallationLease
    );
    assert_eq!(
        plan.operation_engine(),
        ProfileOperationEngine::ExistingLifecycleOperations
    );
    assert_eq!(
        plan.commit_boundary().condition(),
        ProfileCommitCondition::AllLifecycleOperationsVerified
    );
    assert_eq!(
        plan.commit_boundary().on_failure(),
        ProfileFailurePolicy::RollBackAndKeepPreviousActive
    );
    assert_eq!(plan.commit_boundary().previous_profile_id(), Some("active"));
}
