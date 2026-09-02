use crate::profile_registry::ProfileRegistry;
use crate::provider_cache::ProviderCatalogCache;
use deltamod_asset_runtime::{AssetRuntime, Roots as AssetRuntimeRoots};
use deltamod_credentials_adapter::{CredentialStore, KeyringBackend};
use deltamod_game_download_runtime::{
    BuiltInResolver, ButlerConfig, ButlerdAdapter, CancellationToken as GameDownloadCancellation,
    Catalog as GameDownloadCatalog, Runtime as GameDownloadRuntime,
};
use deltamod_mods_themes_runtime::{
    Runtime as ModsThemesRuntime, RuntimeConfig as ModsThemesConfig,
};
use deltamod_native_core::patch_plan::PatchPlatform;
use deltamod_network_runtime::Client as NetworkClient;
use deltamod_patching_runtime::{PlatformDefinition, Runtime as PatchingRuntime};
use deltamod_profile_install_runtime::Runtime as ProfileRuntime;
use deltamod_protocol_domain::{AssetRoots, PendingQueue};
use deltamod_storage_domain::{DataRoot, ProfileStore};
use deltamod_updater_launch_runtime::tauri_adapter::{
    Adapter as TrustedUpdateAdapter, OfficialUpdaterPlugin,
};
use deltamod_updater_launch_runtime::{
    GameLifecycle, GameRuntime, GameRuntimeConfig, HostPlatform, SystemProcessSpawner,
    SystemSteamOpener, UpdateError, UpdateEvent, UpdateEventSink, UpdateInfo, Updater, UpdaterGate,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update as TauriUpdate, UpdaterExt};

const UPDATE_ENDPOINT: &str =
    "https://github.com/cmdr-chara/deltamod/releases/latest/download/latest.json";

pub(crate) struct DownloadedUpdate {
    update: TauriUpdate,
    bytes: Vec<u8>,
}

pub(crate) struct TauriUpdaterHost {
    app: Option<tauri::AppHandle>,
    pending: Option<TauriUpdate>,
}

impl TauriUpdaterHost {
    fn new(app: Option<tauri::AppHandle>) -> Self {
        Self { app, pending: None }
    }

    fn app(&self) -> Result<&tauri::AppHandle, UpdateError> {
        self.app
            .as_ref()
            .ok_or_else(|| UpdateError::Adapter("updater is not configured".into()))
    }
}

impl OfficialUpdaterPlugin for TauriUpdaterHost {
    type VerifiedPayload = DownloadedUpdate;

    fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError> {
        let updater = self.app()?.updater().map_err(plugin_error)?;
        let update = tauri::async_runtime::block_on(updater.check()).map_err(plugin_error)?;
        self.pending = update;
        self.pending
            .as_ref()
            .map(|update| UpdateInfo::available(update.version.clone(), update.body.clone()))
            .transpose()
    }

    fn download_and_verify(
        &mut self,
        info: &UpdateInfo,
        max_bytes: u64,
        progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
    ) -> Result<Self::VerifiedPayload, UpdateError> {
        let update = self
            .pending
            .take()
            .ok_or_else(|| UpdateError::Adapter("checked update is unavailable".into()))?;
        if update.version != info.version {
            return Err(UpdateError::VersionMismatch);
        }
        let mut progress_error = None;
        let bytes = tauri::async_runtime::block_on(update.download(
            |chunk, total| {
                if progress_error.is_none() {
                    progress_error = progress(chunk as u64, total).err();
                }
            },
            || {},
        ))
        .map_err(plugin_error)?;
        if let Some(error) = progress_error {
            return Err(error);
        }
        if bytes.len() as u64 > max_bytes {
            return Err(UpdateError::ArtifactTooLarge { limit: max_bytes });
        }
        Ok(DownloadedUpdate { update, bytes })
    }

    fn install_verified(&mut self, payload: &Self::VerifiedPayload) -> Result<(), UpdateError> {
        payload.update.install(&payload.bytes).map_err(plugin_error)
    }
}

