use crate::{
    hex_digest,
    retention::{
        OperationHistoryEntry, PendingRecoveryDeletion, RecoveryDeletionPhase,
        RecoveryGenerationEntry, RecoveryGenerationStorage, RecoveryPurgeReceipt,
        RecoveryRemovalReceipt, RetentionSnapshot,
    },
    store_identity::{
        configure_no_follow, inspect_path, verify_opened_path, IdentityError, StableObjectIdentity,
        StoreObjectKind,
    },
    valid_id, valid_sha256, FaultInjector, FaultPoint, JournalCheckpointKind, MonotonicClock,
    NoFaults, SystemClock, TrustedClock,
};
use deltamod_product_contracts::{
    decode_current, validate_installation_manifest, validate_journal_binding, AcquireOutcome,
    CompareAndSwapOutcome, ContractDocument, FilesystemBoundaryError, InstallationClaimsLedger,
    JournalDisposition, LifecycleJournal, LifecycleMutationGuard, LifecycleOperationKind,
    LifecycleTransactionStore, ManifestCommitState, MutationCheckpoint, MutationFenceError,
    ObservationSnapshot, ObservedFileState, OperationLease, OperationRecord,
    OperationRecordPayload, OperationRequest, OperationState, OperationStore, ProductError,
    RecoveryAction, RecoveryGeneration, SchemaError, ValidatedMutationReconciliation,
    ValidatedMutationTransition, ValidatedRecoveryRebind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;

const STORE_VERSION: u32 = 1;
const FRAME_MAGIC: &[u8; 8] = b"DMLCST01";
const COMPACTED_FRAME_MAGIC: &[u8; 8] = b"DMLCSC01";
const FRAME_HEADER_BYTES: usize = 8 + 8 + 8 + 32;
const MAX_STATE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationManifest {
    pub records: Vec<deltamod_product_contracts::InstalledModRecord>,
    pub ledger: InstallationClaimsLedger,
}

impl InstallationManifest {
    pub fn new(
        mut records: Vec<deltamod_product_contracts::InstalledModRecord>,
        ledger: InstallationClaimsLedger,
    ) -> Result<Self, SchemaError> {
        records.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
        validate_installation_manifest(&records, &ledger)?;
        Ok(Self { records, ledger })
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.ledger.manifest_generation
    }

    #[must_use]
    pub fn installation_id(&self) -> &str {
        &self.ledger.installation_id
    }

    /// Stable digest used to bind an active-profile pointer to the exact
    /// manifest published in the same durable store frame.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let bytes = serde_json::to_vec(&(&self.ledger, &self.records)).unwrap_or_default();
        hex_digest(&bytes)
    }
}

/// Durable, read-only identity of the profile whose lockfile produced the
/// currently published installation manifest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActiveProfilePointer {
    installation_id: String,
    profile_id: String,
    lock_fingerprint: String,
    manifest_generation: u64,
    manifest_fingerprint: String,
}

impl ActiveProfilePointer {
    pub(crate) fn new(
        installation_id: impl Into<String>,
        profile_id: impl Into<String>,
        lock_fingerprint: impl Into<String>,
        manifest: &InstallationManifest,
    ) -> Result<Self, StoreError> {
        let pointer = Self {
            installation_id: installation_id.into(),
            profile_id: profile_id.into(),
            lock_fingerprint: lock_fingerprint.into(),
            manifest_generation: manifest.generation(),
            manifest_fingerprint: manifest.fingerprint(),
        };
        pointer.validate()?;
        if pointer.installation_id != manifest.installation_id() {
            return Err(StoreError::InvalidTransition);
        }
        Ok(pointer)
    }

    fn validate(&self) -> Result<(), StoreError> {
        if valid_id(&self.installation_id, 256)
            && valid_id(&self.profile_id, 256)
            && valid_sha256(&self.lock_fingerprint)
            && self.manifest_generation > 0
            && valid_sha256(&self.manifest_fingerprint)
        {
            Ok(())
        } else {
            Err(StoreError::Corrupt("active profile pointer"))
        }
    }

    fn matches_manifest(&self, manifest: &InstallationManifest) -> bool {
        self.installation_id == manifest.installation_id()
            && self.manifest_generation == manifest.generation()
            && self
                .manifest_fingerprint
                .eq_ignore_ascii_case(&manifest.fingerprint())
    }

    #[must_use]
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    #[must_use]
    pub fn lock_fingerprint(&self) -> &str {
        &self.lock_fingerprint
    }

    #[must_use]
    pub const fn manifest_generation(&self) -> u64 {
        self.manifest_generation
    }

