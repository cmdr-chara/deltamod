use crate::{error, state::AppState};
use deltamod_credentials_adapter::{CredentialKind, Secret};
use deltamod_network_runtime::GameBanana;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{
    webview::{Cookie, NewWindowResponse, PageLoadEvent},
    AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

const GAMEBANANA_LOGIN_URL: &str = "https://gamebanana.com/members/account/login";
const GAMEBANANA_ORIGIN: &str = "https://gamebanana.com/";
const LOGIN_WINDOW_LABEL: &str = "gamebanana-login";

type LoginResult = Result<Value, String>;
type LoginSender = Arc<Mutex<Option<tokio::sync::oneshot::Sender<LoginResult>>>>;

fn gamebanana_url_allowed(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some_and(gamebanana_domain_allowed)
}

fn gamebanana_domain_allowed(domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    domain == "gamebanana.com" || domain.ends_with(".gamebanana.com")
}

fn serialize_gamebanana_cookies(cookies: &[Cookie<'_>]) -> String {
    cookies
        .iter()
        .filter(|cookie| {
            !cookie.name().is_empty() && cookie.domain().is_none_or(gamebanana_domain_allowed)
        })
        .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
        .collect::<Vec<_>>()
        .join("; ")
}

fn complete_login(app: &AppHandle, sender: &LoginSender, result: LoginResult) {
    if let Ok(mut sender) = sender.lock() {
        if let Some(sender) = sender.take() {
            let _ = sender.send(result);
        }
    }
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = window.close();
    }
}

fn validated_ui_config(state: &AppState, token: String) -> Result<Value, String> {
    let api = GameBanana {
        client: &state.network,
        token: Some(token),
    };
    let config: Value = with_gamebanana(state, api.validate())?;
    if config
        .get("_idMemberRow")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        == 0
    {
        return Err("GAMEBANANA_LOGIN_VALIDATION_FAILED".to_owned());
    }
    Ok(config)
}

fn stored_ui_config(state: &AppState) -> Result<Value, String> {
    validated_ui_config(state, token(state)?)
}

pub async fn login(app: AppHandle) -> Result<Value, String> {
    let state = app.state::<AppState>();
    credentials(&state)?;
    if state
        .gamebanana_login_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("GAMEBANANA_LOGIN_IN_PROGRESS".to_owned());
    }

    let result = start_login(&app).await;
    state
        .gamebanana_login_active
        .store(false, Ordering::Release);
    result
}

