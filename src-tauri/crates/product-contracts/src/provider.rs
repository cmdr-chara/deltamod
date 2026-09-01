use crate::{
    ContractKind, ContractPayload, SchemaError, ValidatedContract, PRODUCT_SCHEMA_VERSION,
};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeSet, fmt};
use url::Url;

const SENSITIVE_CANONICAL_QUERY_SUFFIXES: &[&str] = &[
    "token",
    "secret",
    "signature",
    "password",
    "passwd",
    "authorization",
    "credential",
    "apikey",
    "privatekey",
    "gamekey",
    "bearer",
];

fn looks_like_bearer_credential(value: &str) -> bool {
    let mut characters = value.trim_start().chars();
    for expected in "bearer".chars() {
        let Some(character) = characters.find(|character| character.is_alphanumeric()) else {
            return false;
        };
        if !character.is_ascii() || character.to_ascii_lowercase() != expected {
            return false;
        }
    }

    let mut saw_delimiter = false;
    for character in characters {
        if character.is_alphanumeric() {
            return saw_delimiter;
        }
        saw_delimiter = true;
    }
    false
}

fn percent_decode_once(value: &str) -> Result<Option<String>, ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut changed = false;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push(((high << 4) | low) as u8);
                index += 3;
                changed = true;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    if !changed {
        return Ok(None);
    }
    String::from_utf8(decoded).map(Some).map_err(|_| ())
}

fn bearer_value_is_sensitive(value: &str) -> bool {
    let mut candidate = value.to_owned();
    loop {
        if looks_like_bearer_credential(&candidate) {
            return true;
        }
        candidate = match percent_decode_once(&candidate) {
            Ok(Some(next)) => next,
            Ok(None) => return false,
            Err(()) => return true,
        };
    }
}

fn canonical_url_has_sensitive_query(url: &Url) -> bool {
    url.query().is_some_and(|query| {
        query.split('&').any(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let mut candidate = key.to_owned();
            let sensitive_key = loop {
                if !candidate.is_ascii() {
                    break true;
                }
                let normalized_key = candidate
                    .bytes()
                    .filter(u8::is_ascii_alphanumeric)
                    .map(|byte| byte.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                if SENSITIVE_CANONICAL_QUERY_SUFFIXES
                    .iter()
                    .any(|suffix| normalized_key.ends_with(suffix.as_bytes()))
                    || normalized_key == b"key"
                    || normalized_key == b"auth"
                {
                    break true;
                }
                candidate = match percent_decode_once(&candidate) {
                    Ok(Some(next)) => next,
                    Ok(None) => break false,
                    Err(()) => break true,
                };
            };
            if sensitive_key {
                return true;
            }
            bearer_value_is_sensitive(value)
        })
    })
}

fn valid_key(value: &str, max: usize, lowercase: bool) -> bool {
    !value.is_empty()
        && value.len() <= max
        && (!lowercase || value == value.to_ascii_lowercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

macro_rules! validated_key {
    ($name:ident, $max:expr, $lowercase:expr) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: &str) -> Result<Self, SchemaError> {
                if valid_key(value, $max, $lowercase) {
                    Ok(Self(value.to_owned()))
                } else {
                    Err(SchemaError::InvalidDocument(stringify!($name)))
                }
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(&value).map_err(de::Error::custom)
            }
        }
    };
}

