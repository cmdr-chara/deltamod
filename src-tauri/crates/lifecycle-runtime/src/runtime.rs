use crate::{
    hex_digest, valid_id, ActiveProfilePointer, DurableLifecycleStore, DurableMutationGuard,
    FaultPoint, InstallationManifest, PlanError, ProfileLockfile, ProfilePreflightDisposition,
    ProfileSwitchPlan, RecoveryGenerationSnapshot, StagingSource, StoreError, Terminalization,
    ValidatedInstallPlan, ValidatedRepairPlan, ValidatedUninstallPlan,
};
use deltamod_product_contracts::{
    analyze_install_claims, plan_uninstall_claim, AcquireOutcome, ClaimAction, ClaimAnalysis,
    ConflictReason, ConflictReport, ConflictReportPayload, FileClaim, FileConflict,
    FilesystemBoundaryError, InstallationClaimsLedger, InstallationClaimsLedgerPayload,
    InstalledFileRef, InstalledModPayload, InstalledModRecord, JournalDisposition, JournalMutation,
    LifecycleFilesystemBoundary, LifecycleJournal, LifecycleJournalPayload, LifecycleOperationKind,
    LifecycleTransactionStore, ManifestCommitState, MutationAction, MutationCheckpoint,
    MutationSideEffect, ObservationSnapshot, ObservedFileState, OperationLease, OperationPhase,
    OperationRecord, OperationRequest, OperationState, OperationStore, ProductError,
    ProductErrorCode, ProductErrorPayload, ProposedClaim, PublicationReceipt,
    ReconciliationDecision, ReconciliationOutcome, ReconciliationScope, RecoveryAction,
    RootBoundObservation, RootIdentity, UninstallAction, ValidatedMutationReconciliation,
    ValidatedMutationTransition, ValidatedRecoveryRebind, ValidatedRelativePath,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionIdentity {
    pub owner_instance_id: String,
    pub lease_id: String,
    pub recovery_generation_id: String,
    pub now_ms: u64,
    pub lease_ttl_ms: u64,
}

impl ExecutionIdentity {
    fn validate(&self) -> bool {
        valid_id(&self.owner_instance_id, 128)
            && valid_id(&self.lease_id, 128)
            && valid_id(&self.recovery_generation_id, 128)
            && self.lease_ttl_ms > 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRoots {
    pub transaction: RootIdentity,
    pub staging: RootIdentity,
    pub backup: RootIdentity,
}

impl WorkspaceRoots {
    fn validate(&self) -> bool {
        self.transaction.validate().is_ok()
            && self.staging.validate().is_ok()
            && self.backup.validate().is_ok()
            && !self.transaction.same_filesystem_object(&self.staging)
            && !self.transaction.same_filesystem_object(&self.backup)
            && !self.staging.same_filesystem_object(&self.backup)
            && !self
                .transaction
                .canonical_path_sha256
                .eq_ignore_ascii_case(&self.staging.canonical_path_sha256)
            && !self
                .transaction
                .canonical_path_sha256
                .eq_ignore_ascii_case(&self.backup.canonical_path_sha256)
            && !self
                .staging
                .canonical_path_sha256
                .eq_ignore_ascii_case(&self.backup.canonical_path_sha256)
    }
}

/// Non-destination workspace capabilities. Destination publication, removal,
/// and restoration are deliberately absent: those effects must go through
/// [`LifecycleFilesystemBoundary`] with a durable mutation guard.
pub trait LifecycleWorkspace {
    fn transaction_root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError>;

    fn observe_preflight_no_follow(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError>;

    fn prepare_workspace(
        &mut self,
        operation_id: &str,
        recovery_generation_id: &str,
        expected_transaction_root: &RootIdentity,
    ) -> Result<WorkspaceRoots, FilesystemBoundaryError>;

    fn stage_file(
        &mut self,
        staging_root: &RootIdentity,
        staging_path: &ValidatedRelativePath,
        source: &StagingSource,
        expected_sha256: &str,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError>;

    fn backup_file(
        &mut self,
        transaction_root: &RootIdentity,
        destination_path: &ValidatedRelativePath,
        backup_root: &RootIdentity,
        backup_path: &ValidatedRelativePath,
        expected_sha256: &str,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError>;

    /// Removes staging data. `retain_backup` preserves a completed recovery
    /// generation; rollback removes the incomplete backup workspace as well.
    fn cleanup_workspace(
        &mut self,
        roots: &WorkspaceRoots,
        operation_id: &str,
        recovery_generation_id: &str,
        retain_backup: bool,
    ) -> Result<(), FilesystemBoundaryError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreflightFileAction {
    Create,
    Replace,
    CoOwnIdentical,
    Delete,
    KeepForOtherOwners,
    AlreadyMissing,
    Unchanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightFile {
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub action: PreflightFileAction,
    pub previous_sha256: Option<String>,
    pub proposed_sha256: Option<String>,
    pub existing_owners: BTreeSet<String>,
    pub backup_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreflightReport {
    pub operation_id: String,
    pub installation_id: String,
    pub kind: LifecycleOperationKind,
    pub provider: Option<deltamod_product_contracts::ProviderRef>,
    pub version: Option<String>,
    pub file_plan_fingerprint: Option<String>,
    pub files: Vec<PreflightFile>,
    pub staging_bytes: u64,
    pub backup_files: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleOutcome {
    Succeeded {
        operation: Box<OperationRecord>,
        journal: Option<Box<LifecycleJournal>>,
        preflight: Box<PreflightReport>,
    },
    Existing {
        operation: Box<OperationRecord>,
    },
    Busy {
        active_operation_id: String,
        expires_at_ms: u64,
        error: Box<ProductError>,
    },
    Rejected {
        operation: Option<Box<OperationRecord>>,
        error: Box<ProductError>,
        conflict: Option<Box<ConflictReport>>,
        preflight: Option<Box<PreflightReport>>,
    },
    RecoveryRequired {
        operation: Box<OperationRecord>,
        journal: Option<Box<LifecycleJournal>>,
        error: Box<ProductError>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupRecoveryOutcome {
    Recovered {
        operation: Box<OperationRecord>,
        journal: Option<Box<LifecycleJournal>>,
        disposition: JournalDisposition,
    },
    Active {
        operation: Box<OperationRecord>,
        expires_at_ms: u64,
    },
    Blocked {
        operation: Box<OperationRecord>,
        journal: Option<Box<LifecycleJournal>>,
        error: Box<ProductError>,
    },
    StoreBlocked {
        error: Box<ProductError>,
    },
}

#[derive(Debug)]
struct PreparedMutation {
    mutation: JournalMutation,
    source: Option<StagingSource>,
}

#[derive(Debug)]
struct PreparedOperation {
    request: OperationRequest,
    record: OperationRecord,
    lease: OperationLease,
    previous_manifest: Option<InstallationManifest>,
    target_manifest: InstallationManifest,
    transaction_root: RootIdentity,
    manifest_only_observations: Vec<ObservationSnapshot>,
    mutations: Vec<PreparedMutation>,
    preflight: PreflightReport,
    profile_commit: Option<PreparedProfileCommit>,
}

#[derive(Debug)]
struct PreparedProfileCommit {
    plan_fingerprint: String,
    previous: Option<ActiveProfilePointer>,
    target: ActiveProfilePointer,
    exact_noop: bool,
}

#[derive(Debug)]
struct PreflightFailure {
    error: Box<ProductError>,
    conflict: Option<Box<ConflictReport>>,
    report: Option<Box<PreflightReport>>,
}

#[derive(Debug)]
enum ExecutionFailure {
    Store(StoreError),
    Filesystem(FilesystemBoundaryError),
}

type ForwardResult = Result<
    (OperationRecord, LifecycleJournal, PreflightReport),
    Box<(OperationRecord, LifecycleJournal, ExecutionFailure)>,
>;
type RecoveryResumeResult = Result<
    (OperationRecord, LifecycleJournal),
    Box<(OperationRecord, LifecycleJournal, ProductErrorCode)>,
>;

impl From<StoreError> for ExecutionFailure {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<FilesystemBoundaryError> for ExecutionFailure {
    fn from(error: FilesystemBoundaryError) -> Self {
        Self::Filesystem(error)
    }
}

#[derive(Debug)]
pub struct ReleaseARuntime {
    store: DurableLifecycleStore,
}

impl ReleaseARuntime {
    #[must_use]
    pub fn new(store: DurableLifecycleStore) -> Self {
        Self { store }
    }

    #[must_use]
    pub fn store(&self) -> &DurableLifecycleStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut DurableLifecycleStore {
        &mut self.store
    }

    pub fn install<W>(
        &mut self,
        plan: ValidatedInstallPlan,
        identity: ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        if !identity.validate() {
            return rejected_invalid(None, plan.request().operation_id());
        }
        let acquired = match self.acquire(plan.request(), &identity) {
            Ok(outcome) => outcome,
            Err(outcome) => return outcome,
        };
        let (record, lease) = acquired;
        let prepared = match self.preflight_install(&plan, record.clone(), lease.clone(), workspace)
        {
            Ok(prepared) => prepared,
            Err(failure) => {
                let now_ms = self.store.now_ms();
                return self.finish_preflight_failure(record, lease, failure, now_ms);
            }
        };
        self.execute_prepared(prepared, &identity, workspace)
    }

    /// Explicit A2 update entry point. It uses the same validated file plan,
    /// journal, lease, and publication path as install, while rejecting an
    /// accidentally supplied install intent before lease acquisition.
    pub fn update<W>(
        &mut self,
        plan: ValidatedInstallPlan,
        identity: ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        if plan.request().intent().kind != LifecycleOperationKind::Update {
            return rejected_invalid(None, plan.request().operation_id());
        }
        self.install(plan, identity, workspace)
    }

    /// Executes a complete profile switch as one installation transaction.
    /// Child install/update identities remain plan bindings only: this method
    /// acquires exactly one outer `ProfileSwitch` lease and creates at most one
    /// filesystem journal for the whole batch.
    pub fn switch_profile<W>(
        &mut self,
        request: OperationRequest,
        plan: ProfileSwitchPlan,
        target: ProfileLockfile,
        resolved: Vec<ValidatedInstallPlan>,
        identity: ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let request_matches = request.validate().is_ok()
            && request.intent().kind == LifecycleOperationKind::ProfileSwitch
            && request.intent().mod_instance_id.is_none()
            && request.intent().provider.is_none()
            && request.intent().archive_sha256.is_none()
            && request.intent().installation_id == plan.installation_id()
            && request.intent().profile_id.as_deref() == Some(plan.target_profile_id())
            && request
                .intent()
                .file_plan_fingerprint
                .as_deref()
                .is_some_and(|fingerprint| fingerprint.eq_ignore_ascii_case(plan.fingerprint()))
            && target.installation_id == plan.installation_id()
            && target.profile_id == plan.target_profile_id()
            && target.fingerprint().as_deref() == Ok(plan.target_lock_fingerprint());
        if !identity.validate() || !request_matches {
            return rejected_invalid(None, request.operation_id());
        }
        let acquired = match self.acquire(&request, &identity) {
            Ok(outcome) => outcome,
            Err(outcome) => return outcome,
        };
        let (record, lease) = acquired;
        let prepared = match self.preflight_profile_switch(
            &request,
            &plan,
            &target,
            &resolved,
            record.clone(),
            lease.clone(),
            workspace,
        ) {
            Ok(prepared) => prepared,
            Err(failure) => {
                let now_ms = self.store.now_ms();
                return self.finish_preflight_failure(record, lease, failure, now_ms);
            }
        };
        self.execute_prepared(prepared, &identity, workspace)
    }

    pub fn uninstall<W>(
        &mut self,
        plan: ValidatedUninstallPlan,
        identity: ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        if !identity.validate() {
            return rejected_invalid(None, plan.request().operation_id());
        }
        let acquired = match self.acquire(plan.request(), &identity) {
            Ok(outcome) => outcome,
            Err(outcome) => return outcome,
        };
        let (record, lease) = acquired;
        let prepared = match self.preflight_uninstall(
            plan.request(),
            record.clone(),
            lease.clone(),
            workspace,
        ) {
            Ok(prepared) => prepared,
            Err(failure) => {
                let now_ms = self.store.now_ms();
                return self.finish_preflight_failure(record, lease, failure, now_ms);
            }
        };
        self.execute_prepared(prepared, &identity, workspace)
    }

    pub fn repair<W>(
        &mut self,
        plan: ValidatedRepairPlan,
        identity: ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        if !identity.validate() || plan.request().intent().kind != LifecycleOperationKind::Repair {
            return rejected_invalid(None, plan.request().operation_id());
        }
        let acquired = match self.acquire(plan.request(), &identity) {
            Ok(outcome) => outcome,
            Err(outcome) => return outcome,
        };
        let (record, lease) = acquired;
        let prepared = match self.preflight_repair(&plan, record.clone(), lease.clone(), workspace)
        {
            Ok(prepared) => prepared,
            Err(failure) => {
                let now_ms = self.store.now_ms();
                return self.finish_preflight_failure(record, lease, failure, now_ms);
            }
        };
        self.execute_prepared(prepared, &identity, workspace)
    }

    pub fn restore_last_working_state<W>(
        &mut self,
        request: OperationRequest,
        identity: ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        if !identity.validate()
            || request.validate().is_err()
            || request.intent().kind != LifecycleOperationKind::Recover
        {
            return rejected_invalid(None, request.operation_id());
        }
        let acquired = match self.acquire(&request, &identity) {
            Ok(outcome) => outcome,
            Err(outcome) => return outcome,
        };
        let (record, lease) = acquired;
        let candidates = match self.store.restore_source_candidates(&record, &lease) {
            Ok(candidates) => candidates,
            Err(error) => {
                return self.recovery_required_from_store(record, None, error);
            }
        };
        let mut newest_failure = None;
        for generation in candidates {
            let prepared = match self.preflight_restore(
                request.clone(),
                record.clone(),
                lease.clone(),
                &generation,
                self.store.now_ms(),
                workspace,
            ) {
                Ok(prepared) => prepared,
                Err(failure) => {
                    if newest_failure.is_none() {
                        newest_failure = Some(failure);
                    }
                    continue;
                }
            };
            if let Err(error) = self
                .store
                .protect_restore_source(&record, &lease, &generation)
            {
                return self.recovery_required_from_store(record, None, error);
            }
            return self.execute_prepared(prepared, &identity, workspace);
        }
        let failure = newest_failure.unwrap_or_else(|| PreflightFailure {
            error: Box::new(product_error(
                ProductErrorCode::RecoveryUnavailable,
                "lifecycle.recovery_unavailable",
                Some(request.operation_id()),
                Some(OperationPhase::Preflight),
                false,
                RecoveryAction::NoAction,
            )),
            conflict: None,
            report: None,
        });
        let now_ms = self.store.now_ms();
        self.finish_preflight_failure(record, lease, failure, now_ms)
    }

    fn acquire(
        &mut self,
        request: &OperationRequest,
        identity: &ExecutionIdentity,
    ) -> Result<(OperationRecord, OperationLease), LifecycleOutcome> {
        let now_ms = self.store.now_ms();
        match self.store.acquire_or_replay(
            request,
            &identity.owner_instance_id,
            &identity.lease_id,
            now_ms,
            identity.lease_ttl_ms,
        ) {
            Ok(AcquireOutcome::Acquired { record, lease }) => Ok((record, lease)),
            Ok(AcquireOutcome::Existing(operation)) => Err(LifecycleOutcome::Existing {
                operation: Box::new(operation),
            }),
            Ok(AcquireOutcome::Busy {
                active_operation_id,
                expires_at_ms,
            }) => Err(LifecycleOutcome::Busy {
                error: Box::new(product_error(
                    ProductErrorCode::InstallationBusy,
                    "lifecycle.installation_busy",
                    Some(request.operation_id()),
                    Some(OperationPhase::Preflight),
                    true,
                    RecoveryAction::Retry,
                )),
                active_operation_id,
                expires_at_ms,
            }),
            Err(StoreError::IdempotencyConflict) => Err(LifecycleOutcome::Rejected {
                operation: None,
                error: Box::new(product_error(
                    ProductErrorCode::IdempotencyConflict,
                    "lifecycle.idempotency_conflict",
                    Some(request.operation_id()),
                    Some(OperationPhase::Preflight),
                    false,
                    RecoveryAction::NoAction,
                )),
                conflict: None,
                preflight: None,
            }),
            Err(error) => Err(LifecycleOutcome::Rejected {
                operation: None,
                error: Box::new(product_error(
                    store_error_code(&error),
                    "lifecycle.store_failed",
                    Some(request.operation_id()),
                    Some(OperationPhase::Preflight),
                    false,
                    RecoveryAction::NoAction,
                )),
                conflict: None,
                preflight: None,
            }),
        }
    }

    fn preflight_install<W: LifecycleWorkspace>(
        &self,
        plan: &ValidatedInstallPlan,
        record: OperationRecord,
        lease: OperationLease,
        workspace: &W,
    ) -> Result<PreparedOperation, PreflightFailure> {
        let transaction_root = workspace
            .transaction_root_identity()
            .map_err(|error| preflight_filesystem_error(plan.request(), error, None))?;
        let previous_manifest = self
            .store
            .manifest(&plan.request().intent().installation_id)
            .map_err(|_| preflight_internal(plan.request(), None))?;
        let previous_record = previous_manifest.as_ref().and_then(|manifest| {
            manifest
                .records
                .iter()
                .find(|installed| installed.instance_id == plan.metadata().instance_id)
                .cloned()
        });
        if (plan.request().intent().kind == LifecycleOperationKind::Install
            && previous_record.is_some())
            || (plan.request().intent().kind == LifecycleOperationKind::Update
                && previous_record.is_none())
        {
            return Err(preflight_invalid(plan.request(), None));
        }
        let existing_claims = previous_manifest
            .as_ref()
            .map_or(&[][..], |manifest| manifest.ledger.claims.as_slice());
        let mut effective_claims = existing_claims.to_vec();
        let mut proposed = Vec::with_capacity(plan.files().len());
        let mut manifest_only_observations = Vec::with_capacity(plan.files().len());
        for file in plan.files() {
            let observation = workspace
                .observe_preflight_no_follow(&transaction_root, &file.path)
                .map_err(|error| preflight_filesystem_error(plan.request(), error, None))?;
            validate_preflight_observation(
                &observation,
                &transaction_root,
                &file.path,
                &file.path_identity_key,
            )
            .map_err(|error| preflight_filesystem_error(plan.request(), error, None))?;
            if let Some(expected) = file.expected_previous_sha256.as_deref() {
                if effective_claims
                    .iter()
                    .any(|claim| claim.path_identity_key == file.path_identity_key)
                {
                    return Err(preflight_invalid(plan.request(), None));
                }
                if let ObservedFileState::Regular {
                    sha256,
                    file_id,
                    link_count: 1,
                } = &observation.state
                {
                    if sha256.eq_ignore_ascii_case(expected) && valid_id(file_id, 256) {
                        effective_claims.push(FileClaim {
                            path: file.path.clone(),
                            path_identity_key: file.path_identity_key.clone(),
                            sha256: expected.to_ascii_lowercase(),
                            owners: BTreeSet::from([plan.metadata().instance_id.clone()]),
                        });
                    }
                }
            }
            proposed.push(ProposedClaim {
                observation,
                sha256: file.sha256.to_ascii_lowercase(),
            });
        }
        let analysis = analyze_install_claims(
            &plan.request().intent().installation_id,
            &transaction_root,
            &plan.metadata().instance_id,
            &effective_claims,
            &proposed,
        )
        .map_err(|_| preflight_invalid(plan.request(), None))?;
        let actions = match analysis {
            ClaimAnalysis::Ready(actions) => actions,
            ClaimAnalysis::Blocked(conflict) => {
                let code = conflict_code(&conflict);
                return Err(PreflightFailure {
                    error: Box::new(product_error(
                        code,
                        "lifecycle.conflict_detected",
                        Some(plan.request().operation_id()),
                        Some(OperationPhase::Preflight),
                        true,
                        RecoveryAction::ResolveConflict,
                    )),
                    conflict: Some(Box::new(conflict)),
                    report: None,
                });
            }
        };
        let generation = next_manifest_generation(previous_manifest.as_ref())
            .map_err(|_| preflight_invalid(plan.request(), None))?;
        let mut claims: BTreeMap<String, FileClaim> = effective_claims
            .iter()
            .cloned()
            .map(|claim| (claim.path_identity_key.clone(), claim))
            .collect();
        let mut prepared_mutations = Vec::new();
        let mut report_files = Vec::new();
        let planned_identities: BTreeSet<_> = plan
            .files()
            .iter()
            .map(|file| file.path_identity_key.clone())
            .collect();
        for ((file, proposal), action) in plan.files().iter().zip(proposed.iter()).zip(actions) {
            let previous = claims.get(&file.path_identity_key).cloned();
            let (report_action, mutation_action) = match &action {
                ClaimAction::Create(_) => {
                    (PreflightFileAction::Create, Some(MutationAction::Create))
                }
                ClaimAction::AddOwner(_) => (PreflightFileAction::CoOwnIdentical, None),
                ClaimAction::Replace(_) => {
                    (PreflightFileAction::Replace, Some(MutationAction::Replace))
                }
            };
            let next_claim = match action {
                ClaimAction::Create(claim)
                | ClaimAction::AddOwner(claim)
                | ClaimAction::Replace(claim) => claim,
            };
            claims.insert(next_claim.path_identity_key.clone(), next_claim);
            if let Some(action) = mutation_action {
                let index = u32::try_from(prepared_mutations.len())
                    .map_err(|_| preflight_invalid(plan.request(), None))?;
                let staging_path = generated_path("staging", index)
                    .map_err(|_| preflight_invalid(plan.request(), None))?;
                let backup_path = (action == MutationAction::Replace)
                    .then(|| generated_path("backup", index))
                    .transpose()
                    .map_err(|_| preflight_invalid(plan.request(), None))?;
                prepared_mutations.push(PreparedMutation {
                    mutation: JournalMutation {
                        index,
                        path: file.path.clone(),
                        path_identity_key: file.path_identity_key.clone(),
                        action,
                        checkpoint: MutationCheckpoint::Planned,
                        previous_sha256: previous.as_ref().map(|claim| claim.sha256.clone()),
                        expected_sha256: Some(file.sha256.to_ascii_lowercase()),
                        staging_path: Some(staging_path),
                        staging_sha256: None,
                        backup_path,
                        backup_sha256: None,
                    },
                    source: Some(file.source.clone()),
                });
            } else {
                manifest_only_observations.push(proposal.observation.clone());
            }
            report_files.push(PreflightFile {
                path: file.path.clone(),
                path_identity_key: file.path_identity_key.clone(),
                action: report_action,
                previous_sha256: previous.as_ref().map(|claim| claim.sha256.clone()),
                proposed_sha256: Some(file.sha256.to_ascii_lowercase()),
                existing_owners: previous.map_or_else(BTreeSet::new, |claim| claim.owners),
                backup_required: mutation_action == Some(MutationAction::Replace),
            });
        }
        if let Some(previous_record) = &previous_record {
            let mut removal_conflicts = Vec::new();
            for old_file in previous_record
                .files
                .iter()
                .filter(|file| !planned_identities.contains(&file.path_identity_key))
            {
                let claim = claims
                    .get(&old_file.path_identity_key)
                    .cloned()
                    .ok_or_else(|| preflight_invalid(plan.request(), None))?;
                let observation = workspace
                    .observe_preflight_no_follow(&transaction_root, &old_file.path)
                    .map_err(|error| preflight_filesystem_error(plan.request(), error, None))?;
                match plan_uninstall_claim(
                    &plan.metadata().instance_id,
                    &claim,
                    &transaction_root,
                    observation.clone(),
                )
                .map_err(|_| preflight_invalid(plan.request(), None))?
                {
                    UninstallAction::KeepForOtherOwners(next) => {
                        manifest_only_observations.push(observation);
                        claims.insert(next.path_identity_key.clone(), next);
                        report_files.push(preflight_from_claim(
                            &claim,
                            PreflightFileAction::KeepForOtherOwners,
                            false,
                        ));
                    }
                    UninstallAction::Delete { path } => {
                        claims.remove(&claim.path_identity_key);
                        let index = u32::try_from(prepared_mutations.len())
                            .map_err(|_| preflight_invalid(plan.request(), None))?;
                        prepared_mutations.push(PreparedMutation {
                            mutation: JournalMutation {
                                index,
                                path,
                                path_identity_key: claim.path_identity_key.clone(),
                                action: MutationAction::Delete,
                                checkpoint: MutationCheckpoint::Planned,
                                previous_sha256: Some(claim.sha256.clone()),
                                expected_sha256: None,
                                staging_path: None,
                                staging_sha256: None,
                                backup_path: Some(
                                    generated_path("backup", index)
                                        .map_err(|_| preflight_invalid(plan.request(), None))?,
                                ),
                                backup_sha256: None,
                            },
                            source: None,
                        });
                        report_files.push(preflight_from_claim(
                            &claim,
                            PreflightFileAction::Delete,
                            true,
                        ));
                    }
                    UninstallAction::AlreadyMissing { .. } => {
                        manifest_only_observations.push(observation);
                        claims.remove(&claim.path_identity_key);
                        report_files.push(preflight_from_claim(
                            &claim,
                            PreflightFileAction::AlreadyMissing,
                            false,
                        ));
                    }
                    UninstallAction::Blocked(conflict) => removal_conflicts.push(conflict),
                }
            }
            if !removal_conflicts.is_empty() {
                let conflict = ConflictReport::new(ConflictReportPayload {
                    installation_id: plan.request().intent().installation_id.clone(),
                    conflicts: removal_conflicts,
                })
                .map_err(|_| preflight_invalid(plan.request(), None))?;
                return Err(PreflightFailure {
                    error: Box::new(product_error(
                        conflict_code(&conflict),
                        "lifecycle.external_modification",
                        Some(plan.request().operation_id()),
                        Some(OperationPhase::Preflight),
                        true,
                        RecoveryAction::ResolveConflict,
                    )),
                    conflict: Some(Box::new(conflict)),
                    report: None,
                });
            }
        }
        let ledger = InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
            installation_id: plan.request().intent().installation_id.clone(),
            manifest_generation: generation,
            updated_at_ms: record.updated_at_ms,
            claims: claims.into_values().collect(),
        })
        .map_err(|_| preflight_invalid(plan.request(), None))?;
        let installed = InstalledModRecord::new(InstalledModPayload {
            instance_id: plan.metadata().instance_id.clone(),
            mod_id: plan.metadata().mod_id.clone(),
            installation_id: plan.request().intent().installation_id.clone(),
            display_name: plan.metadata().display_name.clone(),
            version: plan.metadata().version.clone(),
            provider: plan.metadata().provider.clone(),
            archive_sha256: plan.metadata().archive_sha256.clone(),
            file_plan_fingerprint: plan.fingerprint().to_owned(),
            manifest_generation: generation,
            installed_at_ms: previous_record
                .as_ref()
                .map_or(record.created_at_ms, |installed| installed.installed_at_ms),
            updated_at_ms: record.updated_at_ms,
            files: plan
                .files()
                .iter()
                .map(|file| InstalledFileRef {
                    path: file.path.clone(),
                    path_identity_key: file.path_identity_key.clone(),
                    expected_sha256: file.sha256.to_ascii_lowercase(),
                })
                .collect(),
        })
        .map_err(|_| preflight_invalid(plan.request(), None))?;
        let mut records = previous_manifest
            .as_ref()
            .map_or_else(Vec::new, |manifest| {
                manifest
                    .records
                    .iter()
                    .filter(|installed| installed.instance_id != plan.metadata().instance_id)
                    .cloned()
                    .collect()
            });
        for existing in &mut records {
            existing
                .try_update(|payload| {
                    payload.manifest_generation = generation;
                    payload.updated_at_ms = record.updated_at_ms;
                })
                .map_err(|_| preflight_invalid(plan.request(), None))?;
        }
        records.push(installed);
        let target_manifest = InstallationManifest::new(records, ledger)
            .map_err(|_| preflight_invalid(plan.request(), None))?;
        let backup_files = prepared_mutations
            .iter()
            .filter(|mutation| mutation.mutation.backup_path.is_some())
            .count() as u64;
        Ok(PreparedOperation {
            request: plan.request().clone(),
            record,
            lease,
            previous_manifest,
            target_manifest,
            transaction_root,
            manifest_only_observations,
            mutations: prepared_mutations,
            preflight: PreflightReport {
                operation_id: plan.request().operation_id().to_owned(),
                installation_id: plan.request().intent().installation_id.clone(),
                kind: plan.request().intent().kind,
                provider: Some(plan.metadata().provider.clone()),
                version: plan.metadata().version.clone(),
                file_plan_fingerprint: Some(plan.fingerprint().to_owned()),
                files: report_files,
                staging_bytes: plan.staging_bytes(),
                backup_files,
            },
            profile_commit: None,
        })
    }

    fn preflight_uninstall<W: LifecycleWorkspace>(
        &self,
        request: &OperationRequest,
        record: OperationRecord,
        lease: OperationLease,
        workspace: &W,
    ) -> Result<PreparedOperation, PreflightFailure> {
        let transaction_root = workspace
            .transaction_root_identity()
            .map_err(|error| preflight_filesystem_error(request, error, None))?;
        let previous_manifest = self
            .store
            .manifest(&request.intent().installation_id)
            .map_err(|_| preflight_internal(request, None))?
            .ok_or_else(|| preflight_invalid(request, None))?;
        let owner = request
            .intent()
            .mod_instance_id
            .as_deref()
            .ok_or_else(|| preflight_invalid(request, None))?;
        let installed = previous_manifest
            .records
            .iter()
            .find(|installed| installed.instance_id == owner)
            .cloned()
            .ok_or_else(|| preflight_invalid(request, None))?;
        let generation = previous_manifest
            .generation()
            .checked_add(1)
            .ok_or_else(|| preflight_invalid(request, None))?;
        let mut claims: BTreeMap<String, FileClaim> = previous_manifest
            .ledger
            .claims
            .iter()
            .cloned()
            .map(|claim| (claim.path_identity_key.clone(), claim))
            .collect();
        let mut prepared_mutations = Vec::new();
        let mut manifest_only_observations = Vec::with_capacity(installed.files.len());
        let mut report_files = Vec::new();
        let mut conflicts = Vec::new();
        for file in &installed.files {
            let claim = claims
                .get(&file.path_identity_key)
                .cloned()
                .ok_or_else(|| preflight_invalid(request, None))?;
            let observation = workspace
                .observe_preflight_no_follow(&transaction_root, &file.path)
                .map_err(|error| preflight_filesystem_error(request, error, None))?;
            let action =
                plan_uninstall_claim(owner, &claim, &transaction_root, observation.clone())
                    .map_err(|_| preflight_invalid(request, None))?;
            match action {
                UninstallAction::KeepForOtherOwners(next) => {
                    manifest_only_observations.push(observation);
                    claims.insert(next.path_identity_key.clone(), next.clone());
                    report_files.push(preflight_from_claim(
                        &claim,
                        PreflightFileAction::KeepForOtherOwners,
                        false,
                    ));
                }
                UninstallAction::Delete { path } => {
                    claims.remove(&claim.path_identity_key);
                    let index = u32::try_from(prepared_mutations.len())
                        .map_err(|_| preflight_invalid(request, None))?;
                    prepared_mutations.push(PreparedMutation {
                        mutation: JournalMutation {
                            index,
                            path: path.clone(),
                            path_identity_key: claim.path_identity_key.clone(),
                            action: MutationAction::Delete,
                            checkpoint: MutationCheckpoint::Planned,
                            previous_sha256: Some(claim.sha256.clone()),
                            expected_sha256: None,
                            staging_path: None,
                            staging_sha256: None,
                            backup_path: Some(
                                generated_path("backup", index)
                                    .map_err(|_| preflight_invalid(request, None))?,
                            ),
                            backup_sha256: None,
                        },
                        source: None,
                    });
                    report_files.push(preflight_from_claim(
                        &claim,
                        PreflightFileAction::Delete,
                        true,
                    ));
                }
                UninstallAction::AlreadyMissing { .. } => {
                    manifest_only_observations.push(observation);
                    claims.remove(&claim.path_identity_key);
                    report_files.push(preflight_from_claim(
                        &claim,
                        PreflightFileAction::AlreadyMissing,
                        false,
                    ));
                }
                UninstallAction::Blocked(conflict) => conflicts.push(conflict),
            }
        }
        if !conflicts.is_empty() {
            let conflict = ConflictReport::new(ConflictReportPayload {
                installation_id: request.intent().installation_id.clone(),
                conflicts,
            })
            .map_err(|_| preflight_invalid(request, None))?;
            return Err(PreflightFailure {
                error: Box::new(product_error(
                    conflict_code(&conflict),
                    "lifecycle.external_modification",
                    Some(request.operation_id()),
                    Some(OperationPhase::Preflight),
                    true,
                    RecoveryAction::ResolveConflict,
                )),
                conflict: Some(Box::new(conflict)),
                report: None,
            });
        }
        let ledger = InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
            installation_id: request.intent().installation_id.clone(),
            manifest_generation: generation,
            updated_at_ms: record.updated_at_ms,
            claims: claims.into_values().collect(),
        })
        .map_err(|_| preflight_invalid(request, None))?;
        let mut records = previous_manifest
            .records
            .iter()
            .filter(|candidate| candidate.instance_id != owner)
            .cloned()
            .collect::<Vec<_>>();
        for existing in &mut records {
            existing
                .try_update(|payload| {
                    payload.manifest_generation = generation;
                    payload.updated_at_ms = record.updated_at_ms;
                })
                .map_err(|_| preflight_invalid(request, None))?;
        }
        let target_manifest = InstallationManifest::new(records, ledger)
            .map_err(|_| preflight_invalid(request, None))?;
        let backup_files = prepared_mutations.len() as u64;
        Ok(PreparedOperation {
            request: request.clone(),
            record,
            lease,
            previous_manifest: Some(previous_manifest),
            target_manifest,
            transaction_root,
            manifest_only_observations,
            mutations: prepared_mutations,
            preflight: PreflightReport {
                operation_id: request.operation_id().to_owned(),
                installation_id: request.intent().installation_id.clone(),
                kind: LifecycleOperationKind::Uninstall,
                provider: Some(installed.provider.clone()),
                version: installed.version.clone(),
                file_plan_fingerprint: Some(installed.file_plan_fingerprint.clone()),
                files: report_files,
                staging_bytes: 0,
                backup_files,
            },
            profile_commit: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn preflight_profile_switch<W: LifecycleWorkspace>(
        &self,
        request: &OperationRequest,
        plan: &ProfileSwitchPlan,
        target_lock: &ProfileLockfile,
        resolved: &[ValidatedInstallPlan],
        record: OperationRecord,
        lease: OperationLease,
        workspace: &W,
    ) -> Result<PreparedOperation, PreflightFailure> {
        let transaction_root = workspace
            .transaction_root_identity()
            .map_err(|error| preflight_filesystem_error(request, error, None))?;
        let previous_manifest = self
            .store
            .manifest(plan.installation_id())
            .map_err(|_| preflight_internal(request, None))?;
        let previous_profile = self
            .store
            .active_profile(plan.installation_id())
            .map_err(|_| preflight_internal(request, None))?;
        let profile_preflight = plan.preflight(previous_manifest.as_ref(), resolved);
        if profile_preflight.disposition() == ProfilePreflightDisposition::Blocked {
            return Err(preflight_invalid(request, None));
        }
        let previous_profile_matches = match (
            plan.commit_boundary().previous_profile_id(),
            plan.previous_lock_fingerprint(),
            previous_profile.as_ref(),
        ) {
            (None, None, None) => true,
            (Some(profile_id), Some(lock_fingerprint), Some(pointer)) => {
                pointer.profile_id() == profile_id
                    && pointer
                        .lock_fingerprint()
                        .eq_ignore_ascii_case(lock_fingerprint)
                    && previous_manifest.as_ref().is_some_and(|manifest| {
                        pointer.manifest_generation() == manifest.generation()
                            && pointer
                                .manifest_fingerprint()
                                .eq_ignore_ascii_case(&manifest.fingerprint())
                    })
            }
            _ => false,
        };
        if !previous_profile_matches {
            return Err(preflight_invalid(request, None));
        }

        let resolved_by_instance: BTreeMap<_, _> = resolved
            .iter()
            .map(|resolved| (resolved.metadata().instance_id.as_str(), resolved))
            .collect();
        let current_records: BTreeMap<_, _> = previous_manifest
            .iter()
            .flat_map(|manifest| &manifest.records)
            .map(|installed| (installed.instance_id.clone(), installed.clone()))
            .collect();
        let current_claims: BTreeMap<_, _> = previous_manifest
            .iter()
            .flat_map(|manifest| &manifest.ledger.claims)
            .map(|claim| (claim.path_identity_key.clone(), claim.clone()))
            .collect();
        let mut simulated_records = current_records.clone();
        let mut simulated_claims = current_claims.clone();
        let mut sources_by_identity = BTreeMap::<String, StagingSource>::new();
        let mut effect_sequence_by_identity = BTreeMap::<String, u32>::new();

        for operation in profile_preflight.operations() {
            match operation.kind() {
                LifecycleOperationKind::Uninstall => {
                    simulated_records
                        .remove(operation.instance_id())
                        .ok_or_else(|| preflight_invalid(request, None))?;
                    let mut released = Vec::new();
                    for (identity_key, claim) in &mut simulated_claims {
                        if claim.owners.remove(operation.instance_id()) && claim.owners.is_empty() {
                            released.push(identity_key.clone());
                        }
                    }
                    for identity_key in released {
                        simulated_claims.remove(&identity_key);
                        effect_sequence_by_identity.insert(identity_key, operation.sequence());
                    }
                }
                LifecycleOperationKind::Install | LifecycleOperationKind::Update => {
                    let child = resolved_by_instance
                        .get(operation.instance_id())
                        .copied()
                        .ok_or_else(|| preflight_invalid(request, None))?;
                    if operation.kind() == LifecycleOperationKind::Install
                        && simulated_records.contains_key(operation.instance_id())
                        || operation.kind() == LifecycleOperationKind::Update
                            && !simulated_records.contains_key(operation.instance_id())
                    {
                        return Err(preflight_invalid(request, None));
                    }
                    if operation.kind() == LifecycleOperationKind::Update {
                        simulated_records.remove(operation.instance_id());
                        let mut released = Vec::new();
                        for (identity_key, claim) in &mut simulated_claims {
                            if claim.owners.remove(operation.instance_id())
                                && claim.owners.is_empty()
                            {
                                released.push(identity_key.clone());
                            }
                        }
                        for identity_key in released {
                            simulated_claims.remove(&identity_key);
                            effect_sequence_by_identity.insert(identity_key, operation.sequence());
                        }
                    }
                    for file in child.files() {
                        let changes_content = match simulated_claims
                            .get_mut(&file.path_identity_key)
                        {
                            Some(claim)
                                if claim.path == file.path
                                    && claim.sha256.eq_ignore_ascii_case(&file.sha256) =>
                            {
                                claim.owners.insert(operation.instance_id().to_owned());
                                false
                            }
                            Some(claim) if claim.owners.is_empty() => {
                                *claim = FileClaim {
                                    path: file.path.clone(),
                                    path_identity_key: file.path_identity_key.clone(),
                                    sha256: file.sha256.to_ascii_lowercase(),
                                    owners: BTreeSet::from([operation.instance_id().to_owned()]),
                                };
                                true
                            }
                            Some(_) => return Err(preflight_invalid(request, None)),
                            None => {
                                simulated_claims.insert(
                                    file.path_identity_key.clone(),
                                    FileClaim {
                                        path: file.path.clone(),
                                        path_identity_key: file.path_identity_key.clone(),
                                        sha256: file.sha256.to_ascii_lowercase(),
                                        owners: BTreeSet::from([operation
                                            .instance_id()
                                            .to_owned()]),
                                    },
                                );
                                true
                            }
                        };
                        if changes_content {
                            sources_by_identity
                                .insert(file.path_identity_key.clone(), file.source.clone());
                            effect_sequence_by_identity
                                .insert(file.path_identity_key.clone(), operation.sequence());
                        }
                    }
                    let installed_at_ms = current_records
                        .get(operation.instance_id())
                        .map_or(record.created_at_ms, |installed| installed.installed_at_ms);
                    simulated_records.insert(
                        operation.instance_id().to_owned(),
                        InstalledModRecord::new(InstalledModPayload {
                            instance_id: child.metadata().instance_id.clone(),
                            mod_id: child.metadata().mod_id.clone(),
                            installation_id: plan.installation_id().to_owned(),
                            display_name: child.metadata().display_name.clone(),
                            version: child.metadata().version.clone(),
                            provider: child.metadata().provider.clone(),
                            archive_sha256: child.metadata().archive_sha256.clone(),
                            file_plan_fingerprint: child.fingerprint().to_owned(),
                            manifest_generation: 1,
                            installed_at_ms,
                            updated_at_ms: record.updated_at_ms,
                            files: child
                                .files()
                                .iter()
                                .map(|file| InstalledFileRef {
                                    path: file.path.clone(),
                                    path_identity_key: file.path_identity_key.clone(),
                                    expected_sha256: file.sha256.to_ascii_lowercase(),
                                })
                                .collect(),
                        })
                        .map_err(|_| preflight_invalid(request, None))?,
                    );
                }
                _ => return Err(preflight_invalid(request, None)),
            }
        }

        let target_instances: BTreeSet<_> = target_lock
            .mods
            .iter()
            .map(|locked| locked.instance_id.as_str())
            .collect();
        if simulated_records.len() != target_instances.len()
            || simulated_records
                .keys()
                .any(|instance| !target_instances.contains(instance.as_str()))
        {
            return Err(preflight_invalid(request, None));
        }
        let exact_noop = plan.is_noop()
            && plan.commit_boundary().previous_profile_id() == Some(plan.target_profile_id())
            && plan.previous_lock_fingerprint().is_some_and(|fingerprint| {
                fingerprint.eq_ignore_ascii_case(plan.target_lock_fingerprint())
            });
        let target_manifest = if exact_noop {
            previous_manifest
                .clone()
                .ok_or_else(|| preflight_invalid(request, None))?
        } else {
            let generation = next_manifest_generation(previous_manifest.as_ref())
                .map_err(|_| preflight_invalid(request, None))?;
            for installed in simulated_records.values_mut() {
                installed
                    .try_update(|payload| {
                        payload.manifest_generation = generation;
                        payload.updated_at_ms = record.updated_at_ms;
                    })
                    .map_err(|_| preflight_invalid(request, None))?;
            }
            let ledger = InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
                installation_id: plan.installation_id().to_owned(),
                manifest_generation: generation,
                updated_at_ms: record.updated_at_ms,
                claims: simulated_claims.values().cloned().collect(),
            })
            .map_err(|_| preflight_invalid(request, None))?;
            InstallationManifest::new(simulated_records.into_values().collect(), ledger)
                .map_err(|_| preflight_invalid(request, None))?
        };
        validate_target_profile_manifest(target_lock, &target_manifest)
            .map_err(|_| preflight_invalid(request, None))?;

        let target_claims: BTreeMap<_, _> = target_manifest
            .ledger
            .claims
            .iter()
            .map(|claim| (claim.path_identity_key.clone(), claim))
            .collect();
        let mut identities_by_path = BTreeMap::<ValidatedRelativePath, String>::new();
        for claim in current_claims
            .values()
            .chain(target_claims.values().copied())
        {
            if identities_by_path
                .insert(claim.path.clone(), claim.path_identity_key.clone())
                .is_some_and(|existing| existing != claim.path_identity_key)
            {
                return Err(preflight_invalid(request, None));
            }
        }
        let mut ordered_identities: BTreeSet<_> = current_claims.keys().cloned().collect();
        ordered_identities.extend(target_claims.keys().cloned());
        let mut ordered_identities: Vec<_> = ordered_identities.into_iter().collect();
        ordered_identities.sort_by(|left, right| {
            effect_sequence_by_identity
                .get(left)
                .copied()
                .unwrap_or(u32::MAX)
                .cmp(
                    &effect_sequence_by_identity
                        .get(right)
                        .copied()
                        .unwrap_or(u32::MAX),
                )
                .then_with(|| left.cmp(right))
        });
        let mut prepared_mutations = Vec::new();
        let mut manifest_only_observations = Vec::new();
        let mut report_files = Vec::new();
        for identity_key in ordered_identities {
            let before = current_claims.get(&identity_key);
            let after = target_claims.get(&identity_key).copied();
            if before
                .zip(after)
                .is_some_and(|(before, after)| before.path != after.path)
            {
                return Err(preflight_invalid(request, None));
            }
            let claim = after
                .or(before)
                .ok_or_else(|| preflight_invalid(request, None))?;
            let observation = workspace
                .observe_preflight_no_follow(&transaction_root, &claim.path)
                .map_err(|error| preflight_filesystem_error(request, error, None))?;
            validate_preflight_observation(
                &observation,
                &transaction_root,
                &claim.path,
                &identity_key,
            )
            .map_err(|error| preflight_filesystem_error(request, error, None))?;
            let adopted_previous_sha = if before.is_none() {
                match &observation.state {
                    ObservedFileState::Regular { sha256, .. }
                        if resolved.iter().any(|child| {
                            child.files().iter().any(|file| {
                                file.path_identity_key == identity_key
                                    && file.expected_previous_sha256.as_deref().is_some_and(
                                        |expected| expected.eq_ignore_ascii_case(sha256),
                                    )
                            })
                        }) =>
                    {
                        Some(sha256.clone())
                    }
                    _ => None,
                }
            } else {
                None
            };
            let already_missing = before.is_some_and(|before| {
                after.is_none()
                    && before.owners.len() == 1
                    && observation.state == ObservedFileState::Missing
            });
            let observed_matches_before = match before {
                Some(before) => {
                    state_has_hash(&observation.state, &before.sha256) || already_missing
                }
                None => {
                    observation.state == ObservedFileState::Missing
                        || adopted_previous_sha.is_some()
                }
            };
            if !observed_matches_before {
                let conflict = ConflictReport::new(ConflictReportPayload {
                    installation_id: plan.installation_id().to_owned(),
                    conflicts: vec![FileConflict {
                        path: claim.path.clone(),
                        path_identity_key: identity_key.clone(),
                        reason: ConflictReason::ExternalModification,
                        expected_sha256: before.map(|claim| claim.sha256.clone()),
                        actual_sha256: match &observation.state {
                            ObservedFileState::Regular { sha256, .. } => Some(sha256.clone()),
                            _ => None,
                        },
                        proposed_sha256: after.map(|claim| claim.sha256.clone()),
                        existing_owners: before
                            .map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
                        requesting_owner: plan.target_profile_id().to_owned(),
                    }],
                })
                .map_err(|_| preflight_invalid(request, None))?;
                return Err(PreflightFailure {
                    error: Box::new(product_error(
                        ProductErrorCode::ExternalModification,
                        "lifecycle.external_modification",
                        Some(request.operation_id()),
                        Some(OperationPhase::Preflight),
                        true,
                        RecoveryAction::ResolveConflict,
                    )),
                    conflict: Some(Box::new(conflict)),
                    report: None,
                });
            }
            let logical_previous_sha256 = before
                .map(|claim| claim.sha256.clone())
                .or_else(|| adopted_previous_sha.clone());
            let physical_previous_sha256 = (!already_missing)
                .then(|| logical_previous_sha256.clone())
                .flatten();
            let mutation_action = match (physical_previous_sha256.as_deref(), after) {
                (None, Some(_)) if observation.state == ObservedFileState::Missing => {
                    Some(MutationAction::Create)
                }
                (None, Some(_)) => return Err(preflight_invalid(request, None)),
                (Some(previous), Some(after)) if !previous.eq_ignore_ascii_case(&after.sha256) => {
                    Some(MutationAction::Replace)
                }
                (Some(_), None) => Some(MutationAction::Delete),
                _ => None,
            };
            let report_action = match mutation_action {
                Some(MutationAction::Create) => PreflightFileAction::Create,
                Some(MutationAction::Replace) => PreflightFileAction::Replace,
                Some(MutationAction::Delete) => PreflightFileAction::Delete,
                None => PreflightFileAction::Unchanged,
            };
            report_files.push(PreflightFile {
                path: claim.path.clone(),
                path_identity_key: identity_key.clone(),
                action: report_action,
                previous_sha256: logical_previous_sha256,
                proposed_sha256: after.map(|claim| claim.sha256.clone()),
                existing_owners: before.map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
                backup_required: matches!(
                    mutation_action,
                    Some(MutationAction::Replace | MutationAction::Delete)
                ),
            });
            let Some(action) = mutation_action else {
                manifest_only_observations.push(observation);
                continue;
            };
            let index = u32::try_from(prepared_mutations.len())
                .map_err(|_| preflight_invalid(request, None))?;
            let expected_sha256 = after.map(|claim| claim.sha256.clone());
            let source = expected_sha256
                .as_ref()
                .map(|_| {
                    sources_by_identity
                        .get(&identity_key)
                        .cloned()
                        .ok_or_else(|| preflight_invalid(request, None))
                })
                .transpose()?;
            prepared_mutations.push(PreparedMutation {
                mutation: JournalMutation {
                    index,
                    path: claim.path.clone(),
                    path_identity_key: identity_key,
                    action,
                    checkpoint: MutationCheckpoint::Planned,
                    previous_sha256: physical_previous_sha256,
                    expected_sha256,
                    staging_path: (action != MutationAction::Delete)
                        .then(|| generated_path("staging", index))
                        .transpose()
                        .map_err(|_| preflight_invalid(request, None))?,
                    staging_sha256: None,
                    backup_path: matches!(action, MutationAction::Replace | MutationAction::Delete)
                        .then(|| generated_path("backup", index))
                        .transpose()
                        .map_err(|_| preflight_invalid(request, None))?,
                    backup_sha256: None,
                },
                source,
            });
        }
        let target_profile = ActiveProfilePointer::new(
            plan.installation_id(),
            plan.target_profile_id(),
            plan.target_lock_fingerprint(),
            &target_manifest,
        )
        .map_err(|_| preflight_invalid(request, None))?;
        let staging_bytes = resolved
            .iter()
            .try_fold(0_u64, |total, child| {
                total.checked_add(child.staging_bytes())
            })
            .ok_or_else(|| preflight_invalid(request, None))?;
        let backup_files = prepared_mutations
            .iter()
            .filter(|mutation| mutation.mutation.backup_path.is_some())
            .count() as u64;
        Ok(PreparedOperation {
            request: request.clone(),
            record,
            lease,
            previous_manifest,
            target_manifest,
            transaction_root,
            manifest_only_observations,
            mutations: prepared_mutations,
            preflight: PreflightReport {
                operation_id: request.operation_id().to_owned(),
                installation_id: plan.installation_id().to_owned(),
                kind: LifecycleOperationKind::ProfileSwitch,
                provider: None,
                version: None,
                file_plan_fingerprint: Some(plan.fingerprint().to_owned()),
                files: report_files,
                staging_bytes,
                backup_files,
            },
            profile_commit: Some(PreparedProfileCommit {
                plan_fingerprint: plan.fingerprint().to_owned(),
                previous: previous_profile,
                target: target_profile,
                exact_noop,
            }),
        })
    }

    fn preflight_repair<W: LifecycleWorkspace>(
        &self,
        plan: &ValidatedRepairPlan,
        record: OperationRecord,
        lease: OperationLease,
        workspace: &W,
    ) -> Result<PreparedOperation, PreflightFailure> {
        let request = plan.request();
        let transaction_root = workspace
            .transaction_root_identity()
            .map_err(|error| preflight_filesystem_error(request, error, None))?;
        let previous_manifest = self
            .store
            .manifest(&request.intent().installation_id)
            .map_err(|_| preflight_internal(request, None))?
            .ok_or_else(|| preflight_invalid(request, None))?;
        if previous_manifest.generation() != plan.manifest_generation() {
            return Err(preflight_maintenance_changed(request, None));
        }
        let instance_id = request
            .intent()
            .mod_instance_id
            .as_deref()
            .ok_or_else(|| preflight_invalid(request, None))?;
        let installed = previous_manifest
            .records
            .iter()
            .find(|installed| installed.instance_id == instance_id)
            .cloned()
            .ok_or_else(|| preflight_invalid(request, None))?;
        let installed_files: BTreeMap<_, _> = installed
            .files
            .iter()
            .map(|file| (file.path_identity_key.as_str(), file))
            .collect();
        if plan.files().is_empty()
            || plan.files().iter().any(|file| {
                file.source != plan.source().staging_source(&file.path)
                    || installed_files
                        .get(file.path_identity_key.as_str())
                        .is_none_or(|installed| {
                            installed.path != file.path
                                || !installed.expected_sha256.eq_ignore_ascii_case(&file.sha256)
                        })
            })
        {
            return Err(preflight_invalid(request, None));
        }
        let planned: BTreeMap<_, _> = plan
            .files()
            .iter()
            .map(|file| (file.path_identity_key.as_str(), file))
            .collect();
        let mut mutations = Vec::with_capacity(plan.files().len());
        let mut manifest_only_observations = Vec::new();
        let mut report_files = Vec::with_capacity(installed.files.len());
        for installed_file in &installed.files {
            let observation = workspace
                .observe_preflight_no_follow(&transaction_root, &installed_file.path)
                .map_err(|error| preflight_filesystem_error(request, error, None))?;
            validate_preflight_observation(
                &observation,
                &transaction_root,
                &installed_file.path,
                &installed_file.path_identity_key,
            )
            .map_err(|error| preflight_filesystem_error(request, error, None))?;
            if let Some(file) = planned.get(installed_file.path_identity_key.as_str()) {
                if observation.state != ObservedFileState::Missing {
                    return Err(preflight_maintenance_changed(request, None));
                }
                let index =
                    u32::try_from(mutations.len()).map_err(|_| preflight_invalid(request, None))?;
                mutations.push(PreparedMutation {
                    mutation: JournalMutation {
                        index,
                        path: file.path.clone(),
                        path_identity_key: file.path_identity_key.clone(),
                        action: MutationAction::Create,
                        checkpoint: MutationCheckpoint::Planned,
                        previous_sha256: None,
                        expected_sha256: Some(file.sha256.to_ascii_lowercase()),
                        staging_path: Some(
                            generated_path("staging", index)
                                .map_err(|_| preflight_invalid(request, None))?,
                        ),
                        staging_sha256: None,
                        backup_path: None,
                        backup_sha256: None,
                    },
                    source: Some(file.source.clone()),
                });
                report_files.push(PreflightFile {
                    path: file.path.clone(),
                    path_identity_key: file.path_identity_key.clone(),
                    action: PreflightFileAction::Create,
                    previous_sha256: None,
                    proposed_sha256: Some(file.sha256.to_ascii_lowercase()),
                    existing_owners: previous_manifest
                        .ledger
                        .claims
                        .iter()
                        .find(|claim| claim.path_identity_key == file.path_identity_key)
                        .map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
                    backup_required: false,
                });
            } else {
                validate_regular_snapshot(
                    &observation,
                    &transaction_root,
                    &installed_file.path,
                    Some(&installed_file.path_identity_key),
                    &installed_file.expected_sha256,
                )
                .map_err(|error| preflight_filesystem_error(request, error, None))?;
                manifest_only_observations.push(observation);
                report_files.push(PreflightFile {
                    path: installed_file.path.clone(),
                    path_identity_key: installed_file.path_identity_key.clone(),
                    action: PreflightFileAction::Unchanged,
                    previous_sha256: Some(installed_file.expected_sha256.clone()),
                    proposed_sha256: Some(installed_file.expected_sha256.clone()),
                    existing_owners: previous_manifest
                        .ledger
                        .claims
                        .iter()
                        .find(|claim| claim.path_identity_key == installed_file.path_identity_key)
                        .map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
                    backup_required: false,
                });
            }
        }
        if mutations.len() != plan.files().len() {
            return Err(preflight_invalid(request, None));
        }
        let next_generation = previous_manifest
            .generation()
            .checked_add(1)
            .ok_or_else(|| preflight_invalid(request, None))?;
        let target_manifest = rebase_manifest(
            Some(&previous_manifest),
            &request.intent().installation_id,
            next_generation,
            record.updated_at_ms,
        )
        .map_err(|_| preflight_invalid(request, None))?;
        Ok(PreparedOperation {
            request: request.clone(),
            record,
            lease,
            previous_manifest: Some(previous_manifest),
            target_manifest,
            transaction_root,
            manifest_only_observations,
            mutations,
            preflight: PreflightReport {
                operation_id: request.operation_id().to_owned(),
                installation_id: request.intent().installation_id.clone(),
                kind: LifecycleOperationKind::Repair,
                provider: Some(installed.provider.clone()),
                version: installed.version.clone(),
                file_plan_fingerprint: Some(installed.file_plan_fingerprint.clone()),
                files: report_files,
                staging_bytes: plan.files().iter().map(|file| file.size_bytes).sum(),
                backup_files: 0,
            },
            profile_commit: None,
        })
    }

    fn preflight_restore<W: LifecycleWorkspace>(
        &self,
        request: OperationRequest,
        record: OperationRecord,
        lease: OperationLease,
        generation: &RecoveryGenerationSnapshot,
        now_ms: u64,
        workspace: &W,
    ) -> Result<PreparedOperation, PreflightFailure> {
        let transaction_root = workspace
            .transaction_root_identity()
            .map_err(|error| preflight_filesystem_error(&request, error, None))?;
        let current = self
            .store
            .manifest(&request.intent().installation_id)
            .map_err(|_| preflight_internal(&request, None))?
            .ok_or_else(|| preflight_invalid(&request, None))?;
        let next_generation = current
            .generation()
            .checked_add(1)
            .ok_or_else(|| preflight_invalid(&request, None))?;
        let target = rebase_manifest(
            generation.previous_manifest.as_ref(),
            &request.intent().installation_id,
            next_generation,
            now_ms,
        )
        .map_err(|_| preflight_invalid(&request, None))?;
        let current_claims: BTreeMap<_, _> = current
            .ledger
            .claims
            .iter()
            .map(|claim| (claim.path_identity_key.clone(), claim))
            .collect();
        let target_claims: BTreeMap<_, _> = target
            .ledger
            .claims
            .iter()
            .map(|claim| (claim.path_identity_key.clone(), claim))
            .collect();
        let source_journal = self
            .store
            .journal_by_operation(&generation.operation_id)
            .map_err(|_| preflight_internal(&request, None))?;
        let identities: BTreeSet<_> = current_claims
            .keys()
            .chain(target_claims.keys())
            .cloned()
            .collect();
        let mut mutations = Vec::new();
        let mut manifest_only_observations = Vec::new();
        let mut report_files = Vec::new();
        for identity_key in identities {
            let before = current_claims.get(&identity_key).copied();
            let after = target_claims.get(&identity_key).copied();
            let path = before
                .map(|claim| &claim.path)
                .or_else(|| after.map(|claim| &claim.path))
                .ok_or_else(|| preflight_invalid(&request, None))?;
            if before.is_some_and(|claim| after.is_some_and(|next| claim.path != next.path)) {
                return Err(preflight_invalid(&request, None));
            }
            let observation = workspace
                .observe_preflight_no_follow(&transaction_root, path)
                .map_err(|error| preflight_filesystem_error(&request, error, None))?;
            validate_preflight_observation(&observation, &transaction_root, path, &identity_key)
                .map_err(|error| preflight_filesystem_error(&request, error, None))?;
            require_claim_observation(before, &observation)
                .map_err(|error| preflight_filesystem_error(&request, error, None))?;
            let (action, report_action, source) = match (before, after) {
                (Some(_before), None) => (
                    Some(MutationAction::Delete),
                    PreflightFileAction::Delete,
                    None,
                ),
                (None, Some(after)) => (
                    Some(MutationAction::Create),
                    PreflightFileAction::Create,
                    Some(recovery_source(generation, source_journal.as_ref(), after)?),
                ),
                (Some(before), Some(after))
                    if !before.sha256.eq_ignore_ascii_case(&after.sha256) =>
                {
                    (
                        Some(MutationAction::Replace),
                        PreflightFileAction::Replace,
                        Some(recovery_source(generation, source_journal.as_ref(), after)?),
                    )
                }
                (Some(_), Some(_)) => (None, PreflightFileAction::Unchanged, None),
                (None, None) => return Err(preflight_invalid(&request, None)),
            };
            if action.is_none() {
                manifest_only_observations.push(observation);
            }
            if let Some(action) = action {
                let index = u32::try_from(mutations.len())
                    .map_err(|_| preflight_invalid(&request, None))?;
                mutations.push(PreparedMutation {
                    mutation: JournalMutation {
                        index,
                        path: path.clone(),
                        path_identity_key: identity_key.clone(),
                        action,
                        checkpoint: MutationCheckpoint::Planned,
                        previous_sha256: before.map(|claim| claim.sha256.clone()),
                        expected_sha256: after.map(|claim| claim.sha256.clone()),
                        staging_path: (action != MutationAction::Delete)
                            .then(|| generated_path("staging", index))
                            .transpose()
                            .map_err(|_| preflight_invalid(&request, None))?,
                        staging_sha256: None,
                        backup_path: (action != MutationAction::Create)
                            .then(|| generated_path("backup", index))
                            .transpose()
                            .map_err(|_| preflight_invalid(&request, None))?,
                        backup_sha256: None,
                    },
                    source,
                });
            }
            report_files.push(PreflightFile {
                path: path.clone(),
                path_identity_key: identity_key,
                action: report_action,
                previous_sha256: before.map(|claim| claim.sha256.clone()),
                proposed_sha256: after.map(|claim| claim.sha256.clone()),
                existing_owners: before.map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
                backup_required: matches!(
                    action,
                    Some(MutationAction::Replace | MutationAction::Delete)
                ),
            });
        }
        Ok(PreparedOperation {
            request: request.clone(),
            record,
            lease,
            previous_manifest: Some(current),
            target_manifest: target,
            transaction_root,
            manifest_only_observations,
            preflight: PreflightReport {
                operation_id: request.operation_id().to_owned(),
                installation_id: request.intent().installation_id.clone(),
                kind: LifecycleOperationKind::Recover,
                provider: None,
                version: None,
                file_plan_fingerprint: request.intent().file_plan_fingerprint.clone(),
                staging_bytes: 0,
                backup_files: mutations
                    .iter()
                    .filter(|mutation| mutation.mutation.backup_path.is_some())
                    .count() as u64,
                files: report_files,
            },
            mutations,
            profile_commit: None,
        })
    }

    fn execute_prepared<W>(
        &mut self,
        prepared: PreparedOperation,
        identity: &ExecutionIdentity,
        workspace: &mut W,
    ) -> LifecycleOutcome
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let clock = self.store.clock();
        if prepared.mutations.is_empty() {
            let result_fingerprint = manifest_fingerprint(&prepared.target_manifest);
            let transaction_root = &prepared.transaction_root;
            let observations = &prepared.manifest_only_observations;
            let completed = if let Some(profile) = &prepared.profile_commit {
                if profile.exact_noop {
                    self.store.complete_profile_switch_noop(
                        &prepared.record,
                        &prepared.lease,
                        &result_fingerprint,
                        &profile.plan_fingerprint,
                        &profile.target,
                        || {
                            revalidate_manifest_only_claims::<W>(
                                workspace,
                                transaction_root,
                                observations,
                            )
                        },
                    )
                } else {
                    self.store.complete_profile_switch_manifest_only(
                        &prepared.record,
                        &prepared.lease,
                        &identity.recovery_generation_id,
                        &prepared.target_manifest,
                        &result_fingerprint,
                        &profile.plan_fingerprint,
                        profile.previous.as_ref(),
                        &profile.target,
                        || {
                            revalidate_manifest_only_claims::<W>(
                                workspace,
                                transaction_root,
                                observations,
                            )
                        },
                    )
                }
            } else {
                self.store.complete_manifest_only(
                    &prepared.record,
                    &prepared.lease,
                    &identity.recovery_generation_id,
                    &prepared.target_manifest,
                    &result_fingerprint,
                    || {
                        revalidate_manifest_only_claims::<W>(
                            workspace,
                            transaction_root,
                            observations,
                        )
                    },
                )
            };
            return match completed {
                Ok(operation) => LifecycleOutcome::Succeeded {
                    operation: Box::new(operation),
                    journal: None,
                    preflight: Box::new(prepared.preflight),
                },
                Err(error) => self.recovery_required_from_store(prepared.record, None, error),
            };
        }
        let roots = match workspace.prepare_workspace(
            prepared.request.operation_id(),
            &identity.recovery_generation_id,
            &prepared.transaction_root,
        ) {
            Ok(roots) if roots.validate() && roots.transaction == prepared.transaction_root => {
                roots
            }
            Ok(_) => {
                return self.recovery_required_from_filesystem(
                    prepared.record,
                    None,
                    FilesystemBoundaryError::RootIdentityChanged,
                )
            }
            Err(error) => {
                return self.recovery_required_from_filesystem(prepared.record, None, error)
            }
        };
        let recovery_chain_sha256 = hex_digest(
            format!(
                "{}\0{}\0{}",
                prepared.request.operation_id(),
                prepared.lease.lease_id,
                identity.recovery_generation_id
            )
            .as_bytes(),
        );
        let journal_started_at_ms = clock.now_ms();
        let journal = match LifecycleJournal::new(LifecycleJournalPayload {
            journal_sequence: 1,
            operation_id: prepared.request.operation_id().to_owned(),
            idempotency_key: prepared.request.idempotency_key().to_owned(),
            request_fingerprint: prepared.request.request_fingerprint().to_owned(),
            lease_id: prepared.lease.lease_id.clone(),
            operation_revision: prepared.record.revision,
            fencing_token: prepared.record.fencing_token,
            installation_id: prepared.request.intent().installation_id.clone(),
            operation: prepared.request.intent().kind,
            phase: OperationPhase::Preflight,
            transaction_root: roots.transaction.clone(),
            staging_root: roots.staging.clone(),
            backup_root: roots.backup.clone(),
            recovery_generation_id: identity.recovery_generation_id.clone(),
            recovery_chain_sha256,
            manifest_generation_before: prepared
                .previous_manifest
                .as_ref()
                .map_or(0, InstallationManifest::generation),
            manifest_generation_after: prepared.target_manifest.generation(),
            manifest_commit_state: ManifestCommitState::NotStarted,
            started_at_ms: journal_started_at_ms,
            updated_at_ms: journal_started_at_ms,
            mutations: prepared
                .mutations
                .iter()
                .map(|mutation| mutation.mutation.clone())
                .collect(),
            recovery_attempts: 0,
            pinned: false,
        }) {
            Ok(journal) => journal,
            Err(_) => {
                return rejected_invalid(Some(prepared.record), prepared.request.operation_id())
            }
        };
        let journal_created = if let Some(profile) = &prepared.profile_commit {
            self.store.create_profile_switch_journal(
                &prepared.record,
                &prepared.lease,
                &journal,
                prepared.previous_manifest.as_ref(),
                &prepared.target_manifest,
                &prepared.manifest_only_observations,
                &profile.plan_fingerprint,
                profile.previous.as_ref(),
                &profile.target,
            )
        } else {
            self.store.create_journal(
                &prepared.record,
                &prepared.lease,
                &journal,
                prepared.previous_manifest.as_ref(),
                &prepared.target_manifest,
                &prepared.manifest_only_observations,
                clock.now_ms(),
            )
        };
        if let Err(error) = journal_created {
            return self.recovery_required_from_store(prepared.record, None, error);
        }
        let rollback_on_failure = matches!(
            prepared.request.intent().kind,
            LifecycleOperationKind::Update
                | LifecycleOperationKind::Repair
                | LifecycleOperationKind::ProfileSwitch
        );
        let rollback_lease = prepared.lease.clone();
        let rollback_preflight = prepared.preflight.clone();
        match self.run_forward(prepared, identity, roots, journal, workspace) {
            Ok((operation, journal, preflight)) => LifecycleOutcome::Succeeded {
                operation: Box::new(operation),
                journal: Some(Box::new(journal)),
                preflight: Box::new(preflight),
            },
            Err(failure) => {
                let (record, journal, failure) = *failure;
                if rollback_on_failure {
                    if let Some(outcome) = self.try_immediate_failed_rollback(
                        &record,
                        &rollback_lease,
                        &journal,
                        &failure,
                        rollback_preflight,
                        workspace,
                    ) {
                        return outcome;
                    }
                }
                match failure {
                    ExecutionFailure::Store(error) => {
                        self.recovery_required_from_store(record, Some(journal), error)
                    }
                    ExecutionFailure::Filesystem(error) => {
                        self.recovery_required_from_filesystem(record, Some(journal), error)
                    }
                }
            }
        }
    }

    fn try_immediate_failed_rollback<W>(
        &mut self,
        failed_record: &OperationRecord,
        lease: &OperationLease,
        failed_journal: &LifecycleJournal,
        failure: &ExecutionFailure,
        preflight: PreflightReport,
        workspace: &mut W,
    ) -> Option<LifecycleOutcome>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let ExecutionFailure::Filesystem(boundary_error) = failure else {
            return None;
        };
        let record = self
            .store
            .operation_by_id(failed_record.request.operation_id())
            .ok()
            .flatten()?;
        let journal = self
            .store
            .journal_by_operation(failed_journal.operation_id.as_str())
            .ok()
            .flatten()?;
        if record.state != OperationState::Running
            || journal.recovery_disposition() != JournalDisposition::RollBack
        {
            return None;
        }
        let code = filesystem_error_code(boundary_error);
        let (retryable, recovery_action) = match code {
            ProductErrorCode::ExternalModification | ProductErrorCode::PathEscapedRoot => {
                (false, RecoveryAction::ResolveConflict)
            }
            ProductErrorCode::LostOperationLease | ProductErrorCode::StaleOperationRevision => {
                return None
            }
            _ => (true, RecoveryAction::Retry),
        };
        let error = product_error(
            code,
            "lifecycle.mutation_rolled_back",
            Some(record.request.operation_id()),
            Some(record.phase),
            retryable,
            recovery_action,
        );
        let (rollback_record, rollback_journal) = self
            .store
            .begin_failed_rollback(&record, lease, &journal, error.clone())
            .ok()?;
        let terminalization = Terminalization {
            state: OperationState::Failed,
            error: Some(error.clone()),
            result_fingerprint: None,
            now_ms: self.store.now_ms(),
        };
        let (operation, _journal) = self
            .resume_recovery(
                rollback_record,
                lease.clone(),
                rollback_journal,
                JournalDisposition::RollBack,
                terminalization,
                workspace,
            )
            .ok()?;
        Some(LifecycleOutcome::Rejected {
            operation: Some(Box::new(operation)),
            error: Box::new(error),
            conflict: None,
            preflight: Some(Box::new(preflight)),
        })
    }

    fn run_forward<W>(
        &mut self,
        prepared: PreparedOperation,
        identity: &ExecutionIdentity,
        roots: WorkspaceRoots,
        mut journal: LifecycleJournal,
        workspace: &mut W,
    ) -> ForwardResult
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let clock = self.store.clock();
        let mut record = prepared.record;
        macro_rules! attempt {
            ($expression:expr) => {
                match $expression {
                    Ok(value) => value,
                    Err(error) => return Err(Box::new((record, journal, error.into()))),
                }
            };
        }
        (record, journal) = attempt!(self.store.transition_phase(
            &record,
            &prepared.lease,
            &journal,
            OperationPhase::Staging,
            clock.now_ms(),
        ));
        for prepared_mutation in &prepared.mutations {
            if prepared_mutation.mutation.action == MutationAction::Delete {
                continue;
            }
            let index = prepared_mutation.mutation.index as usize;
            let staging_path = attempt!(prepared_mutation
                .mutation
                .staging_path
                .as_ref()
                .ok_or(StoreError::InvalidTransition));
            let expected = attempt!(prepared_mutation
                .mutation
                .expected_sha256
                .as_deref()
                .ok_or(StoreError::InvalidTransition));
            let source = attempt!(prepared_mutation
                .source
                .as_ref()
                .ok_or(StoreError::InvalidTransition));
            attempt!(self.store.check_fault(FaultPoint::BeforeStagingEffect {
                index: prepared_mutation.mutation.index,
            }));
            let staged =
                attempt!(workspace.stage_file(&roots.staging, staging_path, source, expected,));
            attempt!(validate_regular_snapshot(
                &staged,
                &roots.staging,
                staging_path,
                None,
                expected,
            ));
            let mut payload = journal.clone().into_payload();
            payload.journal_sequence += 1;
            payload.mutations[index].checkpoint = MutationCheckpoint::Staged;
            payload.mutations[index].staging_sha256 = Some(expected.to_owned());
            let next = attempt!(LifecycleJournal::new(payload).map_err(StoreError::from));
            attempt!(self.store.checkpoint_mutation(
                &record,
                &prepared.lease,
                &journal,
                &next,
                index,
                clock.now_ms(),
            ));
            journal = next;
        }
        (record, journal) = attempt!(self.store.transition_phase(
            &record,
            &prepared.lease,
            &journal,
            OperationPhase::BackingUp,
            clock.now_ms(),
        ));
        for prepared_mutation in &prepared.mutations {
            if prepared_mutation.mutation.action == MutationAction::Create {
                continue;
            }
            let index = prepared_mutation.mutation.index as usize;
            let backup_path = attempt!(prepared_mutation
                .mutation
                .backup_path
                .as_ref()
                .ok_or(StoreError::InvalidTransition));
            let expected = attempt!(prepared_mutation
                .mutation
                .previous_sha256
                .as_deref()
                .ok_or(StoreError::InvalidTransition));
            attempt!(self.store.check_fault(FaultPoint::BeforeBackupEffect {
                index: prepared_mutation.mutation.index,
            }));
            let backup = attempt!(workspace.backup_file(
                &roots.transaction,
                &prepared_mutation.mutation.path,
                &roots.backup,
                backup_path,
                expected,
            ));
            attempt!(validate_regular_snapshot(
                &backup,
                &roots.backup,
                backup_path,
                None,
                expected,
            ));
            let mut payload = journal.clone().into_payload();
            payload.journal_sequence += 1;
            payload.mutations[index].checkpoint = MutationCheckpoint::BackupVerified;
            payload.mutations[index].backup_sha256 = Some(expected.to_owned());
            let next = attempt!(LifecycleJournal::new(payload).map_err(StoreError::from));
            attempt!(self.store.checkpoint_mutation(
                &record,
                &prepared.lease,
                &journal,
                &next,
                index,
                clock.now_ms(),
            ));
            journal = next;
        }
        (record, journal) = attempt!(self.store.transition_phase(
            &record,
            &prepared.lease,
            &journal,
            OperationPhase::Applying,
            clock.now_ms(),
        ));
        for index in 0..journal.mutations.len() {
            let next = attempt!(journal_with_checkpoint(
                &journal,
                index,
                MutationCheckpoint::Applied,
                clock.now_ms(),
            ));
            let effect = match journal.mutations[index].action {
                MutationAction::Create | MutationAction::Replace => MutationSideEffect::Publish,
                MutationAction::Delete => MutationSideEffect::Remove,
            };
            attempt!(self.store.check_fault(FaultPoint::BeforeFilesystemEffect {
                index: journal.mutations[index].index,
                effect,
            }));
            let transition = attempt!(ValidatedMutationTransition::new(
                &journal,
                &next,
                &record,
                &prepared.lease,
                clock.now_ms(),
                index,
                effect,
            )
            .map_err(StoreError::from));
            let receipt = attempt!(apply_forward_effect(
                &mut self.store,
                workspace,
                &prepared.lease,
                &record,
                &journal,
                &transition,
                clock.now_ms(),
            ));
            attempt!(validate_receipt(&receipt, &next, index));
            journal = next;
        }
        (record, journal) = attempt!(self.store.transition_phase(
            &record,
            &prepared.lease,
            &journal,
            OperationPhase::Verifying,
            clock.now_ms(),
        ));
        for index in 0..journal.mutations.len() {
            let mutation = &journal.mutations[index];
            let observation = attempt!(
                workspace.observe_preflight_no_follow(&roots.transaction, &mutation.path,)
            );
            attempt!(validate_output_observation(
                mutation,
                &roots.transaction,
                &observation,
            ));
            let next = attempt!(journal_with_checkpoint(
                &journal,
                index,
                MutationCheckpoint::OutputVerified,
                clock.now_ms(),
            ));
            attempt!(self.store.checkpoint_mutation(
                &record,
                &prepared.lease,
                &journal,
                &next,
                index,
                clock.now_ms(),
            ));
            journal = next;
        }
        (record, journal) = attempt!(self.store.transition_phase(
            &record,
            &prepared.lease,
            &journal,
            OperationPhase::Committing,
            clock.now_ms(),
        ));
        attempt!(revalidate_publication_claims::<W>(
            workspace,
            &journal,
            &prepared.manifest_only_observations,
        ));
        journal = attempt!(self.store.write_manifest_temporary(
            &record,
            &prepared.lease,
            &journal,
            clock.now_ms(),
        ));
        attempt!(revalidate_publication_claims::<W>(
            workspace,
            &journal,
            &prepared.manifest_only_observations,
        ));
        let publication_journal = journal.clone();
        let publication_observations = &prepared.manifest_only_observations;
        journal = attempt!(self.store.publish_manifest(
            &record,
            &prepared.lease,
            &journal,
            clock.now_ms(),
            || {
                revalidate_publication_claims::<W>(
                    workspace,
                    &publication_journal,
                    publication_observations,
                )
            },
        ));
        (record, journal) = attempt!(self.store.transition_phase(
            &record,
            &prepared.lease,
            &journal,
            OperationPhase::CleaningUp,
            clock.now_ms(),
        ));
        attempt!(self.store.check_fault(FaultPoint::BeforeCleanup));
        attempt!(self.store.assert_lease_current(&record, &prepared.lease));
        attempt!(workspace.cleanup_workspace(
            &roots,
            prepared.request.operation_id(),
            &identity.recovery_generation_id,
            true,
        ));
        attempt!(self.store.assert_lease_current(&record, &prepared.lease));
        attempt!(self.store.check_fault(FaultPoint::AfterCleanup));
        attempt!(self.store.assert_lease_current(&record, &prepared.lease));
        let result_fingerprint = manifest_fingerprint(&prepared.target_manifest);
        let (terminal_record, terminal_journal) = attempt!(self.store.terminalize(
            &record,
            &prepared.lease,
            &journal,
            Terminalization {
                state: OperationState::Succeeded,
                error: None,
                result_fingerprint: Some(result_fingerprint),
                now_ms: clock.now_ms(),
            },
        ));
        Ok((terminal_record, terminal_journal, prepared.preflight))
    }

    fn finish_preflight_failure(
        &mut self,
        record: OperationRecord,
        lease: OperationLease,
        failure: PreflightFailure,
        now_ms: u64,
    ) -> LifecycleOutcome {
        match self
            .store
            .fail_without_journal(&record, &lease, (*failure.error).clone(), now_ms)
        {
            Ok(operation) => LifecycleOutcome::Rejected {
                operation: Some(Box::new(operation)),
                error: failure.error,
                conflict: failure.conflict,
                preflight: failure.report,
            },
            Err(error) => self.recovery_required_from_store(record, None, error),
        }
    }

    fn recovery_required_from_store(
        &self,
        operation: OperationRecord,
        journal: Option<LifecycleJournal>,
        error: StoreError,
    ) -> LifecycleOutcome {
        let operation = self
            .store
            .operation_by_id(operation.request.operation_id())
            .ok()
            .flatten()
            .unwrap_or(operation);
        if operation.state.terminal() {
            return LifecycleOutcome::Existing {
                operation: Box::new(operation),
            };
        }
        let journal = self
            .store
            .journal_by_operation(operation.request.operation_id())
            .ok()
            .flatten()
            .or(journal);
        LifecycleOutcome::RecoveryRequired {
            error: Box::new(product_error(
                store_error_code(&error),
                "lifecycle.recovery_required",
                Some(operation.request.operation_id()),
                Some(operation.phase),
                true,
                RecoveryAction::Recover,
            )),
            operation: Box::new(operation),
            journal: journal.map(Box::new),
        }
    }

    fn recovery_required_from_filesystem(
        &self,
        operation: OperationRecord,
        journal: Option<LifecycleJournal>,
        error: FilesystemBoundaryError,
    ) -> LifecycleOutcome {
        let operation = self
            .store
            .operation_by_id(operation.request.operation_id())
            .ok()
            .flatten()
            .unwrap_or(operation);
        if operation.state.terminal() {
            return LifecycleOutcome::Existing {
                operation: Box::new(operation),
            };
        }
        let journal = self
            .store
            .journal_by_operation(operation.request.operation_id())
            .ok()
            .flatten()
            .or(journal);
        LifecycleOutcome::RecoveryRequired {
            error: Box::new(product_error(
                filesystem_error_code(&error),
                "lifecycle.recovery_required",
                Some(operation.request.operation_id()),
                Some(operation.phase),
                true,
                RecoveryAction::Recover,
            )),
            operation: Box::new(operation),
            journal: journal.map(Box::new),
        }
    }

    pub fn recover_startup<W, F>(
        &mut self,
        owner_instance_id: &str,
        now_ms: u64,
        lease_ttl_ms: u64,
        next_lease_id: F,
        workspace: &mut W,
    ) -> Vec<StartupRecoveryOutcome>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
        F: FnMut(&OperationRecord) -> String,
    {
        self.recover_startup_scoped(
            owner_instance_id,
            None,
            now_ms,
            lease_ttl_ms,
            next_lease_id,
            workspace,
        )
    }

    pub fn recover_startup_installation<W, F>(
        &mut self,
        owner_instance_id: &str,
        installation_id: &str,
        now_ms: u64,
        lease_ttl_ms: u64,
        next_lease_id: F,
        workspace: &mut W,
    ) -> Vec<StartupRecoveryOutcome>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
        F: FnMut(&OperationRecord) -> String,
    {
        self.recover_startup_scoped(
            owner_instance_id,
            Some(installation_id),
            now_ms,
            lease_ttl_ms,
            next_lease_id,
            workspace,
        )
    }

    fn recover_startup_scoped<W, F>(
        &mut self,
        owner_instance_id: &str,
        installation_id: Option<&str>,
        _now_ms: u64,
        lease_ttl_ms: u64,
        mut next_lease_id: F,
        workspace: &mut W,
    ) -> Vec<StartupRecoveryOutcome>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
        F: FnMut(&OperationRecord) -> String,
    {
        let clock = self.store.clock();
        let interrupted = match self.store.interrupted_operations() {
            Ok(interrupted) => interrupted,
            Err(_) => {
                return vec![StartupRecoveryOutcome::StoreBlocked {
                    error: Box::new(product_error(
                        ProductErrorCode::RecoveryUnavailable,
                        "lifecycle.recovery_unavailable",
                        None,
                        None,
                        false,
                        RecoveryAction::NoAction,
                    )),
                }]
            }
        };
        let mut outcomes = Vec::with_capacity(interrupted.len());
        for interrupted in interrupted {
            if installation_id.is_some_and(|installation_id| {
                interrupted.record.request.intent().installation_id != installation_id
            }) {
                continue;
            }
            if interrupted.lease.active_at(clock.now_ms()) {
                outcomes.push(StartupRecoveryOutcome::Active {
                    operation: Box::new(interrupted.record),
                    expires_at_ms: interrupted.lease.expires_at_ms,
                });
                continue;
            }
            let Some(stalled_journal) = interrupted.journal else {
                match self
                    .store
                    .recover_abandoned_preflight(&interrupted, clock.now_ms())
                {
                    Ok(operation) => outcomes.push(StartupRecoveryOutcome::Recovered {
                        operation: Box::new(operation),
                        journal: None,
                        disposition: JournalDisposition::RollBack,
                    }),
                    Err(_) => outcomes.push(StartupRecoveryOutcome::Blocked {
                        operation: Box::new(interrupted.record),
                        journal: None,
                        error: Box::new(product_error(
                            ProductErrorCode::RecoveryUnavailable,
                            "lifecycle.recovery_unavailable",
                            Some(interrupted.lease.operation_id.as_str()),
                            Some(OperationPhase::Preflight),
                            false,
                            RecoveryAction::NoAction,
                        )),
                    }),
                }
                continue;
            };
            let disposition = stalled_journal.recovery_disposition();
            let lease_id = next_lease_id(&interrupted.record);
            let now_ms = clock.now_ms();
            let Some(expires_at_ms) = now_ms.checked_add(lease_ttl_ms) else {
                outcomes.push(recovery_blocked(
                    interrupted.record,
                    Some(stalled_journal),
                    ProductErrorCode::InvalidRequest,
                ));
                continue;
            };
            let recovery_lease = OperationLease {
                installation_id: interrupted.lease.installation_id.clone(),
                operation_id: interrupted.lease.operation_id.clone(),
                lease_id,
                owner_instance_id: owner_instance_id.to_owned(),
                fencing_token: interrupted.lease.fencing_token,
                acquired_at_ms: now_ms,
                expires_at_ms,
            };
            let recovery_phase = if stalled_journal.phase == OperationPhase::CleaningUp {
                OperationPhase::CleaningUp
            } else if disposition == JournalDisposition::FinalizeVerifiedCommit {
                OperationPhase::Committing
            } else {
                OperationPhase::RollingBack
            };
            let mut record_payload = interrupted.record.clone().into_payload();
            record_payload.state = OperationState::RecoveryRequired;
            record_payload.phase = recovery_phase;
            record_payload.updated_at_ms = now_ms.max(interrupted.record.updated_at_ms);
            record_payload.result_fingerprint = None;
            record_payload.error = Some(product_error(
                ProductErrorCode::RecoveryRequired,
                "lifecycle.recovery_required",
                Some(interrupted.record.request.operation_id()),
                Some(recovery_phase),
                true,
                RecoveryAction::Recover,
            ));
            let recovery_record = match OperationRecord::new(record_payload) {
                Ok(record) => record,
                Err(_) => {
                    outcomes.push(recovery_blocked(
                        interrupted.record,
                        Some(stalled_journal),
                        ProductErrorCode::RecoveryUnavailable,
                    ));
                    continue;
                }
            };
            let mut journal_payload = stalled_journal.clone().into_payload();
            journal_payload.lease_id = recovery_lease.lease_id.clone();
            journal_payload.recovery_chain_sha256 =
                match stalled_journal.next_recovery_chain_sha256(&recovery_lease.lease_id) {
                    Ok(hash) => hash,
                    Err(_) => {
                        outcomes.push(recovery_blocked(
                            interrupted.record,
                            Some(stalled_journal),
                            ProductErrorCode::RecoveryUnavailable,
                        ));
                        continue;
                    }
                };
            journal_payload.phase = recovery_phase;
            journal_payload.updated_at_ms = now_ms.max(stalled_journal.updated_at_ms);
            journal_payload.recovery_attempts = stalled_journal.recovery_attempts.saturating_add(1);
            let recovery_journal = match LifecycleJournal::new(journal_payload) {
                Ok(journal) => journal,
                Err(_) => {
                    outcomes.push(recovery_blocked(
                        interrupted.record,
                        Some(stalled_journal),
                        ProductErrorCode::RecoveryUnavailable,
                    ));
                    continue;
                }
            };
            let rebind = match ValidatedRecoveryRebind::new(
                &stalled_journal,
                &interrupted.record,
                &recovery_journal,
                &recovery_record,
                &recovery_lease,
                clock.now_ms(),
            ) {
                Ok(rebind) => rebind,
                Err(_) => {
                    outcomes.push(recovery_blocked(
                        interrupted.record,
                        Some(stalled_journal),
                        ProductErrorCode::RecoveryUnavailable,
                    ));
                    continue;
                }
            };
            match self.store.rebind_and_lock_recovery(&rebind, clock.now_ms()) {
                Ok(guard) => drop(guard),
                Err(_) => {
                    outcomes.push(recovery_blocked(
                        interrupted.record,
                        Some(stalled_journal),
                        ProductErrorCode::RecoveryUnavailable,
                    ));
                    continue;
                }
            }
            let terminalization = Terminalization {
                state: OperationState::Recovered,
                error: None,
                result_fingerprint: None,
                now_ms: clock.now_ms(),
            };
            outcomes.push(
                match self.resume_recovery(
                    recovery_record,
                    recovery_lease,
                    recovery_journal,
                    disposition,
                    terminalization,
                    workspace,
                ) {
                    Ok((operation, journal)) => StartupRecoveryOutcome::Recovered {
                        operation: Box::new(operation),
                        journal: Some(Box::new(journal)),
                        disposition,
                    },
                    Err(failure) => {
                        let (operation, journal, code) = *failure;
                        recovery_blocked(operation, Some(journal), code)
                    }
                },
            );
        }
        outcomes
    }

    fn resume_recovery<W>(
        &mut self,
        mut record: OperationRecord,
        lease: OperationLease,
        mut journal: LifecycleJournal,
        _disposition: JournalDisposition,
        mut terminalization: Terminalization,
        workspace: &mut W,
    ) -> RecoveryResumeResult
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let clock = self.store.clock();
        let roots = WorkspaceRoots {
            transaction: journal.transaction_root.clone(),
            staging: journal.staging_root.clone(),
            backup: journal.backup_root.clone(),
        };
        let result = (|| -> Result<(OperationRecord, LifecycleJournal), ExecutionFailure> {
            let manifest_only_observations = self
                .store
                .generation_manifest_only_observations(&journal.recovery_generation_id)?;
            if journal.phase == OperationPhase::Committing {
                if journal.manifest_commit_state == ManifestCommitState::NotStarted {
                    revalidate_publication_claims::<W>(
                        workspace,
                        &journal,
                        &manifest_only_observations,
                    )?;
                    journal = self.store.write_manifest_temporary(
                        &record,
                        &lease,
                        &journal,
                        clock.now_ms(),
                    )?;
                }
                if journal.manifest_commit_state == ManifestCommitState::TemporaryWritten {
                    revalidate_publication_claims::<W>(
                        workspace,
                        &journal,
                        &manifest_only_observations,
                    )?;
                    let publication_journal = journal.clone();
                    journal = self.store.publish_manifest(
                        &record,
                        &lease,
                        &journal,
                        clock.now_ms(),
                        || {
                            revalidate_publication_claims::<W>(
                                workspace,
                                &publication_journal,
                                &manifest_only_observations,
                            )
                        },
                    )?;
                }
                (record, journal) = self.store.transition_phase(
                    &record,
                    &lease,
                    &journal,
                    OperationPhase::CleaningUp,
                    clock.now_ms(),
                )?;
            } else if journal.phase == OperationPhase::RollingBack {
                let mut rollback_order: Vec<_> = (0..journal.mutations.len()).collect();
                if journal.operation == LifecycleOperationKind::ProfileSwitch {
                    rollback_order.reverse();
                }
                for index in rollback_order {
                    journal = self.recover_mutation(
                        &record,
                        &lease,
                        &journal,
                        index,
                        clock.now_ms(),
                        workspace,
                    )?;
                }
                (record, journal) = self.store.transition_phase(
                    &record,
                    &lease,
                    &journal,
                    OperationPhase::CleaningUp,
                    clock.now_ms(),
                )?;
            }
            if journal.phase != OperationPhase::CleaningUp {
                return Err(StoreError::InvalidTransition.into());
            }
            let committed = journal.manifest_commit_state == ManifestCommitState::Published;
            self.store.check_fault(FaultPoint::BeforeCleanup)?;
            self.store.assert_lease_current(&record, &lease)?;
            workspace.cleanup_workspace(
                &roots,
                &journal.operation_id,
                &journal.recovery_generation_id,
                committed,
            )?;
            self.store.assert_lease_current(&record, &lease)?;
            self.store.check_fault(FaultPoint::AfterCleanup)?;
            self.store.assert_lease_current(&record, &lease)?;
            let result_fingerprint = committed
                .then(|| self.store.manifest(&journal.installation_id))
                .transpose()?
                .flatten()
                .map(|manifest| manifest_fingerprint(&manifest));
            terminalization.result_fingerprint = result_fingerprint;
            terminalization.now_ms = clock.now_ms();
            self.store
                .terminalize(&record, &lease, &journal, terminalization)
                .map_err(ExecutionFailure::from)
        })();
        result.map_err(|failure| {
            let code = match failure {
                ExecutionFailure::Store(error) => store_error_code(&error),
                ExecutionFailure::Filesystem(error) => filesystem_error_code(&error),
            };
            Box::new((record, journal, code))
        })
    }

    fn recover_mutation<W>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        index: usize,
        now_ms: u64,
        workspace: &mut W,
    ) -> Result<LifecycleJournal, ExecutionFailure>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let mutation = &journal.mutations[index];
        match mutation.checkpoint {
            MutationCheckpoint::NoEffect | MutationCheckpoint::RolledBack => Ok(journal.clone()),
            MutationCheckpoint::Planned => self.reconcile(
                record,
                lease,
                journal,
                index,
                ReconciliationScope::PreEffect,
                ReconciliationOutcome::EffectNotApplied,
                now_ms,
                workspace,
            ),
            MutationCheckpoint::Staged if mutation.action == MutationAction::Replace => self
                .reconcile(
                    record,
                    lease,
                    journal,
                    index,
                    ReconciliationScope::PreEffect,
                    ReconciliationOutcome::EffectNotApplied,
                    now_ms,
                    workspace,
                ),
            MutationCheckpoint::Staged | MutationCheckpoint::BackupVerified => {
                let observation = workspace
                    .observe_preflight_no_follow(&journal.transaction_root, &mutation.path)?;
                let outcome = forward_reconciliation_outcome(mutation, &observation)?;
                let reconciled = self.reconcile(
                    record,
                    lease,
                    journal,
                    index,
                    ReconciliationScope::ForwardMutation,
                    outcome,
                    now_ms,
                    workspace,
                )?;
                if outcome == ReconciliationOutcome::EffectApplied {
                    self.rollback_applied(record, lease, &reconciled, index, now_ms, workspace)
                } else {
                    Ok(reconciled)
                }
            }
            MutationCheckpoint::Applied | MutationCheckpoint::OutputVerified => {
                let observation = workspace
                    .observe_preflight_no_follow(&journal.transaction_root, &mutation.path)?;
                if rollback_effect_landed(mutation, &observation)? {
                    self.reconcile(
                        record,
                        lease,
                        journal,
                        index,
                        ReconciliationScope::Rollback,
                        ReconciliationOutcome::EffectApplied,
                        now_ms,
                        workspace,
                    )
                } else {
                    self.rollback_applied(record, lease, journal, index, now_ms, workspace)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reconcile<W>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        index: usize,
        scope: ReconciliationScope,
        outcome: ReconciliationOutcome,
        now_ms: u64,
        workspace: &mut W,
    ) -> Result<LifecycleJournal, ExecutionFailure>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let checkpoint = match (scope, outcome) {
            (ReconciliationScope::ForwardMutation, ReconciliationOutcome::EffectApplied) => {
                MutationCheckpoint::Applied
            }
            (ReconciliationScope::PreEffect, _) => MutationCheckpoint::NoEffect,
            _ => MutationCheckpoint::RolledBack,
        };
        let next = journal_with_checkpoint(journal, index, checkpoint, now_ms)?;
        let reconciliation = ValidatedMutationReconciliation::new(
            journal,
            &next,
            record,
            lease,
            now_ms,
            ReconciliationDecision::new(index, scope, outcome),
        )?;
        let mut guard = self.store.lock_mutation(lease, record, journal, now_ms)?;
        let destination =
            <W as LifecycleFilesystemBoundary<DurableMutationGuard<'_>>>::observe_no_follow(
                workspace,
                &journal.transaction_root,
                &journal.mutations[index].path,
            )?;
        <W as LifecycleFilesystemBoundary<DurableMutationGuard<'_>>>::reconcile_uncertain_effect(
            workspace,
            &mut guard,
            now_ms,
            &destination,
            &reconciliation,
        )?;
        Ok(next)
    }

    fn rollback_applied<W>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        index: usize,
        now_ms: u64,
        workspace: &mut W,
    ) -> Result<LifecycleJournal, ExecutionFailure>
    where
        W: LifecycleWorkspace,
        for<'guard> W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
    {
        let mutation = &journal.mutations[index];
        let effect = if mutation.action == MutationAction::Create {
            MutationSideEffect::Remove
        } else {
            MutationSideEffect::RestoreBackup
        };
        self.store.check_fault(FaultPoint::BeforeFilesystemEffect {
            index: mutation.index,
            effect,
        })?;
        let next = journal_with_checkpoint(journal, index, MutationCheckpoint::RolledBack, now_ms)?;
        let transition =
            ValidatedMutationTransition::new(journal, &next, record, lease, now_ms, index, effect)?;
        let mut guard = self.store.lock_mutation(lease, record, journal, now_ms)?;
        let destination =
            <W as LifecycleFilesystemBoundary<DurableMutationGuard<'_>>>::observe_no_follow(
                workspace,
                &journal.transaction_root,
                &mutation.path,
            )?;
        if mutation.action == MutationAction::Create {
            <W as LifecycleFilesystemBoundary<DurableMutationGuard<'_>>>::remove_verified(
                workspace,
                &mut guard,
                now_ms,
                &destination,
                &transition,
            )?;
        } else {
            let backup_path = mutation
                .backup_path
                .as_ref()
                .ok_or(FilesystemBoundaryError::VerificationFailed)?;
            let backup =
                <W as LifecycleFilesystemBoundary<DurableMutationGuard<'_>>>::observe_no_follow(
                    workspace,
                    &journal.backup_root,
                    backup_path,
                )?;
            <W as LifecycleFilesystemBoundary<DurableMutationGuard<'_>>>::restore_verified_backup(
                workspace,
                &mut guard,
                now_ms,
                &backup,
                &destination,
                &transition,
            )?;
        }
        Ok(next)
    }
}