async fn start_login(app: &AppHandle) -> Result<Value, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let page_sender = Arc::clone(&sender);
    let page_app = app.clone();
    let navigation_sender = Arc::clone(&sender);
    let navigation_app = app.clone();
    let login_check_started = Arc::new(AtomicBool::new(false));
    let page_login_check_started = Arc::clone(&login_check_started);
    let data_directory = app
        .path()
        .app_data_dir()
        .map_err(|_| error::internal())?
        .join("gamebanana-login-webview");
    let blank = Url::parse("about:blank").map_err(|_| error::internal())?;

    let window = WebviewWindowBuilder::new(app, LOGIN_WINDOW_LABEL, WebviewUrl::External(blank))
        .title("Sign in to GameBanana")
        .inner_size(800.0, 600.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .data_directory(data_directory)
        .on_navigation(move |url| {
            let allowed = url.as_str() == "about:blank" || gamebanana_url_allowed(url);
            if !allowed {
                let app = navigation_app.clone();
                let sender = Arc::clone(&navigation_sender);
                std::thread::spawn(move || {
                    complete_login(
                        &app,
                        &sender,
                        Err("GAMEBANANA_LOGIN_NAVIGATION_BLOCKED".to_owned()),
                    );
                });
            }
            allowed
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished
                || !gamebanana_url_allowed(payload.url())
                || payload.url().path().starts_with("/members/account")
                || page_login_check_started.swap(true, Ordering::AcqRel)
            {
                return;
            }
            let sender = Arc::clone(&page_sender);
            let app = page_app.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let origin = Url::parse(GAMEBANANA_ORIGIN).map_err(|_| error::internal())?;
                    let cookies = window
                        .cookies_for_url(origin)
                        .map_err(|_| "GAMEBANANA_LOGIN_COOKIE_READ_FAILED".to_owned())?;
                    let cookie_header = serialize_gamebanana_cookies(&cookies);
                    window
                        .clear_all_browsing_data()
                        .map_err(|_| "GAMEBANANA_LOGIN_CLEAR_FAILED".to_owned())?;
                    let secret = Secret::new(cookie_header)
                        .map_err(|_| "GAMEBANANA_LOGIN_COOKIE_INVALID".to_owned())?;
                    let state = app.state::<AppState>();
                    validated_ui_config(&state, secret.expose().to_owned())?;
                    credentials(&state)?
                        .store(CredentialKind::GameBananaCookies, secret)
                        .map_err(|_| "CREDENTIALS_UNAVAILABLE".to_owned())?;
                    Ok(json!(true))
                })();
                complete_login(&app, &sender, result);
            });
        })
        .build()
        .map_err(|_| error::internal())?;

    let closed_sender = Arc::clone(&sender);
    let closing_window = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let sender = Arc::clone(&closed_sender);
            let window = closing_window.clone();
            std::thread::spawn(move || {
                let _ = window.clear_all_browsing_data();
                if let Ok(mut sender) = sender.lock() {
                    if let Some(sender) = sender.take() {
                        let _ = sender.send(Ok(json!(false)));
                    }
                }
                let _ = window.destroy();
            });
        } else if matches!(event, WindowEvent::Destroyed) {
            if let Ok(mut sender) = closed_sender.lock() {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(Ok(json!(false)));
                }
            }
        }
    });
    if window.clear_all_browsing_data().is_err() {
        let _ = window.close();
        return Err("GAMEBANANA_LOGIN_CLEAR_FAILED".to_owned());
    }
    let login_url = Url::parse(GAMEBANANA_LOGIN_URL).map_err(|_| error::internal())?;
    if window.navigate(login_url).is_err() {
        let _ = window.close();
        return Err("GAMEBANANA_LOGIN_NAVIGATION_FAILED".to_owned());
    }

    receiver
        .await
        .map_err(|_| "GAMEBANANA_LOGIN_CANCELLED".to_owned())?
}

fn credentials(
    state: &AppState,
) -> Result<
    &deltamod_credentials_adapter::CredentialStore<deltamod_credentials_adapter::KeyringBackend>,
    String,
> {
    state
        .credentials
        .as_ref()
        .ok_or_else(|| "CREDENTIALS_UNAVAILABLE".to_owned())
}

fn token(state: &AppState) -> Result<String, String> {
    credentials(state)?
        .load(CredentialKind::GameBananaCookies)
        .map_err(|_| "CREDENTIALS_UNAVAILABLE".to_owned())?
        .map(|secret| secret.expose().to_owned())
        .ok_or_else(|| "CREDENTIALS_NOT_FOUND".to_owned())
}

fn id(value: Option<&Value>, channel: &'static str) -> Result<u64, String> {
    value
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|id| *id > 0)
        .ok_or_else(|| error::invalid(channel))
}

fn string<'a>(value: Option<&'a Value>, channel: &'static str) -> Result<&'a str, String> {
    value
        .and_then(Value::as_str)
        .ok_or_else(|| error::invalid(channel))
}

fn escape_comment(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.chars().count() > 10_000 || value.chars().any(|c| c == '\0')
    {
        return Err(error::invalid("leaveCommentGamebanana"));
    }
    Ok(value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace(['\r', '\n'], "<br>"))
}

