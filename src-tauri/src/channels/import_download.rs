use crate::{channels::auth, error, state::AppState};
use deltamod_archive_import_runtime::{
    import_archive_with_source, DuplicateDecision, ImportError, LegacySourceMetadata, Limits,
};
use deltamod_game_download_runtime::CancellationToken;
use deltamod_network_runtime::GameBanana;
use deltamod_network_runtime::import_download::{
    validate_download_url, DownloadPolicy, HostAllowlist,
};
use deltamod_tauri_os_adapters::{
    validate_dialog_selection, ChoiceBackend, DialogBackend, DialogFilter, DialogRequest,
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, fs};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

const PROTOCOL_DOWNLOAD_FAILED: &str = "The GameBanana one-click download failed.";
const PROTOCOL_IMPORT_FAILED: &str = "The downloaded GameBanana mod could not be imported.";
const MAX_PROTOCOL_ID: u32 = 2_000_000_000;
const MAX_COLLECTION_PAGES: u32 = 64;
const MAX_COLLECTION_ITEMS: usize = 256;
const MAX_COLLECTION_FILES: usize = 128;
const GAMEBANANA_TOOL_ID: u64 = 20_575;

pub(crate) struct ProtocolImportRequest<'a> {
    pub item_id: u32,
    pub file_id: u32,
    pub source_url: &'a str,
}

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

pub(crate) fn run_import<D: ChoiceBackend, C: Fn() -> bool>(
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

fn download_and_import<D: ChoiceBackend>(
    app: &AppHandle,
    state: &AppState,
    dialogs: &D,
    url: &str,
    operation_id: String,
    source: Option<&LegacySourceMetadata>,
) -> Result<Value, String> {
    if let Err(network_error) = validate_download_url(url, HostAllowlist::GAMEBANANA) {
        let message = network_error.to_string();
        emit_download_error(app, &operation_id, &message);
        return Err(message);
    }
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
        source,
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
    let source = optional_source_metadata(data)?;
    download_and_import(app, state, dialogs, url, operation_id, source.as_ref())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CollectionFileCandidate {
    file_id: u32,
    url: String,
    label: String,
}

fn provider_id(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
        .filter(|id| *id > 0 && *id <= u64::from(MAX_PROTOCOL_ID))
}

fn safe_collection_model(value: &Value) -> Option<String> {
    let model = value.as_str()?.trim();
    if model.is_empty()
        || model.len() > 32
        || !model
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !model.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(model.to_owned())
}

fn normalized_collection_download_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let normalized = if let Some(file) = raw.strip_prefix("https://gamebanana.com/dl/") {
        format!("https://gamebanana.com/mmdl/{file}")
    } else if let Some(file) = raw.strip_prefix("https://files.gamebanana.com/dl/") {
        format!("https://files.gamebanana.com/mmdl/{file}")
    } else {
        raw.to_owned()
    };
    validate_download_url(&normalized, HostAllowlist::GAMEBANANA).ok()?;
    protocol_source_file_id(&normalized)?;
    Some(normalized)
}

fn collection_file_candidates(profile: &Value) -> Result<Vec<CollectionFileCandidate>, String> {
    let Some(files) = profile.get("_aFiles").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    if files.len() > MAX_COLLECTION_FILES {
        return Err("GAMEBANANA_COLLECTION_TOO_MANY_FILES".to_owned());
    }
    let mut candidates = Vec::new();
    for file in files {
        let integrated = file
            .get("_aModManagerIntegrations")
            .and_then(Value::as_array)
            .is_some_and(|integrations| {
                integrations.iter().any(|integration| {
                    provider_id(integration.get("_idToolRow").unwrap_or(&Value::Null))
                        == Some(GAMEBANANA_TOOL_ID)
                })
            });
        if !integrated {
            continue;
        }
        let Some(url) = file
            .get("_sDownloadUrl")
            .and_then(Value::as_str)
            .and_then(normalized_collection_download_url)
        else {
            continue;
        };
        let Some(file_id) = protocol_source_file_id(&url) else {
            continue;
        };
        let label = file
            .get("_sFile")
            .and_then(Value::as_str)
            .map(|value| {
                value
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(80)
                    .collect::<String>()
            })
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("GameBanana file {file_id}"));
        candidates.push(CollectionFileCandidate {
            file_id,
            url,
            label,
        });
    }
    Ok(candidates)
}