fn apply_forward_effect<'guard, W>(
    store: &'guard mut DurableLifecycleStore,
    workspace: &mut W,
    lease: &OperationLease,
    record: &OperationRecord,
    journal: &LifecycleJournal,
    transition: &ValidatedMutationTransition,
    now_ms: u64,
) -> Result<PublicationReceipt, ExecutionFailure>
where
    W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
{
    let mutation = transition.mutation();
    let mut guard = store.lock_mutation(lease, record, journal, now_ms)?;
    let destination = workspace.observe_no_follow(&journal.transaction_root, &mutation.path)?;
    match mutation.action {
        MutationAction::Create | MutationAction::Replace => {
            let staging_path = mutation
                .staging_path
                .as_ref()
                .ok_or(FilesystemBoundaryError::VerificationFailed)?;
            let staged = workspace.observe_no_follow(&journal.staging_root, staging_path)?;
            workspace
                .publish_verified(&mut guard, now_ms, &staged, &destination, transition)
                .map_err(ExecutionFailure::from)
        }
        MutationAction::Delete => workspace
            .remove_verified(&mut guard, now_ms, &destination, transition)
            .map_err(ExecutionFailure::from),
    }
}

fn validate_receipt(
    receipt: &PublicationReceipt,
    next: &LifecycleJournal,
    index: usize,
) -> Result<(), FilesystemBoundaryError> {
    let mutation = &next.mutations[index];
    let expected_hash = match mutation.action {
        MutationAction::Create | MutationAction::Replace => mutation.expected_sha256.as_deref(),
        MutationAction::Delete => None,
    };
    if receipt.root_identity != next.transaction_root
        || receipt.path != mutation.path
        || receipt.path_identity_key != mutation.path_identity_key
        || receipt.sha256.as_deref() != expected_hash
        || receipt.operation_id != next.operation_id
        || receipt.lease_id != next.lease_id
        || receipt.fencing_token != next.fencing_token
        || receipt.operation_revision != next.operation_revision
        || receipt.journal_sequence != next.journal_sequence
    {
        Err(FilesystemBoundaryError::VerificationFailed)
    } else {
        Ok(())
    }
}