fn with_gamebanana<T>(
    state: &AppState,
    operation: impl std::future::Future<Output = Result<T, deltamod_network_runtime::RuntimeError>>,
) -> Result<T, String> {
    state
        .network_runtime
        .lock()
        .map_err(|_| error::internal())?
        .block_on(operation)
        .map_err(|_| "GAMEBANANA_REQUEST_FAILED".to_owned())
}

pub fn dispatch(state: &AppState, channel: &str, data: &[Value]) -> Result<Option<Value>, String> {
    match channel {
        "logoutGamebanana" => {
            credentials(state)?
                .clear(CredentialKind::GameBananaCookies)
                .map_err(|_| "CREDENTIALS_UNAVAILABLE".to_owned())?;
            Ok(Some(json!(true)))
        }
        "modSources:clearNexusKey" => {
            credentials(state)?
                .clear(CredentialKind::NexusSsoKey)
                .map_err(|_| "CREDENTIALS_UNAVAILABLE".to_owned())?;
            Ok(Some(json!(true)))
        }
        "validateGamebananaToken" => {
            let Ok(token) = token(state) else {
                return Ok(Some(json!(false)));
            };
            let result = match validated_ui_config(state, token) {
                Ok(result) => result,
                Err(_) => return Ok(Some(json!(false))),
            };
            Ok(Some(json!(
                result
                    .get("_idMemberRow")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0
            )))
        }
        "getGamebananaPic" => {
            let Ok(config) = stored_ui_config(state) else {
                return Ok(Some(Value::Null));
            };
            Ok(Some(
                config.get("_sAvatarUrl").cloned().unwrap_or(Value::Null),
            ))
        }
        "getGamebananaID" => {
            let Ok(config) = stored_ui_config(state) else {
                return Ok(Some(json!(0)));
            };
            Ok(Some(
                config.get("_idMemberRow").cloned().unwrap_or(Value::Null),
            ))
        }
        "getGamebananaUserinfo" => {
            let Ok(config) = stored_ui_config(state) else {
                return Ok(Some(json!({"loggedIn": false})));
            };
            let member_id = config
                .get("_idMemberRow")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let api = GameBanana {
                client: &state.network,
                token: None,
            };
            let profile: Value = match with_gamebanana(
                state,
                api.user(&format!(
                    "https://gamebanana.com/apiv11/Member/{member_id}/ProfilePage"
                )),
            ) {
                Ok(profile) => profile,
                Err(_) => return Ok(Some(json!({"loggedIn": false}))),
            };
            let mut profile = profile.as_object().cloned().unwrap_or_default();
            profile.insert("loggedIn".to_owned(), json!(true));
            Ok(Some(Value::Object(profile)))
        }
        "leaveCommentGamebanana" => {
            let target = id(data.first(), "leaveCommentGamebanana")?;
            let comment = escape_comment(string(data.get(1), "leaveCommentGamebanana")?)?;
            let model = string(data.get(2), "leaveCommentGamebanana")?;
            let token = token(state)?;
            let api = GameBanana {
                client: &state.network,
                token: Some(token),
            };
            Ok(Some(json!(with_gamebanana(
                state,
                api.leave_comment(target, model, &comment)
            )?)))
        }
        "gbLikeMod" => {
            let model = string(data.first(), "gbLikeMod")?;
            let target = id(data.get(1), "gbLikeMod")?;
            let token = token(state)?;
            let api = GameBanana {
                client: &state.network,
                token: Some(token),
            };
            let response = with_gamebanana(state, api.like_target(model, target))?;
            Ok(Some(
                json!({"status": response.status, "data": response.data}),
            ))
        }
        "gamebanana_getCollections" => {
            let token = token(state)?;
            let api = GameBanana {
                client: &state.network,
                token: Some(token),
            };
            let result: Value = with_gamebanana(state, api.list_collections())?;
            let collections = result
                .get("_aAllCollections")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(Some(Value::Array(
                collections
                    .into_iter()
                    .map(|item| {
                        json!({
                            "id": item.get("_idRow").cloned().unwrap_or(Value::Null),
                            "name": item.get("_sName").cloned().unwrap_or(Value::Null)
                        })
                    })
                    .collect(),
            )))
        }
        "gamebanana_createCollection" => {
            let name = string(data.first(), "gamebanana_createCollection")?;
            let token = token(state)?;
            let api = GameBanana {
                client: &state.network,
                token: Some(token),
            };
            let result: Value = with_gamebanana(state, api.create_collection(name))?;
            let success = result.get("_sStatus").and_then(Value::as_str) == Some("SUCCESS");
            Ok(Some(
                json!({"id": result.get("_idRow").cloned().unwrap_or(Value::Null), "success": success, "error": if success { Value::Null } else { result }}),
            ))
        }
        "gamebanana_deleteCollection" => {
            let collection = id(data.first(), "gamebanana_deleteCollection")?;
            let token = token(state)?;
            let api = GameBanana {
                client: &state.network,
                token: Some(token),
            };
            with_gamebanana(state, api.delete_collection_status(collection))?;
            Ok(Some(json!({"success": true, "error": null})))
        }
        "gamebanana_importToCollection" => {
            let collection = id(data.first(), "gamebanana_importToCollection")?;
            let mods = data
                .get(1)
                .and_then(Value::as_array)
                .ok_or_else(|| error::invalid("gamebanana_importToCollection"))?;
            if mods.len() > 256 {
                return Err(error::invalid("gamebanana_importToCollection"));
            }
            let token = token(state)?;
            let mut skipped = Vec::new();
            for item in mods {
                let target = id(item.get("id"), "gamebanana_importToCollection")?;
                let model = string(item.get("model"), "gamebanana_importToCollection")?;
                let api = GameBanana {
                    client: &state.network,
                    token: Some(token.clone()),
                };
                let result: Value =
                    with_gamebanana(state, api.add_to_collection(collection, target, model))?;
                if result.get("_sStatus").and_then(Value::as_str) != Some("SUCCESS") {
                    skipped.push(json!({"name": item.get("name").cloned().unwrap_or(Value::Null), "pid": item.get("pid").cloned().unwrap_or(Value::Null), "reason": "Failed to add to backup (API error)", "api": result}));
                }
            }
            Ok(Some(json!({"done": true, "skippedMods": skipped})))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_are_bounded_and_html_escaped() {
        assert_eq!(escape_comment("a<&\nb").unwrap(), "a&lt;&amp;<br>b");
        assert!(escape_comment("").is_err());
    }

    #[test]
    fn login_urls_and_cookie_domains_are_strict() {
        assert!(gamebanana_url_allowed(
            &Url::parse("https://gamebanana.com/members/account/login").unwrap()
        ));
        assert!(gamebanana_url_allowed(
            &Url::parse("https://api.gamebanana.com/path").unwrap()
        ));
        assert!(!gamebanana_url_allowed(
            &Url::parse("http://gamebanana.com/path").unwrap()
        ));
        assert!(!gamebanana_url_allowed(
            &Url::parse("https://gamebanana.com.example.invalid/path").unwrap()
        ));
        assert!(!gamebanana_url_allowed(
            &Url::parse("https://user@gamebanana.com/path").unwrap()
        ));
    }

    #[test]
    fn cookie_serialization_accepts_host_only_and_filters_foreign_domains() {
        let mut accepted = Cookie::new("session", "secret");
        accepted.set_domain(".GameBanana.com");
        let mut subdomain = Cookie::new("prefs", "dark");
        subdomain.set_domain("www.gamebanana.com");
        let host_only = Cookie::new("host", "value");
        let mut foreign = Cookie::new("foreign", "secret");
        foreign.set_domain("gamebanana.com.example.invalid");
        let mut empty = Cookie::new("empty", "");
        empty.set_domain("gamebanana.com");
        assert_eq!(
            serialize_gamebanana_cookies(&[accepted, subdomain, host_only, foreign, empty]),
            "session=secret; prefs=dark; host=value; empty="
        );
    }
}
