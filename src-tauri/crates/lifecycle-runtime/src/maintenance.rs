use crate::{
    valid_id, valid_sha256, InstallFilePlan, LifecycleWorkspace, ReleaseARuntime, StagingSource,
    StoreError,
};
use deltamod_product_contracts::{
    GameHealthReport, GameHealthReportPayload, GameHealthState, InstalledModRecord,
    JournalDisposition, LifecycleJournal, LifecycleOperationKind, ObservationSnapshot,
    ObservedFileState, OperationPhase, OperationProgress, OperationProgressPayload,
    OperationRecord, OperationRequest, ProductError, ProductErrorCode, ProductErrorPayload,
    ProviderArtifactKind, ProviderRef, RecoveryAction, ValidatedRelativePath, VerificationIssue,
    VerificationResult, VerificationResultPayload, VerificationState,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};
use thiserror::Error;

const ISSUE_MISSING_FILE: &str = "missing_file";
const ISSUE_HASH_MISMATCH: &str = "hash_mismatch";
const ISSUE_EXTERNAL_CHANGE: &str = "external_change";
const ISSUE_OWNERSHIP_CONFLICT: &str = "ownership_conflict";
const ISSUE_MANIFEST_CHANGED: &str = "manifest_changed";
const ISSUE_OPERATION_INTERRUPTED: &str = "operation_interrupted";

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MaintenanceInputError {
    #[error("invalid installation identity")]
    InvalidInstallationId,
    #[error("invalid installed mod instance identity")]
    InvalidModInstanceId,
}

/// Read-only verification scope. Game health is always installation-wide;
/// `mod_instance_id` only narrows the accompanying verification result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationScope {
    installation_id: String,
    mod_instance_id: Option<String>,
}

impl VerificationScope {
    pub fn new(
        installation_id: impl Into<String>,
        mod_instance_id: Option<String>,
    ) -> Result<Self, MaintenanceInputError> {
        let installation_id = installation_id.into();
        if !valid_id(&installation_id, 256) {
            return Err(MaintenanceInputError::InvalidInstallationId);
        }
        if mod_instance_id
            .as_deref()
            .is_some_and(|instance_id| !valid_id(instance_id, 256))
        {
            return Err(MaintenanceInputError::InvalidModInstanceId);
        }
        Ok(Self {
            installation_id,
            mod_instance_id,
        })
    }

    #[must_use]
    pub fn installation_id(&self) -> &str {
        &self.installation_id
    }