fn plugin_error(error: tauri_plugin_updater::Error) -> UpdateError {
    let message = error.to_string().chars().take(512).collect::<String>();
    match error {
        tauri_plugin_updater::Error::Minisign(_)
        | tauri_plugin_updater::Error::Base64(_)
        | tauri_plugin_updater::Error::SignatureUtf8(_) => {
            UpdateError::SignatureVerification(message)
        }
        _ => UpdateError::Adapter(message),
    }
}

#[derive(Clone)]
pub struct UpdateEvents(Option<tauri::AppHandle>);

impl UpdateEventSink for UpdateEvents {
    fn emit(&self, event: UpdateEvent) {
        let Some(app) = &self.0 else { return };
        let (name, payload) = match event {
            UpdateEvent::Available(info) => (
                "updateAvailable",
                json!({"update": info.update, "version": info.version, "releaseName": info.release_name}),
            ),
            UpdateEvent::Status(status) => (
                "updater-status",
                json!({"state": status.state, "available": status.available, "supported": status.supported, "version": status.version, "reason": status.reason}),
            ),
            UpdateEvent::Progress(progress) => (
                "updater-progress",
                json!({"operationId": progress.operation_id, "phase": progress.phase, "completed": progress.completed, "total": progress.total, "percentage": progress.percentage}),
            ),
        };
        let _ = app.emit(name, payload);
    }
}

pub(crate) type ShellUpdater = Updater<TrustedUpdateAdapter<TauriUpdaterHost>, UpdateEvents>;

struct TauriGameLifecycle(tauri::AppHandle);

impl GameLifecycle for TauriGameLifecycle {
    fn launched(&self) {
        if let Some(window) = self.0.get_webview_window("main") {
            let _ = window.hide();
        }
        let _ = self.0.emit("audio", false);
    }

    fn finished(&self, _success: bool) {
        if let Some(window) = self.0.get_webview_window("main") {
            let _ = window.show();
        }
        let _ = self.0.emit("audio", true);
        let _ = self.0.emit("page", "main");
    }

    fn steam_launched(&self) {
        self.0.exit(0);
    }
}

const MAX_FLAGS: usize = 256;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub unique_flags: BTreeMap<String, bool>,
}

#[derive(Debug, Default)]
pub struct EasterEggWindowState {
    pub generation: u64,
    pub origin: Option<(i32, i32)>,
}

pub struct AppState {
    pub data_root: DataRoot,
    pub _assets: AssetRoots,
    pub pending: PendingQueue,
    pub preferences: Mutex<Preferences>,
    pub mods_themes: ModsThemesRuntime,
    pub profile_runtime: ProfileRuntime,
    pub network: NetworkClient,
    pub network_runtime: Mutex<tokio::runtime::Runtime>,
    pub provider_cache: Mutex<ProviderCatalogCache>,
    pub profile_registry: Mutex<ProfileRegistry>,
    pub game_download: GameDownloadRuntime,
    pub butlerd: Option<ButlerdAdapter>,
    pub game_download_cancellations: Mutex<HashMap<String, GameDownloadCancellation>>,
    /// None means the platform secure store could not be initialized. Never fall back to files.
    pub credentials: Option<CredentialStore<KeyringBackend>>,
    pub gamebanana_login_active: AtomicBool,
    pub nexus_oauth_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub nexus_oauth_refresh: Mutex<()>,
    pub assets: AssetRuntime,
    pub mod_images: HashMap<String, String>,
    pub game: GameRuntime,
    pub patching: PatchingRuntime,
    pub patch_cancelled: AtomicBool,
    pub patch_sequence: AtomicU64,
    pub startup_recovery_errors: Mutex<Vec<String>>,
    pub easter_egg_window: Arc<Mutex<EasterEggWindowState>>,
    pub game_store_path: PathBuf,
    pub updater: Mutex<ShellUpdater>,
}

