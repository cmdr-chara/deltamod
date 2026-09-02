use deltamod_lifecycle_runtime::*;
use deltamod_product_contracts::{
    AcquireOutcome, CompareAndSwapOutcome, FilesystemBoundaryError, LifecycleFilesystemBoundary,
    LifecycleMutationGuard, LifecycleOperationKind, LifecycleTransactionStore, MutationCheckpoint,
    MutationFenceError, ObservationSnapshot, ObservedFileState, OperationIntent, OperationPhase,
    OperationRecord, OperationRequest, OperationState, OperationStore, ProductErrorCode,
    PublicationReceipt, RootBoundObservation, RootIdentity, ValidatedMutationReconciliation,
    ValidatedMutationTransition, ValidatedRelativePath,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write as _,
    path::Path,
    sync::{Arc, Mutex},
};

const H1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const H3: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const ARCHIVE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn startup_recovery_scope_leaves_other_installations_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(1);
    let mut store =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap();
    for installation_id in ["game-a", "game-b"] {
        let operation_id = format!("startup-{installation_id}");
        let request = OperationRequest::new(
            &operation_id,
            format!("key-{installation_id}"),
            OperationIntent {
                installation_id: installation_id.into(),
                kind: LifecycleOperationKind::Install,
                mod_instance_id: Some("mod-a".into()),
                provider: None,
                archive_sha256: None,
                file_plan_fingerprint: None,
                profile_id: None,
            },
        )
        .unwrap();
        assert!(matches!(
            store
                .acquire_or_replay(
                    &request,
                    "old-process",
                    &format!("lease-{installation_id}"),
                    1,
                    10
                )
                .unwrap(),
            AcquireOutcome::Acquired { .. }
        ));
    }
    clock.set(100);
    let mut runtime = ReleaseARuntime::new(store);
    let mut workspace = ModelWorkspace::new();
    let recovered = runtime.recover_startup_installation(
        "new-process",
        "game-a",
        100,
        10,
        |_| "new-lease-a".into(),
        &mut workspace,
    );
    assert!(matches!(
        recovered.as_slice(),
        [StartupRecoveryOutcome::Recovered { .. }]
    ));
    assert_eq!(
        runtime
            .store()
            .operation_by_id("startup-game-a")
            .unwrap()
            .unwrap()
            .state,
        OperationState::Recovered
    );
    assert_eq!(
        runtime
            .store()
            .operation_by_id("startup-game-b")
            .unwrap()
            .unwrap()
            .state,
        OperationState::Running
    );
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelObservation(ObservationSnapshot);

impl RootBoundObservation for ModelObservation {
    fn snapshot(&self) -> &ObservationSnapshot {
        &self.0
    }
}

#[derive(Default)]
struct ModelWorkspace {
    transaction_root: Option<RootIdentity>,
    roots: BTreeMap<String, RootIdentity>,
    entries: BTreeMap<(String, String), ObservedFileState>,
    sources: BTreeMap<String, String>,
    generation_backups: BTreeMap<String, RootIdentity>,
    cleanup_calls: Vec<(String, bool)>,
    fault_external_state: Arc<Mutex<Option<(String, ObservedFileState)>>>,
}

impl ModelWorkspace {
    fn new() -> Self {
        let transaction = root("transaction");
        let mut workspace = Self {
            transaction_root: Some(transaction.clone()),
            ..Self::default()
        };
        workspace
            .roots
            .insert(transaction.canonical_path_sha256.clone(), transaction);
        workspace
    }

    fn add_source(&mut self, source_id: &str, sha256: &str) {
        self.sources.insert(source_id.to_owned(), sha256.to_owned());
    }

    fn transaction_state(&self, path: &str) -> ObservedFileState {
        let root = self.transaction_root.as_ref().expect("transaction root");
        if let Some((external_path, state)) = self.fault_external_state.lock().unwrap().as_ref() {
            if external_path == path {
                return state.clone();
            }
        }
        self.entries
            .get(&(root.canonical_path_sha256.clone(), path.to_owned()))
            .cloned()
            .unwrap_or(ObservedFileState::Missing)
    }

    fn externally_replace(&mut self, path: &str, sha256: &str) {
        *self.fault_external_state.lock().unwrap() = None;
        let root = self.transaction_root.as_ref().expect("transaction root");
        self.entries.insert(
            (root.canonical_path_sha256.clone(), path.to_owned()),
            regular(sha256, "external"),
        );
    }

    fn fault_external_state_handle(&self) -> Arc<Mutex<Option<(String, ObservedFileState)>>> {
        Arc::clone(&self.fault_external_state)
    }

    fn clear_fault_external_state(&self, path: &ValidatedRelativePath) {
        let mut external = self.fault_external_state.lock().unwrap();
        if external
            .as_ref()
            .is_some_and(|(external_path, _)| external_path == path.as_str())
        {
            *external = None;
        }
    }

    fn key(root: &RootIdentity, path: &ValidatedRelativePath) -> (String, String) {
        (root.canonical_path_sha256.clone(), path.as_str().to_owned())
    }

