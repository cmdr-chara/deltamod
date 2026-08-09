use crate::{error, state::AppState};
use deltamod_archive_import_runtime::{
    import_archive_with_source, DuplicateDecision, ImportError, LegacySourceMetadata, Limits,
};
use deltamod_network_runtime::import_download::{
    validate_download_url, DownloadPolicy, HostAllowlist,
};
use deltamod_tauri_os_adapters::{
    validate_dialog_selection, ChoiceBackend, DialogBackend, DialogFilter, DialogRequest,
};
use serde_json::{json, Value};
use std::{cell::RefCell, fs};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

fn valid_operation_id(value: &str) -> bool {
    (1..=32).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn emit_download_error(app: &AppHandle, operation_id: &str, message: &str) {
    let _ = app.emit(
        "dlmodURL-progress",
        json!({
            "progress": 0,
            "downloaded": 0,
            "queryme": operation_id,
            "error": true,
            "message": message
        }),
    );
}

fn duplicate_decision<D: ChoiceBackend>(
    dialogs: &D,
    existing: deltamod_archive_import_runtime::ExistingMod<'_>,
) -> Result<DuplicateDecision, String> {
    let old_version = existing.old_version.unwrap_or("Unknown");
    let new_version = existing.new_version.unwrap_or("Unknown");
    let message = format!(
        "The mod \"{}\" is already present in your mods.\n\nPresent version: {old_version}\nTo be imported version: {new_version}\n\nHow would you like to proceed?",
        existing.package_id
    );
    match dialogs
        .choose(
            "Import Failed",
            &message,
            &[
                "Delete old version".into(),
                "Keep old version".into(),
                "Cancel import".into(),
            ],
        )
        .map_err(|_| error::internal())?
    {
        Some(0) => Ok(DuplicateDecision::Replace),
        Some(1) => Ok(DuplicateDecision::KeepExisting),
        Some(2) | None => Ok(DuplicateDecision::Cancel),
        Some(_) => Err(error::internal()),
    }
}

fn run_import<D: ChoiceBackend, C: Fn() -> bool>(
    dialogs: &D,
    archive: &std::path::Path,
    packet_root: &std::path::Path,
    source: Option<&LegacySourceMetadata>,
    cancelled: C,
) -> Result<Value, String> {
    let choice_error = RefCell::new(None);
    let result = import_archive_with_source(
        archive,
        packet_root,
        Limits::default(),
        source,
        cancelled,
        |existing| match duplicate_decision(dialogs, existing) {
            Ok(decision) => decision,
            Err(message) => {
                *choice_error.borrow_mut() = Some(message);
                DuplicateDecision::Cancel
            }
        },
    );
    if let Some(message) = choice_error.into_inner() {
        return Err(message);
    }
    match result {
        Ok(_) => Ok(json!(true)),
        Err(ImportError::Cancelled | ImportError::KeptExisting) => Ok(json!(false)),
        Err(import_error) => Err(import_error.to_string()),
    }
}

fn import_mod<D: DialogBackend + ChoiceBackend>(
    packet_root: &std::path::Path,
    dialogs: &D,
) -> Result<Value, String> {
    let request = DialogRequest::file("Choose a Deltamod compatible archive").filter(
        DialogFilter::new("Deltamod compatible archive", ["zip", "7z", "gz", "lzma"])
            .map_err(|_| error::internal())?,
    );
    let Some(selected) = dialogs.pick(&request).map_err(|_| error::internal())? else {
        return Ok(json!(false));
    };
    let selected =
        validate_dialog_selection(&request, selected).map_err(|_| error::invalid("importMod"))?;
    run_import(dialogs, &selected, packet_root, None, || false)
}

fn optional_source_metadata(data: &[Value]) -> Result<Option<LegacySourceMetadata>, String> {
    let Some(id) = data.get(2).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let Some(model) = data.get(3).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let value_string = |value: &Value| match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    };
    LegacySourceMetadata::new(
        value_string(id).ok_or_else(|| error::invalid("dlmodURL"))?,
        value_string(model).ok_or_else(|| error::invalid("dlmodURL"))?,
    )
    .map(Some)
    .map_err(|_| error::invalid("dlmodURL"))
}

