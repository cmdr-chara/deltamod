use crate::{error, state::AppState};
use deltamod_installations_domain::{Edition, GamePlatform, InstallationId, Ownership};
use deltamod_profile_install_runtime::{PatchPlanInput, ProgressEvent, Runtime as ProfileRuntime};
use deltamod_tauri_os_adapters::{DialogBackend, DialogRequest, ValidatedFolder};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;

const MAX_INSTALLATION_INDEX: u32 = 255;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformDefinition {
    executable: String,
    #[serde(default)]
    data_files: Vec<String>,
    bundle: Option<String>,
}

fn legacy_index(data: &[Value], channel: &'static str) -> Result<u32, String> {
    let raw = data.first().ok_or_else(|| error::invalid(channel))?;
    let parsed = match raw {
        Value::String(value) if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
            value.parse::<u32>().ok()
        }
        Value::Number(value) => value.as_u64().and_then(|value| u32::try_from(value).ok()),
        _ => None,
    };
    parsed
        .filter(|index| *index <= MAX_INSTALLATION_INDEX)
        .ok_or_else(|| error::invalid(channel))
}

fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && !Path::new(value).is_absolute()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

fn game_definition(root: &Path, id: &str, channel: &'static str) -> Result<Value, String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(error::invalid(channel));
    }
    let path = root.join("games").join(format!("{id}.json"));
    let metadata = fs::symlink_metadata(&path).map_err(|_| error::invalid(channel))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(error::invalid(channel));
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| error::internal())?)
        .map_err(|_| error::internal())
}

fn platform_definitions(game: &Value) -> BTreeMap<String, PlatformDefinition> {
    let mut definitions: BTreeMap<String, PlatformDefinition> = game
        .get("platforms")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    if !definitions.contains_key("win32") {
        if let Some(executable) = game.get("exeName").and_then(Value::as_str) {
            definitions.insert(
                "win32".into(),
                PlatformDefinition {
                    executable: executable.into(),
                    data_files: vec!["data.win".into()],
                    bundle: None,
                },
            );
        }
    }
    definitions
}

fn resolve_game_folder(folder: &Path, game: &Value) -> Option<String> {
    let host = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        value => value,
    };
    let mut candidates = vec![host];
    if host == "linux" {
        candidates.push("win32");
    }
    let definitions = platform_definitions(game);
    candidates.into_iter().find_map(|platform| {
        let definition = definitions.get(platform)?;
        let required = std::iter::once(definition.executable.as_str())
            .chain(definition.data_files.iter().map(String::as_str))
            .chain(definition.bundle.iter().map(String::as_str));
        required
            .into_iter()
            .all(|relative| safe_relative(relative) && folder.join(relative).exists())
            .then(|| platform.to_owned())
    })
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn steam_library_roots(contents: &str) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for line in contents.lines() {
        let quoted = line.split('"').skip(1).step_by(2).collect::<Vec<_>>();
        for pair in quoted.windows(2) {
            if !pair[0].eq_ignore_ascii_case("path") {
                continue;
            }
            let value = pair[1].replace("\\\\", "\\");
            if !value.is_empty() {
                push_unique(&mut roots, PathBuf::from(value));
            }
        }
    }
    roots
}