    fn observe(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<ModelObservation, FilesystemBoundaryError> {
        if self
            .roots
            .get(&expected_root.canonical_path_sha256)
            .is_none_or(|actual| actual != expected_root)
        {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        Ok(ModelObservation(ObservationSnapshot {
            root_identity: expected_root.clone(),
            path: path.clone(),
            path_identity_key: path.as_str().to_owned(),
            state: self
                .fault_external_state
                .lock()
                .unwrap()
                .as_ref()
                .filter(|(external_path, _)| external_path == path.as_str())
                .map(|(_, state)| state.clone())
                .or_else(|| self.entries.get(&Self::key(expected_root, path)).cloned())
                .unwrap_or(ObservedFileState::Missing),
            observation_sequence: 1,
        }))
    }

    fn ensure_current(
        &self,
        observation: &ModelObservation,
    ) -> Result<(), FilesystemBoundaryError> {
        if self
            .observe(&observation.0.root_identity, &observation.0.path)?
            .0
            .state
            != observation.0.state
        {
            Err(FilesystemBoundaryError::ObservationChanged)
        } else {
            Ok(())
        }
    }

    fn receipt(
        guard: &impl LifecycleMutationGuard,
        destination: &ModelObservation,
        sha256: Option<String>,
    ) -> PublicationReceipt {
        PublicationReceipt {
            root_identity: destination.0.root_identity.clone(),
            path: destination.0.path.clone(),
            path_identity_key: destination.0.path_identity_key.clone(),
            sha256,
            operation_id: guard.operation_id().to_owned(),
            lease_id: guard.lease_id().to_owned(),
            fencing_token: guard.fencing_token(),
            operation_revision: guard.operation_revision(),
            journal_sequence: guard.journal_sequence(),
        }
    }
}

impl LifecycleWorkspace for ModelWorkspace {
    fn transaction_root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError> {
        self.transaction_root
            .clone()
            .ok_or(FilesystemBoundaryError::RootIdentityChanged)
    }

    fn observe_preflight_no_follow(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError> {
        Ok(self.observe(expected_root, path)?.0)
    }

    fn prepare_workspace(
        &mut self,
        operation_id: &str,
        recovery_generation_id: &str,
        expected_transaction_root: &RootIdentity,
    ) -> Result<WorkspaceRoots, FilesystemBoundaryError> {
        if self.transaction_root.as_ref() != Some(expected_transaction_root) {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        let staging = root(&format!("stage-{operation_id}"));
        let backup = root(&format!("backup-{recovery_generation_id}"));
        self.roots
            .insert(staging.canonical_path_sha256.clone(), staging.clone());
        self.roots
            .insert(backup.canonical_path_sha256.clone(), backup.clone());
        self.generation_backups
            .insert(recovery_generation_id.to_owned(), backup.clone());
        Ok(WorkspaceRoots {
            transaction: expected_transaction_root.clone(),
            staging,
            backup,
        })
    }

    fn stage_file(
        &mut self,
        staging_root: &RootIdentity,
        staging_path: &ValidatedRelativePath,
        source: &StagingSource,
        expected_sha256: &str,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError> {
        let actual = match source {
            StagingSource::Artifact { source_id }
            | StagingSource::ArtifactTree { source_id, .. } => self.sources.get(source_id).cloned(),
            StagingSource::RecoveryBackup {
                generation_id,
                backup_path,
            } => self
                .generation_backups
                .get(generation_id)
                .and_then(|root| self.entries.get(&Self::key(root, backup_path)))
                .and_then(state_hash)
                .map(str::to_owned),
        }
        .ok_or(FilesystemBoundaryError::VerificationFailed)?;
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        self.entries.insert(
            Self::key(staging_root, staging_path),
            regular(&actual, "staged"),
        );
        Ok(self.observe(staging_root, staging_path)?.0)
    }

    fn backup_file(
        &mut self,
        transaction_root: &RootIdentity,
        destination_path: &ValidatedRelativePath,
        backup_root: &RootIdentity,
        backup_path: &ValidatedRelativePath,
        expected_sha256: &str,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError> {
        let current = self.observe(transaction_root, destination_path)?;
        if state_hash(&current.0.state)
            .is_none_or(|hash| !hash.eq_ignore_ascii_case(expected_sha256))
        {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        self.entries.insert(
            Self::key(backup_root, backup_path),
            regular(expected_sha256, "backup"),
        );
        Ok(self.observe(backup_root, backup_path)?.0)
    }

    fn cleanup_workspace(
        &mut self,
        roots: &WorkspaceRoots,
        _operation_id: &str,
        recovery_generation_id: &str,
        retain_backup: bool,
    ) -> Result<(), FilesystemBoundaryError> {
        self.entries
            .retain(|(root_hash, _), _| root_hash != &roots.staging.canonical_path_sha256);
        if !retain_backup {
            self.entries
                .retain(|(root_hash, _), _| root_hash != &roots.backup.canonical_path_sha256);
            self.generation_backups.remove(recovery_generation_id);
        }
        self.cleanup_calls
            .push((recovery_generation_id.to_owned(), retain_backup));
        Ok(())
    }
}

impl<'guard> LifecycleFilesystemBoundary<DurableMutationGuard<'guard>> for ModelWorkspace {
    type Observation = ModelObservation;

    fn root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError> {
        self.transaction_root_identity()
    }

    fn observe_no_follow(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<Self::Observation, FilesystemBoundaryError> {
        self.observe(expected_root, path)
    }

    fn publish_verified(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        staged: &Self::Observation,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(fence_error)?;
        self.ensure_current(staged)?;
        self.ensure_current(destination)?;
        let sha256 = transition.validate_publication(&staged.0, &destination.0)?;
        self.clear_fault_external_state(&destination.0.path);
        self.entries.insert(
            Self::key(&destination.0.root_identity, &destination.0.path),
            staged.0.state.clone(),
        );
        guard
            .checkpoint_after_side_effect(now_ms, transition)
            .map_err(fence_error)?;
        Ok(Self::receipt(guard, destination, Some(sha256)))
    }

    fn remove_verified(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(fence_error)?;
        self.ensure_current(destination)?;
        transition.validate_removal(&destination.0)?;
        self.clear_fault_external_state(&destination.0.path);
        self.entries.remove(&Self::key(
            &destination.0.root_identity,
            &destination.0.path,
        ));
        guard
            .checkpoint_after_side_effect(now_ms, transition)
            .map_err(fence_error)?;
        Ok(Self::receipt(guard, destination, None))
    }

    fn restore_verified_backup(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        backup: &Self::Observation,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(fence_error)?;
        self.ensure_current(backup)?;
        self.ensure_current(destination)?;
        let sha256 = transition.validate_backup_restoration(&backup.0, &destination.0)?;
        self.clear_fault_external_state(&destination.0.path);
        self.entries.insert(
            Self::key(&destination.0.root_identity, &destination.0.path),
            backup.0.state.clone(),
        );
        guard
            .checkpoint_after_side_effect(now_ms, transition)
            .map_err(fence_error)?;
        Ok(Self::receipt(guard, destination, Some(sha256)))
    }

    fn reconcile_uncertain_effect(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        destination: &Self::Observation,
        reconciliation: &ValidatedMutationReconciliation,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(fence_error)?;
        self.ensure_current(destination)?;
        reconciliation.validate_destination_observation(&destination.0)?;
        let sha256 = state_hash(&destination.0.state).map(str::to_owned);
        guard
            .checkpoint_after_reconciliation(now_ms, reconciliation)
            .map_err(fence_error)?;
        Ok(Self::receipt(guard, destination, sha256))
    }
}

fn root(marker: &str) -> RootIdentity {
    let digest = Sha256::digest(marker.as_bytes());
    RootIdentity {
        canonical_path_sha256: digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        volume_id: format!("volume-{}", safe_marker(marker)),
        file_id: format!("file-{}", safe_marker(marker)),
    }
}

fn safe_marker(marker: &str) -> String {
    marker
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn regular(sha256: &str, file_id: &str) -> ObservedFileState {
    ObservedFileState::Regular {
        sha256: sha256.to_owned(),
        file_id: file_id.to_owned(),
        link_count: 1,
    }
}

fn state_hash(state: &ObservedFileState) -> Option<&str> {
    match state {
        ObservedFileState::Regular { sha256, .. } => Some(sha256),
        _ => None,
    }
}

fn fence_error(error: MutationFenceError) -> FilesystemBoundaryError {
    match error {
        MutationFenceError::StaleJournalSequence | MutationFenceError::InvalidJournal => {
            FilesystemBoundaryError::StaleJournal
        }
        MutationFenceError::LostLease
        | MutationFenceError::Expired
        | MutationFenceError::StaleOperationRevision
        | MutationFenceError::Store => FilesystemBoundaryError::LostLease,
    }
}

#[allow(clippy::too_many_arguments)] // Keeps call sites explicit in the fault-matrix fixtures.
fn install_plan(
    operation_id: &str,
    key: &str,
    installation: &str,
    kind: LifecycleOperationKind,
    instance: &str,
    version: &str,
    path: &str,
    source_id: &str,
    sha256: &str,
) -> ValidatedInstallPlan {
    let files = vec![InstallFilePlan {
        path: ValidatedRelativePath::parse(path).unwrap(),
        path_identity_key: path.to_owned(),
        sha256: sha256.to_owned(),
        size_bytes: 4,
        expected_previous_sha256: None,
        source: StagingSource::Artifact {
            source_id: source_id.to_owned(),
        },
    }];
    let intent = OperationIntent {
        installation_id: installation.to_owned(),
        kind,
        mod_instance_id: Some(instance.to_owned()),
        provider: Some(deltamod_product_contracts::fixtures::provider_ref()),
        archive_sha256: Some(ARCHIVE.to_owned()),
        file_plan_fingerprint: Some(file_plan_fingerprint(&files)),
        profile_id: None,
    };
    let request = OperationRequest::new(operation_id, key, intent).unwrap();
    ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: instance.to_owned(),
            mod_id: format!("mod-{instance}"),
            display_name: format!("Mod {instance}"),
            version: Some(version.to_owned()),
            provider: deltamod_product_contracts::fixtures::provider_ref(),
            archive_sha256: Some(ARCHIVE.to_owned()),
        },
        files,
    )
    .unwrap()
}

fn uninstall_plan(
    operation_id: &str,
    key: &str,
    installation: &str,
    instance: &str,
) -> ValidatedUninstallPlan {
    ValidatedUninstallPlan::new(
        OperationRequest::new(
            operation_id,
            key,
            OperationIntent {
                installation_id: installation.to_owned(),
                kind: LifecycleOperationKind::Uninstall,
                mod_instance_id: Some(instance.to_owned()),
                provider: None,
                archive_sha256: None,
                file_plan_fingerprint: None,
                profile_id: None,
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn restore_request(operation_id: &str, key: &str, installation: &str) -> OperationRequest {
    OperationRequest::new(
        operation_id,
        key,
        OperationIntent {
            installation_id: installation.to_owned(),
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

fn identity(lease: &str, generation: &str, now_ms: u64) -> ExecutionIdentity {
    ExecutionIdentity {
        owner_instance_id: "runtime-test".into(),
        lease_id: lease.into(),
        recovery_generation_id: generation.into(),
        now_ms,
        lease_ttl_ms: 60_000,
    }
}

fn identity_with_ttl(lease: &str, generation: &str, lease_ttl_ms: u64) -> ExecutionIdentity {
    let mut identity = identity(lease, generation, 0);
    identity.lease_ttl_ms = lease_ttl_ms;
    identity
}

fn assert_success(outcome: LifecycleOutcome) -> OperationRecord {
    match outcome {
        LifecycleOutcome::Succeeded { operation, .. } => *operation,
        other => panic!("expected success, got {other:?}"),
    }
}

#[derive(Clone, Copy, Debug)]
enum MutationShape {
    Create,
    Replace,
    Delete,
}

impl MutationShape {
    const fn forward_effect(self) -> deltamod_product_contracts::MutationSideEffect {
        match self {
            Self::Create | Self::Replace => deltamod_product_contracts::MutationSideEffect::Publish,
            Self::Delete => deltamod_product_contracts::MutationSideEffect::Remove,
        }
    }

    const fn rollback_effect(self) -> deltamod_product_contracts::MutationSideEffect {
        match self {
            Self::Create => deltamod_product_contracts::MutationSideEffect::Remove,
            Self::Replace | Self::Delete => {
                deltamod_product_contracts::MutationSideEffect::RestoreBackup
            }
        }
    }
}

fn run_faulted_mutation(
    shape: MutationShape,
    fault: FaultPoint,
) -> (
    tempfile::TempDir,
    ReleaseARuntime,
    ManualClock,
    ModelWorkspace,
    String,
) {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(1);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap(),
    );
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    workspace.add_source("source-v2", H2);
    if matches!(shape, MutationShape::Replace | MutationShape::Delete) {
        assert_success(runtime.install(
            install_plan(
                "op-base",
                "key-base",
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity("lease-base", "generation-base", 1),
            &mut workspace,
        ));
    }
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(fault.clone()));
    let operation_id = format!("op-fault-{shape:?}").to_ascii_lowercase();
    let outcome = match shape {
        MutationShape::Create => runtime.install(
            install_plan(
                &operation_id,
                "key-fault",
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity("lease-fault", "generation-fault", 20),
            &mut workspace,
        ),
        MutationShape::Replace => runtime.install(
            install_plan(
                &operation_id,
                "key-fault",
                "game",
                LifecycleOperationKind::Update,
                "a",
                "2",
                "mods/a.dat",
                "source-v2",
                H2,
            ),
            identity("lease-fault", "generation-fault", 20),
            &mut workspace,
        ),
        MutationShape::Delete => runtime.uninstall(
            uninstall_plan(&operation_id, "key-fault", "game", "a"),
            identity("lease-fault", "generation-fault", 20),
            &mut workspace,
        ),
    };
    assert!(
        matches!(
            (&fault, &outcome),
            (
                FaultPoint::AfterJournalCas(JournalCheckpointKind::Terminal(_)),
                LifecycleOutcome::Existing { .. }
            ) | (_, LifecycleOutcome::RecoveryRequired { .. })
        ),
        "{shape:?} at {fault:?}: {outcome:?}"
    );
    (directory, runtime, clock, workspace, operation_id)
}

fn assert_rolled_back(shape: MutationShape, runtime: &ReleaseARuntime, workspace: &ModelWorkspace) {
    match shape {
        MutationShape::Create => {
            assert_eq!(
                workspace.transaction_state("mods/a.dat"),
                ObservedFileState::Missing
            );
            assert!(runtime.store().manifest("game").unwrap().is_none());
        }
        MutationShape::Replace | MutationShape::Delete => {
            assert_eq!(
                state_hash(&workspace.transaction_state("mods/a.dat")),
                Some(H1)
            );
            let manifest = runtime.store().manifest("game").unwrap().unwrap();
            assert_eq!(manifest.records.len(), 1);
            assert_eq!(manifest.records[0].files[0].expected_sha256, H1);
        }
    }
    assert!(!workspace
        .generation_backups
        .contains_key("generation-fault"));
}

fn assert_manifest_matches_workspace(runtime: &ReleaseARuntime, workspace: &ModelWorkspace) {
    let manifest = runtime.store().manifest("game").unwrap();
    let claim = manifest
        .as_ref()
        .and_then(|manifest| manifest.ledger.claims.first());
    match claim {
        Some(claim) => assert_eq!(
            state_hash(&workspace.transaction_state(claim.path.as_str())),
            Some(claim.sha256.as_str())
        ),
        None => assert_eq!(
            workspace.transaction_state("mods/a.dat"),
            ObservedFileState::Missing
        ),
    }
}

fn initialize_bound_store_pair(
    root: &Path,
) -> (DurableLifecycleStore, DurableLifecycleStore, Vec<u8>) {
    let clock = ManualClock::new(1);
    let mut first = DurableLifecycleStore::open_with_clock(root, Arc::new(clock.clone())).unwrap();
    let request = install_plan(
        "op-store-identity",
        "key-store-identity",
        "identity-game",
        LifecycleOperationKind::Install,
        "a",
        "1",
        "mods/a.dat",
        "source-v1",
        H1,
    )
    .request()
    .clone();
    assert!(matches!(
        first
            .acquire_or_replay(
                &request,
                "store-identity-owner",
                "store-identity-lease",
                0,
                10_000,
            )
            .unwrap(),
        AcquireOutcome::Acquired { .. }
    ));
    let second = DurableLifecycleStore::open_with_clock(root, Arc::new(clock)).unwrap();
    assert!(second
        .operation_by_id("op-store-identity")
        .unwrap()
        .is_some());
    let log_bytes = std::fs::read(root.join("lifecycle-state.log")).unwrap();
    (first, second, log_bytes)
}

fn assert_store_identity_changed<T>(result: Result<T, StoreError>) {
    assert!(matches!(result, Err(StoreError::StoreIdentityChanged(_))));
}

#[cfg(windows)]
fn try_symlink_file(original: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_file(original, link) {
        Ok(()) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            false
        }
        Err(error) => panic!("failed to create file symlink: {error}"),
    }
}

#[cfg(unix)]
fn try_symlink_file(original: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(original, link).unwrap();
    true
}

#[cfg(windows)]
fn try_symlink_dir(original: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_dir(original, link) {
        Ok(()) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            false
        }
        Err(error) => panic!("failed to create directory symlink: {error}"),
    }
}

#[cfg(unix)]
fn try_symlink_dir(original: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(original, link).unwrap();
    true
}

#[test]
fn create_replace_coownership_uninstall_and_restore_are_transactional() {
    let directory = tempfile::tempdir().unwrap();
    let store = DurableLifecycleStore::open(directory.path()).unwrap();
    let mut runtime = ReleaseARuntime::new(store);
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    workspace.add_source("source-v2", H2);

    assert_success(runtime.install(
        install_plan(
            "op-create",
            "key-create",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/shared.dat",
            "source-v1",
            H1,
        ),
        identity("lease-create", "generation-create", 1),
        &mut workspace,
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/shared.dat")),
        Some(H1)
    );

    assert_success(runtime.install(
        install_plan(
            "op-replace",
            "key-replace",
            "game",
            LifecycleOperationKind::Update,
            "a",
            "2",
            "mods/shared.dat",
            "source-v2",
            H2,
        ),
        identity("lease-replace", "generation-replace", 20),
        &mut workspace,
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/shared.dat")),
        Some(H2)
    );

    let before_coownership_cleanup = workspace.cleanup_calls.len();
    assert_success(runtime.install(
        install_plan(
            "op-coown",
            "key-coown",
            "game",
            LifecycleOperationKind::Install,
            "b",
            "1",
            "mods/shared.dat",
            "source-v2",
            H2,
        ),
        identity("lease-coown", "generation-coown", 40),
        &mut workspace,
    ));
    assert_eq!(workspace.cleanup_calls.len(), before_coownership_cleanup);
    let manifest = runtime.store().manifest("game").unwrap().unwrap();
    assert_eq!(manifest.ledger.claims[0].owners.len(), 2);

    assert_success(runtime.uninstall(
        uninstall_plan("op-uninstall-a", "key-uninstall-a", "game", "a"),
        identity("lease-uninstall-a", "generation-uninstall-a", 60),
        &mut workspace,
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/shared.dat")),
        Some(H2)
    );
    assert_eq!(
        runtime
            .store()
            .manifest("game")
            .unwrap()
            .unwrap()
            .ledger
            .claims[0]
            .owners,
        ["b".to_owned()].into_iter().collect()
    );

    assert_success(runtime.uninstall(
        uninstall_plan("op-uninstall-b", "key-uninstall-b", "game", "b"),
        identity("lease-uninstall-b", "generation-uninstall-b", 80),
        &mut workspace,
    ));
    assert_eq!(
        workspace.transaction_state("mods/shared.dat"),
        ObservedFileState::Missing
    );

    assert_success(runtime.restore_last_working_state(
        restore_request("op-restore", "key-restore", "game"),
        identity("lease-restore", "generation-restore", 100),
        &mut workspace,
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/shared.dat")),
        Some(H2)
    );
    let restored = runtime.store().manifest("game").unwrap().unwrap();
    assert_eq!(restored.records.len(), 1);
    assert_eq!(restored.records[0].instance_id, "b");
}

#[test]
fn recovery_generation_selection_uses_durable_completion_order_not_time_or_id() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(50);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock)).unwrap(),
    );
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);

    assert_success(runtime.install(
        install_plan(
            "op-generation-one",
            "key-generation-one",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-generation-one", "z-generation-one", 50),
        &mut workspace,
    ));
    let first = runtime
        .store()
        .latest_recovery_generation("game")
        .unwrap()
        .unwrap();
    assert_eq!(first.generation_id, "z-generation-one");

    assert_success(runtime.install(
        install_plan(
            "op-generation-two",
            "key-generation-two",
            "game",
            LifecycleOperationKind::Install,
            "b",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-generation-two", "a-generation-two", 50),
        &mut workspace,
    ));
    let second = runtime
        .store()
        .latest_recovery_generation("game")
        .unwrap()
        .unwrap();
    assert_eq!(second.completed_at_ms, first.completed_at_ms);
    assert_eq!(second.generation_id, "a-generation-two");
    assert!(second.completion_sequence > first.completion_sequence);
    assert_eq!(
        second
            .previous_manifest
            .as_ref()
            .map(InstallationManifest::generation),
        Some(1)
    );
    assert_eq!(second.target_manifest.generation(), 2);

    assert_success(runtime.restore_last_working_state(
        restore_request("op-generation-restore", "key-generation-restore", "game"),
        identity("lease-generation-restore", "generation-restore", 50),
        &mut workspace,
    ));
    let restored = runtime.store().manifest("game").unwrap().unwrap();
    assert_eq!(restored.generation(), 3);
    assert_eq!(restored.records.len(), 1);
    assert_eq!(restored.records[0].instance_id, "a");
}

#[test]
fn double_delivery_is_replayed_and_changed_semantics_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let store = DurableLifecycleStore::open(directory.path()).unwrap();
    let mut runtime = ReleaseARuntime::new(store);
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    workspace.add_source("source-v2", H2);
    let first = install_plan(
        "op-first",
        "delivery-key",
        "game",
        LifecycleOperationKind::Install,
        "a",
        "1",
        "mods/a.dat",
        "source-v1",
        H1,
    );
    assert_success(runtime.install(
        first,
        identity("lease-first", "generation-first", 1),
        &mut workspace,
    ));
    let replay = runtime.install(
        install_plan(
            "op-redelivery",
            "delivery-key",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-redelivery", "generation-redelivery", 20),
        &mut workspace,
    );
    let LifecycleOutcome::Existing { operation } = replay else {
        panic!("expected structured replay: {replay:?}");
    };
    assert_eq!(operation.request.operation_id(), "op-first");

    let conflict = runtime.install(
        install_plan(
            "op-conflict",
            "delivery-key",
            "game",
            LifecycleOperationKind::Install,
            "other",
            "2",
            "mods/other.dat",
            "source-v2",
            H2,
        ),
        identity("lease-conflict", "generation-conflict", 30),
        &mut workspace,
    );
    let LifecycleOutcome::Rejected { error, .. } = conflict else {
        panic!("expected structured conflict: {conflict:?}");
    };
    assert_eq!(error.code, ProductErrorCode::IdempotencyConflict);
}

#[test]
fn external_change_blocks_uninstall_without_overwrite() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    assert_success(runtime.install(
        install_plan(
            "op-install",
            "key-install",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-install", "generation-install", 1),
        &mut workspace,
    ));
    workspace.externally_replace("mods/a.dat", H2);
    let outcome = runtime.uninstall(
        uninstall_plan("op-uninstall", "key-uninstall", "game", "a"),
        identity("lease-uninstall", "generation-uninstall", 20),
        &mut workspace,
    );
    let LifecycleOutcome::Rejected {
        error, operation, ..
    } = outcome
    else {
        panic!("expected external-change rejection: {outcome:?}");
    };
    assert_eq!(error.code, ProductErrorCode::ExternalModification);
    assert!(operation.is_some_and(
        |operation| operation.state == deltamod_product_contracts::OperationState::Failed
    ));
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/a.dat")),
        Some(H2)
    );
}

#[test]
fn file_plan_validation_rejects_duplicates_mismatches_and_traversal() {
    assert!(ValidatedRelativePath::parse("../escape.dat").is_err());
    let duplicate_files = vec![
        InstallFilePlan {
            path: ValidatedRelativePath::parse("mods/a.dat").unwrap(),
            path_identity_key: "mods/a.dat".into(),
            sha256: H1.into(),
            size_bytes: 1,
            expected_previous_sha256: None,
            source: StagingSource::Artifact {
                source_id: "source-a".into(),
            },
        },
        InstallFilePlan {
            path: ValidatedRelativePath::parse("mods/alias.dat").unwrap(),
            path_identity_key: "mods/a.dat".into(),
            sha256: H2.into(),
            size_bytes: 1,
            expected_previous_sha256: None,
            source: StagingSource::Artifact {
                source_id: "source-b".into(),
            },
        },
    ];
    let duplicate_request = OperationRequest::new(
        "op-duplicate-plan",
        "key-duplicate-plan",
        OperationIntent {
            installation_id: "game".into(),
            kind: LifecycleOperationKind::Install,
            mod_instance_id: Some("a".into()),
            provider: Some(deltamod_product_contracts::fixtures::provider_ref()),
            archive_sha256: Some(ARCHIVE.into()),
            file_plan_fingerprint: Some(file_plan_fingerprint(&duplicate_files)),
            profile_id: None,
        },
    )
    .unwrap();
    let metadata = InstallMetadata {
        instance_id: "a".into(),
        mod_id: "mod-a".into(),
        display_name: "Mod A".into(),
        version: Some("1".into()),
        provider: deltamod_product_contracts::fixtures::provider_ref(),
        archive_sha256: Some(ARCHIVE.into()),
    };
    assert_eq!(
        ValidatedInstallPlan::new(duplicate_request, metadata.clone(), duplicate_files),
        Err(PlanError::DuplicateFile)
    );

    let files = vec![InstallFilePlan {
        path: ValidatedRelativePath::parse("mods/a.dat").unwrap(),
        path_identity_key: "mods/a.dat".into(),
        sha256: H1.into(),
        size_bytes: 1,
        expected_previous_sha256: None,
        source: StagingSource::Artifact {
            source_id: "source-a".into(),
        },
    }];
    let mismatch_request = OperationRequest::new(
        "op-mismatch-plan",
        "key-mismatch-plan",
        OperationIntent {
            installation_id: "game".into(),
            kind: LifecycleOperationKind::Install,
            mod_instance_id: Some("a".into()),
            provider: Some(deltamod_product_contracts::fixtures::provider_ref()),
            archive_sha256: Some(ARCHIVE.into()),
            file_plan_fingerprint: Some(H2.into()),
            profile_id: None,
        },
    )
    .unwrap();
    assert_eq!(
        ValidatedInstallPlan::new(mismatch_request, metadata, files),
        Err(PlanError::FingerprintMismatch)
    );
}

#[test]
fn durable_lookup_survives_reopen_and_stale_lease_or_fingerprint_is_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(FaultPoint::AfterJournalCas(
            JournalCheckpointKind::JournalCreated,
        )));
    let outcome = runtime.install(
        install_plan(
            "op-crash",
            "key-crash",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-crash", "generation-crash", 1),
        &mut workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::RecoveryRequired { .. }));
    drop(runtime);

    let mut store = DurableLifecycleStore::open(directory.path()).unwrap();
    assert_eq!(
        store
            .operation_by_idempotency_key("key-crash")
            .unwrap()
            .unwrap()
            .request
            .operation_id(),
        "op-crash"
    );
    let interrupted = store.interrupted_operations().unwrap().pop().unwrap();
    let journal = interrupted.journal.clone().unwrap();
    let mut stale_lease = interrupted.lease.clone();
    stale_lease.lease_id = "different-lease".into();
    assert!(matches!(
        store.lock_mutation(&stale_lease, &interrupted.record, &journal, 2),
        Err(StoreError::LostLease)
    ));
    let mut stale_journal = journal.clone();
    stale_journal
        .try_update(|payload| payload.updated_at_ms += 1)
        .unwrap();
    assert!(matches!(
        store.lock_mutation(&interrupted.lease, &interrupted.record, &stale_journal, 2),
        Err(StoreError::StaleJournal)
    ));
}

#[test]
fn trusted_clock_fences_before_effect_and_at_the_post_effect_journal_cas() {
    let cases = [
        (
            FaultPoint::BeforeFilesystemEffect {
                index: 0,
                effect: deltamod_product_contracts::MutationSideEffect::Publish,
            },
            false,
        ),
        (
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            true,
        ),
    ];
    for (case, (advance_at, effect_landed)) in cases.into_iter().enumerate() {
        let directory = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(1);
        let mut runtime = ReleaseARuntime::new(
            DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone()))
                .unwrap(),
        );
        let mut workspace = ModelWorkspace::new();
        workspace.add_source("source-v1", H1);
        runtime
            .store_mut()
            .set_fault_injector(AdvanceClockAt::new(advance_at, clock.clone(), 11));
        let operation_id = format!("op-expiry-effect-{case}");

        let outcome = runtime.install(
            install_plan(
                &operation_id,
                &format!("key-expiry-effect-{case}"),
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity_with_ttl(
                &format!("lease-expiry-effect-{case}"),
                &format!("generation-expiry-effect-{case}"),
                10,
            ),
            &mut workspace,
        );

        assert!(matches!(outcome, LifecycleOutcome::RecoveryRequired { .. }));
        assert!(runtime.store().manifest("game").unwrap().is_none());
        assert_eq!(
            state_hash(&workspace.transaction_state("mods/a.dat")).is_some(),
            effect_landed
        );
        assert_eq!(
            runtime
                .store()
                .journal_by_operation(&operation_id)
                .unwrap()
                .unwrap()
                .mutations[0]
                .checkpoint,
            MutationCheckpoint::Staged
        );

        runtime.store_mut().clear_fault_injector();
        let recovered = runtime.recover_startup(
            "recovery-runtime",
            u64::MAX,
            100,
            |_| format!("recovery-expiry-effect-{case}"),
            &mut workspace,
        );
        assert!(matches!(
            recovered.as_slice(),
            [StartupRecoveryOutcome::Recovered { .. }]
        ));
        assert_eq!(
            workspace.transaction_state("mods/a.dat"),
            ObservedFileState::Missing
        );
    }
}

#[test]
fn trusted_clock_fences_manifest_cleanup_terminal_and_public_operation_cas() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(1);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap(),
    );
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    runtime.store_mut().set_fault_injector(AdvanceClockAt::new(
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestPublished),
        clock.clone(),
        11,
    ));
    let outcome = runtime.install(
        install_plan(
            "op-expiry-manifest",
            "key-expiry-manifest",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity_with_ttl("lease-expiry-manifest", "generation-expiry-manifest", 10),
        &mut workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::RecoveryRequired { .. }));
    assert!(runtime.store().manifest("game").unwrap().is_none());
    assert_eq!(
        runtime
            .store()
            .journal_by_operation("op-expiry-manifest")
            .unwrap()
            .unwrap()
            .manifest_commit_state,
        deltamod_product_contracts::ManifestCommitState::TemporaryWritten
    );
    runtime.store_mut().clear_fault_injector();
    let recovered = runtime.recover_startup(
        "recovery-runtime",
        0,
        100,
        |_| "recovery-expiry-manifest".into(),
        &mut workspace,
    );
    assert!(matches!(
        recovered.as_slice(),
        [StartupRecoveryOutcome::Recovered { .. }]
    ));
    assert!(runtime.store().manifest("game").unwrap().is_some());

    let cleanup_directory = tempfile::tempdir().unwrap();
    let cleanup_clock = ManualClock::new(1);
    let mut cleanup_runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(
            cleanup_directory.path(),
            Arc::new(cleanup_clock.clone()),
        )
        .unwrap(),
    );
    let mut cleanup_workspace = ModelWorkspace::new();
    cleanup_workspace.add_source("source-v1", H1);
    cleanup_runtime
        .store_mut()
        .set_fault_injector(AdvanceClockAt::new(
            FaultPoint::BeforeCleanup,
            cleanup_clock,
            11,
        ));
    assert!(matches!(
        cleanup_runtime.install(
            install_plan(
                "op-expiry-cleanup",
                "key-expiry-cleanup",
                "cleanup-game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity_with_ttl("lease-expiry-cleanup", "generation-expiry-cleanup", 10),
            &mut cleanup_workspace,
        ),
        LifecycleOutcome::RecoveryRequired { .. }
    ));
    assert!(cleanup_workspace.cleanup_calls.is_empty());

    let terminal_directory = tempfile::tempdir().unwrap();
    let terminal_clock = ManualClock::new(1);
    let mut terminal_runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(
            terminal_directory.path(),
            Arc::new(terminal_clock.clone()),
        )
        .unwrap(),
    );
    let mut terminal_workspace = ModelWorkspace::new();
    terminal_workspace.add_source("source-v1", H1);
    terminal_runtime
        .store_mut()
        .set_fault_injector(AdvanceClockAt::new(
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Terminal(
                OperationState::Succeeded,
            )),
            terminal_clock,
            11,
        ));
    assert!(matches!(
        terminal_runtime.install(
            install_plan(
                "op-expiry-terminal",
                "key-expiry-terminal",
                "terminal-game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity_with_ttl("lease-expiry-terminal", "generation-expiry-terminal", 10),
            &mut terminal_workspace,
        ),
        LifecycleOutcome::RecoveryRequired { .. }
    ));
    assert!(!terminal_runtime
        .store()
        .operation_by_id("op-expiry-terminal")
        .unwrap()
        .unwrap()
        .state
        .terminal());
    assert_eq!(terminal_workspace.cleanup_calls.len(), 1);

    let cas_directory = tempfile::tempdir().unwrap();
    let cas_clock = ManualClock::new(5);
    let mut store =
        DurableLifecycleStore::open_with_clock(cas_directory.path(), Arc::new(cas_clock.clone()))
            .unwrap();
    let request = install_plan(
        "op-expiry-cas",
        "key-expiry-cas",
        "cas-game",
        LifecycleOperationKind::Install,
        "a",
        "1",
        "mods/a.dat",
        "source-v1",
        H1,
    )
    .request()
    .clone();
    let (lease, record) = match store
        .acquire_or_replay(&request, "cas-owner", "cas-lease", u64::MAX, 10)
        .unwrap()
    {
        AcquireOutcome::Acquired { lease, record } => (lease, record),
        other => panic!("expected acquired lease, got {other:?}"),
    };
    assert_eq!(lease.acquired_at_ms, 5);
    let mut payload = record.clone().into_payload();
    payload.phase = OperationPhase::Staging;
    payload.revision += 1;
    payload.updated_at_ms = 6;
    let next = OperationRecord::new(payload).unwrap();
    cas_clock.set(15);
    assert_eq!(
        store
            .compare_and_swap(&lease, record.revision, &next)
            .unwrap(),
        CompareAndSwapOutcome::LostLease
    );
    assert_eq!(
        store.operation_by_id("op-expiry-cas").unwrap().unwrap(),
        record
    );
}

#[test]
fn observed_lease_expiry_survives_clock_rollback_and_store_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(100);
    let mut store =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap();
    let request = install_plan(
        "op-clock-rollback",
        "key-clock-rollback",
        "clock-game",
        LifecycleOperationKind::Install,
        "a",
        "1",
        "mods/a.dat",
        "source-v1",
        H1,
    )
    .request()
    .clone();
    let (lease, record) = match store
        .acquire_or_replay(&request, "clock-owner", "clock-lease", 0, 10)
        .unwrap()
    {
        AcquireOutcome::Acquired { lease, record } => (lease, record),
        other => panic!("expected acquired lease, got {other:?}"),
    };
    let mut payload = record.clone().into_payload();
    payload.phase = OperationPhase::Staging;
    payload.revision += 1;
    payload.updated_at_ms = 111;
    let next = OperationRecord::new(payload).unwrap();

    clock.set(111);
    assert_eq!(
        store
            .compare_and_swap(&lease, record.revision, &next)
            .unwrap(),
        CompareAndSwapOutcome::LostLease
    );
    drop(store);

    clock.set(105);
    let mut reopened =
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap();
    assert_eq!(reopened.now_ms(), 111);
    assert_eq!(
        reopened
            .compare_and_swap(&lease, record.revision, &next)
            .unwrap(),
        CompareAndSwapOutcome::LostLease
    );
    assert!(reopened.renew(&lease, 105, 20).unwrap().is_none());
    assert_eq!(
        reopened.operation_by_id("op-clock-rollback").unwrap(),
        Some(record)
    );
}

#[test]
fn rollback_floor_after_reopen_allows_recovery_rebind_but_not_old_owner() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(100);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap(),
    );
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(FaultPoint::BeforeStagingEffect { index: 0 }));
    assert!(matches!(
        runtime.install(
            install_plan(
                "op-rebind-rollback",
                "key-rebind-rollback",
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity_with_ttl("lease-rebind-rollback", "generation-rebind-rollback", 10,),
            &mut workspace,
        ),
        LifecycleOutcome::RecoveryRequired { .. }
    ));
    let interrupted = runtime.store().interrupted_operations().unwrap().remove(0);
    clock.set(111);
    assert!(matches!(
        runtime
            .store_mut()
            .assert_lease_current(&interrupted.record, &interrupted.lease),
        Err(StoreError::LostLease)
    ));
    drop(runtime);

    clock.set(105);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap(),
    );
    assert_eq!(runtime.store().now_ms(), 111);
    let recovered = runtime.recover_startup(
        "recovery-owner",
        105,
        20,
        |_| "recovery-lease-after-rollback".to_owned(),
        &mut workspace,
    );
    assert!(matches!(
        recovered.as_slice(),
        [StartupRecoveryOutcome::Recovered { .. }]
    ));
    assert!(runtime
        .store()
        .operation_by_id("op-rebind-rollback")
        .unwrap()
        .unwrap()
        .state
        .terminal());
}

#[derive(Clone)]
struct RecordingFaults(Arc<Mutex<Vec<FaultPoint>>>);

impl FaultInjector for RecordingFaults {
    fn check(&mut self, point: &FaultPoint) -> Result<(), InjectedFault> {
        self.0.lock().unwrap().push(point.clone());
        Ok(())
    }
}

struct AdvanceClockAt {
    target: FaultPoint,
    clock: ManualClock,
    now_ms: u64,
    fired: bool,
}

impl AdvanceClockAt {
    fn new(target: FaultPoint, clock: ManualClock, now_ms: u64) -> Self {
        Self {
            target,
            clock,
            now_ms,
            fired: false,
        }
    }
}

impl FaultInjector for AdvanceClockAt {
    fn check(&mut self, point: &FaultPoint) -> Result<(), InjectedFault> {
        if !self.fired && *point == self.target {
            self.clock.set(self.now_ms);
            self.fired = true;
        }
        Ok(())
    }
}

struct ExternalChangeAt {
    target: FaultPoint,
    external_state: Arc<Mutex<Option<(String, ObservedFileState)>>>,
    replacement: Option<(String, ObservedFileState)>,
    fired: bool,
}

impl ExternalChangeAt {
    fn new(
        target: FaultPoint,
        external_state: Arc<Mutex<Option<(String, ObservedFileState)>>>,
        path: &str,
        state: ObservedFileState,
    ) -> Self {
        Self {
            target,
            external_state,
            replacement: Some((path.to_owned(), state)),
            fired: false,
        }
    }
}

impl FaultInjector for ExternalChangeAt {
    fn check(&mut self, point: &FaultPoint) -> Result<(), InjectedFault> {
        if !self.fired && *point == self.target {
            *self.external_state.lock().unwrap() = self.replacement.take();
            self.fired = true;
        }
        Ok(())
    }
}

fn expected_forward_fault_matrix(shape: MutationShape) -> Vec<FaultPoint> {
    use deltamod_product_contracts::{MutationCheckpoint, OperationPhase, OperationState};
    match shape {
        MutationShape::Create => vec![
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::JournalCreated),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::JournalCreated),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Staging)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Staging)),
            FaultPoint::BeforeStagingEffect { index: 0 },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Staged,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Staged,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::BackingUp)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::BackingUp)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Applying)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Applying)),
            FaultPoint::BeforeFilesystemEffect {
                index: 0,
                effect: deltamod_product_contracts::MutationSideEffect::Publish,
            },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Verifying)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Verifying)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::OutputVerified,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::OutputVerified,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Committing)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Committing)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestTemporary),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestTemporary),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestPublished),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestPublished),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
            FaultPoint::BeforeCleanup,
            FaultPoint::AfterCleanup,
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Terminal(
                OperationState::Succeeded,
            )),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Terminal(OperationState::Succeeded)),
        ],
        MutationShape::Replace => vec![
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::JournalCreated),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::JournalCreated),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Staging)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Staging)),
            FaultPoint::BeforeStagingEffect { index: 0 },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Staged,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Staged,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::BackingUp)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::BackingUp)),
            FaultPoint::BeforeBackupEffect { index: 0 },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::BackupVerified,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::BackupVerified,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Applying)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Applying)),
            FaultPoint::BeforeFilesystemEffect {
                index: 0,
                effect: deltamod_product_contracts::MutationSideEffect::Publish,
            },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Verifying)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Verifying)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::OutputVerified,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::OutputVerified,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Committing)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Committing)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestTemporary),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestTemporary),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestPublished),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestPublished),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
            FaultPoint::BeforeCleanup,
            FaultPoint::AfterCleanup,
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Terminal(
                OperationState::Succeeded,
            )),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Terminal(OperationState::Succeeded)),
        ],
        MutationShape::Delete => vec![
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::JournalCreated),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::JournalCreated),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Staging)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Staging)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::BackingUp)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::BackingUp)),
            FaultPoint::BeforeBackupEffect { index: 0 },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::BackupVerified,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::BackupVerified,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Applying)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Applying)),
            FaultPoint::BeforeFilesystemEffect {
                index: 0,
                effect: deltamod_product_contracts::MutationSideEffect::Remove,
            },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::Applied,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Verifying)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Verifying)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::OutputVerified,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: MutationCheckpoint::OutputVerified,
            }),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::Committing)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::Committing)),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestTemporary),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestTemporary),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestPublished),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestPublished),
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
            FaultPoint::BeforeCleanup,
            FaultPoint::AfterCleanup,
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Terminal(
                OperationState::Succeeded,
            )),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Terminal(OperationState::Succeeded)),
        ],
    }
}