    #[must_use]
    pub fn manifest_fingerprint(&self) -> &str {
        &self.manifest_fingerprint
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryGenerationSnapshot {
    pub generation_id: String,
    pub installation_id: String,
    pub operation_id: String,
    pub previous_manifest: Option<InstallationManifest>,
    pub target_manifest: InstallationManifest,
    pub completed_at_ms: u64,
    pub completion_sequence: u64,
    pub size_bytes: u64,
    pub last_accessed_at_ms: u64,
    pub pinned: bool,
    pub protected_by_operations: BTreeSet<String>,
    pub manifest_only_observations: Vec<ObservationSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterruptedOperation {
    pub record: OperationRecord,
    pub lease: OperationLease,
    pub journal: Option<LifecycleJournal>,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("lifecycle store I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("lifecycle store serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("lifecycle contract validation failed: {0}")]
    Schema(#[from] SchemaError),
    #[error("durable lifecycle state is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("operation identity conflicts with durable history")]
    IdempotencyConflict,
    #[error("operation or journal was not found")]
    NotFound,
    #[error("installation already has a mutating operation")]
    InstallationBusy,
    #[error("operation lease is stale or expired")]
    LostLease,
    #[error("operation revision is stale")]
    StaleRevision,
    #[error("journal fingerprint or sequence is stale")]
    StaleJournal,
    #[error("invalid lifecycle state transition")]
    InvalidTransition,
    #[error("retention candidate changed or gained a durable reference")]
    RetentionRace,
    #[error("recovery-generation storage metadata is missing or changed")]
    RecoveryStorageChanged,
    #[error("pending recovery deletion does not match durable state")]
    PendingDeletionConflict,
    #[error("journal sequence has no remaining recovery headroom")]
    SequenceExhausted,
    #[error("manifest destination changed before publication: {0}")]
    ManifestClaimChanged(#[source] FilesystemBoundaryError),
    #[error("lifecycle store object identity changed: {0}")]
    StoreIdentityChanged(&'static str),
    #[error(transparent)]
    Injected(#[from] crate::InjectedFault),
}

#[derive(Clone, Debug)]
pub struct Terminalization {
    pub state: OperationState,
    pub error: Option<ProductError>,
    pub result_fingerprint: Option<String>,
    pub now_ms: u64,
}

/// Evidence from atomically replacing the append log with one checksummed
/// compacted snapshot. The durable store sequence is preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreLogCompaction {
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub store_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedManifest {
    ledger: String,
    records: Vec<String>,
}

impl EncodedManifest {
    fn encode(manifest: &InstallationManifest) -> Result<Self, StoreError> {
        Ok(Self {
            ledger: serde_json::to_string(&manifest.ledger)?,
            records: manifest
                .records
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<_, _>>()?,
        })
    }

    fn decode(&self) -> Result<InstallationManifest, StoreError> {
        let ledger = decode_current::<InstallationClaimsLedger>(self.ledger.as_bytes())?;
        let records = self
            .records
            .iter()
            .map(|record| decode_current(record.as_bytes()).map_err(StoreError::from))
            .collect::<Result<Vec<_>, _>>()?;
        InstallationManifest::new(records, ledger).map_err(StoreError::from)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredGeneration {
    generation_id: String,
    installation_id: String,
    operation_id: String,
    previous_manifest: Option<EncodedManifest>,
    target_manifest: EncodedManifest,
    started_at_ms: u64,
    completed_at_ms: Option<u64>,
    #[serde(default)]
    completion_sequence: Option<u64>,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    last_accessed_at_ms: Option<u64>,
    #[serde(default)]
    storage: Option<RecoveryGenerationStorage>,
    viable: bool,
    pinned: bool,
    #[serde(default)]
    protected_by_operations: BTreeSet<String>,
    #[serde(default)]
    manifest_only_observations: Vec<ObservationSnapshot>,
    #[serde(default)]
    previous_active_profile: Option<ActiveProfilePointer>,
    #[serde(default)]
    target_active_profile: Option<ActiveProfilePointer>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredProfileSwitchCommit {
    operation_id: String,
    plan_fingerprint: String,
    previous: Option<ActiveProfilePointer>,
    target: ActiveProfilePointer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedStoreIdentity {
    root: StableObjectIdentity,
    lock: StableObjectIdentity,
    log: StableObjectIdentity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedState {
    version: u32,
    store_sequence: u64,
    #[serde(default)]
    trusted_time_floor_ms: u64,
    #[serde(default)]
    store_identity: Option<PersistedStoreIdentity>,
    next_fencing_tokens: BTreeMap<String, u64>,
    operations: BTreeMap<String, String>,
    idempotency: BTreeMap<String, String>,
    active_leases: BTreeMap<String, OperationLease>,
    journals: BTreeMap<String, String>,
    manifests: BTreeMap<String, EncodedManifest>,
    pending_manifests: BTreeMap<String, EncodedManifest>,
    generations: BTreeMap<String, StoredGeneration>,
    #[serde(default)]
    pending_generation_deletions: BTreeMap<String, PendingRecoveryDeletion>,
    #[serde(default)]
    active_profiles: BTreeMap<String, ActiveProfilePointer>,
    #[serde(default)]
    profile_switch_commits: BTreeMap<String, StoredProfileSwitchCommit>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            store_sequence: 0,
            trusted_time_floor_ms: 0,
            store_identity: None,
            next_fencing_tokens: BTreeMap::new(),
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            active_leases: BTreeMap::new(),
            journals: BTreeMap::new(),
            manifests: BTreeMap::new(),
            pending_manifests: BTreeMap::new(),
            generations: BTreeMap::new(),
            pending_generation_deletions: BTreeMap::new(),
            active_profiles: BTreeMap::new(),
            profile_switch_commits: BTreeMap::new(),
        }
    }
}

impl PersistedState {
    fn operation(&self, operation_id: &str) -> Result<OperationRecord, StoreError> {
        let encoded = self
            .operations
            .get(operation_id)
            .ok_or(StoreError::NotFound)?;
        decode_current(encoded.as_bytes()).map_err(StoreError::from)
    }

    fn journal(&self, operation_id: &str) -> Result<LifecycleJournal, StoreError> {
        let encoded = self
            .journals
            .get(operation_id)
            .ok_or(StoreError::NotFound)?;
        decode_current(encoded.as_bytes()).map_err(StoreError::from)
    }

    fn manifest(&self, installation_id: &str) -> Result<Option<InstallationManifest>, StoreError> {
        self.manifests
            .get(installation_id)
            .map(EncodedManifest::decode)
            .transpose()
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.version != STORE_VERSION {
            return Err(StoreError::Corrupt("store version"));
        }
        let mut seen_idempotency = BTreeSet::new();
        for (operation_id, encoded) in &self.operations {
            let record: OperationRecord = decode_current(encoded.as_bytes())?;
            if record.request.operation_id() != operation_id
                || !seen_idempotency.insert(record.request.idempotency_key().to_owned())
                || self.idempotency.get(record.request.idempotency_key()) != Some(operation_id)
                || self
                    .next_fencing_tokens
                    .get(&record.request.intent().installation_id)
                    .copied()
                    .unwrap_or_default()
                    < record.fencing_token
            {
                return Err(StoreError::Corrupt("operation index"));
            }
            if !record.state.terminal()
                && self
                    .active_leases
                    .get(&record.request.intent().installation_id)
                    .is_none_or(|lease| {
                        lease.operation_id.as_str() != operation_id.as_str()
                            || lease.fencing_token != record.fencing_token
                    })
            {
                return Err(StoreError::Corrupt("unleased operation"));
            }
        }
        if self.idempotency.len() != seen_idempotency.len()
            || self.idempotency.iter().any(|(key, operation_id)| {
                !self.operations.contains_key(operation_id)
                    || self
                        .operation(operation_id)
                        .map_or(true, |record| record.request.idempotency_key() != key)
            })
        {
            return Err(StoreError::Corrupt("idempotency index"));
        }
        for (installation_id, lease) in &self.active_leases {
            lease.validate()?;
            let record = self.operation(&lease.operation_id)?;
            if installation_id != &lease.installation_id
                || record.state.terminal()
                || record.request.intent().installation_id != *installation_id
                || record.fencing_token != lease.fencing_token
            {
                return Err(StoreError::Corrupt("active lease"));
            }
        }
        for (operation_id, encoded) in &self.journals {
            let journal: LifecycleJournal = decode_current(encoded.as_bytes())?;
            let record = self.operation(operation_id)?;
            if journal.operation_id != *operation_id
                || journal.operation_id != record.request.operation_id()
                || journal.idempotency_key != record.request.idempotency_key()
                || !journal
                    .request_fingerprint
                    .eq_ignore_ascii_case(record.request.request_fingerprint())
                || journal.installation_id != record.request.intent().installation_id
                || journal.operation != record.request.intent().kind
                || journal.operation_revision != record.revision
                || journal.fencing_token != record.fencing_token
            {
                return Err(StoreError::Corrupt("journal index"));
            }
            if journal.phase != deltamod_product_contracts::OperationPhase::Complete
                && ((journal.operation == LifecycleOperationKind::ProfileSwitch)
                    != self.profile_switch_commits.contains_key(operation_id))
            {
                return Err(StoreError::Corrupt("profile switch journal binding"));
            }
        }
        for (installation_id, manifest) in &self.manifests {
            if manifest.decode()?.installation_id() != installation_id {
                return Err(StoreError::Corrupt("manifest index"));
            }
        }
        for (installation_id, pointer) in &self.active_profiles {
            pointer.validate()?;
            let manifest = self
                .manifests
                .get(installation_id)
                .ok_or(StoreError::Corrupt("active profile manifest"))?
                .decode()?;
            if pointer.installation_id() != installation_id || !pointer.matches_manifest(&manifest)
            {
                return Err(StoreError::Corrupt("active profile manifest binding"));
            }
        }
        for (operation_id, manifest) in &self.pending_manifests {
            let journal = self.journal(operation_id)?;
            if manifest.decode()?.installation_id() != journal.installation_id
                || journal.manifest_commit_state != ManifestCommitState::TemporaryWritten
            {
                return Err(StoreError::Corrupt("pending manifest"));
            }
        }
        let mut completion_sequences = BTreeSet::new();
        for (generation_id, generation) in &self.generations {
            if generation_id != &generation.generation_id
                || !valid_id(generation_id, 128)
                || generation.operation_id.is_empty()
                || generation.target_manifest.decode()?.installation_id()
                    != generation.installation_id
                || generation
                    .previous_manifest
                    .as_ref()
                    .map(EncodedManifest::decode)
                    .transpose()?
                    .as_ref()
                    .is_some_and(|manifest| {
                        manifest.installation_id() != generation.installation_id
                    })
                || generation
                    .completed_at_ms
                    .is_some_and(|completed| completed < generation.started_at_ms)
                || generation.completed_at_ms.is_some() != generation.completion_sequence.is_some()
                || generation.completed_at_ms.is_some()
                    != (generation.size_bytes.is_some() && generation.last_accessed_at_ms.is_some())
                || generation
                    .completed_at_ms
                    .zip(generation.last_accessed_at_ms)
                    .is_some_and(|(completed, accessed)| accessed < completed)
                || generation.storage.as_ref().is_some_and(|storage| {
                    !storage.valid()
                        || storage.generation_id() != generation_id
                        || storage.operation_id() != generation.operation_id
                        || generation.size_bytes != Some(storage.size_bytes())
                })
                || generation.completion_sequence.is_some_and(|sequence| {
                    sequence == 0
                        || sequence > self.store_sequence
                        || !completion_sequences.insert(sequence)
                })
            {
                return Err(StoreError::Corrupt("recovery generation"));
            }
            let generation_operation = self.operation(&generation.operation_id)?;
            let previous_manifest = generation
                .previous_manifest
                .as_ref()
                .map(EncodedManifest::decode)
                .transpose()?;
            let target_manifest = generation.target_manifest.decode()?;
            if (generation.target_active_profile.is_some()
                != (generation_operation.request.intent().kind
                    == LifecycleOperationKind::ProfileSwitch))
                || generation
                    .previous_active_profile
                    .as_ref()
                    .is_some_and(|pointer| {
                        previous_manifest
                            .as_ref()
                            .is_none_or(|manifest| !pointer.matches_manifest(manifest))
                    })
                || generation
                    .target_active_profile
                    .as_ref()
                    .is_some_and(|pointer| !pointer.matches_manifest(&target_manifest))
            {
                return Err(StoreError::Corrupt("recovery generation profile binding"));
            }
            for operation_id in &generation.protected_by_operations {
                let protector = self.operation(operation_id)?;
                if protector.state.terminal()
                    || protector.request.intent().kind
                        != deltamod_product_contracts::LifecycleOperationKind::Recover
                    || protector.request.intent().installation_id != generation.installation_id
                {
                    return Err(StoreError::Corrupt("recovery generation protection"));
                }
            }
            if generation
                .manifest_only_observations
                .iter()
                .any(|observation| observation.validate().is_err())
            {
                return Err(StoreError::Corrupt("generation claim observations"));
            }
            if !generation.manifest_only_observations.is_empty() {
                let journal = self.journal(&generation.operation_id)?;
                let target = generation.target_manifest.decode()?;
                let target_claims: BTreeMap<_, _> = target
                    .ledger
                    .claims
                    .iter()
                    .map(|claim| (claim.path_identity_key.as_str(), claim))
                    .collect();
                let mutation_identities: BTreeSet<_> = journal
                    .mutations
                    .iter()
                    .map(|mutation| mutation.path_identity_key.as_str())
                    .collect();
                let mut observation_identities = BTreeSet::new();
                for observation in &generation.manifest_only_observations {
                    let claim = target_claims.get(observation.path_identity_key.as_str());
                    let state_matches_target = match (&observation.state, claim) {
                        (
                            ObservedFileState::Regular {
                                sha256,
                                link_count: 1,
                                ..
                            },
                            Some(claim),
                        ) => {
                            observation.path == claim.path
                                && sha256.eq_ignore_ascii_case(&claim.sha256)
                        }
                        (ObservedFileState::Missing, None) => true,
                        _ => false,
                    };
                    if observation.root_identity != journal.transaction_root
                        || mutation_identities.contains(observation.path_identity_key.as_str())
                        || !observation_identities.insert(observation.path_identity_key.as_str())
                        || !state_matches_target
                    {
                        return Err(StoreError::Corrupt("generation claim observation binding"));
                    }
                }
            }
        }
        for (generation_id, tombstone) in &self.pending_generation_deletions {
            let generation = self
                .generations
                .get(generation_id)
                .ok_or(StoreError::Corrupt("pending generation deletion"))?;
            if !tombstone.valid()
                || tombstone.generation_id() != generation_id
                || tombstone.installation_id() != generation.installation_id
                || tombstone.operation_id() != generation.operation_id
                || tombstone.completion_sequence() != generation.completion_sequence.unwrap_or(0)
                || tombstone.size_bytes() != generation.size_bytes.unwrap_or(0)
                || tombstone.last_accessed_at_ms() != generation.last_accessed_at_ms.unwrap_or(0)
                || tombstone.storage() != generation.storage.as_ref()
            {
                return Err(StoreError::Corrupt("pending generation deletion"));
            }
        }
        for (operation_id, commit) in &self.profile_switch_commits {
            commit
                .previous
                .as_ref()
                .map_or(Ok(()), ActiveProfilePointer::validate)?;
            commit.target.validate()?;
            let record = self.operation(operation_id)?;
            let journal = self.journal(operation_id)?;
            let generation = self
                .generations
                .get(&journal.recovery_generation_id)
                .ok_or(StoreError::Corrupt("profile switch generation"))?;
            let expected_active = if journal.manifest_commit_state == ManifestCommitState::Published
            {
                Some(&commit.target)
            } else {
                commit.previous.as_ref()
            };
            if commit.operation_id != *operation_id
                || !valid_sha256(&commit.plan_fingerprint)
                || record.state.terminal()
                || record.request.intent().kind != LifecycleOperationKind::ProfileSwitch
                || !record
                    .request
                    .intent()
                    .file_plan_fingerprint
                    .as_deref()
                    .is_some_and(|fingerprint| {
                        fingerprint.eq_ignore_ascii_case(&commit.plan_fingerprint)
                    })
                || journal.operation != LifecycleOperationKind::ProfileSwitch
                || journal.phase == deltamod_product_contracts::OperationPhase::Complete
                || commit.target.installation_id() != journal.installation_id
                || generation.previous_active_profile != commit.previous
                || generation.target_active_profile.as_ref() != Some(&commit.target)
                || self.active_profiles.get(&journal.installation_id) != expected_active
            {
                return Err(StoreError::Corrupt("profile switch commit"));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct StoreIdentityBinding {
    root: StableObjectIdentity,
    lock: Mutex<Option<StableObjectIdentity>>,
    log: Mutex<Option<StableObjectIdentity>>,
}

pub struct DurableLifecycleStore {
    root: PathBuf,
    log_path: PathBuf,
    lock_path: PathBuf,
    identity: StoreIdentityBinding,
    clock: Arc<MonotonicClock>,
    faults: Box<dyn FaultInjector>,
}

impl std::fmt::Debug for DurableLifecycleStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DurableLifecycleStore")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl DurableLifecycleStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_clock(root, Arc::new(SystemClock))
    }

    pub fn open_with_clock(
        root: impl AsRef<Path>,
        clock: Arc<dyn TrustedClock>,
    ) -> Result<Self, StoreError> {
        let root = root.as_ref();
        match fs::symlink_metadata(root) {
            Ok(_) => {
                inspect_path(root, StoreObjectKind::Directory)
                    .map_err(|error| map_identity_error(error, "unsafe store root"))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::create_dir_all(root)?;
        let root = fs::canonicalize(root)?;
        let root_identity = inspect_path(&root, StoreObjectKind::Directory)
            .map_err(|error| map_identity_error(error, "unsafe store root"))?;
        let store = Self {
            log_path: root.join("lifecycle-state.log"),
            lock_path: root.join("lifecycle-state.lock"),
            root,
            identity: StoreIdentityBinding {
                root: root_identity,
                lock: Mutex::new(None),
                log: Mutex::new(None),
            },
            clock: Arc::new(MonotonicClock::new(clock)),
            faults: Box::<NoFaults>::default(),
        };
        let (_lock, state) = store.lock_and_load()?;
        state.validate()?;
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }

    #[must_use]
    pub fn clock(&self) -> Arc<dyn TrustedClock> {
        self.clock.clone()
    }

    pub fn set_fault_injector(&mut self, faults: impl FaultInjector + 'static) {
        self.faults = Box::new(faults);
    }

    pub fn clear_fault_injector(&mut self) {
        self.faults = Box::<NoFaults>::default();
    }

    pub fn check_fault(&mut self, point: FaultPoint) -> Result<(), StoreError> {
        self.faults.check(&point).map_err(StoreError::from)
    }

    /// Atomically compacts the append log to one checksummed snapshot without
    /// resetting durable ordering. A crash before replacement leaves the old
    /// log authoritative; a crash after replacement leaves the compacted frame
    /// independently replayable. Other already-open store instances fail their
    /// identity fence and must reopen.
    pub fn compact_append_log(&mut self) -> Result<StoreLogCompaction, StoreError> {
        let (lock, mut state, _) = self.lock_and_load_authoritative()?;
        if state.store_sequence == 0 {
            return Ok(StoreLogCompaction {
                bytes_before: 0,
                bytes_after: 0,
                store_sequence: 0,
            });
        }
        let mut log_options = OpenOptions::new();
        log_options.read(true).write(true);
        configure_no_follow(&mut log_options);
        let log = log_options.open(&self.log_path).map_err(|error| {
            map_store_object_open_error(
                error,
                &self.log_path,
                StoreObjectKind::RegularFile,
                "store log",
            )
        })?;
        let old_identity = self.verify_and_bind_log(&log)?;
        let bytes_before = log.metadata()?.len();
        drop(log);

        let (temporary_path, _temporary_name, mut temporary) = (0_u8..32)
            .find_map(|attempt| {
                let name = format!(
                    ".lifecycle-state-compact-{}-{}-{attempt}.tmp",
                    state.store_sequence,
                    std::process::id()
                );
                let path = self.root.join(&name);
                let mut options = OpenOptions::new();
                options.read(true).write(true).create_new(true);
                configure_no_follow(&mut options);
                match options.open(&path) {
                    Ok(file) => Some(Ok((path, name, file))),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                    Err(error) => Some(Err(StoreError::Io(error))),
                }
            })
            .transpose()?
            .ok_or(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "no compact-log temporary name was available",
            )))?;
        let new_identity =
            verify_opened_path(&temporary_path, &temporary, StoreObjectKind::RegularFile)
                .map_err(|error| map_identity_error(error, "compact log temporary"))?;
        state.store_identity = Some(self.current_store_identity(new_identity.clone())?);
        let frame = encode_compacted_state_frame(&state)?;
        temporary.write_all(&frame)?;
        temporary.flush()?;
        temporary.sync_all()?;
        verify_opened_path(&temporary_path, &temporary, StoreObjectKind::RegularFile)
            .map_err(|error| map_identity_error(error, "compact log temporary"))?;
        drop(temporary);

        self.verify_root_identity()?;
        self.verify_and_bind_lock(&lock)?;
        let current_log = log_options.open(&self.log_path).map_err(|error| {
            map_store_object_open_error(
                error,
                &self.log_path,
                StoreObjectKind::RegularFile,
                "store log",
            )
        })?;
        if self.verify_and_bind_log(&current_log)? != old_identity {
            return Err(StoreError::StoreIdentityChanged("store log"));
        }
        drop(current_log);

        #[cfg(windows)]
        {
            let root = fence_windows::MutationRoot::open(&self.root)
                .map_err(|_| StoreError::StoreIdentityChanged("store root"))?;
            root.into_directory()
                .replace_child(
                    std::ffi::OsStr::new(&_temporary_name),
                    std::ffi::OsStr::new("lifecycle-state.log"),
                )
                .map_err(|_| StoreError::StoreIdentityChanged("store log"))?;
        }
        #[cfg(unix)]
        {
            fs::rename(&temporary_path, &self.log_path)?;
        }
        sync_store_directory(&self.root)?;
        let compacted = log_options.open(&self.log_path).map_err(|error| {
            map_store_object_open_error(
                error,
                &self.log_path,
                StoreObjectKind::RegularFile,
                "store log",
            )
        })?;
        let observed = verify_opened_path(&self.log_path, &compacted, StoreObjectKind::RegularFile)
            .map_err(|error| map_identity_error(error, "compacted store log"))?;
        if observed != new_identity {
            return Err(StoreError::StoreIdentityChanged("compacted store log"));
        }
        let bytes_after = compacted.metadata()?.len();
        drop(compacted);
        self.replace_bound_log_identity(&old_identity, new_identity)?;
        self.verify_persisted_identity(&state)?;
        Ok(StoreLogCompaction {
            bytes_before,
            bytes_after,
            store_sequence: state.store_sequence,
        })
    }

    fn verify_root_identity(&self) -> Result<(), StoreError> {
        let current = inspect_path(&self.root, StoreObjectKind::Directory)
            .map_err(|error| map_bound_identity_error(error, "store root"))?;
        if current != self.identity.root {
            return Err(StoreError::StoreIdentityChanged("store root"));
        }
        Ok(())
    }

    fn verify_and_bind_lock(&self, lock: &File) -> Result<StableObjectIdentity, StoreError> {
        let current = verify_opened_path(&self.lock_path, lock, StoreObjectKind::RegularFile)
            .map_err(|error| map_bound_identity_error(error, "store lock"))?;
        bind_identity(&self.identity.lock, &current, "store lock")?;
        Ok(current)
    }

    fn verify_and_bind_log(&self, log: &File) -> Result<StableObjectIdentity, StoreError> {
        let current = verify_opened_path(&self.log_path, log, StoreObjectKind::RegularFile)
            .map_err(|error| map_bound_identity_error(error, "store log"))?;
        bind_identity(&self.identity.log, &current, "store log")?;
        Ok(current)
    }

    fn replace_bound_log_identity(
        &self,
        expected: &StableObjectIdentity,
        replacement: StableObjectIdentity,
    ) -> Result<(), StoreError> {
        let mut binding = self
            .identity
            .log
            .lock()
            .map_err(|_| StoreError::StoreIdentityChanged("store log binding"))?;
        if binding.as_ref() != Some(expected) {
            return Err(StoreError::StoreIdentityChanged("store log binding"));
        }
        *binding = Some(replacement);
        Ok(())
    }

    fn bound_log_identity(&self) -> Result<Option<StableObjectIdentity>, StoreError> {
        self.identity
            .log
            .lock()
            .map(|identity| identity.clone())
            .map_err(|_| StoreError::StoreIdentityChanged("store log binding"))
    }

    fn current_store_identity(
        &self,
        log: StableObjectIdentity,
    ) -> Result<PersistedStoreIdentity, StoreError> {
        let lock = self
            .identity
            .lock
            .lock()
            .map_err(|_| StoreError::StoreIdentityChanged("store lock binding"))?
            .clone()
            .ok_or(StoreError::StoreIdentityChanged("store lock binding"))?;
        Ok(PersistedStoreIdentity {
            root: self.identity.root.clone(),
            lock,
            log,
        })
    }

    fn verify_persisted_identity(&self, state: &PersistedState) -> Result<(), StoreError> {
        let Some(expected) = &state.store_identity else {
            return Ok(());
        };
        let lock = self
            .identity
            .lock
            .lock()
            .map_err(|_| StoreError::StoreIdentityChanged("store lock binding"))?
            .clone();
        let log = self.bound_log_identity()?;
        if expected.root != self.identity.root
            || lock.as_ref() != Some(&expected.lock)
            || log.as_ref() != Some(&expected.log)
        {
            return Err(StoreError::StoreIdentityChanged("persisted store identity"));
        }
        Ok(())
    }

    fn lock_and_load(&self) -> Result<(File, PersistedState), StoreError> {
        self.verify_root_identity()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        configure_no_follow(&mut options);
        let lock = options.open(&self.lock_path).map_err(|error| {
            map_store_object_open_error(
                error,
                &self.lock_path,
                StoreObjectKind::RegularFile,
                "store lock",
            )
        })?;
        self.verify_and_bind_lock(&lock)?;
        lock.lock()?;
        self.verify_root_identity()?;
        self.verify_and_bind_lock(&lock)?;
        let (state, valid_length, log) = self.load_locked()?;
        self.clock.raise_floor(state.trusted_time_floor_ms);
        self.verify_persisted_identity(&state)?;
        if let Some(log) = log {
            let metadata = log.metadata()?;
            if metadata.len() > valid_length {
                self.verify_root_identity()?;
                self.verify_and_bind_lock(&lock)?;
                self.verify_and_bind_log(&log)?;
                log.set_len(valid_length)?;
                log.sync_all()?;
                self.verify_and_bind_log(&log)?;
            }
        }
        Ok((lock, state))
    }

    fn load_locked(&self) -> Result<(PersistedState, u64, Option<File>), StoreError> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        configure_no_follow(&mut options);
        let mut file = match options.open(&self.log_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if self.bound_log_identity()?.is_some() {
                    return Err(StoreError::StoreIdentityChanged("store log missing"));
                }
                return Ok((PersistedState::default(), 0, None));
            }
            Err(error) => {
                return Err(map_store_object_open_error(
                    error,
                    &self.log_path,
                    StoreObjectKind::RegularFile,
                    "store log",
                ));
            }
        };
        self.verify_and_bind_log(&file)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        self.verify_and_bind_log(&file)?;
        let (latest, offset) = replay_frames(&bytes, 1)?;
        Ok((
            latest,
            u64::try_from(offset).map_err(|_| StoreError::Corrupt("log length"))?,
            Some(file),
        ))
    }

    fn append_frame_locked(
        &mut self,
        lock: &File,
        state: &mut PersistedState,
    ) -> Result<(), StoreError> {
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        configure_no_follow(&mut options);
        let mut file = options.open(&self.log_path).map_err(|error| {
            map_store_object_open_error(
                error,
                &self.log_path,
                StoreObjectKind::RegularFile,
                "store log",
            )
        })?;
        let log_identity = self.verify_and_bind_log(&file)?;
        state.store_identity = Some(self.current_store_identity(log_identity)?);
        state.store_sequence = next_store_sequence(state.store_sequence)?;
        state.validate()?;
        let frame = encode_state_frame(state)?;
        self.verify_root_identity()?;
        self.verify_and_bind_lock(lock)?;
        self.verify_and_bind_log(&file)?;
        file.write_all(&frame)?;
        file.flush()?;
        file.sync_all()?;
        #[cfg(not(windows))]
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    fn persist_trusted_time_locked(
        &mut self,
        lock: &File,
        state: &mut PersistedState,
    ) -> Result<u64, StoreError> {
        let now_ms = self.now_ms();
        let previous_floor = state.trusted_time_floor_ms;
        state.trusted_time_floor_ms = state.trusted_time_floor_ms.max(now_ms);
        if previous_floor < now_ms
            && state
                .active_leases
                .values()
                .any(|lease| !lease.active_at(now_ms))
        {
            self.append_frame_locked(lock, state)?;
        }
        Ok(state.trusted_time_floor_ms)
    }

    fn persist_trusted_time_from_durable_locked(
        &mut self,
        lock: &File,
        now_ms: u64,
    ) -> Result<(), StoreError> {
        let (mut durable, _, _) = self.load_locked()?;
        self.verify_persisted_identity(&durable)?;
        if durable.trusted_time_floor_ms < now_ms {
            durable.trusted_time_floor_ms = now_ms;
            self.append_frame_locked(lock, &mut durable)?;
        }
        Ok(())
    }

    fn lock_and_load_authoritative(&mut self) -> Result<(File, PersistedState, u64), StoreError> {
        let (lock, mut state) = self.lock_and_load()?;
        let now_ms = self.persist_trusted_time_locked(&lock, &mut state)?;
        Ok((lock, state, now_ms))
    }

    fn append_locked_with_validation<F>(
        &mut self,
        lock: &File,
        state: &mut PersistedState,
        checkpoint: Option<JournalCheckpointKind>,
        authority: Option<&OperationLease>,
        validate: F,
    ) -> Result<(), StoreError>
    where
        F: FnOnce() -> Result<(), StoreError>,
    {
        if let Some(kind) = &checkpoint {
            self.check_fault(FaultPoint::BeforeJournalCas(kind.clone()))?;
        }
        let now_ms = self.now_ms();
        if authority.is_some_and(|lease| !lease.active_at(now_ms)) {
            self.persist_trusted_time_from_durable_locked(lock, now_ms)?;
            return Err(StoreError::LostLease);
        }
        state.trusted_time_floor_ms = state.trusted_time_floor_ms.max(now_ms);
        validate()?;
        self.verify_root_identity()?;
        self.verify_and_bind_lock(lock)?;
        let final_now_ms = self.now_ms();
        if authority.is_some_and(|lease| !lease.active_at(final_now_ms)) {
            self.persist_trusted_time_from_durable_locked(lock, final_now_ms)?;
            return Err(StoreError::LostLease);
        }
        state.trusted_time_floor_ms = state.trusted_time_floor_ms.max(final_now_ms);
        self.append_frame_locked(lock, state)?;
        if let Some(kind) = checkpoint {
            self.check_fault(FaultPoint::AfterJournalCas(kind))?;
        }
        Ok(())
    }

    fn append_locked(
        &mut self,
        lock: &File,
        state: &mut PersistedState,
        checkpoint: Option<JournalCheckpointKind>,
        authority: Option<&OperationLease>,
    ) -> Result<(), StoreError> {
        self.append_locked_with_validation(lock, state, checkpoint, authority, || Ok(()))
    }

    pub fn operation_by_id(
        &self,
        operation_id: &str,
    ) -> Result<Option<OperationRecord>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        state
            .operations
            .get(operation_id)
            .map(|encoded| {
                decode_current::<OperationRecord>(encoded.as_bytes()).map_err(StoreError::from)
            })
            .transpose()
    }

    pub fn operations(&self) -> Result<Vec<OperationRecord>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        let mut operations = state
            .operations
            .values()
            .map(|encoded| {
                decode_current::<OperationRecord>(encoded.as_bytes()).map_err(StoreError::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        operations.sort_by(|left, right| {
            right.updated_at_ms.cmp(&left.updated_at_ms).then_with(|| {
                left.request
                    .operation_id()
                    .cmp(right.request.operation_id())
            })
        });
        Ok(operations)
    }

    pub fn operation_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<OperationRecord>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        state
            .idempotency
            .get(idempotency_key)
            .map(|operation_id| state.operation(operation_id))
            .transpose()
    }

    pub fn journal_by_operation(
        &self,
        operation_id: &str,
    ) -> Result<Option<LifecycleJournal>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        state
            .journals
            .get(operation_id)
            .map(|encoded| {
                decode_current::<LifecycleJournal>(encoded.as_bytes()).map_err(StoreError::from)
            })
            .transpose()
    }

    pub fn journals(&self) -> Result<Vec<LifecycleJournal>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        let mut journals = state
            .journals
            .values()
            .map(|encoded| {
                decode_current::<LifecycleJournal>(encoded.as_bytes()).map_err(StoreError::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        journals.sort_by(|left, right| {
            right
                .updated_at_ms
                .cmp(&left.updated_at_ms)
                .then_with(|| left.operation_id.cmp(&right.operation_id))
        });
        Ok(journals)
    }

    pub fn manifest(
        &self,
        installation_id: &str,
    ) -> Result<Option<InstallationManifest>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        state.manifest(installation_id)
    }

    /// Returns the profile pointer atomically associated with the currently
    /// published manifest, if the installation is profile-managed.
    pub fn active_profile(
        &self,
        installation_id: &str,
    ) -> Result<Option<ActiveProfilePointer>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        Ok(state.active_profiles.get(installation_id).cloned())
    }

    pub fn latest_recovery_generation(
        &self,
        installation_id: &str,
    ) -> Result<Option<RecoveryGenerationSnapshot>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        let generation = state
            .generations
            .values()
            .filter(|generation| {
                generation.installation_id == installation_id
                    && generation.viable
                    && generation.completed_at_ms.is_some()
                    && generation.completion_sequence.is_some()
                    && !state
                        .pending_generation_deletions
                        .contains_key(&generation.generation_id)
            })
            .max_by_key(|generation| generation.completion_sequence);
        generation.map(decode_generation).transpose()
    }

    /// Records a successful read of a recovery generation for persisted LRU
    /// ordering. The expected completion sequence prevents an access update
    /// from reviving or touching a replaced/deleting generation.
    pub fn touch_recovery_generation(
        &mut self,
        generation_id: &str,
        expected_completion_sequence: u64,
    ) -> Result<RecoveryGenerationSnapshot, StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if state
            .pending_generation_deletions
            .contains_key(generation_id)
        {
            return Err(StoreError::StaleRevision);
        }
        let generation = state
            .generations
            .get_mut(generation_id)
            .ok_or(StoreError::NotFound)?;
        if generation.completion_sequence != Some(expected_completion_sequence)
            || !generation.viable
        {
            return Err(StoreError::StaleRevision);
        }
        generation.last_accessed_at_ms = Some(
            generation
                .last_accessed_at_ms
                .unwrap_or(generation.completed_at_ms.unwrap_or_default())
                .max(now_ms),
        );
        let snapshot = decode_generation(generation)?;
        self.append_locked(&lock, &mut state, None, None)?;
        Ok(snapshot)
    }

    /// Persists the measured recovery-workspace size and exact object identity.
    /// This binding is immutable once recorded; a replay with the same metadata
    /// is idempotent, while a changed path/object/size fails closed.
    pub fn bind_recovery_generation_storage(
        &mut self,
        storage: RecoveryGenerationStorage,
    ) -> Result<RecoveryGenerationSnapshot, StoreError> {
        if !storage.valid() {
            return Err(StoreError::RecoveryStorageChanged);
        }
        let (lock, mut state, _) = self.lock_and_load_authoritative()?;
        if state
            .pending_generation_deletions
            .contains_key(storage.generation_id())
        {
            return Err(StoreError::RetentionRace);
        }
        let generation = state
            .generations
            .get_mut(storage.generation_id())
            .ok_or(StoreError::NotFound)?;
        if generation.operation_id != storage.operation_id()
            || !generation.viable
            || generation.completed_at_ms.is_none()
            || generation.completion_sequence.is_none()
        {
            return Err(StoreError::RecoveryStorageChanged);
        }
        match &generation.storage {
            Some(existing) if existing == &storage => return decode_generation(generation),
            Some(_) => return Err(StoreError::RecoveryStorageChanged),
            None => {}
        }
        generation.size_bytes = Some(storage.size_bytes());
        generation.last_accessed_at_ms = Some(
            generation
                .last_accessed_at_ms
                .unwrap_or(generation.completed_at_ms.unwrap_or_default()),
        );
        generation.storage = Some(storage);
        let snapshot = decode_generation(generation)?;
        self.append_locked(&lock, &mut state, None, None)?;
        Ok(snapshot)
    }

    /// Returns the authoritative retention view. Pending deletions are excluded
    /// from generation candidates and still contribute source-operation
    /// references until final metadata removal.
    pub fn retention_snapshot(&self) -> Result<RetentionSnapshot, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        retention_snapshot_from_state(&state)
    }

    /// Returns completed, viable journal-backed generations whose immutable
    /// workspace binding must be measured or revalidated before planning.
    pub fn recovery_generation_storage_requests(
        &self,
    ) -> Result<Vec<(String, String, String)>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        Ok(state
            .generations
            .values()
            .filter(|generation| {
                generation.viable
                    && generation.completed_at_ms.is_some()
                    && state.journals.contains_key(&generation.operation_id)
                    && !state
                        .pending_generation_deletions
                        .contains_key(&generation.generation_id)
            })
            .map(|generation| {
                (
                    generation.generation_id.clone(),
                    generation.operation_id.clone(),
                    generation.installation_id.clone(),
                )
            })
            .collect())
    }

    /// Durably prepares a generation deletion. No filesystem namespace change
    /// may occur before this tombstone is committed.
    pub fn prepare_recovery_generation_deletion(
        &mut self,
        expected: &RecoveryGenerationEntry,
    ) -> Result<PendingRecoveryDeletion, StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if let Some(existing) = state
            .pending_generation_deletions
            .get(&expected.generation.generation_id)
        {
            if pending_matches_entry(existing, expected) {
                return Ok(existing.clone());
            }
            return Err(StoreError::PendingDeletionConflict);
        }
        let generation = state
            .generations
            .get(&expected.generation.generation_id)
            .ok_or(StoreError::NotFound)?;
        if generation_retention_entry(&state, generation)? != *expected
            || generation_retention_protected(&state, generation)?
        {
            return Err(StoreError::RetentionRace);
        }
        let tombstone = PendingRecoveryDeletion::new(expected, generation.storage.clone(), now_ms);
        state
            .pending_generation_deletions
            .insert(expected.generation.generation_id.clone(), tombstone.clone());
        self.append_locked(&lock, &mut state, None, None)?;
        Ok(tombstone)
    }

    /// Lists durable deletion work in deterministic generation-ID order for
    /// startup reconciliation.
    pub fn pending_recovery_deletions(&self) -> Result<Vec<PendingRecoveryDeletion>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        Ok(state
            .pending_generation_deletions
            .values()
            .cloned()
            .collect())
    }

    /// Commits the identity-bound purge result while retaining the filesystem
    /// marker shell as a crash witness.
    pub fn mark_recovery_generation_purged(
        &mut self,
        expected: &PendingRecoveryDeletion,
        receipt: &RecoveryPurgeReceipt,
    ) -> Result<PendingRecoveryDeletion, StoreError> {
        let (lock, mut state, _) = self.lock_and_load_authoritative()?;
        let current = state
            .pending_generation_deletions
            .get_mut(expected.generation_id())
            .ok_or(StoreError::NotFound)?;
        if current != expected || !receipt.matches(current) {
            return Err(StoreError::PendingDeletionConflict);
        }
        if current.phase() == RecoveryDeletionPhase::Prepared {
            current.mark_purged();
            let updated = current.clone();
            self.append_locked(&lock, &mut state, None, None)?;
            return Ok(updated);
        }
        Ok(current.clone())
    }

    /// Finalizes a two-phase deletion after the exact quarantine shell is
    /// absent. Generation metadata goes first; operation/idempotency/journal
    /// records are removed only when no durable structure still references the
    /// source operation.
    pub fn finalize_recovery_generation_deletion(
        &mut self,
        expected: &PendingRecoveryDeletion,
        receipt: &RecoveryRemovalReceipt,
    ) -> Result<(), StoreError> {
        let (lock, mut state, _) = self.lock_and_load_authoritative()?;
        let current = state
            .pending_generation_deletions
            .get(expected.generation_id())
            .ok_or(StoreError::NotFound)?;
        if current != expected
            || current.phase() != RecoveryDeletionPhase::Purged
            || !receipt.matches(current)
        {
            return Err(StoreError::PendingDeletionConflict);
        }
        state.generations.remove(expected.generation_id());
        state
            .pending_generation_deletions
            .remove(expected.generation_id());
        self.append_locked(&lock, &mut state, None, None)?;
        Ok(())
    }

    /// Deletes one stale terminal operation and its idempotency/journal index
    /// atomically when the expected snapshot is still current and no generation
    /// or pending deletion references it.
    pub fn delete_operation_history(
        &mut self,
        expected: &OperationHistoryEntry,
    ) -> Result<(), StoreError> {
        let (lock, mut state, _) = self.lock_and_load_authoritative()?;
        let current = operation_history_entry(&state, expected.operation_id.as_str())?;
        if current != *expected || operation_referenced(&state, &expected.operation_id) {
            return Err(StoreError::RetentionRace);
        }
        remove_operation_bundle(&mut state, &expected.operation_id)?;
        self.append_locked(&lock, &mut state, None, None)?;
        Ok(())
    }

    pub(crate) fn restore_source_candidates(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
    ) -> Result<Vec<RecoveryGenerationSnapshot>, StoreError> {
        if record.request.intent().kind
            != deltamod_product_contracts::LifecycleOperationKind::Recover
        {
            return Err(StoreError::InvalidTransition);
        }
        let (lock, state, now_ms) = self.lock_and_load_authoritative()?;
        require_record_and_lease(&state, record, lease, now_ms)?;
        let operation_id = record.request.operation_id();
        let mut already_protected = state
            .generations
            .values()
            .filter(|generation| generation.protected_by_operations.contains(operation_id));
        if let Some(generation) = already_protected.next() {
            if already_protected.next().is_some() {
                return Err(StoreError::Corrupt("multiple restore source protections"));
            }
            let snapshot = decode_generation(generation)?;
            drop(lock);
            return Ok(vec![snapshot]);
        }
        let mut candidates = Vec::new();
        for generation in state.generations.values() {
            if generation.installation_id != lease.installation_id
                || !generation.viable
                || generation.completed_at_ms.is_none()
                || state
                    .pending_generation_deletions
                    .contains_key(&generation.generation_id)
            {
                continue;
            }
            let Some(completion_sequence) = generation.completion_sequence else {
                continue;
            };
            let source_operation = state.operation(&generation.operation_id)?;
            if source_operation.request.intent().kind
                == deltamod_product_contracts::LifecycleOperationKind::Recover
            {
                continue;
            }
            candidates.push((completion_sequence, decode_generation(generation)?));
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        drop(lock);
        Ok(candidates
            .into_iter()
            .map(|(_, generation)| generation)
            .collect())
    }

    pub(crate) fn protect_restore_source(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        expected: &RecoveryGenerationSnapshot,
    ) -> Result<(), StoreError> {
        if record.request.intent().kind
            != deltamod_product_contracts::LifecycleOperationKind::Recover
        {
            return Err(StoreError::InvalidTransition);
        }
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        require_record_and_lease(&state, record, lease, now_ms)?;
        let operation_id = record.request.operation_id();
        let mut already_protected = state
            .generations
            .values()
            .filter(|generation| generation.protected_by_operations.contains(operation_id));
        if let Some(generation) = already_protected.next() {
            if already_protected.next().is_some() {
                return Err(StoreError::Corrupt("multiple restore source protections"));
            }
            if decode_generation(generation)? != *expected {
                return Err(StoreError::StaleRevision);
            }
            drop(lock);
            return Ok(());
        }
        let generation = state
            .generations
            .get(&expected.generation_id)
            .ok_or(StoreError::NotFound)?;
        if generation.installation_id != lease.installation_id
            || !generation.viable
            || generation.completed_at_ms.is_none()
            || generation.completion_sequence.is_none()
            || decode_generation(generation)? != *expected
        {
            return Err(StoreError::StaleRevision);
        }
        let source_operation = state.operation(&generation.operation_id)?;
        if source_operation.request.intent().kind
            == deltamod_product_contracts::LifecycleOperationKind::Recover
        {
            return Err(StoreError::StaleRevision);
        }
        state
            .generations
            .get_mut(&expected.generation_id)
            .ok_or(StoreError::NotFound)?
            .protected_by_operations
            .insert(operation_id.to_owned());
        if let Some(generation) = state.generations.get_mut(&expected.generation_id) {
            generation.last_accessed_at_ms = Some(
                generation
                    .last_accessed_at_ms
                    .unwrap_or(generation.completed_at_ms.unwrap_or_default())
                    .max(now_ms),
            );
        }
        self.append_locked(&lock, &mut state, None, Some(lease))?;
        drop(lock);
        Ok(())
    }

    pub(crate) fn generation_manifest_only_observations(
        &self,
        generation_id: &str,
    ) -> Result<Vec<ObservationSnapshot>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        state
            .generations
            .get(generation_id)
            .map(|generation| generation.manifest_only_observations.clone())
            .ok_or(StoreError::NotFound)
    }

    pub fn interrupted_operations(&self) -> Result<Vec<InterruptedOperation>, StoreError> {
        let (_lock, state) = self.lock_and_load()?;
        let mut interrupted = Vec::new();
        for lease in state.active_leases.values() {
            let record = state.operation(&lease.operation_id)?;
            if !record.state.terminal() {
                interrupted.push(InterruptedOperation {
                    journal: state
                        .journals
                        .get(lease.operation_id.as_str())
                        .map(|encoded| decode_current(encoded.as_bytes()).map_err(StoreError::from))
                        .transpose()?,
                    record,
                    lease: lease.clone(),
                });
            }
        }
        interrupted.sort_by(|left, right| {
            left.record
                .request
                .operation_id()
                .cmp(right.record.request.operation_id())
        });
        Ok(interrupted)
    }

    pub fn assert_lease_current(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
    ) -> Result<(), StoreError> {
        let (lock, state, now_ms) = self.lock_and_load_authoritative()?;
        let result = require_record_and_lease(&state, record, lease, now_ms);
        drop(lock);
        result
    }

    // These arguments form one durable compare-and-swap boundary; grouping them
    // would obscure the operation/lease bindings enforced below.
    #[allow(clippy::too_many_arguments)]
    pub fn create_journal(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        previous_manifest: Option<&InstallationManifest>,
        target_manifest: &InstallationManifest,
        manifest_only_observations: &[ObservationSnapshot],
        _now_ms: u64,
    ) -> Result<(), StoreError> {
        self.create_journal_bound(
            record,
            lease,
            journal,
            previous_manifest,
            target_manifest,
            manifest_only_observations,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn create_profile_switch_journal(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        previous_manifest: Option<&InstallationManifest>,
        target_manifest: &InstallationManifest,
        manifest_only_observations: &[ObservationSnapshot],
        plan_fingerprint: &str,
        previous_profile: Option<&ActiveProfilePointer>,
        target_profile: &ActiveProfilePointer,
    ) -> Result<(), StoreError> {
        let commit = StoredProfileSwitchCommit {
            operation_id: record.request.operation_id().to_owned(),
            plan_fingerprint: plan_fingerprint.to_owned(),
            previous: previous_profile.cloned(),
            target: target_profile.clone(),
        };
        self.create_journal_bound(
            record,
            lease,
            journal,
            previous_manifest,
            target_manifest,
            manifest_only_observations,
            Some(commit),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_journal_bound(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        previous_manifest: Option<&InstallationManifest>,
        target_manifest: &InstallationManifest,
        manifest_only_observations: &[ObservationSnapshot],
        profile_commit: Option<StoredProfileSwitchCommit>,
    ) -> Result<(), StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        validate_journal_binding(journal, record, lease, now_ms)?;
        if journal.journal_sequence != 1
            || journal.manifest_generation_before
                != previous_manifest.map_or(0, InstallationManifest::generation)
            || journal.manifest_generation_after != target_manifest.generation()
            || journal.installation_id != target_manifest.installation_id()
            || ((journal.operation == LifecycleOperationKind::ProfileSwitch)
                != profile_commit.is_some())
        {
            return Err(StoreError::InvalidTransition);
        }
        let current = state.operation(record.request.operation_id())?;
        require_record_and_lease(&state, &current, lease, now_ms)?;
        if current != *record
            || state.journals.contains_key(record.request.operation_id())
            || state
                .generations
                .contains_key(&journal.recovery_generation_id)
            || state
                .profile_switch_commits
                .contains_key(record.request.operation_id())
            || state.manifest(&journal.installation_id)? != previous_manifest.cloned()
        {
            return Err(StoreError::StaleJournal);
        }
        if let Some(commit) = &profile_commit {
            commit
                .previous
                .as_ref()
                .map_or(Ok(()), ActiveProfilePointer::validate)
                .map_err(|_| StoreError::InvalidTransition)?;
            commit
                .target
                .validate()
                .map_err(|_| StoreError::InvalidTransition)?;
            if !valid_sha256(&commit.plan_fingerprint)
                || !record
                    .request
                    .intent()
                    .file_plan_fingerprint
                    .as_deref()
                    .is_some_and(|fingerprint| {
                        fingerprint.eq_ignore_ascii_case(&commit.plan_fingerprint)
                    })
                || state.active_profiles.get(&journal.installation_id) != commit.previous.as_ref()
                || !commit.target.matches_manifest(target_manifest)
            {
                return Err(StoreError::InvalidTransition);
            }
        }
        state.journals.insert(
            journal.operation_id.clone(),
            serde_json::to_string(journal)?,
        );
        state.generations.insert(
            journal.recovery_generation_id.clone(),
            StoredGeneration {
                generation_id: journal.recovery_generation_id.clone(),
                installation_id: journal.installation_id.clone(),
                operation_id: journal.operation_id.clone(),
                previous_manifest: previous_manifest.map(EncodedManifest::encode).transpose()?,
                target_manifest: EncodedManifest::encode(target_manifest)?,
                started_at_ms: journal.started_at_ms,
                completed_at_ms: None,
                completion_sequence: None,
                size_bytes: None,
                last_accessed_at_ms: Some(journal.started_at_ms),
                storage: None,
                viable: false,
                pinned: journal.pinned,
                protected_by_operations: BTreeSet::new(),
                manifest_only_observations: manifest_only_observations.to_vec(),
                previous_active_profile: profile_commit
                    .as_ref()
                    .and_then(|commit| commit.previous.clone()),
                target_active_profile: profile_commit.as_ref().map(|commit| commit.target.clone()),
            },
        );
        if let Some(commit) = profile_commit {
            state
                .profile_switch_commits
                .insert(journal.operation_id.clone(), commit);
        }
        let result = self.append_locked(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::JournalCreated),
            Some(lease),
        );
        drop(lock);
        result
    }

    pub fn checkpoint_mutation(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        current: &LifecycleJournal,
        next: &LifecycleJournal,
        mutation_index: usize,
        _now_ms: u64,
    ) -> Result<(), StoreError> {
        let validation_now_ms = lease.acquired_at_ms;
        validate_journal_binding(current, record, lease, validation_now_ms)?;
        validate_journal_binding(next, record, lease, validation_now_ms)?;
        let current_mutation = current
            .mutations
            .get(mutation_index)
            .ok_or(StoreError::InvalidTransition)?;
        let next_mutation = next
            .mutations
            .get(mutation_index)
            .ok_or(StoreError::InvalidTransition)?;
        let allowed = match (current_mutation.checkpoint, next_mutation.checkpoint) {
            (MutationCheckpoint::Planned, MutationCheckpoint::Staged) => {
                current_mutation.action != deltamod_product_contracts::MutationAction::Delete
                    && next_mutation.staging_sha256 == next_mutation.expected_sha256
            }
            (MutationCheckpoint::Staged, MutationCheckpoint::BackupVerified) => {
                current_mutation.action == deltamod_product_contracts::MutationAction::Replace
                    && next_mutation.backup_sha256 == next_mutation.previous_sha256
            }
            (MutationCheckpoint::Planned, MutationCheckpoint::BackupVerified) => {
                current_mutation.action == deltamod_product_contracts::MutationAction::Delete
                    && next_mutation.backup_sha256 == next_mutation.previous_sha256
            }
            (MutationCheckpoint::Applied, MutationCheckpoint::OutputVerified) => true,
            _ => false,
        };
        let mut expected = current.clone().into_payload();
        expected.journal_sequence = next.journal_sequence;
        expected.updated_at_ms = next.updated_at_ms;
        expected.mutations[mutation_index] = next_mutation.clone();
        if !allowed
            || current.journal_sequence.checked_add(1) != Some(next.journal_sequence)
            || next.updated_at_ms < current.updated_at_ms
            || next.clone().into_payload() != expected
        {
            return Err(StoreError::InvalidTransition);
        }
        self.cas_journal_locked(
            record,
            lease,
            current,
            next,
            validation_now_ms,
            JournalCheckpointKind::Mutation {
                index: next_mutation.index,
                checkpoint: next_mutation.checkpoint,
            },
        )
    }

    pub fn transition_phase(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        next_phase: deltamod_product_contracts::OperationPhase,
        _now_ms: u64,
    ) -> Result<(OperationRecord, LifecycleJournal), StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        use deltamod_product_contracts::OperationPhase;
        validate_journal_binding(journal, record, lease, now_ms)?;
        let allowed = matches!(
            (journal.phase, next_phase),
            (OperationPhase::Preflight, OperationPhase::Staging)
                | (OperationPhase::Staging, OperationPhase::BackingUp)
                | (OperationPhase::BackingUp, OperationPhase::Applying)
                | (OperationPhase::Applying, OperationPhase::Verifying)
                | (OperationPhase::Verifying, OperationPhase::Committing)
                | (OperationPhase::Committing, OperationPhase::CleaningUp)
                | (OperationPhase::RollingBack, OperationPhase::CleaningUp)
        );
        if !allowed || record.state.terminal() {
            return Err(StoreError::InvalidTransition);
        }
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut next_record_payload = record.clone().into_payload();
        next_record_payload.phase = next_phase;
        next_record_payload.revision = next_revision;
        next_record_payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        let next_record = OperationRecord::new(next_record_payload)?;
        let mut next_journal_payload = journal.clone().into_payload();
        next_journal_payload.phase = next_phase;
        next_journal_payload.operation_revision = next_revision;
        next_journal_payload.journal_sequence = journal
            .journal_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        next_journal_payload.updated_at_ms = now_ms.max(journal.updated_at_ms);
        let next_journal = LifecycleJournal::new(next_journal_payload)?;
        validate_journal_binding(&next_journal, &next_record, lease, now_ms)?;
        require_exact_binding(&state, record, lease, journal, now_ms)?;
        state.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&next_record)?,
        );
        state.journals.insert(
            journal.operation_id.clone(),
            serde_json::to_string(&next_journal)?,
        );
        let result = self.append_locked(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::Phase(next_phase)),
            Some(lease),
        );
        drop(lock);
        result.map(|()| (next_record, next_journal))
    }

    pub fn write_manifest_temporary(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        _now_ms: u64,
    ) -> Result<LifecycleJournal, StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        validate_journal_binding(journal, record, lease, now_ms)?;
        if journal.phase != deltamod_product_contracts::OperationPhase::Committing
            || journal.manifest_commit_state != ManifestCommitState::NotStarted
            || !journal
                .mutations
                .iter()
                .all(|mutation| mutation.checkpoint == MutationCheckpoint::OutputVerified)
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut payload = journal.clone().into_payload();
        payload.journal_sequence = journal
            .journal_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        payload.updated_at_ms = now_ms.max(journal.updated_at_ms);
        payload.manifest_commit_state = ManifestCommitState::TemporaryWritten;
        let next = LifecycleJournal::new(payload)?;
        validate_journal_binding(&next, record, lease, now_ms)?;
        require_exact_binding(&state, record, lease, journal, now_ms)?;
        let generation = state
            .generations
            .get(&journal.recovery_generation_id)
            .ok_or(StoreError::Corrupt("journal generation"))?;
        state.pending_manifests.insert(
            journal.operation_id.clone(),
            generation.target_manifest.clone(),
        );
        state
            .journals
            .insert(journal.operation_id.clone(), serde_json::to_string(&next)?);
        let result = self.append_locked(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::ManifestTemporary),
            Some(lease),
        );
        drop(lock);
        result.map(|()| next)
    }

