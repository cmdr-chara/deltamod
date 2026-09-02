use crate::{
    ContractDocument, ContractKind, ContractPayload, LifecycleOperationKind, OperationLease,
    OperationPhase, OperationRecord, OperationState, ProviderRef, SchemaError, ValidatedContract,
    ValidatedRelativePath, PRODUCT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub(crate) fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn valid_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_identity_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8192
        && !value.chars().any(char::is_control)
        && !value.contains(['\\', ':'])
}

fn recovery_chain_sha256(previous: &str, next_lease_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"deltamod:lifecycle-recovery-chain:v1\0");
    hasher.update(previous.to_ascii_lowercase().as_bytes());
    hasher.update([0]);
    hasher.update(next_lease_id.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileClaim {
    pub path: ValidatedRelativePath,
    /// Runtime-produced, platform-aware identity key. This freezes no
    /// incorrect approximation of Windows case/alias semantics in this crate.
    pub path_identity_key: String,
    pub sha256: String,
    /// Installed instance IDs scoped to the enclosing installation ledger.
    /// The globally stable owner key is `(installation_id, instance_id)`.
    pub owners: BTreeSet<String>,
}

impl FileClaim {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if valid_identity_key(&self.path_identity_key)
            && valid_sha256(&self.sha256)
            && !self.owners.is_empty()
            && self.owners.iter().all(|owner| valid_id(owner, 256))
        {
            Ok(())
        } else {
            Err(SchemaError::InvalidDocument("file claim"))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledFileRef {
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub expected_sha256: String,
}

impl InstalledFileRef {
    fn validate(&self) -> Result<(), SchemaError> {
        if valid_identity_key(&self.path_identity_key) && valid_sha256(&self.expected_sha256) {
            Ok(())
        } else {
            Err(SchemaError::InvalidDocument("installed file reference"))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledModPayload {
    /// Stable identity for this installed instance within `installation_id`.
    /// File ownership always refers to this value, never to a provider-local
    /// mod ID. Cross-installation consumers must use [`InstalledModKey`].
    pub instance_id: String,
    pub mod_id: String,
    pub installation_id: String,
    pub display_name: String,
    #[serde(default)]
    pub version: Option<String>,
    pub provider: ProviderRef,
    #[serde(default)]
    pub archive_sha256: Option<String>,
    pub file_plan_fingerprint: String,
    pub manifest_generation: u64,
    pub installed_at_ms: u64,
    pub updated_at_ms: u64,
    pub files: Vec<InstalledFileRef>,
}

pub type InstalledModRecord = ValidatedContract<InstalledModPayload>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InstalledModKey {
    pub installation_id: String,
    pub instance_id: String,
}

impl ValidatedContract<InstalledModPayload> {
    #[must_use]
    pub fn key(&self) -> InstalledModKey {
        InstalledModKey {
            installation_id: self.installation_id.clone(),
            instance_id: self.instance_id.clone(),
        }
    }
}

impl crate::schema::private::Sealed for InstalledModPayload {}
impl ContractPayload for InstalledModPayload {
    const KIND: ContractKind = ContractKind::InstalledMod;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.instance_id, 256)
            || !valid_id(&self.mod_id, 256)
            || !valid_id(&self.installation_id, 256)
            || self.display_name.is_empty()
            || self.display_name.len() > 512
            || self.manifest_generation == 0
            || self.updated_at_ms < self.installed_at_ms
            || !valid_sha256(&self.file_plan_fingerprint)
            || self
                .archive_sha256
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || self.provider.validate().is_err()
        {
            return Err(SchemaError::InvalidDocument("installed mod"));
        }
        let mut identities = BTreeSet::new();
        for file in &self.files {
            file.validate()?;
            if !identities.insert(&file.path_identity_key) {
                return Err(SchemaError::InvalidDocument("duplicate installed file"));
            }
        }
        Ok(())
    }
}

/// The single authoritative ownership ledger for an installation generation.
/// Installed-mod records refer to the same generation but never duplicate the
/// complete owner set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationClaimsLedgerPayload {
    pub installation_id: String,
    pub manifest_generation: u64,
    pub updated_at_ms: u64,
    pub claims: Vec<FileClaim>,
}

pub type InstallationClaimsLedger = ValidatedContract<InstallationClaimsLedgerPayload>;

impl crate::schema::private::Sealed for InstallationClaimsLedgerPayload {}
impl ContractPayload for InstallationClaimsLedgerPayload {
    const KIND: ContractKind = ContractKind::ClaimsLedger;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.installation_id, 256) || self.manifest_generation == 0 {
            return Err(SchemaError::InvalidDocument("claims ledger"));
        }
        let mut identities = BTreeSet::new();
        for claim in &self.claims {
            claim.validate()?;
            if !identities.insert(&claim.path_identity_key) {
                return Err(SchemaError::InvalidDocument("divergent duplicate claim"));
            }
        }
        Ok(())
    }
}

pub fn validate_installed_record_against_ledger(
    record: &InstalledModRecord,
    ledger: &InstallationClaimsLedger,
) -> Result<(), SchemaError> {
    record.validate()?;
    ledger.validate()?;
    if record.installation_id != ledger.installation_id
        || record.manifest_generation != ledger.manifest_generation
    {
        return Err(SchemaError::InvalidDocument("manifest generation mismatch"));
    }
    let claimed: BTreeMap<_, _> = ledger
        .claims
        .iter()
        .map(|claim| (&claim.path_identity_key, claim))
        .collect();
    let installed: BTreeMap<_, _> = record
        .files
        .iter()
        .map(|file| (&file.path_identity_key, file))
        .collect();
    for file in &record.files {
        let claim = claimed
            .get(&file.path_identity_key)
            .ok_or(SchemaError::InvalidDocument("installed file has no claim"))?;
        if !claim.owners.contains(&record.instance_id)
            || !claim.sha256.eq_ignore_ascii_case(&file.expected_sha256)
            || claim.path != file.path
        {
            return Err(SchemaError::InvalidDocument(
                "installed file claim mismatch",
            ));
        }
    }
    if ledger.claims.iter().any(|claim| {
        claim.owners.contains(&record.instance_id)
            && !installed.contains_key(&claim.path_identity_key)
    }) {
        return Err(SchemaError::InvalidDocument(
            "owner claim missing from mod record",
        ));
    }
    Ok(())
}

/// Validates one complete manifest generation. This is the persistence
/// boundary for installation-scoped installed-instance IDs and exact,
/// bidirectional owner references. Callers combining installations must key
/// records by [`InstalledModKey`].
pub fn validate_installation_manifest(
    records: &[InstalledModRecord],
    ledger: &InstallationClaimsLedger,
) -> Result<(), SchemaError> {
    ledger.validate()?;
    let mut instances = BTreeMap::new();
    for record in records {
        if instances
            .insert(record.instance_id.as_str(), record)
            .is_some()
        {
            return Err(SchemaError::InvalidDocument("duplicate installed instance"));
        }
        validate_installed_record_against_ledger(record, ledger)?;
    }
    for claim in &ledger.claims {
        for owner in &claim.owners {
            if !instances.contains_key(owner.as_str()) {
                return Err(SchemaError::InvalidDocument("orphan file owner"));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalDisposition {
    RollBack,
    FinalizeVerifiedCommit,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationAction {
    Create,
    Replace,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationCheckpoint {
    Planned,
    Staged,
    BackupVerified,
    Applied,
    OutputVerified,
    /// Recovery proved that the destination effect never started. Any staged
    /// artifact remains cleanup-only and no backup is required.
    NoEffect,
    RolledBack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestCommitState {
    NotStarted,
    TemporaryWritten,
    Published,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootIdentity {
    pub canonical_path_sha256: String,
    pub volume_id: String,
    pub file_id: String,
}

impl RootIdentity {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if valid_sha256(&self.canonical_path_sha256)
            && valid_id(&self.volume_id, 256)
            && valid_id(&self.file_id, 256)
        {
            Ok(())
        } else {
            Err(SchemaError::InvalidDocument("root identity"))
        }
    }

    #[must_use]
    pub fn same_filesystem_object(&self, other: &Self) -> bool {
        self.volume_id == other.volume_id && self.file_id == other.file_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalMutation {
    pub index: u32,
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub action: MutationAction,
    pub checkpoint: MutationCheckpoint,
    #[serde(default)]
    pub previous_sha256: Option<String>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub staging_path: Option<ValidatedRelativePath>,
    #[serde(default)]
    pub staging_sha256: Option<String>,
    #[serde(default)]
    pub backup_path: Option<ValidatedRelativePath>,
    #[serde(default)]
    pub backup_sha256: Option<String>,
}

impl JournalMutation {
    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_identity_key(&self.path_identity_key)
            || self
                .previous_sha256
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || self
                .expected_sha256
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || self
                .staging_sha256
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || self
                .backup_sha256
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
        {
            return Err(SchemaError::InvalidDocument("journal mutation"));
        }
        let shape_is_valid = match self.action {
            MutationAction::Create => {
                self.previous_sha256.is_none()
                    && self.expected_sha256.is_some()
                    && self.staging_path.is_some()
                    && self.backup_path.is_none()
            }
            MutationAction::Replace => {
                self.previous_sha256.is_some()
                    && self.expected_sha256.is_some()
                    && self.staging_path.is_some()
                    && self.backup_path.is_some()
            }
            MutationAction::Delete => {
                self.previous_sha256.is_some()
                    && self.expected_sha256.is_none()
                    && self.staging_path.is_none()
                    && self.backup_path.is_some()
            }
        };
        if !shape_is_valid {
            return Err(SchemaError::InvalidDocument("journal mutation shape"));
        }
        let checkpoint_is_valid = match self.action {
            MutationAction::Create => {
                !matches!(self.checkpoint, MutationCheckpoint::BackupVerified)
            }
            MutationAction::Replace => true,
            MutationAction::Delete => !matches!(self.checkpoint, MutationCheckpoint::Staged),
        };
        if !checkpoint_is_valid {
            return Err(SchemaError::InvalidDocument("journal action checkpoint"));
        }
        let needs_staged_hash = match self.action {
            MutationAction::Create => self.checkpoint != MutationCheckpoint::Planned,
            MutationAction::Replace => !matches!(self.checkpoint, MutationCheckpoint::Planned),
            MutationAction::Delete => false,
        };
        let needs_backup_hash = match self.action {
            MutationAction::Create => false,
            MutationAction::Replace => matches!(
                self.checkpoint,
                MutationCheckpoint::BackupVerified
                    | MutationCheckpoint::Applied
                    | MutationCheckpoint::OutputVerified
                    | MutationCheckpoint::RolledBack
            ),
            MutationAction::Delete => self.checkpoint != MutationCheckpoint::Planned,
        };
        let staged_hash_presence_is_valid = if self.checkpoint == MutationCheckpoint::NoEffect {
            self.action != MutationAction::Delete || self.staging_sha256.is_none()
        } else {
            needs_staged_hash == self.staging_sha256.is_some()
        };
        let backup_hash_presence_is_valid = if self.checkpoint == MutationCheckpoint::NoEffect {
            self.backup_sha256.is_none()
        } else {
            needs_backup_hash == self.backup_sha256.is_some()
        };
        if !staged_hash_presence_is_valid
            || !backup_hash_presence_is_valid
            || self
                .staging_sha256
                .as_deref()
                .zip(self.expected_sha256.as_deref())
                .is_some_and(|(actual, expected)| !actual.eq_ignore_ascii_case(expected))
            || self
                .backup_sha256
                .as_deref()
                .zip(self.previous_sha256.as_deref())
                .is_some_and(|(actual, expected)| !actual.eq_ignore_ascii_case(expected))
        {
            return Err(SchemaError::InvalidDocument("journal mutation checkpoint"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleJournalPayload {
    /// Monotonic state/checkpoint sequence. Lease-only recovery rebinds keep
    /// this value and compare-and-swap the complete journal fingerprint.
    pub journal_sequence: u64,
    pub operation_id: String,
    pub idempotency_key: String,
    pub request_fingerprint: String,
    pub lease_id: String,
    pub operation_revision: u64,
    pub fencing_token: u64,
    pub installation_id: String,
    pub operation: LifecycleOperationKind,
    pub phase: OperationPhase,
    pub transaction_root: RootIdentity,
    pub staging_root: RootIdentity,
    pub backup_root: RootIdentity,
    pub recovery_generation_id: String,
    pub recovery_chain_sha256: String,
    pub manifest_generation_before: u64,
    pub manifest_generation_after: u64,
    pub manifest_commit_state: ManifestCommitState,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub mutations: Vec<JournalMutation>,
    pub recovery_attempts: u32,
    pub pinned: bool,
}

pub type LifecycleJournal = ValidatedContract<LifecycleJournalPayload>;

impl crate::schema::private::Sealed for LifecycleJournalPayload {}

impl LifecycleJournalPayload {
    fn required_sequence_headroom(&self) -> Result<u64, SchemaError> {
        if self.phase == OperationPhase::Complete {
            return Ok(0);
        }

        let terminalization_steps = if self.phase == OperationPhase::CleaningUp {
            1_u64
        } else {
            2_u64
        };
        let can_finalize_verified_commit = self.phase != OperationPhase::RollingBack
            && (self.manifest_commit_state == ManifestCommitState::Published
                || self
                    .mutations
                    .iter()
                    .all(|mutation| mutation.checkpoint == MutationCheckpoint::OutputVerified));
        if can_finalize_verified_commit {
            let commit_phase_entry_steps = u64::from(self.phase == OperationPhase::Verifying);
            let manifest_steps = match self.manifest_commit_state {
                ManifestCommitState::NotStarted => 2_u64,
                ManifestCommitState::TemporaryWritten => 1_u64,
                ManifestCommitState::Published => 0_u64,
            };
            return terminalization_steps
                .checked_add(commit_phase_entry_steps)
                .and_then(|steps| steps.checked_add(manifest_steps))
                .ok_or(SchemaError::InvalidDocument("journal recovery headroom"));
        }

        self.mutations
            .iter()
            .try_fold(terminalization_steps, |steps, mutation| {
                let mutation_steps = match (mutation.action, mutation.checkpoint) {
                    (_, MutationCheckpoint::Planned)
                    | (MutationAction::Replace, MutationCheckpoint::Staged)
                    | (_, MutationCheckpoint::Applied | MutationCheckpoint::OutputVerified) => 1,
                    (MutationAction::Create, MutationCheckpoint::Staged)
                    | (
                        MutationAction::Replace | MutationAction::Delete,
                        MutationCheckpoint::BackupVerified,
                    ) => 2,
                    (_, MutationCheckpoint::NoEffect | MutationCheckpoint::RolledBack) => 0,
                    _ => return Err(SchemaError::InvalidDocument("journal recovery headroom")),
                };
                steps
                    .checked_add(mutation_steps)
                    .ok_or(SchemaError::InvalidDocument("journal recovery headroom"))
            })
    }
}

impl ValidatedContract<LifecycleJournalPayload> {
    pub fn next_recovery_chain_sha256(&self, next_lease_id: &str) -> Result<String, SchemaError> {
        if !valid_id(next_lease_id, 128) {
            return Err(SchemaError::InvalidDocument("recovery lease identity"));
        }
        Ok(recovery_chain_sha256(
            &self.recovery_chain_sha256,
            next_lease_id,
        ))
    }

    pub fn canonical_fingerprint(&self) -> Result<String, SchemaError> {
        let encoded = serde_json::to_vec(self).map_err(|_| SchemaError::Malformed)?;
        Ok(Sha256::digest(encoded)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    #[must_use]
    pub fn recovery_disposition(&self) -> JournalDisposition {
        if self.phase == OperationPhase::Complete {
            return JournalDisposition::Complete;
        }
        if self.phase == OperationPhase::RollingBack {
            return JournalDisposition::RollBack;
        }
        if self.manifest_commit_state == ManifestCommitState::Published
            || self
                .mutations
                .iter()
                .all(|mutation| mutation.checkpoint == MutationCheckpoint::OutputVerified)
        {
            JournalDisposition::FinalizeVerifiedCommit
        } else {
            JournalDisposition::RollBack
        }
    }
}

impl ContractPayload for LifecycleJournalPayload {
    const KIND: ContractKind = ContractKind::LifecycleJournal;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if self.journal_sequence == 0
            || !valid_id(&self.operation_id, 128)
            || !valid_id(&self.idempotency_key, 128)
            || !valid_sha256(&self.request_fingerprint)
            || !valid_id(&self.lease_id, 128)
            || self.operation_revision == 0
            || self.fencing_token == 0
            || !valid_id(&self.installation_id, 256)
            || !valid_id(&self.recovery_generation_id, 128)
            || !valid_sha256(&self.recovery_chain_sha256)
            || self.manifest_generation_before.checked_add(1)
                != Some(self.manifest_generation_after)
            || self.updated_at_ms < self.started_at_ms
            || self.mutations.is_empty()
        {
            return Err(SchemaError::InvalidDocument("lifecycle journal"));
        }
        self.transaction_root.validate()?;
        self.staging_root.validate()?;
        self.backup_root.validate()?;
        if self
            .transaction_root
            .same_filesystem_object(&self.staging_root)
            || self
                .transaction_root
                .same_filesystem_object(&self.backup_root)
            || self.staging_root.same_filesystem_object(&self.backup_root)
            || self
                .transaction_root
                .canonical_path_sha256
                .eq_ignore_ascii_case(&self.staging_root.canonical_path_sha256)
            || self
                .transaction_root
                .canonical_path_sha256
                .eq_ignore_ascii_case(&self.backup_root.canonical_path_sha256)
            || self
                .staging_root
                .canonical_path_sha256
                .eq_ignore_ascii_case(&self.backup_root.canonical_path_sha256)
        {
            return Err(SchemaError::InvalidDocument(
                "journal roots must be distinct",
            ));
        }
        let mut identities = BTreeSet::new();
        for (expected_index, mutation) in self.mutations.iter().enumerate() {
            mutation.validate()?;
            if mutation.index != u32::try_from(expected_index).unwrap_or(u32::MAX)
                || !identities.insert(&mutation.path_identity_key)
            {
                return Err(SchemaError::InvalidDocument("journal mutation ordering"));
            }
        }
        let all_planned = self
            .mutations
            .iter()
            .all(|mutation| mutation.checkpoint == MutationCheckpoint::Planned);
        let all_output_verified = self
            .mutations
            .iter()
            .all(|mutation| mutation.checkpoint == MutationCheckpoint::OutputVerified);
        let all_reverted = self.mutations.iter().all(|mutation| {
            matches!(
                mutation.checkpoint,
                MutationCheckpoint::NoEffect | MutationCheckpoint::RolledBack
            )
        });
        let staging_progress_is_valid = self.mutations.iter().all(|mutation| {
            matches!(
                (mutation.action, mutation.checkpoint),
                (_, MutationCheckpoint::Planned)
                    | (
                        MutationAction::Create | MutationAction::Replace,
                        MutationCheckpoint::Staged
                    )
            )
        });
        let backup_progress_is_valid = self.mutations.iter().all(|mutation| {
            matches!(
                (mutation.action, mutation.checkpoint),
                (_, MutationCheckpoint::Planned)
                    | (
                        MutationAction::Create | MutationAction::Replace,
                        MutationCheckpoint::Staged
                    )
                    | (
                        MutationAction::Replace | MutationAction::Delete,
                        MutationCheckpoint::BackupVerified
                    )
            )
        });
        let applying_progress_is_valid = self.mutations.iter().all(|mutation| {
            matches!(
                mutation.checkpoint,
                MutationCheckpoint::Planned
                    | MutationCheckpoint::Staged
                    | MutationCheckpoint::BackupVerified
                    | MutationCheckpoint::Applied
            )
        });
        let verifying_progress_is_valid = self.mutations.iter().all(|mutation| {
            matches!(
                mutation.checkpoint,
                MutationCheckpoint::Applied | MutationCheckpoint::OutputVerified
            )
        });
        let committed =
            all_output_verified && self.manifest_commit_state == ManifestCommitState::Published;
        let rolled_back =
            all_reverted && self.manifest_commit_state == ManifestCommitState::NotStarted;
        let phase_is_valid = match self.phase {
            OperationPhase::Preflight | OperationPhase::Downloading => {
                all_planned && self.manifest_commit_state == ManifestCommitState::NotStarted
            }
            OperationPhase::Staging => {
                staging_progress_is_valid
                    && self.manifest_commit_state == ManifestCommitState::NotStarted
            }
            OperationPhase::BackingUp => {
                backup_progress_is_valid
                    && self.manifest_commit_state == ManifestCommitState::NotStarted
            }
            OperationPhase::Applying => {
                applying_progress_is_valid
                    && self.manifest_commit_state == ManifestCommitState::NotStarted
            }
            OperationPhase::Verifying => {
                verifying_progress_is_valid
                    && self.manifest_commit_state == ManifestCommitState::NotStarted
            }
            OperationPhase::Committing => all_output_verified,
            OperationPhase::RollingBack => {
                self.manifest_commit_state != ManifestCommitState::Published
            }
            OperationPhase::CleaningUp | OperationPhase::Complete => committed || rolled_back,
        };
        if !phase_is_valid {
            return Err(SchemaError::InvalidDocument(
                "journal phase checkpoint matrix",
            ));
        }
        if self
            .journal_sequence
            .checked_add(self.required_sequence_headroom()?)
            .is_none()
        {
            return Err(SchemaError::InvalidDocument(
                "journal sequence recovery headroom",
            ));
        }
        Ok(())
    }
}

fn journal_record_state_is_consistent(
    journal: &LifecycleJournal,
    record: &OperationRecord,
) -> bool {
    let rolled_back = journal.manifest_commit_state == ManifestCommitState::NotStarted
        && journal.mutations.iter().all(|mutation| {
            matches!(
                mutation.checkpoint,
                MutationCheckpoint::NoEffect | MutationCheckpoint::RolledBack
            )
        });
    match record.state {
        OperationState::Queued => false,
        OperationState::Running | OperationState::Cancelling => {
            record.phase == journal.phase && journal.phase != OperationPhase::Complete
        }
        OperationState::RecoveryRequired => {
            record.phase == journal.phase
                && matches!(
                    journal.phase,
                    OperationPhase::Committing
                        | OperationPhase::RollingBack
                        | OperationPhase::CleaningUp
                )
        }
        OperationState::Succeeded => {
            record.phase == OperationPhase::Complete
                && journal.phase == OperationPhase::Complete
                && journal.manifest_commit_state == ManifestCommitState::Published
        }
        OperationState::Failed | OperationState::Cancelled => {
            record.phase == OperationPhase::Complete
                && journal.phase == OperationPhase::Complete
                && rolled_back
        }
        OperationState::Recovered => {
            record.phase == OperationPhase::Complete && journal.phase == OperationPhase::Complete
        }
    }
}

pub fn validate_journal_binding(
    journal: &LifecycleJournal,
    record: &OperationRecord,
    lease: &OperationLease,
    now_ms: u64,
) -> Result<(), SchemaError> {
    journal.validate()?;
    record.validate()?;
    lease.validate()?;
    let request = &record.request;
    if !journal_record_state_is_consistent(journal, record)
        || journal.operation_id != request.operation_id()
        || journal.idempotency_key != request.idempotency_key()
        || !journal
            .request_fingerprint
            .eq_ignore_ascii_case(request.request_fingerprint())
        || journal.installation_id != request.intent().installation_id
        || journal.operation != request.intent().kind
        || journal.operation_revision != record.revision
        || journal.fencing_token != record.fencing_token
        || lease.lease_id != journal.lease_id
        || lease.operation_id != journal.operation_id
        || lease.installation_id != journal.installation_id
        || lease.fencing_token != journal.fencing_token
        || !lease.active_at(now_ms)
    {
        Err(SchemaError::InvalidDocument("journal operation binding"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    DifferentContent,
    ExternalModification,
    UnknownExistingFile,
    UnreadableEntry,
    NonRegularEntry,
    AliasedEntry,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileConflict {
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub reason: ConflictReason,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub actual_sha256: Option<String>,
    #[serde(default)]
    pub proposed_sha256: Option<String>,
    pub existing_owners: BTreeSet<String>,
    pub requesting_owner: String,
}

impl FileConflict {
    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_identity_key(&self.path_identity_key)
            || !valid_id(&self.requesting_owner, 256)
            || self
                .existing_owners
                .iter()
                .any(|owner| !valid_id(owner, 256))
            || [
                &self.expected_sha256,
                &self.actual_sha256,
                &self.proposed_sha256,
            ]
            .into_iter()
            .flatten()
            .any(|hash| !valid_sha256(hash))
        {
            Err(SchemaError::InvalidDocument("file conflict"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConflictReportPayload {
    pub installation_id: String,
    pub conflicts: Vec<FileConflict>,
}

pub type ConflictReport = ValidatedContract<ConflictReportPayload>;

impl crate::schema::private::Sealed for ConflictReportPayload {}
impl ValidatedContract<ConflictReportPayload> {
    #[must_use]
    pub fn blocking(&self) -> bool {
        !self.conflicts.is_empty()
    }
}

impl ContractPayload for ConflictReportPayload {
    const KIND: ContractKind = ContractKind::ConflictReport;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.installation_id, 256) || self.conflicts.is_empty() {
            return Err(SchemaError::InvalidDocument("conflict report"));
        }
        self.conflicts.iter().try_for_each(FileConflict::validate)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    Healthy,
    MissingFiles,
    ExternalChanges,
    HashMismatch,
    Incomplete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationIssue {
    pub path: ValidatedRelativePath,
    pub code: String,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub actual_sha256: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationResultPayload {
    pub installation_id: String,
    #[serde(default)]
    pub mod_instance_id: Option<String>,
    pub state: VerificationState,
    pub checked_files: u64,
    pub issues: Vec<VerificationIssue>,
    pub verified_at_ms: u64,
}

pub type VerificationResult = ValidatedContract<VerificationResultPayload>;

impl crate::schema::private::Sealed for VerificationResultPayload {}
impl ContractPayload for VerificationResultPayload {
    const KIND: ContractKind = ContractKind::VerificationResult;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.installation_id, 256)
            || self
                .mod_instance_id
                .as_deref()
                .is_some_and(|id| !valid_id(id, 256))
            || (self.state == VerificationState::Healthy && !self.issues.is_empty())
            || (self.state != VerificationState::Healthy && self.issues.is_empty())
            || self.issues.iter().any(|issue| {
                !valid_id(&issue.code, 128)
                    || issue
                        .expected_sha256
                        .as_deref()
                        .is_some_and(|hash| !valid_sha256(hash))
                    || issue
                        .actual_sha256
                        .as_deref()
                        .is_some_and(|hash| !valid_sha256(hash))
            })
        {
            Err(SchemaError::InvalidDocument("verification result"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameHealthState {
    Healthy,
    ModifiedAsExpected,
    ExternalChangesDetected,
    MissingFiles,
    ConflictingOwnership,
    InterruptedOperation,
    RepairAvailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameHealthReportPayload {
    pub installation_id: String,
    pub state: GameHealthState,
    pub lifecycle_owned_files: u64,
    pub unknown_modified_files: u64,
    pub interrupted_operations: Vec<String>,
    pub checked_at_ms: u64,
}

pub type GameHealthReport = ValidatedContract<GameHealthReportPayload>;

impl crate::schema::private::Sealed for GameHealthReportPayload {}
impl ValidatedContract<GameHealthReportPayload> {
    #[must_use]
    pub fn repair_available(&self) -> bool {
        self.state == GameHealthState::RepairAvailable
    }
}

impl ContractPayload for GameHealthReportPayload {
    const KIND: ContractKind = ContractKind::GameHealthReport;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.installation_id, 256)
            || self
                .interrupted_operations
                .iter()
                .any(|id| !valid_id(id, 128))
            || (self.state == GameHealthState::Healthy
                && (self.unknown_modified_files != 0 || !self.interrupted_operations.is_empty()))
        {
            Err(SchemaError::InvalidDocument("game health report"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ObservedFileState {
    Missing,
    Regular {
        sha256: String,
        file_id: String,
        link_count: u64,
    },
    NonRegular,
    Unreadable,
}

impl ObservedFileState {
    fn validate(&self) -> Result<(), SchemaError> {
        match self {
            Self::Regular {
                sha256,
                file_id,
                link_count,
            } if valid_sha256(sha256) && valid_id(file_id, 256) && *link_count > 0 => Ok(()),
            Self::Missing | Self::NonRegular | Self::Unreadable => Ok(()),
            Self::Regular { .. } => Err(SchemaError::InvalidDocument("file observation")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservationSnapshot {
    pub root_identity: RootIdentity,
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub state: ObservedFileState,
    pub observation_sequence: u64,
}

impl ObservationSnapshot {
    pub fn validate(&self) -> Result<(), SchemaError> {
        self.root_identity.validate()?;
        self.state.validate()?;
        if valid_identity_key(&self.path_identity_key) && self.observation_sequence > 0 {
            Ok(())
        } else {
            Err(SchemaError::InvalidDocument("observation snapshot"))
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum FilesystemBoundaryError {
    #[error("transaction root identity changed")]
    RootIdentityChanged,
    #[error("path component is a link, reparse point, or alias")]
    UnsafeAlias,
    #[error("observed entry changed before publication")]
    ObservationChanged,
    #[error("operation lease is no longer current")]
    LostLease,
    #[error("journal sequence changed before publication")]
    StaleJournal,
    #[error("published output did not match the expected hash")]
    VerificationFailed,
    #[error("filesystem operation failed")]
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MutationSideEffect {
    Publish,
    Remove,
    RestoreBackup,
}

/// A journal transition that authorizes exactly one filesystem side effect.
/// The current and next journal are bound to the same live operation/lease,
/// and the next payload may differ only by sequence, update time, and the one
/// legal mutation checkpoint represented here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMutationTransition {
    current: LifecycleJournal,
    next: LifecycleJournal,
    current_journal_fingerprint: String,
    mutation_index: usize,
    side_effect: MutationSideEffect,
}

impl ValidatedMutationTransition {
    pub fn new(
        current: &LifecycleJournal,
        next: &LifecycleJournal,
        record: &OperationRecord,
        lease: &OperationLease,
        now_ms: u64,
        mutation_index: usize,
        side_effect: MutationSideEffect,
    ) -> Result<Self, SchemaError> {
        validate_journal_binding(current, record, lease, now_ms)?;
        validate_journal_binding(next, record, lease, now_ms)?;
        if record.state.terminal()
            || !record.request.intent().kind.mutates_files()
            || current.phase == OperationPhase::Complete
            || next.phase == OperationPhase::Complete
            || current.journal_sequence.checked_add(1) != Some(next.journal_sequence)
            || next.updated_at_ms < current.updated_at_ms
        {
            return Err(SchemaError::InvalidDocument("mutation authority"));
        }

        let mutation = current
            .mutations
            .get(mutation_index)
            .ok_or(SchemaError::InvalidDocument("mutation index"))?;
        let next_checkpoint = match (
            side_effect,
            current.phase,
            mutation.action,
            mutation.checkpoint,
        ) {
            (
                MutationSideEffect::Publish,
                OperationPhase::Applying,
                MutationAction::Create,
                MutationCheckpoint::Staged,
            )
            | (
                MutationSideEffect::Publish,
                OperationPhase::Applying,
                MutationAction::Replace,
                MutationCheckpoint::BackupVerified,
            )
            | (
                MutationSideEffect::Remove,
                OperationPhase::Applying,
                MutationAction::Delete,
                MutationCheckpoint::BackupVerified,
            ) => MutationCheckpoint::Applied,
            (
                MutationSideEffect::Remove,
                OperationPhase::RollingBack,
                MutationAction::Create,
                MutationCheckpoint::Applied | MutationCheckpoint::OutputVerified,
            )
            | (
                MutationSideEffect::RestoreBackup,
                OperationPhase::RollingBack,
                MutationAction::Replace | MutationAction::Delete,
                MutationCheckpoint::Applied | MutationCheckpoint::OutputVerified,
            ) => MutationCheckpoint::RolledBack,
            _ => {
                return Err(SchemaError::InvalidDocument(
                    "illegal mutation checkpoint transition",
                ));
            }
        };

        let mut expected = current.clone().into_payload();
        expected.journal_sequence = next.journal_sequence;
        expected.updated_at_ms = next.updated_at_ms;
        expected.mutations[mutation_index].checkpoint = next_checkpoint;
        if next.clone().into_payload() != expected {
            return Err(SchemaError::InvalidDocument(
                "mutation transition changed unrelated journal state",
            ));
        }

        let current_journal_fingerprint = current.canonical_fingerprint()?;
        Ok(Self {
            current: current.clone(),
            next: next.clone(),
            current_journal_fingerprint,
            mutation_index,
            side_effect,
        })
    }

    #[must_use]
    pub fn current_journal(&self) -> &LifecycleJournal {
        &self.current
    }

    #[must_use]
    pub fn next_journal(&self) -> &LifecycleJournal {
        &self.next
    }

    #[must_use]
    pub fn current_journal_fingerprint(&self) -> &str {
        &self.current_journal_fingerprint
    }

    #[must_use]
    pub const fn mutation_index(&self) -> usize {
        self.mutation_index
    }

    #[must_use]
    pub const fn side_effect(&self) -> MutationSideEffect {
        self.side_effect
    }

    #[must_use]
    pub fn mutation(&self) -> &JournalMutation {
        &self.current.mutations[self.mutation_index]
    }

    fn validate_snapshot_location(
        snapshot: &ObservationSnapshot,
        root: &RootIdentity,
        path: &ValidatedRelativePath,
        path_identity_key: Option<&str>,
    ) -> Result<(), FilesystemBoundaryError> {
        snapshot
            .validate()
            .map_err(|_| FilesystemBoundaryError::ObservationChanged)?;
        if &snapshot.root_identity != root {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        if &snapshot.path != path {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        if path_identity_key.is_some_and(|expected| snapshot.path_identity_key != expected) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        Ok(())
    }

    fn require_regular_hash(
        state: &ObservedFileState,
        expected_sha256: &str,
        mismatch: FilesystemBoundaryError,
    ) -> Result<(), FilesystemBoundaryError> {
        match state {
            ObservedFileState::Regular {
                sha256,
                link_count: 1,
                ..
            } if sha256.eq_ignore_ascii_case(expected_sha256) => Ok(()),
            ObservedFileState::Regular { link_count, .. } if *link_count != 1 => {
                Err(FilesystemBoundaryError::UnsafeAlias)
            }
            _ => Err(mismatch),
        }
    }

    pub fn validate_publication(
        &self,
        staged: &ObservationSnapshot,
        destination: &ObservationSnapshot,
    ) -> Result<String, FilesystemBoundaryError> {
        if self.side_effect != MutationSideEffect::Publish {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        let mutation = self.mutation();
        let staging_path = mutation
            .staging_path
            .as_ref()
            .ok_or(FilesystemBoundaryError::VerificationFailed)?;
        let expected_sha256 = mutation
            .expected_sha256
            .as_deref()
            .ok_or(FilesystemBoundaryError::VerificationFailed)?;
        Self::validate_snapshot_location(staged, &self.current.staging_root, staging_path, None)?;
        Self::require_regular_hash(
            &staged.state,
            expected_sha256,
            FilesystemBoundaryError::VerificationFailed,
        )?;
        Self::validate_snapshot_location(
            destination,
            &self.current.transaction_root,
            &mutation.path,
            Some(&mutation.path_identity_key),
        )?;
        match mutation.action {
            MutationAction::Create if destination.state == ObservedFileState::Missing => {}
            MutationAction::Replace => Self::require_regular_hash(
                &destination.state,
                mutation
                    .previous_sha256
                    .as_deref()
                    .ok_or(FilesystemBoundaryError::ObservationChanged)?,
                FilesystemBoundaryError::ObservationChanged,
            )?,
            _ => return Err(FilesystemBoundaryError::ObservationChanged),
        }
        Ok(expected_sha256.to_ascii_lowercase())
    }

    pub fn validate_removal(
        &self,
        destination: &ObservationSnapshot,
    ) -> Result<(), FilesystemBoundaryError> {
        if self.side_effect != MutationSideEffect::Remove {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        let mutation = self.mutation();
        Self::validate_snapshot_location(
            destination,
            &self.current.transaction_root,
            &mutation.path,
            Some(&mutation.path_identity_key),
        )?;
        let expected = match (self.current.phase, mutation.action) {
            (OperationPhase::Applying, MutationAction::Delete) => {
                mutation.previous_sha256.as_deref()
            }
            (OperationPhase::RollingBack, MutationAction::Create) => {
                mutation.expected_sha256.as_deref()
            }
            _ => None,
        }
        .ok_or(FilesystemBoundaryError::ObservationChanged)?;
        Self::require_regular_hash(
            &destination.state,
            expected,
            FilesystemBoundaryError::ObservationChanged,
        )
    }

    pub fn validate_backup_restoration(
        &self,
        backup: &ObservationSnapshot,
        destination: &ObservationSnapshot,
    ) -> Result<String, FilesystemBoundaryError> {
        if self.side_effect != MutationSideEffect::RestoreBackup {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        let mutation = self.mutation();
        let backup_path = mutation
            .backup_path
            .as_ref()
            .ok_or(FilesystemBoundaryError::VerificationFailed)?;
        let backup_sha256 = mutation
            .backup_sha256
            .as_deref()
            .ok_or(FilesystemBoundaryError::VerificationFailed)?;
        Self::validate_snapshot_location(backup, &self.current.backup_root, backup_path, None)?;
        Self::require_regular_hash(
            &backup.state,
            backup_sha256,
            FilesystemBoundaryError::VerificationFailed,
        )?;
        Self::validate_snapshot_location(
            destination,
            &self.current.transaction_root,
            &mutation.path,
            Some(&mutation.path_identity_key),
        )?;
        match mutation.action {
            MutationAction::Replace => Self::require_regular_hash(
                &destination.state,
                mutation
                    .expected_sha256
                    .as_deref()
                    .ok_or(FilesystemBoundaryError::ObservationChanged)?,
                FilesystemBoundaryError::ObservationChanged,
            )?,
            MutationAction::Delete if destination.state == ObservedFileState::Missing => {}
            _ => return Err(FilesystemBoundaryError::ObservationChanged),
        }
        Ok(backup_sha256.to_ascii_lowercase())
    }
}

/// Atomically fences an interrupted writer, binds a fresh recovery lease, and
/// moves its exact persisted journal into `RollingBack` before uncertain-effect
/// reconciliation. No expired lease is trusted during this transfer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedRecoveryRebind {
    stalled_journal: LifecycleJournal,
    stalled_record: OperationRecord,
    recovery_journal: LifecycleJournal,
    stalled_journal_fingerprint: String,
    recovery_record: OperationRecord,
    recovery_lease: OperationLease,
}

impl ValidatedRecoveryRebind {
    pub fn new(
        stalled_journal: &LifecycleJournal,
        stalled_record: &OperationRecord,
        recovery_journal: &LifecycleJournal,
        recovery_record: &OperationRecord,
        recovery_lease: &OperationLease,
        now_ms: u64,
    ) -> Result<Self, SchemaError> {
        stalled_journal.validate()?;
        stalled_record.validate()?;
        recovery_journal.validate()?;
        recovery_record.validate()?;
        recovery_lease.validate()?;
        let request = &stalled_record.request;
        let stalled_identity_is_bound =
            journal_record_state_is_consistent(stalled_journal, stalled_record)
                && stalled_journal.operation_id == request.operation_id()
                && stalled_journal.idempotency_key == request.idempotency_key()
                && stalled_journal
                    .request_fingerprint
                    .eq_ignore_ascii_case(request.request_fingerprint())
                && stalled_journal.installation_id == request.intent().installation_id
                && stalled_journal.operation == request.intent().kind
                && stalled_journal.operation_revision == stalled_record.revision
                && stalled_journal.fencing_token == stalled_record.fencing_token;
        let recovery_revision = stalled_record.revision;
        let next_attempt = stalled_journal.recovery_attempts.saturating_add(1);
        let recovery_phase = if stalled_journal.phase == OperationPhase::CleaningUp {
            OperationPhase::CleaningUp
        } else {
            match stalled_journal.recovery_disposition() {
                JournalDisposition::RollBack => OperationPhase::RollingBack,
                JournalDisposition::FinalizeVerifiedCommit => OperationPhase::Committing,
                JournalDisposition::Complete => {
                    return Err(SchemaError::InvalidDocument("recovery rebind authority"));
                }
            }
        };
        let recovery_record_is_derived = recovery_record.request == stalled_record.request
            && recovery_record.state == OperationState::RecoveryRequired
            && recovery_record.phase == recovery_phase
            && recovery_record.revision == recovery_revision
            && recovery_record.fencing_token == stalled_record.fencing_token
            && recovery_record.created_at_ms == stalled_record.created_at_ms
            && recovery_record.updated_at_ms >= stalled_record.updated_at_ms
            && recovery_record.result_fingerprint.is_none()
            && recovery_record.error.as_ref().is_none_or(|error| {
                error.code == crate::ProductErrorCode::RecoveryRequired
                    && error.operation_id.as_deref() == Some(request.operation_id())
            });
        if !stalled_identity_is_bound
            || stalled_record.state.terminal()
            || stalled_journal.phase == OperationPhase::Complete
            || !request.intent().kind.mutates_files()
            || !recovery_record_is_derived
            || recovery_lease.lease_id == stalled_journal.lease_id
        {
            return Err(SchemaError::InvalidDocument("recovery rebind authority"));
        }

        let mut expected = stalled_journal.clone().into_payload();
        expected.journal_sequence = stalled_journal.journal_sequence;
        expected.lease_id = recovery_lease.lease_id.clone();
        expected.recovery_chain_sha256 =
            stalled_journal.next_recovery_chain_sha256(&recovery_lease.lease_id)?;
        expected.operation_revision = recovery_record.revision;
        expected.fencing_token = recovery_record.fencing_token;
        expected.phase = recovery_phase;
        expected.updated_at_ms = recovery_journal.updated_at_ms;
        expected.recovery_attempts = next_attempt;
        if stalled_journal.journal_sequence != recovery_journal.journal_sequence
            || recovery_journal.updated_at_ms < stalled_journal.updated_at_ms
            || recovery_journal.clone().into_payload() != expected
        {
            return Err(SchemaError::InvalidDocument(
                "recovery rebind changed unrelated journal state",
            ));
        }
        validate_journal_binding(recovery_journal, recovery_record, recovery_lease, now_ms)?;

        Ok(Self {
            stalled_journal: stalled_journal.clone(),
            stalled_record: stalled_record.clone(),
            recovery_journal: recovery_journal.clone(),
            stalled_journal_fingerprint: stalled_journal.canonical_fingerprint()?,
            recovery_record: recovery_record.clone(),
            recovery_lease: recovery_lease.clone(),
        })
    }

    #[must_use]
    pub fn stalled_journal(&self) -> &LifecycleJournal {
        &self.stalled_journal
    }

    #[must_use]
    pub fn recovery_journal(&self) -> &LifecycleJournal {
        &self.recovery_journal
    }

    #[must_use]
    pub fn stalled_record(&self) -> &OperationRecord {
        &self.stalled_record
    }

    #[must_use]
    pub fn stalled_journal_fingerprint(&self) -> &str {
        &self.stalled_journal_fingerprint
    }

    #[must_use]
    pub fn recovery_record(&self) -> &OperationRecord {
        &self.recovery_record
    }

    #[must_use]
    pub fn recovery_lease(&self) -> &OperationLease {
        &self.recovery_lease
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationOutcome {
    EffectApplied,
    EffectNotApplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconciliationScope {
    /// A forward create/replace/delete may have landed before its journal CAS.
    ForwardMutation,
    /// A rollback remove/restore may have landed before its journal CAS.
    Rollback,
    /// The checkpoint proves no destination mutation could have started.
    PreEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconciliationDecision {
    mutation_index: usize,
    scope: ReconciliationScope,
    outcome: ReconciliationOutcome,
}

impl ReconciliationDecision {
    #[must_use]
    pub const fn new(
        mutation_index: usize,
        scope: ReconciliationScope,
        outcome: ReconciliationOutcome,
    ) -> Self {
        Self {
            mutation_index,
            scope,
            outcome,
        }
    }
}

/// Resolves the only intentionally uncertain window: a process may stop after
/// a filesystem effect is durably visible but before its journal CAS. Recovery
/// first moves the journal into `RollingBack`, observes the exact destination
/// without following links, then uses this value to record whether the effect
/// landed. Rollback-side reconciliation is persisted only when the rollback
/// effect landed. If it did not land, callers retry the ordinary guarded
/// rollback directly so the journal has no sequence-consuming self-loop.
/// Valid pre-effect checkpoints are recorded as `NoEffect` without mutating
/// the destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMutationReconciliation {
    current: LifecycleJournal,
    next: LifecycleJournal,
    current_journal_fingerprint: String,
    mutation_index: usize,
    scope: ReconciliationScope,
    outcome: ReconciliationOutcome,
}

impl ValidatedMutationReconciliation {
    pub fn new(
        current: &LifecycleJournal,
        next: &LifecycleJournal,
        record: &OperationRecord,
        lease: &OperationLease,
        now_ms: u64,
        decision: ReconciliationDecision,
    ) -> Result<Self, SchemaError> {
        let ReconciliationDecision {
            mutation_index,
            scope,
            outcome,
        } = decision;
        validate_journal_binding(current, record, lease, now_ms)?;
        validate_journal_binding(next, record, lease, now_ms)?;
        if record.state.terminal()
            || !record.request.intent().kind.mutates_files()
            || current.phase != OperationPhase::RollingBack
            || next.phase != OperationPhase::RollingBack
            || current.journal_sequence.checked_add(1) != Some(next.journal_sequence)
            || next.updated_at_ms < current.updated_at_ms
        {
            return Err(SchemaError::InvalidDocument(
                "mutation reconciliation authority",
            ));
        }

        let mutation = current
            .mutations
            .get(mutation_index)
            .ok_or(SchemaError::InvalidDocument("mutation index"))?;
        let checkpoint_is_eligible = match scope {
            ReconciliationScope::ForwardMutation => match mutation.action {
                MutationAction::Create => mutation.checkpoint == MutationCheckpoint::Staged,
                MutationAction::Replace | MutationAction::Delete => {
                    mutation.checkpoint == MutationCheckpoint::BackupVerified
                }
            },
            ReconciliationScope::Rollback => matches!(
                mutation.checkpoint,
                MutationCheckpoint::Applied | MutationCheckpoint::OutputVerified
            ),
            ReconciliationScope::PreEffect => {
                mutation.checkpoint == MutationCheckpoint::Planned
                    || (mutation.action == MutationAction::Replace
                        && mutation.checkpoint == MutationCheckpoint::Staged)
            }
        };
        if !checkpoint_is_eligible
            || (scope == ReconciliationScope::PreEffect
                && outcome != ReconciliationOutcome::EffectNotApplied)
            || (scope == ReconciliationScope::Rollback
                && outcome == ReconciliationOutcome::EffectNotApplied)
        {
            return Err(SchemaError::InvalidDocument(
                "mutation checkpoint is not uncertain",
            ));
        }
        let reconciled_checkpoint = match (scope, outcome) {
            (ReconciliationScope::ForwardMutation, ReconciliationOutcome::EffectApplied) => {
                MutationCheckpoint::Applied
            }
            (ReconciliationScope::ForwardMutation, ReconciliationOutcome::EffectNotApplied)
            | (ReconciliationScope::Rollback, ReconciliationOutcome::EffectApplied) => {
                MutationCheckpoint::RolledBack
            }
            (ReconciliationScope::PreEffect, ReconciliationOutcome::EffectNotApplied)
            | (ReconciliationScope::PreEffect, ReconciliationOutcome::EffectApplied) => {
                MutationCheckpoint::NoEffect
            }
            (ReconciliationScope::Rollback, ReconciliationOutcome::EffectNotApplied) => {
                return Err(SchemaError::InvalidDocument(
                    "rollback effect absence must retry without reconciliation",
                ));
            }
        };
        let mut expected = current.clone().into_payload();
        expected.journal_sequence = next.journal_sequence;
        expected.updated_at_ms = next.updated_at_ms;
        expected.mutations[mutation_index].checkpoint = reconciled_checkpoint;
        if next.clone().into_payload() != expected {
            return Err(SchemaError::InvalidDocument(
                "reconciliation changed unrelated journal state",
            ));
        }

        Ok(Self {
            current: current.clone(),
            next: next.clone(),
            current_journal_fingerprint: current.canonical_fingerprint()?,
            mutation_index,
            scope,
            outcome,
        })
    }

    #[must_use]
    pub fn current_journal(&self) -> &LifecycleJournal {
        &self.current
    }

    #[must_use]
    pub fn next_journal(&self) -> &LifecycleJournal {
        &self.next
    }

    #[must_use]
    pub fn current_journal_fingerprint(&self) -> &str {
        &self.current_journal_fingerprint
    }

    #[must_use]
    pub const fn mutation_index(&self) -> usize {
        self.mutation_index
    }

    #[must_use]
    pub const fn outcome(&self) -> ReconciliationOutcome {
        self.outcome
    }

    #[must_use]
    pub const fn scope(&self) -> ReconciliationScope {
        self.scope
    }

    #[must_use]
    pub fn mutation(&self) -> &JournalMutation {
        &self.current.mutations[self.mutation_index]
    }

    pub fn validate_destination_observation(
        &self,
        destination: &ObservationSnapshot,
    ) -> Result<(), FilesystemBoundaryError> {
        let mutation = self.mutation();
        ValidatedMutationTransition::validate_snapshot_location(
            destination,
            &self.current.transaction_root,
            &mutation.path,
            Some(&mutation.path_identity_key),
        )?;
        let expected_hash = match (self.scope, self.outcome, mutation.action) {
            (
                ReconciliationScope::ForwardMutation,
                ReconciliationOutcome::EffectApplied,
                MutationAction::Create | MutationAction::Replace,
            ) => mutation.expected_sha256.as_deref(),
            (
                ReconciliationScope::ForwardMutation | ReconciliationScope::PreEffect,
                ReconciliationOutcome::EffectNotApplied,
                MutationAction::Replace | MutationAction::Delete,
            )
            | (
                ReconciliationScope::Rollback,
                ReconciliationOutcome::EffectApplied,
                MutationAction::Replace | MutationAction::Delete,
            ) => mutation.previous_sha256.as_deref(),
            (
                ReconciliationScope::ForwardMutation,
                ReconciliationOutcome::EffectApplied,
                MutationAction::Delete,
            )
            | (
                ReconciliationScope::ForwardMutation | ReconciliationScope::PreEffect,
                ReconciliationOutcome::EffectNotApplied,
                MutationAction::Create,
            )
            | (
                ReconciliationScope::Rollback,
                ReconciliationOutcome::EffectApplied,
                MutationAction::Create,
            ) => {
                if destination.state == ObservedFileState::Missing {
                    return Ok(());
                }
                return Err(FilesystemBoundaryError::ObservationChanged);
            }
            (ReconciliationScope::Rollback, ReconciliationOutcome::EffectNotApplied, _) => {
                return Err(FilesystemBoundaryError::ObservationChanged)
            }
            (ReconciliationScope::PreEffect, ReconciliationOutcome::EffectApplied, _) => {
                return Err(FilesystemBoundaryError::ObservationChanged)
            }
        }
        .ok_or(FilesystemBoundaryError::ObservationChanged)?;
        ValidatedMutationTransition::require_regular_hash(
            &destination.state,
            expected_hash,
            FilesystemBoundaryError::ObservationChanged,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicationReceipt {
    pub root_identity: RootIdentity,
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub sha256: Option<String>,
    pub operation_id: String,
    pub lease_id: String,
    pub fencing_token: u64,
    pub operation_revision: u64,
    pub journal_sequence: u64,
}

/// Runtime observation tokens are intentionally opaque to lifecycle callers.
/// Only the filesystem boundary implementation can mint one.
pub trait RootBoundObservation {
    fn snapshot(&self) -> &ObservationSnapshot;
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MutationFenceError {
    #[error("operation lease is no longer current")]
    LostLease,
    #[error("operation lease expired")]
    Expired,
    #[error("operation revision is stale")]
    StaleOperationRevision,
    #[error("journal sequence is stale")]
    StaleJournalSequence,
    #[error("journal checkpoint is invalid")]
    InvalidJournal,
    #[error("durable mutation store failed")]
    Store,
}

/// An implementation holds the installation's exclusive durable mutation lock
/// for this guard's entire lifetime. No newer fencing token may be issued until
/// the filesystem side effect is verified and the following journal checkpoint
/// has been durably compare-and-swapped.
pub trait LifecycleMutationGuard {
    fn operation_id(&self) -> &str;
    fn installation_id(&self) -> &str;
    fn lease_id(&self) -> &str;
    fn fencing_token(&self) -> u64;
    fn operation_revision(&self) -> u64;
    fn journal_sequence(&self) -> u64;
    /// Canonical digest of the complete journal payload loaded while acquiring
    /// the exclusive lock, not merely its identity fields and sequence.
    fn locked_journal_fingerprint(&self) -> &str;

    /// Rechecks lease ID/token, expiry, operation revision, and journal
    /// sequence while the exclusive lock is still held.
    fn assert_current(&mut self, now_ms: u64) -> Result<(), MutationFenceError>;

    /// Fsyncs and CASes current sequence to current sequence + 1 after the side
    /// effect. The next journal must remain bound to this guard.
    fn checkpoint_after_side_effect(
        &mut self,
        now_ms: u64,
        transition: &ValidatedMutationTransition,
    ) -> Result<(), MutationFenceError>;

    /// Fsyncs and CASes a journal-only uncertain-effect reconciliation while
    /// retaining the same exclusive lock and full-journal binding.
    fn checkpoint_after_reconciliation(
        &mut self,
        now_ms: u64,
        reconciliation: &ValidatedMutationReconciliation,
    ) -> Result<(), MutationFenceError>;
}

/// Durable store boundary for one mutation. The implementation atomically
/// reloads the current lease, operation record, and journal; validates their
/// aggregate binding; rejects any revision/sequence mismatch; and returns a
/// guard that retains the exclusive cross-process lock until it is dropped.
pub trait LifecycleTransactionStore: crate::OperationStore {
    type Guard<'a>: LifecycleMutationGuard
    where
        Self: 'a;

    fn lock_mutation<'a>(
        &'a mut self,
        lease: &OperationLease,
        expected_record: &OperationRecord,
        expected_journal: &LifecycleJournal,
        now_ms: u64,
    ) -> Result<Self::Guard<'a>, <Self as crate::OperationStore>::Error>;

    /// Atomically compare-and-swaps the complete stalled journal, operation
    /// record, fresh recovery lease, and rolling-back journal described by
    /// `rebind`, then returns a guard locked to the recovery journal digest.
    fn rebind_and_lock_recovery<'a>(
        &'a mut self,
        rebind: &ValidatedRecoveryRebind,
        now_ms: u64,
    ) -> Result<Self::Guard<'a>, <Self as crate::OperationStore>::Error>;
}

/// Capability boundary required by lifecycle runtimes. Implementations must
/// bind all operations to opened root/parent identities and use no-follow or
/// equivalent platform primitives. Preflight path inspection alone is not an
/// implementation of this trait. Every mutating method must reassert the
/// current guard immediately before touching the filesystem, keep that guard
/// alive across output verification, and durably checkpoint the journal before
/// returning success.
pub trait LifecycleFilesystemBoundary<G: LifecycleMutationGuard> {
    type Observation: RootBoundObservation;

    fn root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError>;

    fn observe_no_follow(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<Self::Observation, FilesystemBoundaryError>;

    fn publish_verified(
        &mut self,
        guard: &mut G,
        now_ms: u64,
        staged: &Self::Observation,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError>;

    fn remove_verified(
        &mut self,
        guard: &mut G,
        now_ms: u64,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError>;

    fn restore_verified_backup(
        &mut self,
        guard: &mut G,
        now_ms: u64,
        backup: &Self::Observation,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError>;

    fn reconcile_uncertain_effect(
        &mut self,
        guard: &mut G,
        now_ms: u64,
        destination: &Self::Observation,
        reconciliation: &ValidatedMutationReconciliation,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProposedClaim {
    pub observation: ObservationSnapshot,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimAction {
    Create(FileClaim),
    AddOwner(FileClaim),
    Replace(FileClaim),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimAnalysis {
    Ready(Vec<ClaimAction>),
    Blocked(ConflictReport),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UninstallAction {
    KeepForOtherOwners(FileClaim),
    Delete { path: ValidatedRelativePath },
    AlreadyMissing { path: ValidatedRelativePath },
    Blocked(FileConflict),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ClaimError {
    #[error("invalid claim input")]
    Invalid,
}

fn observation_conflict(
    current: Option<&FileClaim>,
    proposal: &ProposedClaim,
    owner: &str,
) -> Option<FileConflict> {
    if let Some(claim) = current {
        if claim.path != proposal.observation.path {
            return Some(FileConflict {
                path: proposal.observation.path.clone(),
                path_identity_key: proposal.observation.path_identity_key.clone(),
                reason: ConflictReason::AliasedEntry,
                expected_sha256: Some(claim.sha256.clone()),
                actual_sha256: match &proposal.observation.state {
                    ObservedFileState::Regular { sha256, .. } => Some(sha256.clone()),
                    _ => None,
                },
                proposed_sha256: Some(proposal.sha256.clone()),
                existing_owners: claim.owners.clone(),
                requesting_owner: owner.to_owned(),
            });
        }
    }
    let (reason, expected_sha256, actual_sha256, existing_owners) =
        match (&proposal.observation.state, current) {
            (ObservedFileState::Missing, None) => return None,
            (
                ObservedFileState::Regular {
                    sha256,
                    file_id,
                    link_count: 1,
                },
                Some(claim),
            ) if valid_id(file_id, 256) && sha256.eq_ignore_ascii_case(&claim.sha256) => {
                return None
            }
            (ObservedFileState::Missing, Some(claim)) => (
                ConflictReason::ExternalModification,
                Some(claim.sha256.clone()),
                None,
                claim.owners.clone(),
            ),
            (
                ObservedFileState::Regular {
                    sha256,
                    file_id,
                    link_count,
                },
                current,
            ) if *link_count != 1 || !valid_id(file_id, 256) => (
                ConflictReason::AliasedEntry,
                current.map(|claim| claim.sha256.clone()),
                Some(sha256.clone()),
                current.map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
            ),
            (ObservedFileState::Regular { sha256, .. }, Some(claim)) => (
                ConflictReason::ExternalModification,
                Some(claim.sha256.clone()),
                Some(sha256.clone()),
                claim.owners.clone(),
            ),
            (ObservedFileState::Regular { sha256, .. }, None) => (
                ConflictReason::UnknownExistingFile,
                None,
                Some(sha256.clone()),
                BTreeSet::new(),
            ),
            (ObservedFileState::NonRegular, _) => (
                ConflictReason::NonRegularEntry,
                current.map(|claim| claim.sha256.clone()),
                None,
                current.map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
            ),
            (ObservedFileState::Unreadable, _) => (
                ConflictReason::UnreadableEntry,
                current.map(|claim| claim.sha256.clone()),
                None,
                current.map_or_else(BTreeSet::new, |claim| claim.owners.clone()),
            ),
        };
    Some(FileConflict {
        path: proposal.observation.path.clone(),
        path_identity_key: proposal.observation.path_identity_key.clone(),
        reason,
        expected_sha256,
        actual_sha256,
        proposed_sha256: Some(proposal.sha256.clone()),
        existing_owners,
        requesting_owner: owner.to_owned(),
    })
}

pub fn analyze_install_claims(
    installation_id: &str,
    expected_root: &RootIdentity,
    owner: &str,
    existing: &[FileClaim],
    proposed: &[ProposedClaim],
) -> Result<ClaimAnalysis, ClaimError> {
    if !valid_id(owner, 256) || !valid_id(installation_id, 256) {
        return Err(ClaimError::Invalid);
    }
    let mut inventory = BTreeMap::new();
    for claim in existing {
        claim.validate().map_err(|_| ClaimError::Invalid)?;
        if inventory
            .insert(claim.path_identity_key.clone(), claim)
            .is_some()
        {
            return Err(ClaimError::Invalid);
        }
    }
    let mut seen = BTreeSet::new();
    let mut actions = Vec::new();
    let mut conflicts = Vec::new();
    for proposal in proposed {
        if proposal.observation.validate().is_err()
            || proposal.observation.root_identity != *expected_root
            || !valid_sha256(&proposal.sha256)
            || !seen.insert(proposal.observation.path_identity_key.clone())
        {
            return Err(ClaimError::Invalid);
        }
        let current = inventory
            .get(&proposal.observation.path_identity_key)
            .copied();
        if let Some(conflict) = observation_conflict(current, proposal, owner) {
            conflicts.push(conflict);
            continue;
        }
        match current {
            None => actions.push(ClaimAction::Create(FileClaim {
                path: proposal.observation.path.clone(),
                path_identity_key: proposal.observation.path_identity_key.clone(),
                sha256: proposal.sha256.to_ascii_lowercase(),
                owners: BTreeSet::from([owner.to_owned()]),
            })),
            Some(current) if current.sha256.eq_ignore_ascii_case(&proposal.sha256) => {
                let mut next = current.clone();
                next.owners.insert(owner.to_owned());
                actions.push(ClaimAction::AddOwner(next));
            }
            Some(current) if current.owners.len() == 1 && current.owners.contains(owner) => {
                actions.push(ClaimAction::Replace(FileClaim {
                    path: current.path.clone(),
                    path_identity_key: current.path_identity_key.clone(),
                    sha256: proposal.sha256.to_ascii_lowercase(),
                    owners: current.owners.clone(),
                }));
            }
            Some(current) => conflicts.push(FileConflict {
                path: current.path.clone(),
                path_identity_key: current.path_identity_key.clone(),
                reason: ConflictReason::DifferentContent,
                expected_sha256: Some(current.sha256.clone()),
                actual_sha256: Some(current.sha256.clone()),
                proposed_sha256: Some(proposal.sha256.to_ascii_lowercase()),
                existing_owners: current.owners.clone(),
                requesting_owner: owner.to_owned(),
            }),
        }
    }
    if conflicts.is_empty() {
        Ok(ClaimAnalysis::Ready(actions))
    } else {
        Ok(ClaimAnalysis::Blocked(
            ConflictReport::new(ConflictReportPayload {
                installation_id: installation_id.to_owned(),
                conflicts,
            })
            .map_err(|_| ClaimError::Invalid)?,
        ))
    }
}

pub fn plan_uninstall_claim(
    owner: &str,
    claim: &FileClaim,
    expected_root: &RootIdentity,
    observation: ObservationSnapshot,
) -> Result<UninstallAction, ClaimError> {
    if !valid_id(owner, 256)
        || !claim.owners.contains(owner)
        || claim.validate().is_err()
        || observation.validate().is_err()
        || observation.root_identity != *expected_root
        || observation.path != claim.path
        || observation.path_identity_key != claim.path_identity_key
    {
        return Err(ClaimError::Invalid);
    }
    let proposal = ProposedClaim {
        observation,
        sha256: claim.sha256.clone(),
    };
    if let Some(conflict) = observation_conflict(Some(claim), &proposal, owner) {
        if proposal.observation.state == ObservedFileState::Missing && claim.owners.len() == 1 {
            return Ok(UninstallAction::AlreadyMissing {
                path: claim.path.clone(),
            });
        }
        return Ok(UninstallAction::Blocked(conflict));
    }
    let mut remaining = claim.clone();
    remaining.owners.remove(owner);
    if remaining.owners.is_empty() {
        Ok(UninstallAction::Delete {
            path: claim.path.clone(),
        })
    } else {
        Ok(UninstallAction::KeepForOtherOwners(remaining))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const H2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn path() -> ValidatedRelativePath {
        ValidatedRelativePath::parse("mods/shared.dat").unwrap()
    }

    fn owned(owners: &[&str]) -> FileClaim {
        FileClaim {
            path: path(),
            path_identity_key: "mods/shared.dat".into(),
            sha256: H.into(),
            owners: owners.iter().map(|owner| (*owner).to_owned()).collect(),
        }
    }

    fn root() -> RootIdentity {
        crate::fixtures::root_identity("transaction")
    }

    fn observation(observed: ObservedFileState) -> ObservationSnapshot {
        ObservationSnapshot {
            root_identity: root(),
            path: path(),
            path_identity_key: "mods/shared.dat".into(),
            state: observed,
            observation_sequence: 1,
        }
    }

    fn proposed(hash: &str, observed: ObservedFileState) -> ProposedClaim {
        ProposedClaim {
            observation: observation(observed),
            sha256: hash.into(),
        }
    }

    fn regular(hash: &str) -> ObservedFileState {
        ObservedFileState::Regular {
            sha256: hash.into(),
            file_id: "file-1".into(),
            link_count: 1,
        }
    }

    #[test]
    fn identical_content_can_be_co_owned() {
        let ClaimAnalysis::Ready(actions) = analyze_install_claims(
            "game",
            &root(),
            "mod-b",
            &[owned(&["mod-a"])],
            &[proposed(H, regular(H))],
        )
        .unwrap() else {
            panic!("expected ready plan");
        };
        let ClaimAction::AddOwner(claim) = &actions[0] else {
            panic!("expected co-ownership");
        };
        assert_eq!(
            claim.owners,
            BTreeSet::from(["mod-a".into(), "mod-b".into()])
        );
    }

    #[test]
    fn blocking_conflict_exposes_no_executable_actions() {
        let result = analyze_install_claims(
            "game",
            &root(),
            "mod-b",
            &[owned(&["mod-a", "mod-b"])],
            &[proposed(H2, regular(H))],
        )
        .unwrap();
        assert!(matches!(result, ClaimAnalysis::Blocked(report) if report.blocking()));
    }

    #[test]
    fn external_change_blocks_replace_coownership_and_uninstall() {
        let claim = owned(&["mod-a"]);
        let result = analyze_install_claims(
            "game",
            &root(),
            "mod-a",
            std::slice::from_ref(&claim),
            &[proposed(H2, regular(H2))],
        )
        .unwrap();
        assert!(matches!(result, ClaimAnalysis::Blocked(_)));
        assert!(matches!(
            plan_uninstall_claim("mod-a", &claim, &root(), observation(regular(H2))).unwrap(),
            UninstallAction::Blocked(_)
        ));
    }

    #[test]
    fn hard_link_alias_is_a_blocking_conflict() {
        let claim = owned(&["mod-a"]);
        let result = analyze_install_claims(
            "game",
            &root(),
            "mod-a",
            &[claim],
            &[proposed(
                H2,
                ObservedFileState::Regular {
                    sha256: H.into(),
                    file_id: "file-1".into(),
                    link_count: 2,
                },
            )],
        )
        .unwrap();
        assert!(matches!(
            result,
            ClaimAnalysis::Blocked(report)
                if report.conflicts[0].reason == ConflictReason::AliasedEntry
        ));
    }

    #[test]
    fn uninstall_preserves_until_last_owner() {
        let shared = owned(&["mod-a", "mod-b"]);
        let UninstallAction::KeepForOtherOwners(remaining) =
            plan_uninstall_claim("mod-a", &shared, &root(), observation(regular(H))).unwrap()
        else {
            panic!("expected shared file to remain");
        };
        assert_eq!(remaining.owners, BTreeSet::from(["mod-b".into()]));
        assert!(matches!(
            plan_uninstall_claim("mod-b", &remaining, &root(), observation(regular(H))).unwrap(),
            UninstallAction::Delete { .. }
        ));
    }

    #[test]
    fn installed_record_and_authoritative_ledger_must_share_one_generation() {
        let record = crate::fixtures::installed_mod_record();
        let mut ledger = crate::fixtures::claims_ledger();
        validate_installed_record_against_ledger(&record, &ledger).unwrap();
        ledger
            .try_update(|payload| payload.manifest_generation += 1)
            .unwrap();
        assert!(validate_installed_record_against_ledger(&record, &ledger).is_err());
    }

    #[test]
    fn recovery_disposition_is_deterministic_at_every_checkpoint() {
        let checkpoints = [
            (MutationCheckpoint::Planned, JournalDisposition::RollBack),
            (MutationCheckpoint::Staged, JournalDisposition::RollBack),
            (
                MutationCheckpoint::BackupVerified,
                JournalDisposition::RollBack,
            ),
            (MutationCheckpoint::Applied, JournalDisposition::RollBack),
            (
                MutationCheckpoint::OutputVerified,
                JournalDisposition::FinalizeVerifiedCommit,
            ),
        ];
        for (checkpoint, expected) in checkpoints {
            let mut journal = crate::fixtures::lifecycle_journal();
            journal
                .try_update(|payload| {
                    payload.phase = match checkpoint {
                        MutationCheckpoint::OutputVerified => OperationPhase::Committing,
                        MutationCheckpoint::Applied => OperationPhase::Verifying,
                        _ => OperationPhase::Applying,
                    };
                    payload.manifest_commit_state = ManifestCommitState::NotStarted;
                    payload.mutations[0].checkpoint = checkpoint;
                    payload.mutations[0].staging_sha256 =
                        (checkpoint != MutationCheckpoint::Planned).then(|| H2.into());
                    payload.mutations[0].backup_sha256 = matches!(
                        checkpoint,
                        MutationCheckpoint::BackupVerified
                            | MutationCheckpoint::Applied
                            | MutationCheckpoint::OutputVerified
                            | MutationCheckpoint::RolledBack
                    )
                    .then(|| H.into());
                })
                .unwrap();
            journal.validate().unwrap();
            assert_eq!(journal.recovery_disposition(), expected, "{checkpoint:?}");
            assert_eq!(
                journal.recovery_disposition(),
                expected,
                "second recovery pass"
            );
        }
    }

    #[test]
    fn durable_rollback_intent_never_reverses_into_commit() {
        for manifest_state in [
            ManifestCommitState::NotStarted,
            ManifestCommitState::TemporaryWritten,
        ] {
            let mut journal = crate::fixtures::lifecycle_journal();
            journal
                .try_update(|payload| {
                    payload.phase = OperationPhase::RollingBack;
                    payload.manifest_commit_state = manifest_state;
                })
                .unwrap();
            assert_eq!(journal.recovery_disposition(), JournalDisposition::RollBack);
        }
    }

    #[test]
    fn journal_is_bound_to_the_exact_record_and_live_lease() {
        let journal = crate::fixtures::lifecycle_journal();
        let record = crate::fixtures::operation_record();
        let lease = crate::fixtures::operation_lease();
        validate_journal_binding(&journal, &record, &lease, 1_700_000_000_500).unwrap();

        let mut wrong_kind = journal.clone();
        wrong_kind
            .try_update(|payload| payload.operation = LifecycleOperationKind::Update)
            .unwrap();
        assert!(validate_journal_binding(&wrong_kind, &record, &lease, 1_700_000_000_500).is_err());

        let mut wrong_revision = journal.clone();
        wrong_revision
            .try_update(|payload| payload.operation_revision += 1)
            .unwrap();
        assert!(
            validate_journal_binding(&wrong_revision, &record, &lease, 1_700_000_000_500).is_err()
        );

        let mut wrong_fence = journal.clone();
        wrong_fence
            .try_update(|payload| payload.fencing_token += 1)
            .unwrap();
        assert!(
            validate_journal_binding(&wrong_fence, &record, &lease, 1_700_000_000_500).is_err()
        );

        let mut contradictory_phase = journal.clone();
        contradictory_phase
            .try_update(|payload| {
                payload.phase = OperationPhase::RollingBack;
                payload.manifest_commit_state = ManifestCommitState::TemporaryWritten;
            })
            .unwrap();
        assert!(
            validate_journal_binding(&contradictory_phase, &record, &lease, 1_700_000_000_500)
                .is_err()
        );

        assert!(validate_journal_binding(&journal, &record, &lease, lease.expires_at_ms).is_err());
    }

    #[test]
    fn contradictory_and_vacuous_journal_states_are_rejected() {
        let mut empty = crate::fixtures::lifecycle_journal().into_payload();
        empty.mutations.clear();
        assert!(LifecycleJournal::new(empty).is_err());

        let mut preflight_applied = crate::fixtures::lifecycle_journal().into_payload();
        preflight_applied.phase = OperationPhase::Preflight;
        preflight_applied.manifest_commit_state = ManifestCommitState::NotStarted;
        preflight_applied.mutations[0].checkpoint = MutationCheckpoint::Applied;
        assert!(LifecycleJournal::new(preflight_applied).is_err());

        let mut complete_temporary = crate::fixtures::lifecycle_journal().into_payload();
        complete_temporary.manifest_commit_state = ManifestCommitState::TemporaryWritten;
        assert!(LifecycleJournal::new(complete_temporary).is_err());

        let mut delete_staged = crate::fixtures::lifecycle_journal().into_payload();
        delete_staged.phase = OperationPhase::Staging;
        delete_staged.manifest_commit_state = ManifestCommitState::NotStarted;
        let mutation = &mut delete_staged.mutations[0];
        mutation.action = MutationAction::Delete;
        mutation.checkpoint = MutationCheckpoint::Staged;
        mutation.expected_sha256 = None;
        mutation.staging_path = None;
        mutation.staging_sha256 = None;
        assert!(LifecycleJournal::new(delete_staged).is_err());

        let mut create_backed_up = crate::fixtures::lifecycle_journal().into_payload();
        create_backed_up.phase = OperationPhase::BackingUp;
        create_backed_up.manifest_commit_state = ManifestCommitState::NotStarted;
        let mutation = &mut create_backed_up.mutations[0];
        mutation.action = MutationAction::Create;
        mutation.checkpoint = MutationCheckpoint::BackupVerified;
        mutation.previous_sha256 = None;
        mutation.backup_path = None;
        mutation.backup_sha256 = None;
        assert!(LifecycleJournal::new(create_backed_up).is_err());

        let mut overflow = crate::fixtures::lifecycle_journal().into_payload();
        overflow.manifest_generation_before = u64::MAX;
        overflow.manifest_generation_after = u64::MAX;
        assert!(LifecycleJournal::new(overflow).is_err());

        let mut aliased_roots = crate::fixtures::lifecycle_journal().into_payload();
        aliased_roots.staging_root.volume_id = aliased_roots.transaction_root.volume_id.clone();
        aliased_roots.staging_root.file_id = aliased_roots.transaction_root.file_id.clone();
        aliased_roots.staging_root.canonical_path_sha256 = H2.into();
        assert!(LifecycleJournal::new(aliased_roots).is_err());

        let mut same_canonical_root = crate::fixtures::lifecycle_journal().into_payload();
        same_canonical_root.staging_root.canonical_path_sha256 = same_canonical_root
            .transaction_root
            .canonical_path_sha256
            .clone();
        assert!(LifecycleJournal::new(same_canonical_root).is_err());

        for pair in 0..3 {
            let mut case_alias = crate::fixtures::lifecycle_journal().into_payload();
            match pair {
                0 => {
                    case_alias.staging_root.canonical_path_sha256 = case_alias
                        .transaction_root
                        .canonical_path_sha256
                        .to_ascii_uppercase();
                }
                1 => {
                    case_alias.backup_root.canonical_path_sha256 = case_alias
                        .transaction_root
                        .canonical_path_sha256
                        .to_ascii_uppercase();
                }
                _ => {
                    case_alias.backup_root.canonical_path_sha256 = case_alias
                        .staging_root
                        .canonical_path_sha256
                        .to_ascii_uppercase();
                }
            }
            assert!(LifecycleJournal::new(case_alias).is_err());
        }
    }

    #[test]
    fn provider_identity_never_substitutes_for_installed_instance_identity() {
        let first = crate::fixtures::installed_mod_record();
        let mut second_payload = first.clone().into_payload();
        second_payload.instance_id = "fixture-instance-2".into();
        let second = InstalledModRecord::new(second_payload).unwrap();
        assert_eq!(first.mod_id, second.mod_id);
        assert_eq!(first.provider, second.provider);
        assert_ne!(first.instance_id, second.instance_id);
        assert_ne!(first.key(), second.key());

        let mut ledger = crate::fixtures::claims_ledger();
        ledger
            .try_update(|payload| {
                payload.claims[0].owners.insert(second.instance_id.clone());
            })
            .unwrap();
        validate_installation_manifest(&[first.clone(), second.clone()], &ledger).unwrap();

        assert!(validate_installation_manifest(&[first.clone(), first.clone()], &ledger).is_err());
        let mut orphaned = ledger;
        orphaned
            .try_update(|payload| {
                payload.claims[0].owners.insert("ghost-instance".into());
            })
            .unwrap();
        assert!(validate_installation_manifest(&[second], &orphaned).is_err());

        let mut other_installation_payload = crate::fixtures::installed_mod_record().into_payload();
        other_installation_payload.installation_id = "other-installation".into();
        let other_installation = InstalledModRecord::new(other_installation_payload).unwrap();
        assert_eq!(first.instance_id, other_installation.instance_id);
        assert_ne!(first.key(), other_installation.key());
        let mut other_ledger_payload = crate::fixtures::claims_ledger().into_payload();
        other_ledger_payload.installation_id = "other-installation".into();
        let other_ledger = InstallationClaimsLedger::new(other_ledger_payload).unwrap();
        validate_installation_manifest(&[other_installation], &other_ledger).unwrap();
    }

    #[derive(Clone)]
    struct FakeObservation(ObservationSnapshot);

    impl RootBoundObservation for FakeObservation {
        fn snapshot(&self) -> &ObservationSnapshot {
            &self.0
        }
    }

    struct FakeGuard {
        operation_id: String,
        installation_id: String,
        lease_id: String,
        fencing_token: u64,
        operation_revision: u64,
        journal_sequence: u64,
        locked_journal_fingerprint: String,
        expires_at_ms: u64,
        current: bool,
    }

    impl LifecycleMutationGuard for FakeGuard {
        fn operation_id(&self) -> &str {
            &self.operation_id
        }

        fn installation_id(&self) -> &str {
            &self.installation_id
        }

        fn lease_id(&self) -> &str {
            &self.lease_id
        }

        fn fencing_token(&self) -> u64 {
            self.fencing_token
        }

        fn operation_revision(&self) -> u64 {
            self.operation_revision
        }

        fn journal_sequence(&self) -> u64 {
            self.journal_sequence
        }

        fn locked_journal_fingerprint(&self) -> &str {
            &self.locked_journal_fingerprint
        }

        fn assert_current(&mut self, now_ms: u64) -> Result<(), MutationFenceError> {
            if !self.current {
                return Err(MutationFenceError::LostLease);
            }
            if now_ms >= self.expires_at_ms {
                return Err(MutationFenceError::Expired);
            }
            Ok(())
        }

        fn checkpoint_after_side_effect(
            &mut self,
            now_ms: u64,
            transition: &ValidatedMutationTransition,
        ) -> Result<(), MutationFenceError> {
            self.assert_current(now_ms)?;
            let current = transition.current_journal();
            let next = transition.next_journal();
            if transition.current_journal_fingerprint() != self.locked_journal_fingerprint
                || current.operation_id != self.operation_id
                || current.installation_id != self.installation_id
                || current.lease_id != self.lease_id
                || current.fencing_token != self.fencing_token
                || current.operation_revision != self.operation_revision
                || current.journal_sequence != self.journal_sequence
                || next.operation_id != self.operation_id
                || next.installation_id != self.installation_id
                || next.lease_id != self.lease_id
                || next.fencing_token != self.fencing_token
                || next.operation_revision != self.operation_revision
                || self.journal_sequence.checked_add(1) != Some(next.journal_sequence)
            {
                return Err(MutationFenceError::InvalidJournal);
            }
            self.journal_sequence = next.journal_sequence;
            self.locked_journal_fingerprint = next
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
            let current = reconciliation.current_journal();
            let next = reconciliation.next_journal();
            if reconciliation.current_journal_fingerprint() != self.locked_journal_fingerprint
                || current.operation_id != self.operation_id
                || current.installation_id != self.installation_id
                || current.lease_id != self.lease_id
                || current.fencing_token != self.fencing_token
                || current.operation_revision != self.operation_revision
                || current.journal_sequence != self.journal_sequence
                || next.operation_id != self.operation_id
                || next.installation_id != self.installation_id
                || next.lease_id != self.lease_id
                || next.fencing_token != self.fencing_token
                || next.operation_revision != self.operation_revision
                || self.journal_sequence.checked_add(1) != Some(next.journal_sequence)
            {
                return Err(MutationFenceError::InvalidJournal);
            }
            self.journal_sequence = next.journal_sequence;
            self.locked_journal_fingerprint = next
                .canonical_fingerprint()
                .map_err(|_| MutationFenceError::InvalidJournal)?;
            Ok(())
        }
    }

    struct FakeBoundary {
        transaction_root: RootIdentity,
        roots: Vec<RootIdentity>,
        entries: BTreeMap<(String, String), ObservedFileState>,
    }

    impl FakeBoundary {
        fn entry_key(root: &RootIdentity, path: &ValidatedRelativePath) -> (String, String) {
            (root.canonical_path_sha256.clone(), path.as_str().to_owned())
        }

        fn ensure_current(
            &self,
            observation: &FakeObservation,
        ) -> Result<(), FilesystemBoundaryError> {
            if !self.roots.contains(&observation.0.root_identity) {
                return Err(FilesystemBoundaryError::RootIdentityChanged);
            }
            let current = self
                .entries
                .get(&Self::entry_key(
                    &observation.0.root_identity,
                    &observation.0.path,
                ))
                .cloned()
                .unwrap_or(ObservedFileState::Missing);
            if current != observation.0.state {
                return Err(FilesystemBoundaryError::ObservationChanged);
            }
            Ok(())
        }

        fn receipt(
            guard: &impl LifecycleMutationGuard,
            observation: &FakeObservation,
            sha256: Option<String>,
        ) -> PublicationReceipt {
            PublicationReceipt {
                root_identity: observation.0.root_identity.clone(),
                path: observation.0.path.clone(),
                path_identity_key: observation.0.path_identity_key.clone(),
                sha256,
                operation_id: guard.operation_id().to_owned(),
                lease_id: guard.lease_id().to_owned(),
                fencing_token: guard.fencing_token(),
                operation_revision: guard.operation_revision(),
                journal_sequence: guard.journal_sequence(),
            }
        }

        fn check_guard(
            guard: &mut impl LifecycleMutationGuard,
            now_ms: u64,
            transition: &ValidatedMutationTransition,
        ) -> Result<(), FilesystemBoundaryError> {
            guard.assert_current(now_ms).map_err(|error| match error {
                MutationFenceError::StaleJournalSequence => FilesystemBoundaryError::StaleJournal,
                _ => FilesystemBoundaryError::LostLease,
            })?;
            let current = transition.current_journal();
            if guard.locked_journal_fingerprint() != transition.current_journal_fingerprint()
                || guard.operation_id() != current.operation_id
                || guard.installation_id() != current.installation_id
                || guard.lease_id() != current.lease_id
                || guard.fencing_token() != current.fencing_token
                || guard.operation_revision() != current.operation_revision
                || guard.journal_sequence() != current.journal_sequence
            {
                return Err(FilesystemBoundaryError::StaleJournal);
            }
            Ok(())
        }

        fn check_reconciliation_guard(
            guard: &mut impl LifecycleMutationGuard,
            now_ms: u64,
            reconciliation: &ValidatedMutationReconciliation,
        ) -> Result<(), FilesystemBoundaryError> {
            guard.assert_current(now_ms).map_err(|error| match error {
                MutationFenceError::StaleJournalSequence => FilesystemBoundaryError::StaleJournal,
                _ => FilesystemBoundaryError::LostLease,
            })?;
            let current = reconciliation.current_journal();
            if guard.locked_journal_fingerprint() != reconciliation.current_journal_fingerprint()
                || guard.operation_id() != current.operation_id
                || guard.installation_id() != current.installation_id
                || guard.lease_id() != current.lease_id
                || guard.fencing_token() != current.fencing_token
                || guard.operation_revision() != current.operation_revision
                || guard.journal_sequence() != current.journal_sequence
            {
                return Err(FilesystemBoundaryError::StaleJournal);
            }
            Ok(())
        }

        fn checkpoint(
            guard: &mut impl LifecycleMutationGuard,
            now_ms: u64,
            transition: &ValidatedMutationTransition,
        ) -> Result<(), FilesystemBoundaryError> {
            guard
                .checkpoint_after_side_effect(now_ms, transition)
                .map_err(|error| match error {
                    MutationFenceError::StaleJournalSequence => {
                        FilesystemBoundaryError::StaleJournal
                    }
                    _ => FilesystemBoundaryError::LostLease,
                })
        }

        fn checkpoint_reconciliation(
            guard: &mut impl LifecycleMutationGuard,
            now_ms: u64,
            reconciliation: &ValidatedMutationReconciliation,
        ) -> Result<(), FilesystemBoundaryError> {
            guard
                .checkpoint_after_reconciliation(now_ms, reconciliation)
                .map_err(|error| match error {
                    MutationFenceError::StaleJournalSequence => {
                        FilesystemBoundaryError::StaleJournal
                    }
                    _ => FilesystemBoundaryError::LostLease,
                })
        }
    }

    impl LifecycleFilesystemBoundary<FakeGuard> for FakeBoundary {
        type Observation = FakeObservation;

        fn root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError> {
            Ok(self.transaction_root.clone())
        }

        fn observe_no_follow(
            &self,
            expected_root: &RootIdentity,
            path: &ValidatedRelativePath,
        ) -> Result<Self::Observation, FilesystemBoundaryError> {
            if !self.roots.contains(expected_root) {
                return Err(FilesystemBoundaryError::RootIdentityChanged);
            }
            Ok(FakeObservation(ObservationSnapshot {
                root_identity: expected_root.clone(),
                path: path.clone(),
                path_identity_key: path.as_str().to_owned(),
                state: self
                    .entries
                    .get(&Self::entry_key(expected_root, path))
                    .cloned()
                    .unwrap_or(ObservedFileState::Missing),
                observation_sequence: 1,
            }))
        }

        fn publish_verified(
            &mut self,
            guard: &mut FakeGuard,
            now_ms: u64,
            staged: &Self::Observation,
            destination: &Self::Observation,
            transition: &ValidatedMutationTransition,
        ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
            Self::check_guard(guard, now_ms, transition)?;
            self.ensure_current(staged)?;
            self.ensure_current(destination)?;
            let expected_output_sha256 =
                transition.validate_publication(&staged.0, &destination.0)?;
            self.entries.insert(
                Self::entry_key(&destination.0.root_identity, &destination.0.path),
                staged.0.state.clone(),
            );
            Self::checkpoint(guard, now_ms, transition)?;
            Ok(Self::receipt(
                guard,
                destination,
                Some(expected_output_sha256),
            ))
        }

        fn remove_verified(
            &mut self,
            guard: &mut FakeGuard,
            now_ms: u64,
            destination: &Self::Observation,
            transition: &ValidatedMutationTransition,
        ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
            Self::check_guard(guard, now_ms, transition)?;
            self.ensure_current(destination)?;
            transition.validate_removal(&destination.0)?;
            self.entries.remove(&Self::entry_key(
                &destination.0.root_identity,
                &destination.0.path,
            ));
            Self::checkpoint(guard, now_ms, transition)?;
            Ok(Self::receipt(guard, destination, None))
        }

        fn restore_verified_backup(
            &mut self,
            guard: &mut FakeGuard,
            now_ms: u64,
            backup: &Self::Observation,
            destination: &Self::Observation,
            transition: &ValidatedMutationTransition,
        ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
            Self::check_guard(guard, now_ms, transition)?;
            self.ensure_current(backup)?;
            self.ensure_current(destination)?;
            let expected_backup_sha256 =
                transition.validate_backup_restoration(&backup.0, &destination.0)?;
            self.entries.insert(
                Self::entry_key(&destination.0.root_identity, &destination.0.path),
                backup.0.state.clone(),
            );
            Self::checkpoint(guard, now_ms, transition)?;
            Ok(Self::receipt(
                guard,
                destination,
                Some(expected_backup_sha256),
            ))
        }

        fn reconcile_uncertain_effect(
            &mut self,
            guard: &mut FakeGuard,
            now_ms: u64,
            destination: &Self::Observation,
            reconciliation: &ValidatedMutationReconciliation,
        ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
            Self::check_reconciliation_guard(guard, now_ms, reconciliation)?;
            self.ensure_current(destination)?;
            reconciliation.validate_destination_observation(&destination.0)?;
            let sha256 = match &destination.0.state {
                ObservedFileState::Regular { sha256, .. } => Some(sha256.clone()),
                _ => None,
            };
            Self::checkpoint_reconciliation(guard, now_ms, reconciliation)?;
            Ok(Self::receipt(guard, destination, sha256))
        }
    }

    fn guard_for(journal: &LifecycleJournal) -> FakeGuard {
        FakeGuard {
            installation_id: journal.installation_id.clone(),
            operation_id: journal.operation_id.clone(),
            lease_id: journal.lease_id.clone(),
            fencing_token: journal.fencing_token,
            operation_revision: journal.operation_revision,
            journal_sequence: journal.journal_sequence,
            locked_journal_fingerprint: journal.canonical_fingerprint().unwrap(),
            expires_at_ms: 10,
            current: true,
        }
    }

    fn applying_context() -> (
        LifecycleJournal,
        LifecycleJournal,
        OperationRecord,
        OperationLease,
        ValidatedMutationTransition,
    ) {
        let mut current_payload = crate::fixtures::lifecycle_journal().into_payload();
        current_payload.phase = OperationPhase::Applying;
        current_payload.manifest_commit_state = ManifestCommitState::NotStarted;
        current_payload.mutations[0].checkpoint = MutationCheckpoint::BackupVerified;
        let current = LifecycleJournal::new(current_payload).unwrap();

        let mut next_payload = current.clone().into_payload();
        next_payload.journal_sequence = 8;
        next_payload.updated_at_ms += 1;
        next_payload.mutations[0].checkpoint = MutationCheckpoint::Applied;
        let next = LifecycleJournal::new(next_payload).unwrap();

        let mut record_payload = crate::fixtures::operation_record().into_payload();
        record_payload.state = OperationState::Running;
        record_payload.phase = OperationPhase::Applying;
        record_payload.result_fingerprint = None;
        let record = OperationRecord::new(record_payload).unwrap();

        let mut lease = crate::fixtures::operation_lease();
        lease.acquired_at_ms = 1;
        lease.expires_at_ms = 10;
        let transition = ValidatedMutationTransition::new(
            &current,
            &next,
            &record,
            &lease,
            2,
            0,
            MutationSideEffect::Publish,
        )
        .unwrap();
        (current, next, record, lease, transition)
    }

    fn reconciliation_context(
        action: MutationAction,
        outcome: ReconciliationOutcome,
    ) -> (
        LifecycleJournal,
        LifecycleJournal,
        OperationRecord,
        OperationLease,
        ValidatedMutationReconciliation,
        ObservationSnapshot,
    ) {
        let operation = if action == MutationAction::Delete {
            LifecycleOperationKind::Uninstall
        } else {
            LifecycleOperationKind::Install
        };
        let mut current_payload = crate::fixtures::lifecycle_journal().into_payload();
        current_payload.phase = OperationPhase::RollingBack;
        current_payload.operation = operation;
        current_payload.manifest_commit_state = ManifestCommitState::NotStarted;
        let mutation = &mut current_payload.mutations[0];
        mutation.action = action;
        match action {
            MutationAction::Create => {
                mutation.checkpoint = MutationCheckpoint::Staged;
                mutation.previous_sha256 = None;
                mutation.expected_sha256 = Some(H2.into());
                mutation.staging_sha256 = Some(H2.into());
                mutation.backup_path = None;
                mutation.backup_sha256 = None;
            }
            MutationAction::Replace => {
                mutation.checkpoint = MutationCheckpoint::BackupVerified;
            }
            MutationAction::Delete => {
                mutation.checkpoint = MutationCheckpoint::BackupVerified;
                mutation.expected_sha256 = None;
                mutation.staging_path = None;
                mutation.staging_sha256 = None;
            }
        }

        let mut intent = crate::fixtures::operation_intent();
        intent.kind = operation;
        let request = crate::OperationRequest::new("operation-1", "request-1", intent).unwrap();
        current_payload.request_fingerprint = request.request_fingerprint().into();
        let current = LifecycleJournal::new(current_payload).unwrap();

        let mut next_payload = current.clone().into_payload();
        next_payload.journal_sequence += 1;
        next_payload.updated_at_ms += 1;
        next_payload.mutations[0].checkpoint = match outcome {
            ReconciliationOutcome::EffectApplied => MutationCheckpoint::Applied,
            ReconciliationOutcome::EffectNotApplied => MutationCheckpoint::RolledBack,
        };
        let next = LifecycleJournal::new(next_payload).unwrap();

        let mut record_payload = crate::fixtures::operation_record().into_payload();
        record_payload.request = request;
        record_payload.state = OperationState::RecoveryRequired;
        record_payload.phase = OperationPhase::RollingBack;
        record_payload.result_fingerprint = None;
        let record = OperationRecord::new(record_payload).unwrap();
        let mut lease = crate::fixtures::operation_lease();
        lease.acquired_at_ms = 1;
        lease.expires_at_ms = 10;
        let reconciliation = ValidatedMutationReconciliation::new(
            &current,
            &next,
            &record,
            &lease,
            2,
            ReconciliationDecision::new(0, ReconciliationScope::ForwardMutation, outcome),
        )
        .unwrap();

        let state = match (outcome, action) {
            (ReconciliationOutcome::EffectApplied, MutationAction::Create)
            | (ReconciliationOutcome::EffectApplied, MutationAction::Replace) => regular(H2),
            (ReconciliationOutcome::EffectNotApplied, MutationAction::Replace)
            | (ReconciliationOutcome::EffectNotApplied, MutationAction::Delete) => regular(H),
            (ReconciliationOutcome::EffectApplied, MutationAction::Delete)
            | (ReconciliationOutcome::EffectNotApplied, MutationAction::Create) => {
                ObservedFileState::Missing
            }
        };
        let observation = ObservationSnapshot {
            root_identity: current.transaction_root.clone(),
            path: current.mutations[0].path.clone(),
            path_identity_key: current.mutations[0].path_identity_key.clone(),
            state,
            observation_sequence: 1,
        };
        (current, next, record, lease, reconciliation, observation)
    }

    fn rolling_back_context(
        action: MutationAction,
        checkpoint: MutationCheckpoint,
    ) -> (LifecycleJournal, OperationRecord, OperationLease) {
        let operation = if action == MutationAction::Delete {
            LifecycleOperationKind::Uninstall
        } else {
            LifecycleOperationKind::Install
        };
        let mut journal_payload = crate::fixtures::lifecycle_journal().into_payload();
        journal_payload.phase = OperationPhase::RollingBack;
        journal_payload.operation = operation;
        journal_payload.manifest_commit_state = ManifestCommitState::NotStarted;
        let mutation = &mut journal_payload.mutations[0];
        mutation.action = action;
        mutation.checkpoint = checkpoint;
        match action {
            MutationAction::Create => {
                mutation.previous_sha256 = None;
                mutation.expected_sha256 = Some(H2.into());
                mutation.staging_sha256 =
                    (checkpoint != MutationCheckpoint::Planned).then(|| H2.to_owned());
                mutation.backup_path = None;
                mutation.backup_sha256 = None;
            }
            MutationAction::Replace => {
                mutation.staging_sha256 =
                    (checkpoint != MutationCheckpoint::Planned).then(|| H2.to_owned());
                mutation.backup_sha256 = matches!(
                    checkpoint,
                    MutationCheckpoint::BackupVerified
                        | MutationCheckpoint::Applied
                        | MutationCheckpoint::OutputVerified
                        | MutationCheckpoint::RolledBack
                )
                .then(|| H.to_owned());
            }
            MutationAction::Delete => {
                mutation.expected_sha256 = None;
                mutation.staging_path = None;
                mutation.staging_sha256 = None;
                mutation.backup_sha256 =
                    (checkpoint != MutationCheckpoint::Planned).then(|| H.to_owned());
            }
        }

        let mut intent = crate::fixtures::operation_intent();
        intent.kind = operation;
        let request = crate::OperationRequest::new("operation-1", "request-1", intent).unwrap();
        journal_payload.request_fingerprint = request.request_fingerprint().into();
        let journal = LifecycleJournal::new(journal_payload).unwrap();

        let mut record_payload = crate::fixtures::operation_record().into_payload();
        record_payload.request = request;
        record_payload.state = OperationState::RecoveryRequired;
        record_payload.phase = OperationPhase::RollingBack;
        record_payload.result_fingerprint = None;
        let record = OperationRecord::new(record_payload).unwrap();
        let mut lease = crate::fixtures::operation_lease();
        lease.acquired_at_ms = 1;
        lease.expires_at_ms = 10;
        (journal, record, lease)
    }

    fn boundary_for_publication(
        current: &LifecycleJournal,
    ) -> (FakeBoundary, ValidatedRelativePath, ValidatedRelativePath) {
        let staged_path = current.mutations[0].staging_path.clone().unwrap();
        let destination_path = current.mutations[0].path.clone();
        let roots = vec![
            current.transaction_root.clone(),
            current.staging_root.clone(),
            current.backup_root.clone(),
        ];
        let entries = BTreeMap::from([
            (
                FakeBoundary::entry_key(&current.staging_root, &staged_path),
                ObservedFileState::Regular {
                    sha256: H2.into(),
                    file_id: "staged-file".into(),
                    link_count: 1,
                },
            ),
            (
                FakeBoundary::entry_key(&current.transaction_root, &destination_path),
                ObservedFileState::Regular {
                    sha256: H.into(),
                    file_id: "destination-file-1".into(),
                    link_count: 1,
                },
            ),
        ]);
        (
            FakeBoundary {
                transaction_root: current.transaction_root.clone(),
                roots,
                entries,
            },
            staged_path,
            destination_path,
        )
    }

    #[test]
    fn mutation_transition_changes_only_the_authorized_checkpoint() {
        let (current, next, record, lease, _) = applying_context();

        let mut unchanged_payload = current.clone().into_payload();
        unchanged_payload.journal_sequence = 8;
        unchanged_payload.updated_at_ms += 1;
        let unchanged = LifecycleJournal::new(unchanged_payload).unwrap();
        assert!(ValidatedMutationTransition::new(
            &current,
            &unchanged,
            &record,
            &lease,
            2,
            0,
            MutationSideEffect::Publish,
        )
        .is_err());

        let mut unrelated_payload = next.clone().into_payload();
        unrelated_payload.pinned = true;
        let unrelated = LifecycleJournal::new(unrelated_payload).unwrap();
        assert!(ValidatedMutationTransition::new(
            &current,
            &unrelated,
            &record,
            &lease,
            2,
            0,
            MutationSideEffect::Publish,
        )
        .is_err());

        assert!(ValidatedMutationTransition::new(
            &current,
            &next,
            &record,
            &lease,
            2,
            0,
            MutationSideEffect::Remove,
        )
        .is_err());
    }

    #[test]
    fn mutation_transition_rejects_terminal_or_nonmutating_authority() {
        let current = crate::fixtures::lifecycle_journal();
        let mut next_payload = current.clone().into_payload();
        next_payload.journal_sequence += 1;
        next_payload.updated_at_ms += 1;
        let next = LifecycleJournal::new(next_payload).unwrap();
        let record = crate::fixtures::operation_record();
        let mut lease = crate::fixtures::operation_lease();
        lease.acquired_at_ms = 1;
        lease.expires_at_ms = 10;
        assert!(ValidatedMutationTransition::new(
            &current,
            &next,
            &record,
            &lease,
            2,
            0,
            MutationSideEffect::Publish,
        )
        .is_err());

        let mut verify_record_payload = record.into_payload();
        verify_record_payload.request = crate::OperationRequest::new(
            "operation-1",
            "request-1",
            crate::OperationIntent {
                installation_id: "fixture-installation".into(),
                kind: LifecycleOperationKind::Verify,
                mod_instance_id: Some("fixture-instance".into()),
                provider: None,
                archive_sha256: None,
                file_plan_fingerprint: None,
                profile_id: None,
            },
        )
        .unwrap();
        verify_record_payload.fencing_token = 0;
        let verify_record = OperationRecord::new(verify_record_payload).unwrap();
        assert!(ValidatedMutationTransition::new(
            &current,
            &next,
            &verify_record,
            &lease,
            2,
            0,
            MutationSideEffect::Publish,
        )
        .is_err());
    }

    #[test]
    fn crash_before_journal_cas_is_reconciled_for_create_replace_and_delete() {
        for action in [
            MutationAction::Create,
            MutationAction::Replace,
            MutationAction::Delete,
        ] {
            for outcome in [
                ReconciliationOutcome::EffectApplied,
                ReconciliationOutcome::EffectNotApplied,
            ] {
                let (current, next, record, lease, reconciliation, observation) =
                    reconciliation_context(action, outcome);
                reconciliation
                    .validate_destination_observation(&observation)
                    .unwrap();

                let mut invalid_observation = observation.clone();
                invalid_observation.state = ObservedFileState::NonRegular;
                assert_eq!(
                    reconciliation.validate_destination_observation(&invalid_observation),
                    Err(FilesystemBoundaryError::ObservationChanged)
                );

                let mut entries = BTreeMap::new();
                if observation.state != ObservedFileState::Missing {
                    entries.insert(
                        FakeBoundary::entry_key(
                            &current.transaction_root,
                            &current.mutations[0].path,
                        ),
                        observation.state.clone(),
                    );
                }
                let mut boundary = FakeBoundary {
                    transaction_root: current.transaction_root.clone(),
                    roots: vec![
                        current.transaction_root.clone(),
                        current.staging_root.clone(),
                        current.backup_root.clone(),
                    ],
                    entries,
                };
                let observed = boundary
                    .observe_no_follow(&current.transaction_root, &current.mutations[0].path)
                    .unwrap();
                let mut guard = guard_for(&current);
                let receipt = boundary
                    .reconcile_uncertain_effect(&mut guard, 2, &observed, &reconciliation)
                    .unwrap();
                assert_eq!(receipt.journal_sequence, next.journal_sequence);
                assert_eq!(
                    guard.locked_journal_fingerprint(),
                    next.canonical_fingerprint().unwrap()
                );

                if outcome == ReconciliationOutcome::EffectApplied {
                    let mut rolled_back_payload = next.clone().into_payload();
                    rolled_back_payload.journal_sequence += 1;
                    rolled_back_payload.updated_at_ms += 1;
                    rolled_back_payload.mutations[0].checkpoint = MutationCheckpoint::RolledBack;
                    let rolled_back = LifecycleJournal::new(rolled_back_payload).unwrap();
                    let side_effect = if action == MutationAction::Create {
                        MutationSideEffect::Remove
                    } else {
                        MutationSideEffect::RestoreBackup
                    };
                    ValidatedMutationTransition::new(
                        &next,
                        &rolled_back,
                        &record,
                        &lease,
                        2,
                        0,
                        side_effect,
                    )
                    .unwrap();
                }
            }
        }
    }

    #[test]
    fn crash_before_rollback_cas_is_reconciled_for_every_mutation() {
        for action in [
            MutationAction::Create,
            MutationAction::Replace,
            MutationAction::Delete,
        ] {
            for checkpoint in [
                MutationCheckpoint::Applied,
                MutationCheckpoint::OutputVerified,
            ] {
                let (current, record, lease) = rolling_back_context(action, checkpoint);
                let side_effect = if action == MutationAction::Create {
                    MutationSideEffect::Remove
                } else {
                    MutationSideEffect::RestoreBackup
                };

                let mut rolled_back_payload = current.clone().into_payload();
                rolled_back_payload.journal_sequence += 1;
                rolled_back_payload.updated_at_ms += 1;
                rolled_back_payload.mutations[0].checkpoint = MutationCheckpoint::RolledBack;
                let rolled_back = LifecycleJournal::new(rolled_back_payload).unwrap();

                let reconciliation = ValidatedMutationReconciliation::new(
                    &current,
                    &rolled_back,
                    &record,
                    &lease,
                    2,
                    ReconciliationDecision::new(
                        0,
                        ReconciliationScope::Rollback,
                        ReconciliationOutcome::EffectApplied,
                    ),
                )
                .unwrap();
                let state = if action == MutationAction::Create {
                    ObservedFileState::Missing
                } else {
                    regular(H)
                };
                let mut entries = BTreeMap::new();
                if state != ObservedFileState::Missing {
                    entries.insert(
                        FakeBoundary::entry_key(
                            &current.transaction_root,
                            &current.mutations[0].path,
                        ),
                        state,
                    );
                }
                let mut boundary = FakeBoundary {
                    transaction_root: current.transaction_root.clone(),
                    roots: vec![
                        current.transaction_root.clone(),
                        current.staging_root.clone(),
                        current.backup_root.clone(),
                    ],
                    entries,
                };
                let destination = boundary
                    .observe_no_follow(&current.transaction_root, &current.mutations[0].path)
                    .unwrap();
                let mut guard = guard_for(&current);
                boundary
                    .reconcile_uncertain_effect(&mut guard, 2, &destination, &reconciliation)
                    .unwrap();

                let mut self_loop_payload = current.clone().into_payload();
                self_loop_payload.journal_sequence += 1;
                self_loop_payload.updated_at_ms += 1;
                let self_loop = LifecycleJournal::new(self_loop_payload).unwrap();
                assert!(ValidatedMutationReconciliation::new(
                    &current,
                    &self_loop,
                    &record,
                    &lease,
                    2,
                    ReconciliationDecision::new(
                        0,
                        ReconciliationScope::Rollback,
                        ReconciliationOutcome::EffectNotApplied,
                    ),
                )
                .is_err());
                ValidatedMutationTransition::new(
                    &current,
                    &rolled_back,
                    &record,
                    &lease,
                    2,
                    0,
                    side_effect,
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn every_pre_effect_checkpoint_can_finish_recovery_without_mutation() {
        for (action, checkpoint) in [
            (MutationAction::Create, MutationCheckpoint::Planned),
            (MutationAction::Replace, MutationCheckpoint::Planned),
            (MutationAction::Replace, MutationCheckpoint::Staged),
            (MutationAction::Delete, MutationCheckpoint::Planned),
        ] {
            let (current, record, lease) = rolling_back_context(action, checkpoint);
            let mut next_payload = current.clone().into_payload();
            next_payload.journal_sequence += 1;
            next_payload.updated_at_ms += 1;
            next_payload.mutations[0].checkpoint = MutationCheckpoint::NoEffect;
            let next = LifecycleJournal::new(next_payload).unwrap();
            let reconciliation = ValidatedMutationReconciliation::new(
                &current,
                &next,
                &record,
                &lease,
                2,
                ReconciliationDecision::new(
                    0,
                    ReconciliationScope::PreEffect,
                    ReconciliationOutcome::EffectNotApplied,
                ),
            )
            .unwrap();
            let state = if action == MutationAction::Create {
                ObservedFileState::Missing
            } else {
                regular(H)
            };
            let observation = ObservationSnapshot {
                root_identity: current.transaction_root.clone(),
                path: current.mutations[0].path.clone(),
                path_identity_key: current.mutations[0].path_identity_key.clone(),
                state,
                observation_sequence: 1,
            };
            reconciliation
                .validate_destination_observation(&observation)
                .unwrap();
            assert_eq!(next.mutations[0].checkpoint, MutationCheckpoint::NoEffect);
            assert!(ValidatedMutationReconciliation::new(
                &current,
                &next,
                &record,
                &lease,
                2,
                ReconciliationDecision::new(
                    0,
                    ReconciliationScope::PreEffect,
                    ReconciliationOutcome::EffectApplied,
                ),
            )
            .is_err());
        }
    }

    #[test]
    fn no_effect_is_terminal_recovery_state_only() {
        let mut payload = crate::fixtures::lifecycle_journal().into_payload();
        payload.phase = OperationPhase::Applying;
        payload.manifest_commit_state = ManifestCommitState::NotStarted;
        payload.mutations[0].checkpoint = MutationCheckpoint::NoEffect;
        payload.mutations[0].staging_sha256 = None;
        payload.mutations[0].backup_sha256 = None;
        assert!(LifecycleJournal::new(payload.clone()).is_err());

        payload.phase = OperationPhase::RollingBack;
        LifecycleJournal::new(payload.clone()).unwrap();

        payload.phase = OperationPhase::Complete;
        LifecycleJournal::new(payload).unwrap();
    }

    #[test]
    fn journal_sequence_reserves_deterministic_recovery_headroom() {
        let (template, record, lease) =
            rolling_back_context(MutationAction::Replace, MutationCheckpoint::Planned);
        let mut current_payload = template.into_payload();
        current_payload.journal_sequence = u64::MAX - 3;
        let current = LifecycleJournal::new(current_payload.clone()).unwrap();

        let mut exhausted_payload = current_payload;
        exhausted_payload.journal_sequence = u64::MAX - 2;
        assert!(LifecycleJournal::new(exhausted_payload).is_err());

        let mut no_effect_payload = current.clone().into_payload();
        no_effect_payload.journal_sequence += 1;
        no_effect_payload.updated_at_ms += 1;
        no_effect_payload.mutations[0].checkpoint = MutationCheckpoint::NoEffect;
        let no_effect = LifecycleJournal::new(no_effect_payload).unwrap();
        ValidatedMutationReconciliation::new(
            &current,
            &no_effect,
            &record,
            &lease,
            2,
            ReconciliationDecision::new(
                0,
                ReconciliationScope::PreEffect,
                ReconciliationOutcome::EffectNotApplied,
            ),
        )
        .unwrap();

        let mut cleaning_payload = no_effect.into_payload();
        cleaning_payload.journal_sequence += 1;
        cleaning_payload.updated_at_ms += 1;
        cleaning_payload.phase = OperationPhase::CleaningUp;
        let cleaning = LifecycleJournal::new(cleaning_payload).unwrap();
        let mut complete_payload = cleaning.into_payload();
        complete_payload.journal_sequence += 1;
        complete_payload.updated_at_ms += 1;
        complete_payload.phase = OperationPhase::Complete;
        let complete = LifecycleJournal::new(complete_payload).unwrap();
        assert_eq!(complete.journal_sequence, u64::MAX);
        assert_eq!(
            complete.recovery_disposition(),
            JournalDisposition::Complete
        );

        let (applied_template, applied_record, applied_lease) =
            rolling_back_context(MutationAction::Replace, MutationCheckpoint::Applied);
        let mut applied_payload = applied_template.into_payload();
        applied_payload.journal_sequence = u64::MAX - 3;
        let applied = LifecycleJournal::new(applied_payload).unwrap();
        let mut restored_payload = applied.clone().into_payload();
        restored_payload.journal_sequence += 1;
        restored_payload.updated_at_ms += 1;
        restored_payload.mutations[0].checkpoint = MutationCheckpoint::RolledBack;
        let restored = LifecycleJournal::new(restored_payload).unwrap();
        ValidatedMutationTransition::new(
            &applied,
            &restored,
            &applied_record,
            &applied_lease,
            2,
            0,
            MutationSideEffect::RestoreBackup,
        )
        .unwrap();
        let mut restored_cleaning_payload = restored.into_payload();
        restored_cleaning_payload.journal_sequence += 1;
        restored_cleaning_payload.updated_at_ms += 1;
        restored_cleaning_payload.phase = OperationPhase::CleaningUp;
        let restored_cleaning = LifecycleJournal::new(restored_cleaning_payload).unwrap();
        let mut restored_complete_payload = restored_cleaning.into_payload();
        restored_complete_payload.journal_sequence += 1;
        restored_complete_payload.updated_at_ms += 1;
        restored_complete_payload.phase = OperationPhase::Complete;
        assert_eq!(
            LifecycleJournal::new(restored_complete_payload)
                .unwrap()
                .journal_sequence,
            u64::MAX
        );

        let mut verifying_payload = crate::fixtures::lifecycle_journal().into_payload();
        verifying_payload.phase = OperationPhase::Verifying;
        verifying_payload.manifest_commit_state = ManifestCommitState::NotStarted;
        verifying_payload.journal_sequence = u64::MAX - 5;
        let verifying = LifecycleJournal::new(verifying_payload.clone()).unwrap();
        verifying_payload.journal_sequence += 1;
        assert!(LifecycleJournal::new(verifying_payload).is_err());

        let mut committing_payload = verifying.into_payload();
        committing_payload.journal_sequence += 1;
        committing_payload.updated_at_ms += 1;
        committing_payload.phase = OperationPhase::Committing;
        let committing = LifecycleJournal::new(committing_payload).unwrap();
        let mut temporary_payload = committing.into_payload();
        temporary_payload.journal_sequence += 1;
        temporary_payload.updated_at_ms += 1;
        temporary_payload.manifest_commit_state = ManifestCommitState::TemporaryWritten;
        let temporary = LifecycleJournal::new(temporary_payload).unwrap();
        let mut published_payload = temporary.into_payload();
        published_payload.journal_sequence += 1;
        published_payload.updated_at_ms += 1;
        published_payload.manifest_commit_state = ManifestCommitState::Published;
        let published = LifecycleJournal::new(published_payload).unwrap();
        let mut commit_cleaning_payload = published.into_payload();
        commit_cleaning_payload.journal_sequence += 1;
        commit_cleaning_payload.updated_at_ms += 1;
        commit_cleaning_payload.phase = OperationPhase::CleaningUp;
        let commit_cleaning = LifecycleJournal::new(commit_cleaning_payload).unwrap();
        let mut commit_complete_payload = commit_cleaning.into_payload();
        commit_complete_payload.journal_sequence += 1;
        commit_complete_payload.updated_at_ms += 1;
        commit_complete_payload.phase = OperationPhase::Complete;
        assert_eq!(
            LifecycleJournal::new(commit_complete_payload)
                .unwrap()
                .journal_sequence,
            u64::MAX
        );
    }

    #[test]
    fn expired_writer_is_atomically_rebound_before_reconciliation() {
        let (stalled_journal, _, stalled_record, _, _) = applying_context();
        let mut recovery_record_payload = stalled_record.clone().into_payload();
        recovery_record_payload.state = OperationState::RecoveryRequired;
        recovery_record_payload.phase = OperationPhase::RollingBack;
        recovery_record_payload.updated_at_ms += 1;
        let recovery_record = OperationRecord::new(recovery_record_payload).unwrap();
        let recovery_lease = OperationLease {
            installation_id: stalled_journal.installation_id.clone(),
            operation_id: stalled_journal.operation_id.clone(),
            lease_id: "recovery-lease-2".into(),
            owner_instance_id: "recovery-runtime-2".into(),
            fencing_token: recovery_record.fencing_token,
            acquired_at_ms: 2,
            expires_at_ms: 20,
        };
        let mut recovery_journal_payload = stalled_journal.clone().into_payload();
        recovery_journal_payload.lease_id = recovery_lease.lease_id.clone();
        recovery_journal_payload.recovery_chain_sha256 = stalled_journal
            .next_recovery_chain_sha256(&recovery_lease.lease_id)
            .unwrap();
        recovery_journal_payload.operation_revision = recovery_record.revision;
        recovery_journal_payload.fencing_token = recovery_record.fencing_token;
        recovery_journal_payload.phase = OperationPhase::RollingBack;
        recovery_journal_payload.updated_at_ms += 1;
        recovery_journal_payload.recovery_attempts += 1;
        let recovery_journal = LifecycleJournal::new(recovery_journal_payload).unwrap();
        let rebind = ValidatedRecoveryRebind::new(
            &stalled_journal,
            &stalled_record,
            &recovery_journal,
            &recovery_record,
            &recovery_lease,
            3,
        )
        .unwrap();
        assert_eq!(rebind.stalled_journal(), &stalled_journal);
        assert_eq!(rebind.recovery_journal(), &recovery_journal);
        assert_eq!(
            rebind.stalled_journal().journal_sequence,
            rebind.recovery_journal().journal_sequence
        );

        let mut reconciled_payload = recovery_journal.clone().into_payload();
        reconciled_payload.journal_sequence += 1;
        reconciled_payload.updated_at_ms += 1;
        reconciled_payload.mutations[0].checkpoint = MutationCheckpoint::Applied;
        let reconciled = LifecycleJournal::new(reconciled_payload).unwrap();
        let reconciliation = ValidatedMutationReconciliation::new(
            &recovery_journal,
            &reconciled,
            &recovery_record,
            &recovery_lease,
            3,
            ReconciliationDecision::new(
                0,
                ReconciliationScope::ForwardMutation,
                ReconciliationOutcome::EffectApplied,
            ),
        )
        .unwrap();
        let observation = ObservationSnapshot {
            root_identity: recovery_journal.transaction_root.clone(),
            path: recovery_journal.mutations[0].path.clone(),
            path_identity_key: recovery_journal.mutations[0].path_identity_key.clone(),
            state: regular(H2),
            observation_sequence: 1,
        };
        reconciliation
            .validate_destination_observation(&observation)
            .unwrap();

        let mut unrelated_payload = recovery_journal.clone().into_payload();
        unrelated_payload.pinned = true;
        let unrelated = LifecycleJournal::new(unrelated_payload).unwrap();
        assert!(ValidatedRecoveryRebind::new(
            &stalled_journal,
            &stalled_record,
            &unrelated,
            &recovery_record,
            &recovery_lease,
            3,
        )
        .is_err());

        let mut reused_lease = recovery_lease;
        reused_lease.lease_id = stalled_journal.lease_id.clone();
        assert!(ValidatedRecoveryRebind::new(
            &stalled_journal,
            &stalled_record,
            &recovery_journal,
            &recovery_record,
            &reused_lease,
            3,
        )
        .is_err());
    }

    #[test]
    fn cleaning_up_rebind_preserves_the_terminalization_headroom() {
        for rolled_back in [false, true] {
            let mut stalled_payload = crate::fixtures::lifecycle_journal().into_payload();
            stalled_payload.phase = OperationPhase::CleaningUp;
            stalled_payload.journal_sequence = u64::MAX - 1;
            if rolled_back {
                stalled_payload.operation_revision = u64::MAX - 1;
                stalled_payload.fencing_token = u64::MAX;
                stalled_payload.recovery_attempts = u32::MAX;
                stalled_payload.manifest_commit_state = ManifestCommitState::NotStarted;
                stalled_payload.mutations[0].checkpoint = MutationCheckpoint::NoEffect;
                stalled_payload.mutations[0].staging_sha256 = None;
                stalled_payload.mutations[0].backup_sha256 = None;
            }
            let stalled_journal = LifecycleJournal::new(stalled_payload).unwrap();

            let mut stalled_record_payload = crate::fixtures::operation_record().into_payload();
            stalled_record_payload.state = OperationState::Running;
            stalled_record_payload.phase = OperationPhase::CleaningUp;
            stalled_record_payload.revision = stalled_journal.operation_revision;
            stalled_record_payload.fencing_token = stalled_journal.fencing_token;
            stalled_record_payload.result_fingerprint = None;
            stalled_record_payload.error = None;
            let stalled_record = OperationRecord::new(stalled_record_payload).unwrap();

            let mut recovery_record_payload = stalled_record.clone().into_payload();
            recovery_record_payload.state = OperationState::RecoveryRequired;
            recovery_record_payload.updated_at_ms += 1;
            let recovery_record = OperationRecord::new(recovery_record_payload).unwrap();
            let recovery_lease = OperationLease {
                installation_id: stalled_journal.installation_id.clone(),
                operation_id: stalled_journal.operation_id.clone(),
                lease_id: if rolled_back {
                    "cleanup-rollback-lease"
                } else {
                    "cleanup-commit-lease"
                }
                .into(),
                owner_instance_id: "cleanup-recovery-runtime".into(),
                fencing_token: recovery_record.fencing_token,
                acquired_at_ms: 2,
                expires_at_ms: 20,
            };
            let mut recovery_journal_payload = stalled_journal.clone().into_payload();
            recovery_journal_payload.lease_id = recovery_lease.lease_id.clone();
            recovery_journal_payload.recovery_chain_sha256 = stalled_journal
                .next_recovery_chain_sha256(&recovery_lease.lease_id)
                .unwrap();
            recovery_journal_payload.operation_revision = recovery_record.revision;
            recovery_journal_payload.fencing_token = recovery_record.fencing_token;
            recovery_journal_payload.updated_at_ms += 1;
            recovery_journal_payload.recovery_attempts =
                stalled_journal.recovery_attempts.saturating_add(1);
            let recovery_journal = LifecycleJournal::new(recovery_journal_payload).unwrap();
            ValidatedRecoveryRebind::new(
                &stalled_journal,
                &stalled_record,
                &recovery_journal,
                &recovery_record,
                &recovery_lease,
                3,
            )
            .unwrap();
            assert_eq!(recovery_journal.phase, OperationPhase::CleaningUp);

            let mut terminal_record_payload = recovery_record.clone().into_payload();
            terminal_record_payload.state = OperationState::Recovered;
            terminal_record_payload.phase = OperationPhase::Complete;
            terminal_record_payload.revision += 1;
            terminal_record_payload.updated_at_ms += 1;
            let terminal_record = OperationRecord::new(terminal_record_payload).unwrap();
            let mut complete_payload = recovery_journal.clone().into_payload();
            complete_payload.journal_sequence += 1;
            complete_payload.operation_revision = terminal_record.revision;
            complete_payload.updated_at_ms += 1;
            complete_payload.phase = OperationPhase::Complete;
            let complete = LifecycleJournal::new(complete_payload).unwrap();
            validate_journal_binding(&complete, &terminal_record, &recovery_lease, 3).unwrap();
            assert_eq!(complete.journal_sequence, u64::MAX);
            if rolled_back {
                assert_eq!(terminal_record.revision, u64::MAX);
            }
        }
    }

    #[test]
    fn verified_output_rebind_resumes_commit_without_sequence_write() {
        let mut stalled_payload = crate::fixtures::lifecycle_journal().into_payload();
        stalled_payload.phase = OperationPhase::Verifying;
        stalled_payload.manifest_commit_state = ManifestCommitState::NotStarted;
        stalled_payload.journal_sequence = u64::MAX - 5;
        let stalled_journal = LifecycleJournal::new(stalled_payload).unwrap();

        let mut stalled_record_payload = crate::fixtures::operation_record().into_payload();
        stalled_record_payload.state = OperationState::Running;
        stalled_record_payload.phase = OperationPhase::Verifying;
        stalled_record_payload.result_fingerprint = None;
        stalled_record_payload.error = None;
        let stalled_record = OperationRecord::new(stalled_record_payload).unwrap();
        let mut recovery_record_payload = stalled_record.clone().into_payload();
        recovery_record_payload.state = OperationState::RecoveryRequired;
        recovery_record_payload.phase = OperationPhase::Committing;
        recovery_record_payload.updated_at_ms += 1;
        let recovery_record = OperationRecord::new(recovery_record_payload).unwrap();
        let recovery_lease = OperationLease {
            installation_id: stalled_journal.installation_id.clone(),
            operation_id: stalled_journal.operation_id.clone(),
            lease_id: "verified-commit-recovery-lease".into(),
            owner_instance_id: "verified-commit-recovery-runtime".into(),
            fencing_token: recovery_record.fencing_token,
            acquired_at_ms: 2,
            expires_at_ms: 20,
        };
        let mut recovery_journal_payload = stalled_journal.clone().into_payload();
        recovery_journal_payload.lease_id = recovery_lease.lease_id.clone();
        recovery_journal_payload.recovery_chain_sha256 = stalled_journal
            .next_recovery_chain_sha256(&recovery_lease.lease_id)
            .unwrap();
        recovery_journal_payload.operation_revision = recovery_record.revision;
        recovery_journal_payload.fencing_token = recovery_record.fencing_token;
        recovery_journal_payload.phase = OperationPhase::Committing;
        recovery_journal_payload.updated_at_ms += 1;
        recovery_journal_payload.recovery_attempts += 1;
        let recovery_journal = LifecycleJournal::new(recovery_journal_payload).unwrap();
        ValidatedRecoveryRebind::new(
            &stalled_journal,
            &stalled_record,
            &recovery_journal,
            &recovery_record,
            &recovery_lease,
            3,
        )
        .unwrap();
        assert_eq!(recovery_journal.phase, OperationPhase::Committing);
        assert_eq!(
            stalled_journal.journal_sequence,
            recovery_journal.journal_sequence
        );
    }

    #[test]
    fn recovery_chain_prevents_lease_cycle_aba_at_saturated_counters() {
        let mut initial_payload = crate::fixtures::lifecycle_journal().into_payload();
        initial_payload.phase = OperationPhase::CleaningUp;
        initial_payload.journal_sequence = u64::MAX - 1;
        initial_payload.operation_revision = u64::MAX - 1;
        initial_payload.fencing_token = u64::MAX;
        initial_payload.recovery_attempts = u32::MAX;
        initial_payload.manifest_commit_state = ManifestCommitState::NotStarted;
        initial_payload.mutations[0].checkpoint = MutationCheckpoint::NoEffect;
        initial_payload.mutations[0].staging_sha256 = None;
        initial_payload.mutations[0].backup_sha256 = None;
        let journal_a = LifecycleJournal::new(initial_payload).unwrap();
        let mut initial_record_payload = crate::fixtures::operation_record().into_payload();
        initial_record_payload.state = OperationState::Running;
        initial_record_payload.phase = OperationPhase::CleaningUp;
        initial_record_payload.revision = u64::MAX - 1;
        initial_record_payload.fencing_token = u64::MAX;
        initial_record_payload.result_fingerprint = None;
        initial_record_payload.error = None;
        let record_a = OperationRecord::new(initial_record_payload).unwrap();

        let rebind = |stalled_journal: &LifecycleJournal,
                      stalled_record: &OperationRecord,
                      next_lease_id: &str| {
            let mut recovery_record_payload = stalled_record.clone().into_payload();
            recovery_record_payload.state = OperationState::RecoveryRequired;
            recovery_record_payload.phase = OperationPhase::CleaningUp;
            let recovery_record = OperationRecord::new(recovery_record_payload).unwrap();
            let recovery_lease = OperationLease {
                installation_id: stalled_journal.installation_id.clone(),
                operation_id: stalled_journal.operation_id.clone(),
                lease_id: next_lease_id.into(),
                owner_instance_id: "saturated-recovery-runtime".into(),
                fencing_token: u64::MAX,
                acquired_at_ms: 1_700_000_000_200,
                expires_at_ms: 1_700_000_001_200,
            };
            let mut recovery_journal_payload = stalled_journal.clone().into_payload();
            recovery_journal_payload.lease_id = next_lease_id.into();
            recovery_journal_payload.recovery_chain_sha256 = stalled_journal
                .next_recovery_chain_sha256(next_lease_id)
                .unwrap();
            let recovery_journal = LifecycleJournal::new(recovery_journal_payload).unwrap();
            ValidatedRecoveryRebind::new(
                stalled_journal,
                stalled_record,
                &recovery_journal,
                &recovery_record,
                &recovery_lease,
                1_700_000_000_500,
            )
            .unwrap();
            (recovery_journal, recovery_record, recovery_lease)
        };

        let (journal_b_first, record_b_first, _) = rebind(&journal_a, &record_a, "lease-b");
        let (journal_a_second, record_a_second, _) =
            rebind(&journal_b_first, &record_b_first, "lease-a");
        let (journal_b_second, record_b_second, _) =
            rebind(&journal_a_second, &record_a_second, "lease-b");
        assert_eq!(journal_b_first.lease_id, journal_b_second.lease_id);
        assert_eq!(record_b_first, record_b_second);
        assert_ne!(
            journal_b_first.recovery_chain_sha256,
            journal_b_second.recovery_chain_sha256
        );
        assert_ne!(
            journal_b_first.canonical_fingerprint().unwrap(),
            journal_b_second.canonical_fingerprint().unwrap()
        );
    }

    #[test]
    fn mutation_guard_is_bound_to_the_complete_locked_journal() {
        let (current, _, record, lease, _) = applying_context();
        let mut alternate_payload = current.clone().into_payload();
        alternate_payload.mutations[0].path =
            ValidatedRelativePath::parse("mods/alternate.dat").unwrap();
        alternate_payload.mutations[0].path_identity_key = "mods/alternate.dat".into();
        let alternate = LifecycleJournal::new(alternate_payload).unwrap();
        assert_ne!(
            current.canonical_fingerprint().unwrap(),
            alternate.canonical_fingerprint().unwrap()
        );
        let mut alternate_next_payload = alternate.clone().into_payload();
        alternate_next_payload.journal_sequence += 1;
        alternate_next_payload.updated_at_ms += 1;
        alternate_next_payload.mutations[0].checkpoint = MutationCheckpoint::Applied;
        let alternate_next = LifecycleJournal::new(alternate_next_payload).unwrap();
        let alternate_transition = ValidatedMutationTransition::new(
            &alternate,
            &alternate_next,
            &record,
            &lease,
            2,
            0,
            MutationSideEffect::Publish,
        )
        .unwrap();

        let (mut boundary, staged_path, destination_path) = boundary_for_publication(&current);
        let staged = boundary
            .observe_no_follow(&current.staging_root, &staged_path)
            .unwrap();
        let destination = boundary
            .observe_no_follow(&current.transaction_root, &destination_path)
            .unwrap();
        let original = boundary.entries.clone();
        assert_eq!(
            boundary.publish_verified(
                &mut guard_for(&current),
                2,
                &staged,
                &destination,
                &alternate_transition,
            ),
            Err(FilesystemBoundaryError::StaleJournal)
        );
        assert_eq!(boundary.entries, original);
    }

    #[test]
    fn publication_observations_are_bound_to_the_journal_paths_and_roots() {
        let (current, _, _, _, transition) = applying_context();
        let staged_path = current.mutations[0].staging_path.clone().unwrap();
        let staged = ObservationSnapshot {
            root_identity: current.staging_root.clone(),
            path: staged_path.clone(),
            path_identity_key: staged_path.as_str().into(),
            state: regular(H2),
            observation_sequence: 1,
        };
        let wrong_path = ValidatedRelativePath::parse("mods/wrong.dat").unwrap();
        let destination = ObservationSnapshot {
            root_identity: current.transaction_root.clone(),
            path: wrong_path.clone(),
            path_identity_key: wrong_path.as_str().into(),
            state: regular(H),
            observation_sequence: 1,
        };
        assert_eq!(
            transition.validate_publication(&staged, &destination),
            Err(FilesystemBoundaryError::ObservationChanged)
        );

        let mut wrong_root = staged;
        wrong_root.root_identity = current.transaction_root.clone();
        let destination = ObservationSnapshot {
            root_identity: current.transaction_root.clone(),
            path: current.mutations[0].path.clone(),
            path_identity_key: current.mutations[0].path_identity_key.clone(),
            state: regular(H),
            observation_sequence: 1,
        };
        assert_eq!(
            transition.validate_publication(&wrong_root, &destination),
            Err(FilesystemBoundaryError::RootIdentityChanged)
        );
    }

    #[test]
    fn publication_rejects_same_hash_replacement_and_root_replacement() {
        let (current, _, _, _, transition) = applying_context();
        let (mut boundary, staged_path, destination_path) = boundary_for_publication(&current);
        let staged = boundary
            .observe_no_follow(&current.staging_root, &staged_path)
            .unwrap();
        let destination = boundary
            .observe_no_follow(&current.transaction_root, &destination_path)
            .unwrap();
        boundary.entries.insert(
            FakeBoundary::entry_key(&current.transaction_root, &destination_path),
            ObservedFileState::Regular {
                sha256: H.into(),
                file_id: "destination-file-2".into(),
                link_count: 1,
            },
        );
        assert_eq!(
            boundary.publish_verified(
                &mut guard_for(&current),
                2,
                &staged,
                &destination,
                &transition,
            ),
            Err(FilesystemBoundaryError::ObservationChanged)
        );

        let destination = boundary
            .observe_no_follow(&current.transaction_root, &destination_path)
            .unwrap();
        boundary
            .roots
            .retain(|root| root != &current.transaction_root);
        let mut replaced_root = current.transaction_root.clone();
        replaced_root.file_id = "replaced-root".into();
        boundary.roots.push(replaced_root);
        assert_eq!(
            boundary.publish_verified(
                &mut guard_for(&current),
                2,
                &staged,
                &destination,
                &transition,
            ),
            Err(FilesystemBoundaryError::RootIdentityChanged)
        );
    }

    #[test]
    fn publication_requires_a_live_guard_and_durable_journal_cas() {
        let (current, _, _, _, transition) = applying_context();
        let (mut boundary, staged_path, destination_path) = boundary_for_publication(&current);
        let original = ObservedFileState::Regular {
            sha256: H.into(),
            file_id: "destination-file-1".into(),
            link_count: 1,
        };
        let staged = boundary
            .observe_no_follow(&current.staging_root, &staged_path)
            .unwrap();
        let destination = boundary
            .observe_no_follow(&current.transaction_root, &destination_path)
            .unwrap();

        let mut expired = guard_for(&current);
        expired.expires_at_ms = 2;
        assert_eq!(
            boundary.publish_verified(&mut expired, 2, &staged, &destination, &transition),
            Err(FilesystemBoundaryError::LostLease)
        );
        assert_eq!(
            boundary.entries.get(&FakeBoundary::entry_key(
                &current.transaction_root,
                &destination_path,
            )),
            Some(&original)
        );

        let mut live = guard_for(&current);
        let receipt = boundary
            .publish_verified(&mut live, 2, &staged, &destination, &transition)
            .unwrap();
        assert_eq!(receipt.journal_sequence, 8);
        assert_eq!(live.journal_sequence(), 8);
    }
}