fn download_mod<D: ChoiceBackend>(
    app: &AppHandle,
    state: &AppState,
    dialogs: &D,
    data: &[Value],
) -> Result<Value, String> {
    let url = data
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| error::invalid("dlmodURL"))?;
    let operation_id = data
        .get(1)
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .filter(|value| valid_operation_id(value))
        .ok_or_else(|| error::invalid("dlmodURL"))?;
    if let Err(network_error) = validate_download_url(url, HostAllowlist::GAMEBANANA) {
        let message = network_error.to_string();
        emit_download_error(app, &operation_id, &message);
        return Err(message);
    }
    let source = optional_source_metadata(data)?;
    let (_, cancel) = watch::channel(false);
    let runtime = state
        .network_runtime
        .lock()
        .map_err(|_| error::internal())?;
    let downloaded = runtime.block_on(state.network.download_allowlisted(
        operation_id.clone(),
        url,
        HostAllowlist::GAMEBANANA,
        DownloadPolicy::mods(),
        &cancel,
        |progress| {
            let percentage = progress
                .total
                .filter(|total| *total > 0)
                .map(|total| progress.completed as f64 / total as f64 * 100.0)
                .unwrap_or(0.0);
            let _ = app.emit(
                "dlmodURL-progress",
                json!({
                    "progress": percentage,
                    "downloaded": progress.completed,
                    "total": progress.total.unwrap_or(0),
                    "phase": "download",
                    "queryme": operation_id,
                    "error": false
                }),
            );
        },
    ));
    drop(runtime);
    let downloaded = match downloaded {
        Ok(downloaded) => downloaded,
        Err(network_error) => {
            let message = network_error.to_string();
            emit_download_error(app, &operation_id, &message);
            return Err(message);
        }
    };
    let _ = app.emit(
        "dlmodURL-progress",
        json!({
            "progress": 100,
            "downloaded": downloaded.bytes,
            "total": downloaded.total.unwrap_or(0),
            "phase": "import",
            "queryme": operation_id,
            "error": false
        }),
    );
    let result = run_import(
        dialogs,
        &downloaded.path,
        &state.data_root.root.join("packets"),
        source.as_ref(),
        || *cancel.borrow(),
    );
    match result {
        Ok(Value::Bool(true)) => {
            let _ = app.emit(
                "dlmodURL-progress",
                json!({
                    "progress": 100,
                    "downloaded": downloaded.bytes,
                    "total": downloaded.total.unwrap_or(0),
                    "phase": "complete",
                    "queryme": operation_id,
                    "error": false
                }),
            );
            Ok(json!(true))
        }
        Ok(_) => {
            let message = "The downloaded mod was not imported.".to_owned();
            emit_download_error(app, &operation_id, &message);
            Err(message)
        }
        Err(message) => {
            emit_download_error(app, &operation_id, &message);
            Err(message)
        }
    }
}

fn emit_game_progress(app: &AppHandle, event: &deltamod_game_download_runtime::ProgressEvent) {
    let _ = app.emit("game-import-progress", event);
}

fn game_limits(
    plan: &deltamod_game_download_runtime::ImportTransactionPlan,
) -> deltamod_archive_import_runtime::Limits {
    deltamod_archive_import_runtime::Limits {
        max_entries: plan.archive_limits.max_files as usize,
        max_archive_bytes: plan.archive_limits.max_archive_bytes,
        max_entry_bytes: plan.archive_limits.max_expanded_bytes,
        max_expanded_bytes: plan.archive_limits.max_expanded_bytes,
        max_ratio: 1000,
        max_depth: 32,
        max_manifest_bytes: 1024 * 1024,
    }
}

