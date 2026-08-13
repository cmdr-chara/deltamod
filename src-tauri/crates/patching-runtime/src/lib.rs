#![forbid(unsafe_code)]

use deltamod_hash_worker::Event as NativeHashEvent;
use deltamod_native_core::{
    patch_plan::{validate_patch_plan, PatchCandidate, PatchPlanRequest, PatchPlatform, PatchType},
    patch_transaction::{backup, restore, Journal},
};
use deltamod_tools_runtime::{
    g3m_apply, g3m_merge, run_bounded, sha256_file, undertale_mod_cli, verify_tool, ToolKind,
    ToolPath, DEFAULT_TIMEOUT, MAX_OUTPUT_BYTES,
};
use deltamod_updater_launch_runtime::GameRuntime;
use roxmltree::Document;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;
use thiserror::Error;

pub const JOURNAL_NAME: &str = ".deltamod-community-patch-journal.json";
const MAX_SELECTED_MODS: usize = 1_000;
const MAX_ID_BYTES: usize = 256;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
static TRANSACTION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Error)]
pub enum Error {
    #[error("The selected mod list is invalid.")]
    InvalidSelection,
    #[error("The current game directory is unavailable.")]
    GameUnavailable,
    #[error("The community mod store is unavailable.")]
    ModStoreUnavailable,
    #[error("Mod \"{0}\" is missing its patch manifest.")]
    MissingManifest(String),
    #[error("Mod \"{0}\" has invalid metadata or patch XML.")]
    InvalidManifest(String),
    #[error("Mod \"{mod_name}\" uses unsupported patch type \"{patch_type}\".")]
    UnsupportedPatch {
        mod_name: String,
        patch_type: String,
    },
    #[error("Patch target contains an unsafe platform path.")]
    InvalidTarget,
    #[error("Patch plan validation failed: {0}")]
    Plan(String),
    #[error("Packaged patch tool is unavailable: {0}")]
    Tool(String),
    #[error("Patching was cancelled.")]
    Cancelled,
    #[error("Patch transaction failed: {0}")]
    Transaction(String),
    #[error("Game hashing failed: {0}")]
    Hash(String),
    #[error("Selected mod \"{mod_name}\" is incompatible: {reason}")]
    IncompatibleMod { mod_name: String, reason: String },
    #[error("The patched game could not be started: {0}")]
    Launch(String),
    #[error("Patching failed: {0}")]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformDefinition {
    pub data_files: Vec<String>,
    pub patch_layout: String,
    pub content_root: Option<String>,
}

impl PlatformDefinition {
    pub fn map_patch_target(&self, value: &str) -> Result<String, Error> {
        let normalized = value.replace('\\', "/");
        checked_relative(&normalized).map_err(|_| Error::InvalidTarget)?;
        if normalized.eq_ignore_ascii_case("data.win") {
            if let Some(data) = self.data_files.first() {
                return Ok(data.clone());
            }
        }
        let mapped = match self.patch_layout.as_str() {
            "gamemaker-linux-assets" | "gamemaker-mac-resources" => join_relative(
                self.content_root.as_deref().ok_or(Error::InvalidTarget)?,
                &normalized,
            )?,
            "deltarune-mac-resources" => {
                let mut chapter = normalized.clone();
                for number in 1..=5 {
                    let from = format!("chapter{number}_windows/");
                    if chapter.to_ascii_lowercase().starts_with(&from) {
                        chapter.replace_range(..from.len(), &format!("chapter{number}_mac/"));
                        break;
                    }
                }
                let mut parts = chapter.split('/').map(str::to_owned).collect::<Vec<_>>();
                if parts
                    .last()
                    .is_some_and(|part| part.eq_ignore_ascii_case("data.win"))
                {
                    *parts.last_mut().expect("checked above") = "game.ios".into();
                }
                join_relative(
                    self.content_root.as_deref().ok_or(Error::InvalidTarget)?,
                    &parts.join("/"),
                )?
            }
            _ => normalized,
        };
        checked_relative(&mapped).map_err(|_| Error::InvalidTarget)?;
        Ok(mapped)
    }
}

#[derive(Clone, Debug)]
struct Patch {
    candidate: PatchCandidate,
    source: PathBuf,
    source_sha256: String,
    mod_tree_sha256: Option<String>,
    target: PathBuf,
}

