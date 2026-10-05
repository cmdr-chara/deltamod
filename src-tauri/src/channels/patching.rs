use crate::{channels::runtime, error, state::AppState};
use deltamod_native_core::patch_plan::PatchPlatform;
use deltamod_patching_runtime::{mac_bundle, LifecycleStorageRoots};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
};
use tauri::{AppHandle, Emitter};

fn operation_id(state: &AppState, prefix: &str) -> String {
    let sequence = state.patch_sequence.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{sequence}", std::process::id())
}

/// On macOS Deltamod patches only its own copy of the game bundle. Returns the
/// bundle when the installation instead points at an original outside the
/// app's data folder (typically /Applications), so patching can refuse it.
fn original_mac_bundle(
    platform: PatchPlatform,
    content_root: Option<&str>,
    game_root: &Path,
    data_root: &Path,
) -> Option<PathBuf> {
    if platform != PatchPlatform::Darwin {
        return None;
    }
    let bundle = mac_bundle::bundle_of(game_root, content_root?)?;
    let managed = fs::canonicalize(data_root).unwrap_or_else(|_| data_root.to_owned());
    let game = fs::canonicalize(game_root).unwrap_or_else(|_| game_root.to_owned());
    (!game.starts_with(&managed)).then_some(bundle)
}

fn selected_mods(data: &[Value]) -> Result<Vec<String>, String> {
    let values = data
        .first()
        .and_then(Value::as_array)
        .ok_or_else(|| error::invalid("patchAndRun"))?;
    if values.len() > 1_000 {
        return Err(error::invalid("patchAndRun"));
    }
    let mut selected = Vec::with_capacity(values.len());
    for value in values {
        let id = value
            .as_str()
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 256
                    && !id.chars().any(|character| character.is_control())
            })
            .ok_or_else(|| error::invalid("patchAndRun"))?;
        if !selected.iter().any(|existing| existing == id) {
            selected.push(id.to_owned());
        }
    }
    Ok(selected)
}

fn selected_mods_are_compatible(list: &Value, selected: &[String]) -> bool {
    let Some(records) = list.as_array() else {
        return false;
    };
    selected.iter().all(|selected_id| {
        records.iter().any(|record| {
            record.get("uid").and_then(Value::as_str) == Some(selected_id.as_str())
                && record.get("isIncompatible").and_then(Value::as_bool) != Some(true)
        })
    })
}

