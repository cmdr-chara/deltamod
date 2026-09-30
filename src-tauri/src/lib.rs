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
use deltamod_tauri_os_adapters::{
    validate_dialog_selection, AdapterError, ChoiceBackend, DialogFilter, DialogRequest,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::Ordering,
        Arc,
    },
};

pub struct HeadlessBackend {
    state: state::AppState,
    credentials: Option<CredentialStore<KeyringBackend>>,
}

impl HeadlessBackend {
    pub fn open(data_root: PathBuf, resources: PathBuf) -> Result<Self, String> {
        let state = state::AppState::initialize(data_root, resources)
            .map_err(str::to_owned)?;
        // Recovery runs before the caller can mutate the managed library.
        headless_channels::lifecycle::recover_startup(&state)?;
        let credentials = CredentialStore::new(Arc::new(KeyringBackend::new())).ok();
        Ok(Self { state, credentials })
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

    #[cfg(test)]
    pub fn state(&self) -> &state::AppState {
        &self.state
    }
}