    pub fn publish_manifest<F>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        _now_ms: u64,
        validate_claims: F,
    ) -> Result<LifecycleJournal, StoreError>
    where
        F: FnOnce() -> Result<(), FilesystemBoundaryError>,
    {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        validate_journal_binding(journal, record, lease, now_ms)?;
        if journal.phase != deltamod_product_contracts::OperationPhase::Committing
            || journal.manifest_commit_state != ManifestCommitState::TemporaryWritten
        {
            return Err(StoreError::InvalidTransition);
        }
        let mut payload = journal.clone().into_payload();
        payload.journal_sequence = journal
            .journal_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        payload.updated_at_ms = now_ms.max(journal.updated_at_ms);
        payload.manifest_commit_state = ManifestCommitState::Published;
        let next = LifecycleJournal::new(payload)?;
        validate_journal_binding(&next, record, lease, now_ms)?;
        require_exact_binding(&state, record, lease, journal, now_ms)?;
        let pending = state
            .pending_manifests
            .remove(&journal.operation_id)
            .ok_or(StoreError::Corrupt("missing temporary manifest"))?;
        let target_manifest = pending.decode()?;
        if target_manifest.generation() != journal.manifest_generation_after {
            return Err(StoreError::Corrupt("temporary manifest generation"));
        }
        if journal.operation == LifecycleOperationKind::ProfileSwitch {
            let commit = state
                .profile_switch_commits
                .get(&journal.operation_id)
                .ok_or(StoreError::Corrupt("missing profile switch commit"))?;
            if state.active_profiles.get(&journal.installation_id) != commit.previous.as_ref()
                || !commit.target.matches_manifest(&target_manifest)
            {
                return Err(StoreError::InvalidTransition);
            }
            state
                .active_profiles
                .insert(journal.installation_id.clone(), commit.target.clone());
        } else {
            state.active_profiles.remove(&journal.installation_id);
        }
        state
            .manifests
            .insert(journal.installation_id.clone(), pending);
        state
            .journals
            .insert(journal.operation_id.clone(), serde_json::to_string(&next)?);
        let result = self.append_locked_with_validation(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::ManifestPublished),
            Some(lease),
            || validate_claims().map_err(StoreError::ManifestClaimChanged),
        );
        drop(lock);
        result.map(|()| next)
    }

    pub fn complete_manifest_only<F>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        generation_id: &str,
        target: &InstallationManifest,
        result_fingerprint: &str,
        validate_claims: F,
    ) -> Result<OperationRecord, StoreError>
    where
        F: FnOnce() -> Result<(), FilesystemBoundaryError>,
    {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if !valid_id(generation_id, 128)
            || record.state != OperationState::Running
            || record.request.intent().kind == LifecycleOperationKind::ProfileSwitch
        {
            return Err(StoreError::InvalidTransition);
        }
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut payload = record.clone().into_payload();
        payload.state = OperationState::Succeeded;
        payload.phase = deltamod_product_contracts::OperationPhase::Complete;
        payload.revision = next_revision;
        payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        payload.result_fingerprint = Some(result_fingerprint.to_owned());
        payload.error = None;
        let terminal = OperationRecord::new(payload)?;
        require_record_and_lease(&state, record, lease, now_ms)?;
        let previous = state.manifest(&lease.installation_id)?;
        let expected_generation = previous
            .as_ref()
            .map_or(Some(1), |manifest| manifest.generation().checked_add(1));
        if target.installation_id() != lease.installation_id
            || Some(target.generation()) != expected_generation
            || state.generations.contains_key(generation_id)
        {
            return Err(StoreError::InvalidTransition);
        }
        state.manifests.insert(
            lease.installation_id.clone(),
            EncodedManifest::encode(target)?,
        );
        state.active_profiles.remove(&lease.installation_id);
        state.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&terminal)?,
        );
        state.active_leases.remove(&lease.installation_id);
        let completion_sequence = state
            .store_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        state.generations.insert(
            generation_id.to_owned(),
            StoredGeneration {
                generation_id: generation_id.to_owned(),
                installation_id: lease.installation_id.clone(),
                operation_id: record.request.operation_id().to_owned(),
                previous_manifest: previous.as_ref().map(EncodedManifest::encode).transpose()?,
                target_manifest: EncodedManifest::encode(target)?,
                started_at_ms: record.created_at_ms,
                completed_at_ms: Some(now_ms),
                completion_sequence: Some(completion_sequence),
                size_bytes: Some(0),
                last_accessed_at_ms: Some(now_ms),
                storage: None,
                viable: true,
                pinned: false,
                protected_by_operations: BTreeSet::new(),
                manifest_only_observations: Vec::new(),
                previous_active_profile: None,
                target_active_profile: None,
            },
        );
        release_generation_protection(&mut state, record.request.operation_id());
        let result = self.append_locked_with_validation(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::ManifestOnlyCommit),
            Some(lease),
            || validate_claims().map_err(StoreError::ManifestClaimChanged),
        );
        drop(lock);
        result.map(|()| terminal)
    }

    /// Atomically publishes a manifest and its active-profile pointer for a
    /// profile switch that has no destination-file mutations.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn complete_profile_switch_manifest_only<F>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        generation_id: &str,
        target: &InstallationManifest,
        result_fingerprint: &str,
        plan_fingerprint: &str,
        previous_profile: Option<&ActiveProfilePointer>,
        target_profile: &ActiveProfilePointer,
        validate_claims: F,
    ) -> Result<OperationRecord, StoreError>
    where
        F: FnOnce() -> Result<(), FilesystemBoundaryError>,
    {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if !valid_id(generation_id, 128)
            || !valid_sha256(plan_fingerprint)
            || record.state != OperationState::Running
            || record.request.intent().kind != LifecycleOperationKind::ProfileSwitch
            || !record
                .request
                .intent()
                .file_plan_fingerprint
                .as_deref()
                .is_some_and(|fingerprint| fingerprint.eq_ignore_ascii_case(plan_fingerprint))
        {
            return Err(StoreError::InvalidTransition);
        }
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut payload = record.clone().into_payload();
        payload.state = OperationState::Succeeded;
        payload.phase = deltamod_product_contracts::OperationPhase::Complete;
        payload.revision = next_revision;
        payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        payload.result_fingerprint = Some(result_fingerprint.to_owned());
        payload.error = None;
        let terminal = OperationRecord::new(payload)?;
        require_record_and_lease(&state, record, lease, now_ms)?;
        let previous = state.manifest(&lease.installation_id)?;
        let expected_generation = previous
            .as_ref()
            .map_or(Some(1), |manifest| manifest.generation().checked_add(1));
        target_profile
            .validate()
            .map_err(|_| StoreError::InvalidTransition)?;
        if previous_profile != state.active_profiles.get(&lease.installation_id)
            || target.installation_id() != lease.installation_id
            || Some(target.generation()) != expected_generation
            || !target_profile.matches_manifest(target)
            || state.generations.contains_key(generation_id)
            || state
                .profile_switch_commits
                .contains_key(record.request.operation_id())
        {
            return Err(StoreError::InvalidTransition);
        }
        state.manifests.insert(
            lease.installation_id.clone(),
            EncodedManifest::encode(target)?,
        );
        state
            .active_profiles
            .insert(lease.installation_id.clone(), target_profile.clone());
        state.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&terminal)?,
        );
        state.active_leases.remove(&lease.installation_id);
        let completion_sequence = state
            .store_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        state.generations.insert(
            generation_id.to_owned(),
            StoredGeneration {
                generation_id: generation_id.to_owned(),
                installation_id: lease.installation_id.clone(),
                operation_id: record.request.operation_id().to_owned(),
                previous_manifest: previous.as_ref().map(EncodedManifest::encode).transpose()?,
                target_manifest: EncodedManifest::encode(target)?,
                started_at_ms: record.created_at_ms,
                completed_at_ms: Some(now_ms),
                completion_sequence: Some(completion_sequence),
                size_bytes: Some(0),
                last_accessed_at_ms: Some(now_ms),
                storage: None,
                viable: true,
                pinned: false,
                protected_by_operations: BTreeSet::new(),
                manifest_only_observations: Vec::new(),
                previous_active_profile: previous_profile.cloned(),
                target_active_profile: Some(target_profile.clone()),
            },
        );
        release_generation_protection(&mut state, record.request.operation_id());
        let result = self.append_locked_with_validation(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::ManifestOnlyCommit),
            Some(lease),
            || validate_claims().map_err(StoreError::ManifestClaimChanged),
        );
        drop(lock);
        result.map(|()| terminal)
    }

    /// Completes an exact-active replay under a fresh outer request without
    /// creating a new manifest generation or rewriting the profile pointer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn complete_profile_switch_noop<F>(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        result_fingerprint: &str,
        plan_fingerprint: &str,
        target_profile: &ActiveProfilePointer,
        validate_claims: F,
    ) -> Result<OperationRecord, StoreError>
    where
        F: FnOnce() -> Result<(), FilesystemBoundaryError>,
    {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if !valid_sha256(plan_fingerprint)
            || record.state != OperationState::Running
            || record.request.intent().kind != LifecycleOperationKind::ProfileSwitch
            || !record
                .request
                .intent()
                .file_plan_fingerprint
                .as_deref()
                .is_some_and(|fingerprint| fingerprint.eq_ignore_ascii_case(plan_fingerprint))
        {
            return Err(StoreError::InvalidTransition);
        }
        require_record_and_lease(&state, record, lease, now_ms)?;
        let manifest = state
            .manifest(&lease.installation_id)?
            .ok_or(StoreError::InvalidTransition)?;
        if state.active_profiles.get(&lease.installation_id) != Some(target_profile)
            || !target_profile.matches_manifest(&manifest)
            || state.journals.contains_key(record.request.operation_id())
            || state
                .profile_switch_commits
                .contains_key(record.request.operation_id())
        {
            return Err(StoreError::InvalidTransition);
        }
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut payload = record.clone().into_payload();
        payload.state = OperationState::Succeeded;
        payload.phase = deltamod_product_contracts::OperationPhase::Complete;
        payload.revision = next_revision;
        payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        payload.result_fingerprint = Some(result_fingerprint.to_owned());
        payload.error = None;
        let terminal = OperationRecord::new(payload)?;
        state.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&terminal)?,
        );
        state.active_leases.remove(&lease.installation_id);
        release_generation_protection(&mut state, record.request.operation_id());
        let result =
            self.append_locked_with_validation(&lock, &mut state, None, Some(lease), || {
                validate_claims().map_err(StoreError::ManifestClaimChanged)
            });
        drop(lock);
        result.map(|()| terminal)
    }

    pub fn fail_without_journal(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        error: ProductError,
        _now_ms: u64,
    ) -> Result<OperationRecord, StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut payload = record.clone().into_payload();
        payload.state = OperationState::Failed;
        payload.phase = deltamod_product_contracts::OperationPhase::Complete;
        payload.revision = next_revision;
        payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        payload.error = Some(error);
        payload.result_fingerprint = None;
        let terminal = OperationRecord::new(payload)?;
        require_record_and_lease(&state, record, lease, now_ms)?;
        if state.journals.contains_key(record.request.operation_id()) {
            return Err(StoreError::InvalidTransition);
        }
        state.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&terminal)?,
        );
        state.active_leases.remove(&lease.installation_id);
        release_generation_protection(&mut state, record.request.operation_id());
        self.append_locked(&lock, &mut state, None, Some(lease))?;
        drop(lock);
        Ok(terminal)
    }

    pub fn begin_failed_rollback(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        error: ProductError,
    ) -> Result<(OperationRecord, LifecycleJournal), StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        validate_journal_binding(journal, record, lease, now_ms)?;
        if record.state != OperationState::Running
            || matches!(
                journal.phase,
                deltamod_product_contracts::OperationPhase::Complete
                    | deltamod_product_contracts::OperationPhase::CleaningUp
            )
            || journal.manifest_commit_state != ManifestCommitState::NotStarted
            || journal.recovery_disposition() != JournalDisposition::RollBack
        {
            return Err(StoreError::InvalidTransition);
        }
        require_exact_binding(&state, record, lease, journal, now_ms)?;
        let mut record_payload = record.clone().into_payload();
        record_payload.state = OperationState::RecoveryRequired;
        record_payload.phase = deltamod_product_contracts::OperationPhase::RollingBack;
        record_payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        record_payload.result_fingerprint = None;
        record_payload.error = Some(error);
        let rollback_record = OperationRecord::new(record_payload)?;
        let mut journal_payload = journal.clone().into_payload();
        journal_payload.phase = deltamod_product_contracts::OperationPhase::RollingBack;
        journal_payload.updated_at_ms = now_ms.max(journal.updated_at_ms);
        let rollback_journal = LifecycleJournal::new(journal_payload)?;
        validate_journal_binding(&rollback_journal, &rollback_record, lease, now_ms)?;
        state.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&rollback_record)?,
        );
        state.journals.insert(
            journal.operation_id.clone(),
            serde_json::to_string(&rollback_journal)?,
        );
        self.append_locked(
            &lock,
            &mut state,
            Some(JournalCheckpointKind::Phase(
                deltamod_product_contracts::OperationPhase::RollingBack,
            )),
            Some(lease),
        )?;
        drop(lock);
        Ok((rollback_record, rollback_journal))
    }

    pub fn terminalize(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        journal: &LifecycleJournal,
        terminalization: Terminalization,
    ) -> Result<(OperationRecord, LifecycleJournal), StoreError> {
        use deltamod_product_contracts::OperationPhase;
        let Terminalization {
            state,
            error,
            result_fingerprint,
            now_ms: _,
        } = terminalization;
        let (lock, mut durable, now_ms) = self.lock_and_load_authoritative()?;
        validate_journal_binding(journal, record, lease, now_ms)?;
        if journal.phase != OperationPhase::CleaningUp {
            return Err(StoreError::InvalidTransition);
        }
        let committed = journal.manifest_commit_state == ManifestCommitState::Published;
        let rolled_back = journal.manifest_commit_state == ManifestCommitState::NotStarted
            && journal.mutations.iter().all(|mutation| {
                matches!(
                    mutation.checkpoint,
                    MutationCheckpoint::NoEffect | MutationCheckpoint::RolledBack
                )
            });
        let terminal_state_matches = match state {
            OperationState::Succeeded => committed && error.is_none(),
            OperationState::Failed | OperationState::Cancelled => rolled_back && error.is_some(),
            OperationState::Recovered => committed || rolled_back,
            _ => false,
        };
        if !terminal_state_matches {
            return Err(StoreError::InvalidTransition);
        }
        let revision = record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut record_payload = record.clone().into_payload();
        record_payload.state = state;
        record_payload.phase = OperationPhase::Complete;
        record_payload.revision = revision;
        record_payload.updated_at_ms = now_ms.max(record.updated_at_ms);
        record_payload.error = error;
        record_payload.result_fingerprint = result_fingerprint;
        let terminal_record = OperationRecord::new(record_payload)?;
        let mut journal_payload = journal.clone().into_payload();
        journal_payload.phase = OperationPhase::Complete;
        journal_payload.operation_revision = revision;
        journal_payload.journal_sequence = journal
            .journal_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        journal_payload.updated_at_ms = now_ms.max(journal.updated_at_ms);
        let terminal_journal = LifecycleJournal::new(journal_payload)?;
        require_exact_binding(&durable, record, lease, journal, now_ms)?;
        durable.operations.insert(
            record.request.operation_id().to_owned(),
            serde_json::to_string(&terminal_record)?,
        );
        durable.journals.insert(
            journal.operation_id.clone(),
            serde_json::to_string(&terminal_journal)?,
        );
        durable.active_leases.remove(&lease.installation_id);
        durable.pending_manifests.remove(&journal.operation_id);
        let removed_profile_commit = durable.profile_switch_commits.remove(&journal.operation_id);
        if (journal.operation == LifecycleOperationKind::ProfileSwitch)
            != removed_profile_commit.is_some()
        {
            return Err(StoreError::Corrupt("terminal profile switch commit"));
        }
        release_generation_protection(&mut durable, record.request.operation_id());
        let completion_sequence = durable
            .store_sequence
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let generation = durable
            .generations
            .get_mut(&journal.recovery_generation_id)
            .ok_or(StoreError::Corrupt("terminal generation"))?;
        generation.completed_at_ms = Some(now_ms);
        generation.completion_sequence = Some(completion_sequence);
        generation.last_accessed_at_ms = Some(now_ms);
        generation.viable = committed;
        if committed {
            // The exact filesystem size is bound immediately after cleanup.
            // Until then the completed generation must still satisfy the
            // durable shape invariant without claiming measured bytes.
            generation.size_bytes.get_or_insert(0);
        } else {
            generation.size_bytes = Some(0);
            generation.storage = None;
        }
        let result = self.append_locked(
            &lock,
            &mut durable,
            Some(JournalCheckpointKind::Terminal(state)),
            Some(lease),
        );
        drop(lock);
        result.map(|()| (terminal_record, terminal_journal))
    }

    pub fn recover_abandoned_preflight(
        &mut self,
        interrupted: &InterruptedOperation,
        _now_ms: u64,
    ) -> Result<OperationRecord, StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if interrupted.journal.is_some()
            || interrupted.record.state.terminal()
            || interrupted.lease.active_at(now_ms)
        {
            return Err(StoreError::InvalidTransition);
        }
        let revision = interrupted
            .record
            .revision
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let mut payload = interrupted.record.clone().into_payload();
        payload.state = OperationState::Recovered;
        payload.phase = deltamod_product_contracts::OperationPhase::Complete;
        payload.revision = revision;
        payload.updated_at_ms = now_ms.max(interrupted.record.updated_at_ms);
        payload.error = None;
        let terminal = OperationRecord::new(payload)?;
        let current = state.operation(interrupted.record.request.operation_id())?;
        if current != interrupted.record
            || state.active_leases.get(&interrupted.lease.installation_id)
                != Some(&interrupted.lease)
        {
            return Err(StoreError::StaleRevision);
        }
        state.operations.insert(
            terminal.request.operation_id().to_owned(),
            serde_json::to_string(&terminal)?,
        );
        state
            .active_leases
            .remove(&interrupted.lease.installation_id);
        release_generation_protection(&mut state, interrupted.record.request.operation_id());
        self.append_locked(&lock, &mut state, None, None)?;
        drop(lock);
        Ok(terminal)
    }

    fn cas_journal_locked(
        &mut self,
        record: &OperationRecord,
        lease: &OperationLease,
        current: &LifecycleJournal,
        next: &LifecycleJournal,
        _now_ms: u64,
        checkpoint: JournalCheckpointKind,
    ) -> Result<(), StoreError> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        require_exact_binding(&state, record, lease, current, now_ms)?;
        state
            .journals
            .insert(current.operation_id.clone(), serde_json::to_string(next)?);
        let result = self.append_locked(&lock, &mut state, Some(checkpoint), Some(lease));
        drop(lock);
        result
    }
}

