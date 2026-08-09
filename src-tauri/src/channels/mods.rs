use crate::{error, state::AppState};
use deltamod_credentials_adapter::CredentialKind;
use deltamod_network_domain::{validate_https_url, BrowseRequest, Provider};
use deltamod_network_runtime::ModDb;
use deltamod_tauri_os_adapters::validate_https_external;
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

fn provider(value: &str) -> Option<Provider> {
    match value {
        "gamebanana" => Some(Provider::GameBanana),
        "nexus" => Some(Provider::Nexus),
        "moddb" => Some(Provider::ModDb),
        _ => None,
    }
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
        "modSources:getProviders" => Ok(Some(json!([
            {"id":"gamebanana","name":"GameBanana","available":false},
            {"id":"nexus","name":"Nexus Mods","available":false,"requiresAuthentication":true},
            {"id":"moddb","name":"ModDB (recent)","available":true}
        ]))),
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
            let domain = request
                .get("domain")
                .and_then(Value::as_str)
                .map(str::to_owned);
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
                Ok(value) => Ok(Some(json!({"ok":true,"result":value}))),
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
            let path = state.mod_images.get(uid);
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
