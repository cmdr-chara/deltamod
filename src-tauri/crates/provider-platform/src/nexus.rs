//! Pure mapping for existing Nexus GraphQL/v1/legacy catalogue payloads.

use crate::{
    model::{
        decode_json, decode_json_with_array_limits, normalized_version_alias,
        positive_numeric_alias, provider_ref, string_value, version_contract_id, ContentRating,
        DownloadResolution, JsonArrayLimit, ProviderAccount, ProviderDetails, ProviderItem,
        ProviderItemInput, ProviderPage, ProviderVersion, ProviderVersionInput, SearchRequest,
        MAX_RESULT_ITEMS, MAX_VERSION_ENTRIES,
    },
    text_policy::credential_shaped_scope,
    url_policy::EphemeralDownloadUrl,
    KnownProvider, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{
    ProviderAccountState, ProviderArtifactKind, ProviderItemKind, ProviderScope,
};
use serde_json::Value;

const PROVIDER: KnownProvider = KnownProvider::Nexus;
const NEXUS_DOWNLOAD_ROOTS: &[&str] = &["nexusmods.com", "nexus-cdn.com"];
const NEXUS_CANONICAL_ROOT: &str = "https://www.nexusmods.com/";
const MAX_NEXUS_SOURCE_URL_BYTES: usize = 512;
const SEARCH_ARRAY_LIMITS: &[JsonArrayLimit] = &[
    JsonArrayLimit::new(&[], MAX_RESULT_ITEMS),
    JsonArrayLimit::new(&["items"], MAX_RESULT_ITEMS),
    JsonArrayLimit::new(&["data", "mods", "nodes"], MAX_RESULT_ITEMS),
];
const FILE_ARRAY_LIMITS: &[JsonArrayLimit] = &[
    JsonArrayLimit::new(&[], MAX_VERSION_ENTRIES),
    JsonArrayLimit::new(&["files"], MAX_VERSION_ENTRIES),
];
const DOWNLOAD_LINK_ARRAY_LIMITS: &[JsonArrayLimit] = &[
    JsonArrayLimit::new(&[], 32),
    JsonArrayLimit::new(&["links"], 32),
];

pub fn map_search(payload: &[u8], request: &SearchRequest) -> ProviderResult<ProviderPage> {
    request.validate_for(PROVIDER)?;
    normalized_domain(request.scope())?;
    let value: Value = decode_json_with_array_limits(PROVIDER, payload, SEARCH_ARRAY_LIMITS)?;
    if value
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let records = search_records(&value)?;
    let total_count = value
        .pointer("/data/mods/totalCount")
        .or_else(|| value.get("totalCount"))
        .and_then(Value::as_u64);
    let explicit_has_more = value.get("hasMore").and_then(Value::as_bool);
    let needle = request.query().to_lowercase();
    let mut items = Vec::new();
    for record in records {
        let item = map_record(record)?;
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
    items.sort_by_key(|item| !item.featured());
    let has_more = explicit_has_more.unwrap_or_else(|| {
        total_count.map_or(items.len() >= request.limit() as usize, |total| {
            u64::from(request.offset()).saturating_add(items.len() as u64) < total
        })
    });
    ProviderPage::new(
        PROVIDER,
        request,
        items,
        has_more,
        total_count,
        Some("Metadata and popularity counts provided by Nexus Mods".into()),
    )
}

pub fn map_details(
    mod_payload: &[u8],
    files_payload: &[u8],
    domain: &str,
) -> ProviderResult<ProviderDetails> {
    normalized_domain(domain)?;
    let mod_value: Value =
        decode_json_with_array_limits(PROVIDER, mod_payload, SEARCH_ARRAY_LIMITS)?;
    let record = if has_record_id(&mod_value) {
        &mod_value
    } else {
        search_records(&mod_value)?
            .first()
            .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?
    };
    let item = map_record(record)?;
    let files_value: Value =
        decode_json_with_array_limits(PROVIDER, files_payload, FILE_ARRAY_LIMITS)?;
    let files = files_value
        .get("files")
        .and_then(Value::as_array)
        .or_else(|| files_value.as_array())
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    if files.len() > MAX_VERSION_ENTRIES {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let mut versions = Vec::with_capacity(files.len());
    for file in files {
        if file
            .get("category_name")
            .or_else(|| file.get("categoryName"))
            .and_then(Value::as_str)
            .is_some_and(|category| category.eq_ignore_ascii_case("DELETED"))
        {
            continue;
        }
        versions.push(map_file(&item, file)?);
    }
    ProviderDetails::new(PROVIDER, item, versions)
}

pub fn resolve_download(
    version: &ProviderVersion,
    links_payload: &[u8],
    account_state: ProviderAccountState,
) -> ProviderResult<DownloadResolution> {
    if account_state != ProviderAccountState::SignedIn {
        return Err(ProviderFailure::authentication_required(PROVIDER));
    }
    if version.source().provider_id().as_str() != PROVIDER.as_str()
        || version.source().scope().is_none()
        || !version.directly_downloadable()
    {
        return Err(ProviderFailure::unsupported(PROVIDER));
    }
    let value: Value =
        decode_json_with_array_limits(PROVIDER, links_payload, DOWNLOAD_LINK_ARRAY_LIMITS)?;
    let links = value
        .as_array()
        .or_else(|| value.get("links").and_then(Value::as_array))
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    if links.len() > 32 {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let url = links
        .iter()
        .filter_map(|link| {
            link.get("URI")
                .or_else(|| link.get("uri"))
                .or_else(|| link.get("downloadUrl"))
                .and_then(Value::as_str)
        })
        .find_map(|raw| EphemeralDownloadUrl::parse(raw, NEXUS_DOWNLOAD_ROOTS).ok())
        .ok_or_else(|| ProviderFailure::unsupported(PROVIDER))?;
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
    let pending = value
        .get("ssoPending")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let configured = value
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let connected = value
        .get("connected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let valid = value
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(connected);
    let available = value
        .get("ssoAvailable")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let state = if pending {
        ProviderAccountState::Authorizing
    } else if connected && valid {
        ProviderAccountState::SignedIn
    } else if configured {
        ProviderAccountState::Expired
    } else if !available {
        ProviderAccountState::Unavailable
    } else {
        ProviderAccountState::SignedOut
    };
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| (state == ProviderAccountState::SignedIn).then(|| "Nexus Mods user".into()));
    ProviderAccount::new(
        PROVIDER,
        state,
        name,
        string_value(value.get("userId")),
        value.get("premium").and_then(Value::as_bool),
        value.get("supporter").and_then(Value::as_bool),
    )
}

fn search_records(value: &Value) -> ProviderResult<&[Value]> {
    value
        .as_array()
        .or_else(|| value.pointer("/data/mods/nodes").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .map(Vec::as_slice)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))
}

fn has_record_id(value: &Value) -> bool {
    value.get("modId").is_some() || value.get("mod_id").is_some() || value.get("id").is_some()
}

fn map_record(value: &Value) -> ProviderResult<ProviderItem> {
    let id = positive_numeric_alias(value, &["modId", "mod_id", "id"], PROVIDER)?
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let domain = record_domain(value, &id)?;
    let canonical_url = format!("{NEXUS_CANONICAL_ROOT}{domain}/mods/{id}");
    let source = provider_ref(
        PROVIDER,
        ProviderItemKind::Mod,
        &id,
        Some(&domain),
        None,
        ProviderArtifactKind::Unknown,
        None,
        Some(canonical_url),
    )?;
    let title = value
        .get("name")
        .or_else(|| value.get("title"))
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?
        .to_owned();
    let summary = value
        .get("summary")
        .or_else(|| value.get("description"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let author = value
        .get("author")
        .or_else(|| value.get("uploaded_by"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| Some("Nexus Mods contributor".into()));
    let updated_at = ["updatedAt", "updated_time"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_owned))
        .or_else(|| {
            value
                .get("updated_timestamp")
                .and_then(Value::as_u64)
                .map(|timestamp| timestamp.to_string())
        });
    let adult = value
        .get("adultContent")
        .or_else(|| value.get("contains_adult_content"))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| value.get("contentRating").and_then(Value::as_str) == Some("adult"));
    ProviderItem::from_input(
        PROVIDER,
        ProviderItemInput {
            source,
            title,
            summary,
            author,
            updated_at,
            content_rating: if adult {
                ContentRating::Adult
            } else {
                ContentRating::General
            },
            downloads: value.get("downloads").and_then(Value::as_u64),
            endorsements: value.get("endorsements").and_then(Value::as_u64),
            featured: value.get("featured").and_then(Value::as_bool) == Some(true)
                || (domain == "deltarune" && id == "23"),
        },
    )
}

fn map_file(item: &ProviderItem, value: &Value) -> ProviderResult<ProviderVersion> {
    let file_id = positive_numeric_alias(value, &["file_id", "fileId", "id"], PROVIDER)?
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let version = normalized_version_alias(value, &["version", "mod_version"], PROVIDER)?;
    let version_id = version_contract_id(version.as_deref(), &file_id);
    let source = provider_ref(
        PROVIDER,
        ProviderItemKind::Mod,
        item.source().resource_id().as_str(),
        item.source().scope().map(ProviderScope::as_str),
        Some(&file_id),
        ProviderArtifactKind::File,
        Some(&version_id),
        item.source().canonical_url().map(str::to_owned),
    )?;
    let file_name = value
        .get("file_name")
        .or_else(|| value.get("fileName"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let name = value.get("name").and_then(Value::as_str).map(str::to_owned);
    let label = version
        .or_else(|| name.clone())
        .or_else(|| file_name.clone())
        .unwrap_or_else(|| format!("File {file_id}"));
    let category = value
        .get("category_name")
        .or_else(|| value.get("categoryName"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let directly_downloadable = !matches!(
        category.to_ascii_uppercase().as_str(),
        "DELETED" | "ARCHIVED" | "OLD_VERSION"
    );
    let size_bytes = value.get("sizeBytes").and_then(Value::as_u64).or_else(|| {
        value
            .get("size_kb")
            .or_else(|| value.get("sizeKb"))
            .and_then(Value::as_u64)
            .map(|size| size.saturating_mul(1024))
    });
    ProviderVersion::from_input(
        PROVIDER,
        ProviderVersionInput {
            source,
            label,
            file_name: file_name.or(name),
            size_bytes,
            published_at: value
                .get("uploaded_time")
                .or_else(|| value.get("uploadedAt"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    value
                        .get("uploaded_timestamp")
                        .and_then(Value::as_u64)
                        .map(|timestamp| timestamp.to_string())
                }),
            sha256: ["sha256", "file_hash"]
                .into_iter()
                .find_map(|key| string_value(value.get(key))),
            primary: value
                .get("is_primary")
                .or_else(|| value.get("isPrimary"))
                .and_then(Value::as_bool)
                == Some(true)
                || category.eq_ignore_ascii_case("MAIN"),
            directly_downloadable,
        },
    )
}

fn normalized_domain(value: &str) -> ProviderResult<String> {
    canonical_domain(value).ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))
}

fn canonical_domain(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 80
        || credential_shaped_scope(value)
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let domain = value.to_ascii_lowercase();
    ProviderScope::parse(&domain).map(|_| domain).ok()
}

fn record_domain(value: &Value, expected_id: &str) -> ProviderResult<String> {
    let mut domain = None;
    for key in [
        "gameDomainName",
        "game_domain_name",
        "domainName",
        "domain_name",
        "domain",
    ] {
        if let Some(raw) = value.get(key) {
            let candidate = raw
                .as_str()
                .and_then(canonical_domain)
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
            merge_record_domain(&mut domain, candidate)?;
        }
    }
    if let Some(game) = value.get("game") {
        let game = game
            .as_object()
            .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
        for key in [
            "domainName",
            "domain_name",
            "gameDomainName",
            "game_domain_name",
            "domain",
        ] {
            if let Some(raw) = game.get(key) {
                let candidate = raw
                    .as_str()
                    .and_then(canonical_domain)
                    .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
                merge_record_domain(&mut domain, candidate)?;
            }
        }
    }
    for key in ["sourceUrl", "source_url"] {
        if let Some(raw) = value.get(key) {
            let raw = raw
                .as_str()
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
            let (url_domain, url_id) = nexus_record_route(raw)?;
            if url_id != expected_id {
                return Err(ProviderFailure::invalid_payload(PROVIDER));
            }
            merge_record_domain(&mut domain, url_domain)?;
        }
    }
    domain.ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))
}

fn merge_record_domain(current: &mut Option<String>, candidate: String) -> ProviderResult<()> {
    if current
        .as_deref()
        .is_some_and(|existing| existing != candidate)
    {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    if current.is_none() {
        *current = Some(candidate);
    }
    Ok(())
}

fn nexus_record_route(raw: &str) -> ProviderResult<(String, String)> {
    if raw.len() > MAX_NEXUS_SOURCE_URL_BYTES
        || raw.chars().any(char::is_control)
        || raw.contains(['?', '#', '%', '\\'])
    {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let path = raw
        .strip_prefix(NEXUS_CANONICAL_ROOT)
        .filter(|path| !path.is_empty() && !path.ends_with('/') && !path.contains("//"))
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let mut segments = path.split('/');
    let raw_domain = segments
        .next()
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let category = segments
        .next()
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let raw_id = segments
        .next()
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    if segments.next().is_some() || category != "mods" {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let domain = canonical_domain(raw_domain)
        .filter(|domain| domain == raw_domain)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let id = raw_id
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|id| id.to_string())
        .filter(|id| id == raw_id)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let canonical = format!("{NEXUS_CANONICAL_ROOT}{domain}/mods/{id}");
    if canonical != raw {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    Ok((domain, id))
}