fn journal_with_checkpoint(
    journal: &LifecycleJournal,
    index: usize,
    checkpoint: MutationCheckpoint,
    now_ms: u64,
) -> Result<LifecycleJournal, StoreError> {
    let mut payload = journal.clone().into_payload();
    payload.journal_sequence = payload
        .journal_sequence
        .checked_add(1)
        .ok_or(StoreError::SequenceExhausted)?;
    payload.updated_at_ms = now_ms.max(payload.updated_at_ms);
    payload
        .mutations
        .get_mut(index)
        .ok_or(StoreError::InvalidTransition)?
        .checkpoint = checkpoint;
    LifecycleJournal::new(payload).map_err(StoreError::from)
}

fn validate_preflight_observation(
    observation: &ObservationSnapshot,
    root: &RootIdentity,
    path: &ValidatedRelativePath,
    identity_key: &str,
) -> Result<(), FilesystemBoundaryError> {
    observation
        .validate()
        .map_err(|_| FilesystemBoundaryError::ObservationChanged)?;
    if &observation.root_identity != root {
        return Err(FilesystemBoundaryError::RootIdentityChanged);
    }
    if &observation.path != path {
        return Err(FilesystemBoundaryError::ObservationChanged);
    }
    if observation.path_identity_key != identity_key {
        return Err(FilesystemBoundaryError::UnsafeAlias);
    }
    Ok(())
}

