use crate::{
    valid_id, valid_sha256, ContractDocument, ContractKind, ContractPayload, ProviderRef,
    SchemaError, ValidatedContract, PRODUCT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

pub const OPERATION_INTENT_CANONICALIZATION_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleOperationKind {
    Install,
    Update,
    Uninstall,
    Verify,
    Repair,
    Recover,
    ProfileSwitch,
}

impl LifecycleOperationKind {
    #[must_use]
    pub const fn mutates_files(self) -> bool {
        !matches!(self, Self::Verify)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::Verify => "verify",
            Self::Repair => "repair",
            Self::Recover => "recover",
            Self::ProfileSwitch => "profile_switch",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    RecoveryRequired,
    Recovered,
}

impl OperationState {
    #[must_use]
    pub const fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Recovered
        )
    }

    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running | Self::Cancelled)
                | (
                    Self::Running,
                    Self::Cancelling | Self::Succeeded | Self::Failed | Self::RecoveryRequired
                )
                | (
                    Self::Cancelling,
                    Self::Cancelled | Self::Failed | Self::RecoveryRequired
                )
                | (Self::RecoveryRequired, Self::Running | Self::Recovered)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Preflight,
    Downloading,
    Staging,
    BackingUp,
    Applying,
    Verifying,
    Committing,
    RollingBack,
    CleaningUp,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductErrorCode {
    InvalidRequest,
    UnsupportedSchema,
    InstallationBusy,
    IdempotencyConflict,
    LostOperationLease,
    StaleOperationRevision,
    PathEscapedRoot,
    UnsafeFilesystemEntry,
    ConflictDetected,
    ExternalModification,
    InsufficientSpace,
    DownloadFailed,
    ProviderUnavailable,
    AuthenticationRequired,
    RateLimited,
    VerificationFailed,
    RecoveryRequired,
    RecoveryUnavailable,
    Cancelled,
    Internal,
}

impl ProductErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InstallationBusy => "installation_busy",
            Self::IdempotencyConflict => "idempotency_conflict",
            Self::LostOperationLease => "lost_operation_lease",
            Self::StaleOperationRevision => "stale_operation_revision",
            Self::PathEscapedRoot => "path_escaped_root",
            Self::UnsafeFilesystemEntry => "unsafe_filesystem_entry",
            Self::ConflictDetected => "conflict_detected",
            Self::ExternalModification => "external_modification",
            Self::InsufficientSpace => "insufficient_space",
            Self::DownloadFailed => "download_failed",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::AuthenticationRequired => "authentication_required",
            Self::RateLimited => "rate_limited",
            Self::VerificationFailed => "verification_failed",
            Self::RecoveryRequired => "recovery_required",
            Self::RecoveryUnavailable => "recovery_unavailable",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    NoAction,
    Retry,
    Recover,
    Reauthenticate,
    FreeSpace,
    ResolveConflict,
    SelectExactSource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductErrorPayload {
    pub code: ProductErrorCode,
    pub message_key: String,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub phase: Option<OperationPhase>,
    pub retryable: bool,
    pub recovery_action: RecoveryAction,
    #[serde(default)]
    pub safe_details: BTreeMap<String, String>,
}

pub type ProductError = ValidatedContract<ProductErrorPayload>;