fn download_game(app: &AppHandle, state: &AppState, data: &[Value]) -> Result<Value, String> {
    let game_id = data
        .first()
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 80
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        })
        .ok_or_else(|| error::invalid("downloadGame"))?;
    let request = deltamod_game_download_runtime::GameRequest {
        game_id: game_id.to_owned(),
        platform: deltamod_game_download_runtime::Platform::current(),
        edition: deltamod_game_download_runtime::Edition::Original,
    };
    let (game, artifact) = state
        .game_download
        .catalog_selection(&request)
        .map_err(|value| value.to_string())?;
    let token = deltamod_game_download_runtime::CancellationToken::default();
    match &artifact.metadata {
        deltamod_game_download_runtime::ProviderMetadata::Itch { .. } => {
            let operation_id = Uuid::new_v4();
            state
                .game_download_cancellations
                .lock()
                .map_err(|_| error::internal())?
                .insert(operation_id.to_string(), token.clone());
            let destination = state
                .data_root
                .root
                .join("game-downloads")
                .join(format!("game-{operation_id}"));
            let result = (|| {
                let butlerd = state.butlerd.as_ref().ok_or_else(|| {
                    deltamod_game_download_runtime::RuntimeError::ButlerUnavailable.to_string()
                })?;
                let _ = app.emit(
                    "game-import-progress",
                    json!({
                        "operationId": operation_id, "phase": "resolving", "completed": 0,
                        "total": Value::Null, "currentItem": Value::Null
                    }),
                );
                let installed = butlerd
                    .install(&artifact.metadata, &destination, &token, |progress| {
                        let _ = app.emit(
                            "game-import-progress",
                            json!({
                                "operationId": operation_id, "phase": progress.phase,
                                "completed": progress.completed, "total": progress.total,
                                "currentItem": progress.current_item
                            }),
                        );
                    })
                    .map_err(|value| value.to_string())?;
                let executable =
                    fs::symlink_metadata(installed.join(&game.executable)).map_err(|_| {
                        "ITCH_INSTALL_INVALID: packaged executable is missing".to_owned()
                    })?;
                if !executable.is_file() || executable.file_type().is_symlink() {
                    return Err("ITCH_INSTALL_INVALID: packaged executable is invalid".to_owned());
                }
                let _ = app.emit(
                    "game-import-progress",
                    json!({
                        "operationId": operation_id, "phase": "ready", "completed": 1,
                        "total": 1, "currentItem": game.executable
                    }),
                );
                Ok(json!(installed.to_string_lossy()))
            })();
            state
                .game_download_cancellations
                .lock()
                .map_err(|_| error::internal())?
                .remove(&operation_id.to_string());
            if result.is_err() {
                let _ = fs::remove_dir_all(destination);
            }
            result
        }
        deltamod_game_download_runtime::ProviderMetadata::GameJolt { .. } => {
            let runtime = state
                .network_runtime
                .lock()
                .map_err(|_| error::internal())?;
            let mut registered = None;
            let plan_result = runtime.block_on(state.game_download.download_game(
                request,
                token.clone(),
                |event| {
                    if registered.is_none() {
                        registered = Some(event.operation_id.to_string());
                        if let Ok(mut operations) = state.game_download_cancellations.lock() {
                            operations.insert(event.operation_id.to_string(), token.clone());
                        }
                    }
                    emit_game_progress(app, &event);
                },
            ));
            drop(runtime);
            if plan_result.is_err() {
                if let Some(operation_id) = registered.as_ref() {
                    if let Ok(mut operations) = state.game_download_cancellations.lock() {
                        operations.remove(operation_id);
                    }
                }
            }
            let plan = plan_result.map_err(|value| value.to_string())?;
            let destination = state
                .data_root
                .root
                .join("game-downloads")
                .join(format!("game-{}", plan.operation_id));
            let imported = deltamod_archive_import_runtime::import_game_archive(
                &plan.archive_path,
                &destination,
                &plan.executable,
                game_limits(&plan),
                || token.is_cancelled(),
            );
            if plan.delete_archive_after_import {
                let _ = fs::remove_file(&plan.archive_path);
            }
            if let Some(operation_id) = registered {
                if let Ok(mut operations) = state.game_download_cancellations.lock() {
                    operations.remove(&operation_id);
                }
            }
            let imported = imported.map_err(|value| value.to_string())?;
            Ok(json!(imported.root.to_string_lossy()))
        }
    }
}