#[derive(Debug)]
pub struct PatchPlan {
    game_root: PathBuf,
    patches: Vec<Patch>,
    operation_count: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub operation_id: String,
    pub phase: String,
    pub completed: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HashResult {
    pub done: bool,
    pub file_count: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RequiredFile {
    pub file: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Compatibility {
    pub is_incompatible: bool,
    pub incompatibility_reason: String,
    pub hash_different_files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PatchResult {
    pub patched: bool,
    pub log: String,
    pub full_log: String,
}

pub struct Runtime {
    pub game_root: PathBuf,
    pub mod_root: PathBuf,
    pub tools_root: PathBuf,
    pub hash_cache_path: PathBuf,
    pub platform: PatchPlatform,
    pub platform_name: String,
    pub arch: String,
    pub definition: PlatformDefinition,
}

impl Runtime {
    pub fn check_required_files(
        &self,
        mods: &[(String, Vec<RequiredFile>)],
    ) -> Result<BTreeMap<String, Compatibility>, Error> {
        require_directory(&self.game_root, Error::GameUnavailable)?;
        let mut cache = load_hash_cache(&self.hash_cache_path);
        let mut dirty = false;
        let mut results = BTreeMap::new();
        for (id, required) in mods {
            let mut different = Vec::new();
            let mut invalid_reason = None;
            for item in required {
                let Some(relative) = item.file.as_deref() else {
                    invalid_reason = Some("Invalid neededFiles entry.".to_owned());
                    break;
                };
                let Some(expected) = item.checksum.as_deref() else {
                    invalid_reason = Some("Invalid neededFiles entry.".to_owned());
                    break;
                };
                if !valid_sha256(expected) {
                    invalid_reason = Some("Invalid neededFiles checksum.".to_owned());
                    break;
                }
                let relative_path = match checked_relative(&relative.replace('\\', "/")) {
                    Ok(path) => path,
                    Err(_) => {
                        invalid_reason = Some(format!("Unsafe required game file: {relative}"));
                        break;
                    }
                };
                let key = normalized_hash_key(&relative_path, self.platform);
                let hashed = match deltamod_hash_worker::relative_file_signature(
                    &self.game_root,
                    &relative_path,
                ) {
                    Ok(signature) => {
                        if let Some(entry) = cache.entries.get(&key).filter(|entry| {
                            entry.signature == signature && valid_sha256(&entry.sha256)
                        }) {
                            entry.sha256.clone()
                        } else {
                            match deltamod_hash_worker::hash_relative_file(
                                &self.game_root,
                                &relative_path,
                            ) {
                                Ok((hashed_signature, sha256)) => {
                                    cache.entries.insert(
                                        key,
                                        HashEntry {
                                            signature: hashed_signature,
                                            sha256: sha256.clone(),
                                        },
                                    );
                                    dirty = true;
                                    sha256
                                }
                                Err(_) => {
                                    invalid_reason = Some(format!(
                                        "Required game file is missing or unsafe: {relative}"
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        invalid_reason = Some(format!(
                            "Required game file is missing or unsafe: {relative}"
                        ));
                        break;
                    }
                };
                if !hashed.eq_ignore_ascii_case(expected) {
                    different.push(relative.to_owned());
                }
            }
            let reason = invalid_reason.unwrap_or_else(|| {
                if different.is_empty() {
                    String::new()
                } else {
                    format!(
                        "Mismatching hashes for files: {}",
                        different
                            .iter()
                            .map(|file| format!("\"{file}\""))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            });
            results.insert(
                id.clone(),
                Compatibility {
                    is_incompatible: !reason.is_empty(),
                    incompatibility_reason: reason,
                    hash_different_files: different,
                },
            );
        }
        if dirty {
            atomic_json(&self.hash_cache_path, &cache)
                .map_err(|error| Error::Hash(error.to_string()))?;
        }
        Ok(results)
    }

    /// Returns legacy `__deltaID.json` objects and clears their `new` marker only
    /// after a successful commit, matching the Electron `finishedPatch` payload.
    pub fn mark_selected_patched(&self, selected: &[String]) -> Result<Vec<Value>, Error> {
        validate_selection(selected)?;
        let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut result = Vec::new();
        for entry in fs::read_dir(&self.mod_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path().join("__deltaID.json");
            let mut value = read_bounded_json(&path)?;
            let Some(id) = value.get("uniqueId").and_then(Value::as_str) else {
                continue;
            };
            if selected.contains(id) {
                value["new"] = Value::Bool(false);
                atomic_json(&path, &value)?;
            }
            result.push(value);
        }
        Ok(result)
    }

    pub fn build_plan(&self, selected: &[String]) -> Result<PatchPlan, Error> {
        validate_selection(selected)?;
        require_directory(&self.game_root, Error::GameUnavailable)?;
        require_directory(&self.mod_root, Error::ModStoreUnavailable)?;
        let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut patches = Vec::new();
        for entry in fs::read_dir(&self.mod_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let root = entry.path();
            let identity = read_bounded_json(&root.join("__deltaID.json"))?;
            let Some(id) = identity.get("uniqueId").and_then(Value::as_str) else {
                continue;
            };
            if !selected.contains(id) {
                continue;
            }
            let name =
                read_mod_name(&root).unwrap_or_else(|| entry.file_name().to_string_lossy().into());
            let manifest = variant_manifest(&root)?;
            if !manifest.is_file() {
                return Err(Error::MissingManifest(name));
            }
            let xml = fs::read_to_string(&manifest)?;
            let wrapped;
            let document = match Document::parse(&xml) {
                Ok(document) => document,
                Err(_) => {
                    wrapped = format!("<deltamod>{xml}</deltamod>");
                    Document::parse(&wrapped).map_err(|_| Error::InvalidManifest(name.clone()))?
                }
            };
            for node in document
                .descendants()
                .filter(|node| node.has_tag_name("patch"))
            {
                let kind = node.attribute("type").unwrap_or("").to_ascii_lowercase();
                let patch_name = node.attribute("patch").unwrap_or("").to_owned();
                let to = node.attribute("to").unwrap_or("").to_owned();
                if patch_name.is_empty() || to.is_empty() {
                    return Err(Error::InvalidManifest(name.clone()));
                }
                let patch_type =
                    parse_patch_type(&kind).ok_or_else(|| Error::UnsupportedPatch {
                        mod_name: name.clone(),
                        patch_type: kind,
                    })?;
                let patch_relative = checked_relative(&patch_name)
                    .map_err(|_| Error::InvalidManifest(name.clone()))?;
                let mapped_target = self.definition.map_patch_target(&to)?;
                let target_relative =
                    checked_relative(&mapped_target).map_err(|_| Error::InvalidTarget)?;
                let source = root.join(&patch_relative);
                patches.push(Patch {
                    source_sha256: sha256_file(&source)
                        .map_err(|error| Error::Plan(error.to_string()))?,
                    mod_tree_sha256: (patch_type == PatchType::Csx)
                        .then(|| tree_sha256(&root))
                        .transpose()?,
                    source,
                    target: self.game_root.join(&target_relative),
                    candidate: PatchCandidate {
                        patch_type,
                        patch: patch_name,
                        to,
                        mapped_target,
                        mod_name: name.clone(),
                        mod_id: id.to_owned(),
                        mod_root: root.clone(),
                    },
                });
            }
        }
        let request = PatchPlanRequest {
            game_root: self.game_root.clone(),
            platform: self.platform,
            patches: patches
                .iter()
                .map(|patch| patch.candidate.clone())
                .collect(),
        };
        let approval =
            validate_patch_plan(&request).map_err(|error| Error::Plan(error.to_string()))?;
        Ok(PatchPlan {
            game_root: self.game_root.clone(),
            patches,
            operation_count: approval.operation_count,
        })
    }

    pub fn check_selected_legacy_mods(&self, selected: &[String]) -> Result<(), Error> {
        validate_selection(selected)?;
        let selected = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut requirements = Vec::new();
        let mut names = BTreeMap::new();
        for entry in fs::read_dir(&self.mod_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let root = entry.path();
            let identity = read_bounded_json(&root.join("__deltaID.json"))?;
            let Some(id) = identity.get("uniqueId").and_then(Value::as_str) else {
                continue;
            };
            if !selected.contains(id) {
                continue;
            }
            let name = read_mod_name(&root).unwrap_or_else(|| id.to_owned());
            let required =
                read_legacy_required_files(&root).map_err(|reason| Error::IncompatibleMod {
                    mod_name: name.clone(),
                    reason,
                })?;
            names.insert(id.to_owned(), name);
            requirements.push((id.to_owned(), required));
        }
        for (id, compatibility) in self.check_required_files(&requirements)? {
            if compatibility.is_incompatible {
                return Err(Error::IncompatibleMod {
                    mod_name: names.remove(&id).unwrap_or(id),
                    reason: compatibility.incompatibility_reason,
                });
            }
        }
        Ok(())
    }

    pub fn restore(&self) -> Result<(), Error> {
        restore_existing(&self.game_root)
    }

    pub fn precalc_game_hashes(
        &self,
        operation_id: &str,
        mut emit: impl FnMut(Progress),
        cancelled: impl Fn() -> bool,
    ) -> Result<HashResult, Error> {
        require_directory(&self.game_root, Error::GameUnavailable)?;
        validate_operation_id(operation_id)?;
        let mut entries = BTreeMap::<String, HashEntry>::new();
        let mut file_count = 0;
        deltamod_hash_worker::run(&self.game_root, |event| {
            if cancelled() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match event {
                NativeHashEvent::File {
                    relative,
                    signature,
                    sha256,
                    completed,
                    total,
                } => {
                    let key = if self.platform == PatchPlatform::Win32 {
                        relative.to_lowercase()
                    } else {
                        relative.clone()
                    };
                    entries.insert(
                        key,
                        HashEntry {
                            signature: signature.clone(),
                            sha256: sha256.clone(),
                        },
                    );
                    emit(Progress {
                        operation_id: operation_id.into(),
                        phase: "hashing".into(),
                        completed: *completed,
                        total: *total,
                        current_item: Some(relative.clone()),
                        log: None,
                        percent: Some(percent(*completed, *total)),
                    });
                }
                NativeHashEvent::Done { file_count: count } => file_count = *count,
            }
            Ok(())
        })
        .map_err(|error| {
            if error.kind() == io::ErrorKind::Interrupted {
                Error::Cancelled
            } else {
                Error::Hash(error.to_string())
            }
        })?;
        atomic_json(
            &self.hash_cache_path,
            &HashCache {
                schema_version: 1,
                entries,
            },
        )
        .map_err(|error| Error::Hash(error.to_string()))?;
        Ok(HashResult {
            done: true,
            file_count,
        })
    }

    pub fn patch(
        &self,
        selected: &[String],
        operation_id: &str,
        mut emit: impl FnMut(Progress),
        cancelled: impl Fn() -> bool,
    ) -> Result<PatchResult, Error> {
        validate_operation_id(operation_id)?;
        self.restore()?;
        let plan = self.build_plan(selected)?;
        if plan.operation_count == 0 {
            emit(progress_event(operation_id, "patching", 0, 0, None, None));
            return Ok(PatchResult {
                patched: true,
                log: String::new(),
                full_log: String::new(),
            });
        }
        let g3m = plan
            .patches
            .iter()
            .any(|patch| {
                matches!(
                    patch.candidate.patch_type,
                    PatchType::Xdelta | PatchType::G3mPatch
                )
            })
            .then(|| self.tool(ToolKind::G3mTool))
            .transpose()?;
        let utmt = plan
            .patches
            .iter()
            .any(|patch| patch.candidate.patch_type == PatchType::Csx)
            .then(|| self.tool(ToolKind::UndertaleModCli))
            .transpose()?;
        let staging = tempfile::Builder::new().prefix("deltamod-csx-").tempdir()?;
        let scripts = self.stage_scripts(
            &plan,
            staging.path(),
            utmt.as_ref(),
            operation_id,
            &mut emit,
            &cancelled,
        )?;
        validate_patch_plan(&PatchPlanRequest {
            game_root: plan.game_root.clone(),
            platform: self.platform,
            patches: plan
                .patches
                .iter()
                .map(|patch| patch.candidate.clone())
                .collect(),
        })
        .map_err(|error| Error::Plan(error.to_string()))?;
        let journal_path = plan.game_root.join(JOURNAL_NAME);
        let mut journal = new_journal();
        atomic_json(&journal_path, &journal)?;
        let result = self.commit_plan(
            &plan,
            &scripts,
            g3m.as_ref(),
            &journal_path,
            &mut journal,
            operation_id,
            &mut emit,
            &cancelled,
        );
        match result {
            Ok(full_log) => {
                journal.state = "patched".into();
                journal.completed_at = Some(now_millis().to_string());
                atomic_json(&journal_path, &journal)?;
                Ok(PatchResult {
                    patched: true,
                    log: String::new(),
                    full_log,
                })
            }
            Err(error) => {
                let restore_error = restore(&plan.game_root, &journal_path, &mut journal).err();
                if let Some(restore_error) = restore_error {
                    return Err(Error::Transaction(format!(
                        "{error}; rollback failed: {restore_error}"
                    )));
                }
                Err(error)
            }
        }
    }

    pub fn patch_and_run(
        &self,
        selected: &[String],
        operation_id: &str,
        game: &GameRuntime,
        emit: impl FnMut(Progress),
        cancelled: impl Fn() -> bool,
    ) -> Result<PatchResult, Error> {
        let result = self.patch(selected, operation_id, emit, cancelled)?;
        game.dispatch("startGame", &[]).map_err(|error| {
            let _ = self.restore();
            Error::Launch(error.to_string())
        })?;
        // GameRuntime owns and reaps the child; keeping this operation alive ensures
        // originals are restored after exit even before shell lifecycle wiring lands.
        while game.is_running() {
            thread::sleep(Duration::from_millis(50));
        }
        self.restore()?;
        Ok(result)
    }

    fn tool(&self, kind: ToolKind) -> Result<ToolPath, Error> {
        let path = kind
            .packaged_path(&self.tools_root, &self.platform_name, &self.arch)
            .map_err(|error| Error::Tool(error.to_string()))?;
        verify_tool(&path, kind, None).map_err(|error| Error::Tool(error.to_string()))
    }

    fn stage_scripts(
        &self,
        plan: &PatchPlan,
        staging: &Path,
        tool: Option<&ToolPath>,
        operation_id: &str,
        emit: &mut impl FnMut(Progress),
        cancelled: &impl Fn() -> bool,
    ) -> Result<HashMap<String, PathBuf>, Error> {
        let groups = grouped(plan, PatchType::Csx);
        let total = groups.len();
        let mut outputs = HashMap::new();
        for (index, (target, patches)) in groups.into_iter().enumerate() {
            check_cancel(cancelled)?;
            let tool = tool.ok_or_else(|| Error::Tool("UndertaleModCli is missing".into()))?;
            let input = staging.join(format!("input-{index}"));
            let output = staging.join(format!("output-{index}"));
            fs::copy(&patches[0].target, &input)?;
            let mut snapshots = HashMap::<PathBuf, TempDir>::new();
            let mut staged = Vec::new();
            for patch in &patches {
                if !snapshots.contains_key(&patch.candidate.mod_root) {
                    let snapshot = tempfile::Builder::new()
                        .prefix("mod-")
                        .tempdir_in(staging)?;
                    copy_tree(&patch.candidate.mod_root, snapshot.path())?;
                    if tree_sha256(snapshot.path())?
                        != patch
                            .mod_tree_sha256
                            .as_deref()
                            .ok_or_else(|| Error::Plan("CSX snapshot was not approved".into()))?
                    {
                        return Err(Error::Plan(format!(
                            "Mod resources for CSX patch \"{}\" changed after approval",
                            patch.candidate.patch
                        )));
                    }
                    snapshots.insert(patch.candidate.mod_root.clone(), snapshot);
                }
                let relative =
                    checked_relative(&patch.candidate.patch).map_err(|_| Error::InvalidTarget)?;
                staged.push(snapshots[&patch.candidate.mod_root].path().join(relative));
            }
            emit(progress_event(
                operation_id,
                "scripts",
                index,
                total,
                Some(target.clone()),
                None,
            ));
            let tool_log = run_tool(&undertale_mod_cli(tool, &input, &output, &staged))?;
            if !tool_log.is_empty() {
                emit(progress_event(
                    operation_id,
                    "scripts",
                    index,
                    total,
                    Some(target.clone()),
                    Some(format!("[UTMT] {tool_log}")),
                ));
            }
            regular_file(&output)?;
            outputs.insert(target, output);
        }
        Ok(outputs)
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_plan(
        &self,
        plan: &PatchPlan,
        scripts: &HashMap<String, PathBuf>,
        g3m: Option<&ToolPath>,
        journal_path: &Path,
        journal: &mut Journal,
        operation_id: &str,
        emit: &mut impl FnMut(Progress),
        cancelled: &impl Fn() -> bool,
    ) -> Result<String, Error> {
        let mut completed = 0;
        let mut full_log = String::new();
        for patch in plan.patches.iter().filter(|patch| {
            matches!(
                patch.candidate.patch_type,
                PatchType::Override | PatchType::Copy
            )
        }) {
            check_cancel(cancelled)?;
            verify_approved_source(patch)?;
            backup_target(plan, patch, journal_path, journal)?;
            fs::copy(&patch.source, &patch.target)?;
            completed += 1;
            emit(progress_event(
                operation_id,
                "patching",
                completed,
                plan.operation_count,
                Some(patch.candidate.to.clone()),
                None,
            ));
        }
        for (target, patches) in grouped_merges(plan) {
            check_cancel(cancelled)?;
            let patch = &patches[0];
            for patch in &patches {
                verify_approved_source(patch)?;
            }
            backup_target(plan, patch, journal_path, journal)?;
            let backup_path = plan
                .game_root
                .join(".deltamod-community-patch-backups")
                .join(&journal.transaction_id)
                .join(checked_relative(&target).map_err(|_| Error::InvalidTarget)?);
            let backup_relative = Path::new(".deltamod-community-patch-backups")
                .join(&journal.transaction_id)
                .join(checked_relative(&target).map_err(|_| Error::InvalidTarget)?);
            let tool = g3m.ok_or_else(|| Error::Tool("G3MTool is missing".into()))?;
            let spec = if patches.len() == 1 {
                g3m_apply(
                    tool,
                    &plan.game_root,
                    &backup_relative,
                    &patch.source,
                    Path::new(&target),
                )
            } else {
                g3m_merge(
                    tool,
                    &plan.game_root,
                    &backup_path,
                    &patches.iter().map(|p| p.source.clone()).collect::<Vec<_>>(),
                    &patch.target,
                )
            };
            let output = run_tool(&spec)?;
            let event_output = (!output.is_empty()).then(|| format!("[G3MTOOL] {output}"));
            full_log.push_str(&output);
            completed += 1;
            emit(progress_event(
                operation_id,
                "patching",
                completed,
                plan.operation_count,
                Some(target),
                event_output,
            ));
        }
        for (target, patches) in grouped(plan, PatchType::Csx) {
            check_cancel(cancelled)?;
            let patch = &patches[0];
            backup_target(plan, patch, journal_path, journal)?;
            let output = scripts
                .get(&target)
                .ok_or_else(|| Error::Tool("UndertaleModCli output is missing".into()))?;
            regular_file(output)?;
            fs::copy(output, &patch.target)?;
            completed += 1;
            emit(progress_event(
                operation_id,
                "patching",
                completed,
                plan.operation_count,
                Some(target),
                None,
            ));
        }
        Ok(full_log)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HashCache {
    schema_version: u8,
    entries: BTreeMap<String, HashEntry>,
}
#[derive(Deserialize, Serialize)]
struct HashEntry {
    signature: String,
    sha256: String,
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalized_hash_key(relative: &Path, platform: PatchPlatform) -> String {
    let value = relative.to_string_lossy().replace('\\', "/");
    if platform == PatchPlatform::Win32 {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn load_hash_cache(path: &Path) -> HashCache {
    let empty = || HashCache {
        schema_version: 1,
        entries: BTreeMap::new(),
    };
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return empty();
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 * 1024
    {
        return empty();
    }
    serde_json::from_slice::<HashCache>(&fs::read(path).unwrap_or_default())
        .ok()
        .filter(|cache| cache.schema_version == 1)
        .unwrap_or_else(empty)
}

fn read_legacy_required_files(root: &Path) -> Result<Vec<RequiredFile>, String> {
    let path = root.join("meta.toml");
    let metadata = fs::symlink_metadata(&path).map_err(|_| "Missing meta.toml.".to_owned())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_METADATA_BYTES
    {
        return Err("Unsafe meta.toml.".into());
    }
    let value: toml::Value =
        toml::from_str(&fs::read_to_string(path).map_err(|_| "Unreadable meta.toml.".to_owned())?)
            .map_err(|_| "Malformed meta.toml.".to_owned())?;
    let Some(required) = value.get("neededFiles") else {
        return Ok(Vec::new());
    };
    let values = required
        .as_array()
        .ok_or_else(|| "Invalid neededFiles list.".to_owned())?;
    values
        .iter()
        .map(|item| {
            let table = item
                .as_table()
                .ok_or_else(|| "Invalid neededFiles entry.".to_owned())?;
            Ok(RequiredFile {
                file: table
                    .get("file")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                checksum: table
                    .get("checksum")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn validate_selection(selected: &[String]) -> Result<(), Error> {
    if selected.len() > MAX_SELECTED_MODS
        || selected
            .iter()
            .any(|id| id.is_empty() || id.len() > MAX_ID_BYTES || id.chars().any(char::is_control))
    {
        return Err(Error::InvalidSelection);
    }
    Ok(())
}
fn validate_operation_id(id: &str) -> Result<(), Error> {
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Err(Error::InvalidSelection)
    } else {
        Ok(())
    }
}
fn checked_relative(value: &str) -> Result<PathBuf, ()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\0')
        || path.is_absolute()
        || value.as_bytes().get(1) == Some(&b':')
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(())
    } else {
        Ok(path.to_owned())
    }
}
fn join_relative(a: &str, b: &str) -> Result<String, Error> {
    checked_relative(a).map_err(|_| Error::InvalidTarget)?;
    Ok(format!(
        "{}/{}",
        a.trim_end_matches('/'),
        b.trim_start_matches('/')
    ))
}
fn require_directory(path: &Path, error: Error) -> Result<(), Error> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Err(error);
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(())
    } else {
        Err(error)
    }
}
fn read_bounded_json(path: &Path) -> Result<Value, Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_METADATA_BYTES
    {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| Error::InvalidManifest(path.display().to_string()))
}
fn read_mod_name(root: &Path) -> Option<String> {
    let text = fs::read_to_string(root.join("meta.toml")).ok()?;
    toml::from_str::<toml::Value>(&text)
        .ok()?
        .get("metadata")?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}
fn variant_manifest(root: &Path) -> Result<PathBuf, Error> {
    let marker = root.join("__variant");
    let relative = if marker.is_file() {
        fs::read_to_string(marker)?.trim().to_owned()
    } else {
        "modding.xml".into()
    };
    Ok(root.join(checked_relative(&relative).map_err(|_| Error::InvalidTarget)?))
}
fn parse_patch_type(value: &str) -> Option<PatchType> {
    Some(match value {
        "override" => PatchType::Override,
        "copy" => PatchType::Copy,
        "xdelta" => PatchType::Xdelta,
        "g3mpatch" => PatchType::G3mPatch,
        "csx" => PatchType::Csx,
        _ => return None,
    })
}
fn grouped(plan: &PatchPlan, kind: PatchType) -> Vec<(String, Vec<&Patch>)> {
    let mut groups: Vec<(String, Vec<&Patch>)> = Vec::new();
    let mut indices = HashMap::new();
    for patch in plan
        .patches
        .iter()
        .filter(|p| p.candidate.patch_type == kind)
    {
        let key = patch.candidate.mapped_target.clone();
        let index = *indices.entry(key.clone()).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push(patch);
    }
    groups
}
fn grouped_merges(plan: &PatchPlan) -> Vec<(String, Vec<&Patch>)> {
    let mut groups: Vec<(String, Vec<&Patch>)> = Vec::new();
    let mut indices = HashMap::new();
    for patch in plan.patches.iter().filter(|p| {
        matches!(
            p.candidate.patch_type,
            PatchType::Xdelta | PatchType::G3mPatch
        )
    }) {
        let key = patch.candidate.mapped_target.clone();
        let index = *indices.entry(key.clone()).or_insert_with(|| {
            groups.push((key, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push(patch);
    }
    groups
}
fn backup_target(
    plan: &PatchPlan,
    patch: &Patch,
    journal_path: &Path,
    journal: &mut Journal,
) -> Result<(), Error> {
    if let Some(parent) = patch.target.parent() {
        fs::create_dir_all(parent)?;
    }
    backup(
        &plan.game_root,
        journal_path,
        journal,
        &patch.candidate.mapped_target,
    )
    .map_err(|e| Error::Transaction(e.to_string()))
}
fn run_tool(spec: &deltamod_tools_runtime::CommandSpec) -> Result<String, Error> {
    let output = run_bounded(spec, DEFAULT_TIMEOUT, MAX_OUTPUT_BYTES)
        .map_err(|e| Error::Tool(e.to_string()))?;
    let combined = format!("{}{}", output.stdout, output.stderr);
    if output.timed_out {
        return Err(Error::Tool("tool timed out".into()));
    }
    if !output.status.success() {
        return Err(Error::Tool(format!(
            "tool exited with {}: {combined}",
            output.status
        )));
    }
    Ok(combined)
}
fn regular_file(path: &Path) -> Result<(), Error> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Tool("output is not a regular file".into()));
    }
    Ok(())
}
fn verify_approved_source(patch: &Patch) -> Result<(), Error> {
    let current = sha256_file(&patch.source).map_err(|error| Error::Plan(error.to_string()))?;
    if current != patch.source_sha256 {
        return Err(Error::Plan(format!(
            "Patch source \"{}\" changed after approval",
            patch.candidate.patch
        )));
    }
    Ok(())
}
fn tree_sha256(root: &Path) -> Result<String, Error> {
    fn visit(root: &Path, current: &Path, entries: &mut Vec<String>) -> Result<(), Error> {
        let mut children = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(Error::Plan(
                    "Script resources contain a symbolic link".into(),
                ));
            }
            if metadata.is_dir() {
                visit(root, &path, entries)?;
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error::Plan("Script resource escaped its root".into()))?
                    .to_string_lossy()
                    .replace('\\', "/");
                let hash = sha256_file(&path).map_err(|error| Error::Plan(error.to_string()))?;
                entries.push(format!("{relative}\0{hash}\n"));
            } else {
                return Err(Error::Plan(
                    "Script resources contain an unsupported file".into(),
                ));
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    let digest = sha2_digest(entries.concat().as_bytes());
    Ok(digest)
}
fn sha2_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}
fn copy_tree(source: &Path, destination: &Path) -> Result<(), Error> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let m = fs::symlink_metadata(entry.path())?;
        let to = destination.join(entry.file_name());
        if m.file_type().is_symlink() {
            return Err(Error::Plan(
                "Script resources contain a symbolic link".into(),
            ));
        }
        if m.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if m.is_file() {
            fs::copy(entry.path(), to)?;
        } else {
            return Err(Error::Plan(
                "Script resources contain an unsupported file".into(),
            ));
        }
    }
    Ok(())
}
fn restore_existing(root: &Path) -> Result<(), Error> {
    let path = root.join(JOURNAL_NAME);
    if !path.exists() {
        return Ok(());
    }
    let mut journal: Journal =
        serde_json::from_slice(&fs::read(&path)?).map_err(|e| Error::Transaction(e.to_string()))?;
    restore(root, &path, &mut journal).map_err(|e| Error::Transaction(e.to_string()))
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = File::create(&temp)?;
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    file.sync_all()?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)
}
fn new_journal() -> Journal {
    Journal {
        schema_version: 1,
        transaction_id: format!(
            "{}-{}",
            now_millis(),
            std::process::id() as u64 + TRANSACTION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ),
        state: "patching".into(),
        started_at: Some(now_millis().to_string()),
        completed_at: None,
        operations: vec![],
    }
}
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn percent(completed: usize, total: usize) -> f64 {
    if total == 0 {
        100.0
    } else {
        (completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    }
}
fn progress_event(
    id: &str,
    phase: &str,
    completed: usize,
    total: usize,
    item: Option<String>,
    log: Option<String>,
) -> Progress {
    Progress {
        operation_id: id.into(),
        phase: phase.into(),
        completed,
        total,
        current_item: item,
        log,
        percent: Some(percent(completed, total)),
    }
}
fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_mapping_matches_node_contract() {
        let mac = PlatformDefinition {
            data_files: vec!["game.ios".into()],
            patch_layout: "deltarune-mac-resources".into(),
            content_root: Some("DELTARUNE.app/Contents/Resources".into()),
        };
        assert_eq!(
            mac.map_patch_target("chapter1_windows/data.win").unwrap(),
            "DELTARUNE.app/Contents/Resources/chapter1_mac/game.ios"
        );
        assert!(mac.map_patch_target("../outside").is_err());
    }

    #[test]
    fn hash_cache_and_progress_preserve_legacy_shape() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        fs::create_dir(&game).unwrap();
        fs::write(game.join("data.win"), b"data").unwrap();
        let runtime = Runtime {
            game_root: game,
            mod_root: root.path().join("mods"),
            tools_root: root.path().join("tools"),
            hash_cache_path: root.path().join("_game-hashes.json"),
            platform: PatchPlatform::Linux,
            platform_name: "linux".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        let mut events = Vec::new();
        assert_eq!(
            runtime
                .precalc_game_hashes("op-1", |e| events.push(e), || false)
                .unwrap()
                .file_count,
            1
        );
        let value: Value =
            serde_json::from_slice(&fs::read(&runtime.hash_cache_path).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert!(
            value["entries"]["data.win"]["sha256"]
                .as_str()
                .unwrap()
                .len()
                == 64
        );
        assert_eq!(events[0].phase, "hashing");
        assert_eq!(events[0].percent, Some(100.0));
    }

    #[test]
    fn required_files_reuse_valid_cache_and_rehash_only_stale_entries() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        fs::create_dir(&game).unwrap();
        fs::write(game.join("Data.WIN"), b"data").unwrap();
        let runtime = Runtime {
            game_root: game,
            mod_root: root.path().join("mods"),
            tools_root: root.path().join("tools"),
            hash_cache_path: root.path().join("hash.json"),
            platform: PatchPlatform::Win32,
            platform_name: "win32".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        let digest = sha2_digest(b"data");
        let required = vec![(
            "mod".into(),
            vec![RequiredFile {
                file: Some("Data.WIN".into()),
                checksum: Some(digest.clone()),
            }],
        )];
        assert!(!runtime.check_required_files(&required).unwrap()["mod"].is_incompatible);
        let mut cache: Value =
            serde_json::from_slice(&fs::read(&runtime.hash_cache_path).unwrap()).unwrap();
        assert!(cache["entries"].get("data.win").is_some());
        cache["entries"]["data.win"]["sha256"] = Value::String("0".repeat(64));
        atomic_json(&runtime.hash_cache_path, &cache).unwrap();
        let cached_required = vec![(
            "mod".into(),
            vec![RequiredFile {
                file: Some("Data.WIN".into()),
                checksum: Some("0".repeat(64)),
            }],
        )];
        assert!(!runtime.check_required_files(&cached_required).unwrap()["mod"].is_incompatible);
        cache["entries"]["data.win"]["signature"] = Value::String("stale".into());
        atomic_json(&runtime.hash_cache_path, &cache).unwrap();
        assert!(runtime.check_required_files(&cached_required).unwrap()["mod"].is_incompatible);
    }

    #[test]
    fn unsafe_required_file_only_marks_its_mod_incompatible() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        fs::create_dir(&game).unwrap();
        fs::write(game.join("data.win"), b"data").unwrap();
        let runtime = Runtime {
            game_root: game,
            mod_root: root.path().join("mods"),
            tools_root: root.path().join("tools"),
            hash_cache_path: root.path().join("hash.json"),
            platform: PatchPlatform::Linux,
            platform_name: "linux".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        let checks = vec![
            (
                "good".into(),
                vec![RequiredFile {
                    file: Some("data.win".into()),
                    checksum: Some(sha2_digest(b"data")),
                }],
            ),
            (
                "bad".into(),
                vec![RequiredFile {
                    file: Some("../outside".into()),
                    checksum: Some("0".repeat(64)),
                }],
            ),
        ];
        let result = runtime.check_required_files(&checks).unwrap();
        assert!(!result["good"].is_incompatible);
        assert!(result["bad"].is_incompatible);
        assert!(result["bad"].incompatibility_reason.contains("Unsafe"));
    }

    #[test]
    fn legacy_requirements_are_read_from_the_toml_top_level() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("meta.toml"),
            r#"
[[neededFiles]]
file = "data.win"
checksum = "0000000000000000000000000000000000000000000000000000000000000000"

[metadata]
name = "Test"
"#,
        )
        .unwrap();
        let required = read_legacy_required_files(root.path()).unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0].file.as_deref(), Some("data.win"));
    }

    #[test]
    fn patch_plan_accepts_legacy_xml_fragments() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let mods = root.path().join("mods");
        let packet = mods.join("one");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&packet).unwrap();
        fs::write(game.join("data.win"), b"original").unwrap();
        fs::write(game.join("other.win"), b"original").unwrap();
        fs::write(packet.join("one.bin"), b"one").unwrap();
        fs::write(packet.join("two.bin"), b"two").unwrap();
        fs::write(packet.join("__deltaID.json"), r#"{"uniqueId":"id"}"#).unwrap();
        fs::write(packet.join("meta.toml"), "[metadata]\nname='Test'\n").unwrap();
        fs::write(
            packet.join("modding.xml"),
            concat!(
                r#"<patch type="override" patch="one.bin" to="data.win" />"#,
                r#"<patch type="override" patch="two.bin" to="other.win" />"#
            ),
        )
        .unwrap();
        let runtime = Runtime {
            game_root: game,
            mod_root: mods,
            tools_root: root.path().join("tools"),
            hash_cache_path: root.path().join("hash.json"),
            platform: PatchPlatform::Linux,
            platform_name: "linux".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        assert_eq!(runtime.build_plan(&["id".into()]).unwrap().patches.len(), 2);
    }

    #[test]
    fn direct_patch_is_journaled_and_restorable() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let mods = root.path().join("mods");
        let packet = mods.join("one");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&packet).unwrap();
        fs::write(game.join("data.win"), b"original").unwrap();
        fs::write(packet.join("new.bin"), b"patched").unwrap();
        fs::write(packet.join("__deltaID.json"), r#"{"uniqueId":"id"}"#).unwrap();
        fs::write(packet.join("meta.toml"), "[metadata]\nname='Test'\n").unwrap();
        fs::write(
            packet.join("modding.xml"),
            r#"<root><patch type="override" patch="new.bin" to="data.win"/></root>"#,
        )
        .unwrap();
        let runtime = Runtime {
            game_root: game.clone(),
            mod_root: mods,
            tools_root: root.path().join("tools"),
            hash_cache_path: root.path().join("hash.json"),
            platform: PatchPlatform::Linux,
            platform_name: "linux".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        assert!(
            runtime
                .patch(&["id".into()], "patch-1", |_| {}, || false)
                .unwrap()
                .patched
        );
        assert_eq!(fs::read(game.join("data.win")).unwrap(), b"patched");
        assert!(game.join(JOURNAL_NAME).is_file());
        runtime.restore().unwrap();
        assert_eq!(fs::read(game.join("data.win")).unwrap(), b"original");
    }
}