impl OperationStore for DurableLifecycleStore {
    type Error = StoreError;

    fn acquire_or_replay(
        &mut self,
        request: &OperationRequest,
        owner_instance_id: &str,
        lease_id: &str,
        _now_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<AcquireOutcome, Self::Error> {
        request.validate()?;
        if !request.intent().kind.mutates_files()
            || !valid_id(owner_instance_id, 128)
            || !valid_id(lease_id, 128)
            || lease_ttl_ms == 0
        {
            return Err(StoreError::InvalidTransition);
        }
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        let expires_at_ms = now_ms
            .checked_add(lease_ttl_ms)
            .ok_or(StoreError::InvalidTransition)?;
        if let Some(encoded) = state.operations.get(request.operation_id()) {
            let existing: OperationRecord = decode_current(encoded.as_bytes())?;
            drop(lock);
            return if same_semantic_request(&existing, request) {
                Ok(AcquireOutcome::Existing(existing))
            } else {
                Err(StoreError::IdempotencyConflict)
            };
        }
        if let Some(operation_id) = state.idempotency.get(request.idempotency_key()) {
            let existing = state.operation(operation_id)?;
            drop(lock);
            return if same_semantic_request(&existing, request) {
                Ok(AcquireOutcome::Existing(existing))
            } else {
                Err(StoreError::IdempotencyConflict)
            };
        }
        if let Some(active) = state.active_leases.get(&request.intent().installation_id) {
            let outcome = AcquireOutcome::Busy {
                active_operation_id: active.operation_id.clone(),
                expires_at_ms: active.expires_at_ms,
            };
            drop(lock);
            return Ok(outcome);
        }
        let fencing_token = state
            .next_fencing_tokens
            .get(&request.intent().installation_id)
            .copied()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(StoreError::SequenceExhausted)?;
        let lease = OperationLease {
            installation_id: request.intent().installation_id.clone(),
            operation_id: request.operation_id().to_owned(),
            lease_id: lease_id.to_owned(),
            owner_instance_id: owner_instance_id.to_owned(),
            fencing_token,
            acquired_at_ms: now_ms,
            expires_at_ms,
        };
        lease.validate()?;
        let record = OperationRecord::new(OperationRecordPayload {
            request: request.clone(),
            state: OperationState::Running,
            phase: deltamod_product_contracts::OperationPhase::Preflight,
            revision: 1,
            fencing_token,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            result_fingerprint: None,
            error: None,
        })?;
        state
            .next_fencing_tokens
            .insert(request.intent().installation_id.clone(), fencing_token);
        state.operations.insert(
            request.operation_id().to_owned(),
            serde_json::to_string(&record)?,
        );
        state.idempotency.insert(
            request.idempotency_key().to_owned(),
            request.operation_id().to_owned(),
        );
        state
            .active_leases
            .insert(lease.installation_id.clone(), lease.clone());
        self.append_locked(&lock, &mut state, None, Some(&lease))?;
        drop(lock);
        Ok(AcquireOutcome::Acquired { lease, record })
    }

    fn compare_and_swap(
        &mut self,
        lease: &OperationLease,
        expected_revision: u64,
        next: &OperationRecord,
    ) -> Result<CompareAndSwapOutcome, Self::Error> {
        next.validate()?;
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        let current = state.operation(&lease.operation_id)?;
        if state.active_leases.get(&lease.installation_id) != Some(lease)
            || current.fencing_token != lease.fencing_token
            || !lease.active_at(now_ms)
        {
            drop(lock);
            return Ok(CompareAndSwapOutcome::LostLease);
        }
        if current.revision != expected_revision {
            let current_revision = current.revision;
            drop(lock);
            return Ok(CompareAndSwapOutcome::StaleRevision { current_revision });
        }
        let state_transition =
            current.state == next.state || current.state.can_transition_to(next.state);
        if !state_transition
            || current.state.terminal()
            || current.request != next.request
            || current.fencing_token != next.fencing_token
            || current.created_at_ms != next.created_at_ms
            || current.revision.checked_add(1) != Some(next.revision)
            || next.updated_at_ms < current.updated_at_ms
        {
            return Err(StoreError::InvalidTransition);
        }
        state
            .operations
            .insert(lease.operation_id.clone(), serde_json::to_string(next)?);
        if next.state.terminal() {
            state.active_leases.remove(&lease.installation_id);
            release_generation_protection(&mut state, next.request.operation_id());
        }
        self.append_locked(&lock, &mut state, None, Some(lease))?;
        drop(lock);
        Ok(CompareAndSwapOutcome::Stored(Box::new(next.clone())))
    }

    fn renew(
        &mut self,
        lease: &OperationLease,
        _now_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<Option<OperationLease>, Self::Error> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if lease_ttl_ms == 0 {
            return Ok(None);
        }
        let expires_at_ms = now_ms
            .checked_add(lease_ttl_ms)
            .ok_or(StoreError::InvalidTransition)?;
        if state.active_leases.get(&lease.installation_id) != Some(lease)
            || !lease.active_at(now_ms)
        {
            drop(lock);
            return Ok(None);
        }
        let mut renewed = lease.clone();
        renewed.expires_at_ms = expires_at_ms;
        renewed.validate()?;
        state
            .active_leases
            .insert(lease.installation_id.clone(), renewed.clone());
        self.append_locked(&lock, &mut state, None, Some(&renewed))?;
        drop(lock);
        Ok(Some(renewed))
    }

    fn release(&mut self, lease: &OperationLease) -> Result<bool, Self::Error> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        if state.active_leases.get(&lease.installation_id) != Some(lease)
            || !lease.active_at(now_ms)
        {
            drop(lock);
            return Ok(false);
        }
        if !state.operation(&lease.operation_id)?.state.terminal() {
            return Err(StoreError::InvalidTransition);
        }
        state.active_leases.remove(&lease.installation_id);
        self.append_locked(&lock, &mut state, None, Some(lease))?;
        drop(lock);
        Ok(true)
    }
}

