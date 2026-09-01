use crate::{
    channels::nexus_oauth,
    error,
    provider_cache::{CacheFreshness, ProviderCatalogCache},
    state::AppState,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use deltamod_credentials_adapter::CredentialKind;
use deltamod_network_domain::{validate_https_url, BrowseRequest, Provider};
use deltamod_network_runtime::{ModDb, ModEntry, Provider as RuntimeProvider, RuntimeError};
use deltamod_provider_platform::{
    normalize_provider_error, KnownProvider, ProviderErrorInput, ProviderFailureKind,
};
use deltamod_tauri_os_adapters::validate_https_external;
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShopProvider {
    GameBanana,
    Nexus,
    ModDb,
}

impl ShopProvider {
    const fn network_provider(self) -> Option<Provider> {
        match self {
            Self::GameBanana => Some(Provider::GameBanana),
            Self::Nexus => Some(Provider::Nexus),
            Self::ModDb => Some(Provider::ModDb),
        }
    }
}

const NEXUS_CATALOG_QUERY: &str = r#"
query BrowseMods($filter: ModsFilter, $sort: [ModsSort!], $offset: Int, $count: Int) {
  mods(filter: $filter, sort: $sort, offset: $offset, count: $count) {
    totalCount
    nodes {
      modId
      name
      summary
      author
      updatedAt
      pictureUrl
      adultContent
      downloads
      endorsements
    }
  }
}
"#;

fn legacy_mod_image_from_roots<I>(roots: I, uid: &str) -> Option<String>
where
    I: IntoIterator<Item = std::path::PathBuf>,
{
    for root in roots {
        let Ok(directory) = std::fs::symlink_metadata(&root) else {
            continue;
        };
        if !directory.is_dir() || directory.file_type().is_symlink() {
            continue;
        }
        let identity_path = root.join("__deltaID.json");
        let Ok(identity_meta) = std::fs::symlink_metadata(&identity_path) else {
            continue;
        };
        if !identity_meta.is_file()
            || identity_meta.file_type().is_symlink()
            || identity_meta.len() > 64 * 1024
        {
            continue;
        }
        let Ok(identity_bytes) = std::fs::read(&identity_path) else {
            continue;
        };
        let Ok(identity) = serde_json::from_slice::<Value>(&identity_bytes) else {
            continue;
        };
        if identity.get("uniqueId").and_then(Value::as_str) != Some(uid) {
            continue;
        }
        let icon_path = root.join("icon.png");
        let Ok(icon_meta) = std::fs::symlink_metadata(&icon_path) else {
            continue;
        };
        if !icon_meta.is_file()
            || icon_meta.file_type().is_symlink()
            || icon_meta.len() == 0
            || icon_meta.len() > 4 * 1024 * 1024
        {
            continue;
        }
        let Ok(icon) = std::fs::read(icon_path) else {
            continue;
        };
        if !icon.starts_with(b"\x89PNG\r\n\x1a\n") {
            continue;
        }
        return Some(format!("data:image/png;base64,{}", STANDARD.encode(icon)));
    }
    None
}

fn legacy_mod_image(state: &AppState, uid: &str) -> Option<String> {
    let entries = std::fs::read_dir(state.data_root.root.join("packets")).ok()?;
    legacy_mod_image_from_roots(entries.flatten().map(|entry| entry.path()), uid)
}

fn provider(value: &str) -> Option<ShopProvider> {
    match value {
        "gamebanana" => Some(ShopProvider::GameBanana),
        "nexus" => Some(ShopProvider::Nexus),
        "moddb" => Some(ShopProvider::ModDb),
        _ => None,
    }
}

fn current_game(state: &AppState) -> Option<Value> {
    state
        .game
        .dispatch("getCurrentGameInfo", &[])
        .ok()
        .flatten()
}

fn mapped_source(game: &Value, provider: Provider) -> Option<&str> {
    match provider {
        Provider::GameBanana => game
            .pointer("/gamebanana/id")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0)
            .map(|_| "gamebanana"),
        Provider::Nexus => game
            .pointer("/sources/nexus/domain")
            .and_then(Value::as_str),
        Provider::ModDb => game.pointer("/sources/moddb/slug").and_then(Value::as_str),
    }
    .filter(|value| !value.is_empty())
}

