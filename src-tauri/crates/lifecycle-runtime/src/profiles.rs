use crate::{hex_digest, valid_id, valid_sha256, InstallationManifest, ValidatedInstallPlan};
use deltamod_product_contracts::{
    validate_installation_manifest, LifecycleOperationKind, ProviderArtifactKind, ProviderItemKind,
    ProviderRef, ValidatedRelativePath,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const PROFILE_SCHEMA_VERSION: u32 = 1;
pub const PROFILE_PLAN_CANONICALIZATION_VERSION: u32 = 1;
const MAX_PROFILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PROFILE_MODS: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileDocumentKind {
    ProfileDefinition,
    ProfileLockfile,
}

impl ProfileDocumentKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProfileDefinition => "profile_definition",
            Self::ProfileLockfile => "profile_lockfile",
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProfileError {
    #[error("profile JSON is empty, too large, or malformed")]
    Malformed,
    #[error("wrong profile document kind")]
    WrongKind,
    #[error("unsupported profile schema {found}; supported schema is {supported}")]
    UnsupportedSchema { found: u64, supported: u32 },
    #[error("profile JSON is not in canonical form")]
    NonCanonical,
    #[error("invalid profile field: {0}")]
    Invalid(&'static str),
    #[error("duplicate profile mod instance: {0}")]
    DuplicateInstance(String),
    #[error("profile definition and lockfile differ")]
    DefinitionMismatch,
    #[error("profile installation does not match the current manifest")]
    InstallationMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileModDefinition {
    pub order: u32,
    pub instance_id: String,
    pub mod_id: String,
    pub display_name: String,
    pub provider: ProviderRef,
    #[serde(default)]
    pub configuration_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileDefinition {
    pub document_kind: ProfileDocumentKind,
    pub schema_version: u32,
    pub profile_id: String,
    pub game_id: String,
    pub installation_id: String,
    pub mods: Vec<ProfileModDefinition>,
}

impl ProfileDefinition {
    pub fn new(
        profile_id: impl Into<String>,
        game_id: impl Into<String>,
        installation_id: impl Into<String>,
        mods: Vec<ProfileModDefinition>,
    ) -> Result<Self, ProfileError> {
        let value = Self {
            document_kind: ProfileDocumentKind::ProfileDefinition,
            schema_version: PROFILE_SCHEMA_VERSION,
            profile_id: profile_id.into(),
            game_id: game_id.into(),
            installation_id: installation_id.into(),
            mods,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        validate_header(
            self.document_kind,
            ProfileDocumentKind::ProfileDefinition,
            self.schema_version,
        )?;
        validate_profile_ids(&self.profile_id, &self.game_id, &self.installation_id)?;
        validate_ordered_instances(
            self.mods
                .iter()
                .map(|item| (item.order, item.instance_id.as_str())),
            self.mods.len(),
        )?;
        for item in &self.mods {
            validate_mod_fields(
                &item.instance_id,
                &item.mod_id,
                &item.display_name,
                &item.provider,
                item.configuration_fingerprint.as_deref(),
            )?;
        }
        Ok(())
    }

    pub fn to_canonical_json(&self) -> Result<String, ProfileError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| ProfileError::Malformed)
    }

    pub fn from_canonical_json(input: &str) -> Result<Self, ProfileError> {
        import_canonical(
            input,
            ProfileDocumentKind::ProfileDefinition,
            Self::validate,
        )
    }

    pub fn fingerprint(&self) -> Result<String, ProfileError> {
        fingerprint(
            b"deltamod:profile-definition:v1\0",
            &self.to_canonical_json()?,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LockedProfileMod {
    pub order: u32,
    pub instance_id: String,
    pub mod_id: String,
    pub display_name: String,
    #[serde(default)]
    pub version: Option<String>,
    pub provider: ProviderRef,
    pub archive_sha256: String,
    pub file_plan_fingerprint: String,
    #[serde(default)]
    pub configuration_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileLockfile {
    pub document_kind: ProfileDocumentKind,
    pub schema_version: u32,
    pub profile_id: String,
    pub game_id: String,
    pub installation_id: String,
    pub definition_fingerprint: String,
    pub mods: Vec<LockedProfileMod>,
}

impl ProfileLockfile {
    pub fn new(
        definition: &ProfileDefinition,
        mods: Vec<LockedProfileMod>,
    ) -> Result<Self, ProfileError> {
        definition.validate()?;
        let value = Self {
            document_kind: ProfileDocumentKind::ProfileLockfile,
            schema_version: PROFILE_SCHEMA_VERSION,
            profile_id: definition.profile_id.clone(),
            game_id: definition.game_id.clone(),
            installation_id: definition.installation_id.clone(),
            definition_fingerprint: definition.fingerprint()?,
            mods,
        };
        value.validate()?;
        value.validate_definition(definition)?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        validate_header(
            self.document_kind,
            ProfileDocumentKind::ProfileLockfile,
            self.schema_version,
        )?;
        validate_profile_ids(&self.profile_id, &self.game_id, &self.installation_id)?;
        validate_hash(&self.definition_fingerprint, "definition fingerprint")?;
        validate_ordered_instances(
            self.mods
                .iter()
                .map(|item| (item.order, item.instance_id.as_str())),
            self.mods.len(),
        )?;
        for item in &self.mods {
            validate_mod_fields(
                &item.instance_id,
                &item.mod_id,
                &item.display_name,
                &item.provider,
                item.configuration_fingerprint.as_deref(),
            )?;
            validate_hash(&item.archive_sha256, "archive SHA-256")?;
            validate_hash(&item.file_plan_fingerprint, "file-plan fingerprint")?;
            if item.version.as_deref().is_some_and(|version| {
                version.is_empty() || version.len() > 256 || version.chars().any(char::is_control)
            }) {
                return Err(ProfileError::Invalid("version"));
            }
            if item.provider.item_kind() != ProviderItemKind::LocalArchive
                && (item.provider.canonical_url().is_none()
                    || item.provider.artifact_kind() == ProviderArtifactKind::Unknown
                    || (item.provider.artifact_id().is_none()
                        && item.provider.version_id().is_none()))
            {
                return Err(ProfileError::Invalid("exact provider source pin"));
            }
        }
        Ok(())
    }

    pub fn validate_definition(&self, definition: &ProfileDefinition) -> Result<(), ProfileError> {
        definition.validate()?;
        if self.profile_id != definition.profile_id
            || self.game_id != definition.game_id
            || self.installation_id != definition.installation_id
            || self.definition_fingerprint != definition.fingerprint()?
            || self.mods.len() != definition.mods.len()
        {
            return Err(ProfileError::DefinitionMismatch);
        }
        for (locked, desired) in self.mods.iter().zip(&definition.mods) {
            if locked.order != desired.order
                || locked.instance_id != desired.instance_id
                || locked.mod_id != desired.mod_id
                || locked.display_name != desired.display_name
                || locked.configuration_fingerprint != desired.configuration_fingerprint
                || !provider_refines(&desired.provider, &locked.provider)
            {
                return Err(ProfileError::DefinitionMismatch);
            }
        }
        Ok(())
    }

    pub fn to_canonical_json(&self) -> Result<String, ProfileError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| ProfileError::Malformed)
    }

    pub fn from_canonical_json(input: &str) -> Result<Self, ProfileError> {
        import_canonical(input, ProfileDocumentKind::ProfileLockfile, Self::validate)
    }

    pub fn fingerprint(&self) -> Result<String, ProfileError> {
        fingerprint(
            b"deltamod:profile-lockfile:v1\0",
            &self.to_canonical_json()?,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileUpdateReason {
    ModIdentity,
    DisplayName,
    Version,
    Provider,
    ArchiveSha256,
    FilePlanFingerprint,
    ConfigurationFingerprint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRemoval {
    instance_id: String,
    mod_id: String,
}

impl ProfileRemoval {
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    #[must_use]
    pub fn mod_id(&self) -> &str {
        &self.mod_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileChange {
    target: LockedProfileMod,
    reasons: Vec<ProfileUpdateReason>,
}

impl ProfileChange {
    #[must_use]
    pub fn target(&self) -> &LockedProfileMod {
        &self.target
    }

    #[must_use]
    pub fn reasons(&self) -> &[ProfileUpdateReason] {
        &self.reasons
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileOrderChange {
    instance_id: String,
    previous_order: u32,
    target_order: u32,
}

impl ProfileOrderChange {
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    #[must_use]
    pub const fn previous_order(&self) -> u32 {
        self.previous_order
    }

    #[must_use]
    pub const fn target_order(&self) -> u32 {
        self.target_order
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedProfileOperation {
    sequence: u32,
    kind: LifecycleOperationKind,
    instance_id: String,
    operation_id: String,
    idempotency_key: String,
}

impl PlannedProfileOperation {
    #[must_use]
    pub const fn sequence(&self) -> u32 {
        self.sequence
    }

    #[must_use]
    pub const fn kind(&self) -> LifecycleOperationKind {
        self.kind
    }

    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLeaseContract {
    OneInstallationLease,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileOperationEngine {
    ExistingLifecycleOperations,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileCommitCondition {
    AllLifecycleOperationsVerified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileFailurePolicy {
    RollBackAndKeepPreviousActive,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileCommitBoundary {
    previous_profile_id: Option<String>,
    target_profile_id: String,
    condition: ProfileCommitCondition,
    on_failure: ProfileFailurePolicy,
}

impl ProfileCommitBoundary {
    #[must_use]
    pub fn previous_profile_id(&self) -> Option<&str> {
        self.previous_profile_id.as_deref()
    }

    #[must_use]
    pub fn target_profile_id(&self) -> &str {
        &self.target_profile_id
    }

    #[must_use]
    pub const fn condition(&self) -> ProfileCommitCondition {
        self.condition
    }

    #[must_use]
    pub const fn on_failure(&self) -> ProfileFailurePolicy {
        self.on_failure
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileSwitchPlan {
    installation_id: String,
    target_profile_id: String,
    source_installation_id: Option<String>,
    source_manifest_generation: Option<u64>,
    source_manifest_fingerprint: Option<String>,
    previous_lock_fingerprint: Option<String>,
    target_lock_fingerprint: String,
    removals: Vec<ProfileRemoval>,
    installs: Vec<ProfileChange>,
    updates: Vec<ProfileChange>,
    retained: Vec<LockedProfileMod>,
    order_changes: Vec<ProfileOrderChange>,
    operations: Vec<PlannedProfileOperation>,
    lease_contract: ProfileLeaseContract,
    operation_engine: ProfileOperationEngine,
    commit_boundary: ProfileCommitBoundary,
    fingerprint: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfilePlanFingerprintMaterial<'a> {
    version: u32,
    installation_id: &'a str,
    target_profile_id: &'a str,
    source_installation_id: Option<&'a str>,
    source_generation: Option<u64>,
    current_manifest: Option<&'a str>,
    previous_profile_id: Option<&'a str>,
    previous_lock: Option<&'a str>,
    target_lock: &'a str,
    removals: &'a [ProfileRemoval],
    installs: &'a [ProfileChange],
    updates: &'a [ProfileChange],
    retained: &'a [LockedProfileMod],
    order_changes: &'a [ProfileOrderChange],
    lease_contract: ProfileLeaseContract,
    operation_engine: ProfileOperationEngine,
    commit_condition: ProfileCommitCondition,
    failure_policy: ProfileFailurePolicy,
}

impl ProfileSwitchPlan {
    pub fn build(
        current: Option<&InstallationManifest>,
        previous_active: Option<&ProfileLockfile>,
        target: &ProfileLockfile,
    ) -> Result<Self, ProfileError> {
        target.validate()?;
        if let Some(manifest) = current {
            validate_installation_manifest(&manifest.records, &manifest.ledger)
                .map_err(|_| ProfileError::Invalid("current manifest"))?;
            if manifest.installation_id() != target.installation_id {
                return Err(ProfileError::InstallationMismatch);
            }
        }
        if let Some(active) = previous_active {
            active.validate()?;
            if active.installation_id != target.installation_id || active.game_id != target.game_id
            {
                return Err(ProfileError::InstallationMismatch);
            }
        }

        let current_records: BTreeMap<_, _> = current
            .into_iter()
            .flat_map(|manifest| &manifest.records)
            .map(|record| (record.instance_id.as_str(), record))
            .collect();
        let target_ids: BTreeSet<_> = target
            .mods
            .iter()
            .map(|item| item.instance_id.as_str())
            .collect();
        let mut removals: Vec<_> = current_records
            .values()
            .filter(|record| !target_ids.contains(record.instance_id.as_str()))
            .map(|record| ProfileRemoval {
                instance_id: record.instance_id.clone(),
                mod_id: record.mod_id.clone(),
            })
            .collect();
        removals.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));

        let current_order: BTreeMap<_, _> = previous_active.map_or_else(
            || {
                current
                    .into_iter()
                    .flat_map(|manifest| &manifest.records)
                    .enumerate()
                    .map(|(index, record)| (record.instance_id.as_str(), index as u32))
                    .collect()
            },
            |active| {
                active
                    .mods
                    .iter()
                    .map(|item| (item.instance_id.as_str(), item.order))
                    .collect()
            },
        );
        let active_mods: BTreeMap<_, _> = previous_active
            .into_iter()
            .flat_map(|active| &active.mods)
            .map(|item| (item.instance_id.as_str(), item))
            .collect();
        let mut installs = Vec::new();
        let mut updates = Vec::new();
        let mut retained = Vec::new();
        let mut order_changes = Vec::new();
        for locked in &target.mods {
            let Some(installed) = current_records.get(locked.instance_id.as_str()) else {
                installs.push(ProfileChange {
                    target: locked.clone(),
                    reasons: Vec::new(),
                });
                continue;
            };
            let mut reasons = Vec::new();
            if installed.mod_id != locked.mod_id {
                reasons.push(ProfileUpdateReason::ModIdentity);
            }
            if installed.display_name != locked.display_name {
                reasons.push(ProfileUpdateReason::DisplayName);
            }
            if installed.version != locked.version {
                reasons.push(ProfileUpdateReason::Version);
            }
            if installed.provider != locked.provider {
                reasons.push(ProfileUpdateReason::Provider);
            }
            if installed.archive_sha256.as_deref() != Some(locked.archive_sha256.as_str()) {
                reasons.push(ProfileUpdateReason::ArchiveSha256);
            }
            if installed.file_plan_fingerprint != locked.file_plan_fingerprint {
                reasons.push(ProfileUpdateReason::FilePlanFingerprint);
            }
            let active_configuration = active_mods
                .get(locked.instance_id.as_str())
                .and_then(|item| item.configuration_fingerprint.as_deref());
            if active_configuration != locked.configuration_fingerprint.as_deref() {
                reasons.push(ProfileUpdateReason::ConfigurationFingerprint);
            }
            if reasons.is_empty() {
                retained.push(locked.clone());
            } else {
                updates.push(ProfileChange {
                    target: locked.clone(),
                    reasons,
                });
            }
            if let Some(previous_order) = current_order.get(locked.instance_id.as_str()) {
                if *previous_order != locked.order {
                    order_changes.push(ProfileOrderChange {
                        instance_id: locked.instance_id.clone(),
                        previous_order: *previous_order,
                        target_order: locked.order,
                    });
                }
            }
        }

        let current_manifest = current.map(manifest_fingerprint).transpose()?;
        let source_installation_id = current.map(|manifest| manifest.installation_id().to_owned());
        let previous_profile_id = previous_active.map(|profile| profile.profile_id.clone());
        let previous_lock = previous_active
            .map(ProfileLockfile::fingerprint)
            .transpose()?;
        let target_lock = target.fingerprint()?;
        let plan_fingerprint = profile_switch_plan_fingerprint(ProfilePlanFingerprintMaterial {
            version: PROFILE_PLAN_CANONICALIZATION_VERSION,
            installation_id: &target.installation_id,
            target_profile_id: &target.profile_id,
            source_installation_id: source_installation_id.as_deref(),
            source_generation: current.map(InstallationManifest::generation),
            current_manifest: current_manifest.as_deref(),
            previous_profile_id: previous_profile_id.as_deref(),
            previous_lock: previous_lock.as_deref(),
            target_lock: &target_lock,
            removals: &removals,
            installs: &installs,
            updates: &updates,
            retained: &retained,
            order_changes: &order_changes,
            lease_contract: ProfileLeaseContract::OneInstallationLease,
            operation_engine: ProfileOperationEngine::ExistingLifecycleOperations,
            commit_condition: ProfileCommitCondition::AllLifecycleOperationsVerified,
            failure_policy: ProfileFailurePolicy::RollBackAndKeepPreviousActive,
        })?;

        let mut operation_specs: Vec<_> = removals
            .iter()
            .map(|item| (LifecycleOperationKind::Uninstall, item.instance_id.clone()))
            .collect();
        operation_specs.extend(updates.iter().map(|change| {
            (
                LifecycleOperationKind::Update,
                change.target.instance_id.clone(),
            )
        }));
        operation_specs.extend(installs.iter().map(|change| {
            (
                LifecycleOperationKind::Install,
                change.target.instance_id.clone(),
            )
        }));
        let operations = operation_specs
            .into_iter()
            .enumerate()
            .map(|(index, (kind, instance_id))| {
                let operation_id = profile_operation_identity(
                    b"deltamod:profile-operation-id:v1\0",
                    &plan_fingerprint,
                    kind,
                    &instance_id,
                );
                let idempotency_key = profile_operation_identity(
                    b"deltamod:profile-idempotency-key:v1\0",
                    &plan_fingerprint,
                    kind,
                    &instance_id,
                );
                PlannedProfileOperation {
                    sequence: index as u32,
                    kind,
                    instance_id,
                    operation_id,
                    idempotency_key,
                }
            })
            .collect();

        Ok(Self {
            installation_id: target.installation_id.clone(),
            target_profile_id: target.profile_id.clone(),
            source_installation_id,
            source_manifest_generation: current.map(InstallationManifest::generation),
            source_manifest_fingerprint: current_manifest,
            previous_lock_fingerprint: previous_lock,
            target_lock_fingerprint: target_lock,
            removals,
            installs,
            updates,
            retained,
            order_changes,
            operations,
            lease_contract: ProfileLeaseContract::OneInstallationLease,
            operation_engine: ProfileOperationEngine::ExistingLifecycleOperations,
            commit_boundary: ProfileCommitBoundary {
                previous_profile_id,
                target_profile_id: target.profile_id.clone(),
                condition: ProfileCommitCondition::AllLifecycleOperationsVerified,
                on_failure: ProfileFailurePolicy::RollBackAndKeepPreviousActive,
            },
            fingerprint: plan_fingerprint,
        })
    }

    #[must_use]
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    #[must_use]
    pub fn target_profile_id(&self) -> &str {
        &self.target_profile_id
    }

    #[must_use]
    pub fn source_installation_id(&self) -> Option<&str> {
        self.source_installation_id.as_deref()
    }

    #[must_use]
    pub const fn source_manifest_generation(&self) -> Option<u64> {
        self.source_manifest_generation
    }

    #[must_use]
    pub fn source_manifest_fingerprint(&self) -> Option<&str> {
        self.source_manifest_fingerprint.as_deref()
    }

    #[must_use]
    pub fn previous_lock_fingerprint(&self) -> Option<&str> {
        self.previous_lock_fingerprint.as_deref()
    }

    #[must_use]
    pub fn target_lock_fingerprint(&self) -> &str {
        &self.target_lock_fingerprint
    }

    #[must_use]
    pub fn removals(&self) -> &[ProfileRemoval] {
        &self.removals
    }

    #[must_use]
    pub fn installs(&self) -> &[ProfileChange] {
        &self.installs
    }

    #[must_use]
    pub fn updates(&self) -> &[ProfileChange] {
        &self.updates
    }

    #[must_use]
    pub fn retained(&self) -> &[LockedProfileMod] {
        &self.retained
    }

    #[must_use]
    pub fn order_changes(&self) -> &[ProfileOrderChange] {
        &self.order_changes
    }

    #[must_use]
    pub fn operations(&self) -> &[PlannedProfileOperation] {
        &self.operations
    }

    #[must_use]
    pub const fn lease_contract(&self) -> ProfileLeaseContract {
        self.lease_contract
    }

    #[must_use]
    pub const fn operation_engine(&self) -> ProfileOperationEngine {
        self.operation_engine
    }

    #[must_use]
    pub fn commit_boundary(&self) -> &ProfileCommitBoundary {
        &self.commit_boundary
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn integrity_valid(&self) -> bool {
        let source_presence = [
            self.source_installation_id.is_some(),
            self.source_manifest_generation.is_some(),
            self.source_manifest_fingerprint.is_some(),
        ];
        if source_presence
            .iter()
            .any(|present| *present != source_presence[0])
            || self
                .source_installation_id
                .as_deref()
                .is_some_and(|source| source != self.installation_id)
            || (self.previous_lock_fingerprint.is_some()
                != self.commit_boundary.previous_profile_id.is_some())
            || self.commit_boundary.target_profile_id != self.target_profile_id
            || self.lease_contract != ProfileLeaseContract::OneInstallationLease
            || self.operation_engine != ProfileOperationEngine::ExistingLifecycleOperations
            || self.commit_boundary.condition
                != ProfileCommitCondition::AllLifecycleOperationsVerified
            || self.commit_boundary.on_failure
                != ProfileFailurePolicy::RollBackAndKeepPreviousActive
        {
            return false;
        }
        let expected_fingerprint =
            profile_switch_plan_fingerprint(ProfilePlanFingerprintMaterial {
                version: PROFILE_PLAN_CANONICALIZATION_VERSION,
                installation_id: &self.installation_id,
                target_profile_id: &self.target_profile_id,
                source_installation_id: self.source_installation_id.as_deref(),
                source_generation: self.source_manifest_generation,
                current_manifest: self.source_manifest_fingerprint.as_deref(),
                previous_profile_id: self.commit_boundary.previous_profile_id.as_deref(),
                previous_lock: self.previous_lock_fingerprint.as_deref(),
                target_lock: &self.target_lock_fingerprint,
                removals: &self.removals,
                installs: &self.installs,
                updates: &self.updates,
                retained: &self.retained,
                order_changes: &self.order_changes,
                lease_contract: self.lease_contract,
                operation_engine: self.operation_engine,
                commit_condition: self.commit_boundary.condition,
                failure_policy: self.commit_boundary.on_failure,
            });
        if expected_fingerprint.as_deref() != Ok(self.fingerprint.as_str()) {
            return false;
        }

        let expected_operations: Vec<_> = self
            .removals
            .iter()
            .map(|item| (LifecycleOperationKind::Uninstall, item.instance_id.as_str()))
            .chain(self.updates.iter().map(|change| {
                (
                    LifecycleOperationKind::Update,
                    change.target.instance_id.as_str(),
                )
            }))
            .chain(self.installs.iter().map(|change| {
                (
                    LifecycleOperationKind::Install,
                    change.target.instance_id.as_str(),
                )
            }))
            .collect();
        if expected_operations.len() != self.operations.len() {
            return false;
        }
        expected_operations
            .iter()
            .zip(&self.operations)
            .enumerate()
            .all(|(index, ((kind, instance_id), operation))| {
                u32::try_from(index).ok() == Some(operation.sequence)
                    && *kind == operation.kind
                    && *instance_id == operation.instance_id
                    && operation.operation_id
                        == profile_operation_identity(
                            b"deltamod:profile-operation-id:v1\0",
                            &self.fingerprint,
                            *kind,
                            instance_id,
                        )
                    && operation.idempotency_key
                        == profile_operation_identity(
                            b"deltamod:profile-idempotency-key:v1\0",
                            &self.fingerprint,
                            *kind,
                            instance_id,
                        )
            })
    }

    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.operations.is_empty() && self.order_changes.is_empty()
    }

    pub fn preflight(
        &self,
        current: Option<&InstallationManifest>,
        resolved: &[ValidatedInstallPlan],
    ) -> ProfilePreflightReport {
        if !self.integrity_valid() {
            return ProfilePreflightReport {
                plan_fingerprint: self.fingerprint.clone(),
                disposition: ProfilePreflightDisposition::Blocked,
                errors: vec![ProfilePreflightError::PlanIntegrityMismatch],
                path_conflicts: Vec::new(),
                operations: Vec::new(),
            };
        }
        let mut errors = Vec::new();
        let source_matches = match current {
            Some(manifest) => {
                validate_installation_manifest(&manifest.records, &manifest.ledger).is_ok()
                    && self.source_installation_id.as_deref() == Some(manifest.installation_id())
                    && self.source_manifest_generation == Some(manifest.generation())
                    && manifest_fingerprint(manifest).ok().as_ref()
                        == self.source_manifest_fingerprint.as_ref()
            }
            None => {
                self.source_installation_id.is_none()
                    && self.source_manifest_generation.is_none()
                    && self.source_manifest_fingerprint.is_none()
            }
        };
        if !source_matches {
            errors.push(ProfilePreflightError::StaleManifest);
        }
        let expected_changes: Vec<_> = self
            .operations
            .iter()
            .filter(|operation| {
                matches!(
                    operation.kind,
                    LifecycleOperationKind::Install | LifecycleOperationKind::Update
                )
            })
            .collect();
        let expected_by_instance: BTreeMap<_, _> = expected_changes
            .iter()
            .map(|operation| (operation.instance_id.as_str(), *operation))
            .collect();
        let mut resolved_by_instance: BTreeMap<String, Vec<&ValidatedInstallPlan>> =
            BTreeMap::new();
        for plan in resolved {
            resolved_by_instance
                .entry(plan.metadata().instance_id.clone())
                .or_default()
                .push(plan);
        }
        for (instance_id, plans) in &resolved_by_instance {
            if !expected_by_instance.contains_key(instance_id.as_str()) {
                errors.push(ProfilePreflightError::UnexpectedResolvedPlan {
                    instance_id: instance_id.clone(),
                });
            } else if plans.len() != 1 {
                errors.push(ProfilePreflightError::DuplicateResolvedPlan {
                    instance_id: instance_id.clone(),
                });
            }
        }

        let mut files_by_instance: BTreeMap<String, &[crate::InstallFilePlan]> = BTreeMap::new();
        for expected in &expected_changes {
            let Some(plans) = resolved_by_instance.get(&expected.instance_id) else {
                errors.push(ProfilePreflightError::MissingResolvedPlan {
                    instance_id: expected.instance_id.clone(),
                });
                continue;
            };
            if plans.len() != 1 {
                continue;
            }
            let plan = plans[0];
            let target = self
                .installs
                .iter()
                .chain(&self.updates)
                .find(|change| change.target.instance_id == expected.instance_id)
                .expect("switch operations are derived from changes");
            if plan.request().operation_id() != expected.operation_id
                || plan.request().idempotency_key() != expected.idempotency_key
            {
                errors.push(ProfilePreflightError::OperationBindingMismatch {
                    instance_id: expected.instance_id.clone(),
                });
            }
            if plan.request().intent().kind != expected.kind
                || plan.request().intent().installation_id != self.installation_id
                || plan.request().intent().profile_id.as_deref()
                    != Some(self.target_profile_id.as_str())
                || plan.metadata().mod_id != target.target.mod_id
                || plan.metadata().display_name != target.target.display_name
                || plan.metadata().version != target.target.version
                || plan.metadata().provider != target.target.provider
                || plan.metadata().archive_sha256.as_deref()
                    != Some(target.target.archive_sha256.as_str())
                || plan.fingerprint() != target.target.file_plan_fingerprint
            {
                errors.push(ProfilePreflightError::ExactIdentityMismatch {
                    instance_id: expected.instance_id.clone(),
                });
            }
            if target
                .reasons
                .contains(&ProfileUpdateReason::ConfigurationFingerprint)
                && !target
                    .reasons
                    .contains(&ProfileUpdateReason::FilePlanFingerprint)
            {
                errors.push(ProfilePreflightError::ConfigurationNotMaterialized {
                    instance_id: expected.instance_id.clone(),
                });
            }
            files_by_instance.insert(expected.instance_id.clone(), plan.files());
        }

        let current_records: BTreeMap<_, _> = current
            .into_iter()
            .flat_map(|manifest| &manifest.records)
            .map(|record| (record.instance_id.as_str(), record))
            .collect();
        let mut claims_by_identity: BTreeMap<String, DesiredProfileFile> = BTreeMap::new();
        let mut claims_by_path: BTreeMap<ValidatedRelativePath, DesiredProfileFile> =
            BTreeMap::new();
        let mut conflicts = Vec::new();
        let mut desired: Vec<_> = self
            .installs
            .iter()
            .chain(&self.updates)
            .map(|change| &change.target)
            .chain(self.retained.iter())
            .collect();
        desired.sort_by_key(|item| item.order);
        for item in desired {
            let planned_files = files_by_instance.get(item.instance_id.as_str()).copied();
            let installed_files = current_records
                .get(item.instance_id.as_str())
                .map(|record| record.files.as_slice());
            if let Some(files) = planned_files {
                for file in files {
                    collect_profile_file(
                        &mut claims_by_identity,
                        &mut claims_by_path,
                        &mut conflicts,
                        DesiredProfileFile {
                            instance_id: item.instance_id.clone(),
                            path: file.path.clone(),
                            path_identity_key: file.path_identity_key.clone(),
                            sha256: file.sha256.to_ascii_lowercase(),
                        },
                    );
                }
            } else if let Some(files) = installed_files {
                for file in files {
                    collect_profile_file(
                        &mut claims_by_identity,
                        &mut claims_by_path,
                        &mut conflicts,
                        DesiredProfileFile {
                            instance_id: item.instance_id.clone(),
                            path: file.path.clone(),
                            path_identity_key: file.path_identity_key.clone(),
                            sha256: file.expected_sha256.to_ascii_lowercase(),
                        },
                    );
                }
            }
        }
        conflicts.sort_by(|left, right| {
            left.path_identity_key
                .cmp(&right.path_identity_key)
                .then_with(|| left.first_instance_id.cmp(&right.first_instance_id))
                .then_with(|| left.second_instance_id.cmp(&right.second_instance_id))
        });
        let mut execution_operations = Vec::new();
        if errors.is_empty() && conflicts.is_empty() {
            match schedule_profile_operations(self, current, &files_by_instance) {
                Ok(operations) => execution_operations = operations,
                Err(error) => errors.push(error),
            }
        }
        let disposition = if !errors.is_empty() || !conflicts.is_empty() {
            ProfilePreflightDisposition::Blocked
        } else if self.is_noop() {
            ProfilePreflightDisposition::NoOp
        } else {
            ProfilePreflightDisposition::Ready
        };
        ProfilePreflightReport {
            plan_fingerprint: self.fingerprint.clone(),
            disposition,
            errors,
            path_conflicts: conflicts,
            operations: execution_operations,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfilePreflightError {
    PlanIntegrityMismatch,
    StaleManifest,
    MissingResolvedPlan { instance_id: String },
    UnexpectedResolvedPlan { instance_id: String },
    DuplicateResolvedPlan { instance_id: String },
    OperationBindingMismatch { instance_id: String },
    ExactIdentityMismatch { instance_id: String },
    ConfigurationNotMaterialized { instance_id: String },
    UnsafeOwnershipTransition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilePathConflictReason {
    DifferentContent,
    AliasedIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePathConflict {
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub first_instance_id: String,
    pub second_instance_id: String,
    pub reason: ProfilePathConflictReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfilePreflightDisposition {
    Ready,
    NoOp,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePreflightReport {
    plan_fingerprint: String,
    disposition: ProfilePreflightDisposition,
    errors: Vec<ProfilePreflightError>,
    path_conflicts: Vec<ProfilePathConflict>,
    /// Deterministic dependency-safe order. Empty whenever preflight blocks.
    operations: Vec<PlannedProfileOperation>,
}

impl ProfilePreflightReport {
    #[must_use]
    pub fn plan_fingerprint(&self) -> &str {
        &self.plan_fingerprint
    }

    #[must_use]
    pub const fn disposition(&self) -> ProfilePreflightDisposition {
        self.disposition
    }

    #[must_use]
    pub fn errors(&self) -> &[ProfilePreflightError] {
        &self.errors
    }

    #[must_use]
    pub fn path_conflicts(&self) -> &[ProfilePathConflict] {
        &self.path_conflicts
    }

    #[must_use]
    pub fn operations(&self) -> &[PlannedProfileOperation] {
        &self.operations
    }
}

fn schedule_profile_operations(
    plan: &ProfileSwitchPlan,
    current: Option<&InstallationManifest>,
    files_by_instance: &BTreeMap<String, &[crate::InstallFilePlan]>,
) -> Result<Vec<PlannedProfileOperation>, ProfilePreflightError> {
    let operation_by_instance: BTreeMap<_, _> = plan
        .operations
        .iter()
        .enumerate()
        .map(|(index, operation)| (operation.instance_id.as_str(), index))
        .collect();
    let claims_by_identity: BTreeMap<_, _> = current
        .into_iter()
        .flat_map(|manifest| &manifest.ledger.claims)
        .map(|claim| (claim.path_identity_key.as_str(), claim))
        .collect();
    let mut outgoing = vec![BTreeSet::new(); plan.operations.len()];
    let mut indegree = vec![0_usize; plan.operations.len()];

    for (acquiring_index, operation) in plan.operations.iter().enumerate() {
        if !matches!(
            operation.kind,
            LifecycleOperationKind::Install | LifecycleOperationKind::Update
        ) {
            continue;
        }
        let target_files = files_by_instance
            .get(&operation.instance_id)
            .ok_or(ProfilePreflightError::UnsafeOwnershipTransition)?;
        for target_file in *target_files {
            let Some(current_claim) =
                claims_by_identity.get(target_file.path_identity_key.as_str())
            else {
                continue;
            };
            if current_claim
                .sha256
                .eq_ignore_ascii_case(&target_file.sha256)
            {
                continue;
            }
            for owner in &current_claim.owners {
                if owner == &operation.instance_id {
                    continue;
                }
                let releasing_index = *operation_by_instance
                    .get(owner.as_str())
                    .ok_or(ProfilePreflightError::UnsafeOwnershipTransition)?;
                let releasing = &plan.operations[releasing_index];
                match releasing.kind {
                    LifecycleOperationKind::Uninstall => {}
                    LifecycleOperationKind::Update => {
                        let owner_files = files_by_instance
                            .get(owner)
                            .ok_or(ProfilePreflightError::UnsafeOwnershipTransition)?;
                        if let Some(owner_target) = owner_files
                            .iter()
                            .find(|file| file.path_identity_key == target_file.path_identity_key)
                        {
                            if !owner_target
                                .sha256
                                .eq_ignore_ascii_case(&target_file.sha256)
                            {
                                return Err(ProfilePreflightError::UnsafeOwnershipTransition);
                            }
                        }
                    }
                    _ => return Err(ProfilePreflightError::UnsafeOwnershipTransition),
                }
                if outgoing[releasing_index].insert(acquiring_index) {
                    indegree[acquiring_index] += 1;
                }
            }
        }
    }

    let mut ready: BTreeSet<usize> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect();
    let mut ordered = Vec::with_capacity(plan.operations.len());
    while let Some(index) = ready.pop_first() {
        let mut operation = plan.operations[index].clone();
        operation.sequence = ordered.len() as u32;
        ordered.push(operation);
        for dependent in outgoing[index].iter().copied() {
            indegree[dependent] -= 1;
            if indegree[dependent] == 0 {
                ready.insert(dependent);
            }
        }
    }
    if ordered.len() != plan.operations.len() {
        return Err(ProfilePreflightError::UnsafeOwnershipTransition);
    }
    Ok(ordered)
}

#[derive(Clone)]
struct DesiredProfileFile {
    instance_id: String,
    path: ValidatedRelativePath,
    path_identity_key: String,
    sha256: String,
}

fn collect_profile_file(
    by_identity: &mut BTreeMap<String, DesiredProfileFile>,
    by_path: &mut BTreeMap<ValidatedRelativePath, DesiredProfileFile>,
    conflicts: &mut Vec<ProfilePathConflict>,
    candidate: DesiredProfileFile,
) {
    if let Some(existing) = by_identity.get(&candidate.path_identity_key) {
        if existing.path != candidate.path || existing.sha256 != candidate.sha256 {
            conflicts.push(ProfilePathConflict {
                path: candidate.path.clone(),
                path_identity_key: candidate.path_identity_key.clone(),
                first_instance_id: existing.instance_id.clone(),
                second_instance_id: candidate.instance_id.clone(),
                reason: if existing.path == candidate.path {
                    ProfilePathConflictReason::DifferentContent
                } else {
                    ProfilePathConflictReason::AliasedIdentity
                },
            });
        }
    } else {
        by_identity.insert(candidate.path_identity_key.clone(), candidate.clone());
    }
    if let Some(existing) = by_path.get(&candidate.path) {
        if existing.path_identity_key != candidate.path_identity_key {
            conflicts.push(ProfilePathConflict {
                path: candidate.path.clone(),
                path_identity_key: candidate.path_identity_key.clone(),
                first_instance_id: existing.instance_id.clone(),
                second_instance_id: candidate.instance_id.clone(),
                reason: ProfilePathConflictReason::AliasedIdentity,
            });
        }
    } else {
        by_path.insert(candidate.path.clone(), candidate);
    }
}

fn validate_header(
    actual_kind: ProfileDocumentKind,
    expected_kind: ProfileDocumentKind,
    version: u32,
) -> Result<(), ProfileError> {
    if actual_kind != expected_kind {
        return Err(ProfileError::WrongKind);
    }
    if version != PROFILE_SCHEMA_VERSION {
        return Err(ProfileError::UnsupportedSchema {
            found: u64::from(version),
            supported: PROFILE_SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn validate_profile_ids(profile: &str, game: &str, installation: &str) -> Result<(), ProfileError> {
    if valid_id(profile, 256) && valid_id(game, 256) && valid_id(installation, 256) {
        Ok(())
    } else {
        Err(ProfileError::Invalid("profile/game/installation identity"))
    }
}

fn validate_ordered_instances<'a>(
    items: impl Iterator<Item = (u32, &'a str)>,
    count: usize,
) -> Result<(), ProfileError> {
    if count > MAX_PROFILE_MODS {
        return Err(ProfileError::Invalid("profile size"));
    }
    let mut instances = BTreeSet::new();
    for (index, (order, instance_id)) in items.enumerate() {
        if order != index as u32 {
            return Err(ProfileError::Invalid("non-canonical mod order"));
        }
        if !instances.insert(instance_id) {
            return Err(ProfileError::DuplicateInstance(instance_id.to_owned()));
        }
    }
    Ok(())
}

fn validate_mod_fields(
    instance_id: &str,
    mod_id: &str,
    display_name: &str,
    provider: &ProviderRef,
    configuration_fingerprint: Option<&str>,
) -> Result<(), ProfileError> {
    if !valid_id(instance_id, 256)
        || !valid_id(mod_id, 256)
        || display_name.is_empty()
        || display_name.len() > 512
        || display_name.chars().any(char::is_control)
        || provider.validate().is_err()
    {
        return Err(ProfileError::Invalid("locked mod identity"));
    }
    if let Some(hash) = configuration_fingerprint {
        validate_hash(hash, "configuration fingerprint")?;
    }
    Ok(())
}

fn validate_hash(value: &str, field: &'static str) -> Result<(), ProfileError> {
    if valid_sha256(value) && value == value.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(ProfileError::Invalid(field))
    }
}

fn provider_refines(desired: &ProviderRef, locked: &ProviderRef) -> bool {
    desired.canonical_identity() == locked.canonical_identity()
        && (desired.artifact_kind() == ProviderArtifactKind::Unknown
            || desired.artifact_kind() == locked.artifact_kind())
        && desired
            .artifact_id()
            .is_none_or(|value| Some(value) == locked.artifact_id())
        && desired
            .version_id()
            .is_none_or(|value| Some(value) == locked.version_id())
        && desired
            .canonical_url()
            .is_none_or(|value| Some(value) == locked.canonical_url())
}

fn import_canonical<T>(
    input: &str,
    expected_kind: ProfileDocumentKind,
    validate: fn(&T) -> Result<(), ProfileError>,
) -> Result<T, ProfileError>
where
    T: DeserializeOwned + Serialize,
{
    if input.is_empty() || input.len() > MAX_PROFILE_BYTES {
        return Err(ProfileError::Malformed);
    }
    let header: serde_json::Value =
        serde_json::from_str(input).map_err(|_| ProfileError::Malformed)?;
    let object = header.as_object().ok_or(ProfileError::Malformed)?;
    if object
        .get("documentKind")
        .and_then(serde_json::Value::as_str)
        != Some(expected_kind.as_str())
    {
        return Err(ProfileError::WrongKind);
    }
    let found = object
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ProfileError::Malformed)?;
    if found != u64::from(PROFILE_SCHEMA_VERSION) {
        return Err(ProfileError::UnsupportedSchema {
            found,
            supported: PROFILE_SCHEMA_VERSION,
        });
    }
    let value: T = serde_json::from_str(input).map_err(|_| ProfileError::Malformed)?;
    validate(&value)?;
    let canonical = serde_json::to_string(&value).map_err(|_| ProfileError::Malformed)?;
    if canonical != input {
        return Err(ProfileError::NonCanonical);
    }
    Ok(value)
}

fn fingerprint(domain: &[u8], canonical: &str) -> Result<String, ProfileError> {
    let mut bytes = Vec::with_capacity(domain.len() + canonical.len());
    bytes.extend_from_slice(domain);
    bytes.extend_from_slice(canonical.as_bytes());
    Ok(hex_digest(&bytes))
}

fn manifest_fingerprint(manifest: &InstallationManifest) -> Result<String, ProfileError> {
    #[derive(Serialize)]
    struct Manifest<'a> {
        records: &'a [deltamod_product_contracts::InstalledModRecord],
        ledger: &'a deltamod_product_contracts::InstallationClaimsLedger,
    }
    let canonical = serde_json::to_string(&Manifest {
        records: &manifest.records,
        ledger: &manifest.ledger,
    })
    .map_err(|_| ProfileError::Malformed)?;
    fingerprint(b"deltamod:installation-manifest:v1\0", &canonical)
}

fn profile_switch_plan_fingerprint(
    material: ProfilePlanFingerprintMaterial<'_>,
) -> Result<String, ProfileError> {
    let canonical = serde_json::to_string(&material).map_err(|_| ProfileError::Malformed)?;
    fingerprint(b"deltamod:profile-switch-plan:v1\0", &canonical)
}

fn profile_operation_identity(
    domain: &[u8],
    plan_fingerprint: &str,
    kind: LifecycleOperationKind,
    instance_id: &str,
) -> String {
    let canonical = format!("{plan_fingerprint}\0{}\0{instance_id}", kind.as_str());
    fingerprint(domain, &canonical).expect("string fingerprinting cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use deltamod_product_contracts::{ProviderId, ProviderResourceId};

    const ARCHIVE_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const FILE_PLAN_FINGERPRINT: &str =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn target_lockfile() -> ProfileLockfile {
        let provider = ProviderRef::new(
            ProviderId::parse("gamebanana").unwrap(),
            ProviderItemKind::Mod,
            ProviderResourceId::parse("101").unwrap(),
            None,
            Some(ProviderResourceId::parse("artifact-1").unwrap()),
            ProviderArtifactKind::File,
            Some(ProviderResourceId::parse("1").unwrap()),
            Some("https://gamebanana.com/mods/101".into()),
        )
        .unwrap();
        let definition = ProfileDefinition::new(
            "target",
            "deltarune",
            "game",
            vec![ProfileModDefinition {
                order: 0,
                instance_id: "a".into(),
                mod_id: "mod-a".into(),
                display_name: "Mod a".into(),
                provider: provider.clone(),
                configuration_fingerprint: None,
            }],
        )
        .unwrap();
        ProfileLockfile::new(
            &definition,
            vec![LockedProfileMod {
                order: 0,
                instance_id: "a".into(),
                mod_id: "mod-a".into(),
                display_name: "Mod a".into(),
                version: Some("1".into()),
                provider,
                archive_sha256: ARCHIVE_SHA256.into(),
                file_plan_fingerprint: FILE_PLAN_FINGERPRINT.into(),
                configuration_fingerprint: None,
            }],
        )
        .unwrap()
    }

    fn assert_integrity_blocked(plan: &ProfileSwitchPlan) {
        let report = plan.preflight(None, &[]);
        assert_eq!(report.disposition(), ProfilePreflightDisposition::Blocked);
        assert_eq!(
            report.errors(),
            &[ProfilePreflightError::PlanIntegrityMismatch]
        );
        assert!(report.operations().is_empty());
    }

    #[test]
    fn tampered_plan_bindings_fail_closed_before_preflight_can_be_ready() {
        let plan = ProfileSwitchPlan::build(None, None, &target_lockfile()).unwrap();
        assert!(plan.integrity_valid());

        let mut forged_operation = plan.clone();
        forged_operation.operations[0].operation_id = "forged-operation".into();
        assert_integrity_blocked(&forged_operation);

        let mut forged_key = plan.clone();
        forged_key.operations[0].idempotency_key = "forged-idempotency-key".into();
        assert_integrity_blocked(&forged_key);

        let mut forged_fingerprint = plan;
        forged_fingerprint.fingerprint = ARCHIVE_SHA256.into();
        assert_integrity_blocked(&forged_fingerprint);
    }
}
