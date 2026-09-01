//! Pure mapping for the existing normalized ModDB recent-RSS catalogue payload.

use crate::{
    model::{
        decode_json_with_array_limits, provider_ref, ContentRating, JsonArrayLimit,
        ProviderAccount, ProviderDetails, ProviderItem, ProviderItemInput, ProviderPage,
        SearchRequest, MAX_RESULT_ITEMS,
    },
    KnownProvider, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{
    ProviderArtifactKind, ProviderItemKind, ProviderResourceId, ProviderScope,
};
use serde_json::Value;

const PROVIDER: KnownProvider = KnownProvider::ModDb;
const MODDB_CANONICAL_ROOT: &str = "https://www.moddb.com/";
const MAX_MODDB_URL_BYTES: usize = 512;
const SEARCH_ARRAY_LIMITS: &[JsonArrayLimit] = &[
    JsonArrayLimit::new(&[], MAX_RESULT_ITEMS),
    JsonArrayLimit::new(&["items"], MAX_RESULT_ITEMS),
];

#[derive(Eq, PartialEq)]
pub(crate) struct ModDbRouteIdentity {
    pub resource_id: String,
    pub scope: String,
    pub canonical_url: String,
}

pub fn map_search(payload: &[u8], request: &SearchRequest) -> ProviderResult<ProviderPage> {
    request.validate_for(PROVIDER)?;
    validate_scope(request.scope())?;
    let value: Value = decode_json_with_array_limits(PROVIDER, payload, SEARCH_ARRAY_LIMITS)?;
    let items = value
        .as_array()
        .or_else(|| value.get("items").and_then(Value::as_array))
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let needle = request.query().to_lowercase();
    let mut normalized = Vec::new();
    for item in items {
        let item = map_item(item, request.scope())?;
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
        normalized.push(item);
        if normalized.len() >= request.limit() as usize {
            break;
        }
    }
    let attribution = value
        .get("attribution")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            Some("ModDB RSS exposes only its recent downloads, not the full catalogue".into())
        });
    ProviderPage::new(
        PROVIDER,
        request,
        normalized,
        value
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        value.get("totalCount").and_then(Value::as_u64),
        attribution,
    )
}

pub fn map_details(payload: &[u8], scope: &str) -> ProviderResult<ProviderDetails> {
    validate_scope(scope)?;
    let value: Value = decode_json_with_array_limits(PROVIDER, payload, SEARCH_ARRAY_LIMITS)?;
    if value.is_array() || value.get("items").is_some() {
        return Err(ProviderFailure::unsupported(PROVIDER));
    }
    ProviderDetails::new(PROVIDER, map_item(&value, scope)?, Vec::new())
}

#[must_use]
pub fn account() -> ProviderAccount {
    ProviderAccount::not_required(PROVIDER)
}

fn map_item(value: &Value, fallback_scope: &str) -> ProviderResult<ProviderItem> {
    let identity = route_identity_alias(value, fallback_scope)?;
    let source = provider_ref(
        PROVIDER,
        ProviderItemKind::Mod,
        &identity.resource_id,
        Some(&identity.scope),
        None,
        ProviderArtifactKind::Unknown,
        None,
        Some(identity.canonical_url),
    )?;
    let title = value
        .get("title")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?
        .to_owned();
    let content_rating = match value.get("contentRating").and_then(Value::as_str) {
        Some("adult") => ContentRating::Adult,
        Some("general") => ContentRating::General,
        _ => ContentRating::Unknown,
    };
    ProviderItem::from_input(
        PROVIDER,
        ProviderItemInput {
            source,
            title,
            summary: value
                .get("summary")
                .or_else(|| value.get("description"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            author: value
                .get("author")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| Some("ModDB contributor".into())),
            updated_at: value
                .get("updatedAt")
                .or_else(|| value.get("published"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            content_rating,
            downloads: value.get("downloads").and_then(Value::as_u64),
            endorsements: None,
            featured: false,
        },
    )
}

fn route_identity_alias(value: &Value, fallback_scope: &str) -> ProviderResult<ModDbRouteIdentity> {
    let mut identity = None;
    for key in ["sourceUrl", "link"] {
        if let Some(raw) = value.get(key) {
            let raw = raw
                .as_str()
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
            let candidate = route_identity(raw, Some(fallback_scope))?;
            if identity
                .as_ref()
                .is_some_and(|existing| existing != &candidate)
            {
                return Err(ProviderFailure::invalid_payload(PROVIDER));
            }
            identity = Some(candidate);
        }
    }
    identity.ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))
}

pub(crate) fn route_identity(
    raw_source: &str,
    _fallback_scope: Option<&str>,
) -> ProviderResult<ModDbRouteIdentity> {
    if raw_source.len() > MAX_MODDB_URL_BYTES
        || raw_source.chars().any(char::is_control)
        || raw_source.contains(['?', '#', '%', '\\'])
    {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let raw_path = raw_source
        .strip_prefix(MODDB_CANONICAL_ROOT)
        .filter(|path| !path.is_empty() && !path.ends_with('/') && !path.contains("//"))
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let segments = raw_path
        .split('/')
        .map(canonical_route_segment)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    let (scope_segments, resource_id) = match segments.as_slice() {
        [root, game, category, resource]
            if *root == "games" && matches!(*category, "downloads" | "mods") =>
        {
            (vec![*root, *game, *category], *resource)
        }
        [root, resource] if *root == "mods" => (vec![*root], *resource),
        [root, mod_slug, category, resource] if *root == "mods" && *category == "downloads" => {
            (vec![*root, *mod_slug, *category], *resource)
        }
        _ => return Err(ProviderFailure::invalid_payload(PROVIDER)),
    };
    ProviderResourceId::parse(resource_id)
        .map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    let scope = scope_segments.join(".");
    ProviderScope::parse(&scope).map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    let canonical_path = segments.join("/");
    let canonical_url = format!("{MODDB_CANONICAL_ROOT}{canonical_path}");
    if canonical_url != raw_source {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    Ok(ModDbRouteIdentity {
        resource_id: resource_id.to_owned(),
        scope,
        canonical_url,
    })
}

fn canonical_route_segment(value: &str) -> Option<&str> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        None
    } else {
        Some(value)
    }
}

fn validate_scope(value: &str) -> ProviderResult<()> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let value = value.to_ascii_lowercase();
    ProviderScope::parse(&value)
        .map(|_| ())
        .map_err(|_| ProviderFailure::invalid_request(PROVIDER))
}