fn gamebanana_catalog_url(game: &Value, raw: &str) -> Option<String> {
    let game_id = game
        .pointer("/gamebanana/id")?
        .as_u64()
        .filter(|id| *id > 0)?;
    let safe = validate_https_url(Provider::GameBanana, raw).ok()?;
    if safe.host != "gamebanana.com" {
        return None;
    }
    let parsed = Url::parse(&safe.raw).ok()?;
    let segments = parsed.path_segments()?.collect::<Vec<_>>();
    let game_id_text = game_id.to_string();
    let game_feed = segments.len() == 4
        && segments[0] == "apiv11"
        && segments[1] == "Game"
        && segments[2] == game_id_text
        && matches!(segments[3], "Subfeed" | "TopSubs");
    let search = segments == ["apiv11", "Util", "Search", "Results"]
        && parsed
            .query_pairs()
            .any(|(key, value)| key == "_idGameRow" && value == game_id_text)
        && parsed
            .query_pairs()
            .any(|(key, value)| key == "_sModelName" && value == "Mod")
        && parsed.query_pairs().any(|(key, value)| {
            key == "_sSearchString"
                && !value.is_empty()
                && value.len() <= 256
                && !value.chars().any(char::is_control)
        });
    (game_feed || search).then_some(safe.raw)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn cached_result(result: Value, cached: bool, stale: bool, stored_at_ms: Option<u64>) -> Value {
    let mut result = result;
    if let Some(object) = result.as_object_mut() {
        object.insert("cached".into(), Value::Bool(cached));
        object.insert("stale".into(), Value::Bool(stale));
        if let Some(stored_at_ms) = stored_at_ms {
            object.insert("cachedAtMs".into(), Value::from(stored_at_ms));
        }
    }
    result
}

fn normalized_provider_error(provider: KnownProvider, kind: ProviderFailureKind) -> Value {
    let contract =
        normalize_provider_error(ProviderErrorInput::new(provider, kind)).into_contract();
    let payload = contract.into_payload();
    json!({
        "code": payload.code.as_str(),
        "messageKey": payload.message_key,
        "message": match kind {
            ProviderFailureKind::Offline => "The provider is offline. Cached results are unavailable for this request.",
            ProviderFailureKind::AuthenticationRequired => "Connect this provider account before browsing its catalogue.",
            ProviderFailureKind::RateLimited => "The provider rate limit was reached. Try again after the reported wait period.",
            _ => "The provider catalogue is temporarily unavailable.",
        },
        "retryable": payload.retryable,
        "recoveryAction": payload.recovery_action,
    })
}

fn provider_failure_kind(error: &RuntimeError) -> ProviderFailureKind {
    match error {
        RuntimeError::Http {
            status: 401 | 403, ..
        }
        | RuntimeError::Auth(_) => ProviderFailureKind::AuthenticationRequired,
        RuntimeError::Http { status: 429, .. } => ProviderFailureKind::RateLimited,
        RuntimeError::Http { .. } => ProviderFailureKind::Http,
        RuntimeError::Request(_) | RuntimeError::Io(_) => ProviderFailureKind::Offline,
        RuntimeError::Json(_) | RuntimeError::Xml(_) | RuntimeError::TooLarge { .. } => {
            ProviderFailureKind::InvalidPayload
        }
        RuntimeError::Cancelled => ProviderFailureKind::Cancelled,
        RuntimeError::Url(_) | RuntimeError::InvalidInput(_) => ProviderFailureKind::InvalidRequest,
        RuntimeError::Unsupported(_) => ProviderFailureKind::UnsupportedCapability,
    }
}

fn browse_with_cache<F>(
    state: &AppState,
    key: &str,
    provider: KnownProvider,
    offline: bool,
    fetch: F,
) -> Value
where
    F: FnOnce() -> Result<Value, ProviderFailureKind>,
{
    let cached = state
        .provider_cache
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(key));
    if let Some(entry) = cached
        .as_ref()
        .filter(|entry| entry.freshness == CacheFreshness::Fresh && !offline)
    {
        return json!({
            "ok": true,
            "result": cached_result(entry.result.clone(), true, false, Some(entry.stored_at_ms))
        });
    }
    if !offline {
        match fetch() {
            Ok(result) => {
                if let Ok(mut cache) = state.provider_cache.lock() {
                    let _ = cache.put(key, &result);
                }
                return json!({
                    "ok": true,
                    "result": cached_result(result, false, false, Some(unix_ms()))
                });
            }
            Err(kind) => {
                if let Some(entry) = cached {
                    return json!({
                        "ok": true,
                        "result": cached_result(entry.result, true, true, Some(entry.stored_at_ms)),
                        "warning": normalized_provider_error(provider, kind),
                    });
                }
                return json!({"ok": false, "error": normalized_provider_error(provider, kind)});
            }
        }
    }
    if let Some(entry) = cached {
        return json!({
            "ok": true,
            "result": cached_result(entry.result, true, true, Some(entry.stored_at_ms)),
            "warning": normalized_provider_error(provider, ProviderFailureKind::Offline),
        });
    }
    json!({
        "ok": false,
        "error": normalized_provider_error(provider, ProviderFailureKind::Offline)
    })
}