impl crate::schema::private::Sealed for ProductErrorPayload {}
impl ContractPayload for ProductErrorPayload {
    const KIND: ContractKind = ContractKind::ProductError;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        let details_are_safe = self.safe_details.len() <= 16
            && self.safe_details.iter().all(|(key, value)| {
                valid_id(key, 64)
                    && value.len() <= 512
                    && !value.chars().any(char::is_control)
                    && !value.contains(['\\', '/'])
                    && !value.contains("://")
                    && !key.to_ascii_lowercase().contains("token")
                    && !key.to_ascii_lowercase().contains("secret")
            });
        if !valid_id(&self.message_key, 128)
            || self
                .operation_id
                .as_deref()
                .is_some_and(|id| !valid_id(id, 128))
            || !details_are_safe
            || (self.retryable && self.recovery_action == RecoveryAction::NoAction)
        {
            Err(SchemaError::InvalidDocument("product error"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationIntent {
    pub installation_id: String,
    pub kind: LifecycleOperationKind,
    #[serde(default)]
    pub mod_instance_id: Option<String>,
    #[serde(default)]
    pub provider: Option<ProviderRef>,
    #[serde(default)]
    pub archive_sha256: Option<String>,
    #[serde(default)]
    pub file_plan_fingerprint: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
}

impl OperationIntent {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.installation_id, 256)
            || self
                .mod_instance_id
                .as_deref()
                .is_some_and(|id| !valid_id(id, 256))
            || self
                .profile_id
                .as_deref()
                .is_some_and(|id| !valid_id(id, 256))
            || self
                .archive_sha256
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || self
                .file_plan_fingerprint
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || self
                .provider
                .as_ref()
                .is_some_and(|provider| provider.validate().is_err())
        {
            return Err(SchemaError::InvalidDocument("operation intent"));
        }
        if self.kind == LifecycleOperationKind::ProfileSwitch && self.profile_id.is_none() {
            return Err(SchemaError::InvalidDocument("profile switch intent"));
        }
        Ok(())
    }

    /// Canonical byte format v1 is length-prefixed UTF-8 in the fixed field
    /// order below. Optional fields use u32::MAX; hashes are lower-cased.
    #[must_use]
    pub fn canonical_bytes(&self) -> Vec<u8> {
        fn field(output: &mut Vec<u8>, value: Option<&str>) {
            match value {
                Some(value) => {
                    let bytes = value.as_bytes();
                    output.extend_from_slice(
                        &u32::try_from(bytes.len())
                            .unwrap_or(u32::MAX - 1)
                            .to_be_bytes(),
                    );
                    output.extend_from_slice(bytes);
                }
                None => output.extend_from_slice(&u32::MAX.to_be_bytes()),
            }
        }

        let mut output = b"deltamod-operation-intent\0".to_vec();
        output.extend_from_slice(&OPERATION_INTENT_CANONICALIZATION_VERSION.to_be_bytes());
        field(&mut output, Some(&self.installation_id));
        field(&mut output, Some(self.kind.as_str()));
        field(&mut output, self.mod_instance_id.as_deref());
        if let Some(provider) = &self.provider {
            field(&mut output, Some(provider.provider_id().as_str()));
            field(&mut output, Some(provider.item_kind().as_str()));
            field(&mut output, Some(provider.resource_id().as_str()));
            field(
                &mut output,
                provider.scope().map(crate::ProviderScope::as_str),
            );
            field(
                &mut output,
                provider
                    .artifact_id()
                    .map(crate::ProviderResourceId::as_str),
            );
            field(&mut output, Some(provider.artifact_kind().as_str()));
            field(
                &mut output,
                provider.version_id().map(crate::ProviderResourceId::as_str),
            );
        } else {
            for _ in 0..7 {
                field(&mut output, None);
            }
        }
        let archive = self.archive_sha256.as_deref().map(str::to_ascii_lowercase);
        let plan = self
            .file_plan_fingerprint
            .as_deref()
            .map(str::to_ascii_lowercase);
        field(&mut output, archive.as_deref());
        field(&mut output, plan.as_deref());
        field(&mut output, self.profile_id.as_deref());
        output
    }

    #[must_use]
    pub fn fingerprint(&self) -> String {
        Sha256::digest(self.canonical_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRequest {
    operation_id: String,
    idempotency_key: String,
    request_fingerprint: String,
    intent: OperationIntent,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawOperationRequest {
    operation_id: String,
    idempotency_key: String,
    request_fingerprint: String,
    intent: OperationIntent,
}

impl OperationRequest {
    pub fn new(
        operation_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        intent: OperationIntent,
    ) -> Result<Self, SchemaError> {
        let request = Self {
            operation_id: operation_id.into(),
            idempotency_key: idempotency_key.into(),
            request_fingerprint: intent.fingerprint(),
            intent,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), SchemaError> {
        self.intent.validate()?;
        if valid_id(&self.operation_id, 128)
            && valid_id(&self.idempotency_key, 128)
            && valid_sha256(&self.request_fingerprint)
            && self
                .request_fingerprint
                .eq_ignore_ascii_case(&self.intent.fingerprint())
        {
            Ok(())
        } else {
            Err(SchemaError::InvalidDocument("operation request"))
        }
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    #[must_use]
    pub fn request_fingerprint(&self) -> &str {
        &self.request_fingerprint
    }

    #[must_use]
    pub fn intent(&self) -> &OperationIntent {
        &self.intent
    }

    fn same_semantic_request(&self, other: &Self) -> bool {
        self.idempotency_key == other.idempotency_key
            && self
                .request_fingerprint
                .eq_ignore_ascii_case(&other.request_fingerprint)
    }
}

impl<'de> Deserialize<'de> for OperationRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawOperationRequest::deserialize(deserializer)?;
        let request = Self {
            operation_id: raw.operation_id,
            idempotency_key: raw.idempotency_key,
            request_fingerprint: raw.request_fingerprint,
            intent: raw.intent,
        };
        request.validate().map_err(serde::de::Error::custom)?;
        Ok(request)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationProgressPayload {
    pub operation_id: String,
    pub installation_id: String,
    pub kind: LifecycleOperationKind,
    pub state: OperationState,
    pub phase: OperationPhase,
    pub completed: u64,
    #[serde(default)]
    pub total: Option<u64>,
    pub cancellable: bool,
    #[serde(default)]
    pub current_item: Option<String>,
    pub updated_at_ms: u64,
}

pub type OperationProgress = ValidatedContract<OperationProgressPayload>;

impl crate::schema::private::Sealed for OperationProgressPayload {}
impl ContractPayload for OperationProgressPayload {
    const KIND: ContractKind = ContractKind::OperationProgress;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if !valid_id(&self.operation_id, 128)
            || !valid_id(&self.installation_id, 256)
            || self.total.is_some_and(|total| self.completed > total)
            || self.current_item.as_deref().is_some_and(|item| {
                item.is_empty() || item.len() > 512 || item.chars().any(char::is_control)
            })
            || (self.state.terminal() != (self.phase == OperationPhase::Complete))
            || (self.state.terminal() && self.cancellable)
        {
            Err(SchemaError::InvalidDocument("operation progress"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationRecordPayload {
    pub request: OperationRequest,
    pub state: OperationState,
    pub phase: OperationPhase,
    pub revision: u64,
    pub fencing_token: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub result_fingerprint: Option<String>,
    #[serde(default)]
    pub error: Option<ProductError>,
}

pub type OperationRecord = ValidatedContract<OperationRecordPayload>;

impl crate::schema::private::Sealed for OperationRecordPayload {}
impl ContractPayload for OperationRecordPayload {
    const KIND: ContractKind = ContractKind::OperationRecord;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        self.request.validate()?;
        if self.revision == 0
            || (self.revision == u64::MAX && !self.state.terminal())
            || (self.request.intent.kind.mutates_files() != (self.fencing_token > 0))
            || self.updated_at_ms < self.created_at_ms
            || self
                .result_fingerprint
                .as_deref()
                .is_some_and(|hash| !valid_sha256(hash))
            || (self.state.terminal() != (self.phase == OperationPhase::Complete))
            || self.error.as_ref().is_some_and(|error| {
                error.validate().is_err()
                    || error.operation_id.as_deref() != Some(&self.request.operation_id)
            })
            || (self.state == OperationState::Succeeded && self.error.is_some())
        {
            Err(SchemaError::InvalidDocument("operation record"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationLease {
    pub installation_id: String,
    pub operation_id: String,
    pub lease_id: String,
    pub owner_instance_id: String,
    pub fencing_token: u64,
    pub acquired_at_ms: u64,
    pub expires_at_ms: u64,
}

impl OperationLease {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if valid_id(&self.installation_id, 256)
            && valid_id(&self.operation_id, 128)
            && valid_id(&self.lease_id, 128)
            && valid_id(&self.owner_instance_id, 128)
            && self.fencing_token > 0
            && self.expires_at_ms > self.acquired_at_ms
        {
            Ok(())
        } else {
            Err(SchemaError::InvalidDocument("operation lease"))
        }
    }

    #[must_use]
    pub fn active_at(&self, now_ms: u64) -> bool {
        self.acquired_at_ms <= now_ms && now_ms < self.expires_at_ms
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquireOutcome {
    Acquired {
        lease: OperationLease,
        record: OperationRecord,
    },
    Existing(OperationRecord),
    Busy {
        active_operation_id: String,
        expires_at_ms: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompareAndSwapOutcome {
    Stored(Box<OperationRecord>),
    StaleRevision { current_revision: u64 },
    LostLease,
}

/// Durable implementations must atomically deduplicate idempotency keys,
/// acquire one lease per installation, allocate monotonically increasing
/// fencing tokens between operations, and compare-and-swap records. A recovery
/// lease for the same operation retains its fencing token and rotates its lease
/// ID plus the bound journal fingerprint; an expired writer may never publish
/// through either an older token or an older lease/fingerprint binding.
pub trait OperationStore {
    type Error;

    fn acquire_or_replay(
        &mut self,
        request: &OperationRequest,
        owner_instance_id: &str,
        lease_id: &str,
        now_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<AcquireOutcome, Self::Error>;

    fn compare_and_swap(
        &mut self,
        lease: &OperationLease,
        expected_revision: u64,
        next: &OperationRecord,
    ) -> Result<CompareAndSwapOutcome, Self::Error>;

    fn renew(
        &mut self,
        lease: &OperationLease,
        now_ms: u64,
        lease_ttl_ms: u64,
    ) -> Result<Option<OperationLease>, Self::Error>;

    fn release(&mut self, lease: &OperationLease) -> Result<bool, Self::Error>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BeginOutcome {
    Started(OperationRecord),
    Existing(OperationRecord),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum OperationRegistryError {
    #[error("invalid operation identity")]
    InvalidIdentity,
    #[error("installation_busy")]
    InstallationBusy { active_operation_id: String },
    #[error("idempotency_conflict")]
    IdempotencyConflict,
    #[error("lost_operation_lease")]
    LostLease,
    #[error("stale_operation_revision")]
    StaleRevision,
    #[error("invalid_operation_transition")]
    InvalidTransition,
    #[error("unknown_operation")]
    UnknownOperation,
    #[error("invalid_persisted_registry")]
    InvalidPersistedRegistry,
}

#[derive(Debug, Default)]
pub struct OperationRegistry {
    operations: BTreeMap<String, OperationRecord>,
    idempotency: BTreeMap<String, String>,
    active_installations: BTreeMap<String, String>,
    next_fencing_tokens: BTreeMap<String, u64>,
}

impl OperationRegistry {
    pub fn restore(records: Vec<OperationRecord>) -> Result<Self, OperationRegistryError> {
        let mut registry = Self::default();
        for record in records {
            record
                .validate()
                .map_err(|_| OperationRegistryError::InvalidPersistedRegistry)?;
            let operation_id = record.request.operation_id.clone();
            let idempotency_key = record.request.idempotency_key.clone();
            let installation_id = record.request.intent.installation_id.clone();
            registry
                .next_fencing_tokens
                .entry(installation_id.clone())
                .and_modify(|token| *token = (*token).max(record.fencing_token))
                .or_insert(record.fencing_token);
            if registry.operations.contains_key(&operation_id)
                || registry.idempotency.contains_key(&idempotency_key)
            {
                return Err(OperationRegistryError::InvalidPersistedRegistry);
            }
            if record.request.intent.kind.mutates_files()
                && !record.state.terminal()
                && registry
                    .active_installations
                    .insert(installation_id, operation_id.clone())
                    .is_some()
            {
                return Err(OperationRegistryError::InvalidPersistedRegistry);
            }
            registry
                .idempotency
                .insert(idempotency_key, operation_id.clone());
            registry.operations.insert(operation_id, record);
        }
        Ok(registry)
    }

    pub fn begin(
        &mut self,
        request: OperationRequest,
        now_ms: u64,
    ) -> Result<BeginOutcome, OperationRegistryError> {
        request
            .validate()
            .map_err(|_| OperationRegistryError::InvalidIdentity)?;
        if let Some(existing) = self.operations.get(&request.operation_id) {
            return if existing.request.same_semantic_request(&request) {
                Ok(BeginOutcome::Existing(existing.clone()))
            } else {
                Err(OperationRegistryError::IdempotencyConflict)
            };
        }
        if let Some(operation_id) = self.idempotency.get(&request.idempotency_key) {
            let existing = self
                .operations
                .get(operation_id)
                .ok_or(OperationRegistryError::InvalidPersistedRegistry)?;
            return if existing.request.same_semantic_request(&request) {
                Ok(BeginOutcome::Existing(existing.clone()))
            } else {
                Err(OperationRegistryError::IdempotencyConflict)
            };
        }
        let installation_id = request.intent.installation_id.clone();
        if request.intent.kind.mutates_files() {
            if let Some(active_operation_id) = self.active_installations.get(&installation_id) {
                return Err(OperationRegistryError::InstallationBusy {
                    active_operation_id: active_operation_id.clone(),
                });
            }
        }
        let fencing_token = if request.intent.kind.mutates_files() {
            let next = self
                .next_fencing_tokens
                .get(&installation_id)
                .copied()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(OperationRegistryError::InvalidIdentity)?;
            self.next_fencing_tokens
                .insert(installation_id.clone(), next);
            next
        } else {
            0
        };
        let record = OperationRecord::new(OperationRecordPayload {
            request: request.clone(),
            state: OperationState::Running,
            phase: OperationPhase::Preflight,
            revision: 1,
            fencing_token,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            result_fingerprint: None,
            error: None,
        })
        .map_err(|_| OperationRegistryError::InvalidIdentity)?;
        self.idempotency.insert(
            request.idempotency_key.clone(),
            request.operation_id.clone(),
        );
        if request.intent.kind.mutates_files() {
            self.active_installations
                .insert(installation_id, request.operation_id.clone());
        }
        self.operations.insert(request.operation_id, record.clone());
        Ok(BeginOutcome::Started(record))
    }

    pub fn transition(
        &mut self,
        operation_id: &str,
        expected_fencing_token: u64,
        expected_revision: u64,
        state: OperationState,
        phase: OperationPhase,
        updated_at_ms: u64,
    ) -> Result<OperationRecord, OperationRegistryError> {
        let record = self
            .operations
            .get_mut(operation_id)
            .ok_or(OperationRegistryError::UnknownOperation)?;
        if record.fencing_token != expected_fencing_token {
            return Err(OperationRegistryError::LostLease);
        }
        if record.revision != expected_revision {
            return Err(OperationRegistryError::StaleRevision);
        }
        if !record.state.can_transition_to(state)
            || updated_at_ms < record.updated_at_ms
            || state.terminal() != (phase == OperationPhase::Complete)
        {
            return Err(OperationRegistryError::InvalidTransition);
        }
        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or(OperationRegistryError::InvalidTransition)?;
        record
            .try_update(|payload| {
                payload.state = state;
                payload.phase = phase;
                payload.updated_at_ms = updated_at_ms;
                payload.revision = next_revision;
            })
            .map_err(|_| OperationRegistryError::InvalidTransition)?;
        if state.terminal()
            && self
                .active_installations
                .get(&record.request.intent.installation_id)
                .is_some_and(|active| active == operation_id)
        {
            self.active_installations
                .remove(&record.request.intent.installation_id);
        }
        Ok(record.clone())
    }

    #[must_use]
    pub fn records(&self) -> Vec<OperationRecord> {
        self.operations.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(installation: &str) -> OperationIntent {
        OperationIntent {
            installation_id: installation.into(),
            kind: LifecycleOperationKind::Install,
            mod_instance_id: Some("instance-a".into()),
            provider: None,
            archive_sha256: None,
            file_plan_fingerprint: None,
            profile_id: None,
        }
    }

    fn request(operation: &str, key: &str, installation: &str) -> OperationRequest {
        OperationRequest::new(operation, key, intent(installation)).unwrap()
    }

    #[test]
    fn intent_fingerprint_has_a_stable_v1_vector() {
        assert_eq!(
            intent("game-1").fingerprint(),
            "999e835cfabd2abf32972769ddce170653d5eb12569e8e11d31f212e66bcccfb"
        );
    }

    #[test]
    fn full_intent_fingerprint_has_a_stable_v1_vector() {
        let intent = OperationIntent {
            installation_id: "game-main".into(),
            kind: LifecycleOperationKind::ProfileSwitch,
            mod_instance_id: Some("instance-main".into()),
            provider: Some(
                ProviderRef::new(
                    crate::ProviderId::parse("nexus").unwrap(),
                    crate::ProviderItemKind::Mod,
                    crate::ProviderResourceId::parse("42").unwrap(),
                    Some(crate::ProviderScope::parse("deltarune").unwrap()),
                    Some(crate::ProviderResourceId::parse("99").unwrap()),
                    crate::ProviderArtifactKind::File,
                    Some(crate::ProviderResourceId::parse("7").unwrap()),
                    Some("https://www.nexusmods.com/deltarune/mods/42".into()),
                )
                .unwrap(),
            ),
            archive_sha256: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            ),
            file_plan_fingerprint: Some(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            ),
            profile_id: Some("profile-main".into()),
        };
        assert_eq!(
            intent.fingerprint(),
            "490e2a23ae351fc55af39f6e771f345861194e03dc6563b938e5d952cb9499cb"
        );
    }

    #[test]
    fn idempotency_key_returns_original_even_with_new_delivery_operation_id() {
        let mut registry = OperationRegistry::default();
        registry
            .begin(request("op-1", "click-1", "game-1"), 1)
            .unwrap();
        let BeginOutcome::Existing(existing) = registry
            .begin(request("op-2", "click-1", "game-1"), 2)
            .unwrap()
        else {
            panic!("expected existing operation");
        };
        assert_eq!(existing.request.operation_id, "op-1");
    }

    #[test]
    fn changed_semantics_under_same_key_conflict() {
        let mut registry = OperationRegistry::default();
        registry
            .begin(request("op-1", "click-1", "game-1"), 1)
            .unwrap();
        let mut changed_intent = intent("game-1");
        changed_intent.mod_instance_id = Some("instance-b".into());
        let changed = OperationRequest::new("op-2", "click-1", changed_intent).unwrap();
        assert_eq!(
            registry.begin(changed, 2),
            Err(OperationRegistryError::IdempotencyConflict)
        );
    }

    #[test]
    fn replay_equivalence_uses_the_canonical_intent_not_raw_formatting() {
        let original_intent = crate::fixtures::operation_intent();
        let mut equivalent_intent = original_intent.clone();
        equivalent_intent.archive_sha256 = equivalent_intent
            .archive_sha256
            .map(|hash| hash.to_ascii_uppercase());
        equivalent_intent.file_plan_fingerprint = equivalent_intent
            .file_plan_fingerprint
            .map(|hash| hash.to_ascii_uppercase());
        equivalent_intent.provider = Some(
            equivalent_intent
                .provider
                .take()
                .unwrap()
                .with_canonical_url(Some(
                    "https://gamebanana.com/mods/1234?view=alternate".into(),
                ))
                .unwrap(),
        );

        let original = OperationRequest::new("op-1", "key-1", original_intent).unwrap();
        let equivalent = OperationRequest::new("op-2", "key-1", equivalent_intent).unwrap();
        assert_eq!(
            original.request_fingerprint(),
            equivalent.request_fingerprint()
        );

        let mut registry = OperationRegistry::default();
        registry.begin(original, 1).unwrap();
        assert!(matches!(
            registry.begin(equivalent, 2),
            Ok(BeginOutcome::Existing(_))
        ));
    }

    #[test]
    fn stale_fencing_token_and_revision_cannot_transition() {
        let mut registry = OperationRegistry::default();
        let BeginOutcome::Started(record) = registry
            .begin(request("op-1", "key-1", "game-1"), 1)
            .unwrap()
        else {
            panic!("expected start");
        };
        assert_eq!(
            registry.transition(
                "op-1",
                record.fencing_token + 1,
                record.revision,
                OperationState::Cancelling,
                OperationPhase::Applying,
                2,
            ),
            Err(OperationRegistryError::LostLease)
        );
        assert_eq!(
            registry.transition(
                "op-1",
                record.fencing_token,
                record.revision + 1,
                OperationState::Cancelling,
                OperationPhase::Applying,
                2,
            ),
            Err(OperationRegistryError::StaleRevision)
        );
    }

    #[test]
    fn exhausted_revision_is_rejected_without_overflow() {
        let payload = OperationRecordPayload {
            request: request("op-max", "key-max", "game-1"),
            state: OperationState::Running,
            phase: OperationPhase::Preflight,
            revision: u64::MAX,
            fencing_token: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
            result_fingerprint: None,
            error: None,
        };
        assert!(OperationRecord::new(payload.clone()).is_err());

        let mut terminal = payload;
        terminal.state = OperationState::Recovered;
        terminal.phase = OperationPhase::Complete;
        assert_eq!(OperationRecord::new(terminal).unwrap().revision, u64::MAX);
    }

    #[test]
    fn maximum_fencing_token_is_recoverable_but_not_reissued() {
        let mut prior_payload = OperationRecordPayload {
            request: request("op-prior", "key-prior", "game-1"),
            state: OperationState::Succeeded,
            phase: OperationPhase::Complete,
            revision: 2,
            fencing_token: u64::MAX - 1,
            created_at_ms: 1,
            updated_at_ms: 2,
            result_fingerprint: None,
            error: None,
        };
        let prior = OperationRecord::new(prior_payload.clone()).unwrap();
        let mut registry = OperationRegistry::restore(vec![prior]).unwrap();
        let BeginOutcome::Started(active) = registry
            .begin(request("op-max", "key-max", "game-1"), 3)
            .unwrap()
        else {
            panic!("expected final fencing token to start");
        };
        assert_eq!(active.fencing_token, u64::MAX);
        registry
            .transition(
                "op-max",
                active.fencing_token,
                active.revision,
                OperationState::Succeeded,
                OperationPhase::Complete,
                4,
            )
            .unwrap();
        assert_eq!(
            registry.begin(request("op-overflow", "key-overflow", "game-1"), 5),
            Err(OperationRegistryError::InvalidIdentity)
        );

        prior_payload.fencing_token = u64::MAX;
        assert!(OperationRecord::new(prior_payload).is_ok());
    }

    #[test]
    fn recovery_required_keeps_installation_lock_until_terminal_complete() {
        let mut registry = OperationRegistry::default();
        let BeginOutcome::Started(record) = registry
            .begin(request("op-1", "key-1", "game-1"), 1)
            .unwrap()
        else {
            panic!("expected start");
        };
        let record = registry
            .transition(
                "op-1",
                record.fencing_token,
                record.revision,
                OperationState::RecoveryRequired,
                OperationPhase::RollingBack,
                2,
            )
            .unwrap();
        assert!(matches!(
            registry.begin(request("op-2", "key-2", "game-1"), 3),
            Err(OperationRegistryError::InstallationBusy { .. })
        ));
        registry
            .transition(
                "op-1",
                record.fencing_token,
                record.revision,
                OperationState::Recovered,
                OperationPhase::Complete,
                4,
            )
            .unwrap();
        registry
            .begin(request("op-2", "key-2", "game-1"), 5)
            .unwrap();
    }

    #[test]
    fn every_terminal_state_is_rejected_outside_complete_phase() {
        let terminals = [
            OperationState::Succeeded,
            OperationState::Failed,
            OperationState::Cancelled,
            OperationState::Recovered,
        ];
        let phases = [
            OperationPhase::Preflight,
            OperationPhase::Downloading,
            OperationPhase::Staging,
            OperationPhase::BackingUp,
            OperationPhase::Applying,
            OperationPhase::Verifying,
            OperationPhase::Committing,
            OperationPhase::RollingBack,
            OperationPhase::CleaningUp,
        ];
        for state in terminals {
            for phase in phases {
                let payload = OperationProgressPayload {
                    operation_id: "op".into(),
                    installation_id: "game".into(),
                    kind: LifecycleOperationKind::Install,
                    state,
                    phase,
                    completed: 0,
                    total: None,
                    cancellable: false,
                    current_item: None,
                    updated_at_ms: 1,
                };
                assert!(
                    OperationProgress::new(payload).is_err(),
                    "{state:?} {phase:?}"
                );
            }
        }
    }

    #[test]
    fn direct_serde_cannot_construct_an_invalid_record() {
        let record = OperationRecord::new(OperationRecordPayload {
            request: request("op-1", "key-1", "game-1"),
            state: OperationState::Running,
            phase: OperationPhase::Preflight,
            revision: 1,
            fencing_token: 1,
            created_at_ms: 1,
            updated_at_ms: 1,
            result_fingerprint: None,
            error: None,
        })
        .unwrap();
        let invalid = serde_json::to_string(&record)
            .unwrap()
            .replace("\"revision\":1", "\"revision\":0");
        assert!(serde_json::from_str::<OperationRecord>(&invalid).is_err());
        let request = serde_json::to_string(&request("op-2", "key-2", "game-1")).unwrap();
        let invalid_request = request.replace(
            "999e835cfabd2abf32972769ddce170653d5eb12569e8e11d31f212e66bcccfb",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        assert!(serde_json::from_str::<OperationRequest>(&invalid_request).is_err());
    }
}
