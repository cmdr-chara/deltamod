use crate::*;
use std::collections::{BTreeMap, BTreeSet};

pub const FIXTURE_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const FIXTURE_SHA256_B: &str =
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub const FIXTURE_SHA256_C: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn path(value: &str) -> ValidatedRelativePath {
    ValidatedRelativePath::parse(value).expect("fixture path")
}

pub fn provider_ref() -> ProviderRef {
    ProviderRef::new(
        ProviderId::parse("gamebanana").expect("fixture provider"),
        ProviderItemKind::Mod,
        ProviderResourceId::parse("1234").expect("fixture resource"),
        None,
        Some(ProviderResourceId::parse("5678").expect("fixture artifact")),
        ProviderArtifactKind::File,
        Some(ProviderResourceId::parse("1.0.0").expect("fixture version")),
        Some("https://gamebanana.com/mods/1234".into()),
    )
    .expect("fixture reference")
}

#[must_use]
pub fn installed_mod_record() -> InstalledModRecord {
    InstalledModRecord::new(InstalledModPayload {
        instance_id: "fixture-instance".into(),
        mod_id: "fixture-mod".into(),
        installation_id: "fixture-installation".into(),
        display_name: "Fixture Mod".into(),
        version: Some("1.0.0".into()),
        provider: provider_ref(),
        archive_sha256: Some(FIXTURE_SHA256.into()),
        file_plan_fingerprint: FIXTURE_SHA256.into(),
        manifest_generation: 1,
        installed_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_000_000,
        files: vec![InstalledFileRef {
            path: path("mods/fixture.dat"),
            path_identity_key: "mods/fixture.dat".into(),
            expected_sha256: FIXTURE_SHA256.into(),
        }],
    })
    .expect("installed mod fixture")
}

#[must_use]
pub fn claims_ledger() -> InstallationClaimsLedger {
    InstallationClaimsLedger::new(InstallationClaimsLedgerPayload {
        installation_id: "fixture-installation".into(),
        manifest_generation: 1,
        updated_at_ms: 1_700_000_000_000,
        claims: vec![FileClaim {
            path: path("mods/fixture.dat"),
            path_identity_key: "mods/fixture.dat".into(),
            sha256: FIXTURE_SHA256.into(),
            owners: BTreeSet::from(["fixture-instance".into()]),
        }],
    })
    .expect("claims fixture")
}

pub fn root_identity(marker: &str) -> RootIdentity {
    let canonical_path_sha256 = match marker {
        "transaction" => FIXTURE_SHA256,
        "staging" => FIXTURE_SHA256_B,
        "backup" => FIXTURE_SHA256_C,
        _ => FIXTURE_SHA256,
    };
    RootIdentity {
        canonical_path_sha256: canonical_path_sha256.into(),
        volume_id: format!("volume-{marker}"),
        file_id: format!("file-{marker}"),
    }
}

pub fn operation_intent() -> OperationIntent {
    OperationIntent {
        installation_id: "fixture-installation".into(),
        kind: LifecycleOperationKind::Install,
        mod_instance_id: Some("fixture-instance".into()),
        provider: Some(provider_ref()),
        archive_sha256: Some(FIXTURE_SHA256.into()),
        file_plan_fingerprint: Some(FIXTURE_SHA256.into()),
        profile_id: None,
    }
}

pub fn operation_request() -> OperationRequest {
    OperationRequest::new("operation-1", "request-1", operation_intent())
        .expect("operation request fixture")
}

pub fn operation_lease() -> OperationLease {
    OperationLease {
        installation_id: "fixture-installation".into(),
        operation_id: "operation-1".into(),
        lease_id: "lease-1".into(),
        owner_instance_id: "runtime-1".into(),
        fencing_token: 1,
        acquired_at_ms: 1_700_000_000_000,
        expires_at_ms: 1_700_000_001_000,
    }
}

#[must_use]
pub fn lifecycle_journal() -> LifecycleJournal {
    LifecycleJournal::new(LifecycleJournalPayload {
        journal_sequence: 7,
        operation_id: "operation-1".into(),
        idempotency_key: "request-1".into(),
        request_fingerprint: operation_intent().fingerprint(),
        lease_id: "lease-1".into(),
        operation_revision: 2,
        fencing_token: 1,
        installation_id: "fixture-installation".into(),
        operation: LifecycleOperationKind::Install,
        phase: OperationPhase::Complete,
        transaction_root: root_identity("transaction"),
        staging_root: root_identity("staging"),
        backup_root: root_identity("backup"),
        recovery_generation_id: "recovery-1".into(),
        recovery_chain_sha256: FIXTURE_SHA256_C.into(),
        manifest_generation_before: 1,
        manifest_generation_after: 2,
        manifest_commit_state: ManifestCommitState::Published,
        started_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_000_100,
        mutations: vec![JournalMutation {
            index: 0,
            path: path("mods/fixture.dat"),
            path_identity_key: "mods/fixture.dat".into(),
            action: MutationAction::Replace,
            checkpoint: MutationCheckpoint::OutputVerified,
            previous_sha256: Some(FIXTURE_SHA256.into()),
            expected_sha256: Some(FIXTURE_SHA256_B.into()),
            staging_path: Some(path("staged/fixture.dat")),
            staging_sha256: Some(FIXTURE_SHA256_B.into()),
            backup_path: Some(path("backups/fixture.dat")),
            backup_sha256: Some(FIXTURE_SHA256.into()),
        }],
        recovery_attempts: 0,
        pinned: false,
    })
    .expect("journal fixture")
}