impl AppState {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn initialize(data_dir: PathBuf, resource_dir: PathBuf) -> Result<Self, &'static str> {
        Self::initialize_inner(data_dir, resource_dir, None)
    }

    pub fn initialize_with_app(
        data_dir: PathBuf,
        resource_dir: PathBuf,
        app: tauri::AppHandle,
    ) -> Result<Self, &'static str> {
        crate::controller::install_protocols(&app)?;
        Self::initialize_inner(data_dir, resource_dir, Some(app))
    }

    fn initialize_inner(
        data_dir: PathBuf,
        resource_dir: PathBuf,
        app_handle: Option<tauri::AppHandle>,
    ) -> Result<Self, &'static str> {
        let test_mode = app_handle.is_none();
        let data_root = DataRoot::new(data_dir).map_err(|_| "state root unavailable")?;
        let themes = data_root.root.join("themes");
        fs::create_dir_all(&themes).map_err(|_| "state root unavailable")?;
        let packaged_themes = resource_dir.join("themes");
        let builtin_themes = if packaged_themes.is_dir() {
            packaged_themes
        } else if test_mode {
            themes.clone()
        } else {
            return Err("packaged themes unavailable");
        };
        let app = resource_dir;
        let packet_dir = data_root.root.join("packets");
        fs::create_dir_all(&packet_dir).map_err(|_| "state root unavailable")?;
        let packet_roots: HashMap<String, PathBuf> = fs::read_dir(&packet_dir)
            .map_err(|_| "state root unavailable")?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_type()
                    .ok()
                    .filter(|kind| kind.is_dir())
                    .and_then(|_| entry.file_name().into_string().ok())
                    .map(|name| (name, entry.path()))
            })
            .collect();
        let mod_images = index_mod_images(&packet_roots);
        let preferences = load_preferences(&data_root);
        let mods_themes = ModsThemesRuntime::open(ModsThemesConfig::new(
            data_root.root.join("runtime"),
            data_root.root.join("mods"),
            themes.clone(),
            "base",
        ))
        .map_err(|_| "state root unavailable")?;
        let profile_runtime =
            ProfileRuntime::open(data_root.root.clone()).map_err(|_| "state root unavailable")?;
        let network = NetworkClient::new(Duration::from_secs(20), 2, Duration::from_millis(100))
            .map_err(|_| "network unavailable")?;
        let credentials = if test_mode {
            None
        } else {
            CredentialStore::new(Arc::new(KeyringBackend::new())).ok()
        };
        let network_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "network unavailable")?;
        let provider_cache = ProviderCatalogCache::open(&data_root.root)
            .map_err(|_| "provider cache unavailable")?;
        let profile_registry =
            ProfileRegistry::open(&data_root.root).map_err(|_| "profile registry unavailable")?;
        let game_download_catalog = if test_mode && !app.join("games").is_dir() {
            GameDownloadCatalog::new(Vec::new()).map_err(|_| "game download unavailable")?
        } else {
            packaged_game_download_catalog(&app)?
        };
        let game_download = network_runtime
            .block_on(GameDownloadRuntime::new(
                game_download_catalog,
                Arc::new(BuiltInResolver::new().map_err(|_| "game download unavailable")?),
                data_root.root.join("game-downloads"),
            ))
            .map_err(|_| "game download unavailable")?;
        let butlerd = packaged_butler(&app, &data_root.root).ok();
        let assets = AssetRuntime::new(AssetRuntimeRoots {
            app: app.clone(),
            builtin_theme: builtin_themes.clone(),
            user_theme: Some(themes.clone()),
            packets: packet_roots,
        })
        .map_err(|_| "asset roots unavailable")?;
        let game_store_path = selected_game_store(&data_root);
        let host = HostPlatform::current().map_err(|_| "game platform unavailable")?;
        let game_config = GameRuntimeConfig::new(app.join("games"), game_store_path.clone(), host);
        let game = if let Some(app_handle) = app_handle.clone() {
            GameRuntime::with_adapters(
                game_config,
                Arc::new(SystemProcessSpawner),
                Arc::new(SystemSteamOpener),
                Arc::new(TauriGameLifecycle(app_handle)),
            )
        } else {
            GameRuntime::new(game_config)
        };
        let patching = patching_runtime(&data_root, &app, &game_store_path)?;
        let updater_app = app_handle.clone();
        let updater_gate = updater_gate(app_handle.as_ref());
        let updater = Updater::new(
            TrustedUpdateAdapter(TauriUpdaterHost::new(updater_app.clone())),
            UpdateEvents(updater_app),
            updater_gate,
        );
        Ok(Self {
            data_root,
            _assets: AssetRoots {
                app,
                builtin_theme: builtin_themes,
                user_theme: Some(themes),
                packets: HashMap::new(),
            },
            pending: PendingQueue::new(),
            preferences: Mutex::new(preferences),
            mods_themes,
            profile_runtime,
            network,
            network_runtime: Mutex::new(network_runtime),
            provider_cache: Mutex::new(provider_cache),
            profile_registry: Mutex::new(profile_registry),
            game_download,
            butlerd,
            game_download_cancellations: Mutex::new(HashMap::new()),
            credentials,
            gamebanana_login_active: AtomicBool::new(false),
            nexus_oauth_cancel: Mutex::new(None),
            nexus_oauth_refresh: Mutex::new(()),
            assets,
            mod_images,
            game,
            patching,
            patch_cancelled: AtomicBool::new(false),
            patch_sequence: AtomicU64::new(0),
            startup_recovery_errors: Mutex::new(Vec::new()),
            easter_egg_window: Arc::new(Mutex::new(EasterEggWindowState::default())),
            game_store_path,
            updater: Mutex::new(updater),
        })
    }

    pub fn profile(&self) -> Result<ProfileStore, &'static str> {
        if !self.data_root.installations().is_file() {
            return Ok(ProfileStore::default());
        }
        deltamod_storage_domain::load_json(&self.data_root.installations())
            .map_err(|_| "profile unavailable")
    }

    pub fn save_preferences(&self, preferences: &Preferences) -> Result<(), &'static str> {
        if preferences.unique_flags.len() > MAX_FLAGS {
            return Err("preferences limit exceeded");
        }
        deltamod_storage_domain::save_json(
            &self.data_root.root.join("preferences.json"),
            preferences,
            true,
        )
        .map_err(|_| "preferences unavailable")
    }
}

