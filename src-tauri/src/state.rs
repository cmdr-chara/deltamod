// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
//! Preserve the existing state implementation behind a shared writer lease.
//! The lease is acquired before recovery/migration and held until every runtime
//! field has been dropped. This is cooperative with builds using this boundary.
#[path = "state_impl.rs"]
mod implementation;
#[path = "../../native/desktop-runtime/src/lease.rs"]
mod runtime_lease;

#[allow(unused_imports)]
pub use implementation::{EasterEggWindowState, Preferences};
#[cfg(deltamod_tauri_shell)]
#[allow(unused_imports)]
pub use implementation::UpdateEvents;
#[allow(unused_imports)]
#[cfg(deltamod_tauri_shell)]
pub(crate) use implementation::{DownloadedUpdate, ShellUpdater, TauriUpdaterHost};
use deltamod_storage_domain::DataRoot;
use std::{ops::{Deref, DerefMut}, path::PathBuf};

pub struct AppState {
    inner: implementation::AppState,
    // Rust drops fields in declaration order: release after the runtime.
    _lease: runtime_lease::RuntimeLease,
}

impl AppState {
    fn claim(data_dir: PathBuf) -> Result<(PathBuf, runtime_lease::RuntimeLease), &'static str> {
        let root = DataRoot::new(data_dir).map_err(|_| "state root unavailable")?;
        let lease = runtime_lease::RuntimeLease::acquire(&root.root)
            .map_err(|_| "managed data root is already in use or cannot be locked")?;
        Ok((root.root.clone(), lease))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn initialize(data_dir: PathBuf, resource_dir: PathBuf) -> Result<Self, &'static str> {
        let (data_dir, lease) = Self::claim(data_dir)?;
        let inner = implementation::AppState::initialize(data_dir, resource_dir)?;
        Ok(Self { inner, _lease: lease })
    }

    #[cfg(deltamod_tauri_shell)]
    pub fn initialize_with_app(data_dir: PathBuf, resource_dir: PathBuf, app: tauri::AppHandle) -> Result<Self, &'static str> {
        let (data_dir, lease) = Self::claim(data_dir)?;
        let inner = implementation::AppState::initialize_with_app(data_dir, resource_dir, app)?;
        Ok(Self { inner, _lease: lease })
    }
}

impl Deref for AppState {
    type Target = implementation::AppState;
    fn deref(&self) -> &Self::Target { &self.inner }
}
impl DerefMut for AppState {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}