fn choose_collection_file<D: ChoiceBackend>(
    dialogs: &D,
    candidates: &[CollectionFileCandidate],
) -> Result<Option<CollectionFileCandidate>, String> {
    if candidates.is_empty() {
        return Ok(None);
    }
    if candidates.len() == 1 {
        return Ok(candidates.first().cloned());
    }
    let choices = candidates
        .iter()
        .map(|candidate| candidate.label.clone())
        .collect::<Vec<_>>();
    let selected = dialogs
        .choose(
            "Choose GameBanana file",
            "This collection item has multiple Deltamod-compatible files. Choose which one to restore.",
            &choices,
        )
        .map_err(|_| error::internal())?;
    Ok(selected.and_then(|index| candidates.get(index).cloned()))
}

fn emit_collection_progress(
    app: &AppHandle,
    operation_id: &str,
    collection_id: u64,
    phase: &str,
    completed: usize,
    total: usize,
    current_item: Option<&str>,
    imported: usize,
    skipped: usize,
) {
    let _ = app.emit(
        "collection-restore-progress",
        json!({
            "operationId": operation_id,
            "collectionId": collection_id,
            "phase": phase,
            "completed": completed,
            "total": total,
            "currentItem": current_item,
            "imported": imported,
            "skipped": skipped
        }),
    );
}

fn restore_gamebanana_collection<D: DialogBackend + ChoiceBackend>(
    app: &AppHandle,
    state: &AppState,
    dialogs: &D,
    data: &[Value],
) -> Result<Value, String> {
    let collection_id = provider_id(
        data.first()
            .ok_or_else(|| error::invalid("gamebanana_downloadAllInCollection"))?,
    )
    .ok_or_else(|| error::invalid("gamebanana_downloadAllInCollection"))?;
    let token = auth::token(state)?;
    let operation_id = Uuid::new_v4().simple().to_string();
    emit_collection_progress(
        app,
        &operation_id,
        collection_id,
        "resolving",
        0,
        0,
        None,
        0,
        0,
    );

    let api = GameBanana {
        client: &state.network,
        token: Some(token),
    };
    let mut records = Vec::new();
    let mut complete = false;
    for page in 1..=MAX_COLLECTION_PAGES {
        let response = {
            let runtime = state
                .network_runtime
                .lock()
                .map_err(|_| error::internal())?;
            runtime
                .block_on(api.collection_items::<Value>(collection_id, page))
                .map_err(|_| "GAMEBANANA_COLLECTION_REQUEST_FAILED".to_owned())?
        };
        let page_records = response
            .get("_aRecords")
            .and_then(Value::as_array)
            .ok_or_else(|| "GAMEBANANA_COLLECTION_RESPONSE_INVALID".to_owned())?;
        if records.len().saturating_add(page_records.len()) > MAX_COLLECTION_ITEMS {
            return Err("GAMEBANANA_COLLECTION_TOO_MANY_ITEMS".to_owned());
        }
        records.extend(page_records.iter().cloned());
        complete = response
            .pointer("/_aMetadata/_bIsComplete")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if complete || page_records.is_empty() {
            break;
        }
    }
    if !complete && !records.is_empty() {
        return Err("GAMEBANANA_COLLECTION_TOO_MANY_PAGES".to_owned());
    }

    let total = records.len();
    let mut imported = 0usize;
    let mut skipped = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let item_id = record.get("_idRow").and_then(provider_id);
        let model = record
            .get("_sModelName")
            .and_then(safe_collection_model);
        let current_item = match (item_id, model.as_deref()) {
            (Some(item_id), Some(model)) => format!("{model} {item_id}"),
            _ => "invalid GameBanana collection item".to_owned(),
        };
        emit_collection_progress(
            app,
            &operation_id,
            collection_id,
            "resolving",
            index,
            total,
            Some(&current_item),
            imported,
            skipped.len(),
        );
        let (Some(item_id), Some(model)) = (item_id, model) else {
            skipped.push("invalid-record".to_owned());
            continue;
        };
        let profile = {
            let runtime = state
                .network_runtime
                .lock()
                .map_err(|_| error::internal())?;
            runtime
                .block_on(api.submission_profile::<Value>(&model, item_id))
        };
        let profile = match profile {
            Ok(profile) => profile,
            Err(_) => {
                skipped.push(format!("{current_item}: profile-request-failed"));
                continue;
            }
        };
        let candidates = match collection_file_candidates(&profile) {
            Ok(candidates) => candidates,
            Err(reason) => {
                skipped.push(format!("{current_item}: {reason}"));
                continue;
            }
        };
        let Some(candidate) = choose_collection_file(dialogs, &candidates)? else {
            skipped.push(format!("{current_item}: no-compatible-file"));
            continue;
        };
        let source = match LegacySourceMetadata::new(item_id.to_string(), model.clone()) {
            Ok(source) => source,
            Err(_) => {
                skipped.push(format!("{current_item}: invalid-source"));
                continue;
            }
        };
        emit_collection_progress(
            app,
            &operation_id,
            collection_id,
            "downloading",
            index,
            total,
            Some(&current_item),
            imported,
            skipped.len(),
        );
        let item_operation = Uuid::new_v4().simple().to_string();
        match download_and_import(
            app,
            state,
            dialogs,
            &candidate.url,
            item_operation,
            Some(&source),
        ) {
            Ok(Value::Bool(true)) => imported += 1,
            Ok(_) | Err(_) => skipped.push(format!("{current_item}: import-failed")),
        }
        emit_collection_progress(
            app,
            &operation_id,
            collection_id,
            "item-complete",
            index.saturating_add(1),
            total,
            Some(&current_item),
            imported,
            skipped.len(),
        );
    }
    emit_collection_progress(
        app,
        &operation_id,
        collection_id,
        "complete",
        total,
        total,
        None,
        imported,
        skipped.len(),
    );
    Ok(json!({
        "done": true,
        "operationId": operation_id,
        "collectionId": collection_id,
        "imported": imported,
        "skipped": skipped.len(),
        "skippedMods": skipped
    }))
}