fn updater_gate(app: Option<&tauri::AppHandle>) -> UpdaterGate {
    let Some(app) = app else {
        return UpdaterGate::disabled();
    };
    let config = app.config();
    let updater = config
        .plugins
        .0
        .get("updater")
        .and_then(|value| value.as_object());
    let endpoints = updater
        .and_then(|value| value.get("endpoints"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let public_key = updater
        .and_then(|value| value.get("pubkey"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let endpoints = if endpoints.as_slice() == [UPDATE_ENDPOINT] {
        endpoints
    } else {
        Vec::new()
    };
    let updater_artifacts = matches!(
        config.bundle.create_updater_artifacts,
        tauri::utils::config::Updater::Bool(true)
    );
    UpdaterGate::configured(
        !cfg!(debug_assertions),
        cfg!(any(target_os = "windows", target_os = "macos")),
        updater_artifacts,
        &endpoints,
        public_key,
    )
}

fn packaged_game_download_catalog(
    resources: &std::path::Path,
) -> Result<GameDownloadCatalog, &'static str> {
    let root = resources.join("games");
    let mut values = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| "packaged game catalog unavailable")? {
        let path = entry
            .map_err(|_| "packaged game catalog unavailable")?
            .path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| "packaged game catalog unavailable")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024
        {
            return Err("packaged game catalog unavailable");
        }
        values.push(
            serde_json::from_slice(
                &fs::read(path).map_err(|_| "packaged game catalog unavailable")?,
            )
            .map_err(|_| "packaged game catalog unavailable")?,
        );
    }
    GameDownloadCatalog::from_legacy_values(&values)
        .map_err(|_| "packaged game catalog unavailable")
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ButlerProvenance {
    version: String,
    executable: String,
    sha256: String,
}

fn packaged_butler(
    resources: &std::path::Path,
    data_root: &std::path::Path,
) -> Result<ButlerdAdapter, &'static str> {
    let root = resources.join("third-party").join("butler");
    let provenance_path = root.join("provenance.json");
    let metadata = fs::symlink_metadata(&provenance_path).map_err(|_| "butler unavailable")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16 * 1024 {
        return Err("butler unavailable");
    }
    let provenance: ButlerProvenance =
        serde_json::from_slice(&fs::read(provenance_path).map_err(|_| "butler unavailable")?)
            .map_err(|_| "butler unavailable")?;
    if provenance.version != "15.30.0"
        || provenance.executable.contains(['/', '\\'])
        || provenance.executable.is_empty()
    {
        return Err("butler unavailable");
    }
    ButlerdAdapter::new(ButlerConfig {
        executable: root.join(provenance.executable),
        executable_sha256: provenance.sha256,
        database: data_root.join("butler").join("butler.db"),
        user_agent: format!("Deltamod-Community/{}", env!("CARGO_PKG_VERSION")),
    })
    .map_err(|_| "butler unavailable")
}

fn patching_runtime(
    root: &DataRoot,
    resources: &std::path::Path,
    store_path: &std::path::Path,
) -> Result<PatchingRuntime, &'static str> {
    let store = deltamod_storage_domain::load_json::<serde_json::Map<String, serde_json::Value>>(
        store_path,
    )
    .unwrap_or_default();
    let game_root = store
        .get("gamePath")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            store_path
                .parent()
                .unwrap_or(&root.root)
                .join("deltaruneInstall")
        });
    let platform_name = store
        .get("gamePlatform")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            other => other,
        });
    let platform = match platform_name {
        "win32" => PatchPlatform::Win32,
        "darwin" => PatchPlatform::Darwin,
        _ => PatchPlatform::Linux,
    };
    let definition = patch_definition(resources, &store, platform_name);
    let packaged_tools = resources.join("third-party");
    let tools_root = if packaged_tools.is_dir() {
        packaged_tools
    } else if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("tool root unavailable")?
            .join("tools")
    } else {
        return Err("tool root unavailable");
    };
    Ok(PatchingRuntime {
        game_root,
        mod_root: root.root.join("packets"),
        tools_root,
        hash_cache_path: root.root.join("_game-hashes.json"),
        platform,
        platform_name: match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            other => other,
        }
        .into(),
        arch: match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            other => other,
        }
        .into(),
        definition,
    })
}