validated_key!(ProviderId, 64, true);
validated_key!(ProviderResourceId, 256, false);
validated_key!(ProviderScope, 128, true);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapability {
    Search,
    Details,
    Versions,
    DirectDownload,
    ExternalDownload,
    Authentication,
    UpdateOrdering,
    Images,
    RateLimit,
    ModDiscovery,
    GameAcquisition,
    ProviderInstall,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderItemKind {
    Mod,
    Game,
    LocalArchive,
}

impl ProviderItemKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Game => "game",
            Self::LocalArchive => "local_archive",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderArtifactKind {
    File,
    Build,
    Upload,
    Archive,
    Unknown,
}

impl ProviderArtifactKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Build => "build",
            Self::Upload => "upload",
            Self::Archive => "archive",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRef {
    provider_id: ProviderId,
    item_kind: ProviderItemKind,
    resource_id: ProviderResourceId,
    #[serde(default)]
    scope: Option<ProviderScope>,
    #[serde(default)]
    artifact_id: Option<ProviderResourceId>,
    artifact_kind: ProviderArtifactKind,
    #[serde(default)]
    version_id: Option<ProviderResourceId>,
    #[serde(default)]
    canonical_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawProviderRef {
    provider_id: ProviderId,
    item_kind: ProviderItemKind,
    resource_id: ProviderResourceId,
    #[serde(default)]
    scope: Option<ProviderScope>,
    #[serde(default)]
    artifact_id: Option<ProviderResourceId>,
    artifact_kind: ProviderArtifactKind,
    #[serde(default)]
    version_id: Option<ProviderResourceId>,
    #[serde(default)]
    canonical_url: Option<String>,
}

impl ProviderRef {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider_id: ProviderId,
        item_kind: ProviderItemKind,
        resource_id: ProviderResourceId,
        scope: Option<ProviderScope>,
        artifact_id: Option<ProviderResourceId>,
        artifact_kind: ProviderArtifactKind,
        version_id: Option<ProviderResourceId>,
        canonical_url: Option<String>,
    ) -> Result<Self, SchemaError> {
        let provider = Self {
            provider_id,
            item_kind,
            resource_id,
            scope,
            artifact_id,
            artifact_kind,
            version_id,
            canonical_url,
        };
        provider.validate()?;
        Ok(provider)
    }

    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.canonical_url.as_deref().is_some_and(|value| {
            Url::parse(value).map_or(true, |url| {
                url.scheme() != "https"
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || canonical_url_has_sensitive_query(&url)
                    || url.fragment().is_some()
            })
        }) {
            return Err(SchemaError::InvalidDocument("provider canonical URL"));
        }
        if self.item_kind == ProviderItemKind::LocalArchive && self.provider_id.as_str() != "local"
        {
            return Err(SchemaError::InvalidDocument("local provider identity"));
        }
        Ok(())
    }

    #[must_use]
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    #[must_use]
    pub const fn item_kind(&self) -> ProviderItemKind {
        self.item_kind
    }

    #[must_use]
    pub fn resource_id(&self) -> &ProviderResourceId {
        &self.resource_id
    }

    #[must_use]
    pub fn scope(&self) -> Option<&ProviderScope> {
        self.scope.as_ref()
    }

    #[must_use]
    pub fn artifact_id(&self) -> Option<&ProviderResourceId> {
        self.artifact_id.as_ref()
    }

    #[must_use]
    pub const fn artifact_kind(&self) -> ProviderArtifactKind {
        self.artifact_kind
    }

    #[must_use]
    pub fn version_id(&self) -> Option<&ProviderResourceId> {
        self.version_id.as_ref()
    }

    #[must_use]
    pub fn canonical_url(&self) -> Option<&str> {
        self.canonical_url.as_deref()
    }

    pub fn with_canonical_url(
        mut self,
        canonical_url: Option<String>,
    ) -> Result<Self, SchemaError> {
        self.canonical_url = canonical_url;
        self.validate()?;
        Ok(self)
    }

    /// Stable identity for one logical mod/game. Artifact/version IDs are
    /// intentionally excluded so alternate versions group under one identity.
    #[must_use]
    pub fn canonical_identity(&self) -> String {
        fn part(value: &str) -> String {
            format!("{}:{value}", value.len())
        }
        let scope = self
            .scope
            .as_ref()
            .map_or_else(|| "-".to_owned(), |scope| part(scope.as_str()));
        format!(
            "v1|{}|{}|{scope}|{}",
            part(self.provider_id.as_str()),
            part(self.item_kind.as_str()),
            part(self.resource_id.as_str())
        )
    }
}