fn expected_no_effect_recovery_fault_matrix() -> Vec<FaultPoint> {
    vec![
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
            index: 0,
            checkpoint: MutationCheckpoint::NoEffect,
        }),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
            index: 0,
            checkpoint: MutationCheckpoint::NoEffect,
        }),
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
        FaultPoint::BeforeCleanup,
        FaultPoint::AfterCleanup,
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::Terminal(OperationState::Recovered)),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::Terminal(OperationState::Recovered)),
    ]
}

fn expected_verified_commit_recovery_fault_matrix() -> Vec<FaultPoint> {
    vec![
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestTemporary),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestTemporary),
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestPublished),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestPublished),
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::Phase(OperationPhase::CleaningUp)),
        FaultPoint::BeforeCleanup,
        FaultPoint::AfterCleanup,
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::Terminal(OperationState::Recovered)),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::Terminal(OperationState::Recovered)),
    ]
}

fn recovery_seed_fault(shape: MutationShape) -> FaultPoint {
    match shape {
        MutationShape::Create | MutationShape::Replace => {
            FaultPoint::BeforeStagingEffect { index: 0 }
        }
        MutationShape::Delete => FaultPoint::BeforeBackupEffect { index: 0 },
    }
}

#[test]
fn every_forward_journal_checkpoint_is_faultable_and_recovers_deterministically() {
    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let probe_directory = tempfile::tempdir().unwrap();
        let probe_clock = ManualClock::new(1);
        let mut probe = ReleaseARuntime::new(
            DurableLifecycleStore::open_with_clock(probe_directory.path(), Arc::new(probe_clock))
                .unwrap(),
        );
        let mut probe_workspace = ModelWorkspace::new();
        probe_workspace.add_source("source-v1", H1);
        probe_workspace.add_source("source-v2", H2);
        if matches!(shape, MutationShape::Replace | MutationShape::Delete) {
            assert_success(probe.install(
                install_plan(
                    "op-probe-base",
                    "key-probe-base",
                    "game",
                    LifecycleOperationKind::Install,
                    "a",
                    "1",
                    "mods/a.dat",
                    "source-v1",
                    H1,
                ),
                identity("lease-probe-base", "generation-probe-base", 1),
                &mut probe_workspace,
            ));
        }
        let recorded = Arc::new(Mutex::new(Vec::new()));
        probe
            .store_mut()
            .set_fault_injector(RecordingFaults(recorded.clone()));
        let outcome = match shape {
            MutationShape::Create => probe.install(
                install_plan(
                    "op-probe",
                    "key-probe",
                    "game",
                    LifecycleOperationKind::Install,
                    "a",
                    "1",
                    "mods/a.dat",
                    "source-v1",
                    H1,
                ),
                identity("lease-probe", "generation-probe", 1),
                &mut probe_workspace,
            ),
            MutationShape::Replace => probe.install(
                install_plan(
                    "op-probe",
                    "key-probe",
                    "game",
                    LifecycleOperationKind::Update,
                    "a",
                    "2",
                    "mods/a.dat",
                    "source-v2",
                    H2,
                ),
                identity("lease-probe", "generation-probe", 1),
                &mut probe_workspace,
            ),
            MutationShape::Delete => probe.uninstall(
                uninstall_plan("op-probe", "key-probe", "game", "a"),
                identity("lease-probe", "generation-probe", 1),
                &mut probe_workspace,
            ),
        };
        assert_success(outcome);
        assert_eq!(
            recorded.lock().unwrap().as_slice(),
            expected_forward_fault_matrix(shape),
            "{shape:?} fault/checkpoint matrix drifted"
        );
    }

    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        for (case, point) in expected_forward_fault_matrix(shape).into_iter().enumerate() {
            let (_directory, mut runtime, clock, mut workspace, operation_id) =
                run_faulted_mutation(shape, point.clone());
            runtime.store_mut().clear_fault_injector();
            clock.set(100_000);
            let recovered = runtime.recover_startup(
                "recovery-runtime",
                100,
                20,
                |_| format!("recovery-forward-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            let operation = runtime
                .store()
                .operation_by_id(&operation_id)
                .unwrap()
                .unwrap();
            assert!(
                operation.state.terminal(),
                "{shape:?} at {point:?}: {operation:?}"
            );
            assert!(
                recovered.is_empty()
                    || recovered
                        .iter()
                        .all(|outcome| matches!(outcome, StartupRecoveryOutcome::Recovered { .. })),
                "{shape:?} at {point:?}: {recovered:?}"
            );
            assert_manifest_matches_workspace(&runtime, &workspace);
        }
    }
}