fn steam_common_folders() -> Vec<PathBuf> {
    let mut steam_roots = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(variable) {
                push_unique(&mut steam_roots, PathBuf::from(root).join("Steam"));
            }
        }
        for letter in b'A'..=b'Z' {
            let drive = PathBuf::from(format!("{}:\\", char::from(letter)));
            if !drive.is_dir() {
                continue;
            }
            for relative in ["Steam", "SteamLibrary", "Program Files (x86)\\Steam"] {
                push_unique(&mut steam_roots, drive.join(relative));
            }
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(home) = std::env::var_os("HOME") {
        push_unique(
            &mut steam_roots,
            PathBuf::from(home).join(".local/share/Steam"),
        );
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        push_unique(
            &mut steam_roots,
            PathBuf::from(home).join("Library/Application Support/Steam"),
        );
    }

    let mut library_roots = steam_roots.clone();
    for root in steam_roots {
        let manifest = root.join("steamapps/libraryfolders.vdf");
        let Ok(metadata) = fs::symlink_metadata(&manifest) else {
            continue;
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024
        {
            continue;
        }
        if let Ok(contents) = fs::read_to_string(manifest) {
            for library in steam_library_roots(&contents) {
                push_unique(&mut library_roots, library);
            }
        }
    }

    let mut common = Vec::new();
    for root in library_roots {
        push_unique(&mut common, root.join("steamapps/common"));
    }
    common
}

fn steam_source(game: &Value) -> Option<(PathBuf, String)> {
    let data = game
        .get("availableFeatures")?
        .as_array()?
        .iter()
        .find(|feature| feature.get("feat").and_then(Value::as_str) == Some("steam"))?
        .get("data")?;
    let folder = data.get("folder")?.as_str()?;
    let app_id = data.get("appid")?.as_str()?.to_owned();
    if !safe_relative(folder) || !app_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let candidates = steam_common_folders()
        .into_iter()
        .map(|common| common.join(folder))
        .collect::<Vec<_>>();
    let source = candidates
        .iter()
        .find(|candidate| candidate.is_dir())
        .cloned()
        .or_else(|| candidates.into_iter().next())?;
    Some((source, app_id))
}

fn schedule_restart(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        app.restart();
    });
}

fn string_arg(data: &[Value], index: usize, channel: &'static str) -> Result<String, String> {
    data.get(index)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| error::invalid(channel))
}

fn installation_id(
    data: &[Value],
    index: usize,
    channel: &'static str,
) -> Result<InstallationId, String> {
    InstallationId::new(string_arg(data, index, channel)?).map_err(|_| error::invalid(channel))
}

fn platform(value: &str, channel: &'static str) -> Result<GamePlatform, String> {
    match value {
        "windows" | "win32" => Ok(GamePlatform::Windows),
        "linux" => Ok(GamePlatform::Linux),
        "macos" | "darwin" => Ok(GamePlatform::Macos),
        "wine" => Ok(GamePlatform::Wine),
        _ => Err(error::invalid(channel)),
    }
}

fn edition(value: &str, channel: &'static str) -> Result<Edition, String> {
    if value.is_empty() || value.len() > 100 || value.chars().any(char::is_control) {
        return Err(error::invalid(channel));
    }
    Ok(match value {
        "original" => Edition::Original,
        "expanded" => Edition::Expanded,
        other => Edition::Other(other.to_owned()),
    })
}

fn emit_copy_events(app: &AppHandle, runtime: &ProfileRuntime) {
    let Ok(events) = runtime.drain_events() else {
        return;
    };
    for event in events {
        let payload = match event {
            ProgressEvent::Started {
                operation_id,
                operation,
            } => json!({
                "operationId": operation_id,
                "phase": "started",
                "operation": operation,
                "completed": 0
            }),
            ProgressEvent::Phase {
                operation_id,
                phase,
                completed,
                total,
            } => json!({
                "operationId": operation_id,
                "phase": phase,
                "completed": completed,
                "total": total
            }),
            ProgressEvent::Warning {
                operation_id,
                message,
            } => json!({
                "operationId": operation_id,
                "phase": "warning",
                "completed": 0,
                "message": message
            }),
            ProgressEvent::Finished {
                operation_id,
                success,
                message,
            } => json!({
                "operationId": operation_id,
                "phase": if success { "complete" } else { "failed" },
                "success": success,
                "message": message
            }),
        };
        let _ = app.emit("game-import-progress", payload);
    }
}

fn run_copy_workflow<T>(
    app: &AppHandle,
    runtime: &ProfileRuntime,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let forwarding = Arc::new(AtomicBool::new(true));
    let worker_flag = Arc::clone(&forwarding);
    let worker_app = app.clone();
    let worker_runtime = runtime.clone();
    let forwarder = thread::spawn(move || {
        while worker_flag.load(Ordering::Acquire) {
            emit_copy_events(&worker_app, &worker_runtime);
            thread::sleep(Duration::from_millis(10));
        }
        emit_copy_events(&worker_app, &worker_runtime);
    });
    let result = work();
    forwarding.store(false, Ordering::Release);
    let _ = forwarder.join();
    result
}

fn serialize<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| error::internal())
}

