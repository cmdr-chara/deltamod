use deltamod_lifecycle_runtime::{
    DurableMutationGuard, LifecycleWorkspace, StagingSource, WorkspaceRoots,
};
use deltamod_product_contracts::{
    FilesystemBoundaryError, LifecycleFilesystemBoundary, LifecycleMutationGuard,
    MutationFenceError, ObservationSnapshot, ObservedFileState, PublicationReceipt,
    RootBoundObservation, RootIdentity, ValidatedMutationReconciliation,
    ValidatedMutationTransition, ValidatedRelativePath,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelObservation(ObservationSnapshot);

impl RootBoundObservation for ModelObservation {
    fn snapshot(&self) -> &ObservationSnapshot {
        &self.0
    }
}

#[derive(Default)]
pub struct ModelWorkspace {
    transaction_root: Option<RootIdentity>,
    roots: BTreeMap<String, RootIdentity>,
    entries: BTreeMap<(String, String), ObservedFileState>,
    sources: BTreeMap<String, String>,
    generation_backups: BTreeMap<String, RootIdentity>,
    fail_publish_index: Option<usize>,
}

impl ModelWorkspace {
    pub fn new() -> Self {
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

    pub fn add_source(&mut self, source_id: &str, sha256: &str) {
        self.sources.insert(source_id.to_owned(), sha256.to_owned());
    }

    pub fn transaction_state(&self, path: &str) -> ObservedFileState {
        let root = self.transaction_root.as_ref().expect("transaction root");
        self.entries
            .get(&(root.canonical_path_sha256.clone(), path.to_owned()))
            .cloned()
            .unwrap_or(ObservedFileState::Missing)
    }

    pub fn remove_transaction(&mut self, path: &str) {
        let root = self.transaction_root.as_ref().expect("transaction root");
        self.entries
            .remove(&(root.canonical_path_sha256.clone(), path.to_owned()));
    }

    pub fn replace_transaction(&mut self, path: &str, sha256: &str) {
        let root = self.transaction_root.as_ref().expect("transaction root");
        self.entries.insert(
            (root.canonical_path_sha256.clone(), path.to_owned()),
            regular(sha256, "external"),
        );
    }

    pub fn fail_publish_once(&mut self, index: usize) {
        self.fail_publish_index = Some(index);
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
                .entries
                .get(&Self::key(expected_root, path))
                .cloned()
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
        if self.fail_publish_index == Some(transition.mutation_index()) {
            self.fail_publish_index = None;
            return Err(FilesystemBoundaryError::Io);
        }
        guard.assert_current(now_ms).map_err(fence_error)?;
        self.ensure_current(staged)?;
        self.ensure_current(destination)?;
        let sha256 = transition.validate_publication(&staged.0, &destination.0)?;
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

pub fn state_hash(state: &ObservedFileState) -> Option<&str> {
    match state {
        ObservedFileState::Regular { sha256, .. } => Some(sha256),
        _ => None,
    }
}

fn root(marker: &str) -> RootIdentity {
    let digest = |suffix: &str| {
        Sha256::digest(format!("{marker}-{suffix}").as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    RootIdentity {
        canonical_path_sha256: digest("path"),
        volume_id: digest("volume"),
        file_id: digest("file"),
    }
}

fn regular(sha256: &str, file_id: &str) -> ObservedFileState {
    ObservedFileState::Regular {
        sha256: sha256.to_owned(),
        file_id: file_id.to_owned(),
        link_count: 1,
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
