//! Deterministic retention planning and enforcement for lifecycle state.
//!
//! The lifecycle store owns durable records and the filesystem adapter owns
//! path-bound deletion.  This module deliberately sits between those two
//! boundaries: it turns a complete, authoritative snapshot into a retention
//! plan and only asks a [`RetentionBackend`] to remove entries that were
//! explicitly selected.  A backend must revalidate the supplied entry while it
//! holds its durable mutation lock; in particular, it must reject a deletion
//! if a journal became active after the snapshot was read.

use deltamod_product_contracts::{
    OperationRecord, OperationState, RecoveryAction, RecoveryGeneration, RetentionDecision,
    RetentionDecisionPayload, RetentionPolicy, SchemaError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use crate::store_identity::StableObjectIdentity;

/// Number of milliseconds in one day for operation-history age calculations.
pub const MILLISECONDS_PER_DAY: u64 = 86_400_000;

pub(crate) const RECOVERY_QUARANTINE_PREFIX: &str = ".deltamod-retention-";

/// Exact filesystem binding and measured logical size of a retained recovery
/// workspace. The fields are intentionally opaque: callers obtain this value
/// from [`crate::OsLifecycleWorkspace`] and hand it back to the durable store.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryGenerationStorage {
    generation_id: String,
    operation_id: String,
    workspace_name: String,
    workspace_root_identity: StableObjectIdentity,
    workspace_object_identity: StableObjectIdentity,
    size_bytes: u64,
}

impl RecoveryGenerationStorage {
    pub(crate) fn new(
        generation_id: impl Into<String>,
        operation_id: impl Into<String>,
        workspace_name: impl Into<String>,
        workspace_root_identity: StableObjectIdentity,
        workspace_object_identity: StableObjectIdentity,
        size_bytes: u64,
    ) -> Self {
        Self {
            generation_id: generation_id.into(),
            operation_id: operation_id.into(),
            workspace_name: workspace_name.into(),
            workspace_root_identity,
            workspace_object_identity,
            size_bytes,
        }
    }

    #[must_use]
    pub fn generation_id(&self) -> &str {
        &self.generation_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub fn workspace_name(&self) -> &str {
        &self.workspace_name
    }

    #[must_use]
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub(crate) fn workspace_root_identity(&self) -> &StableObjectIdentity {
        &self.workspace_root_identity
    }

    pub(crate) fn workspace_object_identity(&self) -> &StableObjectIdentity {
        &self.workspace_object_identity
    }

    pub(crate) fn valid(&self) -> bool {
        valid_id(&self.generation_id, 128)
            && valid_id(&self.operation_id, 128)
            && valid_workspace_component(&self.workspace_name)
    }
}

/// Durable phase of a recovery-generation deletion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryDeletionPhase {
    /// The tombstone is durable. The exact live object may still need to be
    /// renamed and its contents purged.
    Prepared,
    /// Quarantined contents were purged while the exact marker shell remained
    /// as a durable filesystem witness.
    Purged,
}

/// Durable tombstone for one recovery generation. It is written before the
/// filesystem namespace changes and removed only after an identity-bound purge
/// and final metadata commit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingRecoveryDeletion {
    generation_id: String,
    installation_id: String,
    operation_id: String,
    completion_sequence: u64,
    size_bytes: u64,
    last_accessed_at_ms: u64,
    prepared_at_ms: u64,
    quarantine_name: String,
    storage: Option<RecoveryGenerationStorage>,
    phase: RecoveryDeletionPhase,
}

impl PendingRecoveryDeletion {
    pub(crate) fn new(
        expected: &RecoveryGenerationEntry,
        storage: Option<RecoveryGenerationStorage>,
        prepared_at_ms: u64,
    ) -> Self {
        let mut material = Vec::new();
        material.extend_from_slice(expected.generation.generation_id.as_bytes());
        material.push(0);
        material.extend_from_slice(expected.operation_id.as_bytes());
        if let Some(storage) = &storage {
            material.push(0);
            material.extend_from_slice(storage.workspace_name.as_bytes());
            material.push(0);
            material.extend_from_slice(&storage.workspace_object_identity.volume_id.to_be_bytes());
            material.extend_from_slice(&storage.workspace_object_identity.file_id.to_be_bytes());
        }
        let digest = Sha256::digest(&material)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Self {
            generation_id: expected.generation.generation_id.clone(),
            installation_id: expected.generation.installation_id.clone(),
            operation_id: expected.operation_id.clone(),
            completion_sequence: expected.completion_sequence,
            size_bytes: expected.generation.size_bytes,
            last_accessed_at_ms: expected.generation.last_accessed_at_ms,
            prepared_at_ms,
            quarantine_name: format!("{RECOVERY_QUARANTINE_PREFIX}{digest}"),
            storage,
            phase: RecoveryDeletionPhase::Prepared,
        }
    }

