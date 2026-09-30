// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
#![forbid(unsafe_code)]

//! Windowless access to Deltamod's production Rust runtime.
//!
//! The Tauri binary remains unchanged. GPUIX links this library to reuse the
//! exact lifecycle, archive, launch, storage, patching and credential contracts
//! without starting a WebView or copying those implementations.

mod error;
mod headless_channels;
mod profile_registry;
mod provider_cache;
pub mod state;

// state.rs references the shell protocol installer only from initialize_with_app.
// The library's windowless initializer never calls it. Keeping this stub in the
// library crate avoids pulling controller/window ownership into headless callers.
pub mod controller {
    pub fn install_protocols(_app: &tauri::AppHandle) -> Result<(), &'static str> {
        Err("protocol registration belongs to the Tauri shell")
    }
}

use deltamod_credentials_adapter::{CredentialKind, CredentialStore, KeyringBackend};
use deltamod_patching_runtime::LifecycleStorageRoots;
use deltamod_protocol_domain::{parse_deep_link, CommunityAction, MAX_ID, MAX_URI_BYTES};
use deltamod_tools_runtime::{controller_mode_launch, verify_tool, OwnedProcess, ProcessRegistry, ToolKind};
use deltamod_tauri_os_adapters::{
    validate_dialog_selection, AdapterError, ChoiceBackend, DialogFilter, DialogRequest,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub struct HeadlessBackend {
    state: state::AppState,
    managed_operation_active: AtomicBool,
    controller_registry: ProcessRegistry,
    controller_process: Mutex<Option<OwnedProcess>>,
    controller_executable: PathBuf,
    protocol_cancel: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
}

impl HeadlessBackend {
    pub fn open(data_root: PathBuf, resources: PathBuf) -> Result<Self, String> {
        let controller_executable = resources.join("tools").join("cmodeutil.exe");
        let mut state = state::AppState::initialize(data_root, resources)
            .map_err(str::to_owned)?;
        state.credentials = CredentialStore::new(Arc::new(KeyringBackend::new())).ok();
        // Recovery runs before the caller can mutate the managed library.
        headless_channels::lifecycle::recover_startup(&state)?;
        Ok(Self {
            state,
            managed_operation_active: AtomicBool::new(false),
            controller_registry: ProcessRegistry::default(),
            controller_process: Mutex::new(None),
            controller_executable,
            protocol_cancel: Mutex::new(None),
        })
    }

    pub fn invoke(&self, channel: &str, data: &[Value]) -> Result<Value, String> {
        if channel.len() > error::MAX_CHANNEL_BYTES || !channel.is_ascii() || data.len() > 32 {
            return Err(error::invalid("headless"));
        }
        if let Some(value) = headless_channels::game::dispatch(&self.state, channel, data)? {
            return Ok(value);
        }
        if let Some(value) = headless_channels::lifecycle::dispatch(&self.state, channel, data)? {
            return Ok(value);
        }
        if let Some(value) = headless_channels::runtime::dispatch(&self.state, channel, data)? {
            return Ok(value);
        }
        if let Some(value) = headless_channels::installations::dispatch(&self.state, channel, data)? {
            return Ok(value);
        }
        if let Some(value) = headless_channels::protocol::dispatch(&self.state, channel, data)? {
            return Ok(value);
        }
        Err(error::unavailable(channel))
    }

    /// Import a local archive into the managed packet library. The existing
    /// archive importer validates/bounds the archive and publishes atomically.
    /// Immediately querying the lifecycle catalogue adopts the result into the
    /// durable lifecycle store so later update/repair/uninstall uses journals.
    pub fn import_archive(&self, source: &Path, replace_existing: bool) -> Result<Value, String> {
        struct FixedChoice(bool);
        impl ChoiceBackend for FixedChoice {
            fn choose(
                &self,
                _title: &str,
                _message: &str,
                _choices: &[String],
            ) -> Result<Option<usize>, AdapterError> {
                Ok(Some(if self.0 { 0 } else { 2 }))
            }
        }

        let request = DialogRequest::file("Choose a Deltamod compatible archive").filter(
            DialogFilter::new("Deltamod compatible archive", ["zip", "7z", "gz", "lzma"])
                .map_err(|_| error::internal())?,
        );
        let source = validate_dialog_selection(&request, source.to_path_buf())
            .map_err(|_| error::invalid("managed:importArchive"))?;
        let packet_root = self.state.data_root.root.join("packets");
        let result = headless_channels::import_download::run_import(
            &FixedChoice(replace_existing),
            &source,
            &packet_root,
            None,
            || false,
        )?;
        if result == json!(true) {
            let catalog = headless_channels::lifecycle::dispatch(
                &self.state,
                "lifecycle:getInstalledMods",
                &[],
            )?
            .ok_or_else(error::internal)?;
            return Ok(json!({ "imported": true, "catalog": catalog }));
        }
        Ok(json!({ "imported": false, "catalog": Value::Null }))
    }

    pub fn patch_and_run(&self, selected: &[String]) -> Result<Value, String> {
        if self.managed_operation_active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err("MANAGED_OPERATION_IN_PROGRESS".into());
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> { fn drop(&mut self) { self.0.store(false, Ordering::Release); } }
        let _reset = Reset(&self.managed_operation_active);
        if selected.len() > 1000
            || selected.iter().any(|id| {
                id.is_empty() || id.len() > 256 || id.chars().any(char::is_control)
            })
        {
            return Err(error::invalid("managed:patchAndRun"));
        }
        let mut selected = selected.to_vec();
        selected.sort();
        selected.dedup();
        let catalog = headless_channels::runtime::mod_list(&self.state, "managed:patchAndRun")?;
        let records = catalog.as_array().ok_or_else(error::internal)?;
        if selected.iter().any(|id| {
            !records.iter().any(|record| {
                record.get("uid").and_then(Value::as_str) == Some(id.as_str())
                    && record.get("isIncompatible").and_then(Value::as_bool) != Some(true)
            })
        }) {
            return Err("A selected mod is unavailable or incompatible with the active game installation.".into());
        }

        self.state.patch_cancelled.store(false, Ordering::Release);
        let sequence = self.state.patch_sequence.fetch_add(1, Ordering::Relaxed);
        let operation_id = format!("gpui-patch-{}-{sequence}", std::process::id());
        let hash_checks = self
            .state
            .preferences
            .lock()
            .map_err(|_| error::internal())?
            .unique_flags
            .get("HASHCHECKS")
            .copied()
            .unwrap_or(false);
        let lifecycle = LifecycleStorageRoots {
            store: self.state.data_root.root.join("lifecycle-store"),
            workspace: self.state.data_root.root.join("lifecycle-workspaces"),
        };
        if hash_checks {
            self.state
                .patching
                .check_selected_legacy_mods(&selected)
                .map_err(|error| error.to_string())?;
        }
        let result = self
            .state
            .patching
            .patch_and_run(
                &selected,
                &operation_id,
                &lifecycle,
                &self.state.game,
                |_progress| {},
                || self.state.patch_cancelled.load(Ordering::Acquire),
            )
            .map_err(|error| error.to_string())?;
        if result.patched {
            let mods = self
                .state
                .patching
                .mark_selected_patched(&selected)
                .map_err(|error| error.to_string())?;
            Ok(json!({ "ok": true, "operationId": operation_id, "mods": mods }))
        } else {
            Ok(json!({ "ok": false, "operationId": operation_id }))
        }
    }

    pub fn cancel_patch(&self) -> bool {
        if !self.managed_operation_active.load(Ordering::Acquire) {
            return false;
        }
        !self.state.patch_cancelled.swap(true, Ordering::AcqRel)
    }

    pub fn precalc_hashes(&self) -> Result<Value, String> {
        self.state.patch_cancelled.store(false, Ordering::Release);
        let sequence = self.state.patch_sequence.fetch_add(1, Ordering::Relaxed);
        let operation_id = format!("gpui-hash-{}-{sequence}", std::process::id());
        let result = self
            .state
            .patching
            .precalc_game_hashes(
                &operation_id,
                |_progress| {},
                || self.state.patch_cancelled.load(Ordering::Acquire),
            )
            .map_err(|error| error.to_string())?;
        serde_json::to_value(result).map_err(|_| error::internal())
    }

    pub fn mod_states(&self) -> Result<Value, String> {
        let state = self
            .state
            .mods_themes
            .mods()
            .state()
            .map_err(|_| error::internal())?;
        Ok(json!({
            "enabled": state.enabled.into_iter().map(|uid| uid.to_string()).collect::<Vec<_>>()
        }))
    }

    pub fn credential_status(&self) -> Result<Value, String> {
        let store = self
            .state
            .credentials
            .as_ref()
            .ok_or_else(|| "CREDENTIALS_UNAVAILABLE".to_owned())?;
        serde_json::to_value(store.status().map_err(|_| "CREDENTIALS_UNAVAILABLE".to_owned())?)
            .map_err(|_| error::internal())
    }

    pub fn clear_credential(&self, kind: &str) -> Result<Value, String> {
        let kind = match kind {
            "gamebanana" => CredentialKind::GameBananaCookies,
            "nexus" => CredentialKind::NexusOAuthTokens,
            "nexus-legacy" => CredentialKind::NexusLegacySsoKey,
            _ => return Err(error::invalid("managed:credentialClear")),
        };
        self.credentials
            .as_ref()
            .ok_or_else(|| "CREDENTIALS_UNAVAILABLE".to_owned())?
            .clear(kind)
            .map_err(|_| "CREDENTIALS_UNAVAILABLE".to_owned())?;
        Ok(json!(true))
    }

    pub fn controller_status(&self) -> Value {
        json!({
            "supported": cfg!(target_os = "windows"),
            "active": self.controller_process.lock().is_ok_and(|process| process.is_some())
        })
    }

    pub fn controller_start(&self) -> Result<Value, String> {
        if !cfg!(target_os = "windows") {
            return Err("CONTROLLER_MODE_UNSUPPORTED".into());
        }
        let mut process = self.controller_process.lock().map_err(|_| "CONTROLLER_MODE_UNAVAILABLE")?;
        if process.is_none() {
            const SHA256: &str = "04ACDBB53C96CD99B01FE53A0297AC06308DDAD14B5253A3AF4F9A319985AA45";
            let tool = verify_tool(&self.controller_executable, ToolKind::ControllerMode, Some(SHA256))
                .map_err(|_| "CONTROLLER_MODE_UNAVAILABLE")?;
            *process = Some(self.controller_registry.spawn_silent(&controller_mode_launch(&tool))
                .map_err(|_| "CONTROLLER_MODE_UNAVAILABLE")?);
        }
        Ok(self.controller_status())
    }

    pub fn controller_stop(&self) -> Result<Value, String> {
        let process = self.controller_process.lock().map_err(|_| "CONTROLLER_MODE_UNAVAILABLE")?.take();
        if let Some(process) = process {
            process.terminate().map_err(|_| "CONTROLLER_MODE_UNAVAILABLE")?;
        }
        Ok(self.controller_status())
    }

    pub fn protocol_review(&self, raw: &str) -> Result<Value, String> {
        match protocol_action(raw)? {
            CommunityAction::Import { item_id, file_id, .. } =>
                Ok(json!({"kind":"import","itemId":item_id,"fileId":file_id})),
            CommunityAction::Launch { item_id } =>
                Ok(json!({"kind":"launch","itemId":item_id})),
        }
    }

    pub fn protocol_import(&self, raw: &str, replace_existing: bool) -> Result<Value, String> {
        struct FixedChoice(bool);
        impl ChoiceBackend for FixedChoice {
            fn choose(&self, _title: &str, _message: &str, _choices: &[String]) -> Result<Option<usize>, AdapterError> {
                Ok(Some(if self.0 { 0 } else { 2 }))
            }
        }
        let CommunityAction::Import { item_id, file_id, source } = protocol_action(raw)? else {
            return Err(error::invalid("managed:protocolImport"));
        };
        let (sender, receiver) = tokio::sync::watch::channel(false);
        {
            let mut active = self.protocol_cancel.lock().map_err(|_| "PROTOCOL_IMPORT_UNAVAILABLE")?;
            if active.is_some() { return Err("PROTOCOL_IMPORT_IN_PROGRESS".into()); }
            *active = Some(sender);
        }
        struct Clear<'a>(&'a Mutex<Option<tokio::sync::watch::Sender<bool>>>);
        impl Drop for Clear<'_> { fn drop(&mut self) { if let Ok(mut active)=self.0.lock(){*active=None;} } }
        let _clear = Clear(&self.protocol_cancel);
        let imported = headless_channels::import_download::run_protocol_import_headless(
            &self.state,
            &FixedChoice(replace_existing),
            headless_channels::import_download::ProtocolImportRequest {
                item_id, file_id, source_url: &source,
            },
            &receiver,
        )?;
        if imported == json!(true) {
            let catalog = headless_channels::lifecycle::dispatch(&self.state, "lifecycle:getInstalledMods", &[])?
                .ok_or_else(error::internal)?;
            return Ok(json!({"imported":true,"catalog":catalog}));
        }
        Ok(json!({"imported":false,"catalog":Value::Null}))
    }

    pub fn cancel_protocol_import(&self) -> bool {
        self.protocol_cancel.lock().ok().and_then(|active| active.as_ref().cloned())
            .is_some_and(|sender| sender.send(true).is_ok())
    }

    pub fn nexus_login(&self) -> Result<Value, String> {
        let value = headless_channels::nexus_oauth::start_with_opener(&self.state, |url| {
            open_system_url(url).map_err(|_| ())
        });
        if value.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(value)
        } else {
            let code = value.pointer("/error/code").and_then(Value::as_str).unwrap_or("NEXUS_SSO_FAILED");
            let message = value.pointer("/error/message").and_then(Value::as_str).unwrap_or("Nexus Mods sign-in failed.");
            Err(format!("{code}: {message}"))
        }
    }

    pub fn cancel_nexus_login(&self) -> bool {
        headless_channels::nexus_oauth::cancel(&self.state)
    }

    #[cfg(test)]
    pub fn state(&self) -> &state::AppState {
        &self.state
    }
}


