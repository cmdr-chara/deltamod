use crate::{channels::nexus_oauth, error, state::AppState};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use deltamod_credentials_adapter::CredentialKind;
use deltamod_network_domain::{validate_https_url, BrowseRequest, Provider};
use deltamod_network_runtime::{ModDb, ModEntry};
use deltamod_tauri_os_adapters::validate_https_external;
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

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

fn legacy_mod_image(state: &AppState, uid: &str) -> Option<String> {
    let entries = std::fs::read_dir(state.data_root.root.join("packets")).ok()?;
    for entry in entries.flatten() {
        let root = entry.path();
        let directory = std::fs::symlink_metadata(&root).ok()?;
        if !directory.is_dir() || directory.file_type().is_symlink() {
            continue;
        }
        let identity_path = root.join("__deltaID.json");
        let identity_meta = std::fs::symlink_metadata(&identity_path).ok()?;
        if !identity_meta.is_file()
            || identity_meta.file_type().is_symlink()
            || identity_meta.len() > 64 * 1024
        {
            continue;
        }
        let identity: Value = serde_json::from_slice(&std::fs::read(identity_path).ok()?).ok()?;
        if identity.get("uniqueId").and_then(Value::as_str) != Some(uid) {
            continue;
        }
        let icon_path = root.join("icon.png");
        let icon_meta = std::fs::symlink_metadata(&icon_path).ok()?;
        if !icon_meta.is_file()
            || icon_meta.file_type().is_symlink()
            || icon_meta.len() == 0
            || icon_meta.len() > 4 * 1024 * 1024
        {
            return None;
        }
        let icon = std::fs::read(icon_path).ok()?;
        if !icon.starts_with(b"\x89PNG\r\n\x1a\n") {
            return None;
        }
        return Some(format!("data:image/png;base64,{}", STANDARD.encode(icon)));
    }
    None
}

fn provider(value: &str) -> Option<Provider> {
    match value {
        "gamebanana" => Some(Provider::GameBanana),
        "nexus" => Some(Provider::Nexus),
        "moddb" => Some(Provider::ModDb),
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
            json!({
                "provider": "moddb",
                "id": id,
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
) -> Result<Value, String> {
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
        .map_err(|_| error::internal())?
        .block_on(state.network.nexus_graphql(body, access_token.as_deref()))
        .map_err(|_| "MOD_SOURCE_BROWSE_FAILED".to_owned())?;
    if payload
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err("MOD_SOURCE_BROWSE_FAILED".to_owned());
    }
    let page = payload
        .pointer("/data/mods")
        .ok_or_else(|| "MOD_SOURCE_BROWSE_FAILED".to_owned())?;
    let nodes = page
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| "MOD_SOURCE_BROWSE_FAILED".to_owned())?;
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

fn open_provider_url(app: &AppHandle, raw: &str, provider: Provider) -> Result<Value, String> {
    let hosts: &[&str] = match provider {
        Provider::GameBanana => &["gamebanana.com"],
        Provider::Nexus => &["nexusmods.com", "www.nexusmods.com"],
        Provider::ModDb => &["moddb.com", "www.moddb.com"],
    };
    let url = validate_https_external(raw, hosts).map_err(|_| error::invalid("modSources:open"))?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
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
            let provider =
                provider(provider_name).ok_or_else(|| error::invalid("modSources:browse"))?;
            let game =
                current_game(state).ok_or_else(|| error::unavailable("modSources:browse"))?;
            let domain = mapped_source(&game, provider).map(str::to_owned);
            let query = request
                .get("query")
                .and_then(Value::as_str)
                .map(str::to_owned);
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
                return Ok(Some(
                    match nexus_catalog(
                        state,
                        &domain,
                        query.as_deref(),
                        &sort,
                        offset,
                        count,
                        None,
                    ) {
                        Ok(result) => json!({"ok": true, "result": result}),
                        Err(_) => json!({
                            "ok": false,
                            "error": {"code":"MOD_SOURCE_BROWSE_FAILED","message":"The Nexus Mods catalogue could not be loaded."}
                        }),
                    },
                ));
            }
            if provider == Provider::GameBanana {
                return Ok(Some(
                    json!({"ok":false,"error":{"code":"MOD_SOURCE_LEGACY_PROVIDER","message":"GameBanana uses the compatibility catalogue."}}),
                ));
            }
            let slug = domain.ok_or_else(|| error::invalid("modSources:browse"))?;
            if slug.is_empty()
                || slug.len() > 80
                || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(error::invalid("modSources:browse"));
            }
            let url = format!("https://rss.moddb.com/games/{slug}/downloads/feed/rss.xml");
            let client = state.network.clone();
            let result = std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| "network runtime unavailable".to_owned())?;
                runtime
                    .block_on(ModDb { client: &client }.browse(&url))
                    .map(|response| response.value)
                    .map_err(|error| error.to_string())
            })
            .join()
            .map_err(|_| error::internal())?;
            match result {
                Ok(value) => Ok(Some(json!({
                    "ok": true,
                    "result": moddb_catalog(&slug, value, query.as_deref())
                }))),
                Err(message) => Ok(Some(
                    json!({"ok":false,"error":{"code":"MOD_SOURCE_BROWSE_FAILED","message":message.chars().take(256).collect::<String>()}}),
                )),
            }
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
                    .ok_or_else(|| nexus_oauth::OAuthFailure {
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
    use super::moddb_catalog;
    use deltamod_network_runtime::ModEntry;

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
