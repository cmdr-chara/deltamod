#![forbid(unsafe_code)]

use deltamod_mods_themes_domain as domain;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("path is outside managed root: {0}")]
    OutsideRoot(PathBuf),
    #[error("unsafe filesystem entry: {0}")]
    UnsafePath(PathBuf),
    #[error("not found: {0}")]
    NotFound(PathBuf),
    #[error("filesystem error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("invalid JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("domain validation failed: {0}")]
    Domain(String),
    #[error("archive operation is unavailable: {0}")]
    Archive(String),
}

impl From<domain::DomainError> for RuntimeError {
    fn from(e: domain::DomainError) -> Self {
        Self::Domain(e.0)
    }
}
fn io_at(path: &Path, source: io::Error) -> RuntimeError {
    RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventIntent {
    ModsChanged,
    ModStateChanged { uid: String },
    ThemesChanged,
    ActiveThemeChanged { id: String },
    PreferencesChanged,
    SharedChanged,
    SponsorsChanged,
}

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    pub data_root: PathBuf,
    pub mods_root: PathBuf,
    pub themes_root: PathBuf,
    pub default_theme: String,
}
impl RuntimeConfig {
    pub fn new(
        data_root: impl Into<PathBuf>,
        mods_root: impl Into<PathBuf>,
        themes_root: impl Into<PathBuf>,
        default_theme: impl Into<String>,
    ) -> Self {
        Self {
            data_root: data_root.into(),
            mods_root: mods_root.into(),
            themes_root: themes_root.into(),
            default_theme: default_theme.into(),
        }
    }
}

#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}
struct Inner {
    config: RuntimeConfig,
    lock: Mutex<()>,
    events: Mutex<Vec<EventIntent>>,
}
impl Runtime {
    pub fn open(config: RuntimeConfig) -> Result<Self> {
        for root in [&config.data_root, &config.mods_root, &config.themes_root] {
            fs::create_dir_all(root).map_err(|e| io_at(root, e))?;
            ensure_safe_dir(root)?;
        }
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                lock: Mutex::new(()),
                events: Mutex::new(Vec::new()),
            }),
        })
    }
    pub fn mods(&self) -> ModService {
        ModService {
            runtime: self.clone(),
        }
    }
    pub fn themes(&self) -> ThemeService {
        ThemeService {
            runtime: self.clone(),
        }
    }
    pub fn preferences(&self) -> PreferenceService {
        PreferenceService {
            runtime: self.clone(),
        }
    }
    pub fn shared(&self) -> SharedService {
        SharedService {
            runtime: self.clone(),
        }
    }
    pub fn sponsors(&self) -> SponsorService {
        SponsorService {
            runtime: self.clone(),
        }
    }
    pub fn drain_events(&self) -> Vec<EventIntent> {
        self.inner
            .events
            .lock()
            .map(|mut e| std::mem::take(&mut *e))
            .unwrap_or_default()
    }
    fn emit(&self, event: EventIntent) {
        if let Ok(mut events) = self.inner.events.lock() {
            events.push(event);
        }
    }
    fn transaction<T>(&self, f: impl FnOnce(&Runtime) -> Result<T>) -> Result<T> {
        let _guard = self
            .inner
            .lock
            .lock()
            .map_err(|_| RuntimeError::Invalid("mutation lock poisoned".into()))?;
        f(self)
    }
}

