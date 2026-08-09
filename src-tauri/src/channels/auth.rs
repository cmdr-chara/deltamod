use crate::{error, state::AppState};
use deltamod_credentials_adapter::CredentialKind;
use deltamod_network_runtime::GameBanana;
use serde_json::{json, Value};

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
            let api = GameBanana {
                client: &state.network,
                token: Some(token),
            };
            let result: Value = match with_gamebanana(state, api.validate()) {
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
}
