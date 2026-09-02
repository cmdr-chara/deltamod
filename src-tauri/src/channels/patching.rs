use crate::{channels::runtime, error, state::AppState};
use deltamod_patching_runtime::LifecycleStorageRoots;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};

fn operation_id(state: &AppState, prefix: &str) -> String {
    let sequence = state.patch_sequence.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{sequence}", std::process::id())
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
                return Ok(Some(json!(false)));
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
                    let _ = state.patching.restore();
                    let _ = app.emit("audio", true);
                    let _ = app.emit("page", "main");
                    Ok(Some(json!(false)))
                }
                Err(error) => {
                    let _ = app.emit("gplog", json!({"log": error.to_string(), "percent": -1.0}));
                    let _ = state.patching.restore();
                    let _ = app.emit("audio", true);
                    let _ = app.emit("page", "main");
                    Ok(Some(json!(false)))
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
