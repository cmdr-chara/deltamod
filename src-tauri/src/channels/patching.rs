use crate::{error, state::AppState};
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
            state.patch_cancelled.store(false, Ordering::Release);
            let id = operation_id(state, "patch");
            let result = state.patching.patch_and_run(
                &selected,
                &id,
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
            );
            match result {
                Ok(result) if result.patched => {
                    let mods = state
                        .patching
                        .mark_selected_patched(&selected)
                        .map_err(|_| error::internal())?;
                    let _ = app.emit("finishedPatch", mods);
                    Ok(Some(Value::Null))
                }
                Ok(_) | Err(_) => {
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
}
