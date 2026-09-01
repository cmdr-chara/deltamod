use serde::{
    de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize,
};
use serde_json::Value;
use std::{fmt, ops::Deref};
use thiserror::Error;

pub const MAX_CONTRACT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    InstalledMod,
    ClaimsLedger,
    LifecycleJournal,
    ConflictReport,
    VerificationResult,
    GameHealthReport,
    OperationProgress,
    OperationRecord,
    ProviderDescriptor,
    ProductError,
    RetentionDecision,
}

impl ContractKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InstalledMod => "installed_mod",
            Self::ClaimsLedger => "claims_ledger",
            Self::LifecycleJournal => "lifecycle_journal",
            Self::ConflictReport => "conflict_report",
            Self::VerificationResult => "verification_result",
            Self::GameHealthReport => "game_health_report",
            Self::OperationProgress => "operation_progress",
            Self::OperationRecord => "operation_record",
            Self::ProviderDescriptor => "provider_descriptor",
            Self::ProductError => "product_error",
            Self::RetentionDecision => "retention_decision",
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SchemaError {
    #[error("contract payload is empty or too large")]
    Size,
    #[error("contract payload is malformed")]
    Malformed,
    #[error("contract document kind is missing or invalid")]
    MissingKind,
    #[error("expected contract kind {expected}, found {found}")]
    WrongKind {
        expected: &'static str,
        found: String,
    },
    #[error("contract schema version is missing or invalid")]
    MissingVersion,
    #[error("contract schema version {found} is newer than supported version {supported}")]
    FutureVersion { found: u32, supported: u32 },
    #[error("no migration exists from contract schema version {0}")]
    MissingMigration(u32),
    #[error("contract migration did not advance exactly one version")]
    InvalidMigration,
    #[error("contract invariant failed: {0}")]
    InvalidDocument(&'static str),
}

pub(crate) mod private {
    pub trait Sealed {}
}

pub trait ContractPayload: private::Sealed + Clone + DeserializeOwned + Serialize {
    const KIND: ContractKind;
    const VERSION: u32;
    fn validate(&self) -> Result<(), SchemaError>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedContract<T: ContractPayload> {
    document_kind: ContractKind,
    schema_version: u32,
    #[serde(flatten)]
    pub(crate) payload: T,
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictValueVisitor)
    }
}

struct StrictValueVisitor;

impl<'de> Visitor<'de> for StrictValueVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(StrictValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let StrictValue(value) = map.next_value()?;
            if values.insert(key, value).is_some() {
                return Err(de::Error::custom("duplicate JSON object key"));
            }
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

impl<T: ContractPayload> ValidatedContract<T> {
    pub fn new(payload: T) -> Result<Self, SchemaError> {
        payload.validate()?;
        Ok(Self {
            document_kind: T::KIND,
            schema_version: T::VERSION,
            payload,
        })
    }

    #[must_use]
    pub const fn document_kind(&self) -> ContractKind {
        self.document_kind
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn try_update<F>(&mut self, update: F) -> Result<(), SchemaError>
    where
        F: FnOnce(&mut T),
    {
        let mut candidate = self.payload.clone();
        update(&mut candidate);
        candidate.validate()?;
        self.payload = candidate;
        Ok(())
    }

    #[must_use]
    pub fn into_payload(self) -> T {
        self.payload
    }

    fn from_value(mut value: Value) -> Result<Self, SchemaError> {
        let found_kind = kind(&value)?.to_owned();
        let found_version = version(&value)?;
        if found_kind != T::KIND.as_str() || found_version != T::VERSION {
            return Err(SchemaError::InvalidDocument("contract header"));
        }
        let object = value.as_object_mut().ok_or(SchemaError::Malformed)?;
        object.remove("documentKind");
        object.remove("schemaVersion");
        let payload = serde_json::from_value(Value::Object(std::mem::take(object)))
            .map_err(|_| SchemaError::Malformed)?;
        Self::new(payload)
    }
}

impl<T: ContractPayload> Deref for ValidatedContract<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.payload
    }
}

impl<'de, T: ContractPayload> Deserialize<'de> for ValidatedContract<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let StrictValue(value) = StrictValue::deserialize(deserializer)?;
        Self::from_value(value).map_err(de::Error::custom)
    }
}

pub trait ContractDocument: DeserializeOwned {
    const KIND: ContractKind;
    const VERSION: u32;

    fn validate(&self) -> Result<(), SchemaError>;
    fn from_current_value(value: Value) -> Result<Self, SchemaError>;
}

impl<T: ContractPayload> ContractDocument for ValidatedContract<T> {
    const KIND: ContractKind = T::KIND;
    const VERSION: u32 = T::VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if self.document_kind != T::KIND || self.schema_version != T::VERSION {
            return Err(SchemaError::InvalidDocument("contract header"));
        }
        self.payload.validate()
    }

    fn from_current_value(value: Value) -> Result<Self, SchemaError> {
        Self::from_value(value)
    }
}