fn open_system_url(url: &str) -> Result<(), &'static str> {
    let parsed = url::Url::parse(url).map_err(|_| "browser URL invalid")?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host_str() != Some("users.nexusmods.com")
        || parsed.path() != "/oauth/authorize"
    {
        return Err("browser URL rejected");
    }
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command.spawn().map(|_| ()).map_err(|_| "browser unavailable")
}


fn protocol_action(raw: &str) -> Result<CommunityAction, String> {
    if let Ok(action) = parse_deep_link(raw) {
        if let CommunityAction::Import { file_id, source, .. } = &action {
            if !headless_channels::import_download::protocol_source_matches_file_id(source, *file_id) {
                return Err("PROTOCOL_SOURCE_MISMATCH".into());
            }
        }
        return Ok(action);
    }
    if raw.len() > MAX_URI_BYTES || raw.contains(['?', '#']) || raw.chars().any(char::is_control) {
        return Err("PROTOCOL_INVALID".into());
    }
    let legacy = raw.strip_prefix("deltamod-community://gb/Mod/").ok_or("PROTOCOL_INVALID")?;
    let (item, source) = legacy.split_once('/').ok_or("PROTOCOL_INVALID")?;
    if item.is_empty() || !item.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("PROTOCOL_INVALID".into());
    }
    let item_id = item.parse::<u32>().map_err(|_| "PROTOCOL_INVALID")?;
    if item_id == 0 || item_id > MAX_ID { return Err("PROTOCOL_INVALID".into()); }
    let file_id = headless_channels::import_download::protocol_source_file_id(source)
        .ok_or("PROTOCOL_INVALID")?;
    Ok(CommunityAction::Import { item_id, file_id, source: source.to_owned() })
}