#[test]
fn manifest_only_and_recovery_checkpoint_matrices_are_explicit_and_faultable() {
    let directory = tempfile::tempdir().unwrap();
    let matrix_clock = ManualClock::new(1);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(matrix_clock)).unwrap(),
    );
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    assert_success(runtime.install(
        install_plan(
            "op-matrix-base",
            "key-matrix-base",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-matrix-base", "generation-matrix-base", 1),
        &mut workspace,
    ));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    runtime
        .store_mut()
        .set_fault_injector(RecordingFaults(recorded.clone()));
    assert_success(runtime.install(
        install_plan(
            "op-matrix-coown",
            "key-matrix-coown",
            "game",
            LifecycleOperationKind::Install,
            "b",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-matrix-coown", "generation-matrix-coown", 1),
        &mut workspace,
    ));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        [
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestOnlyCommit),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestOnlyCommit),
        ]
    );

    for point in [
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestOnlyCommit),
        FaultPoint::AfterJournalCas(JournalCheckpointKind::ManifestOnlyCommit),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let matrix_clock = ManualClock::new(1);
        let mut runtime = ReleaseARuntime::new(
            DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(matrix_clock))
                .unwrap(),
        );
        let mut workspace = ModelWorkspace::new();
        workspace.add_source("source-v1", H1);
        assert_success(runtime.install(
            install_plan(
                "op-manifest-only-base",
                "key-manifest-only-base",
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity(
                "lease-manifest-only-base",
                "generation-manifest-only-base",
                1,
            ),
            &mut workspace,
        ));
        runtime
            .store_mut()
            .set_fault_injector(FailOnce::new(point.clone()));
        let outcome = runtime.install(
            install_plan(
                "op-manifest-only-fault",
                "key-manifest-only-fault",
                "game",
                LifecycleOperationKind::Install,
                "b",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity(
                "lease-manifest-only-fault",
                "generation-manifest-only-fault",
                1,
            ),
            &mut workspace,
        );
        assert!(matches!(
            (&point, outcome),
            (
                FaultPoint::BeforeJournalCas(_),
                LifecycleOutcome::RecoveryRequired { .. }
            ) | (
                FaultPoint::AfterJournalCas(_),
                LifecycleOutcome::Existing { .. }
            )
        ));
    }

    let (_directory, mut runtime, clock, mut workspace, _operation_id) = run_faulted_mutation(
        MutationShape::Create,
        FaultPoint::BeforeStagingEffect { index: 0 },
    );
    runtime.store_mut().clear_fault_injector();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    runtime
        .store_mut()
        .set_fault_injector(RecordingFaults(recorded.clone()));
    clock.set(100_000);
    assert!(matches!(
        runtime
            .recover_startup(
                "recovery-runtime",
                0,
                100,
                |_| "recovery-matrix".into(),
                &mut workspace,
            )
            .as_slice(),
        [StartupRecoveryOutcome::Recovered { .. }]
    ));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        expected_no_effect_recovery_fault_matrix()
    );

    for (case, point) in expected_no_effect_recovery_fault_matrix()
        .into_iter()
        .enumerate()
    {
        let (_directory, mut runtime, clock, mut workspace, operation_id) = run_faulted_mutation(
            MutationShape::Create,
            FaultPoint::BeforeStagingEffect { index: 0 },
        );
        runtime.store_mut().set_fault_injector(FailOnce::new(point));
        clock.set(100_000);
        let first = runtime.recover_startup(
            "recovery-runtime",
            0,
            100,
            |_| format!("recovery-matrix-first-{case}"),
            &mut workspace,
        );
        assert!(matches!(
            first.as_slice(),
            [StartupRecoveryOutcome::Blocked { .. }]
        ));
        runtime.store_mut().clear_fault_injector();
        clock.set(100_200);
        let second = runtime.recover_startup(
            "recovery-runtime",
            0,
            100,
            |_| format!("recovery-matrix-second-{case}"),
            &mut workspace,
        );
        assert!(
            second.is_empty()
                || matches!(
                    second.as_slice(),
                    [StartupRecoveryOutcome::Recovered { .. }]
                )
        );
        assert!(runtime
            .store()
            .operation_by_id(&operation_id)
            .unwrap()
            .unwrap()
            .state
            .terminal());
    }
}