    #[must_use]
    pub fn generation_id(&self) -> &str {
        &self.generation_id
    }

    #[must_use]
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub const fn completion_sequence(&self) -> u64 {
        self.completion_sequence
    }

    #[must_use]
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    #[must_use]
    pub const fn last_accessed_at_ms(&self) -> u64 {
        self.last_accessed_at_ms
    }

    #[must_use]
    pub const fn prepared_at_ms(&self) -> u64 {
        self.prepared_at_ms
    }

    #[must_use]
    pub fn quarantine_name(&self) -> &str {
        &self.quarantine_name
    }

    #[must_use]
    pub const fn phase(&self) -> RecoveryDeletionPhase {
        self.phase
    }

    #[must_use]
    pub fn storage(&self) -> Option<&RecoveryGenerationStorage> {
        self.storage.as_ref()
    }

    pub(crate) fn mark_purged(&mut self) {
        self.phase = RecoveryDeletionPhase::Purged;
    }

    pub(crate) fn valid(&self) -> bool {
        valid_id(&self.generation_id, 128)
            && valid_id(&self.installation_id, 256)
            && valid_id(&self.operation_id, 128)
            && self.completion_sequence > 0
            && self.last_accessed_at_ms >= self.prepared_at_ms.min(self.last_accessed_at_ms)
            && valid_workspace_component(&self.quarantine_name)
            && self.quarantine_name.starts_with(RECOVERY_QUARANTINE_PREFIX)
            && self.storage.as_ref().is_none_or(|storage| {
                storage.valid()
                    && storage.generation_id == self.generation_id
                    && storage.operation_id == self.operation_id
                    && storage.size_bytes == self.size_bytes
            })
    }
}

/// Proof returned after the exact quarantined workspace has been reduced to
/// its marker shell. The private fields prevent callers from manufacturing a
/// purge acknowledgement without going through the filesystem boundary.
#[derive(Debug)]
pub struct RecoveryPurgeReceipt {
    generation_id: String,
    quarantine_name: String,
    workspace_object_identity: Option<StableObjectIdentity>,
}

impl RecoveryPurgeReceipt {
    pub(crate) fn new(tombstone: &PendingRecoveryDeletion) -> Self {
        Self {
            generation_id: tombstone.generation_id.clone(),
            quarantine_name: tombstone.quarantine_name.clone(),
            workspace_object_identity: tombstone
                .storage
                .as_ref()
                .map(|storage| storage.workspace_object_identity.clone()),
        }
    }

    pub(crate) fn matches(&self, tombstone: &PendingRecoveryDeletion) -> bool {
        self.generation_id == tombstone.generation_id
            && self.quarantine_name == tombstone.quarantine_name
            && self.workspace_object_identity
                == tombstone
                    .storage
                    .as_ref()
                    .map(|storage| storage.workspace_object_identity.clone())
    }
}

/// Proof that the exact quarantined marker shell is absent. The store requires
/// this receipt before it removes the tombstone and generation metadata.
#[derive(Debug)]
pub struct RecoveryRemovalReceipt {
    generation_id: String,
    quarantine_name: String,
    workspace_object_identity: Option<StableObjectIdentity>,
}

impl RecoveryRemovalReceipt {
    pub(crate) fn new(tombstone: &PendingRecoveryDeletion) -> Self {
        Self {
            generation_id: tombstone.generation_id.clone(),
            quarantine_name: tombstone.quarantine_name.clone(),
            workspace_object_identity: tombstone
                .storage
                .as_ref()
                .map(|storage| storage.workspace_object_identity.clone()),
        }
    }

    pub(crate) fn matches(&self, tombstone: &PendingRecoveryDeletion) -> bool {
        self.generation_id == tombstone.generation_id
            && self.quarantine_name == tombstone.quarantine_name
            && self.workspace_object_identity
                == tombstone
                    .storage
                    .as_ref()
                    .map(|storage| storage.workspace_object_identity.clone())
    }
}

/// A recovery generation plus the durable ordering and ownership data needed
/// to make retention decisions.  `completion_sequence` is zero only for an
/// unfinished generation; completed generations must have a non-zero sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryGenerationEntry {
    pub generation: RecoveryGeneration,
    pub operation_id: String,
    pub completion_sequence: u64,
}

impl RecoveryGenerationEntry {
    #[must_use]
    pub fn new(
        generation: RecoveryGeneration,
        operation_id: impl Into<String>,
        completion_sequence: u64,
    ) -> Self {
        Self {
            generation,
            operation_id: operation_id.into(),
            completion_sequence,
        }
    }