fn validate_regular_snapshot(
    observation: &ObservationSnapshot,
    root: &RootIdentity,
    path: &ValidatedRelativePath,
    identity_key: Option<&str>,
    expected_sha256: &str,
) -> Result<(), FilesystemBoundaryError> {
    validate_preflight_observation(
        observation,
        root,
        path,
        identity_key.unwrap_or(&observation.path_identity_key),
    )?;
    match &observation.state {
        ObservedFileState::Regular {
            sha256,
            link_count: 1,
            ..
        } if sha256.eq_ignore_ascii_case(expected_sha256) => Ok(()),
        ObservedFileState::Regular { link_count, .. } if *link_count != 1 => {
            Err(FilesystemBoundaryError::UnsafeAlias)
        }
        _ => Err(FilesystemBoundaryError::VerificationFailed),
    }
}

fn validate_output_observation(
    mutation: &JournalMutation,
    expected_root: &RootIdentity,
    observation: &ObservationSnapshot,
) -> Result<(), FilesystemBoundaryError> {
    if &observation.root_identity != expected_root
        || observation.path != mutation.path
        || observation.path_identity_key != mutation.path_identity_key
    {
        return Err(FilesystemBoundaryError::ObservationChanged);
    }
    match mutation.action {
        MutationAction::Create | MutationAction::Replace => validate_regular_snapshot(
            observation,
            &observation.root_identity,
            &mutation.path,
            Some(&mutation.path_identity_key),
            mutation
                .expected_sha256
                .as_deref()
                .ok_or(FilesystemBoundaryError::VerificationFailed)?,
        ),
        MutationAction::Delete if observation.state == ObservedFileState::Missing => Ok(()),
        MutationAction::Delete => Err(FilesystemBoundaryError::VerificationFailed),
    }
}