/// Workflow channels that can be enabled without weakening the native filesystem boundary.
/// Dialog-owned Electron channels remain unavailable until a Tauri dialog integration exists.
pub fn dispatch(
    app: &AppHandle,
    state: &AppState,
    channel: &str,
    data: &[Value],
) -> Result<Option<Value>, String> {
    let result = match channel {
        "createNewInstallation" => {
            let steam = data.first().and_then(Value::as_str) == Some("steam");
            let from_locate = data.get(1).and_then(Value::as_str) == Some("locate");
            let from_manager = data.get(3).and_then(Value::as_bool).unwrap_or(false);
            let game_id = string_arg(data, 4, "createNewInstallation")?;
            let copy_to_managed = data.get(5).and_then(Value::as_str) == Some("copy");
            if !steam && !from_locate {
                return Err(error::invalid("createNewInstallation"));
            }
            let game = game_definition(&state._assets.app, &game_id, "createNewInstallation")?;
            let (source, steam_app_id) = if steam {
                let (mut source, app_id) = steam_source(&game)
                    .ok_or_else(|| error::unavailable("createNewInstallation"))?;
                if !source.is_dir() {
                    let dialogs =
                        deltamod_tauri_os_adapters::tauri_adapter::TauriDialogBackend::new(app);
                    let Some(common) = dialogs
                        .pick(&DialogRequest::folder("Select the Steam common folder"))
                        .map_err(|_| error::internal())?
                    else {
                        return Ok(Some(json!(false)));
                    };
                    let folder = source
                        .file_name()
                        .ok_or_else(|| error::invalid("createNewInstallation"))?
                        .to_owned();
                    source = if common.file_name() == Some(folder.as_os_str()) {
                        common
                    } else {
                        common.join(folder)
                    };
                }
                if state.profile()?.installations.iter().any(|record| {
                    record.extra.get("steamAppId").and_then(Value::as_str) == Some(&app_id)
                }) {
                    return Ok(Some(json!(false)));
                }
                (source, app_id)
            } else {
                (
                    PathBuf::from(string_arg(data, 2, "createNewInstallation")?),
                    String::new(),
                )
            };
            if !source.is_absolute() {
                return Err(error::invalid("createNewInstallation"));
            }
            let platform = match resolve_game_folder(&source, &game) {
                Some(platform) => platform,
                None => return Ok(Some(json!(false))),
            };
            let profile = state.profile()?;
            let index = if from_locate && !from_manager {
                profile.current_index.unwrap_or(0)
            } else {
                profile
                    .installations
                    .iter()
                    .filter_map(|record| record.index)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .filter(|index| *index <= MAX_INSTALLATION_INDEX)
                    .ok_or_else(|| error::invalid("createNewInstallation"))?
            };
            let mut store = Map::new();
            store.insert(
                "version".into(),
                json!(format!("DELTAMOD_DATA_{}", app.package_info().version)),
            );
            store.insert("loadedDeltarune".into(), json!(true));
            store.insert("gamePid".into(), json!(game_id));
            store.insert("gamePlatform".into(), json!(platform));
            store.insert("deltaruneEdition".into(), json!("rem"));
            store.insert("enabledMods".into(), json!([]));
            store.insert("isSteam".into(), json!(steam));
            store.insert("steamAppId".into(), json!(steam_app_id));
            let created = run_copy_workflow(app, &state.profile_runtime, || {
                state
                    .profile_runtime
                    .legacy_create_installation(
                        index,
                        &source,
                        format!("Install #{}", index + 1),
                        copy_to_managed,
                        store,
                    )
                    .map_err(|_| error::internal())
            })?;
            let _ = app.emit(
                "page",
                if from_manager {
                    "installmanager"
                } else {
                    "main"
                },
            );
            json!(created)
        }
        "repairInstallation" => {
            let index = legacy_index(data, "repairInstallation")?;
            let profile = state
                .profile_runtime
                .legacy_profile_folder(index)
                .map_err(|_| error::internal())?;
            let store_result = state.profile_runtime.legacy_store(index);
            let store_missing = store_result.is_err();
            let store = store_result.unwrap_or_default();
            let game_path = store
                .get("gamePath")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| profile.join("deltaruneInstall"));
            let recovery = if game_path.is_dir() {
                state
                    .profile_runtime
                    .restore_patch_transaction(&game_path)
                    .map(|_| ())
            } else {
                Ok(())
            };
            if let Err(failure) = recovery {
                json!({"repaired":false,"issues":[format!("Patch recovery failed: {failure}")]})
            } else {
                let mut issues = Vec::new();
                if store_missing {
                    issues.push("Installation data store is missing");
                }
                let game_id = store.get("gamePid").and_then(Value::as_str);
                let valid = game_id
                    .and_then(|id| {
                        game_definition(&state._assets.app, id, "repairInstallation").ok()
                    })
                    .and_then(|game| resolve_game_folder(&game_path, &game))
                    .is_some();
                if !valid {
                    issues.push("Game directory, executable, or GameMaker data file is missing");
                }
                json!({"repaired":issues.is_empty(),"issues":issues})
            }
        }
        "reimportInstallation" => {
            let index = legacy_index(data, "reimportInstallation")?;
            let store = state
                .profile_runtime
                .legacy_store(index)
                .map_err(|_| error::internal())?;
            let game_id = store
                .get("gamePid")
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("reimportInstallation"))?;
            let game = game_definition(&state._assets.app, game_id, "reimportInstallation")?;
            let dialogs = deltamod_tauri_os_adapters::tauri_adapter::TauriDialogBackend::new(app);
            let selected = dialogs
                .pick(&DialogRequest::folder(
                    "Choose a clean game installation to re-import",
                ))
                .map_err(|_| error::internal())?;
            let Some(source) = selected else {
                return Ok(Some(json!({"cancelled":true})));
            };
            let platform = resolve_game_folder(&source, &game)
                .ok_or_else(|| error::invalid("reimportInstallation"))?;
            serialize(run_copy_workflow(app, &state.profile_runtime, || {
                state
                    .profile_runtime
                    .legacy_reimport_installation(index, &source, platform)
                    .map_err(|_| error::internal())
            })?)?
        }
        "cancelGameImport" => {
            let id = data
                .first()
                .and_then(|value| {
                    value
                        .as_u64()
                        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
                })
                .ok_or_else(|| error::invalid("cancelGameImport"))?;
            json!(state.profile_runtime.cancel(id).is_ok())
        }
        "deleteSystemIndex" => {
            let deleted = state
                .profile_runtime
                .legacy_delete_installation(legacy_index(data, "deleteSystemIndex")?)
                .map_err(|_| error::internal())?;
            schedule_restart(app.clone());
            json!(deleted)
        }
        "setInstallationCName" => {
            let index = legacy_index(data, "setInstallationCName")?;
            let name = string_arg(data, 1, "setInstallationCName")?
                .replace(['\r', '\n', '\0'], " ")
                .trim()
                .to_owned();
            if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
                return Err(error::invalid("setInstallationCName"));
            }
            json!(state
                .profile_runtime
                .legacy_set_name(index, name)
                .map_err(|_| error::internal())?)
        }
        "changeSystemIndex" => {
            state
                .profile_runtime
                .legacy_change_system_index(legacy_index(data, "changeSystemIndex")?)
                .map_err(|_| error::internal())?;
            schedule_restart(app.clone());
            Value::Null
        }
        "openInstallationFolder" => {
            let folder = state
                .profile_runtime
                .legacy_managed_folder(legacy_index(data, "openInstallationFolder")?)
                .map_err(|_| error::internal())?;
            let folder =
                ValidatedFolder::from_backend(&folder, std::slice::from_ref(&state.data_root.root))
                    .map_err(|_| error::unavailable("openInstallationFolder"))?;
            app.opener()
                .open_path(folder.path().to_string_lossy(), None::<&str>)
                .map_err(|_| error::internal())?;
            json!("")
        }
        "createInstallation" => {
            let source = PathBuf::from(string_arg(data, 0, "createInstallation")?);
            if !source.is_absolute() {
                return Err(error::invalid("createInstallation"));
            }
            let name = string_arg(data, 1, "createInstallation")?;
            let platform = platform(
                &string_arg(data, 2, "createInstallation")?,
                "createInstallation",
            )?;
            let ownership = match data.get(3).and_then(Value::as_str) {
                Some("managed-copy" | "copy") => Ownership::ManagedCopy,
                Some("linked-external" | "link") => Ownership::LinkedExternal,
                _ => return Err(error::invalid("createInstallation")),
            };
            serialize(
                state
                    .profile_runtime
                    .create_installation(&source, name, platform, ownership)
                    .map_err(|_| error::internal())?,
            )?
        }
        "copyInstallation" => serialize(
            state
                .profile_runtime
                .copy_installation(
                    &installation_id(data, 0, "copyInstallation")?,
                    string_arg(data, 1, "copyInstallation")?,
                )
                .map_err(|_| error::internal())?,
        )?,
        "removeInstallation" => serialize(
            state
                .profile_runtime
                .delete_installation(
                    &installation_id(data, 0, "removeInstallation")?,
                    data.get(1)
                        .and_then(Value::as_bool)
                        .ok_or_else(|| error::invalid("removeInstallation"))?,
                )
                .map_err(|_| error::internal())?,
        )?,
        "setCurrentInstallation" => {
            let id = match data.first() {
                Some(Value::Null) | None => None,
                Some(Value::String(_)) => Some(installation_id(data, 0, "setCurrentInstallation")?),
                _ => return Err(error::invalid("setCurrentInstallation")),
            };
            serialize(
                state
                    .profile_runtime
                    .select(id.as_ref())
                    .map_err(|_| error::internal())?,
            )?
        }
        "setCurrentInstallationVariant" => serialize(
            state
                .profile_runtime
                .set_edition(
                    &installation_id(data, 0, "setCurrentInstallationVariant")?,
                    edition(
                        &string_arg(data, 1, "setCurrentInstallationVariant")?,
                        "setCurrentInstallationVariant",
                    )?,
                )
                .map_err(|_| error::internal())?,
        )?,
        "removeMod" => {
            let folder = string_arg(data, 0, "removeMod")?;
            if folder.is_empty()
                || folder.len() > 128
                || !folder
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            {
                return Err(error::invalid("removeMod"));
            }
            let root = state.data_root.root.join("packets");
            let path = root.join(&folder);
            let directory = fs::symlink_metadata(&path).map_err(|_| error::internal())?;
            let marker =
                fs::symlink_metadata(path.join("__deltaID.json")).map_err(|_| error::internal())?;
            if !directory.is_dir()
                || directory.file_type().is_symlink()
                || !marker.is_file()
                || marker.file_type().is_symlink()
            {
                return Err(error::invalid("removeMod"));
            }
            fs::remove_dir_all(path).map_err(|_| error::internal())?;
            json!(true)
        }
        "preparePatchPlan" => {
            let input: PatchPlanInput = serde_json::from_value(
                data.first()
                    .cloned()
                    .ok_or_else(|| error::invalid("preparePatchPlan"))?,
            )
            .map_err(|_| error::invalid("preparePatchPlan"))?;
            serialize(
                state
                    .profile_runtime
                    .prepare_patch_plan(input)
                    .map_err(|_| error::internal())?,
            )?
        }
        "restorePatchTransaction" => json!(state
            .profile_runtime
            .restore_patch_transaction(&PathBuf::from(string_arg(
                data,
                0,
                "restorePatchTransaction"
            )?))
            .map_err(|_| error::internal())?),
        "importMod" | "dlmodURL" | "precalcGameHashes" | "executePatchPlan" | "patchAndRun" => {
            return Err(error::unavailable(channel));
        }
        _ => return Ok(None),
    };
    emit_copy_events(app, &state.profile_runtime);
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modern_steam_library_paths() {
        let roots = steam_library_roots(
            r#"
            "libraryfolders"
            {
                "0" { "path" "C:\\Program Files (x86)\\Steam" }
                "1" { "path" "E:\\SteamLibrary" }
            }
            "#,
        );
        assert_eq!(
            roots,
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"E:\SteamLibrary")
            ]
        );
    }

    #[test]
    fn ignores_unrelated_vdf_fields_and_duplicate_paths() {
        let roots = steam_library_roots(
            r#"
            "path" "E:\\SteamLibrary"
            "apps" "391540"
            "path" "E:\\SteamLibrary"
            "#,
        );
        assert_eq!(roots, vec![PathBuf::from(r"E:\SteamLibrary")]);
    }
}
