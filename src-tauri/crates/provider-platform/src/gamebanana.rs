//! Pure mapping for the existing GameBanana Subfeed/ProfilePage/UI-config payloads.

use crate::{
    model::{
        decode_json, decode_json_with_array_limits, normalized_version_alias,
        positive_numeric_alias, provider_ref, string_value, version_contract_id, ContentRating,
        DownloadResolution, JsonArrayLimit, ProviderAccount, ProviderDetails, ProviderItem,
        ProviderItemInput, ProviderPage, ProviderVersion, ProviderVersionInput, SearchRequest,
        MAX_RESULT_ITEMS, MAX_VERSION_ENTRIES,
    },
    url_policy::{canonical_public_url, EphemeralDownloadUrl},
    KnownProvider, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{
    ProviderAccountState, ProviderArtifactKind, ProviderItemKind, ProviderRef,
};
use serde_json::Value;
use url::Url;

const PROVIDER: KnownProvider = KnownProvider::GameBanana;
const GAMEBANANA_ROOTS: &[&str] = &["gamebanana.com"];
const DELTAMOD_TOOL_ID: u64 = 20_575;
const MAX_INTEGRATION_ENTRIES: usize = 64;
const SEARCH_ARRAY_LIMITS: &[JsonArrayLimit] = &[
    JsonArrayLimit::new(&[], MAX_RESULT_ITEMS),
    JsonArrayLimit::new(&["_aRecords"], MAX_RESULT_ITEMS),
];
const DETAIL_ARRAY_LIMITS: &[JsonArrayLimit] = &[
    JsonArrayLimit::new(&["_aFiles"], MAX_VERSION_ENTRIES),
    JsonArrayLimit::new(
        &["_aFiles", "_aModManagerIntegrations"],
        MAX_INTEGRATION_ENTRIES,
    ),
];

pub fn map_search(payload: &[u8], request: &SearchRequest) -> ProviderResult<ProviderPage> {
    request.validate_for(PROVIDER)?;
    if request
        .scope()
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .is_none()
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let value: Value = decode_json_with_array_limits(PROVIDER, payload, SEARCH_ARRAY_LIMITS)?;
    let (records, complete, total_count) = if let Some(records) = value.as_array() {
        (records, true, Some(records.len() as u64))
    } else {
        let records = value
            .get("_aRecords")
            .and_then(Value::as_array)
            .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
        let complete = value
            .pointer("/_aMetadata/_bIsComplete")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let total_count = value
            .pointer("/_aMetadata/_nRecordCount")
            .and_then(Value::as_u64);
        (records, complete, total_count)
    };

    let needle = request.query().to_lowercase();
    let mut items = Vec::new();
    for record in records {
        let Some(item) = map_submission(record, None, None)? else {
            continue;
        };
        if !needle.is_empty() {
            let haystack = format!(
                "{} {} {}",
                item.title(),
                item.summary().unwrap_or_default(),
                item.author().unwrap_or_default()
            )
            .to_lowercase();
            if !haystack.contains(&needle) {
                continue;
            }
        }
        items.push(item);
        if items.len() >= request.limit() as usize {
            break;
        }
    }
    ProviderPage::new(
        PROVIDER,
        request,
        items,
        !complete,
        total_count,
        Some("Metadata provided by GameBanana".into()),
    )
}

pub fn map_details(payload: &[u8]) -> ProviderResult<ProviderDetails> {
    let value: Value = decode_json_with_array_limits(PROVIDER, payload, DETAIL_ARRAY_LIMITS)?;
    let item = map_submission(&value, None, None)?
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let files = value
        .get("_aFiles")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if files.len() > MAX_VERSION_ENTRIES {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let mut versions = Vec::with_capacity(files.len());
    for file in files {
        versions.push(map_file(&item, file)?);
    }
    ProviderDetails::new(PROVIDER, item, versions)
}

pub fn resolve_download(
    detail_payload: &[u8],
    selected: &ProviderRef,
) -> ProviderResult<DownloadResolution> {
    let value: Value =
        decode_json_with_array_limits(PROVIDER, detail_payload, DETAIL_ARRAY_LIMITS)?;
    let item = map_submission(&value, None, None)?
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    if selected.provider_id().as_str() != PROVIDER.as_str()
        || selected.canonical_identity() != item.source().canonical_identity()
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let artifact_id = selected
        .artifact_id()
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?
        .as_str();
    let files = value
        .get("_aFiles")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    if files.len() > MAX_VERSION_ENTRIES {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let mut selected_file = None;
    for file in files {
        let file_id = positive_numeric_alias(file, &["_idRow", "id"], PROVIDER)?;
        if file_id.as_deref() == Some(artifact_id) {
            selected_file = Some(file);
            break;
        }
    }
    let file = selected_file.ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    let version = map_file(&item, file)?;
    if !version.directly_downloadable() {
        return Err(ProviderFailure::unsupported(PROVIDER));
    }
    let raw_url = file
        .get("_sDownloadUrl")
        .or_else(|| file.get("downloadUrl"))
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let mut url = Url::parse(raw_url).map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    if url.path().starts_with("/dl/") {
        let path = url.path().replacen("/dl/", "/mmdl/", 1);
        url.set_path(&path);
    }
    let url = EphemeralDownloadUrl::from_url(url, GAMEBANANA_ROOTS)
        .map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    DownloadResolution::new(
        PROVIDER,
        version.source().clone(),
        url,
        version.file_name().unwrap_or(version.label()),
        version.size_bytes(),
        version.sha256().map(str::to_owned),
    )
}

pub fn map_account(payload: &[u8]) -> ProviderResult<ProviderAccount> {
    let value: Value = decode_json(PROVIDER, payload)?;
    let id = value
        .get("_idMemberRow")
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(value) => value.parse().ok(),
            _ => None,
        })
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    if id == 0 {
        return ProviderAccount::new(
            PROVIDER,
            ProviderAccountState::SignedOut,
            None,
            None,
            None,
            None,
        );
    }
    let name = value
        .get("_sName")
        .or_else(|| value.get("_sUsername"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    ProviderAccount::new(
        PROVIDER,
        ProviderAccountState::SignedIn,
        name,
        Some(id.to_string()),
        None,
        None,
    )
}

fn map_submission(
    value: &Value,
    artifact_id: Option<&str>,
    version_id: Option<&str>,
) -> ProviderResult<Option<ProviderItem>> {
    let id = positive_numeric_alias(value, &["_idRow", "id"], PROVIDER)?
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let model = submission_model_alias(value)?;
    let (scope, plural) = if model.eq_ignore_ascii_case("mod") {
        ("mod", "mods")
    } else if model.eq_ignore_ascii_case("wip") {
        if value.get("_bHasFiles").and_then(Value::as_bool) == Some(false) {
            return Ok(None);
        }
        ("wip", "wips")
    } else {
        return Ok(None);
    };
    let canonical_url = canonical_public_url(
        &format!("https://gamebanana.com/{plural}/{id}"),
        GAMEBANANA_ROOTS,
    )
    .map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    let source = provider_ref(
        PROVIDER,
        ProviderItemKind::Mod,
        &id,
        Some(scope),
        artifact_id,
        if artifact_id.is_some() {
            ProviderArtifactKind::File
        } else {
            ProviderArtifactKind::Unknown
        },
        version_id,
        Some(canonical_url),
    )?;
    let title = value
        .get("_sName")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?
        .to_owned();
    let summary = value
        .get("_sDescription")
        .or_else(|| value.get("description"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let author = value
        .pointer("/_aSubmitter/_sName")
        .or_else(|| value.get("author"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| Some("GameBanana contributor".into()));
    let updated = ["_tsDateModified", "_tsDateAdded"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_u64))
        .max()
        .map(|timestamp| timestamp.to_string());
    let content_rating = content_rating(value);
    let featured = value.get("featuredDataset").and_then(Value::as_bool) == Some(true)
        || value.get("_sPeriod").and_then(Value::as_str).is_some();
    ProviderItem::from_input(
        PROVIDER,
        ProviderItemInput {
            source,
            title,
            summary,
            author,
            updated_at: updated,
            content_rating,
            downloads: value
                .get("_nDownloadCount")
                .or_else(|| value.get("downloads"))
                .and_then(Value::as_u64),
            endorsements: None,
            featured,
        },
    )
    .map(Some)
}

fn submission_model_alias(value: &Value) -> ProviderResult<&str> {
    let mut model = None;
    for key in ["_sModelName", "model"] {
        if let Some(raw) = value.get(key) {
            let candidate = raw
                .as_str()
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
            if model.is_some_and(|existing: &str| !existing.eq_ignore_ascii_case(candidate)) {
                return Err(ProviderFailure::invalid_payload(PROVIDER));
            }
            model = Some(candidate);
        }
    }
    model.ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))
}

fn map_file(item: &ProviderItem, file: &Value) -> ProviderResult<ProviderVersion> {
    let file_id = positive_numeric_alias(file, &["_idRow", "id"], PROVIDER)?
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let version = normalized_version_alias(file, &["_sVersion", "version"], PROVIDER)?;
    let version_id = version_contract_id(version.as_deref(), &file_id);
    let source = provider_ref(
        PROVIDER,
        ProviderItemKind::Mod,
        item.source().resource_id().as_str(),
        item.source().scope().map(|scope| scope.as_str()),
        Some(&file_id),
        ProviderArtifactKind::File,
        Some(&version_id),
        item.source().canonical_url().map(str::to_owned),
    )?;
    let file_name = file
        .get("_sFile")
        .or_else(|| file.get("fileName"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let label = version
        .or_else(|| file_name.clone())
        .or_else(|| {
            file.get("_sName")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format!("File {file_id}"));
    let integrations = file
        .get("_aModManagerIntegrations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if integrations.len() > MAX_INTEGRATION_ENTRIES {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let integrated = integrations.iter().any(|integration| {
        integration.get("_idToolRow").and_then(Value::as_u64) == Some(DELTAMOD_TOOL_ID)
    });
    let has_download_url = file
        .get("_sDownloadUrl")
        .or_else(|| file.get("downloadUrl"))
        .and_then(Value::as_str)
        .is_some();
    let sha256 = ["_sSha256Checksum", "sha256", "sha256Checksum"]
        .into_iter()
        .find_map(|key| string_value(file.get(key)));
    ProviderVersion::from_input(
        PROVIDER,
        ProviderVersionInput {
            source,
            label,
            file_name,
            size_bytes: file
                .get("_nFilesize")
                .or_else(|| file.get("size"))
                .and_then(Value::as_u64),
            published_at: file
                .get("_tsDateAdded")
                .and_then(Value::as_u64)
                .map(|timestamp| timestamp.to_string()),
            sha256,
            primary: file.get("_bIsPrimary").and_then(Value::as_bool) == Some(true),
            directly_downloadable: integrated && has_download_url,
        },
    )
}

fn content_rating(value: &Value) -> ContentRating {
    let adult = value
        .get("_aContentRatings")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|ratings| ratings.values())
        .filter_map(Value::as_str)
        .any(|rating| {
            let rating = rating.to_ascii_lowercase();
            rating.contains("adult") || rating.contains("mature")
        });
    if adult {
        ContentRating::Adult
    } else if value.get("_bHasContentRatings").and_then(Value::as_bool) == Some(true) {
        ContentRating::Unknown
    } else {
        ContentRating::General
    }
}