    /// Returns whether this generation is protected from retention eviction.
    #[must_use]
    pub fn protected(&self, active_journal_operation_ids: &BTreeSet<String>) -> bool {
        self.generation.active
            || !self.generation.finished
            || self.generation.pinned
            || self.generation.journal_references > 0
            || active_journal_operation_ids.contains(&self.operation_id)
    }
}

/// The operation-history fields that retention needs.  Keeping this as a
/// small runtime summary lets the store adapter gather journals and operation
/// records under one authoritative read without making the planner depend on
/// the store's private representation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationHistoryEntry {
    pub operation_id: String,
    pub updated_at_ms: u64,
    pub terminal: bool,
    pub recoverable: bool,
    pub active_journal_dependency: bool,
}

impl OperationHistoryEntry {
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        updated_at_ms: u64,
        terminal: bool,
        recoverable: bool,
        active_journal_dependency: bool,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            updated_at_ms,
            terminal,
            recoverable,
            active_journal_dependency,
        }
    }

    /// Builds a retention summary from the frozen operation contract.  A
    /// recovery-required operation or an operation whose error explicitly
    /// requests recovery remains pinned even after it is no longer active.
    #[must_use]
    pub fn from_operation_record(
        record: &OperationRecord,
        active_journal_dependency: bool,
    ) -> Self {
        let recoverable = record.state == OperationState::RecoveryRequired
            || record
                .error
                .as_ref()
                .is_some_and(|error| error.recovery_action == RecoveryAction::Recover);
        Self {
            operation_id: record.request.operation_id().to_owned(),
            updated_at_ms: record.updated_at_ms,
            terminal: record.state.terminal(),
            recoverable,
            active_journal_dependency,
        }
    }

    /// Returns whether this operation must remain in durable history.
    #[must_use]
    pub fn protected(&self, active_journal: bool) -> bool {
        !self.terminal || self.recoverable || self.active_journal_dependency || active_journal
    }
}

/// Authoritative retention inputs.  The snapshot must include unfinished
/// generations and every active journal operation; omitting either kind of
/// record would make a destructive decision unsafe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionSnapshot {
    pub recovery_generations: Vec<RecoveryGenerationEntry>,
    pub operations: Vec<OperationHistoryEntry>,
    pub active_journal_operation_ids: BTreeSet<String>,
    /// Operation records that are the durable source of a recovery generation.
    /// These records remain live until the last referencing generation has
    /// completed its two-phase deletion.
    pub recovery_source_operation_ids: BTreeSet<String>,
}

impl RetentionSnapshot {
    #[must_use]
    pub fn new(
        recovery_generations: Vec<RecoveryGenerationEntry>,
        operations: Vec<OperationHistoryEntry>,
        active_journal_operation_ids: BTreeSet<String>,
    ) -> Self {
        let recovery_source_operation_ids = recovery_generations
            .iter()
            .map(|entry| entry.operation_id.clone())
            .collect();
        Self {
            recovery_generations,
            operations,
            active_journal_operation_ids,
            recovery_source_operation_ids,
        }
    }

    /// Constructs a snapshot with an explicit source-operation set. Backends
    /// use this while deletion tombstones are pending because the generation is
    /// temporarily absent from the plannable generation list but still owns its
    /// source history.
    #[must_use]
    pub fn with_recovery_sources(
        recovery_generations: Vec<RecoveryGenerationEntry>,
        operations: Vec<OperationHistoryEntry>,
        active_journal_operation_ids: BTreeSet<String>,
        recovery_source_operation_ids: BTreeSet<String>,
    ) -> Self {
        Self {
            recovery_generations,
            operations,
            active_journal_operation_ids,
            recovery_source_operation_ids,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum RetentionError {
    #[error("recovery generation retention count must be greater than zero")]
    InvalidRecoveryGenerationCount,
    #[error("invalid recovery generation entry: {0}")]
    InvalidRecoveryGeneration(String),
    #[error("duplicate recovery generation id: {0}")]
    DuplicateRecoveryGeneration(String),
    #[error("invalid operation history entry: {0}")]
    InvalidOperationHistory(String),
    #[error("operation history retention count must be greater than zero")]
    InvalidOperationHistoryCount,
    #[error("duplicate operation history id: {0}")]
    DuplicateOperationHistory(String),
    #[error("invalid active journal operation id: {0}")]
    InvalidActiveJournalOperation(String),
    #[error("retention decision contract rejected: {0}")]
    DecisionSchema(#[from] SchemaError),
    #[error("retention plan target was not present in its snapshot: {0}")]
    MissingPlanTarget(String),
}

/// Recovery retention output.  The shared contract is stable and sorted for
/// wire consumers; `eviction_order` preserves the deterministic oldest-first
/// LRU order used by the mutating adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryRetentionPlan {
    pub decision: RetentionDecision,
    pub eviction_order: Vec<String>,
}

/// Operation-history retention output.  Eviction order is oldest-first, with
/// operation ID as the deterministic tie breaker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationHistoryDecision {
    pub keep_operation_ids: Vec<String>,
    pub evict_operation_ids: Vec<String>,
}

/// Complete retention output for one authoritative snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionPlan {
    pub recovery: RecoveryRetentionPlan,
    pub operation_history: OperationHistoryDecision,
}

/// A backend supplies an authoritative snapshot and owns the actual durable
/// deletion.  Implementations must compare the supplied entry against current
/// durable state under the same lock used for deletion.  They must reject a
/// generation with a new active journal dependency and must remove an
/// operation's journal only when that journal is terminal and no longer needed
/// for recovery.
pub trait RetentionBackend {
    type Error;

