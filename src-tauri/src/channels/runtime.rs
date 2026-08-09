use crate::{error, state::AppState};
use deltamod_tools_runtime::{verify_tool, ToolKind};
use serde_json::{json, Value};
use std::fs;

fn runtime_error(channel: &str) -> String {
    let _ = channel;
    error::internal()
}

fn arg_string(data: &[Value], index: usize, channel: &'static str) -> Result<String, String> {
    data.get(index)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| error::invalid(channel))
}

pub fn dispatch(state: &AppState, channel: &str, data: &[Value]) -> Result<Option<Value>, String> {
    match channel {
        "getModListFull" => Ok(Some(
            state
                .mods_themes
                .mods()
                .list_json()
                .map_err(|_| runtime_error(channel))?,
        )),
        "getModList" => {
            let list = state
                .mods_themes
                .mods()
                .list_json()
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(json!({"modList": list, "errors": []})))
        }
        "howManyMods" => Ok(Some(json!(state
            .mods_themes
            .mods()
            .count()
            .map_err(|_| runtime_error(channel))?))),
        "getModState" => {
            let uid = arg_string(data, 0, "getModState")?;
            let state_value = state
                .mods_themes
                .mods()
                .state()
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(json!(state_value
                .enabled
                .iter()
                .any(|item| item.as_str() == uid))))
        }
        "toggleModState" => {
            let uid = arg_string(data, 0, "toggleModState")?;
            let enabled = data
                .get(1)
                .and_then(Value::as_bool)
                .ok_or_else(|| error::invalid("toggleModState"))?;
            state
                .mods_themes
                .mods()
                .toggle(&uid, enabled)
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(Value::Null))
        }
        "setModVariant" => {
            let uid = arg_string(data, 0, "setModVariant")?;
            let variant = arg_string(data, 1, "setModVariant")?;
            state
                .mods_themes
                .mods()
                .set_variant(&uid, &variant)
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(Value::Null))
        }
        "removeMod" => {
            let uid = arg_string(data, 0, "removeMod")?;
            state
                .mods_themes
                .mods()
                .remove(&uid)
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(Value::Null))
        }
        "getThemes" => Ok(Some(
            serde_json::to_value(
                state
                    .mods_themes
                    .themes()
                    .list()
                    .map_err(|_| runtime_error(channel))?,
            )
            .map_err(|_| error::internal())?,
        )),
        "getTheme" => {
            let id = state
                .mods_themes
                .themes()
                .active()
                .map(|theme| theme.id)
                .unwrap_or_else(|_| "base".to_owned());
            Ok(Some(json!(id)))
        }
        "setTheme" => {
            let id = arg_string(data, 0, "setTheme")?;
            state
                .mods_themes
                .themes()
                .set_active(&id)
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(json!(true)))
        }
        "renameCustomTheme" => {
            let id = arg_string(data, 0, "renameCustomTheme")?;
            let name = arg_string(data, 1, "renameCustomTheme")?;
            state
                .mods_themes
                .themes()
                .rename(&id, &name)
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(json!(true)))
        }
        "deleteCustomTheme" => {
            let id = arg_string(data, 0, "deleteCustomTheme")?;
            state
                .mods_themes
                .themes()
                .delete(&id)
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(json!(true)))
        }
        "getSponsor" => {
            let path = state.data_root.root.join("_sponsor");
            let value = fs::read_to_string(path).unwrap_or_else(|_| "cd".to_owned());
            Ok(Some(json!(value
                .trim()
                .chars()
                .take(64)
                .collect::<String>())))
        }
        "fetchSharedVariable" => {
            let name = arg_string(data, 0, "fetchSharedVariable")?;
            if !matches!(name.as_str(), "errorMessage" | "gb1click") {
                return Err(error::invalid("fetchSharedVariable"));
            }
            let value = state
                .mods_themes
                .shared()
                .get()
                .map_err(|_| runtime_error(channel))?;
            Ok(Some(value.get(&name).cloned().unwrap_or(Value::Null)))
        }
        "getOfficialProfileSummary" => Ok(Some(
            state
                .profile_runtime
                .get_official_profile_summary()
                .map_err(|_| runtime_error(channel))?,
        )),
        "undertaleModTool:status" => {
            let platform = crate::platform_name();
            let arch = match std::env::consts::ARCH {
                "x86_64" => "x64",
                "aarch64" => "arm64",
                other => other,
            };
            let path = ToolKind::UndertaleModCli
                .packaged_path(&state.patching.tools_root, platform, arch)
                .ok();
            let verified = path
                .as_deref()
                .and_then(|candidate| verify_tool(candidate, ToolKind::UndertaleModCli, None).ok())
                .is_some();
            Ok(Some(json!({
                "available": verified,
                "provenance": if verified { "packaged" } else { "unavailable" },
                "launchable": false
            })))
        }
        "fireUpdate" => Ok(Some(json!({
            "state": "idle",
            "available": false,
            "supported": false
        }))),
        "cancelOfficialProfileImport" => {
            let value = data
                .first()
                .ok_or_else(|| error::invalid("cancelOfficialProfileImport"))?;
            if let Some(id) = value.as_u64() {
                state
                    .profile_runtime
                    .cancel(id)
                    .map_err(|_| runtime_error(channel))?;
                Ok(Some(json!(true)))
            } else if value.as_str().is_some() {
                // Legacy imports use UUIDs, while the filesystem runtime uses numeric IDs.
                // No UUID-backed import is started by this shell, so the legacy no-op result
                // is the truthful recovery response.
                Ok(Some(json!(false)))
            } else {
                Err(error::invalid("cancelOfficialProfileImport"))
            }
        }
        "getInstallations" => Ok(Some(
            state
                .profile_runtime
                .legacy_installations()
                .map_err(|_| runtime_error(channel))?,
        )),
        "getSystemIndex" => Ok(Some(
            state
                .profile_runtime
                .legacy_system_index()
                .map_err(|_| runtime_error(channel))?,
        )),
        "getMaxExistingIndex" | "isCurrentIndexSteam" => {
            let profile = state.profile().map_err(|_| error::internal())?;
            if channel == "getMaxExistingIndex" {
                Ok(Some(json!(profile
                    .installations
                    .iter()
                    .filter_map(|x| x.index)
                    .max()
                    .unwrap_or(0))))
            } else {
                Ok(Some(json!(profile.installations.iter().any(|x| x.index
                    == profile.current_index
                    && x.steam == Some(true)))))
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn state() -> (AppState, std::path::PathBuf) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("deltamod-shell-test-{nonce}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let resource = root.join("resource");
        fs::create_dir_all(&resource).unwrap();
        (
            AppState::initialize(root.join("data"), resource).unwrap(),
            root,
        )
    }

    #[test]
    fn mod_channels_preserve_legacy_shapes() {
        let (state, root) = state();
        let mod_dir = state.data_root.root.join("mods").join("folder");
        fs::create_dir_all(&mod_dir).unwrap();
        fs::write(
            mod_dir.join("manifest.json"),
            r#"{"uid":"mod-a","name":"A","variants":[]}"#,
        )
        .unwrap();
        assert!(dispatch(&state, "getModList", &[])
            .unwrap()
            .unwrap()
            .get("modList")
            .is_some());
        assert_eq!(
            dispatch(&state, "howManyMods", &[]).unwrap(),
            Some(json!(1))
        );
        assert_eq!(
            dispatch(&state, "getModState", &[json!("mod-a")]).unwrap(),
            Some(json!(false))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn theme_and_profile_channels_are_real_reads() {
        let (state, root) = state();
        let theme = state.data_root.root.join("themes").join("base");
        fs::create_dir_all(&theme).unwrap();
        fs::write(
            theme.join("theme.json"),
            r#"{"id":"base","name":"Base","builtIn":false,"icon":null,"music":null}"#,
        )
        .unwrap();
        assert_eq!(
            dispatch(&state, "getTheme", &[]).unwrap(),
            Some(json!("base"))
        );
        assert!(dispatch(&state, "getThemes", &[])
            .unwrap()
            .unwrap()
            .is_array());
        let summary = dispatch(&state, "getOfficialProfileSummary", &[])
            .unwrap()
            .unwrap();
        assert_eq!(summary.get("installed"), Some(&json!(false)));
        let _ = fs::remove_dir_all(root);
    }
}
