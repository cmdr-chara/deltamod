use crate::{error, state::AppState};
use deltamod_patching_runtime::{Compatibility, RequiredFile};
use deltamod_tools_runtime::{verify_tool, ToolKind};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};

const MAX_MOD_METADATA_BYTES: u64 = 1024 * 1024;

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

fn packaged_themes(root: &Path) -> Result<Vec<Value>, String> {
    let data_root = root.join("data");
    let mut themes = Vec::new();
    for entry in fs::read_dir(&data_root).map_err(|_| error::internal())? {
        let entry = entry.map_err(|_| error::internal())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| error::internal())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > 256 * 1024
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".theme.json"))
        {
            continue;
        }
        let mut theme: Value =
            serde_json::from_slice(&fs::read(&path).map_err(|_| error::internal())?)
                .map_err(|_| error::internal())?;
        let id = theme.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() || id.len() > 64 {
            return Err(error::internal());
        }
        theme
            .as_object_mut()
            .ok_or_else(error::internal)?
            .insert("builtIn".to_owned(), json!(true));
        themes.push(theme);
    }
    themes.sort_by(|a, b| {
        a.get("id")
            .and_then(Value::as_str)
            .cmp(&b.get("id").and_then(Value::as_str))
    });
    Ok(themes)
}

fn all_themes(state: &AppState, channel: &str) -> Result<Vec<Value>, String> {
    let mut themes = packaged_themes(&state._assets.builtin_theme)?;
    let built_in_ids: BTreeSet<String> = themes
        .iter()
        .filter_map(|theme| theme.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    for custom in state
        .mods_themes
        .themes()
        .list()
        .map_err(|_| runtime_error(channel))?
    {
        if !built_in_ids.contains(&custom.id) {
            themes.push(json!({
                "id": custom.id,
                "name": custom.name,
                "description": custom.description.unwrap_or_else(|| "Custom theme".to_owned()),
                "builtIn": false,
                "background": custom.icon,
                "mainSong": custom.music,
                "musicTrack": "Custom audio",
                "color": custom.color.unwrap_or_else(|| "rgb(205, 68, 81)".to_owned()),
                "soulColor": custom.soul_color.unwrap_or_else(|| "#FF0000".to_owned()),
                "runtimeLayout": true
            }));
        }
    }
    Ok(themes)
}

fn active_theme_path(state: &AppState) -> std::path::PathBuf {
    state
        .data_root
        .root
        .join("runtime")
        .join("active-theme.json")
}

fn active_theme_id(state: &AppState) -> String {
    fs::read(active_theme_path(state))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "base".to_owned())
}

