use crate::{error, state::AppState};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use deltamod_credentials_adapter::CredentialKind;
use deltamod_network_domain::{validate_https_url, BrowseRequest, Provider};
use deltamod_network_runtime::{ModDb, ModEntry};
use deltamod_tauri_os_adapters::validate_https_external;
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

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
                return Ok(Some(
                    json!({"ok":false,"error":{"code":"NEXUS_SSO_REQUIRED","message":"Nexus Mods single sign-on is required."}}),
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
            let configured = state
                .credentials
                .as_ref()
                .and_then(|store| store.load(CredentialKind::NexusSsoKey).ok())
                .flatten()
                .is_some();
            Ok(Some(json!({
                "ssoAvailable": false,
                "ssoPending": false,
                "personalKeyFallbackAllowed": false,
                "authMethod": configured.then_some("sso"),
                "configured": configured,
                "connected": configured
            })))
        }
        "modSources:startNexusSso" => Ok(Some(json!({
            "ok": false,
            "error": {"code":"NEXUS_SSO_UNAVAILABLE","message":"Nexus Mods SSO is not available in this build."}
        }))),
        "modSources:cancelNexusSso" => Err(error::unavailable("modSources:cancelNexusSso")),
        "modSources:clearNexusKey" => {
            let store = state
                .credentials
                .as_ref()
                .ok_or_else(|| error::unavailable("credentials"))?;
            store
                .clear(CredentialKind::NexusSsoKey)
                .map_err(|_| error::internal())?;
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