pub trait MigrationSet {
    fn migrate_one(kind: ContractKind, from: u32, value: Value) -> Result<Value, SchemaError>;
}

pub struct NoMigrations;
impl MigrationSet for NoMigrations {
    fn migrate_one(_kind: ContractKind, from: u32, _value: Value) -> Result<Value, SchemaError> {
        Err(SchemaError::MissingMigration(from))
    }
}

fn version(value: &Value) -> Result<u32, SchemaError> {
    value
        .as_object()
        .and_then(|object| object.get("schemaVersion"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(SchemaError::MissingVersion)
}

fn kind(value: &Value) -> Result<&str, SchemaError> {
    value
        .as_object()
        .and_then(|object| object.get("documentKind"))
        .and_then(Value::as_str)
        .ok_or(SchemaError::MissingKind)
}

pub fn decode_with_migrations<T, M>(bytes: &[u8]) -> Result<T, SchemaError>
where
    T: ContractDocument,
    M: MigrationSet,
{
    if bytes.is_empty() || bytes.len() > MAX_CONTRACT_BYTES {
        return Err(SchemaError::Size);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let StrictValue(mut value) =
        StrictValue::deserialize(&mut deserializer).map_err(|_| SchemaError::Malformed)?;
    deserializer.end().map_err(|_| SchemaError::Malformed)?;
    let found_kind = kind(&value)?;
    if found_kind != T::KIND.as_str() {
        return Err(SchemaError::WrongKind {
            expected: T::KIND.as_str(),
            found: found_kind.to_owned(),
        });
    }
    let found = version(&value)?;
    if found > T::VERSION {
        return Err(SchemaError::FutureVersion {
            found,
            supported: T::VERSION,
        });
    }
    while version(&value)? < T::VERSION {
        let before = version(&value)?;
        value = M::migrate_one(T::KIND, before, value)?;
        if kind(&value)? != T::KIND.as_str() || version(&value)? != before + 1 {
            return Err(SchemaError::InvalidMigration);
        }
    }
    let decoded = T::from_current_value(value)?;
    decoded.validate()?;
    Ok(decoded)
}

pub fn decode_current<T: ContractDocument>(bytes: &[u8]) -> Result<T, SchemaError> {
    decode_with_migrations::<T, NoMigrations>(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Deserialize, Eq, PartialEq)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Example {
        document_kind: ContractKind,
        schema_version: u32,
        name: String,
    }

    impl ContractDocument for Example {
        const KIND: ContractKind = ContractKind::InstalledMod;
        const VERSION: u32 = 1;

        fn validate(&self) -> Result<(), SchemaError> {
            if self.document_kind == Self::KIND
                && self.schema_version == Self::VERSION
                && !self.name.is_empty()
            {
                Ok(())
            } else {
                Err(SchemaError::InvalidDocument("example"))
            }
        }

        fn from_current_value(value: Value) -> Result<Self, SchemaError> {
            serde_json::from_value(value).map_err(|_| SchemaError::Malformed)
        }
    }

    struct ExampleMigrations;
    impl MigrationSet for ExampleMigrations {
        fn migrate_one(
            kind: ContractKind,
            from: u32,
            mut value: Value,
        ) -> Result<Value, SchemaError> {
            if kind != ContractKind::InstalledMod || from != 0 {
                return Err(SchemaError::MissingMigration(from));
            }
            let object = value.as_object_mut().ok_or(SchemaError::Malformed)?;
            let label = object.remove("label").ok_or(SchemaError::Malformed)?;
            object.insert("schemaVersion".into(), json!(1));
            object.insert("name".into(), label);
            Ok(value)
        }
    }

    #[test]
    fn migration_harness_advances_one_version_and_validates() {
        let decoded = decode_with_migrations::<Example, ExampleMigrations>(
            br#"{"documentKind":"installed_mod","schemaVersion":0,"label":"demo"}"#,
        )
        .unwrap();
        assert_eq!(decoded.name, "demo");
    }

    #[test]
    fn future_versions_and_wrong_kinds_fail_closed() {
        assert_eq!(
            decode_current::<Example>(
                br#"{"documentKind":"installed_mod","schemaVersion":2,"name":"future"}"#
            ),
            Err(SchemaError::FutureVersion {
                found: 2,
                supported: 1
            })
        );
        assert!(matches!(
            decode_current::<Example>(
                br#"{"documentKind":"product_error","schemaVersion":1,"name":"wrong"}"#
            ),
            Err(SchemaError::WrongKind { .. })
        ));
    }
}