fn revalidate_publication_claims<'guard, W>(
    workspace: &W,
    journal: &LifecycleJournal,
    manifest_only_observations: &[ObservationSnapshot],
) -> Result<(), FilesystemBoundaryError>
where
    W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
{
    revalidate_verified_outputs::<W>(workspace, journal)?;
    revalidate_manifest_only_claims::<W>(
        workspace,
        &journal.transaction_root,
        manifest_only_observations,
    )
}

fn revalidate_verified_outputs<'guard, W>(
    workspace: &W,
    journal: &LifecycleJournal,
) -> Result<(), FilesystemBoundaryError>
where
    W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
{
    for mutation in &journal.mutations {
        if mutation.checkpoint != MutationCheckpoint::OutputVerified {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        let observation = workspace.observe_no_follow(&journal.transaction_root, &mutation.path)?;
        validate_output_observation(mutation, &journal.transaction_root, observation.snapshot())
            .map_err(|error| match error {
                FilesystemBoundaryError::VerificationFailed => {
                    FilesystemBoundaryError::ObservationChanged
                }
                other => other,
            })?;
    }
    Ok(())
}

fn revalidate_manifest_only_claims<'guard, W>(
    workspace: &W,
    transaction_root: &RootIdentity,
    expected_observations: &[ObservationSnapshot],
) -> Result<(), FilesystemBoundaryError>
where
    W: LifecycleFilesystemBoundary<DurableMutationGuard<'guard>>,
{
    if workspace.root_identity()? != *transaction_root {
        return Err(FilesystemBoundaryError::RootIdentityChanged);
    }
    for expected in expected_observations {
        let current = workspace.observe_no_follow(transaction_root, &expected.path)?;
        let snapshot = current.snapshot();
        validate_preflight_observation(
            snapshot,
            transaction_root,
            &expected.path,
            &expected.path_identity_key,
        )?;
        if snapshot.state != expected.state {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
    }
    Ok(())
}

fn require_claim_observation(
    claim: Option<&FileClaim>,
    observation: &ObservationSnapshot,
) -> Result<(), FilesystemBoundaryError> {
    match (claim, &observation.state) {
        (None, ObservedFileState::Missing) => Ok(()),
        (
            Some(claim),
            ObservedFileState::Regular {
                sha256,
                link_count: 1,
                ..
            },
        ) if sha256.eq_ignore_ascii_case(&claim.sha256) => Ok(()),
        (_, ObservedFileState::Regular { link_count, .. }) if *link_count != 1 => {
            Err(FilesystemBoundaryError::UnsafeAlias)
        }
        _ => Err(FilesystemBoundaryError::ObservationChanged),
    }
}

fn forward_reconciliation_outcome(
    mutation: &JournalMutation,
    observation: &ObservationSnapshot,
) -> Result<ReconciliationOutcome, FilesystemBoundaryError> {
    let applied = match mutation.action {
        MutationAction::Create | MutationAction::Replace => state_has_hash(
            &observation.state,
            mutation.expected_sha256.as_deref().unwrap_or_default(),
        ),
        MutationAction::Delete => observation.state == ObservedFileState::Missing,
    };
    if applied {
        return Ok(ReconciliationOutcome::EffectApplied);
    }
    let not_applied = match mutation.action {
        MutationAction::Create => observation.state == ObservedFileState::Missing,
        MutationAction::Replace | MutationAction::Delete => state_has_hash(
            &observation.state,
            mutation.previous_sha256.as_deref().unwrap_or_default(),
        ),
    };
    if not_applied {
        Ok(ReconciliationOutcome::EffectNotApplied)
    } else {
        Err(FilesystemBoundaryError::ObservationChanged)
    }
}

fn rollback_effect_landed(
    mutation: &JournalMutation,
    observation: &ObservationSnapshot,
) -> Result<bool, FilesystemBoundaryError> {
    let landed = match mutation.action {
        MutationAction::Create => observation.state == ObservedFileState::Missing,
        MutationAction::Replace | MutationAction::Delete => state_has_hash(
            &observation.state,
            mutation.previous_sha256.as_deref().unwrap_or_default(),
        ),
    };
    if landed {
        return Ok(true);
    }
    let forward = match mutation.action {
        MutationAction::Create | MutationAction::Replace => state_has_hash(
            &observation.state,
            mutation.expected_sha256.as_deref().unwrap_or_default(),
        ),
        MutationAction::Delete => observation.state == ObservedFileState::Missing,
    };
    if forward {
        Ok(false)
    } else {
        Err(FilesystemBoundaryError::ObservationChanged)
    }
}

fn state_has_hash(state: &ObservedFileState, expected: &str) -> bool {
    matches!(
        state,
        ObservedFileState::Regular {
            sha256,
            link_count: 1,
            ..
        } if sha256.eq_ignore_ascii_case(expected)
    )
}

fn generated_path(prefix: &str, index: u32) -> Result<ValidatedRelativePath, PlanError> {
    ValidatedRelativePath::parse(&format!("{prefix}/{index:08}.bin"))
        .map_err(|_| PlanError::InvalidFilePlan)
}

fn next_manifest_generation(previous: Option<&InstallationManifest>) -> Result<u64, StoreError> {
    previous.map_or(Ok(1), |manifest| {
        manifest
            .generation()
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)
    })
}