fn patch_definition(
    resources: &std::path::Path,
    store: &serde_json::Map<String, serde_json::Value>,
    platform: &str,
) -> PlatformDefinition {
    let game = store
        .get("gamePid")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| {
            deltamod_storage_domain::load_json::<serde_json::Value>(
                &resources.join("games").join(format!("{id}.json")),
            )
            .ok()
        });
    let definition = game
        .as_ref()
        .and_then(|game| game.get("platforms"))
        .and_then(|platforms| platforms.get(platform));
    PlatformDefinition {
        data_files: definition
            .and_then(|value| value.get("dataFiles"))
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .filter(|values: &Vec<String>| !values.is_empty())
            .unwrap_or_else(|| vec!["data.win".into()]),
        patch_layout: definition
            .and_then(|value| value.get("patchLayout"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("windows-root")
            .into(),
        content_root: definition
            .and_then(|value| value.get("contentRoot"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    }
}

fn selected_game_store(root: &DataRoot) -> PathBuf {
    let index = deltamod_storage_domain::load_json::<ProfileStore>(&root.installations())
        .ok()
        .and_then(|profile| profile.current_index)
        .unwrap_or(0);
    root.root
        .join(format!("deltamod_system-{index}"))
        .join("store.json")
}

fn index_mod_images(packet_roots: &HashMap<String, PathBuf>) -> HashMap<String, String> {
    packet_roots
        .iter()
        .filter(|(packet, _)| {
            !packet.is_empty()
                && packet.len() <= 128
                && packet
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .filter_map(|(packet, root)| {
            let metadata_path = root.join("__deltaID.json");
            let icon_path = root.join("icon.png");
            let metadata = fs::symlink_metadata(&metadata_path).ok()?;
            let icon = fs::symlink_metadata(&icon_path).ok()?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > 64 * 1024
                || !icon.is_file()
                || icon.file_type().is_symlink()
            {
                return None;
            }
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(metadata_path).ok()?).ok()?;
            let uid = value.get("uniqueId")?.as_str()?;
            if uid.is_empty() || uid.len() > 256 || uid.chars().any(char::is_control) {
                return None;
            }
            Some((uid.to_owned(), format!("packet://{packet}/icon.png")))
        })
        .collect()
}

fn load_preferences(root: &DataRoot) -> Preferences {
    deltamod_storage_domain::load_json(&root.root.join("preferences.json")).unwrap_or_default()
}