fn moddb_catalog(slug: &str, entries: Vec<ModEntry>, query: Option<&str>) -> Value {
    let needle = query.unwrap_or_default().trim().to_ascii_lowercase();
    let items: Vec<Value> = entries
        .into_iter()
        .filter(|entry| {
            needle.is_empty()
                || entry.title.to_ascii_lowercase().contains(&needle)
                || entry
                    .summary
                    .as_deref()
                    .is_some_and(|summary| summary.to_ascii_lowercase().contains(&needle))
        })
        .map(|entry| {
            let id = entry
                .link
                .rsplit('/')
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or("moddb-download")
                .to_owned();
            let canonical_identity = format!("moddb:{}", entry.link);
            json!({
                "provider": "moddb",
                "id": id,
                "canonicalIdentity": canonical_identity,
                "title": entry.title,
                "summary": entry.summary.unwrap_or_default(),
                "author": "ModDB contributor",
                "updatedAt": entry.published.unwrap_or_default(),
                "imageUrl": entry.image_url,
                "sourceUrl": entry.link,
                "contentRating": "unknown",
                "installMode": "manual",
                "actionLabel": "Open download page"
            })
        })
        .collect();
    json!({
        "provider": "moddb",
        "catalogScope": "recent-rss",
        "feedLimit": 10,
        "hasMore": false,
        "attribution": "ModDB RSS exposes only its 10 most recent downloads",
        "catalogUrl": format!("https://www.moddb.com/games/{slug}/downloads"),
        "items": items
    })
}

fn nexus_catalog(
    state: &AppState,
    domain: &str,
    query: Option<&str>,
    sort: &str,
    offset: u32,
    count: u32,
    access_token: Option<String>,
) -> Result<Value, ProviderFailureKind> {
    let query = query.unwrap_or_default().trim();
    let secondary_sort = match sort {
        "latest_updated" => "updatedAt",
        "trending" => "endorsements",
        _ => "createdAt",
    };
    let mut filter = json!({
        "op": "AND",
        "gameDomainName": [{"value": domain, "op": "EQUALS"}]
    });
    if !query.is_empty() {
        filter["nameStemmed"] = json!([{"value": query, "op": "MATCHES"}]);
    }
    let sort_value = if !query.is_empty() {
        let secondary = match secondary_sort {
            "updatedAt" => json!({"updatedAt": {"direction": "DESC"}}),
            "endorsements" => json!({"endorsements": {"direction": "DESC"}}),
            _ => json!({"createdAt": {"direction": "DESC"}}),
        };
        json!([{"relevance": {"direction": "DESC"}}, secondary])
    } else if sort == "trending" {
        json!([
            {"endorsements": {"direction": "DESC"}},
            {"downloads": {"direction": "DESC"}}
        ])
    } else if secondary_sort == "updatedAt" {
        json!([{"updatedAt": {"direction": "DESC"}}])
    } else {
        json!([{"createdAt": {"direction": "DESC"}}])
    };
    let body = json!({
        "query": NEXUS_CATALOG_QUERY,
        "variables": {
            "filter": filter,
            "sort": sort_value,
            "offset": offset,
            "count": count
        }
    });
    let payload = state
        .network_runtime
        .lock()
        .map_err(|_| ProviderFailureKind::Internal)?
        .block_on(state.network.nexus_graphql(body, access_token.as_deref()))
        .map_err(|failure| provider_failure_kind(&failure))?;
    if payload
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(ProviderFailureKind::InvalidPayload);
    }
    let page = payload
        .pointer("/data/mods")
        .ok_or(ProviderFailureKind::InvalidPayload)?;
    let nodes = page
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or(ProviderFailureKind::InvalidPayload)?;
    let mut items: Vec<Value> = nodes
        .iter()
        .take(count as usize)
        .filter_map(|node| {
            let mod_id = node
                .get("modId")
                .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
                .filter(|id| *id > 0)?;
            let picture_url = node
                .get("pictureUrl")
                .and_then(Value::as_str)
                .filter(|url| validate_https_url(Provider::Nexus, url).is_ok());
            Some(json!({
                "provider": "nexus",
                "id": mod_id.to_string(),
                "canonicalIdentity": format!("nexus:{domain}:{mod_id}"),
                "title": node.get("name").and_then(Value::as_str).unwrap_or("Nexus mod"),
                "summary": node.get("summary").and_then(Value::as_str).unwrap_or_default(),
                "author": node.get("author").and_then(Value::as_str).unwrap_or("Nexus Mods contributor"),
                "updatedAt": node.get("updatedAt").and_then(Value::as_str).unwrap_or_default(),
                "imageUrl": picture_url,
                "sourceUrl": format!("https://www.nexusmods.com/{domain}/mods/{mod_id}"),
                "contentRating": if node.get("adultContent").and_then(Value::as_bool).unwrap_or(false) { "adult" } else { "general" },
                "downloads": node.get("downloads").and_then(Value::as_u64).unwrap_or(0),
                "endorsements": node.get("endorsements").and_then(Value::as_u64).unwrap_or(0),
                "featured": domain.eq_ignore_ascii_case("deltarune") && mod_id == 23,
                "installMode": "nexus",
                "actionLabel": "Download"
            }))
        })
        .collect();
    items.sort_by_key(|item| {
        !item
            .get("featured")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });
    let total_count = page.get("totalCount").and_then(Value::as_u64);
    let has_more = total_count
        .map(|total| u64::from(offset).saturating_add(items.len() as u64) < total)
        .unwrap_or(items.len() >= count as usize);
    Ok(json!({
        "provider": "nexus",
        "catalogScope": "page",
        "hasMore": has_more,
        "totalCount": total_count,
        "attribution": "Metadata and popularity counts provided by Nexus Mods",
        "items": items
    }))
}