    fn retention_snapshot(&mut self) -> Result<RetentionSnapshot, Self::Error>;

    fn delete_recovery_generation(
        &mut self,
        expected: &RecoveryGenerationEntry,
    ) -> Result<(), Self::Error>;

    fn delete_operation_history(
        &mut self,
        expected: &OperationHistoryEntry,
    ) -> Result<(), Self::Error>;
}

/// Result of a successful enforcement pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetentionRun {
    pub plan: RetentionPlan,
    pub deleted_generation_ids: Vec<String>,
    pub deleted_operation_ids: Vec<String>,
}

/// Failure from planning or from a backend deletion.  Backend failures retain
/// the IDs already deleted so callers can report a partial, durable cleanup
/// rather than presenting it as an all-or-nothing operation.
#[derive(Debug)]
pub enum RetentionEnforcementError<E> {
    Planning(RetentionError),
    Store {
        error: E,
        deleted_generation_ids: Vec<String>,
        deleted_operation_ids: Vec<String>,
    },
}

/// Store/filesystem failure from the production crash-safe retention adapter.
#[cfg(any(unix, windows))]
#[derive(Debug, Error)]
pub enum CrashSafeRetentionError {
    #[error(transparent)]
    Store(#[from] crate::StoreError),
    #[error("recovery workspace retention failed: {0}")]
    Filesystem(deltamod_product_contracts::FilesystemBoundaryError),
}

#[cfg(any(unix, windows))]
impl From<deltamod_product_contracts::FilesystemBoundaryError> for CrashSafeRetentionError {
    fn from(error: deltamod_product_contracts::FilesystemBoundaryError) -> Self {
        Self::Filesystem(error)
    }
}

/// Result of reconciling durable deletion tombstones during startup.
#[cfg(any(unix, windows))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryRetentionReconciliation {
    pub finalized_generation_ids: Vec<String>,
}

/// Production adapter that coordinates the durable store with the exact
/// identity-pinned OS workspace. `reconcile_startup` is intentionally public so
/// shell integration can run it immediately after both roots are opened.
#[cfg(any(unix, windows))]
pub struct CrashSafeRetentionRuntime<'a> {
    store: &'a mut crate::DurableLifecycleStore,
    workspace: &'a mut crate::OsLifecycleWorkspace,
}

#[cfg(any(unix, windows))]
impl<'a> CrashSafeRetentionRuntime<'a> {
    #[must_use]
    pub fn new(
        store: &'a mut crate::DurableLifecycleStore,
        workspace: &'a mut crate::OsLifecycleWorkspace,
    ) -> Self {
        Self { store, workspace }
    }

    /// Finishes every prepared or purged deletion tombstone. Each phase is
    /// idempotent, so process termination between any two calls is safe.
    pub fn reconcile_startup(
        &mut self,
    ) -> Result<RecoveryRetentionReconciliation, CrashSafeRetentionError> {
        let pending = self.store.pending_recovery_deletions()?;
        let mut finalized_generation_ids = Vec::new();
        for tombstone in pending {
            let generation_id = tombstone.generation_id().to_owned();
            self.reconcile_one(tombstone)?;
            finalized_generation_ids.push(generation_id);
        }
        Ok(RecoveryRetentionReconciliation {
            finalized_generation_ids,
        })
    }

    /// Reconciles one tombstone after the shell has selected the workspace
    /// belonging to its installation. Stores shared by multiple installation
    /// roots must route each tombstone before calling this method.
    pub fn reconcile_deletion(
        &mut self,
        tombstone: PendingRecoveryDeletion,
    ) -> Result<(), CrashSafeRetentionError> {
        self.reconcile_one(tombstone)
    }

    /// Runs startup reconciliation, refreshes persisted storage measurements,
    /// plans retention, and executes generation deletion before operation
    /// history cleanup.
    pub fn enforce(
        &mut self,
        policy: RetentionPolicy,
        now_ms: u64,
    ) -> Result<RetentionRun, RetentionEnforcementError<CrashSafeRetentionError>> {
        enforce_retention(self, policy, now_ms)
    }