pub fn dispatch(
    app: &AppHandle,
    state: &AppState,
    channel: &str,
    data: &[Value],
) -> Result<Option<Value>, String> {
    match channel {
        "precalcGameHashes" => {
            state.patch_cancelled.store(false, Ordering::Release);
            let id = operation_id(state, "hash");
            let result = state
                .patching
                .precalc_game_hashes(
                    &id,
                    |progress| {
                        let _ = app.emit("hash-progress", progress);
                    },
                    || state.patch_cancelled.load(Ordering::Acquire),
                )
                .map_err(|_| error::internal())?;
            serde_json::to_value(result)
                .map(Some)
                .map_err(|_| error::internal())
        }
        "patchAndRun" => {
            let selected = selected_mods(data)?;
            let compatibility = runtime::mod_list(state, "patchAndRun")?;
            if !selected_mods_are_compatible(&compatibility, &selected) {
                let _ = app.emit(
                    "gplog",
                    json!({
                        "log": "A selected mod is unavailable or incompatible with the active game installation.",
                        "percent": -1.0
                    }),
                );
                let _ = app.emit("audio", true);
                let _ = app.emit("page", "main");
                return Err("A selected mod is unavailable or incompatible with the active game installation.".into());
            }
            if let Some(bundle) = original_mac_bundle(
                state.patching.platform,
                state.patching.definition.content_root.as_deref(),
                &state.patching.game_root,
                &state.data_root.root,
            ) {
                let message = format!(
                    "This installation uses your original game at \"{}\". To keep it untouched, Deltamod only applies mods to its own copy on macOS. Open Options → Installation → Install Manager, add the game again, and switch to the new installation.",
                    bundle.display()
                );
                let _ = app.emit("gplog", json!({"log": message, "percent": -1.0}));
                let _ = app.emit("audio", true);
                let _ = app.emit("page", "main");
                return Err(message);
            }
            state.patch_cancelled.store(false, Ordering::Release);
            let id = operation_id(state, "patch");
            let hash_checks = state
                .preferences
                .lock()
                .map_err(|_| error::internal())?
                .unique_flags
                .get("HASHCHECKS")
                .copied()
                .unwrap_or(false);
            let lifecycle = LifecycleStorageRoots {
                store: state.data_root.root.join("lifecycle-store"),
                workspace: state.data_root.root.join("lifecycle-workspaces"),
            };
            let result = if hash_checks {
                state
                    .patching
                    .check_selected_legacy_mods(&selected)
                    .and_then(|_| {
                        state.patching.patch_and_run(
                            &selected,
                            &id,
                            &lifecycle,
                            &state.game,
                            |progress| {
                                let _ = app.emit(
                                    "gplog",
                                    json!({
                                        "log": progress.log.unwrap_or_default(),
                                        "percent": progress.percent.unwrap_or(-1.0)
                                    }),
                                );
                            },
                            || state.patch_cancelled.load(Ordering::Acquire),
                        )
                    })
            } else {
                state.patching.patch_and_run(
                    &selected,
                    &id,
                    &lifecycle,
                    &state.game,
                    |progress| {
                        let _ = app.emit(
                            "gplog",
                            json!({
                                "log": progress.log.unwrap_or_default(),
                                "percent": progress.percent.unwrap_or(-1.0)
                            }),
                        );
                    },
                    || state.patch_cancelled.load(Ordering::Acquire),
                )
            };
            match result {
                Ok(result) if result.patched => {
                    let mods = state
                        .patching
                        .mark_selected_patched(&selected)
                        .map_err(|_| error::internal())?;
                    let _ = app.emit("finishedPatch", mods);
                    Ok(Some(Value::Null))
                }
                Ok(_) => {
                    let _ = app.emit("audio", true);
                    let _ = app.emit("page", "main");
                    Ok(Some(json!(false)))
                }
                Err(error) => {
                    let _ = app.emit("gplog", json!({"log": error.to_string(), "percent": -1.0}));
                    let _ = app.emit("audio", true);
                    let _ = app.emit("page", "main");
                    // The runtime owns rollback and launch reservations. Do not
                    // perform an unowned legacy restore here, or hide the failure
                    // behind a successful IPC acknowledgement and page change.
                    Err(error.to_string())
                }
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_patching_refuses_the_original_game_bundle() {
        let data = std::env::temp_dir().join(format!("deltamod-data-{}", uuid::Uuid::new_v4()));
        let managed = data.join("deltamod_system-1/deltaruneInstall");
        fs::create_dir_all(&managed).unwrap();
        let resources = Some("DELTARUNE.app/Contents/Resources");
        assert_eq!(
            original_mac_bundle(
                PatchPlatform::Darwin,
                resources,
                Path::new("/Applications"),
                &data
            ),
            Some(PathBuf::from("/Applications/DELTARUNE.app"))
        );
        assert_eq!(
            original_mac_bundle(PatchPlatform::Darwin, resources, &managed, &data),
            None
        );
        assert_eq!(
            original_mac_bundle(
                PatchPlatform::Win32,
                resources,
                Path::new("/Applications"),
                &data
            ),
            None
        );
        assert_eq!(
            original_mac_bundle(
                PatchPlatform::Darwin,
                None,
                Path::new("/Applications"),
                &data
            ),
            None
        );
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn selected_mods_rejects_invalid_and_deduplicates() {
        assert_eq!(
            selected_mods(&[json!(["one", "one", "two"])]).unwrap(),
            ["one", "two"]
        );
        assert!(selected_mods(&[json!([""])]).is_err());
        assert!(selected_mods(&[json!("one")]).is_err());
    }

    #[test]
    fn patch_selection_rejects_missing_and_incompatible_records() {
        let list = json!([
            {"uid":"undertale-mod","isIncompatible":false},
            {"uid":"deltarune-mod","isIncompatible":true}
        ]);
        assert!(selected_mods_are_compatible(
            &list,
            &["undertale-mod".into()]
        ));
        assert!(!selected_mods_are_compatible(
            &list,
            &["deltarune-mod".into()]
        ));
        assert!(!selected_mods_are_compatible(
            &list,
            &["missing-mod".into()]
        ));
    }
}