impl<'de> Deserialize<'de> for ProviderRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawProviderRef::deserialize(deserializer)?;
        Self::new(
            raw.provider_id,
            raw.item_kind,
            raw.resource_id,
            raw.scope,
            raw.artifact_id,
            raw.artifact_kind,
            raw.version_id,
            raw.canonical_url,
        )
        .map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthentication {
    None,
    Optional,
    Required,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderDescriptorPayload {
    pub provider_id: ProviderId,
    pub display_name: String,
    pub capabilities: BTreeSet<ProviderCapability>,
    pub authentication: ProviderAuthentication,
}

pub type ProviderDescriptor = ValidatedContract<ProviderDescriptorPayload>;

impl crate::schema::private::Sealed for ProviderDescriptorPayload {}
impl ContractPayload for ProviderDescriptorPayload {
    const KIND: ContractKind = ContractKind::ProviderDescriptor;
    const VERSION: u32 = PRODUCT_SCHEMA_VERSION;

    fn validate(&self) -> Result<(), SchemaError> {
        if self.display_name.is_empty()
            || self.display_name.len() > 128
            || self.capabilities.is_empty()
            || (self.authentication == ProviderAuthentication::Required
                && !self
                    .capabilities
                    .contains(&ProviderCapability::Authentication))
        {
            Err(SchemaError::InvalidDocument("provider descriptor"))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccountState {
    NotRequired,
    SignedOut,
    Authorizing,
    SignedIn,
    Expired,
    Unavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_identity_is_stable_and_scope_sensitive() {
        let provider = |scope: Option<&str>| {
            ProviderRef::new(
                ProviderId::parse("nexus").unwrap(),
                ProviderItemKind::Mod,
                ProviderResourceId::parse("42").unwrap(),
                scope.map(|scope| ProviderScope::parse(scope).unwrap()),
                Some(ProviderResourceId::parse("99").unwrap()),
                ProviderArtifactKind::File,
                None,
                Some("https://www.nexusmods.com/deltarune/mods/42".into()),
            )
            .unwrap()
        };
        assert_eq!(
            provider(Some("deltarune")).canonical_identity(),
            "v1|5:nexus|3:mod|9:deltarune|2:42"
        );
        assert_ne!(
            provider(None).canonical_identity(),
            provider(Some("_")).canonical_identity()
        );
        assert_ne!(
            provider(Some("deltarune")).canonical_identity(),
            provider(Some("anothergame")).canonical_identity()
        );
    }

    #[test]
    fn credentials_fragments_and_debug_identity_drift_are_rejected() {
        let json = r#"{
          "providerId":"nexus","itemKind":"mod","resourceId":"42",
          "artifactKind":"file","canonicalUrl":"https://token@example.com/mods/42#x"
        }"#;
        assert!(serde_json::from_str::<ProviderRef>(json).is_err());
        assert!(serde_json::from_str::<ProviderRef>(&json.replace("nexus", "NEXUS")).is_err());
    }

    #[test]
    fn canonical_urls_reject_queries_before_public_serialization() {
        for canonical_url in [
            "https://example.invalid/mods/42?t-o-k-e-n=密钥",
            "https://example.invalid/mods/42?s-e-c-r-e-t=%22!%22",
            "https://example.invalid/mods/42?Authorization=Bearer%20ROUND9",
            "https://example.invalid/mods/42?%74-%6f-%6b-%65-%6e=encoded",
            "https://example.invalid/mods/42?cursor=Bearer%20ROUND9",
            "https://example.invalid/mods/42?cursor=Bearer%09ROUND10",
            "https://example.invalid/mods/42?cursor=Bearer%0AROUND10",
            "https://example.invalid/mods/42?cursor=Bearer%0DROUND10",
            "https://example.invalid/mods/42?cursor=Bearer%C2%A0ROUND10",
            "https://example.invalid/mods/42?cursor=Bearer%E2%80%83ROUND10",
            "https://example.invalid/mods/42?cursor=b-e-a-r-e-r%20ROUND10",
            "https://example.invalid/mods/42?cursor=Bearer%2520ROUND10",
            "https://example.invalid/mods/42?%2574%256f%256b%2565%256e=encoded",
            "https://example.invalid/mods/42?cursor=Bearer%25252520PUBLIC_LEAK",
            "https://example.invalid/mods/42?cursor=%FF",
            "https://example.invalid/mods/42?%FF=value",
        ] {
            let json = serde_json::json!({
                "providerId": "nexus",
                "itemKind": "mod",
                "resourceId": "42",
                "artifactKind": "file",
                "canonicalUrl": canonical_url,
            });
            assert!(serde_json::from_value::<ProviderRef>(json).is_err());
        }

        let benign = ProviderRef::new(
            ProviderId::parse("nexus").unwrap(),
            ProviderItemKind::Mod,
            ProviderResourceId::parse("42").unwrap(),
            None,
            None,
            ProviderArtifactKind::File,
            None,
            Some("https://example.invalid/mods/42?page=2&view=alternate".into()),
        );
        assert!(benign.is_ok());

        for value in ["bearerish", "bearer", "not-bearer", "alternate-bearer-view"] {
            let benign = ProviderRef::new(
                ProviderId::parse("nexus").unwrap(),
                ProviderItemKind::Mod,
                ProviderResourceId::parse("42").unwrap(),
                None,
                None,
                ProviderArtifactKind::File,
                None,
                Some(format!("https://example.invalid/mods/42?view={value}")),
            );
            assert!(benign.is_ok(), "benign value was rejected: {value}");
        }
    }
}