    fn synchronize_generation_storage(&mut self) -> Result<(), CrashSafeRetentionError> {
        for (generation_id, operation_id, _) in self.store.recovery_generation_storage_requests()? {
            let storage = self
                .workspace
                .recovery_generation_storage(&generation_id, &operation_id)?;
            self.store.bind_recovery_generation_storage(storage)?;
        }
        Ok(())
    }

    fn reconcile_one(
        &mut self,
        mut tombstone: PendingRecoveryDeletion,
    ) -> Result<(), CrashSafeRetentionError> {
        if tombstone.phase() == RecoveryDeletionPhase::Prepared {
            self.workspace.quarantine_recovery_generation(&tombstone)?;
            let receipt = self
                .workspace
                .purge_quarantined_recovery_generation(&tombstone)?;
            tombstone = self
                .store
                .mark_recovery_generation_purged(&tombstone, &receipt)?;
        }
        let receipt = self
            .workspace
            .remove_purged_recovery_quarantine(&tombstone)?;
        self.store
            .finalize_recovery_generation_deletion(&tombstone, &receipt)?;
        Ok(())
    }
}

#[cfg(any(unix, windows))]
impl RetentionBackend for CrashSafeRetentionRuntime<'_> {
    type Error = CrashSafeRetentionError;

    fn retention_snapshot(&mut self) -> Result<RetentionSnapshot, Self::Error> {
        self.reconcile_startup()?;
        self.synchronize_generation_storage()?;
        self.store.retention_snapshot().map_err(Into::into)
    }

    fn delete_recovery_generation(
        &mut self,
        expected: &RecoveryGenerationEntry,
    ) -> Result<(), Self::Error> {
        let storage_required = self
            .store
            .recovery_generation_storage_requests()?
            .iter()
            .any(|(generation_id, operation_id, _)| {
                generation_id == &expected.generation.generation_id
                    && operation_id == &expected.operation_id
            });
        if storage_required {
            let storage = self.workspace.recovery_generation_storage(
                &expected.generation.generation_id,
                &expected.operation_id,
            )?;
            self.store.bind_recovery_generation_storage(storage)?;
        }
        let tombstone = self.store.prepare_recovery_generation_deletion(expected)?;
        self.reconcile_one(tombstone)
    }

    fn delete_operation_history(
        &mut self,
        expected: &OperationHistoryEntry,
    ) -> Result<(), Self::Error> {
        self.store
            .delete_operation_history(expected)
            .map_err(Into::into)
    }
}

/// Plans recovery and operation-history retention from one snapshot.
pub fn plan_retention(
    snapshot: &RetentionSnapshot,
    policy: RetentionPolicy,
    now_ms: u64,
) -> Result<RetentionPlan, RetentionError> {
    Ok(RetentionPlan {
        recovery: plan_recovery_retention(
            &snapshot.recovery_generations,
            &snapshot.active_journal_operation_ids,
            policy,
        )?,
        operation_history: plan_operation_history_with_sources(
            &snapshot.operations,
            &snapshot.active_journal_operation_ids,
            &snapshot.recovery_source_operation_ids,
            policy,
            now_ms,
        )?,
    })
}

/// Selects completed recovery generations for eviction.
pub fn plan_recovery_retention(
    entries: &[RecoveryGenerationEntry],
    active_journal_operation_ids: &BTreeSet<String>,
    policy: RetentionPolicy,
) -> Result<RecoveryRetentionPlan, RetentionError> {
    validate_recovery_inputs(entries, active_journal_operation_ids, policy)?;

    let mut by_installation: BTreeMap<&str, Vec<&RecoveryGenerationEntry>> = BTreeMap::new();
    for entry in entries {
        by_installation
            .entry(&entry.generation.installation_id)
            .or_default()
            .push(entry);
    }

    let mut protected = BTreeSet::new();
    for group in by_installation.values_mut() {
        group.sort_by(|left, right| recovery_newest_cmp(left, right));

        for entry in group
            .iter()
            .filter(|entry| entry.generation.finished)
            .take(policy.recovery_generations_per_installation)
        {
            protected.insert(entry.generation.generation_id.clone());
        }

        let viable: Vec<_> = group
            .iter()
            .filter(|entry| entry.generation.viable)
            .collect();
        if viable.len() == 1 {
            protected.insert(viable[0].generation.generation_id.clone());
        }

        for entry in group {
            if entry.protected(active_journal_operation_ids) {
                protected.insert(entry.generation.generation_id.clone());
            }
        }
    }

    let bytes_before = entries
        .iter()
        .filter(|entry| entry.generation.finished)
        .try_fold(0_u64, |total, entry| {
            total.checked_add(entry.generation.size_bytes)
        })
        .ok_or_else(|| RetentionError::InvalidRecoveryGeneration("byte overflow".into()))?;

    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|entry| entry.generation.finished)
        .filter(|entry| !protected.contains(&entry.generation.generation_id))
        .collect();
    candidates.sort_by(|left, right| recovery_lru_cmp(left, right));

    let mut bytes_after = bytes_before;
    let mut eviction_order = Vec::new();
    for entry in candidates {
        if bytes_after <= policy.recovery_limit_bytes {
            break;
        }
        bytes_after = bytes_after
            .checked_sub(entry.generation.size_bytes)
            .ok_or_else(|| RetentionError::InvalidRecoveryGeneration("byte underflow".into()))?;
        eviction_order.push(entry.generation.generation_id.clone());
    }

    let evicted: BTreeSet<_> = eviction_order.iter().cloned().collect();
    let mut keep_generation_ids: Vec<_> = entries
        .iter()
        .filter(|entry| !evicted.contains(&entry.generation.generation_id))
        .map(|entry| entry.generation.generation_id.clone())
        .collect();
    let mut evict_generation_ids = eviction_order.clone();
    keep_generation_ids.sort();
    evict_generation_ids.sort();

    let decision = RetentionDecision::new(RetentionDecisionPayload {
        limit_bytes: policy.recovery_limit_bytes,
        bytes_before,
        bytes_after,
        keep_generation_ids,
        evict_generation_ids,
        over_limit: bytes_after > policy.recovery_limit_bytes,
    })?;

    Ok(RecoveryRetentionPlan {
        decision,
        eviction_order,
    })
}