/// Isolated legacy channel adapter. Integration must place this before `workflows::dispatch`,
/// which currently returns an unavailable error for `importMod` and `dlmodURL`.
pub fn dispatch<D: DialogBackend + ChoiceBackend>(
    app: &AppHandle,
    _state: &AppState,
    dialogs: &D,
    channel: &str,
    data: &[Value],
) -> Result<Option<Value>, String> {
    match channel {
        "importMod" => import_mod(&_state.data_root.root.join("packets"), dialogs).map(Some),
        "dlmodURL" => download_mod(app, _state, dialogs, data).map(Some),
        "downloadGame" => download_game(app, _state, data).map(Some),
        "cancelGameImport" => {
            let operation_id = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("cancelGameImport"))?;
            let operations = _state
                .game_download_cancellations
                .lock()
                .map_err(|_| error::internal())?;
            if let Some(token) = operations.get(operation_id) {
                token.cancel();
                Ok(Some(json!(true)))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deltamod_tauri_os_adapters::AdapterError;
    use std::path::{Path, PathBuf};

    struct TestDialogs(Option<usize>);

    impl DialogBackend for TestDialogs {
        fn pick(&self, _: &DialogRequest) -> Result<Option<PathBuf>, AdapterError> {
            Ok(None)
        }
    }

    impl ChoiceBackend for TestDialogs {
        fn choose(&self, _: &str, _: &str, _: &[String]) -> Result<Option<usize>, AdapterError> {
            Ok(self.0)
        }
    }

    #[test]
    fn legacy_download_ids_are_bounded() {
        assert!(valid_operation_id("abc123"));
        assert!(valid_operation_id("A"));
        assert!(!valid_operation_id(""));
        assert!(!valid_operation_id("has-dash"));
        assert!(!valid_operation_id(&"a".repeat(33)));
    }

    #[test]
    fn source_metadata_preserves_legacy_number_and_string_inputs() {
        let source = optional_source_metadata(&[
            json!("https://gamebanana.com/file"),
            json!("op"),
            json!(42),
            json!("Mod"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(source.gamebanana_id, "42");
        assert_eq!(source.gamebanana_model, "Mod");
    }

    #[test]
    fn picker_cancellation_has_exact_legacy_false_shape() {
        assert_eq!(
            import_mod(Path::new("unused"), &TestDialogs(None)).unwrap(),
            json!(false)
        );
    }

    #[test]
    fn duplicate_native_choices_map_to_importer_decisions() {
        let existing = |dialogs: &TestDialogs| {
            duplicate_decision(
                dialogs,
                deltamod_archive_import_runtime::ExistingMod {
                    package_id: "example.mod",
                    destination: Path::new("unused"),
                    old_version: Some("1"),
                    new_version: Some("2"),
                },
            )
            .unwrap()
        };
        assert_eq!(existing(&TestDialogs(Some(0))), DuplicateDecision::Replace);
        assert_eq!(
            existing(&TestDialogs(Some(1))),
            DuplicateDecision::KeepExisting
        );
        assert_eq!(existing(&TestDialogs(Some(2))), DuplicateDecision::Cancel);
        assert_eq!(existing(&TestDialogs(None)), DuplicateDecision::Cancel);
    }
}
