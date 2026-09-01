//! Pure normalization for user-supplied local archives.

use crate::{
    model::{
        decode_json, normalize_sha256, provider_ref, required_stable_text, safe_file_name,
        version_contract_id, ContentRating, ProviderAccount, ProviderDetails, ProviderItem,
        ProviderItemInput, ProviderSourceRecord, ProviderVersion, ProviderVersionInput,
    },
    text_policy::normalize_version_label,
    KnownProvider, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{ProviderArtifactKind, ProviderItemKind, ProviderRef};
use serde_json::Value;

const PROVIDER: KnownProvider = KnownProvider::Local;

/// Maps trusted archive metadata while using the separately computed archive SHA-256 as the
/// immutable provider/resource identity. A hash claimed inside the archive is never trusted for
/// identity.
pub fn map_archive(payload: &[u8], archive_sha256: &str) -> ProviderResult<ProviderDetails> {
    let archive_sha256 = normalize_sha256(archive_sha256)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let value: Value = decode_json(PROVIDER, payload)?;
    let metadata = value.get("metadata").unwrap_or(&value);
    let name = required_string(metadata, "name", 512)?;
    let version = optional_version(metadata, "version")?;
    let description = optional_string(metadata, "description", 4_096)?;
    let author = optional_string(metadata, "author", 256)?;
    let file_name = optional_file_name(&value, "fileName")?;
    let source = reference(&archive_sha256, version.as_deref())?;
    let item = ProviderItem::from_input(
        PROVIDER,
        ProviderItemInput {
            source: source.clone(),
            title: name.clone(),
            summary: description,
            author,
            updated_at: None,
            content_rating: ContentRating::Unknown,
            downloads: None,
            endorsements: None,
            featured: false,
        },
    )?;
    let version_entry = ProviderVersion::from_input(
        PROVIDER,
        ProviderVersionInput {
            source,
            label: version.clone().unwrap_or_else(|| "Local archive".into()),
            file_name,
            size_bytes: value.get("sizeBytes").and_then(Value::as_u64),
            published_at: None,
            sha256: Some(archive_sha256),
            primary: true,
            directly_downloadable: false,
        },
    )?;
    ProviderDetails::new(PROVIDER, item, vec![version_entry])
}

pub fn reference(archive_sha256: &str, version: Option<&str>) -> ProviderResult<ProviderRef> {
    let hash = normalize_sha256(archive_sha256)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let version = match version {
        Some(value) => Some(
            normalize_version_label(value)
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?,
        ),
        None => None,
    };
    let version_id = version_contract_id(version.as_deref(), &hash);
    provider_ref(
        PROVIDER,
        ProviderItemKind::LocalArchive,
        &hash,
        None,
        Some(&hash),
        ProviderArtifactKind::Archive,
        Some(&version_id),
        None,
    )
}

pub fn source_record(
    archive_sha256: &str,
    version: Option<&str>,
) -> ProviderResult<ProviderSourceRecord> {
    let hash = normalize_sha256(archive_sha256)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let version = match version {
        Some(value) => Some(
            normalize_version_label(value)
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?,
        ),
        None => None,
    };
    ProviderSourceRecord::new(
        PROVIDER,
        reference(&hash, version.as_deref())?,
        Some(hash),
        version,
    )
}

fn required_string(value: &Value, key: &str, maximum: usize) -> ProviderResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| required_stable_text(value, maximum))
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))
}

fn optional_string(value: &Value, key: &str, maximum: usize) -> ProviderResult<Option<String>> {
    match value.get(key) {
        Some(Value::String(value)) => required_stable_text(value, maximum)
            .map(Some)
            .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(ProviderFailure::invalid_payload(PROVIDER)),
    }
}

fn optional_version(value: &Value, key: &str) -> ProviderResult<Option<String>> {
    match value.get(key) {
        Some(Value::String(value)) => normalize_version_label(value)
            .map(Some)
            .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(ProviderFailure::invalid_payload(PROVIDER)),
    }
}

fn optional_file_name(value: &Value, key: &str) -> ProviderResult<Option<String>> {
    match value.get(key) {
        Some(Value::String(value)) => safe_file_name(value, 255)
            .map(Some)
            .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(ProviderFailure::invalid_payload(PROVIDER)),
    }
}

#[must_use]
pub fn account() -> ProviderAccount {
    ProviderAccount::not_required(PROVIDER)
}