/// Selects operation records outside the newest-N-or-age window.
pub fn plan_operation_history(
    entries: &[OperationHistoryEntry],
    active_journal_operation_ids: &BTreeSet<String>,
    policy: RetentionPolicy,
    now_ms: u64,
) -> Result<OperationHistoryDecision, RetentionError> {
    plan_operation_history_with_sources(
        entries,
        active_journal_operation_ids,
        &BTreeSet::new(),
        policy,
        now_ms,
    )
}

/// Selects operation records while protecting every operation that is still
/// the durable source of a recovery generation or deletion tombstone.
pub fn plan_operation_history_with_sources(
    entries: &[OperationHistoryEntry],
    active_journal_operation_ids: &BTreeSet<String>,
    recovery_source_operation_ids: &BTreeSet<String>,
    policy: RetentionPolicy,
    now_ms: u64,
) -> Result<OperationHistoryDecision, RetentionError> {
    validate_operation_inputs(entries, active_journal_operation_ids, policy)?;

    let age_ms = u64::from(policy.operation_history_age_days) * MILLISECONDS_PER_DAY;
    let cutoff_ms = now_ms.saturating_sub(age_ms);
    let mut newest: Vec<_> = entries.iter().collect();
    newest.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.operation_id.cmp(&right.operation_id))
    });

    let mut keep = BTreeSet::new();
    for entry in newest.into_iter().take(policy.operation_history_items) {
        keep.insert(entry.operation_id.clone());
    }
    for entry in entries {
        if entry.updated_at_ms >= cutoff_ms
            || entry.protected(active_journal_operation_ids.contains(&entry.operation_id))
            || recovery_source_operation_ids.contains(&entry.operation_id)
        {
            keep.insert(entry.operation_id.clone());
        }
    }

    let mut evict: Vec<_> = entries
        .iter()
        .filter(|entry| !keep.contains(&entry.operation_id))
        .collect();
    evict.sort_by(|left, right| {
        left.updated_at_ms
            .cmp(&right.updated_at_ms)
            .then_with(|| left.operation_id.cmp(&right.operation_id))
    });

    let mut keep_operation_ids: Vec<_> = keep.into_iter().collect();
    let evict_operation_ids: Vec<_> = evict
        .into_iter()
        .map(|entry| entry.operation_id.clone())
        .collect();
    keep_operation_ids.sort();

    Ok(OperationHistoryDecision {
        keep_operation_ids,
        evict_operation_ids,
    })
}