fn ensure_safe_dir(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|e| io_at(path, e))?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(RuntimeError::UnsafePath(path.to_path_buf()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(RuntimeError::UnsafePath(path.to_path_buf()));
        }
    }
    Ok(())
}
fn child(root: &Path, name: &str) -> Result<PathBuf> {
    let p = root.join(name);
    if !p.starts_with(root) || p == root {
        return Err(RuntimeError::OutsideRoot(p));
    }
    Ok(p)
}
fn safe_entry(root: &Path, name: &str) -> Result<PathBuf> {
    let p = child(root, name)?;
    let m = fs::symlink_metadata(&p).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            RuntimeError::NotFound(p.clone())
        } else {
            io_at(&p, e)
        }
    })?;
    if m.file_type().is_symlink() {
        return Err(RuntimeError::UnsafePath(p));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(RuntimeError::UnsafePath(p));
        }
    }
    Ok(p)
}
fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| RuntimeError::Invalid("JSON path has no parent".into()))?;
    ensure_safe_dir(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("state")
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| RuntimeError::Json {
        path: path.to_path_buf(),
        source: e,
    })?;
    {
        let mut f = fs::File::create(&tmp).map_err(|e| io_at(&tmp, e))?;
        use std::io::Write;
        f.write_all(&bytes).map_err(|e| io_at(&tmp, e))?;
        f.sync_all().map_err(|e| io_at(&tmp, e))?;
    }
    fs::rename(&tmp, path).map_err(|e| io_at(path, e))?;
    Ok(())
}
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).map_err(|e| io_at(path, e))?;
    serde_json::from_slice(&bytes).map_err(|e| RuntimeError::Json {
        path: path.to_path_buf(),
        source: e,
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModManifest {
    uid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    variants: Vec<ManifestVariant>,
}
#[derive(Clone, Debug, Deserialize)]
struct ManifestVariant {
    id: String,
    #[serde(default)]
    label: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(Deserialize)]
struct EnabledState {
    enabled: Vec<String>,
    #[serde(default)]
    selected_variants: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct ModService {
    runtime: Runtime,
}
impl ModService {
    fn state_path(&self) -> PathBuf {
        self.runtime.inner.config.data_root.join("mods-state.json")
    }
    pub fn list_json(&self) -> Result<Value> {
        let records = self.list()?;
        Ok(Value::Array(records.into_iter().map(|m| json!({
            "uid": m.uid.to_string(), "folder": m.folder.to_string(), "name": m.name,
            "variants": m.variants.into_iter().map(|v| json!({"id": v.id.to_string(), "label": v.label})).collect::<Vec<_>>()
        })).collect()))
    }
    pub fn list(&self) -> Result<Vec<domain::ModRecord>> {
        let root = &self.runtime.inner.config.mods_root;
        ensure_safe_dir(root)?;
        let mut out = Vec::new();
        for entry in fs::read_dir(root).map_err(|e| io_at(root, e))? {
            let entry = entry.map_err(|e| io_at(root, e))?;
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let m = fs::symlink_metadata(&p).map_err(|e| io_at(&p, e))?;
            if !m.is_dir() || m.file_type().is_symlink() {
                continue;
            }
            let manifest_path = p.join("manifest.json");
            if let Ok(manifest) = read_json::<ModManifest>(&manifest_path) {
                let uid = domain::ModUid::new(&manifest.uid)?;
                let folder = domain::ModFolderId::new(&name)?;
                let variants = manifest
                    .variants
                    .into_iter()
                    .map(|v| {
                        Ok(domain::ModVariant {
                            id: domain::VariantId::new(&v.id)?,
                            label: v.label,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                domain::validate_declared_variants(&variants, None)?;
                out.push(domain::ModRecord {
                    uid,
                    folder,
                    name: manifest.name,
                    variants,
                });
            }
        }
        out.sort_by(|a, b| a.uid.cmp(&b.uid));
        Ok(out)
    }
    pub fn count(&self) -> Result<usize> {
        Ok(self.list()?.len())
    }
    pub fn state(&self) -> Result<domain::ModListState> {
        let mods = self.list()?;
        let enabled = read_optional_state(&self.state_path())?
            .enabled
            .into_iter()
            .filter_map(|s| domain::ModUid::new(&s).ok())
            .collect();
        Ok(domain::ModListState { mods, enabled })
    }
    pub fn toggle(&self, uid: &str, enabled: bool) -> Result<domain::ModListState> {
        let uid = domain::ModUid::new(uid)?;
        self.runtime.transaction(|rt| {
            let mut state = read_optional_state(&rt.mods().state_path())?;
            if enabled {
                if !state.enabled.iter().any(|v| v == uid.as_str()) {
                    state.enabled.push(uid.to_string());
                }
            } else {
                state.enabled.retain(|v| v != uid.as_str());
            }
            state.enabled = domain::normalize_enabled(
                &state
                    .enabled
                    .iter()
                    .filter_map(|v| domain::ModUid::new(v).ok())
                    .collect::<Vec<_>>(),
            )
            .into_iter()
            .map(|v| v.to_string())
            .collect();
            atomic_json(&rt.mods().state_path(), &state)?;
            rt.emit(EventIntent::ModStateChanged {
                uid: uid.to_string(),
            });
            rt.mods().state()
        })
    }
    pub fn set_variant(&self, uid: &str, variant: &str) -> Result<()> {
        let uid = domain::ModUid::new(uid)?;
        let variant = domain::VariantId::new(variant)?;
        self.runtime.transaction(|rt| {
            let records = rt.mods().list()?;
            let m = records
                .iter()
                .find(|m| m.uid == uid)
                .ok_or_else(|| RuntimeError::NotFound(uid.to_string().into()))?;
            domain::validate_declared_variants(&m.variants, Some(&variant))?;
            let mut state = read_optional_state(&rt.mods().state_path())?;
            state
                .selected_variants
                .insert(uid.to_string(), variant.to_string());
            atomic_json(&rt.mods().state_path(), &state)?;
            rt.emit(EventIntent::ModStateChanged {
                uid: uid.to_string(),
            });
            Ok(())
        })
    }
    pub fn remove(&self, uid: &str) -> Result<()> {
        let uid = domain::ModUid::new(uid)?;
        self.runtime.transaction(|rt| {
            let m = rt
                .mods()
                .list()?
                .into_iter()
                .find(|m| m.uid == uid)
                .ok_or_else(|| RuntimeError::NotFound(uid.to_string().into()))?;
            let path = domain::plan_mod_deletion(&rt.inner.config.mods_root, &m.folder)?.path();
            safe_entry(&rt.inner.config.mods_root, m.folder.as_str())?;
            fs::remove_dir_all(path).map_err(|e| io_at(&rt.inner.config.mods_root, e))?;
            let mut state = read_optional_state(&rt.mods().state_path())?;
            state.enabled.retain(|v| v != uid.as_str());
            state.selected_variants.remove(uid.as_str());
            atomic_json(&rt.mods().state_path(), &state)?;
            rt.emit(EventIntent::ModsChanged);
            Ok(())
        })
    }

    /// Legacy `removeMod` accepts the packet folder name and only removes directories
    /// carrying the legacy identity marker.
    pub fn remove_legacy_folder(&self, folder: &str) -> Result<bool> {
        let folder = domain::ModFolderId::new(folder)?;
        self.runtime.transaction(|rt| {
            let root = &rt.inner.config.mods_root;
            let path = match safe_entry(root, folder.as_str()) {
                Ok(path) => path,
                Err(RuntimeError::NotFound(_)) => return Ok(false),
                Err(error) => return Err(error),
            };
            let metadata = fs::symlink_metadata(&path).map_err(|e| io_at(&path, e))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(RuntimeError::UnsafePath(path));
            }
            let marker = path.join("__deltaID.json");
            let marker_metadata = match fs::symlink_metadata(&marker) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(io_at(&marker, error)),
            };
            if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
                return Ok(false);
            }
            fs::remove_dir_all(&path).map_err(|e| io_at(&path, e))?;
            rt.emit(EventIntent::ModsChanged);
            Ok(true)
        })
    }

    /// Validates and stages an archive without pretending to extract it. The integration
    /// owns format-specific extraction, but receives a root-constrained staging path.
    pub fn stage_archive(
        &self,
        source: &Path,
        validator: &dyn ArchiveAssetValidator,
    ) -> Result<PathBuf> {
        let source = source.canonicalize().map_err(|e| io_at(source, e))?;
        validator.validate_archive(&source)?;
        let staging = self
            .runtime
            .inner
            .config
            .data_root
            .join("staging")
            .join("mod-import");
        fs::create_dir_all(&staging).map_err(|e| io_at(&staging, e))?;
        ensure_safe_dir(&staging)?;
        let destination = staging.join(
            source
                .file_name()
                .ok_or_else(|| RuntimeError::Invalid("archive has no name".into()))?,
        );
        fs::copy(&source, &destination).map_err(|e| io_at(&destination, e))?;
        Ok(destination)
    }
}
fn read_optional_state(path: &Path) -> Result<EnabledState> {
    if !path.exists() {
        return Ok(EnabledState {
            enabled: Vec::new(),
            selected_variants: BTreeMap::new(),
        });
    }
    read_json(path)
}

trait PlanPath {
    fn path(self) -> PathBuf;
}
impl PlanPath for domain::ModPlan {
    fn path(self) -> PathBuf {
        match self {
            domain::ModPlan::DeleteFolder(p) => p,
            domain::ModPlan::RemoveEnabled(_) => PathBuf::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeJson {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub built_in: bool,
    pub icon: Option<String>,
    pub music: Option<String>,
}
#[derive(Clone)]
pub struct ThemeService {
    runtime: Runtime,
}
impl ThemeService {
    fn active_path(&self) -> PathBuf {
        self.runtime
            .inner
            .config
            .data_root
            .join("active-theme.json")
    }
    pub fn list(&self) -> Result<Vec<ThemeJson>> {
        let root = &self.runtime.inner.config.themes_root;
        ensure_safe_dir(root)?;
        let mut out = Vec::new();
        for e in fs::read_dir(root).map_err(|e| io_at(root, e))? {
            let e = e.map_err(|e| io_at(root, e))?;
            let p = e.path();
            let m = fs::symlink_metadata(&p).map_err(|x| io_at(&p, x))?;
            if !m.is_dir() || m.file_type().is_symlink() {
                continue;
            }
            let f = p.join("theme.json");
            if f.is_file() {
                out.push(read_json(&f)?)
            }
        }
        out.sort_by(|a: &ThemeJson, b| a.id.cmp(&b.id));
        Ok(out)
    }
    pub fn get(&self, id: &str) -> Result<ThemeJson> {
        let id = domain::ThemeId::new(id)?;
        let p = safe_entry(&self.runtime.inner.config.themes_root, id.as_str())?.join("theme.json");
        read_json(&p)
    }
    pub fn set_active(&self, id: &str) -> Result<ThemeJson> {
        let id = domain::ThemeId::new(id)?;
        let theme = self.get(id.as_str())?;
        atomic_json(&self.active_path(), &json!({"id":id.as_str()}))?;
        self.runtime
            .emit(EventIntent::ActiveThemeChanged { id: id.to_string() });
        Ok(theme)
    }
    pub fn active(&self) -> Result<ThemeJson> {
        let id: Value = read_json(&self.active_path())?;
        self.get(
            id.get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| RuntimeError::Invalid("active theme state".into()))?,
        )
    }
    pub fn rename(&self, id: &str, name: &str) -> Result<ThemeJson> {
        let mut t = self.get(id)?;
        let tid = domain::ThemeId::new(id)?;
        let tr = domain::ThemeRecord {
            id: tid.clone(),
            name: t.name.clone(),
            built_in: t.built_in,
            icon: None,
            music: None,
        };
        domain::reject_builtin_mutation(&tr)?;
        let import = domain::ThemeImport {
            id: tid,
            name: name.to_string(),
            files: Vec::new(),
        };
        domain::validate_theme_import(&import)?;
        t.name = name.to_string();
        let p = safe_entry(&self.runtime.inner.config.themes_root, id)?.join("theme.json");
        atomic_json(&p, &t)?;
        self.runtime.emit(EventIntent::ThemesChanged);
        Ok(t)
    }
    pub fn delete(&self, id: &str) -> Result<()> {
        let t = self.get(id)?;
        let tid = domain::ThemeId::new(id)?;
        let tr = domain::ThemeRecord {
            id: tid.clone(),
            name: t.name,
            built_in: t.built_in,
            icon: None,
            music: None,
        };
        domain::reject_builtin_mutation(&tr)?;
        let active = self.active().ok();
        let available = self
            .list()?
            .into_iter()
            .map(|x| domain::ThemeId::new(&x.id))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if active.as_ref().is_some_and(|x| x.id == id) {
            let fallback = domain::active_deletion_fallback(
                &tid,
                &tid,
                &available,
                &domain::ThemeId::new(&self.runtime.inner.config.default_theme)?,
            )?;
            self.set_active(fallback.as_str())?;
        }
        let p = safe_entry(&self.runtime.inner.config.themes_root, id)?;
        fs::remove_dir_all(p).map_err(|e| io_at(&self.runtime.inner.config.themes_root, e))?;
        self.runtime.emit(EventIntent::ThemesChanged);
        Ok(())
    }

    pub fn import(
        &self,
        theme: ThemeJson,
        assets: Vec<domain::AssetInput>,
        validator: &dyn ThemeAssetValidator,
    ) -> Result<()> {
        let id = domain::ThemeId::new(&theme.id)?;
        let import = domain::ThemeImport {
            id: id.clone(),
            name: theme.name.clone(),
            files: assets.clone(),
        };
        domain::validate_theme_import(&import)?;
        validator.validate_assets(&assets)?;
        self.runtime.transaction(|rt| {
            let root = &rt.inner.config.themes_root;
            let final_dir = child(root, id.as_str())?;
            if final_dir.exists() {
                return Err(RuntimeError::Invalid("theme already exists".into()));
            }
            let staging = rt
                .inner
                .config
                .data_root
                .join("staging")
                .join(format!("theme-{}", id));
            if staging.exists() {
                fs::remove_dir_all(&staging).map_err(|e| io_at(&staging, e))?;
            }
            fs::create_dir_all(&staging).map_err(|e| io_at(&staging, e))?;
            for asset in assets {
                let destination = child(&staging, &asset.name)?;
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|e| io_at(parent, e))?;
                }
                fs::write(&destination, asset.bytes).map_err(|e| io_at(&destination, e))?;
            }
            atomic_json(&staging.join("theme.json"), &theme)?;
            fs::rename(&staging, &final_dir).map_err(|e| io_at(&final_dir, e))?;
            rt.emit(EventIntent::ThemesChanged);
            Ok(())
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PreferenceData {
    #[serde(default)]
    pub unique_flags: BTreeMap<String, bool>,
}
#[derive(Clone)]
pub struct PreferenceService {
    runtime: Runtime,
}
impl PreferenceService {
    fn path(&self) -> PathBuf {
        self.runtime.inner.config.data_root.join("preferences.json")
    }
    pub fn get(&self, name: &str) -> Result<bool> {
        validate_flag(name)?;
        Ok(read_optional::<PreferenceData>(&self.path())?
            .unique_flags
            .get(name)
            .copied()
            .unwrap_or(false))
    }
    pub fn set(&self, name: &str, value: bool) -> Result<bool> {
        validate_flag(name)?;
        self.runtime.transaction(|rt| {
            let mut p = read_optional::<PreferenceData>(&rt.preferences().path())?;
            p.unique_flags.insert(name.to_string(), value);
            atomic_json(&rt.preferences().path(), &p)?;
            rt.emit(EventIntent::PreferencesChanged);
            Ok(value)
        })
    }
}
fn validate_flag(s: &str) -> Result<()> {
    if (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && s.as_bytes()[0].is_ascii_uppercase()
    {
        Ok(())
    } else {
        Err(RuntimeError::Invalid("invalid preference flag".into()))
    }
}
fn read_optional<T: Default + for<'de> Deserialize<'de>>(p: &Path) -> Result<T> {
    if !p.exists() {
        Ok(T::default())
    } else {
        read_json(p)
    }
}

#[derive(Clone)]
pub struct SharedService {
    runtime: Runtime,
}
impl SharedService {
    pub fn get(&self) -> Result<Value> {
        let p = self.runtime.inner.config.data_root.join("shared.json");
        if !p.exists() {
            return Ok(json!([]));
        }
        read_json(&p)
    }
    pub fn set(&self, value: Value) -> Result<()> {
        let p = self.runtime.inner.config.data_root.join("shared.json");
        self.runtime.transaction(|rt| {
            let vars: Vec<WireSharedVariable> = serde_json::from_value(value.clone())
                .map_err(|e| RuntimeError::Invalid(e.to_string()))?;
            let vars = vars
                .into_iter()
                .map(WireSharedVariable::domain)
                .collect::<Result<Vec<_>>>()?;
            domain::validate_shared_variables(&vars)?;
            atomic_json(&p, &value)?;
            rt.emit(EventIntent::SharedChanged);
            Ok(())
        })
    }
}
#[derive(Deserialize)]
struct WireSharedVariable {
    name: String,
    kind: String,
    value: String,
}
impl WireSharedVariable {
    fn domain(self) -> Result<domain::SharedVariable> {
        let kind = match self.kind.as_str() {
            "text" | "Text" => domain::VariableType::Text,
            "integer" | "Integer" => domain::VariableType::Integer,
            "boolean" | "Boolean" => domain::VariableType::Boolean,
            "color" | "Color" => domain::VariableType::Color,
            "asset" | "Asset" => domain::VariableType::Asset,
            _ => return Err(RuntimeError::Invalid("invalid shared variable type".into())),
        };
        Ok(domain::SharedVariable {
            name: self.name,
            kind,
            value: self.value,
        })
    }
}
#[derive(Clone)]
pub struct SponsorService {
    runtime: Runtime,
}
impl SponsorService {
    pub fn list(&self) -> Result<Value> {
        let p = self.runtime.inner.config.data_root.join("sponsors.json");
        if !p.exists() {
            return Ok(json!([]));
        }
        read_json(&p)
    }
    pub fn set(&self, value: Value) -> Result<()> {
        let p = self.runtime.inner.config.data_root.join("sponsors.json");
        let list: Vec<WireSponsor> = serde_json::from_value(value.clone())
            .map_err(|e| RuntimeError::Invalid(e.to_string()))?;
        for s in list {
            domain::validate_sponsor_manifest(&s.domain()?)?
        }
        atomic_json(&p, &value)?;
        self.runtime.emit(EventIntent::SponsorsChanged);
        Ok(())
    }
}
#[derive(Deserialize)]
struct WireSponsor {
    id: String,
    display_name: String,
    root: Option<String>,
}
impl WireSponsor {
    fn domain(&self) -> Result<domain::SponsorManifest> {
        Ok(domain::SponsorManifest {
            id: domain::SponsorId::new(&self.id)?,
            display_name: self.display_name.clone(),
            root: self
                .root
                .as_deref()
                .map(domain::AssetRef::new)
                .transpose()?,
        })
    }
}

pub trait ArchiveAssetValidator: Send + Sync {
    fn validate_archive(&self, source: &Path) -> Result<()>;
}
pub trait ThemeAssetValidator: Send + Sync {
    fn validate_assets(&self, assets: &[domain::AssetInput]) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    fn rt() -> Runtime {
        let d = tempdir().unwrap();
        let base = d.keep();
        Runtime::open(RuntimeConfig::new(
            base.join("data"),
            base.join("mods"),
            base.join("themes"),
            "base",
        ))
        .unwrap()
    }
    #[test]
    fn prefs_are_atomic_and_legacy_shaped() {
        let r = rt();
        assert!(!r.preferences().get("AUDIO").unwrap());
        assert!(r.preferences().set("AUDIO", true).unwrap());
        assert!(r.preferences().get("AUDIO").unwrap());
        assert!(r.preferences().get("audio").is_err())
    }
    #[test]
    fn mod_operations_use_manifests() {
        let r = rt();
        let p = r.inner.config.mods_root.join("folder");
        fs::create_dir_all(&p).unwrap();
        fs::write(
            p.join("manifest.json"),
            r#"{"uid":"mod-a","name":"A","variants":[{"id":"default","label":"Default"}]}"#,
        )
        .unwrap();
        assert_eq!(r.mods().count().unwrap(), 1);
        r.mods().toggle("mod-a", true).unwrap();
        r.mods().set_variant("mod-a", "default").unwrap();
        r.mods().remove("mod-a").unwrap();
        assert_eq!(r.mods().count().unwrap(), 0)
    }
    #[test]
    fn legacy_removal_requires_marker_and_folder_name() {
        let r = rt();
        let unmarked = r.inner.config.mods_root.join("unmarked");
        fs::create_dir_all(&unmarked).unwrap();
        assert!(!r.mods().remove_legacy_folder("unmarked").unwrap());
        assert!(unmarked.exists());

        let marked = r.inner.config.mods_root.join("marked");
        fs::create_dir_all(&marked).unwrap();
        fs::write(marked.join("__deltaID.json"), b"{}").unwrap();
        assert!(r.mods().remove_legacy_folder("marked").unwrap());
        assert!(!marked.exists());
        assert!(r.mods().remove_legacy_folder("../outside").is_err());
    }
    #[test]
    fn symlink_is_rejected() {
        let r = rt();
        let outside = tempdir().unwrap();
        let link = r.inner.config.themes_root.join("x");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        #[cfg(windows)]
        if std::os::windows::fs::symlink_dir(outside.path(), &link).is_err() {
            return;
        }
        assert!(r.themes().list().is_ok());
        assert!(r.themes().get("x").is_err())
    }
}