fn preflight_from_claim(
    claim: &FileClaim,
    action: PreflightFileAction,
    backup_required: bool,
) -> PreflightFile {
    PreflightFile {
        path: claim.path.clone(),
        path_identity_key: claim.path_identity_key.clone(),
        action,
        previous_sha256: Some(claim.sha256.clone()),
        proposed_sha256: None,
        existing_owners: claim.owners.clone(),
        backup_required,
    }
}

fn manifest_fingerprint(manifest: &InstallationManifest) -> String {
    manifest.fingerprint()
}

fn validate_target_profile_manifest(
    target: &ProfileLockfile,
    manifest: &InstallationManifest,
) -> Result<(), ()> {
    if target.validate().is_err()
        || target.installation_id != manifest.installation_id()
        || target.mods.len() != manifest.records.len()
    {
        return Err(());
    }
    let records: BTreeMap<_, _> = manifest
        .records
        .iter()
        .map(|record| (record.instance_id.as_str(), record))
        .collect();
    for locked in &target.mods {
        let record = records.get(locked.instance_id.as_str()).ok_or(())?;
        if record.mod_id != locked.mod_id
            || record.display_name != locked.display_name
            || record.version != locked.version
            || record.provider != locked.provider
            || record.archive_sha256.as_deref() != Some(locked.archive_sha256.as_str())
            || record.file_plan_fingerprint != locked.file_plan_fingerprint
        {
            return Err(());
        }
    }
    Ok(())
}