/// Executes one planned pass through the backend's identity- and journal-aware
/// deletion methods.
pub fn enforce_retention<B: RetentionBackend>(
    backend: &mut B,
    policy: RetentionPolicy,
    now_ms: u64,
) -> Result<RetentionRun, RetentionEnforcementError<B::Error>> {
    let snapshot =
        backend
            .retention_snapshot()
            .map_err(|error| RetentionEnforcementError::Store {
                error,
                deleted_generation_ids: Vec::new(),
                deleted_operation_ids: Vec::new(),
            })?;
    let plan =
        plan_retention(&snapshot, policy, now_ms).map_err(RetentionEnforcementError::Planning)?;

    let mut deleted_generation_ids = Vec::new();
    for generation_id in &plan.recovery.eviction_order {
        let entry = snapshot
            .recovery_generations
            .iter()
            .find(|entry| entry.generation.generation_id == *generation_id)
            .ok_or_else(|| {
                RetentionEnforcementError::Planning(RetentionError::MissingPlanTarget(
                    generation_id.clone(),
                ))
            })?;
        if let Err(error) = backend.delete_recovery_generation(entry) {
            return Err(RetentionEnforcementError::Store {
                error,
                deleted_generation_ids,
                deleted_operation_ids: Vec::new(),
            });
        }
        deleted_generation_ids.push(generation_id.clone());
    }

    let mut deleted_operation_ids = Vec::new();
    for operation_id in &plan.operation_history.evict_operation_ids {
        let entry = snapshot
            .operations
            .iter()
            .find(|entry| entry.operation_id == *operation_id)
            .ok_or_else(|| {
                RetentionEnforcementError::Planning(RetentionError::MissingPlanTarget(
                    operation_id.clone(),
                ))
            })?;
        if let Err(error) = backend.delete_operation_history(entry) {
            return Err(RetentionEnforcementError::Store {
                error,
                deleted_generation_ids,
                deleted_operation_ids,
            });
        }
        deleted_operation_ids.push(operation_id.clone());
    }

    Ok(RetentionRun {
        plan,
        deleted_generation_ids,
        deleted_operation_ids,
    })
}

