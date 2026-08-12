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
use deltamod_updater_launch_runtime::{
    GameLifecycle, GameRuntime, GameRuntimeConfig, HostPlatform, SystemProcessSpawner,
    SystemSteamOpener, UpdateAdapter, UpdateError, UpdateEvent, UpdateEventSink, UpdateInfo,
    Updater, UpdaterGate, VerifiedArtifact,
};
use serde::{Deserialize, Serialize};
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

pub struct DisabledUpdateAdapter;

impl UpdateAdapter for DisabledUpdateAdapter {
    fn check(&mut self) -> Result<Option<UpdateInfo>, UpdateError> {
        Err(UpdateError::Adapter("updater is not configured".into()))
    }

    fn download_and_verify(
        &mut self,
        _: &UpdateInfo,
        _: u64,
        _: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
    ) -> Result<VerifiedArtifact, UpdateError> {
        Err(UpdateError::Adapter("updater is not configured".into()))
    }

    fn install(&mut self, _: VerifiedArtifact) -> Result<(), UpdateError> {
        Err(UpdateError::Adapter("updater is not configured".into()))
    }
}

#[derive(Clone)]
pub struct UpdateEvents(pub Arc<Mutex<Vec<UpdateEvent>>>);

impl UpdateEventSink for UpdateEvents {
    fn emit(&self, event: UpdateEvent) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event);
        }
    }
}

pub type ShellUpdater = Updater<DisabledUpdateAdapter, UpdateEvents>;

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

pub struct AppState {
    pub data_root: DataRoot,
    pub _assets: AssetRoots,
    pub pending: PendingQueue,
    pub preferences: Mutex<Preferences>,
    pub mods_themes: ModsThemesRuntime,
    pub profile_runtime: ProfileRuntime,
    pub network: NetworkClient,
    pub network_runtime: Mutex<tokio::runtime::Runtime>,
    pub game_download: GameDownloadRuntime,
    pub butlerd: Option<ButlerdAdapter>,
    pub game_download_cancellations: Mutex<HashMap<String, GameDownloadCancellation>>,
    /// None means the platform secure store could not be initialized. Never fall back to files.
    pub credentials: Option<CredentialStore<KeyringBackend>>,
    pub gamebanana_login_active: AtomicBool,
    pub assets: AssetRuntime,
    pub mod_images: HashMap<String, String>,
    pub game: GameRuntime,
    pub patching: PatchingRuntime,
    pub patch_cancelled: AtomicBool,
    pub patch_sequence: AtomicU64,
    pub game_store_path: PathBuf,
    pub updater: Mutex<ShellUpdater>,
    pub updater_events: Arc<Mutex<Vec<UpdateEvent>>>,
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
        let game = if let Some(app_handle) = app_handle {
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
        let updater_events = Arc::new(Mutex::new(Vec::new()));
        let updater = Updater::new(
            DisabledUpdateAdapter,
            UpdateEvents(Arc::clone(&updater_events)),
            UpdaterGate::disabled(),
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
            game_download,
            butlerd,
            game_download_cancellations: Mutex::new(HashMap::new()),
            credentials,
            gamebanana_login_active: AtomicBool::new(false),
            assets,
            mod_images,
            game,
            patching,
            patch_cancelled: AtomicBool::new(false),
            patch_sequence: AtomicU64::new(0),
            game_store_path,
            updater: Mutex::new(updater),
            updater_events,
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
