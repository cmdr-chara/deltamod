#![forbid(unsafe_code)]
//! Tauri-independent filesystem adapter for profile and installation channels.
//!
//! The adapter owns persistence and transactions. A UI integration only needs to
//! translate channel arguments and forward [`ProgressEvent`] values.

use deltamod_installations_domain::{
    allocate_installation_id, Edition, GamePlatform, Installation, InstallationId,
    InstallationListResponse, InstallationResponse, OperationResponse, OperationState, Ownership,
};
use deltamod_native_core::staged_copy::{
    copy_directory_staged, inspect_source_tree, StagedCopyError,
};
use deltamod_native_core::{patch_plan, patch_transaction};
use deltamod_storage_domain::{
    atomic_write_bytes, atomic_write_json, load_json, InstallationRecord, ProfileStore,
    StorageError,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("domain: {0}")]
    Domain(String),
    #[error("copy: {0}")]
    Copy(#[from] StagedCopyError),
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    #[error("operation {0} is not available")]
    UnknownOperation(u64),
    #[error("operation {0} is already committed")]
    CommittedOperation(u64),
    #[error("operation {0} was cancelled")]
    Cancelled(u64),
    #[error("journal is invalid: {0}")]
    Journal(String),
    #[error("patch plan: {0}")]
    PatchPlan(#[from] patch_plan::PatchPlanError),
    #[error("patch recovery: {0}")]
    PatchRecovery(#[from] patch_transaction::TransactionError),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ProgressEvent {
    Started {
        operation_id: u64,
        operation: String,
    },
    Phase {
        operation_id: u64,
        phase: String,
        completed: u64,
        total: Option<u64>,
    },
    Warning {
        operation_id: u64,
        message: String,
    },
    Finished {
        operation_id: u64,
        success: bool,
        message: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPlanInput {
    pub game_root: PathBuf,
    pub platform: PatchPlatformInput,
    #[serde(default)]
    pub patches: Vec<PatchCandidateInput>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchPlatformInput {
    Win32,
    Linux,
    Darwin,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchCandidateInput {
    pub patch_type: PatchTypeInput,
    pub patch: String,
    pub to: String,
    pub mapped_target: String,
    pub mod_name: String,
    pub mod_id: String,
    pub mod_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PatchTypeInput {
    Override,
    Copy,
    Xdelta,
    #[serde(alias = "g3mPatch")]
    G3mPatch,
    Csx,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PatchPlanResponse {
    pub operation_count: usize,
    pub patch_count: usize,
    pub snapshot_count: usize,
}

pub const MAX_LEGACY_INSTALLATION_INDEX: u32 = 255;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReimportResponse {
    pub repaired: bool,
    pub operation_id: u64,
    pub destination: PathBuf,
}

pub trait CopyBackend: Send + Sync {
    fn copy(
        &self,
        source: &Path,
        destination: &Path,
        staging: &Path,
        cancelled: &dyn Fn() -> bool,
        progress: &mut dyn FnMut(u64, &str) -> Result<(), StagedCopyError>,
        before_commit: &dyn Fn() -> Result<(), StagedCopyError>,
    ) -> Result<(), StagedCopyError>;
}

#[derive(Debug, Default)]
pub struct StagedCopyBackend;
impl CopyBackend for StagedCopyBackend {
    fn copy(
        &self,
        source: &Path,
        destination: &Path,
        staging: &Path,
        cancelled: &dyn Fn() -> bool,
        progress: &mut dyn FnMut(u64, &str) -> Result<(), StagedCopyError>,
        before_commit: &dyn Fn() -> Result<(), StagedCopyError>,
    ) -> Result<(), StagedCopyError> {
        let inventory = inspect_source_tree(source)?;
        copy_directory_staged(
            &inventory,
            destination,
            staging,
            3,
            progress,
            before_commit,
            cancelled,
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    installations: Vec<Installation>,
    selected_id: Option<InstallationId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Journal {
    version: u32,
    operation_id: u64,
    kind: String,
    source: Option<String>,
    destination: Option<String>,
    staging: Option<String>,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default)]
    backup: Option<String>,
    status: JournalStatus,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum JournalStatus {
    Prepared,
    Committed,
}

#[derive(Debug)]
struct OperationEntry {
    state: OperationState,
    cancel: bool,
}
#[derive(Debug, Default)]
struct Operations {
    next: u64,
    entries: BTreeMap<u64, OperationEntry>,
}

#[derive(Clone)]
pub struct Runtime {
    root: PathBuf,
    state_path: PathBuf,
    operations: Arc<Mutex<Operations>>,
    copy: Arc<dyn CopyBackend>,
    events: Arc<Mutex<Vec<ProgressEvent>>>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime").field("root", &self.root).finish()
    }
}

impl Runtime {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        Self::with_backend(root, Arc::new(StagedCopyBackend))
    }

    pub fn with_backend(
        root: impl Into<PathBuf>,
        copy: Arc<dyn CopyBackend>,
    ) -> Result<Self, RuntimeError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(root)?;
        fs::create_dir_all(root.join("installations"))?;
        fs::create_dir_all(root.join(".runtime-journals"))?;
        let runtime = Self {
            state_path: root.join("installations-adapter.json"),
            root,
            operations: Arc::new(Mutex::new(Operations::default())),
            copy,
            events: Arc::new(Mutex::new(Vec::new())),
        };
        runtime.recover()?;
        Ok(runtime)
    }

    pub fn events(&self) -> Result<Vec<ProgressEvent>, RuntimeError> {
        Ok(self
            .events
            .lock()
            .map_err(|_| RuntimeError::Journal("event lock poisoned".into()))?
            .clone())
    }
    pub fn clear_events(&self) -> Result<(), RuntimeError> {
        self.events
            .lock()
            .map_err(|_| RuntimeError::Journal("event lock poisoned".into()))?
            .clear();
        Ok(())
    }
    pub fn drain_events(&self) -> Result<Vec<ProgressEvent>, RuntimeError> {
        Ok(self
            .events
            .lock()
            .map_err(|_| RuntimeError::Journal("event lock poisoned".into()))?
            .drain(..)
            .collect())
    }

    pub fn get_official_profile_summary(&self) -> Result<Value, RuntimeError> {
        let path = self.root.join("official-profile.json");
        if path.is_file() {
            return Ok(load_json(&path)?);
        }
        Ok(json!({ "installed": false, "path": Value::Null }))
    }

    /// `source` is the path selected by the host UI; this method never opens a dialog.
    pub fn import_official_profile(
        &self,
        source: &Path,
    ) -> Result<OperationResponse, RuntimeError> {
        self.copy_operation(
            "official-profile-import",
            source,
            &self.root.join("official-profile"),
            None,
        )
    }
    pub fn cancel(&self, id: u64) -> Result<(), RuntimeError> {
        let mut ops = self
            .operations
            .lock()
            .map_err(|_| RuntimeError::Journal("operation lock poisoned".into()))?;
        let op = ops
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownOperation(id))?;
        match op.state {
            OperationState::Running => {
                op.cancel = true;
                op.state = OperationState::Cancelling;
                Ok(())
            }
            OperationState::Committing | OperationState::Completed => {
                Err(RuntimeError::CommittedOperation(id))
            }
            _ => Ok(()),
        }
    }

    pub fn list_installations(&self) -> Result<InstallationListResponse, RuntimeError> {
        Ok(self.state()?.into())
    }
    pub fn create_installation(
        &self,
        source: &Path,
        name: String,
        platform: GamePlatform,
        ownership: Ownership,
    ) -> Result<OperationResponse, RuntimeError> {
        let source = safe_directory(source)?;
        let state = self.state()?;
        let id = allocate_installation_id(
            &source,
            platform,
            &state.installations.iter().map(|x| x.id.clone()).collect(),
        );
        let destination = self.root.join("installations").join(id.0.clone());
        let installation = Installation {
            id,
            name: valid_name(name)?,
            edition: Edition::Original,
            platform,
            source: source.clone(),
            install_path: destination.clone(),
            ownership,
        };
        if ownership == Ownership::LinkedExternal {
            self.persist_added(installation)?;
            return Ok(self.accepted("link"));
        }
        self.copy_operation(
            "installation-create",
            &source,
            &destination,
            Some(installation),
        )
    }
    pub fn copy_installation(
        &self,
        id: &InstallationId,
        name: String,
    ) -> Result<OperationResponse, RuntimeError> {
        let source_installation = self.find(id)?;
        let state = self.state()?;
        let new_id = allocate_installation_id(
            &source_installation.install_path,
            source_installation.platform,
            &state.installations.iter().map(|x| x.id.clone()).collect(),
        );
        let destination = self.root.join("installations").join(&new_id.0);
        let installation = Installation {
            id: new_id,
            name: valid_name(name)?,
            edition: source_installation.edition,
            platform: source_installation.platform,
            source: source_installation.install_path.clone(),
            install_path: destination.clone(),
            ownership: Ownership::ManagedCopy,
        };
        self.copy_operation(
            "installation-copy",
            &source_installation.install_path,
            &destination,
            Some(installation),
        )
    }
    pub fn reimport_installation(
        &self,
        id: &InstallationId,
    ) -> Result<OperationResponse, RuntimeError> {
        let i = self.find(id)?;
        let source = i.source.clone();
        let destination = i.install_path.clone();
        self.copy_operation("installation-reimport", &source, &destination, Some(i))
    }
    pub fn repair_installation(
        &self,
        id: &InstallationId,
    ) -> Result<OperationResponse, RuntimeError> {
        let i = self.find(id)?;
        if i.ownership != Ownership::ManagedCopy {
            return Err(RuntimeError::Domain(
                "linked installations cannot be repaired in place".into(),
            ));
        }
        let source = i.source.clone();
        let destination = i.install_path.clone();
        self.copy_operation("installation-repair", &source, &destination, Some(i))
    }
    pub fn delete_installation(
        &self,
        id: &InstallationId,
        delete_files: bool,
    ) -> Result<OperationResponse, RuntimeError> {
        let mut state = self.state()?;
        let i = state
            .installations
            .iter()
            .find(|x| &x.id == id)
            .cloned()
            .ok_or_else(|| RuntimeError::Domain("installation not found".into()))?;
        if delete_files && i.ownership == Ownership::ManagedCopy {
            self.validate_managed_install_path(&i.install_path)?;
            let journal = self.prepare(
                "installation-delete",
                None,
                Some(&i.install_path),
                None,
                None,
                None,
            )?;
            match fs::remove_dir_all(&i.install_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            self.finish_journal(&journal)?;
        }
        state.installations.retain(|x| &x.id != id);
        if state.selected_id.as_ref() == Some(id) {
            state.selected_id = None;
        }
        self.save_state(&state)?;
        Ok(self.accepted("installation-delete"))
    }
    pub fn set_name(
        &self,
        id: &InstallationId,
        name: String,
    ) -> Result<InstallationResponse, RuntimeError> {
        let mut s = self.state()?;
        let i = s
            .installations
            .iter_mut()
            .find(|x| &x.id == id)
            .ok_or_else(|| RuntimeError::Domain("installation not found".into()))?;
        i.name = valid_name(name)?;
        let out = i.clone();
        self.save_state(&s)?;
        Ok(InstallationResponse { installation: out })
    }
    pub fn set_edition(
        &self,
        id: &InstallationId,
        edition: Edition,
    ) -> Result<InstallationResponse, RuntimeError> {
        let mut s = self.state()?;
        let i = s
            .installations
            .iter_mut()
            .find(|x| &x.id == id)
            .ok_or_else(|| RuntimeError::Domain("installation not found".into()))?;
        i.edition = edition;
        let out = i.clone();
        self.save_state(&s)?;
        Ok(InstallationResponse { installation: out })
    }
    pub fn select(
        &self,
        id: Option<&InstallationId>,
    ) -> Result<InstallationListResponse, RuntimeError> {
        let mut s = self.state()?;
        if let Some(id) = id {
            self.find(id)?;
        }
        s.selected_id = id.cloned();
        self.save_state(&s)?;
        Ok(s.into())
    }

    pub fn prepare_patch_plan(
        &self,
        input: PatchPlanInput,
    ) -> Result<PatchPlanResponse, RuntimeError> {
        let platform = match input.platform {
            PatchPlatformInput::Win32 => patch_plan::PatchPlatform::Win32,
            PatchPlatformInput::Linux => patch_plan::PatchPlatform::Linux,
            PatchPlatformInput::Darwin => patch_plan::PatchPlatform::Darwin,
        };
        let patches = input
            .patches
            .into_iter()
            .map(|candidate| patch_plan::PatchCandidate {
                patch_type: match candidate.patch_type {
                    PatchTypeInput::Override => patch_plan::PatchType::Override,
                    PatchTypeInput::Copy => patch_plan::PatchType::Copy,
                    PatchTypeInput::Xdelta => patch_plan::PatchType::Xdelta,
                    PatchTypeInput::G3mPatch => patch_plan::PatchType::G3mPatch,
                    PatchTypeInput::Csx => patch_plan::PatchType::Csx,
                },
                patch: candidate.patch,
                to: candidate.to,
                mapped_target: candidate.mapped_target,
                mod_name: candidate.mod_name,
                mod_id: candidate.mod_id,
                mod_root: candidate.mod_root,
            })
            .collect();
        let approval = patch_plan::validate_patch_plan(&patch_plan::PatchPlanRequest {
            game_root: input.game_root,
            platform,
            patches,
        })?;
        Ok(PatchPlanResponse {
            operation_count: approval.operation_count,
            patch_count: approval.patch_count,
            snapshot_count: approval.snapshot_count,
        })
    }

    pub fn restore_patch_transaction(&self, game_root: &Path) -> Result<bool, RuntimeError> {
        let game_root = fs::canonicalize(game_root)?;
        let metadata = fs::symlink_metadata(&game_root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(RuntimeError::Domain("invalid game root".into()));
        }
        let journal_path = game_root.join(".deltamod-community-patch-journal.json");
        match fs::symlink_metadata(&journal_path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(RuntimeError::Domain("invalid patch journal".into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        }
        let mut journal: patch_transaction::Journal =
            serde_json::from_slice(&fs::read(&journal_path)?)?;
        patch_transaction::restore(&game_root, &journal_path, &mut journal)?;
        Ok(true)
    }

    /// Legacy channel shape: `getInstallations` returns an array, not an envelope.
    pub fn legacy_installations(&self) -> Result<Value, RuntimeError> {
        let path = self.root.join("profiles").join("installations.json");
        if !path.is_file() {
            return Ok(json!([]));
        }
        let p: ProfileStore = load_json(&path)?;
        Ok(serde_json::to_value(
            p.installations.into_iter().take(256).collect::<Vec<_>>(),
        )?)
    }
    pub fn legacy_system_index(&self) -> Result<Value, RuntimeError> {
        let path = self.root.join("profiles").join("installations.json");
        if !path.is_file() {
            return Ok(json!(0));
        }
        let p: ProfileStore = load_json(&path)?;
        Ok(json!(p.current_index.unwrap_or(0)))
    }

    pub fn legacy_create_installation(
        &self,
        index: u32,
        source: &Path,
        name: String,
        copy_to_managed: bool,
        mut store: serde_json::Map<String, Value>,
    ) -> Result<bool, RuntimeError> {
        let source = safe_directory(source)?;
        let profile = self.legacy_profile_path(index)?;
        if profile.exists() {
            return Err(RuntimeError::Domain(
                "installation profile already exists".into(),
            ));
        }
        fs::create_dir(&profile)?;
        let result = (|| {
            let game_path = if copy_to_managed {
                let destination = profile.join("deltaruneInstall");
                self.copy_operation("legacy-installation-create", &source, &destination, None)?;
                destination
            } else {
                source.clone()
            };
            store.insert(
                "gamePath".into(),
                Value::String(game_path.to_string_lossy().into_owned()),
            );
            atomic_write_json(&profile.join("store.json"), &store, true)?;
            atomic_write_bytes(
                &profile.join("_cname"),
                valid_legacy_name(name)?.as_bytes(),
                false,
            )?;
            self.upsert_legacy_record(index, &store)?;
            Ok(true)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&profile);
        }
        result
    }

    pub fn legacy_store(&self, index: u32) -> Result<serde_json::Map<String, Value>, RuntimeError> {
        let profile = self.existing_legacy_profile(index)?;
        let value: Value = load_json(&profile.join("store.json"))?;
        value
            .as_object()
            .cloned()
            .ok_or_else(|| RuntimeError::Domain("invalid installation store".into()))
    }

    pub fn legacy_set_name(&self, index: u32, name: String) -> Result<bool, RuntimeError> {
        let profile = self.existing_legacy_profile(index)?;
        let name = valid_legacy_name(name)?;
        atomic_write_bytes(&profile.join("_cname"), name.as_bytes(), true)?;
        let path = self.legacy_profile_store_path();
        let mut profiles = self.load_legacy_profiles()?;
        let record = profiles
            .installations
            .iter_mut()
            .find(|record| record.index == Some(index))
            .ok_or_else(|| RuntimeError::Domain("installation profile not indexed".into()))?;
        record.name = Some(name);
        atomic_write_json(&path, &profiles, true)?;
        Ok(true)
    }

    pub fn legacy_change_system_index(&self, index: u32) -> Result<(), RuntimeError> {
        self.existing_legacy_profile(index)?;
        let path = self.legacy_profile_store_path();
        let mut profiles = self.load_legacy_profiles()?;
        if !profiles
            .installations
            .iter()
            .any(|record| record.index == Some(index))
        {
            return Err(RuntimeError::Domain(
                "installation profile not indexed".into(),
            ));
        }
        profiles.current_index = Some(index);
        atomic_write_json(&path, &profiles, true)?;
        Ok(())
    }

    pub fn legacy_reimport_installation(
        &self,
        index: u32,
        source: &Path,
        platform: String,
    ) -> Result<LegacyReimportResponse, RuntimeError> {
        let source = safe_directory(source)?;
        let profile = self.existing_legacy_profile(index)?;
        let default = profile.join("deltaruneInstall");
        let destination = if default.exists() {
            profile.join(format!(
                "deltaruneInstall-reimport-{}",
                self.next_operation_id()?
            ))
        } else {
            default
        };
        let operation =
            self.copy_operation("legacy-installation-reimport", &source, &destination, None)?;
        let update = (|| {
            let mut store = self.legacy_store(index)?;
            store.insert(
                "gamePath".into(),
                Value::String(destination.to_string_lossy().into_owned()),
            );
            store.insert("gamePlatform".into(), Value::String(platform));
            store.insert("loadedDeltarune".into(), Value::Bool(true));
            atomic_write_json(&profile.join("store.json"), &store, true)?;
            self.upsert_legacy_record(index, &store)
        })();
        if let Err(error) = update {
            let _ = fs::remove_dir_all(&destination);
            return Err(error);
        }
        Ok(LegacyReimportResponse {
            repaired: true,
            operation_id: operation.operation_id,
            destination,
        })
    }

    pub fn legacy_delete_installation(&self, index: u32) -> Result<bool, RuntimeError> {
        let profile = self.existing_legacy_profile(index)?;
        let operation_id = self.start("legacy-installation-delete")?;
        let trash = self
            .root
            .join(".runtime-replacements")
            .join(format!("legacy-delete-{operation_id}"));
        fs::create_dir_all(trash.parent().expect("trash has parent"))?;
        let journal = self.prepare(
            "legacy-profile-delete",
            None,
            Some(&profile),
            None,
            Some(&trash),
            None,
        )?;
        fs::rename(&profile, &trash)?;
        let mut profiles = self.load_legacy_profiles()?;
        profiles
            .installations
            .retain(|record| record.index != Some(index));
        profiles.current_index = Some(0);
        if let Err(error) = atomic_write_json(&self.legacy_profile_store_path(), &profiles, true) {
            let _ = fs::rename(&trash, &profile);
            self.abort(operation_id, &journal);
            return Err(error.into());
        }
        fs::remove_dir_all(&trash)?;
        self.finish_journal(&journal)?;
        self.complete_operation(operation_id);
        self.emit(ProgressEvent::Finished {
            operation_id,
            success: true,
            message: None,
        });
        Ok(true)
    }

    /// Resolves the managed folder solely from a validated legacy index.
    pub fn legacy_managed_folder(&self, index: u32) -> Result<PathBuf, RuntimeError> {
        let profile = self.existing_legacy_profile(index)?;
        let folder = profile.join("deltaruneInstall");
        let metadata = fs::symlink_metadata(&folder)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(RuntimeError::Domain(
                "managed installation folder is unavailable".into(),
            ));
        }
        let canonical = fs::canonicalize(folder)?;
        if !canonical.starts_with(&profile) {
            return Err(RuntimeError::Domain(
                "managed installation folder escaped profile".into(),
            ));
        }
        Ok(canonical)
    }

    pub fn legacy_profile_folder(&self, index: u32) -> Result<PathBuf, RuntimeError> {
        self.existing_legacy_profile(index)
    }

    fn next_operation_id(&self) -> Result<u64, RuntimeError> {
        Ok(self
            .operations
            .lock()
            .map_err(|_| RuntimeError::Journal("operation lock poisoned".into()))?
            .next
            + 1)
    }

    fn legacy_profile_path(&self, index: u32) -> Result<PathBuf, RuntimeError> {
        if index > MAX_LEGACY_INSTALLATION_INDEX {
            return Err(RuntimeError::Domain("invalid installation index".into()));
        }
        Ok(self.root.join(format!("deltamod_system-{index}")))
    }

    fn existing_legacy_profile(&self, index: u32) -> Result<PathBuf, RuntimeError> {
        let path = self.legacy_profile_path(index)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(RuntimeError::Domain("invalid installation profile".into()));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(RuntimeError::Domain(
                    "installation profile is a reparse point".into(),
                ));
            }
        }
        let canonical = fs::canonicalize(path)?;
        if canonical.parent() != Some(self.root.as_path()) {
            return Err(RuntimeError::Domain(
                "installation profile escaped data root".into(),
            ));
        }
        Ok(canonical)
    }

    fn legacy_profile_store_path(&self) -> PathBuf {
        self.root.join("profiles").join("installations.json")
    }

    fn load_legacy_profiles(&self) -> Result<ProfileStore, RuntimeError> {
        let path = self.legacy_profile_store_path();
        if path.is_file() {
            Ok(load_json(&path)?)
        } else {
            Ok(ProfileStore::default())
        }
    }

    fn upsert_legacy_record(
        &self,
        index: u32,
        store: &serde_json::Map<String, Value>,
    ) -> Result<(), RuntimeError> {
        let mut profiles = self.load_legacy_profiles()?;
        let record = if let Some(record) = profiles
            .installations
            .iter_mut()
            .find(|record| record.index == Some(index))
        {
            record
        } else {
            profiles.installations.push(InstallationRecord {
                index: Some(index),
                name: Some(format!("Install #{}", index + 1)),
                ..InstallationRecord::default()
            });
            profiles
                .installations
                .last_mut()
                .expect("record was just inserted")
        };
        record.pid = store
            .get("gamePid")
            .and_then(Value::as_str)
            .map(str::to_owned);
        record.steam = store.get("isSteam").and_then(Value::as_bool);
        record.valid = Some(true);
        record.issues = Some(Vec::new());
        record.can_open_in_undertale_mod_tool = Some(cfg!(windows));
        for key in ["steamAppId", "gamePlatform"] {
            if let Some(value) = store.get(key) {
                record.extra.insert(key.to_owned(), value.clone());
            }
        }
        profiles.installations.sort_by_key(|record| record.index);
        atomic_write_json(&self.legacy_profile_store_path(), &profiles, true)?;
        Ok(())
    }

    fn copy_operation(
        &self,
        kind: &str,
        source: &Path,
        destination: &Path,
        installation: Option<Installation>,
    ) -> Result<OperationResponse, RuntimeError> {
        let id = self.start(kind)?;
        // The staged copier requires a missing destination. Reimports stage beside
        // the live directory and publish only after the complete copy succeeds.
        let target = if destination.exists() {
            self.root
                .join(".runtime-replacements")
                .join(format!("{id}"))
        } else {
            destination.to_path_buf()
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let staging = target.with_extension(format!("importing-{id}"));
        let backup = (target != destination)
            .then(|| destination.with_extension(format!("deltamod-replacing-{id}")));
        let journal = self.prepare(
            kind,
            Some(source),
            Some(destination),
            Some(&staging),
            (target != destination).then_some(target.as_path()),
            backup.as_deref(),
        )?;
        let cancelled = || self.is_cancelled(id);
        let mut emit_progress = |done: u64, relative: &str| {
            self.emit(ProgressEvent::Phase {
                operation_id: id,
                phase: "copy".into(),
                completed: done,
                total: None,
            });
            let _ = relative;
            if cancelled() {
                Err(StagedCopyError::Cancelled)
            } else {
                Ok(())
            }
        };
        let result = self.copy.copy(
            source,
            &target,
            &staging,
            &cancelled,
            &mut emit_progress,
            &|| {
                if cancelled() {
                    Err(StagedCopyError::Cancelled)
                } else {
                    Ok(())
                }
            },
        );
        match result {
            Ok(()) => {
                if target != destination {
                    if cancelled() {
                        self.abort(id, &journal);
                        let _ = fs::remove_dir_all(&target);
                        return Err(RuntimeError::Cancelled(id));
                    }
                    let backup = backup.expect("replacement operations have a backup path");
                    if let Err(error) = (|| -> Result<(), std::io::Error> {
                        fs::rename(destination, &backup)?;
                        if let Err(error) = fs::rename(&target, destination) {
                            let _ = fs::rename(&backup, destination);
                            return Err(error);
                        }
                        fs::remove_dir_all(backup)
                    })() {
                        self.abort(id, &journal);
                        let _ = fs::remove_dir_all(&target);
                        return Err(error.into());
                    }
                }
                self.commit_state(id, journal, installation)?;
                self.emit(ProgressEvent::Finished {
                    operation_id: id,
                    success: true,
                    message: None,
                });
                Ok(self.accepted(kind))
            }
            Err(e) => {
                self.abort(id, &journal);
                self.emit(ProgressEvent::Finished {
                    operation_id: id,
                    success: false,
                    message: Some(e.to_string()),
                });
                Err(if matches!(e, StagedCopyError::Cancelled) {
                    RuntimeError::Cancelled(id)
                } else {
                    RuntimeError::Copy(e)
                })
            }
        }
    }
    fn start(&self, kind: &str) -> Result<u64, RuntimeError> {
        let mut o = self
            .operations
            .lock()
            .map_err(|_| RuntimeError::Journal("operation lock poisoned".into()))?;
        o.next += 1;
        let id = o.next;
        o.entries.insert(
            id,
            OperationEntry {
                state: OperationState::Running,
                cancel: false,
            },
        );
        self.emit(ProgressEvent::Started {
            operation_id: id,
            operation: kind.into(),
        });
        Ok(id)
    }
    fn is_cancelled(&self, id: u64) -> bool {
        self.operations
            .lock()
            .ok()
            .and_then(|o| o.entries.get(&id).map(|x| x.cancel))
            .unwrap_or(true)
    }
    fn accepted(&self, kind: &str) -> OperationResponse {
        let _ = kind;
        OperationResponse {
            operation_id: self.operations.lock().ok().map(|o| o.next).unwrap_or(0),
            accepted: true,
        }
    }
    fn emit(&self, event: ProgressEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
    fn state(&self) -> Result<PersistedState, RuntimeError> {
        if self.state_path.is_file() {
            Ok(serde_json::from_slice(&fs::read(&self.state_path)?)?)
        } else {
            Ok(PersistedState::default())
        }
    }
    fn save_state(&self, state: &PersistedState) -> Result<(), RuntimeError> {
        atomic_write_json(&self.state_path, state, true)?;
        Ok(())
    }
    fn find(&self, id: &InstallationId) -> Result<Installation, RuntimeError> {
        self.state()?
            .installations
            .into_iter()
            .find(|x| &x.id == id)
            .ok_or_else(|| RuntimeError::Domain("installation not found".into()))
    }
    fn validate_managed_install_path(&self, path: &Path) -> Result<(), RuntimeError> {
        let managed_root = fs::canonicalize(self.root.join("installations"))?;
        let parent = path
            .parent()
            .ok_or_else(|| RuntimeError::Domain("invalid managed installation path".into()))?;
        let canonical_parent = fs::canonicalize(parent)?;
        if canonical_parent != managed_root || path.file_name().is_none() || path == managed_root {
            return Err(RuntimeError::Domain(
                "managed installation path is outside the managed root".into(),
            ));
        }
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.file_type().is_symlink() {
                return Err(RuntimeError::Domain(
                    "managed installation path is a link".into(),
                ));
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(RuntimeError::Domain(
                        "managed installation path is a reparse point".into(),
                    ));
                }
            }
        }
        Ok(())
    }
    fn persist_added(&self, i: Installation) -> Result<(), RuntimeError> {
        let mut s = self.state()?;
        s.installations.push(i);
        self.save_state(&s)
    }
    fn prepare(
        &self,
        kind: &str,
        source: Option<&Path>,
        destination: Option<&Path>,
        staging: Option<&Path>,
        replacement: Option<&Path>,
        backup: Option<&Path>,
    ) -> Result<PathBuf, RuntimeError> {
        let id = self.operations.lock().ok().map(|o| o.next).unwrap_or(0);
        let path = self
            .root
            .join(".runtime-journals")
            .join(format!("{id}.json"));
        let j = Journal {
            version: 1,
            operation_id: id,
            kind: kind.into(),
            source: source.map(|p| p.to_string_lossy().into()),
            destination: destination.map(|p| p.to_string_lossy().into()),
            staging: staging.map(|p| p.to_string_lossy().into()),
            replacement: replacement.map(|p| p.to_string_lossy().into()),
            backup: backup.map(|p| p.to_string_lossy().into()),
            status: JournalStatus::Prepared,
        };
        atomic_write_json(&path, &j, false)?;
        Ok(path)
    }
    fn finish_journal(&self, path: &Path) -> Result<(), RuntimeError> {
        fs::remove_file(path).or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else {
                Err(e)
            }
        })?;
        Ok(())
    }
    fn commit_state(
        &self,
        id: u64,
        path: PathBuf,
        installation: Option<Installation>,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state()?;
        if let Some(i) = installation {
            s.installations.retain(|x| x.id != i.id);
            s.installations.push(i);
            self.save_state(&s)?;
        }
        self.finish_journal(&path)?;
        if let Ok(mut o) = self.operations.lock() {
            if let Some(e) = o.entries.get_mut(&id) {
                e.state = OperationState::Completed;
            }
        }
        Ok(())
    }
    fn abort(&self, id: u64, journal: &Path) {
        let _ = self.finish_journal(journal);
        if let Ok(mut o) = self.operations.lock() {
            if let Some(e) = o.entries.get_mut(&id) {
                e.state = if e.cancel {
                    OperationState::Cancelled
                } else {
                    OperationState::Failed
                };
            }
        }
    }
    fn complete_operation(&self, id: u64) {
        if let Ok(mut operations) = self.operations.lock() {
            if let Some(entry) = operations.entries.get_mut(&id) {
                entry.state = OperationState::Completed;
            }
        }
    }
    fn recover(&self) -> Result<(), RuntimeError> {
        for entry in fs::read_dir(self.root.join(".runtime-journals"))? {
            let path = entry?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let j: Journal = serde_json::from_slice(&fs::read(&path)?)
                .map_err(|e| RuntimeError::Journal(e.to_string()))?;
            if j.version != 1 {
                return Err(RuntimeError::Journal("unsupported version".into()));
            }
            if j.kind == "legacy-profile-delete" {
                let destination =
                    j.destination.as_deref().map(PathBuf::from).ok_or_else(|| {
                        RuntimeError::Journal("delete destination missing".into())
                    })?;
                let trash =
                    j.replacement.as_deref().map(PathBuf::from).ok_or_else(|| {
                        RuntimeError::Journal("delete replacement missing".into())
                    })?;
                if !destination.starts_with(&self.root) || !trash.starts_with(&self.root) {
                    return Err(RuntimeError::Journal("delete path escaped root".into()));
                }
                let index = destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_prefix("deltamod_system-"))
                    .and_then(|value| value.parse::<u32>().ok())
                    .filter(|index| *index <= MAX_LEGACY_INSTALLATION_INDEX)
                    .ok_or_else(|| RuntimeError::Journal("invalid delete profile".into()))?;
                let indexed = self
                    .load_legacy_profiles()?
                    .installations
                    .iter()
                    .any(|record| record.index == Some(index));
                if indexed && !destination.exists() && trash.exists() {
                    fs::rename(&trash, &destination)?;
                } else if trash.exists() {
                    fs::remove_dir_all(&trash)?;
                }
                self.finish_journal(&path)?;
                continue;
            }
            if let Some(staging) = j.staging {
                let p = PathBuf::from(staging);
                if p.starts_with(&self.root) {
                    let _ = fs::remove_dir_all(p);
                }
            }
            if let Some(replacement) = j.replacement {
                let replacement = PathBuf::from(replacement);
                if replacement.starts_with(&self.root) {
                    let _ = fs::remove_dir_all(replacement);
                }
            }
            if let (Some(destination), Some(backup)) = (j.destination, j.backup) {
                let destination = PathBuf::from(destination);
                let backup = PathBuf::from(backup);
                if destination.starts_with(&self.root) && backup.starts_with(&self.root) {
                    if !destination.exists() && backup.exists() {
                        fs::rename(&backup, &destination)?;
                    } else if backup.exists() {
                        fs::remove_dir_all(backup)?;
                    }
                }
            }
            self.finish_journal(&path)?;
        }
        Ok(())
    }
}

impl From<PersistedState> for InstallationListResponse {
    fn from(s: PersistedState) -> Self {
        Self {
            installations: s.installations,
            selected_id: s.selected_id,
        }
    }
}
fn valid_name(name: String) -> Result<String, RuntimeError> {
    let n = name.trim().to_owned();
    if n.is_empty() || n.len() > 100 {
        Err(RuntimeError::Domain("invalid installation name".into()))
    } else {
        Ok(n)
    }
}

fn valid_legacy_name(name: String) -> Result<String, RuntimeError> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
        Err(RuntimeError::Domain("invalid installation name".into()))
    } else {
        Ok(name)
    }
}

fn safe_directory(path: &Path) -> Result<PathBuf, RuntimeError> {
    if !path.is_absolute() {
        return Err(RuntimeError::Domain("source must be absolute".into()));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(RuntimeError::Domain(
            "source is not a safe directory".into(),
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(RuntimeError::Domain(
                "source directory is a reparse point".into(),
            ));
        }
    }
    Ok(fs::canonicalize(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Barrier, thread, time::Duration};
    use tempfile::tempdir;

    #[derive(Debug)]
    struct FailingCopy;
    impl CopyBackend for FailingCopy {
        fn copy(
            &self,
            _: &Path,
            _: &Path,
            _: &Path,
            _: &dyn Fn() -> bool,
            _: &mut dyn FnMut(u64, &str) -> Result<(), StagedCopyError>,
            _: &dyn Fn() -> Result<(), StagedCopyError>,
        ) -> Result<(), StagedCopyError> {
            Err(StagedCopyError::CopyFailed(std::io::Error::other(
                "injected",
            )))
        }
    }

    #[derive(Debug)]
    struct CancellableCopy(Arc<Barrier>);
    impl CopyBackend for CancellableCopy {
        fn copy(
            &self,
            _: &Path,
            _: &Path,
            _: &Path,
            cancelled: &dyn Fn() -> bool,
            _: &mut dyn FnMut(u64, &str) -> Result<(), StagedCopyError>,
            _: &dyn Fn() -> Result<(), StagedCopyError>,
        ) -> Result<(), StagedCopyError> {
            self.0.wait();
            for _ in 0..100 {
                if cancelled() {
                    return Err(StagedCopyError::Cancelled);
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(StagedCopyError::CopyFailed(std::io::Error::other(
                "cancellation did not arrive",
            )))
        }
    }
    #[test]
    fn linked_delete_never_deletes_external_source() {
        let d = tempdir().unwrap();
        let source = d.path().join("source");
        fs::create_dir(&source).unwrap();
        let r = Runtime::open(d.path()).unwrap();
        let i = r
            .create_installation(
                &source,
                "Game".into(),
                GamePlatform::Windows,
                Ownership::LinkedExternal,
            )
            .unwrap();
        assert!(i.accepted);
        let list = r.list_installations().unwrap();
        r.delete_installation(&list.installations[0].id, true)
            .unwrap();
        assert!(source.exists());
    }
    #[test]
    fn failed_copy_does_not_publish_record() {
        let d = tempdir().unwrap();
        let source = d.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("data"), b"x").unwrap();
        let r = Runtime::open(d.path()).unwrap();
        assert!(r
            .create_installation(
                &source,
                "Game".into(),
                GamePlatform::Windows,
                Ownership::ManagedCopy
            )
            .is_ok());
        assert_eq!(r.list_installations().unwrap().installations.len(), 1);
    }
    #[test]
    fn injected_copy_failure_does_not_publish_state() {
        let d = tempdir().unwrap();
        let source = d.path().join("source");
        fs::create_dir(&source).unwrap();
        let r = Runtime::with_backend(d.path(), Arc::new(FailingCopy)).unwrap();
        assert!(r
            .create_installation(
                &source,
                "Game".into(),
                GamePlatform::Windows,
                Ownership::ManagedCopy
            )
            .is_err());
        assert!(r.list_installations().unwrap().installations.is_empty());
        assert_eq!(
            fs::read_dir(d.path().join(".runtime-journals"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn legacy_missing_profile_is_array() {
        let r = Runtime::open(tempdir().unwrap().path()).unwrap();
        assert_eq!(r.legacy_installations().unwrap(), json!([]));
    }

    #[test]
    fn managed_installations_can_be_copied_selected_and_replaced() {
        let d = tempdir().unwrap();
        let source = d.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("data.win"), b"first").unwrap();
        let r = Runtime::open(d.path().join("runtime")).unwrap();
        r.create_installation(
            &source,
            "Game".into(),
            GamePlatform::Windows,
            Ownership::ManagedCopy,
        )
        .unwrap();
        let first = r.list_installations().unwrap().installations[0].clone();
        r.copy_installation(&first.id, "Copy".into()).unwrap();
        let list = r.list_installations().unwrap();
        assert_eq!(list.installations.len(), 2);
        assert_eq!(
            fs::read(list.installations[1].install_path.join("data.win")).unwrap(),
            b"first"
        );

        fs::write(source.join("data.win"), b"second").unwrap();
        r.reimport_installation(&first.id).unwrap();
        assert_eq!(
            fs::read(first.install_path.join("data.win")).unwrap(),
            b"second"
        );
        r.select(Some(&first.id)).unwrap();
        r.set_edition(&first.id, Edition::Expanded).unwrap();
        let selected = r.list_installations().unwrap();
        assert_eq!(selected.selected_id, Some(first.id));
        assert_eq!(
            selected
                .installations
                .iter()
                .find(|installation| installation.id == selected.selected_id.clone().unwrap())
                .unwrap()
                .edition,
            Edition::Expanded
        );
    }

    #[test]
    fn managed_delete_rejects_tampered_outside_path() {
        let d = tempdir().unwrap();
        let source = d.path().join("source");
        let outside = d.path().join("outside");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&outside).unwrap();
        let r = Runtime::open(d.path().join("runtime")).unwrap();
        r.create_installation(
            &source,
            "Game".into(),
            GamePlatform::Windows,
            Ownership::LinkedExternal,
        )
        .unwrap();
        let mut state = r.state().unwrap();
        state.installations[0].ownership = Ownership::ManagedCopy;
        state.installations[0].install_path = outside.clone();
        r.save_state(&state).unwrap();
        assert!(r
            .delete_installation(&state.installations[0].id, true)
            .is_err());
        assert!(outside.exists());
    }

    #[test]
    fn patch_preparation_uses_native_conflict_validation() {
        let d = tempdir().unwrap();
        let runtime = Runtime::open(d.path().join("runtime")).unwrap();
        let game = d.path().join("game");
        let mod_root = d.path().join("mod");
        fs::create_dir(&game).unwrap();
        fs::create_dir(&mod_root).unwrap();
        fs::write(mod_root.join("one"), b"1").unwrap();
        fs::write(mod_root.join("two"), b"2").unwrap();
        let candidate = |patch: &str| PatchCandidateInput {
            patch_type: PatchTypeInput::Override,
            patch: patch.into(),
            to: "data.win".into(),
            mapped_target: "data.win".into(),
            mod_name: patch.into(),
            mod_id: patch.into(),
            mod_root: mod_root.clone(),
        };
        let error = runtime
            .prepare_patch_plan(PatchPlanInput {
                game_root: game,
                platform: PatchPlatformInput::Win32,
                patches: vec![candidate("one"), candidate("two")],
            })
            .unwrap_err();
        assert!(matches!(error, RuntimeError::PatchPlan(_)));
    }

    fn legacy_store_fields() -> serde_json::Map<String, Value> {
        serde_json::from_value(json!({
            "gamePid": "test.game",
            "gamePlatform": "win32",
            "isSteam": false
        }))
        .unwrap()
    }

    #[test]
    fn legacy_linked_profile_delete_preserves_external_game() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("external-game");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("data.win"), b"external").unwrap();
        let runtime = Runtime::open(directory.path().join("runtime")).unwrap();
        runtime
            .legacy_create_installation(4, &source, "External".into(), false, legacy_store_fields())
            .unwrap();

        runtime.legacy_delete_installation(4).unwrap();

        assert_eq!(fs::read(source.join("data.win")).unwrap(), b"external");
        assert!(!runtime.root.join("deltamod_system-4").exists());
    }

    #[test]
    fn legacy_reimport_failure_keeps_live_managed_copy() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("data.win"), b"old").unwrap();
        let root = directory.path().join("runtime");
        let runtime = Runtime::open(&root).unwrap();
        runtime
            .legacy_create_installation(0, &source, "Managed".into(), true, legacy_store_fields())
            .unwrap();
        let live = root.join("deltamod_system-0").join("deltaruneInstall");
        let failing = Runtime::with_backend(&root, Arc::new(FailingCopy)).unwrap();

        assert!(failing
            .legacy_reimport_installation(0, &source, "win32".into())
            .is_err());
        assert_eq!(fs::read(live.join("data.win")).unwrap(), b"old");
    }

    #[test]
    fn legacy_indexes_and_names_are_bounded() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        let runtime = Runtime::open(directory.path().join("runtime")).unwrap();
        assert!(runtime
            .legacy_create_installation(
                MAX_LEGACY_INSTALLATION_INDEX + 1,
                &source,
                "Name".into(),
                false,
                legacy_store_fields(),
            )
            .is_err());
        assert!(runtime
            .legacy_create_installation(0, &source, "x".repeat(81), false, legacy_store_fields(),)
            .is_err());
    }

    #[test]
    fn legacy_managed_import_can_be_cancelled_before_publish() {
        let directory = tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let runtime = Runtime::with_backend(
            directory.path().join("runtime"),
            Arc::new(CancellableCopy(Arc::clone(&barrier))),
        )
        .unwrap();
        let worker = runtime.clone();
        let source_for_worker = source.clone();
        let handle = thread::spawn(move || {
            worker.legacy_create_installation(
                0,
                &source_for_worker,
                "Managed".into(),
                true,
                legacy_store_fields(),
            )
        });
        barrier.wait();
        runtime.cancel(1).unwrap();

        assert!(matches!(
            handle.join().unwrap(),
            Err(RuntimeError::Cancelled(1))
        ));
        assert!(!runtime.root.join("deltamod_system-0").exists());
        assert!(runtime
            .load_legacy_profiles()
            .unwrap()
            .installations
            .is_empty());
    }

    #[test]
    fn startup_recovers_interrupted_legacy_delete() {
        let directory = tempdir().unwrap();
        let root = directory.path().join("runtime");
        let source = directory.path().join("source");
        fs::create_dir(&source).unwrap();
        let runtime = Runtime::open(&root).unwrap();
        runtime
            .legacy_create_installation(2, &source, "Linked".into(), false, legacy_store_fields())
            .unwrap();
        let profile = runtime.existing_legacy_profile(2).unwrap();
        let id = runtime.start("legacy-installation-delete").unwrap();
        let trash = runtime
            .root
            .join(".runtime-replacements")
            .join(format!("legacy-delete-{id}"));
        fs::create_dir_all(trash.parent().unwrap()).unwrap();
        runtime
            .prepare(
                "legacy-profile-delete",
                None,
                Some(&profile),
                None,
                Some(&trash),
                None,
            )
            .unwrap();
        fs::rename(&profile, &trash).unwrap();
        drop(runtime);

        let recovered = Runtime::open(&root).unwrap();
        assert!(recovered.existing_legacy_profile(2).is_ok());
        assert!(!trash.exists());
        assert!(source.exists());
    }
}