fn validate_recovery_inputs(
    entries: &[RecoveryGenerationEntry],
    active_journal_operation_ids: &BTreeSet<String>,
    policy: RetentionPolicy,
) -> Result<(), RetentionError> {
    if policy.recovery_generations_per_installation == 0 {
        return Err(RetentionError::InvalidRecoveryGenerationCount);
    }
    for operation_id in active_journal_operation_ids {
        if !valid_id(operation_id, 128) {
            return Err(RetentionError::InvalidActiveJournalOperation(
                operation_id.clone(),
            ));
        }
    }

    let mut ids = BTreeSet::new();
    for entry in entries {
        let generation = &entry.generation;
        if !valid_id(&generation.generation_id, 128)
            || !valid_id(&generation.installation_id, 256)
            || !valid_id(&entry.operation_id, 128)
            || generation.last_accessed_at_ms < generation.completed_at_ms
            || (generation.active && generation.finished)
            || (generation.finished && entry.completion_sequence == 0)
            || (!generation.finished && entry.completion_sequence != 0)
        {
            return Err(RetentionError::InvalidRecoveryGeneration(
                generation.generation_id.clone(),
            ));
        }
        if !ids.insert(generation.generation_id.clone()) {
            return Err(RetentionError::DuplicateRecoveryGeneration(
                generation.generation_id.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_operation_inputs(
    entries: &[OperationHistoryEntry],
    active_journal_operation_ids: &BTreeSet<String>,
    policy: RetentionPolicy,
) -> Result<(), RetentionError> {
    if policy.operation_history_items == 0 {
        return Err(RetentionError::InvalidOperationHistoryCount);
    }
    for operation_id in active_journal_operation_ids {
        if !valid_id(operation_id, 128) {
            return Err(RetentionError::InvalidActiveJournalOperation(
                operation_id.clone(),
            ));
        }
    }
    let mut ids = BTreeSet::new();
    for entry in entries {
        if !valid_id(&entry.operation_id, 128) {
            return Err(RetentionError::InvalidOperationHistory(
                entry.operation_id.clone(),
            ));
        }
        if !ids.insert(entry.operation_id.clone()) {
            return Err(RetentionError::DuplicateOperationHistory(
                entry.operation_id.clone(),
            ));
        }
    }
    Ok(())
}

fn recovery_newest_cmp(
    left: &RecoveryGenerationEntry,
    right: &RecoveryGenerationEntry,
) -> std::cmp::Ordering {
    right
        .completion_sequence
        .cmp(&left.completion_sequence)
        .then_with(|| {
            right
                .generation
                .completed_at_ms
                .cmp(&left.generation.completed_at_ms)
        })
        .then_with(|| {
            right
                .generation
                .generation_id
                .cmp(&left.generation.generation_id)
        })
}

fn recovery_lru_cmp(
    left: &RecoveryGenerationEntry,
    right: &RecoveryGenerationEntry,
) -> std::cmp::Ordering {
    left.generation
        .last_accessed_at_ms
        .cmp(&right.generation.last_accessed_at_ms)
        .then_with(|| {
            left.generation
                .completed_at_ms
                .cmp(&right.generation.completed_at_ms)
        })
        .then_with(|| left.completion_sequence.cmp(&right.completion_sequence))
        .then_with(|| {
            left.generation
                .generation_id
                .cmp(&right.generation.generation_id)
        })
}

fn valid_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_workspace_component(value: &str) -> bool {
    valid_id(value, 255) && value != "." && value != ".."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(
        id: &str,
        installation: &str,
        completion: u64,
        size_bytes: u64,
    ) -> RecoveryGenerationEntry {
        RecoveryGenerationEntry::new(
            RecoveryGeneration {
                generation_id: id.into(),
                installation_id: installation.into(),
                size_bytes,
                completed_at_ms: completion,
                last_accessed_at_ms: completion,
                active: false,
                finished: true,
                pinned: false,
                journal_references: 0,
                viable: true,
            },
            format!("op-{id}"),
            completion,
        )
    }

    fn policy() -> RetentionPolicy {
        RetentionPolicy {
            recovery_limit_bytes: 30,
            recovery_generations_per_installation: 3,
            operation_history_items: 2,
            operation_history_age_days: 30,
            ..RetentionPolicy::default()
        }
    }

    #[test]
    fn recovery_keeps_latest_three_per_installation_and_evicts_global_lru() {
        let entries = vec![
            generation("a1", "a", 1, 10),
            generation("a2", "a", 2, 10),
            generation("a3", "a", 3, 10),
            generation("a4", "a", 4, 10),
            generation("b1", "b", 5, 10),
            generation("b2", "b", 6, 10),
            generation("b3", "b", 7, 10),
            generation("b4", "b", 8, 10),
        ];
        let plan = plan_recovery_retention(&entries, &BTreeSet::new(), policy()).unwrap();

        assert_eq!(plan.eviction_order, ["a1", "b1"]);
        assert!(plan.decision.over_limit);
        assert_eq!(plan.decision.bytes_before, 80);
        assert_eq!(plan.decision.bytes_after, 60);
    }

    #[test]
    fn protected_and_sole_viable_generations_survive_even_over_limit() {
        let mut unfinished = generation("unfinished", "game", 0, 10);
        unfinished.generation.finished = false;
        unfinished.generation.active = true;
        unfinished.completion_sequence = 0;
        let mut pinned = generation("pinned", "game", 2, 10);
        pinned.generation.pinned = true;
        let mut journal = generation("journal", "game", 3, 10);
        journal.generation.journal_references = 1;
        let mut sole = generation("sole", "other", 4, 10);
        sole.generation.viable = true;
        let active = BTreeSet::from([journal.operation_id.clone()]);
        let mut constrained = policy();
        constrained.recovery_limit_bytes = 0;
        constrained.recovery_generations_per_installation = 1;

        let plan =
            plan_recovery_retention(&[unfinished, pinned, journal, sole], &active, constrained)
                .unwrap();
        assert!(plan.eviction_order.is_empty());
        assert!(plan.decision.over_limit);
    }

    #[test]
    fn operation_history_keeps_newest_or_recent_and_always_keeps_recoverable() {
        let day = MILLISECONDS_PER_DAY;
        let now = 100 * day;
        let entries = vec![
            OperationHistoryEntry::new("old", 1, true, false, false),
            OperationHistoryEntry::new("recover", 2, true, true, false),
            OperationHistoryEntry::new("recent", now - 10 * day, true, false, false),
            OperationHistoryEntry::new("newest", now, true, false, false),
        ];
        let plan = plan_operation_history(&entries, &BTreeSet::new(), policy(), now).unwrap();

        assert_eq!(plan.evict_operation_ids, ["old"]);
        assert_eq!(plan.keep_operation_ids, ["newest", "recent", "recover"]);
    }

    #[test]
    fn active_journal_operation_is_never_evicted() {
        let entries = vec![OperationHistoryEntry::new(
            "journal-op",
            1,
            true,
            false,
            false,
        )];
        let active = BTreeSet::from(["journal-op".to_owned()]);
        let mut constrained = policy();
        constrained.operation_history_items = 1;
        constrained.operation_history_age_days = 0;
        let plan = plan_operation_history(&entries, &active, constrained, 10).unwrap();
        assert_eq!(plan.keep_operation_ids, ["journal-op"]);
        assert!(plan.evict_operation_ids.is_empty());
    }

    #[test]
    fn recovery_source_operation_history_is_protected_until_generation_removal() {
        let entries = vec![
            OperationHistoryEntry::new("source-op", 1, true, false, false),
            OperationHistoryEntry::new("newer-op", 2, true, false, false),
        ];
        let mut constrained = policy();
        constrained.operation_history_items = 1;
        constrained.operation_history_age_days = 0;
        let plan = plan_operation_history_with_sources(
            &entries,
            &BTreeSet::new(),
            &BTreeSet::from(["source-op".to_owned()]),
            constrained,
            10,
        )
        .unwrap();

        assert_eq!(plan.keep_operation_ids, ["newer-op", "source-op"]);
        assert!(plan.evict_operation_ids.is_empty());
    }
}