    #[must_use]
    pub fn mod_instance_id(&self) -> Option<&str> {
        self.mod_instance_id.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedArchiveSource {
    pub source_id: String,
    pub archive_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderArtifactSource {
    pub source_id: String,
    pub provider: ProviderRef,
    pub archive_sha256: Option<String>,
}

/// Read-only catalog queried in strict priority order. Implementations should
/// not download or mutate here; they report artifacts already resolved by an
/// integration layer.
pub trait RepairSourceCatalog {
    fn cached_archive(&self, expected_archive_sha256: &str) -> Option<CachedArchiveSource>;

    fn exact_provider_artifact(
        &self,
        expected_provider: &ProviderRef,
    ) -> Option<ProviderArtifactSource>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoRepairSources;

impl RepairSourceCatalog for NoRepairSources {
    fn cached_archive(&self, _expected_archive_sha256: &str) -> Option<CachedArchiveSource> {
        None
    }

    fn exact_provider_artifact(
        &self,
        _expected_provider: &ProviderRef,
    ) -> Option<ProviderArtifactSource> {
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveIdentityState {
    NotChecked,
    ExactCachedArchive,
    ExactProviderArtifact,
    SourceRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveIdentityVerification {
    pub mod_instance_id: String,
    pub expected_archive_sha256: Option<String>,
    pub expected_provider: ProviderRef,
    pub state: ArchiveIdentityState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaintenanceReport {
    pub manifest_generation: u64,
    pub verification: VerificationResult,
    pub health: GameHealthReport,
    pub archive_identities: Vec<ArchiveIdentityVerification>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaintenanceFailure {
    pub error: ProductError,
    pub report: Option<Box<MaintenanceReport>>,
}

impl fmt::Display for MaintenanceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "lifecycle maintenance failed: {}",
            self.error.code.as_str()
        )
    }
}

impl std::error::Error for MaintenanceFailure {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepairSource {
    CachedArchive(CachedArchiveSource),
    ProviderArtifact(ProviderArtifactSource),
}

impl RepairSource {
    #[must_use]
    pub fn source_id(&self) -> &str {
        match self {
            Self::CachedArchive(source) => &source.source_id,
            Self::ProviderArtifact(source) => &source.source_id,
        }
    }

    pub(crate) fn staging_source(&self, source_path: &ValidatedRelativePath) -> StagingSource {
        StagingSource::ArtifactTree {
            source_id: self.source_id().to_owned(),
            source_path: source_path.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedRepairPlan {
    request: OperationRequest,
    manifest_generation: u64,
    source: RepairSource,
    files: Vec<InstallFilePlan>,
    report: MaintenanceReport,
}

impl ValidatedRepairPlan {
    #[must_use]
    pub fn request(&self) -> &OperationRequest {
        &self.request
    }

    #[must_use]
    pub const fn manifest_generation(&self) -> u64 {
        self.manifest_generation
    }

    #[must_use]
    pub fn source(&self) -> &RepairSource {
        &self.source
    }

    #[must_use]
    pub fn files(&self) -> &[InstallFilePlan] {
        &self.files
    }

    #[must_use]
    pub fn report(&self) -> &MaintenanceReport {
        &self.report
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepairPlanDisposition {
    NotNeeded(Box<MaintenanceReport>),
    Ready(Box<ValidatedRepairPlan>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleOperationStatus {
    pub operation: OperationRecord,
    pub progress: OperationProgress,
    pub recovery_disposition: Option<JournalDisposition>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum FindingKind {
    Healthy,
    Missing,
    HashMismatch,
    ExternalChange,
    OwnershipConflict,
    Incomplete,
}

#[derive(Clone, Debug)]
struct FileFinding {
    kind: FindingKind,
    issue: Option<VerificationIssue>,
}

impl ReleaseARuntime {
    pub fn verify_installation<W>(
        &self,
        scope: &VerificationScope,
        workspace: &W,
    ) -> Result<MaintenanceReport, MaintenanceFailure>
    where
        W: LifecycleWorkspace,
    {
        self.verify_scoped(scope, workspace, None)
    }

    pub fn verify_installation_with_sources<W, S>(
        &self,
        scope: &VerificationScope,
        workspace: &W,
        sources: &S,
    ) -> Result<MaintenanceReport, MaintenanceFailure>
    where
        W: LifecycleWorkspace,
        S: RepairSourceCatalog,
    {
        self.verify_scoped(scope, workspace, Some(sources))
    }

    fn verify_scoped<W>(
        &self,
        scope: &VerificationScope,
        workspace: &W,
        sources: Option<&dyn RepairSourceCatalog>,
    ) -> Result<MaintenanceReport, MaintenanceFailure>
    where
        W: LifecycleWorkspace,
    {
        let manifest = self
            .store()
            .manifest(scope.installation_id())
            .map_err(|error| maintenance_store_failure(error, None))?
            .ok_or_else(|| {
                maintenance_failure(
                    ProductErrorCode::InvalidRequest,
                    "lifecycle.installation_not_found",
                    None,
                    None,
                    false,
                    RecoveryAction::NoAction,
                    BTreeMap::from([("installation_id".into(), scope.installation_id().into())]),
                    None,
                )
            })?;
        let selected_records: Vec<&InstalledModRecord> = match scope.mod_instance_id() {
            Some(instance_id) => vec![manifest
                .records
                .iter()
                .find(|record| record.instance_id == instance_id)
                .ok_or_else(|| {
                    maintenance_failure(
                        ProductErrorCode::InvalidRequest,
                        "lifecycle.mod_instance_not_found",
                        None,
                        None,
                        false,
                        RecoveryAction::NoAction,
                        BTreeMap::from([("mod_instance_id".into(), instance_id.into())]),
                        None,
                    )
                })?],
            None => manifest.records.iter().collect(),
        };
        let selected_identities: BTreeSet<&str> = selected_records
            .iter()
            .flat_map(|record| record.files.iter())
            .map(|file| file.path_identity_key.as_str())
            .collect();
        let transaction_root = workspace.transaction_root_identity().map_err(|error| {
            maintenance_failure(
                filesystem_error_code(&error),
                "lifecycle.verification_failed",
                None,
                Some(OperationPhase::Verifying),
                false,
                RecoveryAction::ResolveConflict,
                BTreeMap::new(),
                None,
            )
        })?;
        let interrupted = self
            .store()
            .interrupted_operations()
            .map_err(|error| maintenance_store_failure(error, None))?
            .into_iter()
            .filter(|operation| {
                operation.record.request.intent().installation_id == scope.installation_id()
            })
            .map(|operation| operation.record.request.operation_id().to_owned())
            .collect::<Vec<_>>();
        let claims: BTreeMap<&str, _> = manifest
            .ledger
            .claims
            .iter()
            .map(|claim| (claim.path_identity_key.as_str(), claim))
            .collect();
        let mut findings = BTreeMap::<String, FileFinding>::new();
        for claim in &manifest.ledger.claims {
            let finding =
                match workspace.observe_preflight_no_follow(&transaction_root, &claim.path) {
                    Ok(observation) => classify_observation(&observation, &transaction_root, claim),
                    Err(_) => FileFinding {
                        kind: FindingKind::ExternalChange,
                        issue: Some(VerificationIssue {
                            path: claim.path.clone(),
                            code: ISSUE_EXTERNAL_CHANGE.into(),
                            expected_sha256: Some(claim.sha256.clone()),
                            actual_sha256: None,
                        }),
                    },
                };
            findings.insert(claim.path_identity_key.clone(), finding);
        }
        for record in &manifest.records {
            for file in &record.files {
                let ownership_matches =
                    claims
                        .get(file.path_identity_key.as_str())
                        .is_some_and(|claim| {
                            claim.owners.contains(&record.instance_id)
                                && claim.path == file.path
                                && claim.sha256.eq_ignore_ascii_case(&file.expected_sha256)
                        });
                if !ownership_matches {
                    findings.insert(
                        file.path_identity_key.clone(),
                        FileFinding {
                            kind: FindingKind::OwnershipConflict,
                            issue: Some(VerificationIssue {
                                path: file.path.clone(),
                                code: ISSUE_OWNERSHIP_CONFLICT.into(),
                                expected_sha256: Some(file.expected_sha256.clone()),
                                actual_sha256: None,
                            }),
                        },
                    );
                }
            }
        }
        let manifest_after = self
            .store()
            .manifest(scope.installation_id())
            .map_err(|error| maintenance_store_failure(error, None))?;
        let manifest_changed = manifest_after.as_ref() != Some(&manifest);
        if manifest_changed {
            if let Some(claim) = manifest
                .ledger
                .claims
                .iter()
                .find(|claim| selected_identities.contains(claim.path_identity_key.as_str()))
            {
                findings.insert(
                    claim.path_identity_key.clone(),
                    FileFinding {
                        kind: FindingKind::Incomplete,
                        issue: Some(VerificationIssue {
                            path: claim.path.clone(),
                            code: ISSUE_MANIFEST_CHANGED.into(),
                            expected_sha256: Some(claim.sha256.clone()),
                            actual_sha256: None,
                        }),
                    },
                );
            }
        } else if !interrupted.is_empty() {
            if let Some(claim) = manifest
                .ledger
                .claims
                .iter()
                .find(|claim| selected_identities.contains(claim.path_identity_key.as_str()))
            {
                findings.insert(
                    claim.path_identity_key.clone(),
                    FileFinding {
                        kind: FindingKind::Incomplete,
                        issue: Some(VerificationIssue {
                            path: claim.path.clone(),
                            code: ISSUE_OPERATION_INTERRUPTED.into(),
                            expected_sha256: Some(claim.sha256.clone()),
                            actual_sha256: None,
                        }),
                    },
                );
            }
        }
        let checked_at_ms = self.store().now_ms();
        let mut scoped_issues: Vec<_> = findings
            .iter()
            .filter(|(identity, _)| selected_identities.contains(identity.as_str()))
            .filter_map(|(_, finding)| finding.issue.clone())
            .collect();
        scoped_issues.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.code.cmp(&right.code))
        });
        let scoped_kind = findings
            .iter()
            .filter(|(identity, _)| selected_identities.contains(identity.as_str()))
            .map(|(_, finding)| finding.kind)
            .max()
            .unwrap_or(FindingKind::Healthy);
        let verification_state = match scoped_kind {
            FindingKind::Healthy => VerificationState::Healthy,
            FindingKind::Missing => VerificationState::MissingFiles,
            FindingKind::HashMismatch => VerificationState::HashMismatch,
            FindingKind::ExternalChange => VerificationState::ExternalChanges,
            FindingKind::OwnershipConflict | FindingKind::Incomplete => {
                VerificationState::Incomplete
            }
        };
        let verification = VerificationResult::new(VerificationResultPayload {
            installation_id: scope.installation_id().to_owned(),
            mod_instance_id: scope.mod_instance_id().map(str::to_owned),
            state: verification_state,
            checked_files: u64::try_from(selected_identities.len()).unwrap_or(u64::MAX),
            issues: scoped_issues,
            verified_at_ms: checked_at_ms,
        })
        .map_err(|_| maintenance_internal(None))?;
        let all_kinds: BTreeSet<_> = findings.values().map(|finding| finding.kind).collect();
        let health_state = if !interrupted.is_empty() || manifest_changed {
            GameHealthState::InterruptedOperation
        } else if all_kinds.contains(&FindingKind::OwnershipConflict) {
            GameHealthState::ConflictingOwnership
        } else if all_kinds.contains(&FindingKind::ExternalChange)
            || all_kinds.contains(&FindingKind::HashMismatch)
        {
            GameHealthState::ExternalChangesDetected
        } else if all_kinds.contains(&FindingKind::Missing) {
            GameHealthState::MissingFiles
        } else {
            GameHealthState::Healthy
        };
        let unknown_modified_files = findings
            .values()
            .filter(|finding| {
                matches!(
                    finding.kind,
                    FindingKind::HashMismatch
                        | FindingKind::ExternalChange
                        | FindingKind::OwnershipConflict
                        | FindingKind::Incomplete
                )
            })
            .count();
        let health = GameHealthReport::new(GameHealthReportPayload {
            installation_id: scope.installation_id().to_owned(),
            state: health_state,
            lifecycle_owned_files: u64::try_from(manifest.ledger.claims.len()).unwrap_or(u64::MAX),
            unknown_modified_files: u64::try_from(unknown_modified_files).unwrap_or(u64::MAX),
            interrupted_operations: interrupted,
            checked_at_ms,
        })
        .map_err(|_| maintenance_internal(None))?;
        let archive_identities = selected_records
            .into_iter()
            .map(|record| {
                let state = sources.map_or(ArchiveIdentityState::NotChecked, |catalog| {
                    match resolve_exact_source(record, catalog) {
                        Some(RepairSource::CachedArchive(_)) => {
                            ArchiveIdentityState::ExactCachedArchive
                        }
                        Some(RepairSource::ProviderArtifact(_)) => {
                            ArchiveIdentityState::ExactProviderArtifact
                        }
                        None => ArchiveIdentityState::SourceRequired,
                    }
                });
                ArchiveIdentityVerification {
                    mod_instance_id: record.instance_id.clone(),
                    expected_archive_sha256: record.archive_sha256.clone(),
                    expected_provider: record.provider.clone(),
                    state,
                }
            })
            .collect();
        Ok(MaintenanceReport {
            manifest_generation: manifest.generation(),
            verification,
            health,
            archive_identities,
        })
    }

    pub fn plan_repair<W, S>(
        &self,
        request: OperationRequest,
        workspace: &W,
        sources: &S,
    ) -> Result<RepairPlanDisposition, MaintenanceFailure>
    where
        W: LifecycleWorkspace,
        S: RepairSourceCatalog,
    {
        let operation_id = request.operation_id().to_owned();
        request
            .validate()
            .map_err(|_| maintenance_invalid(Some(&operation_id)))?;
        let intent = request.intent();
        let instance_id = intent
            .mod_instance_id
            .as_deref()
            .filter(|_| {
                intent.kind == LifecycleOperationKind::Repair
                    && intent.profile_id.is_none()
                    && intent.provider.is_some()
                    && intent.file_plan_fingerprint.is_some()
            })
            .ok_or_else(|| maintenance_invalid(Some(&operation_id)))?;
        let scope =
            VerificationScope::new(intent.installation_id.clone(), Some(instance_id.to_owned()))
                .map_err(|_| maintenance_invalid(Some(&operation_id)))?;
        let report = self.verify_installation_with_sources(&scope, workspace, sources)?;
        let manifest = self
            .store()
            .manifest(scope.installation_id())
            .map_err(|error| maintenance_store_failure(error, Some(&operation_id)))?
            .ok_or_else(|| maintenance_invalid(Some(&operation_id)))?;
        if manifest.generation() != report.manifest_generation {
            return Err(maintenance_failure(
                ProductErrorCode::ExternalModification,
                "lifecycle.maintenance_state_changed",
                Some(&operation_id),
                Some(OperationPhase::Preflight),
                true,
                RecoveryAction::Retry,
                BTreeMap::new(),
                Some(report),
            ));
        }
        let record = manifest
            .records
            .iter()
            .find(|record| record.instance_id == instance_id)
            .ok_or_else(|| maintenance_invalid(Some(&operation_id)))?;
        let request_matches_installed = intent
            .provider
            .as_ref()
            .is_some_and(|provider| same_provider_artifact_tuple(provider, &record.provider))
            && optional_hash_eq(
                intent.archive_sha256.as_deref(),
                record.archive_sha256.as_deref(),
            )
            && intent
                .file_plan_fingerprint
                .as_deref()
                .is_some_and(|fingerprint| {
                    fingerprint.eq_ignore_ascii_case(&record.file_plan_fingerprint)
                });
        if !request_matches_installed {
            return Err(maintenance_failure(
                ProductErrorCode::IdempotencyConflict,
                "lifecycle.repair_identity_conflict",
                Some(&operation_id),
                Some(OperationPhase::Preflight),
                false,
                RecoveryAction::SelectExactSource,
                BTreeMap::new(),
                Some(report),
            ));
        }
        if matches!(
            report.health.state,
            GameHealthState::ExternalChangesDetected
                | GameHealthState::ConflictingOwnership
                | GameHealthState::InterruptedOperation
        ) {
            return Err(maintenance_failure(
                ProductErrorCode::ExternalModification,
                "lifecycle.repair_external_change",
                Some(&operation_id),
                Some(OperationPhase::Preflight),
                false,
                RecoveryAction::ResolveConflict,
                BTreeMap::new(),
                Some(report),
            ));
        }
        match report.verification.state {
            VerificationState::Healthy => {
                return Ok(RepairPlanDisposition::NotNeeded(Box::new(report)))
            }
            VerificationState::MissingFiles
                if report
                    .verification
                    .issues
                    .iter()
                    .all(|issue| issue.code == ISSUE_MISSING_FILE) => {}
            VerificationState::Incomplete => {
                return Err(maintenance_failure(
                    ProductErrorCode::ConflictDetected,
                    "lifecycle.repair_ownership_conflict",
                    Some(&operation_id),
                    Some(OperationPhase::Preflight),
                    false,
                    RecoveryAction::ResolveConflict,
                    BTreeMap::new(),
                    Some(report),
                ))
            }
            _ => {
                return Err(maintenance_failure(
                    ProductErrorCode::ExternalModification,
                    "lifecycle.repair_external_change",
                    Some(&operation_id),
                    Some(OperationPhase::Preflight),
                    false,
                    RecoveryAction::ResolveConflict,
                    BTreeMap::new(),
                    Some(report),
                ))
            }
        }
        let source = resolve_exact_source(record, sources).ok_or_else(|| {
            let mut details = BTreeMap::from([("mod_instance_id".into(), instance_id.into())]);
            if let Some(hash) = &record.archive_sha256 {
                details.insert("archive_sha256".into(), hash.clone());
            }
            maintenance_failure(
                ProductErrorCode::RecoveryUnavailable,
                "lifecycle.repair_source_required",
                Some(&operation_id),
                Some(OperationPhase::Preflight),
                false,
                RecoveryAction::SelectExactSource,
                details,
                Some(report.clone()),
            )
        })?;
        let missing: BTreeSet<_> = report
            .verification
            .issues
            .iter()
            .filter(|issue| issue.code == ISSUE_MISSING_FILE)
            .map(|issue| issue.path.clone())
            .collect();
        let mut files: Vec<_> = record
            .files
            .iter()
            .filter(|file| missing.contains(&file.path))
            .map(|file| InstallFilePlan {
                path: file.path.clone(),
                path_identity_key: file.path_identity_key.clone(),
                sha256: file.expected_sha256.clone(),
                size_bytes: 0,
                expected_previous_sha256: None,
                source: source.staging_source(&file.path),
            })
            .collect();
        files.sort_by(|left, right| left.path_identity_key.cmp(&right.path_identity_key));
        if files.len() != missing.len() || files.is_empty() {
            return Err(maintenance_internal(Some(&operation_id)));
        }
        let mut report = report;
        let mut health_payload = report.health.clone().into_payload();
        health_payload.state = GameHealthState::RepairAvailable;
        report.health = GameHealthReport::new(health_payload)
            .map_err(|_| maintenance_internal(Some(&operation_id)))?;
        Ok(RepairPlanDisposition::Ready(Box::new(
            ValidatedRepairPlan {
                request,
                manifest_generation: manifest.generation(),
                source,
                files,
                report,
            },
        )))
    }

    pub fn operation_status(
        &self,
        operation_id: &str,
    ) -> Result<Option<LifecycleOperationStatus>, MaintenanceFailure> {
        if !valid_id(operation_id, 128) {
            return Err(maintenance_invalid(Some(operation_id)));
        }
        let Some(operation) = self
            .store()
            .operation_by_id(operation_id)
            .map_err(|error| maintenance_store_failure(error, Some(operation_id)))?
        else {
            return Ok(None);
        };
        let journal = self
            .store()
            .journal_by_operation(operation_id)
            .map_err(|error| maintenance_store_failure(error, Some(operation_id)))?;
        let (completed, total, current_item) = progress_counts(&operation, journal.as_ref());
        let progress = OperationProgress::new(OperationProgressPayload {
            operation_id: operation_id.to_owned(),
            installation_id: operation.request.intent().installation_id.clone(),
            kind: operation.request.intent().kind,
            state: operation.state,
            phase: operation.phase,
            completed,
            total,
            cancellable: false,
            current_item,
            updated_at_ms: journal.as_ref().map_or(operation.updated_at_ms, |journal| {
                journal.updated_at_ms.max(operation.updated_at_ms)
            }),
        })
        .map_err(|_| maintenance_internal(Some(operation_id)))?;
        Ok(Some(LifecycleOperationStatus {
            recovery_disposition: journal.as_ref().map(LifecycleJournal::recovery_disposition),
            operation,
            progress,
        }))
    }
}

fn classify_observation(
    observation: &ObservationSnapshot,
    root: &deltamod_product_contracts::RootIdentity,
    claim: &deltamod_product_contracts::FileClaim,
) -> FileFinding {
    let trusted_binding = observation.validate().is_ok()
        && &observation.root_identity == root
        && observation.path == claim.path
        && observation.path_identity_key == claim.path_identity_key;
    if !trusted_binding {
        return FileFinding {
            kind: FindingKind::ExternalChange,
            issue: Some(VerificationIssue {
                path: claim.path.clone(),
                code: ISSUE_EXTERNAL_CHANGE.into(),
                expected_sha256: Some(claim.sha256.clone()),
                actual_sha256: None,
            }),
        };
    }
    match &observation.state {
        ObservedFileState::Missing => FileFinding {
            kind: FindingKind::Missing,
            issue: Some(VerificationIssue {
                path: claim.path.clone(),
                code: ISSUE_MISSING_FILE.into(),
                expected_sha256: Some(claim.sha256.clone()),
                actual_sha256: None,
            }),
        },
        ObservedFileState::Regular {
            sha256,
            link_count: 1,
            ..
        } if sha256.eq_ignore_ascii_case(&claim.sha256) => FileFinding {
            kind: FindingKind::Healthy,
            issue: None,
        },
        ObservedFileState::Regular {
            sha256,
            link_count: 1,
            ..
        } => FileFinding {
            kind: FindingKind::HashMismatch,
            issue: Some(VerificationIssue {
                path: claim.path.clone(),
                code: ISSUE_HASH_MISMATCH.into(),
                expected_sha256: Some(claim.sha256.clone()),
                actual_sha256: Some(sha256.clone()),
            }),
        },
        ObservedFileState::Regular { sha256, .. } => FileFinding {
            kind: FindingKind::ExternalChange,
            issue: Some(VerificationIssue {
                path: claim.path.clone(),
                code: ISSUE_EXTERNAL_CHANGE.into(),
                expected_sha256: Some(claim.sha256.clone()),
                actual_sha256: Some(sha256.clone()),
            }),
        },
        ObservedFileState::NonRegular | ObservedFileState::Unreadable => FileFinding {
            kind: FindingKind::ExternalChange,
            issue: Some(VerificationIssue {
                path: claim.path.clone(),
                code: ISSUE_EXTERNAL_CHANGE.into(),
                expected_sha256: Some(claim.sha256.clone()),
                actual_sha256: None,
            }),
        },
    }
}

fn resolve_exact_source(
    record: &InstalledModRecord,
    sources: &dyn RepairSourceCatalog,
) -> Option<RepairSource> {
    if let Some(expected_hash) = record.archive_sha256.as_deref() {
        if let Some(source) = sources.cached_archive(expected_hash) {
            if valid_id(&source.source_id, 256)
                && valid_sha256(&source.archive_sha256)
                && source.archive_sha256.eq_ignore_ascii_case(expected_hash)
            {
                return Some(RepairSource::CachedArchive(source));
            }
        }
    }
    let source = sources.exact_provider_artifact(&record.provider)?;
    if !valid_id(&source.source_id, 256)
        || !exact_provider_artifact_identity(&source.provider, &record.provider)
        || source
            .archive_sha256
            .as_deref()
            .is_some_and(|hash| !valid_sha256(hash))
        || record.archive_sha256.as_deref().is_some_and(|expected| {
            source
                .archive_sha256
                .as_deref()
                .is_none_or(|actual| !actual.eq_ignore_ascii_case(expected))
        })
    {
        return None;
    }
    Some(RepairSource::ProviderArtifact(source))
}

/// Compares the immutable provider resource/artifact/version tuple and ignores
/// canonical URLs, which are navigation metadata rather than artifact identity.
#[must_use]
pub fn exact_provider_artifact_identity(left: &ProviderRef, right: &ProviderRef) -> bool {
    let pinned = left.artifact_kind() != ProviderArtifactKind::Unknown
        && (left.artifact_id().is_some() || left.version_id().is_some())
        && right.artifact_kind() != ProviderArtifactKind::Unknown
        && (right.artifact_id().is_some() || right.version_id().is_some());
    pinned && same_provider_artifact_tuple(left, right)
}

fn same_provider_artifact_tuple(left: &ProviderRef, right: &ProviderRef) -> bool {
    left.provider_id() == right.provider_id()
        && left.item_kind() == right.item_kind()
        && left.resource_id() == right.resource_id()
        && left.scope() == right.scope()
        && left.artifact_id() == right.artifact_id()
        && left.artifact_kind() == right.artifact_kind()
        && left.version_id() == right.version_id()
}

fn optional_hash_eq(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn progress_counts(
    operation: &OperationRecord,
    journal: Option<&LifecycleJournal>,
) -> (u64, Option<u64>, Option<String>) {
    let Some(journal) = journal else {
        return (0, operation.state.terminal().then_some(0), None);
    };
    let total = u64::try_from(journal.mutations.len()).unwrap_or(u64::MAX);
    let completed = if operation.state.terminal() {
        total
    } else {
        u64::try_from(
            journal
                .mutations
                .iter()
                .filter(|mutation| {
                    matches!(
                        mutation.checkpoint,
                        deltamod_product_contracts::MutationCheckpoint::OutputVerified
                            | deltamod_product_contracts::MutationCheckpoint::NoEffect
                            | deltamod_product_contracts::MutationCheckpoint::RolledBack
                    )
                })
                .count(),
        )
        .unwrap_or(u64::MAX)
    };
    let current_item = journal
        .mutations
        .iter()
        .find(|mutation| {
            !matches!(
                mutation.checkpoint,
                deltamod_product_contracts::MutationCheckpoint::OutputVerified
                    | deltamod_product_contracts::MutationCheckpoint::NoEffect
                    | deltamod_product_contracts::MutationCheckpoint::RolledBack
            )
        })
        .map(|mutation| mutation.path.as_str())
        .filter(|path| path.len() <= 512)
        .map(str::to_owned);
    (completed, Some(total), current_item)
}

fn maintenance_invalid(operation_id: Option<&str>) -> MaintenanceFailure {
    maintenance_failure(
        ProductErrorCode::InvalidRequest,
        "lifecycle.invalid_request",
        operation_id,
        Some(OperationPhase::Preflight),
        false,
        RecoveryAction::NoAction,
        BTreeMap::new(),
        None,
    )
}

fn maintenance_internal(operation_id: Option<&str>) -> MaintenanceFailure {
    maintenance_failure(
        ProductErrorCode::Internal,
        "lifecycle.maintenance_failed",
        operation_id,
        None,
        false,
        RecoveryAction::NoAction,
        BTreeMap::new(),
        None,
    )
}

fn maintenance_store_failure(error: StoreError, operation_id: Option<&str>) -> MaintenanceFailure {
    let code = match error {
        StoreError::IdempotencyConflict => ProductErrorCode::IdempotencyConflict,
        StoreError::InstallationBusy => ProductErrorCode::InstallationBusy,
        StoreError::LostLease => ProductErrorCode::LostOperationLease,
        StoreError::StaleRevision | StoreError::StaleJournal => {
            ProductErrorCode::StaleOperationRevision
        }
        StoreError::SequenceExhausted => ProductErrorCode::RecoveryUnavailable,
        StoreError::ManifestClaimChanged(ref boundary) => filesystem_error_code(boundary),
        _ => ProductErrorCode::Internal,
    };
    maintenance_failure(
        code,
        "lifecycle.maintenance_failed",
        operation_id,
        None,
        false,
        RecoveryAction::NoAction,
        BTreeMap::new(),
        None,
    )
}

fn filesystem_error_code(
    error: &deltamod_product_contracts::FilesystemBoundaryError,
) -> ProductErrorCode {
    use deltamod_product_contracts::FilesystemBoundaryError;
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

#[allow(clippy::too_many_arguments)]
fn maintenance_failure(
    code: ProductErrorCode,
    message_key: &str,
    operation_id: Option<&str>,
    phase: Option<OperationPhase>,
    retryable: bool,
    recovery_action: RecoveryAction,
    safe_details: BTreeMap<String, String>,
    report: Option<MaintenanceReport>,
) -> MaintenanceFailure {
    let error = ProductError::new(ProductErrorPayload {
        code,
        message_key: message_key.into(),
        operation_id: operation_id.map(str::to_owned),
        phase,
        retryable,
        recovery_action,
        safe_details,
    })
    .expect("maintenance product errors are statically valid");
    MaintenanceFailure {
        error,
        report: report.map(Box::new),
    }
}