fn open_provider_url(app: &AppHandle, raw: &str, provider: ShopProvider) -> Result<Value, String> {
    let url = match provider {
        ShopProvider::GameBanana => validate_https_external(raw, &["gamebanana.com"])
            .map(|url| url.to_string())
            .map_err(|_| ()),
        ShopProvider::Nexus => {
            validate_https_external(raw, &["nexusmods.com", "www.nexusmods.com"])
                .map(|url| url.to_string())
                .map_err(|_| ())
        }
        ShopProvider::ModDb => validate_https_external(raw, &["moddb.com", "www.moddb.com"])
            .map(|url| url.to_string())
            .map_err(|_| ()),
    }
    .map_err(|_| error::invalid("modSources:open"))?;
    app.opener()
        .open_url(&url, None::<&str>)
        .map(|_| Value::Null)
        .map_err(|_| error::internal())
}

pub fn dispatch(
    app: &AppHandle,
    state: &AppState,
    channel: &str,
    data: &[Value],
) -> Result<Option<Value>, String> {
    match channel {
        "modSources:getProviders" => {
            let game = current_game(state).unwrap_or(Value::Null);
            Ok(Some(json!([
                {"id":"gamebanana","name":"GameBanana","available":mapped_source(&game, Provider::GameBanana).is_some()},
                {"id":"nexus","name":"Nexus Mods","available":mapped_source(&game, Provider::Nexus).is_some(),"requiresAuthentication":true},
                {"id":"moddb","name":"ModDB (10 recent)","available":mapped_source(&game, Provider::ModDb).is_some()}
            ])))
        }
        "modSources:browse" => {
            let request = data
                .first()
                .ok_or_else(|| error::invalid("modSources:browse"))?;
            let provider_name = request
                .get("provider")
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("modSources:browse"))?;
            let shop_provider =
                provider(provider_name).ok_or_else(|| error::invalid("modSources:browse"))?;
            let query = request
                .get("query")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let provider = shop_provider
                .network_provider()
                .ok_or_else(|| error::invalid("modSources:browse"))?;
            let game =
                current_game(state).ok_or_else(|| error::unavailable("modSources:browse"))?;
            let offline = request
                .get("offline")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if provider == Provider::GameBanana {
                let url = request
                    .get("url")
                    .and_then(Value::as_str)
                    .and_then(|url| gamebanana_catalog_url(&game, url))
                    .ok_or_else(|| error::invalid("modSources:browse"))?;
                let key = ProviderCatalogCache::request_key(&["gamebanana", &url]);
                return Ok(Some(browse_with_cache(
                    state,
                    &key,
                    KnownProvider::GameBanana,
                    offline,
                    || {
                        state
                            .network_runtime
                            .lock()
                            .map_err(|_| ProviderFailureKind::Internal)?
                            .block_on(state.network.json::<Value>(
                                RuntimeProvider::GameBanana,
                                &url,
                                None,
                            ))
                            .map(|payload| {
                                json!({
                                    "provider": "gamebanana",
                                    "payload": payload
                                })
                            })
                            .map_err(|failure| provider_failure_kind(&failure))
                    },
                )));
            }
            let domain = mapped_source(&game, provider).map(str::to_owned);
            let sort = request
                .get("sort")
                .and_then(Value::as_str)
                .filter(|value| matches!(*value, "latest_added" | "latest_updated" | "trending"))
                .unwrap_or("latest_added")
                .to_owned();
            let offset = request
                .get("offset")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .try_into()
                .map_err(|_| error::invalid("modSources:browse"))?;
            let count = request
                .get("count")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .try_into()
                .map_err(|_| error::invalid("modSources:browse"))?;
            BrowseRequest::new(provider, domain.clone(), query.clone(), offset, count)
                .map_err(|_| error::invalid("modSources:browse"))?;
            if provider == Provider::Nexus {
                let domain = domain.ok_or_else(|| error::invalid("modSources:browse"))?;
                let key = ProviderCatalogCache::request_key(&[
                    "nexus",
                    &domain,
                    query.as_deref().unwrap_or_default(),
                    &sort,
                    &offset.to_string(),
                    &count.to_string(),
                ]);
                return Ok(Some(browse_with_cache(
                    state,
                    &key,
                    KnownProvider::Nexus,
                    offline,
                    || nexus_catalog(state, &domain, query.as_deref(), &sort, offset, count, None),
                )));
            }
            let slug = domain.ok_or_else(|| error::invalid("modSources:browse"))?;
            if slug.is_empty()
                || slug.len() > 80
                || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(error::invalid("modSources:browse"));
            }
            let url = format!("https://rss.moddb.com/games/{slug}/downloads/feed/rss.xml");
            let key = ProviderCatalogCache::request_key(&[
                "moddb",
                &slug,
                query.as_deref().unwrap_or_default(),
            ]);
            Ok(Some(browse_with_cache(
                state,
                &key,
                KnownProvider::ModDb,
                offline,
                || {
                    let client = state.network.clone();
                    let result = std::thread::spawn(move || {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|_| ProviderFailureKind::Internal)?;
                        runtime
                            .block_on(ModDb { client: &client }.browse(&url))
                            .map(|response| response.value)
                            .map_err(|failure| provider_failure_kind(&failure))
                    })
                    .join()
                    .unwrap_or(Err(ProviderFailureKind::Internal));
                    result.map(|value| moddb_catalog(&slug, value, query.as_deref()))
                },
            )))
        }
        "modSources:open" => {
            let request = data
                .first()
                .ok_or_else(|| error::invalid("modSources:open"))?;
            let provider = request
                .get("provider")
                .and_then(Value::as_str)
                .and_then(provider)
                .ok_or_else(|| error::invalid("modSources:open"))?;
            let url = request
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("modSources:open"))?;
            open_provider_url(app, url, provider).map(Some)
        }
        "modSources:nexusStatus" => {
            let configured = nexus_oauth::has_tokens(state);
            let available = nexus_oauth::configured_client_id().is_some();
            let pending = nexus_oauth::pending(state);
            let base = json!({
                "ssoAvailable": available,
                "ssoPending": pending,
                "personalKeyFallbackAllowed": false,
                "authMethod": configured.then_some("oauth-pkce"),
                "configured": configured,
                "connected": false
            });
            if !configured {
                return Ok(Some(base));
            }
            match nexus_oauth::access_token(state).and_then(|token| {
                token
                    .ok_or(nexus_oauth::OAuthFailure {
                        code: "NEXUS_AUTH_REQUIRED",
                        message: "Nexus Mods authorization is required.",
                        status: None,
                    })
                    .and_then(|token| nexus_oauth::validate_access_token(state, token))
            }) {
                Ok(user) => Ok(Some(json!({
                    "ssoAvailable": available,
                    "ssoPending": pending,
                    "personalKeyFallbackAllowed": false,
                    "authMethod": "oauth-pkce",
                    "configured": true,
                    "connected": true,
                    "valid": true,
                    "name": user.name.unwrap_or_else(|| "Nexus Mods user".to_owned()),
                    "userId": user.user_id,
                    "premium": user.is_premium.unwrap_or(false),
                    "supporter": user.is_supporter.unwrap_or(false)
                }))),
                Err(failure) => Ok(Some(json!({
                    "ssoAvailable": available,
                    "ssoPending": pending,
                    "personalKeyFallbackAllowed": false,
                    "authMethod": "oauth-pkce",
                    "configured": nexus_oauth::has_tokens(state),
                    "connected": false,
                    "code": failure.code,
                    "error": failure.message,
                    "status": failure.status
                }))),
            }
        }
        "modSources:startNexusSso" => Ok(Some(nexus_oauth::start(app, state))),
        "modSources:cancelNexusSso" => Ok(Some(json!(nexus_oauth::cancel(state)))),
        "modSources:clearNexusKey" => {
            nexus_oauth::clear_tokens(state).map_err(|_| error::internal())?;
            Ok(Some(Value::Null))
        }
        "logoutGamebanana" | "eraseGamebananaCache" => {
            let store = state
                .credentials
                .as_ref()
                .ok_or_else(|| error::unavailable("credentials"))?;
            store
                .clear(CredentialKind::GameBananaCookies)
                .map_err(|_| error::internal())?;
            Ok(Some(Value::Null))
        }
        "loginGamebanana" | "validateGamebananaToken" => Err(error::unavailable(channel)),
        "getModImage" => {
            let uid = data
                .first()
                .and_then(Value::as_str)
                .filter(|uid| !uid.is_empty() && uid.len() <= 256)
                .ok_or_else(|| error::invalid("getModImage"))?;
            let path = legacy_mod_image(state, uid).or_else(|| state.mod_images.get(uid).cloned());
            Ok(Some(json!({"exists": path.is_some(), "path": path})))
        }
        "modSources:validateUrl" => {
            let request = data
                .first()
                .ok_or_else(|| error::invalid("modSources:validateUrl"))?;
            let provider = request
                .get("provider")
                .and_then(Value::as_str)
                .and_then(provider)
                .and_then(ShopProvider::network_provider)
                .ok_or_else(|| error::invalid("modSources:validateUrl"))?;
            let url = request
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("modSources:validateUrl"))?;
            let safe = validate_https_url(provider, url)
                .map_err(|_| error::invalid("modSources:validateUrl"))?;
            Ok(Some(json!({"url":safe.raw,"host":safe.host})))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{legacy_mod_image_from_roots, moddb_catalog};
    use deltamod_network_runtime::ModEntry;

    #[test]
    fn legacy_mod_image_skips_malformed_packet_entries() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deltamod-mod-image-test-{}-{nonce}",
            std::process::id()
        ));
        let broken = root.join("broken");
        let target = root.join("target");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(
            target.join("__deltaID.json"),
            br#"{"uniqueId":"target-uid"}"#,
        )
        .unwrap();
        std::fs::write(target.join("icon.png"), b"\x89PNG\r\n\x1a\n").unwrap();

        let image = legacy_mod_image_from_roots(vec![broken, target], "target-uid");

        std::fs::remove_dir_all(root).unwrap();
        assert!(image
            .as_deref()
            .is_some_and(|value| value.starts_with("data:image/png;base64,")));
    }

    #[test]
    fn moddb_entries_match_the_renderer_catalog_contract() {
        let result = moddb_catalog(
            "undertale",
            vec![ModEntry {
                title: "A recent mod".into(),
                link: "https://www.moddb.com/mods/example/downloads/release".into(),
                published: Some("Mon, 10 Aug 2026 12:00:00 +0000".into()),
                summary: Some("Description".into()),
                image_url: Some("https://media.moddb.com/example.png".into()),
            }],
            None,
        );
        assert_eq!(result["items"].as_array().map(Vec::len), Some(1));
        assert_eq!(result["items"][0]["provider"], "moddb");
        assert_eq!(
            result["items"][0]["sourceUrl"],
            "https://www.moddb.com/mods/example/downloads/release"
        );
        assert_eq!(
            result["catalogUrl"],
            "https://www.moddb.com/games/undertale/downloads"
        );
    }
}