#[test]
fn create_replace_and_delete_reconcile_crashes_around_each_forward_effect() {
    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let mut points = vec![
            FaultPoint::BeforeFilesystemEffect {
                index: 0,
                effect: shape.forward_effect(),
            },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: deltamod_product_contracts::MutationCheckpoint::Applied,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: deltamod_product_contracts::MutationCheckpoint::Applied,
            }),
        ];
        if !matches!(shape, MutationShape::Delete) {
            points.extend([
                FaultPoint::BeforeStagingEffect { index: 0 },
                FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                    index: 0,
                    checkpoint: deltamod_product_contracts::MutationCheckpoint::Staged,
                }),
                FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                    index: 0,
                    checkpoint: deltamod_product_contracts::MutationCheckpoint::Staged,
                }),
            ]);
        }
        if !matches!(shape, MutationShape::Create) {
            points.extend([
                FaultPoint::BeforeBackupEffect { index: 0 },
                FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                    index: 0,
                    checkpoint: deltamod_product_contracts::MutationCheckpoint::BackupVerified,
                }),
                FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                    index: 0,
                    checkpoint: deltamod_product_contracts::MutationCheckpoint::BackupVerified,
                }),
            ]);
        }
        for (case, point) in points.into_iter().enumerate() {
            let (_directory, mut runtime, clock, mut workspace, operation_id) =
                run_faulted_mutation(shape, point.clone());
            runtime.store_mut().clear_fault_injector();
            clock.set(100_000);
            let recovered = runtime.recover_startup(
                "recovery-runtime",
                100,
                20,
                |_| format!("recovery-forward-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                matches!(
                    recovered.as_slice(),
                    [StartupRecoveryOutcome::Recovered { .. }]
                ),
                "{shape:?} {point:?}: {recovered:?}"
            );
            assert_eq!(
                runtime
                    .store()
                    .operation_by_id(&operation_id)
                    .unwrap()
                    .unwrap()
                    .state,
                deltamod_product_contracts::OperationState::Recovered
            );
            assert_rolled_back(shape, &runtime, &workspace);
        }
    }
}