pub struct DurableMutationGuard<'a> {
    store: &'a mut DurableLifecycleStore,
    lock: File,
    state: PersistedState,
    lease: OperationLease,
    record: OperationRecord,
    journal: LifecycleJournal,
    fingerprint: String,
}

impl LifecycleMutationGuard for DurableMutationGuard<'_> {
    fn operation_id(&self) -> &str {
        self.journal.operation_id.as_str()
    }

    fn installation_id(&self) -> &str {
        self.journal.installation_id.as_str()
    }

    fn lease_id(&self) -> &str {
        self.lease.lease_id.as_str()
    }

    fn fencing_token(&self) -> u64 {
        self.lease.fencing_token
    }

    fn operation_revision(&self) -> u64 {
        self.record.revision
    }

    fn journal_sequence(&self) -> u64 {
        self.journal.journal_sequence
    }

    fn locked_journal_fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn assert_current(&mut self, _now_ms: u64) -> Result<(), MutationFenceError> {
        let now_ms = self
            .store
            .persist_trusted_time_locked(&self.lock, &mut self.state)
            .map_err(|_| MutationFenceError::Store)?;
        self.store
            .verify_root_identity()
            .and_then(|()| self.store.verify_and_bind_lock(&self.lock).map(drop))
            .map_err(|_| MutationFenceError::Store)?;
        if self.state.active_leases.get(&self.lease.installation_id) != Some(&self.lease)
            || self.state.operation(&self.lease.operation_id).ok().as_ref() != Some(&self.record)
        {
            return Err(MutationFenceError::LostLease);
        }
        if !self.lease.active_at(now_ms) {
            return Err(MutationFenceError::Expired);
        }
        let journal = self
            .state
            .journal(&self.lease.operation_id)
            .map_err(|_| MutationFenceError::InvalidJournal)?;
        if journal.journal_sequence != self.journal.journal_sequence
            || journal
                .canonical_fingerprint()
                .map_err(|_| MutationFenceError::InvalidJournal)?
                != self.fingerprint
        {
            return Err(MutationFenceError::StaleJournalSequence);
        }
        Ok(())
    }

    fn checkpoint_after_side_effect(
        &mut self,
        now_ms: u64,
        transition: &ValidatedMutationTransition,
    ) -> Result<(), MutationFenceError> {
        self.assert_current(now_ms)?;
        if transition.current_journal() != &self.journal
            || transition.current_journal_fingerprint() != self.fingerprint
        {
            return Err(MutationFenceError::InvalidJournal);
        }
        let next = transition.next_journal();
        self.state.journals.insert(
            next.operation_id.clone(),
            serde_json::to_string(next).map_err(|_| MutationFenceError::Store)?,
        );
        self.store
            .append_locked(
                &self.lock,
                &mut self.state,
                Some(JournalCheckpointKind::Mutation {
                    index: next.mutations[transition.mutation_index()].index,
                    checkpoint: next.mutations[transition.mutation_index()].checkpoint,
                }),
                Some(&self.lease),
            )
            .map_err(|_| MutationFenceError::Store)?;
        self.journal = next.clone();
        self.fingerprint = next
            .canonical_fingerprint()
            .map_err(|_| MutationFenceError::InvalidJournal)?;
        Ok(())
    }

    fn checkpoint_after_reconciliation(
        &mut self,
        now_ms: u64,
        reconciliation: &ValidatedMutationReconciliation,
    ) -> Result<(), MutationFenceError> {
        self.assert_current(now_ms)?;
        if reconciliation.current_journal() != &self.journal
            || reconciliation.current_journal_fingerprint() != self.fingerprint
        {
            return Err(MutationFenceError::InvalidJournal);
        }
        let next = reconciliation.next_journal();
        self.state.journals.insert(
            next.operation_id.clone(),
            serde_json::to_string(next).map_err(|_| MutationFenceError::Store)?,
        );
        self.store
            .append_locked(
                &self.lock,
                &mut self.state,
                Some(JournalCheckpointKind::Mutation {
                    index: next.mutations[reconciliation.mutation_index()].index,
                    checkpoint: next.mutations[reconciliation.mutation_index()].checkpoint,
                }),
                Some(&self.lease),
            )
            .map_err(|_| MutationFenceError::Store)?;
        self.journal = next.clone();
        self.fingerprint = next
            .canonical_fingerprint()
            .map_err(|_| MutationFenceError::InvalidJournal)?;
        Ok(())
    }
}