fn recovery_source(
    generation: &RecoveryGenerationSnapshot,
    journal: Option<&LifecycleJournal>,
    target: &FileClaim,
) -> Result<StagingSource, PreflightFailure> {
    let backup_path = journal
        .and_then(|journal| {
            journal.mutations.iter().find(|mutation| {
                mutation.path_identity_key == target.path_identity_key
                    && mutation
                        .previous_sha256
                        .as_deref()
                        .is_some_and(|hash| hash.eq_ignore_ascii_case(&target.sha256))
            })
        })
        .and_then(|mutation| mutation.backup_path.clone())
        .ok_or_else(|| PreflightFailure {
            error: Box::new(product_error(
                ProductErrorCode::RecoveryUnavailable,
                "lifecycle.recovery_unavailable",
                Some(generation.operation_id.as_str()),
                Some(OperationPhase::Preflight),
                false,
                RecoveryAction::NoAction,
            )),
            conflict: None,
            report: None,
        })?;
    Ok(StagingSource::RecoveryBackup {
        generation_id: generation.generation_id.clone(),
        backup_path,
    })
}

fn rebase_manifest(
    previous: Option<&InstallationManifest>,
    installation_id: &str,
    generation: u64,
    now_ms: u64,
) -> Result<InstallationManifest, StoreError> {
    let mut records = previous.map_or_else(Vec::new, |manifest| manifest.records.clone());
    for record in &mut records {
        record.try_update(|payload| {
            payload.manifest_generation = generation;
            payload.updated_at_ms = now_ms.max(payload.updated_at_ms);
        })?;
    }
    let claims = previous.map_or_else(Vec::new, |manifest| manifest.ledger.claims.clone());
    let ledger = InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
        installation_id: installation_id.to_owned(),
        manifest_generation: generation,
        updated_at_ms: now_ms,
        claims,
    })?;
    InstallationManifest::new(records, ledger).map_err(StoreError::from)
}