#[must_use]
pub fn conflict_report() -> ConflictReport {
    ConflictReport::new(ConflictReportPayload {
        installation_id: "fixture-installation".into(),
        conflicts: vec![FileConflict {
            path: path("mods/fixture.dat"),
            path_identity_key: "mods/fixture.dat".into(),
            reason: ConflictReason::DifferentContent,
            expected_sha256: Some(FIXTURE_SHA256.into()),
            actual_sha256: Some(FIXTURE_SHA256.into()),
            proposed_sha256: Some(FIXTURE_SHA256_B.into()),
            existing_owners: BTreeSet::from(["fixture-instance".into()]),
            requesting_owner: "other-instance".into(),
        }],
    })
    .expect("conflict fixture")
}

#[must_use]
pub fn verification_result() -> VerificationResult {
    VerificationResult::new(VerificationResultPayload {
        installation_id: "fixture-installation".into(),
        mod_instance_id: Some("fixture-instance".into()),
        state: VerificationState::Healthy,
        checked_files: 1,
        issues: Vec::new(),
        verified_at_ms: 1_700_000_000_200,
    })
    .expect("verification fixture")
}

#[must_use]
pub fn game_health_report() -> GameHealthReport {
    GameHealthReport::new(GameHealthReportPayload {
        installation_id: "fixture-installation".into(),
        state: GameHealthState::Healthy,
        lifecycle_owned_files: 1,
        unknown_modified_files: 0,
        interrupted_operations: Vec::new(),
        checked_at_ms: 1_700_000_000_300,
    })
    .expect("game health fixture")
}

#[must_use]
pub fn operation_record() -> OperationRecord {
    OperationRecord::new(OperationRecordPayload {
        request: operation_request(),
        state: OperationState::Succeeded,
        phase: OperationPhase::Complete,
        revision: 2,
        fencing_token: 1,
        created_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_000_400,
        result_fingerprint: Some(FIXTURE_SHA256_B.into()),
        error: None,
    })
    .expect("operation record fixture")
}

#[must_use]
pub fn operation_progress() -> OperationProgress {
    OperationProgress::new(OperationProgressPayload {
        operation_id: "operation-1".into(),
        installation_id: "fixture-installation".into(),
        kind: LifecycleOperationKind::Install,
        state: OperationState::Succeeded,
        phase: OperationPhase::Complete,
        completed: 1,
        total: Some(1),
        cancellable: false,
        current_item: None,
        updated_at_ms: 1_700_000_000_400,
    })
    .expect("operation progress fixture")
}

#[must_use]
pub fn provider_descriptor() -> ProviderDescriptor {
    ProviderDescriptor::new(ProviderDescriptorPayload {
        provider_id: ProviderId::parse("gamebanana").expect("fixture provider"),
        display_name: "GameBanana".into(),
        capabilities: BTreeSet::from([
            ProviderCapability::Search,
            ProviderCapability::Details,
            ProviderCapability::DirectDownload,
        ]),
        authentication: ProviderAuthentication::Optional,
    })
    .expect("provider descriptor fixture")
}

#[must_use]
pub fn product_error() -> ProductError {
    ProductError::new(ProductErrorPayload {
        code: ProductErrorCode::InstallationBusy,
        message_key: "operation.installation_busy".into(),
        operation_id: Some("operation-1".into()),
        phase: Some(OperationPhase::Preflight),
        retryable: false,
        recovery_action: RecoveryAction::NoAction,
        safe_details: BTreeMap::from([("activeOperation".into(), "operation-0".into())]),
    })
    .expect("product error fixture")
}

#[must_use]
pub fn retention_decision() -> RetentionDecision {
    RetentionDecision::new(RetentionDecisionPayload {
        limit_bytes: 12,
        bytes_before: 16,
        bytes_after: 12,
        keep_generation_ids: vec!["g2".into(), "g3".into(), "g4".into()],
        evict_generation_ids: vec!["g1".into()],
        over_limit: false,
    })
    .expect("retention fixture")
}