#[test]
fn rollback_remove_and_restore_effects_are_reconciled_before_and_after_their_cas() {
    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let rollback_points = [
            FaultPoint::BeforeFilesystemEffect {
                index: 0,
                effect: shape.rollback_effect(),
            },
            FaultPoint::BeforeJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: deltamod_product_contracts::MutationCheckpoint::RolledBack,
            }),
            FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: deltamod_product_contracts::MutationCheckpoint::RolledBack,
            }),
        ];
        for (case, rollback_point) in rollback_points.into_iter().enumerate() {
            let forward_fault = FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
                index: 0,
                checkpoint: deltamod_product_contracts::MutationCheckpoint::Applied,
            });
            let (_directory, mut runtime, clock, mut workspace, operation_id) =
                run_faulted_mutation(shape, forward_fault);
            clock.set(100_000);
            runtime
                .store_mut()
                .set_fault_injector(FailOnce::new(rollback_point.clone()));
            let first_recovery = runtime.recover_startup(
                "recovery-runtime",
                100,
                20,
                |_| format!("rollback-first-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                matches!(
                    first_recovery.as_slice(),
                    [StartupRecoveryOutcome::Blocked { .. }]
                ),
                "{shape:?} {rollback_point:?}: {first_recovery:?}"
            );
            runtime.store_mut().clear_fault_injector();
            clock.set(100_100);
            let second_recovery = runtime.recover_startup(
                "recovery-runtime",
                200,
                20,
                |_| format!("rollback-second-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                matches!(
                    second_recovery.as_slice(),
                    [StartupRecoveryOutcome::Recovered { .. }]
                ),
                "{shape:?} {rollback_point:?}: {second_recovery:?}"
            );
            assert_eq!(
                runtime
                    .store()
                    .operation_by_id(&operation_id)
                    .unwrap()
                    .unwrap()
                    .state,
                deltamod_product_contracts::OperationState::Recovered
            );
            assert_rolled_back(shape, &runtime, &workspace);
        }
    }
}

#[test]
fn independent_recovery_and_verified_commit_matrices_fault_every_expected_edge() {
    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let (_directory, mut probe, clock, mut workspace, _operation_id) =
            run_faulted_mutation(shape, recovery_seed_fault(shape));
        probe.store_mut().clear_fault_injector();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        probe
            .store_mut()
            .set_fault_injector(RecordingFaults(recorded.clone()));
        clock.set(100_000);
        assert!(matches!(
            probe
                .recover_startup(
                    "recovery-runtime",
                    0,
                    100,
                    |_| format!("rollback-probe-{shape:?}").to_ascii_lowercase(),
                    &mut workspace,
                )
                .as_slice(),
            [StartupRecoveryOutcome::Recovered { .. }]
        ));
        assert_eq!(
            recorded.lock().unwrap().as_slice(),
            expected_no_effect_recovery_fault_matrix(),
            "{shape:?} rollback recovery checkpoint drift"
        );
        assert_rolled_back(shape, &probe, &workspace);

        for (case, point) in expected_no_effect_recovery_fault_matrix()
            .into_iter()
            .enumerate()
        {
            let (_directory, mut runtime, clock, mut workspace, operation_id) =
                run_faulted_mutation(shape, recovery_seed_fault(shape));
            runtime
                .store_mut()
                .set_fault_injector(FailOnce::new(point.clone()));
            clock.set(100_000);
            let first = runtime.recover_startup(
                "recovery-runtime",
                0,
                100,
                |_| format!("rollback-first-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                matches!(first.as_slice(), [StartupRecoveryOutcome::Blocked { .. }]),
                "{shape:?} at {point:?}: {first:?}"
            );
            runtime.store_mut().clear_fault_injector();
            clock.set(100_200);
            let second = runtime.recover_startup(
                "recovery-runtime",
                0,
                100,
                |_| format!("rollback-second-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                second.is_empty()
                    || matches!(
                        second.as_slice(),
                        [StartupRecoveryOutcome::Recovered { .. }]
                    ),
                "{shape:?} at {point:?}: {second:?}"
            );
            assert!(runtime
                .store()
                .operation_by_id(&operation_id)
                .unwrap()
                .unwrap()
                .state
                .terminal());
            assert_rolled_back(shape, &runtime, &workspace);
        }

        let verified_seed = FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
            index: 0,
            checkpoint: MutationCheckpoint::OutputVerified,
        });
        let (_directory, mut probe, clock, mut workspace, _operation_id) =
            run_faulted_mutation(shape, verified_seed.clone());
        probe.store_mut().clear_fault_injector();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        probe
            .store_mut()
            .set_fault_injector(RecordingFaults(recorded.clone()));
        clock.set(100_000);
        assert!(matches!(
            probe
                .recover_startup(
                    "recovery-runtime",
                    0,
                    100,
                    |_| format!("verified-probe-{shape:?}").to_ascii_lowercase(),
                    &mut workspace,
                )
                .as_slice(),
            [StartupRecoveryOutcome::Recovered { .. }]
        ));
        assert_eq!(
            recorded.lock().unwrap().as_slice(),
            expected_verified_commit_recovery_fault_matrix(),
            "{shape:?} verified-commit recovery checkpoint drift"
        );
        assert_manifest_matches_workspace(&probe, &workspace);

        for (case, point) in expected_verified_commit_recovery_fault_matrix()
            .into_iter()
            .enumerate()
        {
            let (_directory, mut runtime, clock, mut workspace, operation_id) =
                run_faulted_mutation(shape, verified_seed.clone());
            runtime
                .store_mut()
                .set_fault_injector(FailOnce::new(point.clone()));
            clock.set(100_000);
            let first = runtime.recover_startup(
                "recovery-runtime",
                0,
                100,
                |_| format!("verified-first-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                matches!(first.as_slice(), [StartupRecoveryOutcome::Blocked { .. }]),
                "{shape:?} at {point:?}: {first:?}"
            );
            runtime.store_mut().clear_fault_injector();
            clock.set(100_200);
            let second = runtime.recover_startup(
                "recovery-runtime",
                0,
                100,
                |_| format!("verified-second-{shape:?}-{case}").to_ascii_lowercase(),
                &mut workspace,
            );
            assert!(
                second.is_empty()
                    || matches!(
                        second.as_slice(),
                        [StartupRecoveryOutcome::Recovered { .. }]
                    ),
                "{shape:?} at {point:?}: {second:?}"
            );
            assert!(runtime
                .store()
                .operation_by_id(&operation_id)
                .unwrap()
                .unwrap()
                .state
                .terminal());
            assert_manifest_matches_workspace(&runtime, &workspace);
        }
    }
}