fn product_error(
    code: ProductErrorCode,
    message_key: &str,
    operation_id: Option<&str>,
    phase: Option<OperationPhase>,
    retryable: bool,
    recovery_action: RecoveryAction,
) -> ProductError {
    ProductError::new(ProductErrorPayload {
        code,
        message_key: message_key.to_owned(),
        operation_id: operation_id.map(str::to_owned),
        phase,
        retryable,
        recovery_action,
        safe_details: BTreeMap::new(),
    })
    .expect("runtime product errors are statically valid")
}

fn conflict_code(conflict: &ConflictReport) -> ProductErrorCode {
    if conflict
        .conflicts
        .iter()
        .any(|conflict| conflict.reason == ConflictReason::ExternalModification)
    {
        ProductErrorCode::ExternalModification
    } else {
        ProductErrorCode::ConflictDetected
    }
}

fn store_error_code(error: &StoreError) -> ProductErrorCode {
    match error {
        StoreError::IdempotencyConflict => ProductErrorCode::IdempotencyConflict,
        StoreError::InstallationBusy => ProductErrorCode::InstallationBusy,
        StoreError::LostLease => ProductErrorCode::LostOperationLease,
        StoreError::StaleRevision => ProductErrorCode::StaleOperationRevision,
        StoreError::StaleJournal => ProductErrorCode::StaleOperationRevision,
        StoreError::SequenceExhausted => ProductErrorCode::RecoveryUnavailable,
        StoreError::ManifestClaimChanged(error) => filesystem_error_code(error),
        StoreError::Injected(_) => ProductErrorCode::RecoveryRequired,
        _ => ProductErrorCode::Internal,
    }
}

fn filesystem_error_code(error: &FilesystemBoundaryError) -> ProductErrorCode {
    match error {
        FilesystemBoundaryError::RootIdentityChanged | FilesystemBoundaryError::UnsafeAlias => {
            ProductErrorCode::PathEscapedRoot
        }
        FilesystemBoundaryError::ObservationChanged => ProductErrorCode::ExternalModification,
        FilesystemBoundaryError::LostLease => ProductErrorCode::LostOperationLease,
        FilesystemBoundaryError::StaleJournal => ProductErrorCode::StaleOperationRevision,
        FilesystemBoundaryError::VerificationFailed => ProductErrorCode::VerificationFailed,
        FilesystemBoundaryError::Io => ProductErrorCode::Internal,
    }
}

fn preflight_invalid(
    request: &OperationRequest,
    report: Option<PreflightReport>,
) -> PreflightFailure {
    PreflightFailure {
        error: Box::new(product_error(
            ProductErrorCode::InvalidRequest,
            "lifecycle.invalid_request",
            Some(request.operation_id()),
            Some(OperationPhase::Preflight),
            false,
            RecoveryAction::NoAction,
        )),
        conflict: None,
        report: report.map(Box::new),
    }
}

fn preflight_internal(
    request: &OperationRequest,
    report: Option<PreflightReport>,
) -> PreflightFailure {
    PreflightFailure {
        error: Box::new(product_error(
            ProductErrorCode::Internal,
            "lifecycle.preflight_failed",
            Some(request.operation_id()),
            Some(OperationPhase::Preflight),
            false,
            RecoveryAction::NoAction,
        )),
        conflict: None,
        report: report.map(Box::new),
    }
}

fn preflight_maintenance_changed(
    request: &OperationRequest,
    report: Option<PreflightReport>,
) -> PreflightFailure {
    PreflightFailure {
        error: Box::new(product_error(
            ProductErrorCode::ExternalModification,
            "lifecycle.maintenance_state_changed",
            Some(request.operation_id()),
            Some(OperationPhase::Preflight),
            true,
            RecoveryAction::Retry,
        )),
        conflict: None,
        report: report.map(Box::new),
    }
}

fn preflight_filesystem_error(
    request: &OperationRequest,
    error: FilesystemBoundaryError,
    report: Option<PreflightReport>,
) -> PreflightFailure {
    PreflightFailure {
        error: Box::new(product_error(
            filesystem_error_code(&error),
            "lifecycle.preflight_blocked",
            Some(request.operation_id()),
            Some(OperationPhase::Preflight),
            false,
            RecoveryAction::ResolveConflict,
        )),
        conflict: None,
        report: report.map(Box::new),
    }
}

fn rejected_invalid(operation: Option<OperationRecord>, operation_id: &str) -> LifecycleOutcome {
    LifecycleOutcome::Rejected {
        operation: operation.map(Box::new),
        error: Box::new(product_error(
            ProductErrorCode::InvalidRequest,
            "lifecycle.invalid_request",
            Some(operation_id),
            Some(OperationPhase::Preflight),
            false,
            RecoveryAction::NoAction,
        )),
        conflict: None,
        preflight: None,
    }
}

fn recovery_blocked(
    operation: OperationRecord,
    journal: Option<LifecycleJournal>,
    code: ProductErrorCode,
) -> StartupRecoveryOutcome {
    StartupRecoveryOutcome::Blocked {
        error: Box::new(product_error(
            code,
            "lifecycle.recovery_unavailable",
            Some(operation.request.operation_id()),
            Some(operation.phase),
            false,
            RecoveryAction::NoAction,
        )),
        operation: Box::new(operation),
        journal: journal.map(Box::new),
    }
}

impl From<deltamod_product_contracts::SchemaError> for ExecutionFailure {
    fn from(error: deltamod_product_contracts::SchemaError) -> Self {
        Self::Store(StoreError::Schema(error))
    }
}