impl LifecycleTransactionStore for DurableLifecycleStore {
    type Guard<'a>
        = DurableMutationGuard<'a>
    where
        Self: 'a;

    fn lock_mutation<'a>(
        &'a mut self,
        lease: &OperationLease,
        expected_record: &OperationRecord,
        expected_journal: &LifecycleJournal,
        _now_ms: u64,
    ) -> Result<Self::Guard<'a>, <Self as OperationStore>::Error> {
        let (lock, state, now_ms) = self.lock_and_load_authoritative()?;
        require_exact_binding(&state, expected_record, lease, expected_journal, now_ms)?;
        let fingerprint = expected_journal.canonical_fingerprint()?;
        Ok(DurableMutationGuard {
            store: self,
            lock,
            state,
            lease: lease.clone(),
            record: expected_record.clone(),
            journal: expected_journal.clone(),
            fingerprint,
        })
    }

    fn rebind_and_lock_recovery<'a>(
        &'a mut self,
        rebind: &ValidatedRecoveryRebind,
        _now_ms: u64,
    ) -> Result<Self::Guard<'a>, <Self as OperationStore>::Error> {
        let (lock, mut state, now_ms) = self.lock_and_load_authoritative()?;
        let stalled = state.operation(rebind.stalled_record().request.operation_id())?;
        let stalled_journal = state.journal(rebind.stalled_record().request.operation_id())?;
        let active = state
            .active_leases
            .get(&rebind.stalled_record().request.intent().installation_id)
            .ok_or(StoreError::LostLease)?;
        if stalled != *rebind.stalled_record()
            || stalled_journal != *rebind.stalled_journal()
            || stalled_journal.canonical_fingerprint()? != rebind.stalled_journal_fingerprint()
            || active.active_at(now_ms)
        {
            return Err(StoreError::StaleJournal);
        }
        state.operations.insert(
            rebind.recovery_record().request.operation_id().to_owned(),
            serde_json::to_string(rebind.recovery_record())?,
        );
        state.journals.insert(
            rebind.recovery_journal().operation_id.clone(),
            serde_json::to_string(rebind.recovery_journal())?,
        );
        state.active_leases.insert(
            rebind.recovery_lease().installation_id.clone(),
            rebind.recovery_lease().clone(),
        );
        self.append_locked(&lock, &mut state, None, Some(rebind.recovery_lease()))?;
        let fingerprint = rebind.recovery_journal().canonical_fingerprint()?;
        Ok(DurableMutationGuard {
            store: self,
            lock,
            state,
            lease: rebind.recovery_lease().clone(),
            record: rebind.recovery_record().clone(),
            journal: rebind.recovery_journal().clone(),
            fingerprint,
        })
    }
}