fn protocol_operation_id() -> String {
 -> String {
    Uuid::new_v4().to_string()
}

fn protocol_current_item(item_id: u32, file_id: u32) -> String {
    format!("GameBanana item {item_id}, file {file_id}")
}

pub(crate) fn protocol_source_file_id(source_url: &str) -> Option<u32> {
    validate_download_url(source_url, HostAllowlist::GAMEBANANA).ok()?;
    let authority_and_path = source_url.strip_prefix("https://")?;
    let (authority, path) = authority_and_path.split_once('/')?;
    if authority.contains([':', '@'])
        || !authority
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return None;
    }
    let file = path.strip_prefix("mmdl/")?;
    if file.is_empty()
        || file.contains(['/', '?', '#'])
        || !file.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    file.parse::<u32>()
        .ok()
        .filter(|file_id| *file_id > 0 && *file_id <= MAX_PROTOCOL_ID)
}

pub(crate) fn protocol_source_matches_file_id(source_url: &str, file_id: u32) -> bool {
    protocol_source_file_id(source_url) == Some(file_id)
}

fn validate_protocol_import_request(
    item_id: u32,
    file_id: u32,
    source_url: &str,
) -> Result<(), String> {
    if item_id == 0
        || item_id > MAX_PROTOCOL_ID
        || file_id == 0
        || file_id > MAX_PROTOCOL_ID
        || !protocol_source_matches_file_id(source_url, file_id)
    {
        return Err(PROTOCOL_DOWNLOAD_FAILED.into());
    }
    Ok(())
}

fn protocol_percentage(completed: u64, total: Option<u64>) -> Option<u64> {
    total
        .filter(|total| *total > 0)
        .map(|total| ((u128::from(completed) * 100) / u128::from(total)) as u64)
}

fn protocol_download_progress_payload(
    operation_id: &str,
    completed: u64,
    total: Option<u64>,
    current_item: &str,
) -> Value {
    json!({
        "operationId": operation_id,
        "phase": "download",
        "completed": completed,
        "total": total.unwrap_or(0),
        "currentItem": current_item,
        "percentage": protocol_percentage(completed, total)
    })
}