#[test]
fn manifest_only_commit_revalidates_changed_missing_and_replaced_claims_after_fault() {
    for (case, external_state) in [
        ("changed", regular(H3, "external-changed")),
        ("missing", ObservedFileState::Missing),
        ("replaced", regular(H1, "external-replaced")),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut runtime =
            ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
        let mut workspace = ModelWorkspace::new();
        workspace.add_source("source-v1", H1);
        assert_success(runtime.install(
            install_plan(
                &format!("op-manifest-stale-base-{case}"),
                &format!("key-manifest-stale-base-{case}"),
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity(
                &format!("lease-manifest-stale-base-{case}"),
                &format!("generation-manifest-stale-base-{case}"),
                1,
            ),
            &mut workspace,
        ));
        let manifest_before = runtime.store().manifest("game").unwrap().unwrap();
        let operation_id = format!("op-manifest-stale-{case}");
        runtime
            .store_mut()
            .set_fault_injector(ExternalChangeAt::new(
                FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestOnlyCommit),
                workspace.fault_external_state_handle(),
                "mods/a.dat",
                external_state.clone(),
            ));

        let outcome = runtime.install(
            install_plan(
                &operation_id,
                &format!("key-manifest-stale-{case}"),
                "game",
                LifecycleOperationKind::Install,
                "b",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity(
                &format!("lease-manifest-stale-{case}"),
                &format!("generation-manifest-stale-{case}"),
                20,
            ),
            &mut workspace,
        );

        match outcome {
            LifecycleOutcome::RecoveryRequired {
                operation,
                journal,
                error,
            } => {
                assert_eq!(error.code, ProductErrorCode::ExternalModification, "{case}");
                assert_eq!(operation.state, OperationState::Running, "{case}");
                assert!(journal.is_none(), "{case}");
            }
            other => panic!("{case}: expected recovery-required rejection, got {other:?}"),
        }
        assert_eq!(
            runtime.store().manifest("game").unwrap().unwrap(),
            manifest_before,
            "{case} published a stale co-owner claim"
        );
        assert_eq!(
            workspace.transaction_state("mods/a.dat"),
            external_state,
            "{case}"
        );
        assert!(!runtime
            .store()
            .operation_by_id(&operation_id)
            .unwrap()
            .unwrap()
            .state
            .terminal());
    }
}

#[test]
fn manifest_publication_cas_revalidates_create_replace_delete_forward_and_recovery() {
    let publication_edge = FaultPoint::BeforeJournalCas(JournalCheckpointKind::ManifestPublished);

    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let clock = ManualClock::new(1);
        let mut runtime = ReleaseARuntime::new(
            DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone()))
                .unwrap(),
        );
        let mut workspace = ModelWorkspace::new();
        workspace.add_source("source-v1", H1);
        workspace.add_source("source-v2", H2);
        if matches!(shape, MutationShape::Replace | MutationShape::Delete) {
            assert_success(runtime.install(
                install_plan(
                    "op-publication-base",
                    "key-publication-base",
                    "game",
                    LifecycleOperationKind::Install,
                    "a",
                    "1",
                    "mods/a.dat",
                    "source-v1",
                    H1,
                ),
                identity("lease-publication-base", "generation-publication-base", 1),
                &mut workspace,
            ));
        }
        let manifest_before = runtime.store().manifest("game").unwrap();
        runtime
            .store_mut()
            .set_fault_injector(ExternalChangeAt::new(
                publication_edge.clone(),
                workspace.fault_external_state_handle(),
                "mods/a.dat",
                regular(H3, "external-forward-publication"),
            ));
        let outcome = match shape {
            MutationShape::Create => runtime.install(
                install_plan(
                    "op-publication-forward",
                    "key-publication-forward",
                    "game",
                    LifecycleOperationKind::Install,
                    "a",
                    "1",
                    "mods/a.dat",
                    "source-v1",
                    H1,
                ),
                identity(
                    "lease-publication-forward",
                    "generation-publication-forward",
                    20,
                ),
                &mut workspace,
            ),
            MutationShape::Replace => runtime.install(
                install_plan(
                    "op-publication-forward",
                    "key-publication-forward",
                    "game",
                    LifecycleOperationKind::Update,
                    "a",
                    "2",
                    "mods/a.dat",
                    "source-v2",
                    H2,
                ),
                identity(
                    "lease-publication-forward",
                    "generation-publication-forward",
                    20,
                ),
                &mut workspace,
            ),
            MutationShape::Delete => runtime.uninstall(
                uninstall_plan(
                    "op-publication-forward",
                    "key-publication-forward",
                    "game",
                    "a",
                ),
                identity(
                    "lease-publication-forward",
                    "generation-publication-forward",
                    20,
                ),
                &mut workspace,
            ),
        };
        match outcome {
            LifecycleOutcome::RecoveryRequired {
                operation,
                journal: Some(journal),
                error,
            } => {
                assert_eq!(
                    error.code,
                    ProductErrorCode::ExternalModification,
                    "{shape:?}"
                );
                assert_eq!(operation.state, OperationState::Running, "{shape:?}");
                assert_eq!(
                    journal.manifest_commit_state,
                    deltamod_product_contracts::ManifestCommitState::TemporaryWritten,
                    "{shape:?}"
                );
            }
            other => panic!("{shape:?}: expected forward publication rejection, got {other:?}"),
        }
        assert_eq!(
            runtime.store().manifest("game").unwrap(),
            manifest_before,
            "{shape:?} forward path published stale claims"
        );
        assert_eq!(
            workspace.transaction_state("mods/a.dat"),
            regular(H3, "external-forward-publication"),
            "{shape:?} forward path overwrote the external file"
        );
    }

    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let verified_seed = FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
            index: 0,
            checkpoint: MutationCheckpoint::OutputVerified,
        });
        let (_directory, mut runtime, clock, mut workspace, operation_id) =
            run_faulted_mutation(shape, verified_seed);
        runtime.store_mut().clear_fault_injector();
        let manifest_before = runtime.store().manifest("game").unwrap();
        runtime
            .store_mut()
            .set_fault_injector(ExternalChangeAt::new(
                publication_edge.clone(),
                workspace.fault_external_state_handle(),
                "mods/a.dat",
                regular(H3, "external-recovery-publication"),
            ));
        clock.set(100_000);
        let recovered = runtime.recover_startup(
            "recovery-runtime",
            0,
            100,
            |_| format!("recovery-publication-{shape:?}").to_ascii_lowercase(),
            &mut workspace,
        );
        match recovered.as_slice() {
            [StartupRecoveryOutcome::Blocked {
                operation,
                journal: Some(journal),
                error,
            }] => {
                assert_eq!(
                    error.code,
                    ProductErrorCode::ExternalModification,
                    "{shape:?}"
                );
                assert_eq!(
                    operation.state,
                    OperationState::RecoveryRequired,
                    "{shape:?}"
                );
                assert_eq!(
                    journal.manifest_commit_state,
                    deltamod_product_contracts::ManifestCommitState::TemporaryWritten,
                    "{shape:?}"
                );
            }
            other => panic!("{shape:?}: expected recovery publication rejection, got {other:?}"),
        }
        assert_eq!(
            runtime.store().manifest("game").unwrap(),
            manifest_before,
            "{shape:?} recovery path published stale claims"
        );
        assert_eq!(
            workspace.transaction_state("mods/a.dat"),
            regular(H3, "external-recovery-publication"),
            "{shape:?} recovery path overwrote the external file"
        );
        assert_eq!(
            runtime
                .store()
                .operation_by_id(&operation_id)
                .unwrap()
                .unwrap()
                .state,
            OperationState::RecoveryRequired,
            "{shape:?} recovery path terminalized a blocked operation"
        );
    }
}

#[test]
fn startup_records_no_effect_and_finalizes_fully_verified_output() {
    let (_directory, mut runtime, clock, mut workspace, operation_id) = run_faulted_mutation(
        MutationShape::Create,
        FaultPoint::BeforeStagingEffect { index: 0 },
    );
    runtime.store_mut().clear_fault_injector();
    clock.set(100_000);
    let recovered = runtime.recover_startup(
        "recovery-runtime",
        100,
        20,
        |_| "recovery-no-effect".into(),
        &mut workspace,
    );
    assert!(matches!(
        recovered.as_slice(),
        [StartupRecoveryOutcome::Recovered {
            disposition: deltamod_product_contracts::JournalDisposition::RollBack,
            ..
        }]
    ));
    let journal = runtime
        .store()
        .journal_by_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        journal.mutations[0].checkpoint,
        deltamod_product_contracts::MutationCheckpoint::NoEffect
    );

    let (_directory, mut runtime, clock, mut workspace, operation_id) = run_faulted_mutation(
        MutationShape::Create,
        FaultPoint::BeforeJournalCas(JournalCheckpointKind::Phase(
            deltamod_product_contracts::OperationPhase::Committing,
        )),
    );
    runtime.store_mut().clear_fault_injector();
    clock.set(100_000);
    let recovered = runtime.recover_startup(
        "recovery-runtime",
        100,
        20,
        |_| "recovery-finalize".into(),
        &mut workspace,
    );
    assert!(matches!(
        recovered.as_slice(),
        [StartupRecoveryOutcome::Recovered {
            disposition: deltamod_product_contracts::JournalDisposition::FinalizeVerifiedCommit,
            ..
        }]
    ));
    let journal = runtime
        .store()
        .journal_by_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        journal.manifest_commit_state,
        deltamod_product_contracts::ManifestCommitState::Published
    );
    assert_eq!(
        state_hash(&workspace.transaction_state("mods/a.dat")),
        Some(H1)
    );
    assert!(runtime.store().manifest("game").unwrap().is_some());
}

