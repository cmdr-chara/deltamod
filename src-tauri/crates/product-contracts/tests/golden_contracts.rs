use deltamod_product_contracts::{
    decode_current, fixtures, ConflictReport, ContractDocument, GameHealthReport,
    InstallationClaimsLedger, InstalledModRecord, LifecycleJournal, OperationProgress,
    OperationRecord, ProductError, ProviderDescriptor, RetentionDecision, SchemaError,
    VerificationResult,
};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Debug;

fn bundle() -> Value {
    serde_json::from_slice(include_bytes!("fixtures/contracts-v1.json")).unwrap()
}

fn value(key: &str) -> Value {
    bundle().get(key).cloned().expect("golden fixture key")
}

fn assert_golden<T>(key: &str, expected: T)
where
    T: ContractDocument + Serialize + Debug + Eq,
{
    let fixture = value(key);
    let bytes = serde_json::to_vec(&fixture).unwrap();
    let decoded: T = decode_current(&bytes).unwrap();
    assert_eq!(decoded, expected);
    assert_eq!(serde_json::to_value(decoded).unwrap(), fixture);
}

fn assert_future_rejected<T: ContractDocument>(key: &str) {
    let mut fixture = value(key);
    fixture["schemaVersion"] = Value::from(999);
    assert!(matches!(
        decode_current::<T>(&serde_json::to_vec(&fixture).unwrap()),
        Err(SchemaError::FutureVersion {
            found: 999,
            supported: 1
        })
    ));
}

#[test]
fn every_frozen_contract_matches_its_golden_fixture() {
    assert_golden("installedMod", fixtures::installed_mod_record());
    assert_golden("claimsLedger", fixtures::claims_ledger());
    assert_golden("lifecycleJournal", fixtures::lifecycle_journal());
    assert_golden("conflictReport", fixtures::conflict_report());
    assert_golden("verificationResult", fixtures::verification_result());
    assert_golden("gameHealthReport", fixtures::game_health_report());
    assert_golden("operationProgress", fixtures::operation_progress());
    assert_golden("operationRecord", fixtures::operation_record());
    assert_golden("providerDescriptor", fixtures::provider_descriptor());
    assert_golden("productError", fixtures::product_error());
    assert_golden("retentionDecision", fixtures::retention_decision());
}

#[test]
fn every_frozen_contract_rejects_forward_versions() {
    assert_future_rejected::<InstalledModRecord>("installedMod");
    assert_future_rejected::<InstallationClaimsLedger>("claimsLedger");
    assert_future_rejected::<LifecycleJournal>("lifecycleJournal");
    assert_future_rejected::<ConflictReport>("conflictReport");
    assert_future_rejected::<VerificationResult>("verificationResult");
    assert_future_rejected::<GameHealthReport>("gameHealthReport");
    assert_future_rejected::<OperationProgress>("operationProgress");
    assert_future_rejected::<OperationRecord>("operationRecord");
    assert_future_rejected::<ProviderDescriptor>("providerDescriptor");
    assert_future_rejected::<ProductError>("productError");
    assert_future_rejected::<RetentionDecision>("retentionDecision");
}

#[test]
fn durable_lifecycle_paths_cannot_bypass_validation() {
    for (key, pointer) in [
        ("installedMod", "/files/0/path"),
        ("claimsLedger", "/claims/0/path"),
        ("lifecycleJournal", "/mutations/0/path"),
        ("conflictReport", "/conflicts/0/path"),
    ] {
        let mut fixture = value(key);
        *fixture.pointer_mut(pointer).unwrap() = Value::from("%252e%252e/outside.dat");
        let bytes = serde_json::to_vec(&fixture).unwrap();
        let rejected = match key {
            "installedMod" => decode_current::<InstalledModRecord>(&bytes).is_err(),
            "claimsLedger" => decode_current::<InstallationClaimsLedger>(&bytes).is_err(),
            "lifecycleJournal" => decode_current::<LifecycleJournal>(&bytes).is_err(),
            "conflictReport" => decode_current::<ConflictReport>(&bytes).is_err(),
            _ => false,
        };
        assert!(rejected, "{key} accepted unsafe path");
    }
}

#[test]
fn contradictory_and_incomplete_documents_fail_validation() {
    let mut journal = value("lifecycleJournal");
    journal["mutations"][0]["checkpoint"] = Value::from("applied");
    assert!(decode_current::<LifecycleJournal>(&serde_json::to_vec(&journal).unwrap()).is_err());

    let mut health = value("gameHealthReport");
    health["unknownModifiedFiles"] = Value::from(1);
    assert!(decode_current::<GameHealthReport>(&serde_json::to_vec(&health).unwrap()).is_err());
}

#[test]
fn persisted_json_rejects_unknown_fields_and_duplicate_keys() {
    let mut unknown = value("installedMod");
    unknown["unexpectedField"] = Value::from(true);
    let unknown_bytes = serde_json::to_vec(&unknown).unwrap();
    assert!(decode_current::<InstalledModRecord>(&unknown_bytes).is_err());
    assert!(serde_json::from_value::<InstalledModRecord>(unknown).is_err());

    let json = serde_json::to_string(&value("installedMod")).unwrap();
    let duplicate_version = json.replacen(
        "\"schemaVersion\":1",
        "\"schemaVersion\":999,\"schemaVersion\":1",
        1,
    );
    assert!(decode_current::<InstalledModRecord>(duplicate_version.as_bytes()).is_err());

    let duplicate_nested = json.replacen(
        "\"resourceId\":\"1234\"",
        "\"resourceId\":\"other\",\"resourceId\":\"1234\"",
        1,
    );
    assert!(decode_current::<InstalledModRecord>(duplicate_nested.as_bytes()).is_err());
}