pub(crate) fn run_protocol_import<D: ChoiceBackend>(
    app: &AppHandle,
    state: &AppState,
    dialogs: &D,
    request: ProtocolImportRequest<'_>,
    cancel: &watch::Receiver<bool>,
    with_current_generation: &(dyn Fn(&mut dyn FnMut()) + Sync),
) -> Result<Value, String> {
    let ProtocolImportRequest {
        item_id,
        file_id,
        source_url,
    } = request;
    validate_protocol_import_request(item_id, file_id, source_url)?;
    if *cancel.borrow() {
        return Err(PROTOCOL_DOWNLOAD_FAILED.into());
    }
    let source = LegacySourceMetadata::new(item_id.to_string(), "Mod")
        .map_err(|_| PROTOCOL_IMPORT_FAILED.to_owned())?;
    let operation_id = protocol_operation_id();
    let current_item = protocol_current_item(item_id, file_id);
    let runtime = state
        .network_runtime
        .lock()
        .map_err(|_| PROTOCOL_DOWNLOAD_FAILED.to_owned())?;
    let downloaded = runtime.block_on(state.network.download_allowlisted(
        operation_id,
        source_url,
        HostAllowlist::GAMEBANANA,
        DownloadPolicy::mods(),
        cancel,
        |progress| {
            if *cancel.borrow() {
                return;
            }
            let payload = protocol_download_progress_payload(
                &progress.operation_id,
                progress.completed,
                progress.total,
                &current_item,
            );
            let mut emit = || {
                if !*cancel.borrow() {
                    let _ = app.emit("protocol-download-progress", &payload);
                }
            };
            with_current_generation(&mut emit);
        },
    ));
    drop(runtime);
    let downloaded = downloaded.map_err(|_| PROTOCOL_DOWNLOAD_FAILED.to_owned())?;
    if *cancel.borrow() {
        return Err(PROTOCOL_DOWNLOAD_FAILED.into());
    }
    let imported = run_import(
        dialogs,
        &downloaded.path,
        &state.data_root.root.join("packets"),
        Some(&source),
        || *cancel.borrow(),
    )
    .map_err(|_| PROTOCOL_IMPORT_FAILED.to_owned())?;
    if *cancel.borrow() {
        return Err(PROTOCOL_IMPORT_FAILED.into());
    }
    Ok(imported)
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

fn cancel_game_download(
    operations: &HashMap<String, CancellationToken>,
    data: &[Value],
) -> Result<Option<Value>, String> {
    let operation = data
        .first()
        .ok_or_else(|| error::invalid("cancelGameImport"))?;
    // Local copies emit numeric IDs and are owned by workflows::dispatch.
    if operation.as_u64().is_some() {
        return Ok(None);
    }
    let operation_id = operation
        .as_str()
        .ok_or_else(|| error::invalid("cancelGameImport"))?;
    if let Some(token) = operations.get(operation_id) {
        token.cancel();
        Ok(Some(json!(true)))
    } else if operation_id.parse::<u64>().is_ok() {
        Ok(None)
    } else {
        // Downloads can finish between their final progress event and a cancel
        // click. Preserve the existing no-op response for an expired token.
        Ok(Some(json!(false)))
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
        "gamebanana_downloadAllInCollection" => {
            restore_gamebanana_collection(app, _state, dialogs, data).map(Some)
        },
        "downloadGame" => download_game(app, _state, data).map(Some),
        "cancelGameImport" => {
            let operations = _state
                .game_download_cancellations
                .lock()
                .map_err(|_| error::internal())?;
            cancel_game_download(&operations, data)
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
    fn copy_cancellation_ids_pass_through_without_cancelling_downloads() {
        let token = CancellationToken::default();
        let operations = HashMap::from([(Uuid::new_v4().to_string(), token.clone())]);
        for operation in [json!(1), json!("1"), json!(0), json!(u64::MAX)] {
            assert_eq!(cancel_game_download(&operations, &[operation]), Ok(None));
            assert!(!token.is_cancelled());
        }
    }

    #[test]
    fn download_cancellation_targets_only_the_registered_uuid() {
        let id = Uuid::new_v4().to_string();
        let selected = CancellationToken::default();
        let other = CancellationToken::default();
        let operations = HashMap::from([
            (id.clone(), selected.clone()),
            (Uuid::new_v4().to_string(), other.clone()),
        ]);
        assert_eq!(
            cancel_game_download(&operations, &[json!(id)]),
            Ok(Some(json!(true)))
        );
        assert!(selected.is_cancelled());
        assert!(!other.is_cancelled());
        assert_eq!(
            cancel_game_download(&operations, &[json!(Uuid::new_v4().to_string())]),
            Ok(Some(json!(false)))
        );
    }

    #[test]
    fn malformed_cancellation_ids_are_rejected() {
        let operations = HashMap::new();
        assert!(cancel_game_download(&operations, &[]).is_err());
        for operation in [json!(-1), json!(1.5), json!(null), json!([]), json!({})] {
            assert!(cancel_game_download(&operations, &[operation]).is_err());
        }
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
    fn protocol_source_identity_is_bound_to_the_exact_mmdl_file_path() {
        assert_eq!(
            protocol_source_file_id("https://gamebanana.com/mmdl/456"),
            Some(456)
        );
        assert_eq!(
            protocol_source_file_id("https://files.gamebanana.com/mmdl/456"),
            Some(456)
        );
        assert!(protocol_source_matches_file_id(
            "https://gamebanana.com/mmdl/456",
            456
        ));
        assert!(!protocol_source_matches_file_id(
            "https://gamebanana.com/mmdl/456",
            457
        ));
    }

    #[test]
    fn protocol_source_identity_rejects_arbitrary_or_ambiguous_urls_before_download() {
        for source in [
            "http://gamebanana.com/mmdl/456",
            "https://gamebanana.com.evil.example/mmdl/456",
            "https://gamebanana.com/dl/456",
            "https://gamebanana.com/mods/123",
            "https://gamebanana.com/mmdl/456/extra",
            "https://gamebanana.com/mmdl/456?download=1",
            "https://gamebanana.com/mmdl/456#download",
            "https://gamebanana.com/mmdl/%34%35%36",
            "https://gamebanana.com:443/mmdl/456",
            "https://gamebanana.com\\evil/mmdl/456",
            "https://gamebanana.com%2fevil/mmdl/456",
        ] {
            assert_eq!(protocol_source_file_id(source), None, "{source}");
        }
        assert_eq!(
            validate_protocol_import_request(123, 457, "https://gamebanana.com/mmdl/456"),
            Err(PROTOCOL_DOWNLOAD_FAILED.into())
        );
        assert_eq!(
            validate_protocol_import_request(0, 456, "https://gamebanana.com/mmdl/456"),
            Err(PROTOCOL_DOWNLOAD_FAILED.into())
        );
    }

    #[test]
    fn protocol_progress_matches_the_electron_shape_and_flooring() {
        assert_eq!(
            protocol_download_progress_payload(
                "4b9ff748-39df-4a4d-9402-d29e8ca8c8b2",
                1,
                Some(3),
                "GameBanana item 12, file 13",
            ),
            json!({
                "operationId": "4b9ff748-39df-4a4d-9402-d29e8ca8c8b2",
                "phase": "download",
                "completed": 1,
                "total": 3,
                "currentItem": "GameBanana item 12, file 13",
                "percentage": 33
            })
        );
    }

    #[test]
    fn protocol_progress_uses_null_percentage_when_total_is_unknown() {
        let payload = protocol_download_progress_payload(
            "4b9ff748-39df-4a4d-9402-d29e8ca8c8b2",
            512,
            None,
            "GameBanana item 12, file 13",
        );
        assert_eq!(payload["total"], json!(0));
        assert_eq!(payload["percentage"], Value::Null);
        assert_eq!(payload.as_object().unwrap().len(), 6);
    }

    #[test]
    fn protocol_progress_does_not_expose_remote_urls() {
        let current_item = protocol_current_item(u32::MAX, u32::MAX);
        assert!(current_item.len() <= 64);
        assert!(current_item
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b',')));
        assert!(!current_item.contains("://"));
        assert!(!PROTOCOL_DOWNLOAD_FAILED.contains("://"));
        assert!(!PROTOCOL_IMPORT_FAILED.contains("://"));
    }

    #[test]
    fn protocol_operation_ids_are_unique_uuid_values() {
        let first = protocol_operation_id();
        let second = protocol_operation_id();
        assert_ne!(first, second);
        assert!(Uuid::parse_str(&first).is_ok());
        assert!(Uuid::parse_str(&second).is_ok());
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