#[test]
fn verified_commit_recovery_blocks_external_create_replace_and_delete_changes() {
    for shape in [
        MutationShape::Create,
        MutationShape::Replace,
        MutationShape::Delete,
    ] {
        let fault = FaultPoint::AfterJournalCas(JournalCheckpointKind::Mutation {
            index: 0,
            checkpoint: MutationCheckpoint::OutputVerified,
        });
        let (_directory, mut runtime, clock, mut workspace, operation_id) =
            run_faulted_mutation(shape, fault);
        let manifest_before = runtime.store().manifest("game").unwrap();
        workspace.externally_replace("mods/a.dat", H3);
        runtime.store_mut().clear_fault_injector();
        clock.set(100_000);

        let recovered = runtime.recover_startup(
            "recovery-runtime",
            0,
            100,
            |_| format!("stale-verified-{shape:?}").to_ascii_lowercase(),
            &mut workspace,
        );

        assert!(
            matches!(
                recovered.as_slice(),
                [StartupRecoveryOutcome::Blocked { .. }]
            ),
            "{shape:?}: {recovered:?}"
        );
        assert_eq!(runtime.store().manifest("game").unwrap(), manifest_before);
        assert_eq!(
            state_hash(&workspace.transaction_state("mods/a.dat")),
            Some(H3),
            "{shape:?} overwrote the external file"
        );
        let journal = runtime
            .store()
            .journal_by_operation(&operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            journal.manifest_commit_state,
            deltamod_product_contracts::ManifestCommitState::NotStarted,
            "{shape:?} published a stale target manifest"
        );
    }
}

#[test]
fn truncated_final_store_frame_is_removed_before_later_appends() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(directory.path()).unwrap());
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    assert_success(runtime.install(
        install_plan(
            "op-install",
            "key-install",
            "game",
            LifecycleOperationKind::Install,
            "a",
            "1",
            "mods/a.dat",
            "source-v1",
            H1,
        ),
        identity("lease-install", "generation-install", 1),
        &mut workspace,
    ));
    let log = runtime.store().root().join("lifecycle-state.log");
    let valid_length = std::fs::metadata(&log).unwrap().len();
    drop(runtime);
    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    file.write_all(b"DMLCST01\0\0").unwrap();
    file.sync_all().unwrap();
    drop(file);

    let store = DurableLifecycleStore::open(directory.path()).unwrap();
    assert_eq!(std::fs::metadata(&log).unwrap().len(), valid_length);
    assert!(store.operation_by_id("op-install").unwrap().is_some());
    let mut runtime = ReleaseARuntime::new(store);
    assert_success(runtime.uninstall(
        uninstall_plan("op-uninstall", "key-uninstall", "game", "a"),
        identity("lease-uninstall", "generation-uninstall", 20),
        &mut workspace,
    ));
    drop(runtime);
    let reopened = DurableLifecycleStore::open(directory.path()).unwrap();
    assert!(reopened.operation_by_id("op-uninstall").unwrap().is_some());
}

#[test]
fn opened_store_instances_reject_root_lock_and_log_identity_replacement() {
    for object in ["root", "lock", "log"] {
        let directory = tempfile::tempdir().unwrap();
        let store_root = directory.path().join("store");
        let (first, second, valid_log) = initialize_bound_store_pair(&store_root);

        match object {
            "root" => {
                let preserved_root = directory.path().join("preserved-store");
                std::fs::rename(&store_root, &preserved_root).unwrap();
                std::fs::create_dir(&store_root).unwrap();
                assert_store_identity_changed(first.operation_by_id("op-store-identity"));
                assert_store_identity_changed(second.operation_by_id("op-store-identity"));
                assert_eq!(
                    std::fs::read(preserved_root.join("lifecycle-state.log")).unwrap(),
                    valid_log
                );
            }
            "lock" => {
                let lock = store_root.join("lifecycle-state.lock");
                let preserved_lock = store_root.join("preserved.lock");
                std::fs::rename(&lock, &preserved_lock).unwrap();
                std::fs::File::create(&lock).unwrap();
                assert_store_identity_changed(first.operation_by_id("op-store-identity"));
                assert_store_identity_changed(second.operation_by_id("op-store-identity"));
                assert_eq!(
                    std::fs::read(store_root.join("lifecycle-state.log")).unwrap(),
                    valid_log
                );
            }
            "log" => {
                let log = store_root.join("lifecycle-state.log");
                let preserved_log = store_root.join("preserved.log");
                std::fs::rename(&log, &preserved_log).unwrap();
                std::fs::File::create(&log)
                    .unwrap()
                    .write_all(&valid_log)
                    .unwrap();
                assert_store_identity_changed(first.operation_by_id("op-store-identity"));
                assert_store_identity_changed(second.operation_by_id("op-store-identity"));
                assert_eq!(std::fs::read(&preserved_log).unwrap(), valid_log);
                assert_eq!(std::fs::read(&log).unwrap(), valid_log);
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(windows)]
#[test]
fn reopened_store_rejects_copied_root_lock_and_log_objects() {
    for object in ["root", "lock", "log"] {
        let directory = tempfile::tempdir().unwrap();
        let store_root = directory.path().join("store");
        let (first, second, valid_log) = initialize_bound_store_pair(&store_root);
        drop(first);
        drop(second);

        match object {
            "root" => {
                let preserved_root = directory.path().join("preserved-store");
                std::fs::rename(&store_root, &preserved_root).unwrap();
                std::fs::create_dir(&store_root).unwrap();
                std::fs::copy(
                    preserved_root.join("lifecycle-state.lock"),
                    store_root.join("lifecycle-state.lock"),
                )
                .unwrap();
                std::fs::copy(
                    preserved_root.join("lifecycle-state.log"),
                    store_root.join("lifecycle-state.log"),
                )
                .unwrap();
                assert_store_identity_changed(DurableLifecycleStore::open(&store_root));
                assert_eq!(
                    std::fs::read(preserved_root.join("lifecycle-state.log")).unwrap(),
                    valid_log
                );
            }
            "lock" | "log" => {
                let file_name = format!("lifecycle-state.{object}");
                let original = store_root.join(&file_name);
                let preserved = store_root.join(format!("preserved-{object}"));
                let replacement = store_root.join(format!("replacement-{object}"));
                std::fs::copy(&original, &replacement).unwrap();
                std::fs::rename(&original, &preserved).unwrap();
                std::fs::rename(&replacement, &original).unwrap();
                assert_store_identity_changed(DurableLifecycleStore::open(&store_root));
                assert_eq!(
                    std::fs::read(store_root.join("lifecycle-state.log")).unwrap(),
                    valid_log
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn store_root_lock_and_log_reparse_points_fail_closed_when_creation_is_permitted() {
    let directory = tempfile::tempdir().unwrap();
    let actual_root = directory.path().join("actual-root");
    let alias_root = directory.path().join("alias-root");
    let store = DurableLifecycleStore::open(&actual_root).unwrap();
    drop(store);
    if try_symlink_dir(&actual_root, &alias_root) {
        assert_store_identity_changed(DurableLifecycleStore::open(&alias_root));
    }

    let lock_directory = tempfile::tempdir().unwrap();
    let lock_root = lock_directory.path().join("store");
    let (lock_store, _second, valid_log) = initialize_bound_store_pair(&lock_root);
    let lock = lock_root.join("lifecycle-state.lock");
    let preserved_lock = lock_root.join("preserved.lock");
    std::fs::rename(&lock, &preserved_lock).unwrap();
    if try_symlink_file(&preserved_lock, &lock) {
        assert_store_identity_changed(lock_store.operation_by_id("op-store-identity"));
        assert_eq!(
            std::fs::read(lock_root.join("lifecycle-state.log")).unwrap(),
            valid_log
        );
    }

    let log_directory = tempfile::tempdir().unwrap();
    let log_root = log_directory.path().join("store");
    let (log_store, _second, valid_log) = initialize_bound_store_pair(&log_root);
    let log = log_root.join("lifecycle-state.log");
    let preserved_log = log_root.join("preserved.log");
    std::fs::rename(&log, &preserved_log).unwrap();
    if try_symlink_file(&preserved_log, &log) {
        assert_store_identity_changed(log_store.operation_by_id("op-store-identity"));
        assert_eq!(std::fs::read(&preserved_log).unwrap(), valid_log);
    }
}

#[test]
fn active_operation_blocks_second_writer_until_stale_preflight_is_recovered() {
    let directory = tempfile::tempdir().unwrap();
    let clock = ManualClock::new(1);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap(),
    );
    let mut workspace = ModelWorkspace::new();
    workspace.add_source("source-v1", H1);
    runtime
        .store_mut()
        .set_fault_injector(FailOnce::new(FaultPoint::BeforeJournalCas(
            JournalCheckpointKind::JournalCreated,
        )));
    assert!(matches!(
        runtime.install(
            install_plan(
                "op-first",
                "key-first",
                "game",
                LifecycleOperationKind::Install,
                "a",
                "1",
                "mods/a.dat",
                "source-v1",
                H1,
            ),
            identity("lease-first", "generation-first", 1),
            &mut workspace,
        ),
        LifecycleOutcome::RecoveryRequired { .. }
    ));
    drop(runtime);
    let mut runtime = ReleaseARuntime::new(
        DurableLifecycleStore::open_with_clock(directory.path(), Arc::new(clock.clone())).unwrap(),
    );
    runtime.store_mut().clear_fault_injector();
    assert!(matches!(
        runtime.install(
            install_plan(
                "op-second",
                "key-second",
                "game",
                LifecycleOperationKind::Install,
                "b",
                "1",
                "mods/b.dat",
                "source-v1",
                H1,
            ),
            identity("lease-second", "generation-second", 20),
            &mut workspace,
        ),
        LifecycleOutcome::Busy { .. }
    ));
    clock.set(100_000);
    let recovered = runtime.recover_startup(
        "recovery-runtime",
        20,
        10,
        |_| "recovery-preflight".into(),
        &mut workspace,
    );
    assert!(matches!(
        recovered.as_slice(),
        [StartupRecoveryOutcome::Recovered { journal: None, .. }]
    ));
}

#[test]
fn contract_sequence_headroom_reserves_the_final_terminal_checkpoint() {
    let mut payload = deltamod_product_contracts::fixtures::lifecycle_journal().into_payload();
    payload.phase = deltamod_product_contracts::OperationPhase::CleaningUp;
    payload.journal_sequence = u64::MAX - 1;
    let journal = deltamod_product_contracts::LifecycleJournal::new(payload.clone()).unwrap();
    assert_eq!(journal.journal_sequence, u64::MAX - 1);
    payload.journal_sequence = u64::MAX;
    assert!(deltamod_product_contracts::LifecycleJournal::new(payload).is_err());
}
