use deltamod_product_contracts::{
    LifecycleOperationKind, OperationRequest, ProviderRef, SchemaError, ValidatedRelativePath,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use thiserror::Error;

pub const FILE_PLAN_CANONICALIZATION_VERSION: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum StagingSource {
    Artifact {
        source_id: String,
    },
    ArtifactTree {
        source_id: String,
        source_path: ValidatedRelativePath,
    },
    RecoveryBackup {
        generation_id: String,
        backup_path: ValidatedRelativePath,
    },
}

impl StagingSource {
    fn validate(&self) -> bool {
        match self {
            Self::Artifact { source_id } | Self::ArtifactTree { source_id, .. } => {
                valid_id(source_id, 256)
            }
            Self::RecoveryBackup { generation_id, .. } => valid_id(generation_id, 128),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallFilePlan {
    pub path: ValidatedRelativePath,
    pub path_identity_key: String,
    pub sha256: String,
    pub size_bytes: u64,
    /// Exact hash of an existing, currently unclaimed file that this plan is
    /// explicitly authorized to adopt. A mismatch remains a hard conflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_previous_sha256: Option<String>,
    pub source: StagingSource,
}

impl InstallFilePlan {
    fn validate(&self) -> bool {
        valid_identity_key(&self.path_identity_key)
            && valid_sha256(&self.sha256)
            && self
                .expected_previous_sha256
                .as_deref()
                .is_none_or(valid_sha256)
            && self.source.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallMetadata {
    pub instance_id: String,
    pub mod_id: String,
    pub display_name: String,
    pub version: Option<String>,
    pub provider: ProviderRef,
    pub archive_sha256: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedInstallPlan {
    request: OperationRequest,
    metadata: InstallMetadata,
    files: Vec<InstallFilePlan>,
    fingerprint: String,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PlanError {
    #[error("invalid operation request")]
    InvalidRequest,
    #[error("install identity does not match the frozen operation intent")]
    IntentMismatch,
    #[error("file plan is empty or invalid")]
    InvalidFilePlan,
    #[error("file plan contains duplicate paths or identity keys")]
    DuplicateFile,
    #[error("file plan fingerprint does not match the frozen operation intent")]
    FingerprintMismatch,
    #[error("file plan size overflow")]
    SizeOverflow,
}

impl ValidatedInstallPlan {
    pub fn new(
        request: OperationRequest,
        metadata: InstallMetadata,
        mut files: Vec<InstallFilePlan>,
    ) -> Result<Self, PlanError> {
        request.validate().map_err(|_| PlanError::InvalidRequest)?;
        if !matches!(
            request.intent().kind,
            LifecycleOperationKind::Install | LifecycleOperationKind::Update
        ) || request.intent().installation_id.is_empty()
            || request.intent().mod_instance_id.as_deref() != Some(&metadata.instance_id)
            || request.intent().provider.as_ref() != Some(&metadata.provider)
            || !optional_hash_eq(
                request.intent().archive_sha256.as_deref(),
                metadata.archive_sha256.as_deref(),
            )
            || !valid_metadata(&metadata)
        {
            return Err(PlanError::IntentMismatch);
        }
        if files.is_empty() || files.iter().any(|file| !file.validate()) {
            return Err(PlanError::InvalidFilePlan);
        }
        files.sort_by(|left, right| {
            left.path_identity_key
                .cmp(&right.path_identity_key)
                .then_with(|| left.path.cmp(&right.path))
        });
        let mut identities = BTreeSet::new();
        let mut paths = BTreeSet::new();
        if files.iter().any(|file| {
            !identities.insert(file.path_identity_key.clone()) || !paths.insert(file.path.clone())
        }) {
            return Err(PlanError::DuplicateFile);
        }
        files.iter().try_fold(0_u64, |total, file| {
            total
                .checked_add(file.size_bytes)
                .ok_or(PlanError::SizeOverflow)
        })?;
        let fingerprint = file_plan_fingerprint(&files);
        if request
            .intent()
            .file_plan_fingerprint
            .as_deref()
            .is_none_or(|expected| !expected.eq_ignore_ascii_case(&fingerprint))
        {
            return Err(PlanError::FingerprintMismatch);
        }
        Ok(Self {
            request,
            metadata,
            files,
            fingerprint,
        })
    }

    #[must_use]
    pub fn request(&self) -> &OperationRequest {
        &self.request
    }

    #[must_use]
    pub fn metadata(&self) -> &InstallMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn files(&self) -> &[InstallFilePlan] {
        &self.files
    }

    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    #[must_use]
    pub fn staging_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size_bytes).sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedUninstallPlan {
    request: OperationRequest,
}

impl ValidatedUninstallPlan {
    pub fn new(request: OperationRequest) -> Result<Self, PlanError> {
        request.validate().map_err(|_| PlanError::InvalidRequest)?;
        if request.intent().kind != LifecycleOperationKind::Uninstall
            || request.intent().mod_instance_id.is_none()
            || request.intent().provider.is_some()
            || request.intent().archive_sha256.is_some()
            || request.intent().file_plan_fingerprint.is_some()
            || request.intent().profile_id.is_some()
        {
            return Err(PlanError::IntentMismatch);
        }
        Ok(Self { request })
    }

    #[must_use]
    pub fn request(&self) -> &OperationRequest {
        &self.request
    }
}

#[must_use]
pub fn file_plan_fingerprint(files: &[InstallFilePlan]) -> String {
    fn field(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_be_bytes());
        output.extend_from_slice(value.as_bytes());
    }

    let mut ordered = files.to_vec();
    ordered.sort_by(|left, right| {
        left.path_identity_key
            .cmp(&right.path_identity_key)
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut bytes = b"deltamod:file-plan\0".to_vec();
    bytes.extend_from_slice(&FILE_PLAN_CANONICALIZATION_VERSION.to_be_bytes());
    bytes.extend_from_slice(
        &u32::try_from(ordered.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for file in ordered {
        field(&mut bytes, file.path.as_str());
        field(&mut bytes, &file.path_identity_key);
        field(&mut bytes, &file.sha256.to_ascii_lowercase());
        bytes.extend_from_slice(&file.size_bytes.to_be_bytes());
        field(
            &mut bytes,
            file.expected_previous_sha256.as_deref().unwrap_or(""),
        );
    }
    hex_digest(&bytes)
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

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

pub(crate) fn valid_identity_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 8192
        && !value.chars().any(char::is_control)
        && !value.contains(['\\', ':'])
}

fn optional_hash_eq(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn valid_metadata(metadata: &InstallMetadata) -> bool {
    valid_id(&metadata.instance_id, 256)
        && valid_id(&metadata.mod_id, 256)
        && !metadata.display_name.is_empty()
        && metadata.display_name.len() <= 512
        && !metadata.display_name.chars().any(char::is_control)
        && metadata.version.as_deref().is_none_or(|version| {
            !version.is_empty() && version.len() <= 256 && !version.chars().any(char::is_control)
        })
        && metadata.archive_sha256.as_deref().is_none_or(valid_sha256)
        && metadata.provider.validate().is_ok()
}

impl From<SchemaError> for PlanError {
    fn from(_error: SchemaError) -> Self {
        Self::InvalidRequest
    }
}