fn directory_size(root: &Path) -> u64 {
    let mut total = 0_u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

fn legacy_mod_records(state: &AppState) -> Vec<Value> {
    let root = state.data_root.root.join("packets");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut records = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(directory) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !directory.is_dir() || directory.file_type().is_symlink() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        let identity_path = path.join("__deltaID.json");
        let manifest_path = path.join("meta.toml");
        let (Ok(identity_meta), Ok(manifest_meta)) = (
            fs::symlink_metadata(&identity_path),
            fs::symlink_metadata(&manifest_path),
        ) else {
            continue;
        };
        if !identity_meta.is_file()
            || identity_meta.file_type().is_symlink()
            || identity_meta.len() > 64 * 1024
            || !manifest_meta.is_file()
            || manifest_meta.file_type().is_symlink()
            || manifest_meta.len() == 0
            || manifest_meta.len() > MAX_MOD_METADATA_BYTES
        {
            continue;
        }
        let Ok(identity) = fs::read(&identity_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .ok_or(())
        else {
            continue;
        };
        let Some(uid) = identity
            .get("uniqueId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
        else {
            continue;
        };
        let Ok(manifest) = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .ok_or(())
        else {
            continue;
        };
        let Some(metadata) = manifest.get("metadata").and_then(toml::Value::as_table) else {
            continue;
        };
        let text = |key: &str, default: &str| {
            metadata
                .get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or(default)
                .to_owned()
        };
        let author = metadata
            .get("author")
            .and_then(|value| serde_json::to_value(value).ok())
            .unwrap_or_else(|| json!(["Unknown"]));
        let needed_files = manifest
            .get("neededFiles")
            .and_then(|value| serde_json::to_value(value).ok())
            .unwrap_or(Value::Null);
        let gamebanana_id = metadata.get("gamebanana_id").and_then(toml::Value::as_str);
        let gamebanana_model = metadata
            .get("gamebanana_model")
            .and_then(toml::Value::as_str);
        records.push(json!({
            "uid": uid,
            "uniqueId": uid,
            "id": uid,
            "folder": folder,
            "name": text("name", &folder),
            "description": text("description", ""),
            "author": author,
            "version": text("version", "Unknown"),
            "game": text("game", "toby.deltarune"),
            "packageID": text("packageID", "und.und.und"),
            "size": ((directory_size(&path) as f64 / (1024.0 * 1024.0)) * 100.0).round() / 100.0,
            "mergeSupport": metadata.get("mergeSupport").and_then(toml::Value::as_bool).unwrap_or(true),
            "variants": Value::Null,
            "new": identity.get("new").and_then(Value::as_bool).unwrap_or(false),
            "gamebanana": {
                "supports": gamebanana_id.is_some() && gamebanana_model.is_some(),
                "id": gamebanana_id,
                "model": gamebanana_model
            },
            "_neededFiles": needed_files
        }));
    }
    records.sort_by(|a, b| {
        a.get("uid")
            .and_then(Value::as_str)
            .cmp(&b.get("uid").and_then(Value::as_str))
    });
    records
}

fn mod_list(state: &AppState, channel: &str) -> Result<Value, String> {
    let mut list = state
        .mods_themes
        .mods()
        .list_json()
        .map_err(|_| runtime_error(channel))?;
    list.as_array_mut()
        .ok_or_else(error::internal)?
        .extend(legacy_mod_records(state));
    let enabled = state
        .preferences
        .lock()
        .map_err(|_| runtime_error(channel))?
        .unique_flags
        .get("HASHCHECKS")
        .copied()
        .unwrap_or(false);
    let records = list.as_array_mut().ok_or_else(error::internal)?;
    let mut requirements = Vec::new();
    for record in records.iter_mut() {
        let object = record.as_object_mut().ok_or_else(error::internal)?;
        let needed = object.remove("_neededFiles").unwrap_or(Value::Null);
        object.insert("isIncompatible".into(), json!(false));
        object.insert("incompatibilityReason".into(), json!(""));
        object.insert("hashDifferentFiles".into(), json!([]));
        if !enabled || needed.is_null() {
            continue;
        }
        let id = object
            .get("uid")
            .and_then(Value::as_str)
            .ok_or_else(error::internal)?
            .to_owned();
        let required = match needed {
            Value::Array(values) => values
                .into_iter()
                .map(|value| {
                    serde_json::from_value(value).unwrap_or(RequiredFile {
                        file: None,
                        checksum: None,
                    })
                })
                .collect(),
            _ => vec![RequiredFile {
                file: None,
                checksum: None,
            }],
        };
        if !required.is_empty() {
            requirements.push((id, required));
        }
    }
    if enabled && !requirements.is_empty() {
        let checked = state.patching.check_required_files(&requirements);
        for record in records {
            let object = record.as_object_mut().ok_or_else(error::internal)?;
            let id = object
                .get("uid")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let compatibility = match &checked {
                Ok(results) => results.get(id).cloned(),
                Err(error)
                    if requirements
                        .iter()
                        .any(|(required_id, _)| required_id == id) =>
                {
                    Some(Compatibility {
                        is_incompatible: true,
                        incompatibility_reason: format!(
                            "Required-file checks could not be completed: {error}"
                        ),
                        hash_different_files: Vec::new(),
                    })
                }
                Err(_) => None,
            };
            if let Some(compatibility) = compatibility {
                object.insert(
                    "isIncompatible".into(),
                    json!(compatibility.is_incompatible),
                );
                object.insert(
                    "incompatibilityReason".into(),
                    json!(compatibility.incompatibility_reason),
                );
                object.insert(
                    "hashDifferentFiles".into(),
                    json!(compatibility.hash_different_files),
                );
            }
        }
    }
    Ok(list)
}

pub fn dispatch(state: &AppState, channel: &str, data: &[Value]) -> Result<Option<Value>, String> {
    match channel {
        "getModListFull" => Ok(Some(mod_list(state, channel)?)),
        "getModList" => {
            let list = mod_list(state, channel)?;
            Ok(Some(json!({"modList": list, "errors": []})))
        }
        "howManyMods" => Ok(Some(json!(
            state
                .mods_themes
                .mods()
                .count()
                .map_err(|_| runtime_error(channel))?
                + legacy_mod_records(state).len()
        ))),
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
        "getThemes" => Ok(Some(json!(all_themes(state, channel)?))),
        "getTheme" => {
            let available: BTreeSet<String> = all_themes(state, channel)?
                .into_iter()
                .filter_map(|theme| theme.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect();
            let active = active_theme_id(state);
            let id = if available.contains(&active) {
                active
            } else {
                "base".to_owned()
            };
            Ok(Some(json!(id)))
        }
        "setTheme" => {
            let id = arg_string(data, 0, "setTheme")?;
            let built_in = packaged_themes(&state._assets.builtin_theme)?
                .iter()
                .any(|theme| theme.get("id").and_then(Value::as_str) == Some(id.as_str()));
            if built_in {
                deltamod_storage_domain::save_json(
                    &active_theme_path(state),
                    &json!({"id": id}),
                    true,
                )
                .map_err(|_| runtime_error(channel))?;
            } else {
                state
                    .mods_themes
                    .themes()
                    .set_active(&id)
                    .map_err(|_| runtime_error(channel))?;
            }
            Ok(Some(json!(true)))
        }
        "renameCustomTheme" => {
            let id = arg_string(data, 0, "renameCustomTheme")?;
            let name = arg_string(data, 1, "renameCustomTheme")?;
            let description = arg_string(data, 2, "renameCustomTheme")?;
            state
                .mods_themes
                .themes()
                .rename(&id, &name, &description)
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
        assert!(state.credentials.is_none());
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
        let record = dispatch(&state, "getModListFull", &[]).unwrap().unwrap();
        assert_eq!(record[0]["isIncompatible"], json!(false));
        assert_eq!(record[0]["incompatibilityReason"], json!(""));
        assert_eq!(record[0]["hashDifferentFiles"], json!([]));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imported_packet_metadata_uses_the_legacy_renderer_shape() {
        let (state, root) = state();
        let packet = state.data_root.root.join("packets").join("example.mod");
        fs::create_dir_all(&packet).unwrap();
        fs::write(
            packet.join("meta.toml"),
            r#"
[metadata]
name = "Example"
version = "1.2"
description = "Imported packet"
author = ["Tester"]
game = "toby.deltarune"
packageID = "example.mod"
gamebanana_id = "123"
gamebanana_model = "Mod"
"#,
        )
        .unwrap();
        fs::write(
            packet.join("__deltaID.json"),
            r#"{"uniqueId":"packet-uid","new":true}"#,
        )
        .unwrap();
        fs::write(packet.join("modding.xml"), "<mod/>").unwrap();
        let list = dispatch(&state, "getModList", &[]).unwrap().unwrap();
        let record = &list["modList"][0];
        assert_eq!(record["uid"], json!("packet-uid"));
        assert_eq!(record["folder"], json!("example.mod"));
        assert_eq!(record["description"], json!("Imported packet"));
        assert_eq!(record["author"], json!(["Tester"]));
        assert_eq!(record["gamebanana"]["supports"], json!(true));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn hash_checks_enrich_each_mod_without_failing_the_list() {
        let (state, root) = state();
        fs::create_dir_all(&state.patching.game_root).unwrap();
        fs::write(state.patching.game_root.join("data.win"), b"data").unwrap();
        state
            .preferences
            .lock()
            .unwrap()
            .unique_flags
            .insert("HASHCHECKS".into(), true);
        for (folder, manifest) in [
            ("good", r#"{"uid":"good","name":"Good","neededFiles":[{"file":"data.win","checksum":"3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"}]}"#.to_owned()),
            ("bad", r#"{"uid":"bad","name":"Bad","neededFiles":[{"file":"../outside","checksum":"0000000000000000000000000000000000000000000000000000000000000000"}]}"#.to_owned()),
        ] {
            let directory = state.data_root.root.join("mods").join(folder);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("manifest.json"), manifest).unwrap();
        }
        let result = dispatch(&state, "getModList", &[]).unwrap().unwrap();
        let records = result["modList"].as_array().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records
                .iter()
                .find(|record| record["uid"] == "good")
                .unwrap()["isIncompatible"],
            false
        );
        assert_eq!(
            records
                .iter()
                .find(|record| record["uid"] == "bad")
                .unwrap()["isIncompatible"],
            true
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn theme_and_profile_channels_are_real_reads() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("deltamod-shell-theme-test-{nonce}"));
        let resource = root.join("resource");
        fs::create_dir_all(resource.join("themes").join("data")).unwrap();
        fs::write(
            resource.join("themes").join("data").join("base.theme.json"),
            r#"{"id":"base","name":"Base","description":"Built in"}"#,
        )
        .unwrap();
        let state = AppState::initialize(root.join("data"), resource).unwrap();
        assert_eq!(
            dispatch(&state, "getTheme", &[]).unwrap(),
            Some(json!("base"))
        );
        let themes = dispatch(&state, "getThemes", &[]).unwrap().unwrap();
        assert_eq!(themes[0]["builtIn"], json!(true));
        assert_eq!(themes[0]["description"], json!("Built in"));
        assert_eq!(
            dispatch(&state, "setTheme", &[json!("base")]).unwrap(),
            Some(json!(true))
        );
        let summary = dispatch(&state, "getOfficialProfileSummary", &[])
            .unwrap()
            .unwrap();
        assert_eq!(summary.get("installed"), Some(&json!(false)));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn every_packaged_theme_round_trips_through_tauri_channels() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("deltamod-shell-catalog-test-{nonce}"));
        let web_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("web");
        let state = AppState::initialize(root.join("data"), web_root).unwrap();
        let themes = dispatch(&state, "getThemes", &[])
            .unwrap()
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let expected_count = fs::read_dir(state._assets.builtin_theme.join("data"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.ends_with(".theme.json"))
            })
            .count();

        assert_eq!(themes.len(), expected_count);
        assert!(themes.len() >= 14);
        for theme in themes {
            let id = theme["id"].as_str().unwrap();
            assert_eq!(theme["builtIn"], json!(true));
            for field in [
                "name",
                "description",
                "background",
                "mainSong",
                "musicTrack",
                "color",
                "soulColor",
            ] {
                assert!(theme[field].as_str().is_some_and(|value| !value.is_empty()));
            }
            assert!(state
                ._assets
                .builtin_theme
                .join("img")
                .join(theme["background"].as_str().unwrap())
                .is_file());
            let background_uri = format!(
                "theme://asset/img/{}",
                theme["background"].as_str().unwrap()
            );
            let background_plan = state.assets.resolve(&background_uri).unwrap();
            assert!(background_plan.path.is_file());
            assert!(background_plan.content_type.starts_with("image/"));
            assert!(state
                ._assets
                .builtin_theme
                .join("mus")
                .join(theme["mainSong"].as_str().unwrap())
                .is_file());
            let music_uri = format!("theme://asset/mus/{}", theme["mainSong"].as_str().unwrap());
            let music_plan = state.assets.resolve(&music_uri).unwrap();
            assert!(music_plan.path.is_file());
            assert!(music_plan.content_type.starts_with("audio/"));
            if let Some(video) = theme.get("backgroundVideo").and_then(Value::as_str) {
                assert!(state
                    ._assets
                    .builtin_theme
                    .join("video")
                    .join(video)
                    .is_file());
            }

            assert_eq!(
                dispatch(&state, "setTheme", &[json!(id)]).unwrap(),
                Some(json!(true))
            );
            assert_eq!(dispatch(&state, "getTheme", &[]).unwrap(), Some(json!(id)));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn custom_theme_round_trips_metadata_assets_and_description() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("deltamod-shell-custom-theme-{nonce}"));
        let web_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("web");
        let state = AppState::initialize(root.join("data"), web_root).unwrap();
        let theme_root = state.data_root.root.join("themes").join("custom_test");
        fs::create_dir_all(&theme_root).unwrap();
        fs::write(
            theme_root.join("theme.json"),
            r##"{"id":"custom_test","name":"Custom","description":"Original","builtIn":false,"icon":"background.png","music":"music.ogg","color":"#143E80","soulColor":"#003CFF"}"##,
        )
        .unwrap();
        fs::write(theme_root.join("background.png"), b"test image").unwrap();
        fs::write(theme_root.join("music.ogg"), b"test audio").unwrap();

        let themes = dispatch(&state, "getThemes", &[]).unwrap().unwrap();
        let custom = themes
            .as_array()
            .unwrap()
            .iter()
            .find(|theme| theme["id"] == "custom_test")
            .unwrap();
        assert_eq!(custom["background"], "background.png");
        assert_eq!(custom["mainSong"], "music.ogg");
        assert_eq!(custom["color"], "#143E80");
        assert_eq!(custom["soulColor"], "#003CFF");
        assert_eq!(custom["runtimeLayout"], true);
        assert!(state
            .assets
            .resolve("theme://asset/custom_test/background.png")
            .is_ok());
        assert_eq!(
            dispatch(&state, "setTheme", &[json!("custom_test")]).unwrap(),
            Some(json!(true))
        );
        assert_eq!(
            dispatch(&state, "getTheme", &[]).unwrap(),
            Some(json!("custom_test"))
        );
        assert_eq!(
            dispatch(
                &state,
                "renameCustomTheme",
                &[json!("custom_test"), json!("Renamed"), json!("Updated")]
            )
            .unwrap(),
            Some(json!(true))
        );
        let stored: Value =
            serde_json::from_slice(&fs::read(theme_root.join("theme.json")).unwrap()).unwrap();
        assert_eq!(stored["name"], "Renamed");
        assert_eq!(stored["description"], "Updated");
        let _ = fs::remove_dir_all(root);
    }
}