fn next_store_sequence(current: u64) -> Result<u64, StoreError> {
    current.checked_add(1).ok_or(StoreError::SequenceExhausted)
}

fn encode_state_frame(state: &PersistedState) -> Result<Vec<u8>, StoreError> {
    encode_state_frame_with_magic(state, FRAME_MAGIC)
}

fn encode_compacted_state_frame(state: &PersistedState) -> Result<Vec<u8>, StoreError> {
    encode_state_frame_with_magic(state, COMPACTED_FRAME_MAGIC)
}

fn encode_state_frame_with_magic(
    state: &PersistedState,
    magic: &[u8; 8],
) -> Result<Vec<u8>, StoreError> {
    let payload = serde_json::to_vec(state)?;
    if payload.is_empty() || payload.len() > MAX_STATE_BYTES {
        return Err(StoreError::Corrupt("state size"));
    }
    let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
    frame.extend_from_slice(magic);
    frame.extend_from_slice(&state.store_sequence.to_be_bytes());
    frame.extend_from_slice(
        &u64::try_from(payload.len())
            .map_err(|_| StoreError::Corrupt("state size"))?
            .to_be_bytes(),
    );
    frame.extend_from_slice(&Sha256::digest(&payload));
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn replay_frames(
    bytes: &[u8],
    initial_expected_sequence: u64,
) -> Result<(PersistedState, usize), StoreError> {
    let mut offset = 0_usize;
    let mut latest = PersistedState::default();
    let mut expected_sequence = initial_expected_sequence;
    let mut observed_generation_completions = BTreeMap::new();
    while offset < bytes.len() {
        if bytes.len() - offset < FRAME_HEADER_BYTES {
            break;
        }
        let magic = &bytes[offset..offset + 8];
        let compacted_base = offset == 0 && magic == COMPACTED_FRAME_MAGIC;
        if magic != FRAME_MAGIC && !compacted_base {
            return Err(StoreError::Corrupt("frame magic"));
        }
        let sequence = u64::from_be_bytes(
            bytes[offset + 8..offset + 16]
                .try_into()
                .map_err(|_| StoreError::Corrupt("frame sequence"))?,
        );
        let length = u64::from_be_bytes(
            bytes[offset + 16..offset + 24]
                .try_into()
                .map_err(|_| StoreError::Corrupt("frame length"))?,
        );
        let length = usize::try_from(length).map_err(|_| StoreError::Corrupt("frame length"))?;
        if length == 0 || length > MAX_STATE_BYTES {
            return Err(StoreError::Corrupt("frame size"));
        }
        let frame_end = offset
            .checked_add(FRAME_HEADER_BYTES)
            .and_then(|value| value.checked_add(length))
            .ok_or(StoreError::Corrupt("frame overflow"))?;
        if frame_end > bytes.len() {
            break;
        }
        if compacted_base {
            if sequence == 0 {
                return Err(StoreError::Corrupt("frame sequence"));
            }
            expected_sequence = sequence;
        }
        if sequence != expected_sequence {
            return Err(StoreError::Corrupt("frame ordering"));
        }
        let payload = &bytes[offset + FRAME_HEADER_BYTES..frame_end];
        let expected_hash = &bytes[offset + 24..offset + 56];
        if Sha256::digest(payload).as_slice() != expected_hash {
            return Err(StoreError::Corrupt("frame hash"));
        }
        let mut state: PersistedState = serde_json::from_slice(payload)?;
        if state.store_sequence != sequence {
            return Err(StoreError::Corrupt("store sequence"));
        }
        for generation in state
            .generations
            .values_mut()
            .filter(|generation| generation.completed_at_ms.is_some())
        {
            let completion_sequence = if compacted_base {
                generation
                    .completion_sequence
                    .ok_or(StoreError::Corrupt("recovery completion sequence"))?
            } else {
                *observed_generation_completions
                    .entry(generation.generation_id.clone())
                    .or_insert(sequence)
            };
            if generation
                .completion_sequence
                .is_some_and(|stored| stored != completion_sequence)
            {
                return Err(StoreError::Corrupt("recovery completion sequence"));
            }
            generation.completion_sequence = Some(completion_sequence);
            observed_generation_completions
                .entry(generation.generation_id.clone())
                .or_insert(completion_sequence);
            let completed_at_ms = generation
                .completed_at_ms
                .ok_or(StoreError::Corrupt("recovery completion time"))?;
            if generation.size_bytes.is_none() {
                generation.size_bytes = Some(0);
            }
            if generation.last_accessed_at_ms.is_none() {
                generation.last_accessed_at_ms = Some(completed_at_ms);
            }
        }
        state.validate()?;
        offset = frame_end;
        latest = state;
        if sequence == u64::MAX {
            if offset != bytes.len() {
                return Err(StoreError::Corrupt("frames after exhausted sequence"));
            }
            break;
        }
        expected_sequence = sequence + 1;
    }
    Ok((latest, offset))
}

fn map_store_object_open_error(
    error: std::io::Error,
    path: &Path,
    expected_kind: StoreObjectKind,
    label: &'static str,
) -> StoreError {
    match inspect_path(path, expected_kind) {
        Err(IdentityError::Unsafe | IdentityError::WrongKind | IdentityError::Replaced) => {
            StoreError::StoreIdentityChanged(label)
        }
        #[cfg(not(any(unix, windows)))]
        Err(IdentityError::Unavailable) => StoreError::StoreIdentityChanged(label),
        Ok(_) | Err(IdentityError::Io(_)) => StoreError::Io(error),
    }
}

fn map_identity_error(error: IdentityError, label: &'static str) -> StoreError {
    match error {
        IdentityError::Io(error) => StoreError::Io(error),
        IdentityError::Unsafe | IdentityError::WrongKind | IdentityError::Replaced => {
            StoreError::StoreIdentityChanged(label)
        }
        #[cfg(not(any(unix, windows)))]
        IdentityError::Unavailable => StoreError::StoreIdentityChanged(label),
    }
}

#[cfg(unix)]
fn sync_store_directory(path: &Path) -> Result<(), StoreError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn sync_store_directory(path: &Path) -> Result<(), StoreError> {
    use std::os::windows::fs::OpenOptionsExt as _;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    options.open(path)?.sync_all()?;
    Ok(())
}

fn map_bound_identity_error(error: IdentityError, label: &'static str) -> StoreError {
    match error {
        IdentityError::Io(error) if error.kind() != std::io::ErrorKind::NotFound => {
            StoreError::Io(error)
        }
        IdentityError::Io(_)
        | IdentityError::Unsafe
        | IdentityError::WrongKind
        | IdentityError::Replaced => StoreError::StoreIdentityChanged(label),
        #[cfg(not(any(unix, windows)))]
        IdentityError::Unavailable => StoreError::StoreIdentityChanged(label),
    }
}

fn bind_identity(
    binding: &Mutex<Option<StableObjectIdentity>>,
    current: &StableObjectIdentity,
    label: &'static str,
) -> Result<(), StoreError> {
    let mut binding = binding
        .lock()
        .map_err(|_| StoreError::StoreIdentityChanged(label))?;
    if binding.as_ref().is_some_and(|bound| bound != current) {
        return Err(StoreError::StoreIdentityChanged(label));
    }
    if binding.is_none() {
        *binding = Some(current.clone());
    }
    Ok(())
}

fn same_semantic_request(existing: &OperationRecord, request: &OperationRequest) -> bool {
    existing.request.idempotency_key() == request.idempotency_key()
        && existing
            .request
            .request_fingerprint()
            .eq_ignore_ascii_case(request.request_fingerprint())
}

fn require_record_and_lease(
    state: &PersistedState,
    record: &OperationRecord,
    lease: &OperationLease,
    now_ms: u64,
) -> Result<(), StoreError> {
    let durable = state.operation(record.request.operation_id())?;
    if durable.revision != record.revision {
        return Err(StoreError::StaleRevision);
    }
    if durable != *record
        || state.active_leases.get(&lease.installation_id) != Some(lease)
        || durable.fencing_token != lease.fencing_token
        || !lease.active_at(now_ms)
    {
        return Err(StoreError::LostLease);
    }
    Ok(())
}

fn require_exact_binding(
    state: &PersistedState,
    record: &OperationRecord,
    lease: &OperationLease,
    journal: &LifecycleJournal,
    now_ms: u64,
) -> Result<(), StoreError> {
    require_record_and_lease(state, record, lease, now_ms)?;
    let durable = state.journal(record.request.operation_id())?;
    if durable.journal_sequence != journal.journal_sequence
        || durable.canonical_fingerprint()? != journal.canonical_fingerprint()?
    {
        return Err(StoreError::StaleJournal);
    }
    validate_journal_binding(&durable, record, lease, now_ms)?;
    Ok(())
}

fn active_journal_operation_ids(state: &PersistedState) -> Result<BTreeSet<String>, StoreError> {
    state
        .journals
        .iter()
        .filter_map(|(operation_id, encoded)| {
            let journal =
                decode_current::<LifecycleJournal>(encoded.as_bytes()).map_err(StoreError::from);
            match journal {
                Ok(journal)
                    if journal.phase != deltamod_product_contracts::OperationPhase::Complete =>
                {
                    Some(Ok(operation_id.clone()))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

fn generation_retention_entry(
    state: &PersistedState,
    generation: &StoredGeneration,
) -> Result<RecoveryGenerationEntry, StoreError> {
    let finished = generation.completed_at_ms.is_some();
    let completed_at_ms = generation
        .completed_at_ms
        .unwrap_or(generation.started_at_ms);
    let last_accessed_at_ms = generation.last_accessed_at_ms.unwrap_or(completed_at_ms);
    let active_journals = state
        .journals
        .values()
        .map(|encoded| decode_current::<LifecycleJournal>(encoded.as_bytes()))
        .collect::<Result<Vec<_>, _>>()?;
    let active_generation_journals = active_journals
        .iter()
        .filter(|journal| {
            journal.recovery_generation_id == generation.generation_id
                && journal.phase != deltamod_product_contracts::OperationPhase::Complete
        })
        .count();
    let references = generation
        .protected_by_operations
        .len()
        .checked_add(active_generation_journals)
        .and_then(|count| u32::try_from(count).ok())
        .ok_or(StoreError::Corrupt("recovery generation reference count"))?;
    Ok(RecoveryGenerationEntry::new(
        RecoveryGeneration {
            generation_id: generation.generation_id.clone(),
            installation_id: generation.installation_id.clone(),
            size_bytes: generation.size_bytes.unwrap_or_default(),
            completed_at_ms,
            last_accessed_at_ms,
            active: !finished,
            finished,
            pinned: generation.pinned,
            journal_references: references,
            viable: generation.viable,
        },
        generation.operation_id.clone(),
        generation.completion_sequence.unwrap_or_default(),
    ))
}

fn generation_retention_protected(
    state: &PersistedState,
    generation: &StoredGeneration,
) -> Result<bool, StoreError> {
    let active_journals = active_journal_operation_ids(state)?;
    let entry = generation_retention_entry(state, generation)?;
    let last_viable = generation.viable
        && !state.generations.values().any(|other| {
            other.generation_id != generation.generation_id
                && other.installation_id == generation.installation_id
                && other.viable
                && other.completed_at_ms.is_some()
                && !state
                    .pending_generation_deletions
                    .contains_key(&other.generation_id)
        });
    Ok(entry.protected(&active_journals) || last_viable)
}

fn operation_history_entry(
    state: &PersistedState,
    operation_id: &str,
) -> Result<OperationHistoryEntry, StoreError> {
    let record = state.operation(operation_id)?;
    let active_journal_dependency = state
        .journals
        .get(operation_id)
        .map(|encoded| decode_current::<LifecycleJournal>(encoded.as_bytes()))
        .transpose()?
        .is_some_and(|journal| {
            journal.phase != deltamod_product_contracts::OperationPhase::Complete
        });
    Ok(OperationHistoryEntry::from_operation_record(
        &record,
        active_journal_dependency,
    ))
}

fn retention_snapshot_from_state(state: &PersistedState) -> Result<RetentionSnapshot, StoreError> {
    let active_journal_operation_ids = active_journal_operation_ids(state)?;
    let mut recovery_generations = Vec::new();
    for generation in state.generations.values() {
        if state
            .pending_generation_deletions
            .contains_key(&generation.generation_id)
        {
            continue;
        }
        if generation.viable
            && generation.completed_at_ms.is_some()
            && state.journals.contains_key(&generation.operation_id)
            && generation.storage.is_none()
        {
            return Err(StoreError::RecoveryStorageChanged);
        }
        recovery_generations.push(generation_retention_entry(state, generation)?);
    }
    let operations = state
        .operations
        .keys()
        .map(|operation_id| operation_history_entry(state, operation_id))
        .collect::<Result<Vec<_>, _>>()?;
    let recovery_source_operation_ids = state
        .generations
        .values()
        .map(|generation| generation.operation_id.clone())
        .collect();
    Ok(RetentionSnapshot::with_recovery_sources(
        recovery_generations,
        operations,
        active_journal_operation_ids,
        recovery_source_operation_ids,
    ))
}

fn pending_matches_entry(
    tombstone: &PendingRecoveryDeletion,
    expected: &RecoveryGenerationEntry,
) -> bool {
    tombstone.generation_id() == expected.generation.generation_id
        && tombstone.installation_id() == expected.generation.installation_id
        && tombstone.operation_id() == expected.operation_id
        && tombstone.completion_sequence() == expected.completion_sequence
        && tombstone.size_bytes() == expected.generation.size_bytes
        && tombstone.last_accessed_at_ms() == expected.generation.last_accessed_at_ms
}

fn operation_referenced(state: &PersistedState, operation_id: &str) -> bool {
    state
        .active_leases
        .values()
        .any(|lease| lease.operation_id == operation_id)
        || state.pending_manifests.contains_key(operation_id)
        || state.profile_switch_commits.contains_key(operation_id)
        || state.generations.values().any(|generation| {
            generation.operation_id == operation_id
                || generation.protected_by_operations.contains(operation_id)
        })
        || state
            .pending_generation_deletions
            .values()
            .any(|tombstone| tombstone.operation_id() == operation_id)
}

fn remove_operation_bundle(
    state: &mut PersistedState,
    operation_id: &str,
) -> Result<(), StoreError> {
    if operation_referenced(state, operation_id) {
        return Err(StoreError::RetentionRace);
    }
    let record = state.operation(operation_id)?;
    let recoverable = record.state == OperationState::RecoveryRequired
        || record
            .error
            .as_ref()
            .is_some_and(|error| error.recovery_action == RecoveryAction::Recover);
    if !record.state.terminal() || recoverable {
        return Err(StoreError::RetentionRace);
    }
    if state
        .journals
        .get(operation_id)
        .map(|encoded| decode_current::<LifecycleJournal>(encoded.as_bytes()))
        .transpose()?
        .is_some_and(|journal| {
            journal.phase != deltamod_product_contracts::OperationPhase::Complete
        })
    {
        return Err(StoreError::RetentionRace);
    }
    let idempotency_key = record.request.idempotency_key().to_owned();
    if state.idempotency.get(&idempotency_key).map(String::as_str) != Some(operation_id) {
        return Err(StoreError::Corrupt("idempotency index"));
    }
    state.journals.remove(operation_id);
    state.operations.remove(operation_id);
    state.idempotency.remove(&idempotency_key);
    Ok(())
}

fn decode_generation(
    generation: &StoredGeneration,
) -> Result<RecoveryGenerationSnapshot, StoreError> {
    Ok(RecoveryGenerationSnapshot {
        generation_id: generation.generation_id.clone(),
        installation_id: generation.installation_id.clone(),
        operation_id: generation.operation_id.clone(),
        previous_manifest: generation
            .previous_manifest
            .as_ref()
            .map(EncodedManifest::decode)
            .transpose()?,
        target_manifest: generation.target_manifest.decode()?,
        completed_at_ms: generation
            .completed_at_ms
            .ok_or(StoreError::Corrupt("unfinished recovery generation"))?,
        completion_sequence: generation
            .completion_sequence
            .ok_or(StoreError::Corrupt("unfinished recovery generation"))?,
        size_bytes: generation
            .size_bytes
            .ok_or(StoreError::Corrupt("unfinished recovery generation"))?,
        last_accessed_at_ms: generation
            .last_accessed_at_ms
            .ok_or(StoreError::Corrupt("unfinished recovery generation"))?,
        pinned: generation.pinned,
        protected_by_operations: generation.protected_by_operations.clone(),
        manifest_only_observations: generation.manifest_only_observations.clone(),
    })
}

fn release_generation_protection(state: &mut PersistedState, operation_id: &str) {
    for generation in state.generations.values_mut() {
        generation.protected_by_operations.remove(operation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_final_sequence_replays_and_rejects_any_further_append() {
        let mut state = PersistedState {
            store_sequence: u64::MAX,
            ..PersistedState::default()
        };
        state.trusted_time_floor_ms = 77;
        let frame = encode_state_frame(&state).unwrap();

        let (reopened, valid_length) = replay_frames(&frame, u64::MAX).unwrap();
        assert_eq!(valid_length, frame.len());
        assert_eq!(reopened.store_sequence, u64::MAX);
        assert_eq!(reopened.trusted_time_floor_ms, 77);
        assert!(matches!(
            next_store_sequence(reopened.store_sequence),
            Err(StoreError::SequenceExhausted)
        ));

        let mut impossible_tail = frame.clone();
        impossible_tail.extend_from_slice(&frame);
        assert!(matches!(
            replay_frames(&impossible_tail, u64::MAX),
            Err(StoreError::Corrupt("frames after exhausted sequence"))
        ));
    }
}
