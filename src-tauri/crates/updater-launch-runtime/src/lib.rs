#![forbid(unsafe_code)]

use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    env, fmt, fs, io,
    path::{Component, Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
};

const MAX_CATALOG_FILES: usize = 128;
const MAX_JSON_BYTES: u64 = 1024 * 1024;

pub mod updater;
pub use updater::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Windows,
    Linux,
    Macos,
    Wine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchSpec {
    pub platform: Platform,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl LaunchSpec {
    pub fn new(
        platform: Platform,
        executable: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Result<Self, LaunchError> {
        let executable = executable.into();
        let cwd = cwd.into();
        if executable.as_os_str().is_empty() || cwd.as_os_str().is_empty() {
            return Err(LaunchError::InvalidSpec("executable and cwd are required"));
        }
        if !cwd.is_absolute() {
            return Err(LaunchError::InvalidSpec("cwd must be absolute"));
        }
        Ok(Self {
            platform,
            executable,
            args: Vec::new(),
            cwd,
            env: BTreeMap::new(),
        })
    }
    pub fn arg(mut self, value: impl Into<String>) -> Self {
        self.args.push(value.into());
        self
    }
    pub fn env(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, LaunchError> {
        let key = key.into();
        if !valid_env_key(&key) || is_secret_key(&key) {
            return Err(LaunchError::SecretEnvironment(key));
        }
        self.env.insert(key, value.into());
        Ok(self)
    }
    pub fn command(&self) -> Command {
        let mut command = if self.platform == Platform::Wine {
            let mut command = Command::new("wine");
            command.arg(&self.executable);
            command
        } else {
            Command::new(&self.executable)
        };
        command.args(&self.args).current_dir(&self.cwd).env_clear();
        for (key, value) in inherited_non_secret_environment() {
            command.env(key, value);
        }
        for (key, value) in sanitized_environment(&self.env) {
            command.env(key, value);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        command
    }
}

fn valid_env_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PRIVATE_KEY",
        "API_KEY",
        "AUTH",
    ]
    .iter()
    .any(|needle| k.contains(needle))
}
pub fn sanitized_environment(explicit: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    explicit
        .iter()
        .filter(|(key, _)| !is_secret_key(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
pub fn inherited_non_secret_environment() -> BTreeMap<String, String> {
    env::vars()
        .filter(|(key, _)| allowed_inherited_key(key))
        .collect()
}

fn allowed_inherited_key(key: &str) -> bool {
    let key = key.to_ascii_uppercase();
    matches!(
        key.as_str(),
        "PATH"
            | "PATHEXT"
            | "SYSTEMROOT"
            | "WINDIR"
            | "COMSPEC"
            | "TEMP"
            | "TMP"
            | "HOME"
            | "USERPROFILE"
            | "LANG"
            | "DISPLAY"
            | "WAYLAND_DISPLAY"
            | "XDG_RUNTIME_DIR"
            | "XAUTHORITY"
            | "WINEPREFIX"
            | "WINEARCH"
            | "WINEDLLOVERRIDES"
    ) || key.starts_with("LC_")
}

pub trait ChildProcess: Send {
    fn wait(&mut self) -> io::Result<ExitStatus>;
    fn kill(&mut self) -> io::Result<()>;
}
pub trait ProcessSpawner: Send + Sync {
    fn spawn(&self, spec: &LaunchSpec) -> Result<Box<dyn ChildProcess>, LaunchError>;
}
#[derive(Default)]
pub struct SystemProcessSpawner;
struct SystemChild(Child);
impl ChildProcess for SystemChild {
    fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.wait()
    }
    fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }
}
impl ProcessSpawner for SystemProcessSpawner {
    fn spawn(&self, spec: &LaunchSpec) -> Result<Box<dyn ChildProcess>, LaunchError> {
        Ok(Box::new(SystemChild(
            spec.command().spawn().map_err(LaunchError::Io)?,
        )))
    }
}

pub struct OwnedChild {
    child: Option<Box<dyn ChildProcess>>,
    hooks: Vec<Box<dyn FnOnce() + Send>>,
}
impl OwnedChild {
    pub fn new(child: Box<dyn ChildProcess>) -> Self {
        Self {
            child: Some(child),
            hooks: Vec::new(),
        }
    }
    pub fn on_finalize(&mut self, hook: impl FnOnce() + Send + 'static) {
        self.hooks.push(Box::new(hook));
    }
    pub fn wait(mut self) -> Result<ExitStatus, LaunchError> {
        let result = self
            .child
            .as_mut()
            .expect("owned child exists")
            .wait()
            .map_err(LaunchError::Io);
        self.finalize();
        result
    }
    pub fn kill(&mut self) -> Result<(), LaunchError> {
        self.child
            .as_mut()
            .ok_or(LaunchError::AlreadyFinalized)?
            .kill()
            .map_err(LaunchError::Io)
    }
    pub fn finalize(&mut self) {
        if self.child.take().is_some() {
            for hook in self.hooks.drain(..) {
                hook();
            }
        }
    }
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.finalize();
    }
}

pub fn launch(spec: LaunchSpec, spawner: &dyn ProcessSpawner) -> Result<OwnedChild, LaunchError> {
    Ok(OwnedChild::new(spawner.spawn(&spec)?))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SteamUri(String);
impl SteamUri {
    pub fn run(app_id: u32) -> Result<Self, SteamError> {
        if app_id == 0 {
            Err(SteamError::InvalidUri)
        } else {
            Ok(Self(format!("steam://run/{app_id}")))
        }
    }
    pub fn parse(raw: impl Into<String>) -> Result<Self, SteamError> {
        let raw = raw.into();
        let rest = raw
            .strip_prefix("steam://run/")
            .or_else(|| raw.strip_prefix("steam://rungameid/"))
            .ok_or(SteamError::InvalidUri)?;
        let id = rest.split(['/', '?']).next().unwrap_or("");
        if id.is_empty() || id.parse::<u32>().ok().filter(|n| *n > 0).is_none() || raw.contains('#')
        {
            return Err(SteamError::InvalidUri);
        }
        Ok(Self(raw))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
pub trait SteamOpener {
    fn open(&self, uri: &SteamUri) -> Result<(), SteamError>;
}
pub struct SystemSteamOpener;
impl SteamOpener for SystemSteamOpener {
    fn open(&self, uri: &SteamUri) -> Result<(), SteamError> {
        let mut command = if cfg!(target_os = "windows") {
            let mut c = Command::new("explorer.exe");
            c.arg(uri.as_str());
            c
        } else if cfg!(target_os = "macos") {
            let mut c = Command::new("open");
            c.arg(uri.as_str());
            c
        } else {
            let mut c = Command::new("xdg-open");
            c.arg(uri.as_str());
            c
        };
        command
            .status()
            .map(|_| ())
            .map_err(|e| SteamError::Io(e.to_string()))
    }
}

/// Host platform names intentionally match Node's `process.platform` values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostPlatform {
    Win32,
    Linux,
    Darwin,
}

impl HostPlatform {
    pub fn current() -> Result<Self, GameError> {
        match env::consts::OS {
            "windows" => Ok(Self::Win32),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Darwin),
            _ => Err(GameError::UnsupportedPlatform),
        }
    }

    fn as_legacy(self) -> &'static str {
        match self {
            Self::Win32 => "win32",
            Self::Linux => "linux",
            Self::Darwin => "darwin",
        }
    }
}

#[derive(Clone, Debug)]
pub struct GameRuntimeConfig {
    pub games_dir: PathBuf,
    pub store_path: PathBuf,
    pub host: HostPlatform,
}

impl GameRuntimeConfig {
    pub fn new(
        games_dir: impl Into<PathBuf>,
        store_path: impl Into<PathBuf>,
        host: HostPlatform,
    ) -> Self {
        Self {
            games_dir: games_dir.into(),
            store_path: store_path.into(),
            host,
        }
    }
}

pub trait GameLifecycle: Send + Sync {
    fn launched(&self) {}
    fn finished(&self, _success: bool) {}
    fn steam_launched(&self) {}
}

#[derive(Default)]
pub struct NoopGameLifecycle;
impl GameLifecycle for NoopGameLifecycle {}

#[derive(Default)]
struct ProcessState {
    running: bool,
    generation: u64,
}

/// Tauri-independent implementation of the legacy game query and launch channels.
///
/// A launch is reaped on a dedicated thread. The lifecycle callback is the host's
/// boundary for restoring patched files and restoring window/controller state.
pub struct GameRuntime {
    config: GameRuntimeConfig,
    spawner: Arc<dyn ProcessSpawner>,
    steam: Arc<dyn SteamOpener + Send + Sync>,
    lifecycle: Arc<dyn GameLifecycle>,
    process: Arc<Mutex<ProcessState>>,
}

impl GameRuntime {
    pub fn new(config: GameRuntimeConfig) -> Self {
        Self::with_adapters(
            config,
            Arc::new(SystemProcessSpawner),
            Arc::new(SystemSteamOpener),
            Arc::new(NoopGameLifecycle),
        )
    }

    pub fn with_adapters(
        config: GameRuntimeConfig,
        spawner: Arc<dyn ProcessSpawner>,
        steam: Arc<dyn SteamOpener + Send + Sync>,
        lifecycle: Arc<dyn GameLifecycle>,
    ) -> Self {
        Self {
            config,
            spawner,
            steam,
            lifecycle,
            process: Arc::new(Mutex::new(ProcessState::default())),
        }
    }

    /// Returns `Ok(None)` for channels owned by another dispatcher.
    pub fn dispatch(&self, channel: &str, data: &[Value]) -> Result<Option<Value>, GameError> {
        match channel {
            "getAvailableGames" => Ok(Some(Value::Array(self.catalog()?))),
            "getGameInfo" => {
                let value = match data.first().and_then(Value::as_str) {
                    Some(id) => self.game_by_id(id)?.unwrap_or(Value::Null),
                    None => Value::Null,
                };
                Ok(Some(value))
            }
            "getCurrentGameInfo" => {
                let store = self.store()?;
                let value = store
                    .get("gamePid")
                    .and_then(Value::as_str)
                    .map(|id| self.game_by_id(id))
                    .transpose()?
                    .flatten()
                    .unwrap_or(Value::Null);
                Ok(Some(value))
            }
            "loadedDeltarune" => Ok(Some(self.loaded_deltarune())),
            "startGame" => Ok(Some(json!(self.start_game()?))),
            // Electron has no such handler. Do not silently give it startGame semantics.
            "startGameVanilla" => Err(GameError::UnsupportedChannel),
            // The Electron handler is deliberately a no-op and resolves undefined.
            "executeArgumentCmd" => Ok(Some(Value::Null)),
            _ => Ok(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.process
            .lock()
            .map(|state| state.running)
            .unwrap_or(false)
    }

    fn catalog(&self) -> Result<Vec<Value>, GameError> {
        let mut entries = fs::read_dir(&self.config.games_dir)
            .map_err(|_| GameError::CatalogUnavailable)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| GameError::CatalogUnavailable)?;
        if entries.len() > MAX_CATALOG_FILES {
            return Err(GameError::CatalogLimit);
        }
        entries.sort_by_key(|entry| entry.file_name());
        entries
            .into_iter()
            .map(|entry| read_bounded_json_file(&entry.path(), GameError::InvalidCatalog))
            .collect()
    }

    fn game_by_id(&self, id: &str) -> Result<Option<Value>, GameError> {
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Ok(None);
        }
        Ok(self
            .catalog()?
            .into_iter()
            .find(|game| game.get("id").and_then(Value::as_str) == Some(id)))
    }

    fn store(&self) -> Result<Map<String, Value>, GameError> {
        if !self.config.store_path.is_file() {
            return Ok(Map::new());
        }
        read_bounded_json_file(&self.config.store_path, GameError::InvalidStore)?
            .as_object()
            .cloned()
            .ok_or(GameError::InvalidStore)
    }

    fn loaded_deltarune(&self) -> Value {
        let result = (|| {
            let store = self.store()?;
            let id = store
                .get("gamePid")
                .and_then(Value::as_str)
                .ok_or(GameError::InstallationUnavailable)?;
            let game = self
                .game_by_id(id)?
                .ok_or(GameError::InstallationUnavailable)?;
            self.resolve_installation(&store, &game)?;
            Ok::<_, GameError>(id.to_owned())
        })();
        match result {
            Ok(id) => json!({"loaded": true, "path": id}),
            Err(_) => json!({"loaded": false, "path": ""}),
        }
    }

    fn start_game(&self) -> Result<bool, GameError> {
        let store = self.store()?;
        let id = store
            .get("gamePid")
            .and_then(Value::as_str)
            .ok_or(GameError::InstallationUnavailable)?;
        let game = self
            .game_by_id(id)?
            .ok_or(GameError::InstallationUnavailable)?;
        let resolution = self.resolve_installation(&store, &game)?;

        if store.get("isSteam").and_then(Value::as_bool) == Some(true)
            && self.config.host == HostPlatform::Win32
        {
            let app_id = store
                .get("steamAppId")
                .and_then(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| value.as_u64().map(|n| n.to_string()))
                })
                .filter(|id| {
                    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
                })
                .and_then(|id| id.parse::<u32>().ok())
                .filter(|id| *id > 0)
                .ok_or(GameError::InvalidSteamAppId)?;
            let uri = SteamUri::parse(format!("steam://rungameid/{app_id}"))
                .or_else(|_| SteamUri::run(app_id))
                .map_err(|_| GameError::InvalidSteamAppId)?;
            self.steam.open(&uri).map_err(|_| GameError::LaunchFailed)?;
            self.lifecycle.steam_launched();
            return Ok(true);
        }

        if self.config.host == HostPlatform::Linux
            && resolution.platform == "win32"
            && store
                .get("linuxLauncher")
                .is_some_and(|value| !value.is_null())
        {
            // Electron permits an arbitrary persisted command here. Reproducing that
            // behavior would turn profile data into a command-execution primitive.
            return Err(GameError::ConfiguredLauncherUnsupported);
        }

        let spec = resolution.launch_spec(self.config.host)?;
        let mut state = self
            .process
            .lock()
            .map_err(|_| GameError::RuntimeUnavailable)?;
        if state.running {
            return Err(GameError::AlreadyRunning);
        }
        let child = self
            .spawner
            .spawn(&spec)
            .map_err(|_| GameError::LaunchFailed)?;
        state.running = true;
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        drop(state);
        self.lifecycle.launched();

        let process = Arc::clone(&self.process);
        let lifecycle = Arc::clone(&self.lifecycle);
        let child = Arc::new(Mutex::new(Some(child)));
        let waiter_child = Arc::clone(&child);
        let spawn_result = thread::Builder::new()
            .name("deltamod-game-wait".into())
            .spawn(move || {
                let success = waiter_child
                    .lock()
                    .ok()
                    .and_then(|mut slot| slot.take())
                    .and_then(|mut child| child.wait().ok())
                    .map(|status| status.success())
                    .unwrap_or(false);
                if let Ok(mut state) = process.lock() {
                    if state.generation == generation {
                        state.running = false;
                    }
                }
                lifecycle.finished(success);
            });
        if spawn_result.is_err() {
            if let Ok(mut state) = self.process.lock() {
                state.running = false;
            }
            if let Ok(mut slot) = child.lock() {
                if let Some(child) = slot.as_mut() {
                    let _ = child.kill();
                }
            }
            return Err(GameError::LaunchFailed);
        }
        Ok(true)
    }

    fn resolve_installation(
        &self,
        store: &Map<String, Value>,
        game: &Value,
    ) -> Result<GameResolution, GameError> {
        let root = store
            .get("gamePath")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or(GameError::InstallationUnavailable)?;
        if !root.is_absolute() {
            return Err(GameError::InstallationUnavailable);
        }
        let root = fs::canonicalize(root).map_err(|_| GameError::InstallationUnavailable)?;
        if !root.is_dir() {
            return Err(GameError::InstallationUnavailable);
        }
        let definitions = platform_definitions(game)?;
        let preferred = store.get("gamePlatform").and_then(Value::as_str);
        let mut candidates = Vec::new();
        for candidate in [
            preferred,
            Some(self.config.host.as_legacy()),
            (self.config.host == HostPlatform::Linux).then_some("win32"),
        ]
        .into_iter()
        .flatten()
        {
            if !candidates.contains(&candidate)
                && definitions.contains_key(candidate)
                && (candidate == self.config.host.as_legacy()
                    || (self.config.host == HostPlatform::Linux && candidate == "win32"))
            {
                candidates.push(candidate);
            }
        }
        for platform in candidates {
            let definition = definitions.get(platform).ok_or(GameError::InvalidCatalog)?;
            if let Ok(resolution) = GameResolution::from_definition(&root, platform, definition) {
                return Ok(resolution);
            }
        }
        Err(GameError::InstallationUnavailable)
    }
}

fn read_bounded_json_file(path: &Path, invalid: GameError) -> Result<Value, GameError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_JSON_BYTES {
        return Err(invalid);
    }
    let bytes = fs::read(path).map_err(|_| invalid)?;
    serde_json::from_slice(&bytes).map_err(|_| invalid)
}

fn platform_definitions(game: &Value) -> Result<Map<String, Value>, GameError> {
    let mut definitions = game
        .get("platforms")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !definitions.contains_key("win32") {
        if let Some(executable) = game.get("exeName").and_then(Value::as_str) {
            definitions.insert(
                "win32".into(),
                json!({"executable": executable, "dataFiles": ["data.win"], "patchLayout": "windows-root"}),
            );
        }
    }
    if definitions.is_empty() {
        Err(GameError::InvalidCatalog)
    } else {
        Ok(definitions)
    }
}

struct GameResolution {
    root: PathBuf,
    platform: String,
    executable: PathBuf,
    bundle: Option<PathBuf>,
}

impl GameResolution {
    fn from_definition(root: &Path, platform: &str, value: &Value) -> Result<Self, GameError> {
        let definition = value.as_object().ok_or(GameError::InvalidCatalog)?;
        let executable = definition
            .get("executable")
            .and_then(Value::as_str)
            .ok_or(GameError::InvalidCatalog)?;
        let executable = checked_required_path(root, executable, false)?;
        let data_files = definition
            .get("dataFiles")
            .and_then(Value::as_array)
            .ok_or(GameError::InvalidCatalog)?;
        if data_files.len() > 16 {
            return Err(GameError::InvalidCatalog);
        }
        for data in data_files {
            checked_required_path(root, data.as_str().ok_or(GameError::InvalidCatalog)?, false)?;
        }
        let bundle = definition
            .get("bundle")
            .and_then(Value::as_str)
            .map(|path| checked_required_path(root, path, true))
            .transpose()?;
        Ok(Self {
            root: root.to_path_buf(),
            platform: platform.to_owned(),
            executable,
            bundle,
        })
    }

    fn launch_spec(&self, host: HostPlatform) -> Result<LaunchSpec, GameError> {
        match self.platform.as_str() {
            "darwin" => {
                let bundle = self.bundle.as_ref().ok_or(GameError::InvalidCatalog)?;
                LaunchSpec::new(Platform::Macos, "open", &self.root)
                    .map_err(|_| GameError::InvalidCatalog)
                    .map(|spec| spec.arg("-W").arg(bundle.to_string_lossy()))
            }
            "linux" => LaunchSpec::new(Platform::Linux, "sh", &self.root)
                .map_err(|_| GameError::InvalidCatalog)
                .map(|spec| spec.arg(self.executable.to_string_lossy())),
            "win32" if host == HostPlatform::Linux => {
                LaunchSpec::new(Platform::Wine, &self.executable, &self.root)
                    .map_err(|_| GameError::InvalidCatalog)
            }
            "win32" => LaunchSpec::new(Platform::Windows, &self.executable, &self.root)
                .map_err(|_| GameError::InvalidCatalog),
            _ => Err(GameError::UnsupportedPlatform),
        }
    }
}

fn checked_required_path(
    root: &Path,
    relative: &str,
    directory: bool,
) -> Result<PathBuf, GameError> {
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative.len() > 512
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(GameError::InvalidCatalog);
    }
    let path = root.join(relative_path);
    let metadata = fs::symlink_metadata(&path).map_err(|_| GameError::InstallationUnavailable)?;
    if metadata.file_type().is_symlink()
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(GameError::InstallationUnavailable);
    }
    let canonical = fs::canonicalize(path).map_err(|_| GameError::InstallationUnavailable)?;
    if !canonical.starts_with(root) {
        return Err(GameError::InstallationUnavailable);
    }
    Ok(canonical)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameError {
    UnsupportedChannel,
    UnsupportedPlatform,
    CatalogUnavailable,
    CatalogLimit,
    InvalidCatalog,
    InvalidStore,
    InstallationUnavailable,
    InvalidSteamAppId,
    ConfiguredLauncherUnsupported,
    AlreadyRunning,
    RuntimeUnavailable,
    LaunchFailed,
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::UnsupportedChannel => "GAME_CHANNEL_UNSUPPORTED",
            Self::UnsupportedPlatform => "GAME_PLATFORM_UNSUPPORTED",
            Self::CatalogUnavailable => "GAME_CATALOG_UNAVAILABLE",
            Self::CatalogLimit => "GAME_CATALOG_LIMIT",
            Self::InvalidCatalog => "GAME_CATALOG_INVALID",
            Self::InvalidStore => "GAME_STORE_INVALID",
            Self::InstallationUnavailable => "GAME_INSTALLATION_UNAVAILABLE",
            Self::InvalidSteamAppId => "GAME_STEAM_APP_ID_INVALID",
            Self::ConfiguredLauncherUnsupported => "GAME_CONFIGURED_LAUNCHER_UNSUPPORTED",
            Self::AlreadyRunning => "GAME_ALREADY_RUNNING",
            Self::RuntimeUnavailable => "GAME_RUNTIME_UNAVAILABLE",
            Self::LaunchFailed => "GAME_LAUNCH_FAILED",
        };
        f.write_str(code)
    }
}

impl std::error::Error for GameError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppLifecycle {
    Starting,
    Ready,
    Launching,
    Running,
    ShuttingDown,
    Stopped,
    Failed,
}
#[derive(Debug)]
pub struct AppState {
    state: AppLifecycle,
}
impl AppState {
    pub fn new() -> Self {
        Self {
            state: AppLifecycle::Starting,
        }
    }
    pub fn state(&self) -> AppLifecycle {
        self.state
    }
    pub fn transition(&mut self, next: AppLifecycle) -> Result<(), StateError> {
        let valid = matches!(
            (self.state, next),
            (
                AppLifecycle::Starting,
                AppLifecycle::Ready | AppLifecycle::Failed
            ) | (
                AppLifecycle::Ready,
                AppLifecycle::Launching | AppLifecycle::ShuttingDown | AppLifecycle::Failed
            ) | (
                AppLifecycle::Launching,
                AppLifecycle::Running | AppLifecycle::Failed
            ) | (
                AppLifecycle::Running,
                AppLifecycle::ShuttingDown | AppLifecycle::Failed
            ) | (
                AppLifecycle::ShuttingDown,
                AppLifecycle::Stopped | AppLifecycle::Failed
            )
        );
        if valid {
            self.state = next;
            Ok(())
        } else {
            Err(StateError::InvalidTransition {
                from: self.state,
                to: next,
            })
        }
    }
}
impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum LaunchError {
    InvalidSpec(&'static str),
    SecretEnvironment(String),
    AlreadyFinalized,
    Io(io::Error),
}
impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for LaunchError {}
#[derive(Debug, Eq, PartialEq)]
pub enum SteamError {
    InvalidUri,
    Io(String),
}
impl fmt::Display for SteamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for SteamError {}
#[derive(Debug, Eq, PartialEq)]
pub enum StateError {
    InvalidTransition {
        from: AppLifecycle,
        to: AppLifecycle,
    },
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    struct FakeChild {
        waited: bool,
    }
    impl ChildProcess for FakeChild {
        fn wait(&mut self) -> io::Result<ExitStatus> {
            self.waited = true;
            Err(io::Error::other("fake wait"))
        }
        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    struct FakeSpawner;
    impl ProcessSpawner for FakeSpawner {
        fn spawn(&self, spec: &LaunchSpec) -> Result<Box<dyn ChildProcess>, LaunchError> {
            assert_eq!(spec.args, vec!["--safe", "two words"]);
            assert!(spec.cwd.is_absolute());
            assert!(!spec.env.contains_key("API_KEY"));
            Ok(Box::new(FakeChild { waited: false }))
        }
    }
    #[test]
    fn launch_preserves_argv_cwd_and_filters_secrets() {
        let cwd = std::env::temp_dir().join("game");
        let executable = cwd.join("run");
        let spec = LaunchSpec::new(Platform::Linux, executable, cwd)
            .unwrap()
            .arg("--safe")
            .arg("two words")
            .env("VISIBLE", "yes")
            .unwrap();
        let spec = spec.env("API_KEY", "nope");
        assert!(spec.is_err());
        let cwd = std::env::temp_dir().join("game");
        let executable = cwd.join("run");
        let child = launch(
            LaunchSpec::new(Platform::Linux, executable, cwd)
                .unwrap()
                .arg("--safe")
                .arg("two words")
                .env("VISIBLE", "yes")
                .unwrap(),
            &FakeSpawner,
        )
        .unwrap();
        assert!(child.is_running());
    }
    #[test]
    fn finalize_hooks_are_idempotent() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut child = OwnedChild::new(Box::new(FakeChild { waited: false }));
        let c = calls.clone();
        child.on_finalize(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        child.finalize();
        child.finalize();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn process_errors_are_owned_and_returned() {
        let child = OwnedChild::new(Box::new(FakeChild { waited: false }));
        assert!(matches!(child.wait(), Err(LaunchError::Io(_))));
    }
    #[test]
    fn steam_uri_is_strict() {
        assert_eq!(
            SteamUri::parse("steam://run/123?x=y").unwrap().as_str(),
            "steam://run/123?x=y"
        );
        assert!(SteamUri::parse("steam://run/0").is_err());
        assert!(SteamUri::parse("steam://run/12#x").is_err());
    }
    #[test]
    fn lifecycle_transitions_are_guarded() {
        let mut state = AppState::new();
        state.transition(AppLifecycle::Ready).unwrap();
        state.transition(AppLifecycle::Launching).unwrap();
        assert!(state.transition(AppLifecycle::Ready).is_err());
    }

    struct TestRoot(PathBuf);
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn game_fixture() -> (TestRoot, GameRuntimeConfig) {
        let root = env::temp_dir().join(format!(
            "deltamod-game-runtime-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let games = root.join("games");
        let install = root.join("install");
        fs::create_dir_all(&games).unwrap();
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("GAME.exe"), b"fixture").unwrap();
        fs::write(install.join("data.win"), b"fixture").unwrap();
        fs::write(
            games.join("fixture.json"),
            br#"{"name":"Fixture","author":"Test","id":"test.game","gamebanana":{"id":7},"exeName":"GAME.exe","availableFeatures":[]}"#,
        )
        .unwrap();
        let store = root.join("store.json");
        fs::write(
            &store,
            serde_json::to_vec(&json!({
                "gamePid": "test.game",
                "gamePath": install,
                "gamePlatform": "win32",
                "isSteam": false
            }))
            .unwrap(),
        )
        .unwrap();
        (
            TestRoot(root),
            GameRuntimeConfig::new(games, store, HostPlatform::Win32),
        )
    }

    #[derive(Default)]
    struct CaptureSpawner(Mutex<Vec<LaunchSpec>>);
    impl ProcessSpawner for CaptureSpawner {
        fn spawn(&self, spec: &LaunchSpec) -> Result<Box<dyn ChildProcess>, LaunchError> {
            self.0.lock().unwrap().push(spec.clone());
            Ok(Box::new(SlowChild))
        }
    }
    struct SlowChild;
    impl ChildProcess for SlowChild {
        fn wait(&mut self) -> io::Result<ExitStatus> {
            thread::sleep(Duration::from_millis(50));
            Err(io::Error::other("fixture exit"))
        }
        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    struct RejectSteam;
    impl SteamOpener for RejectSteam {
        fn open(&self, _: &SteamUri) -> Result<(), SteamError> {
            Err(SteamError::InvalidUri)
        }
    }

    #[test]
    fn game_query_channels_preserve_legacy_shapes() {
        let (_root, config) = game_fixture();
        let runtime = GameRuntime::new(config);
        let games = runtime.dispatch("getAvailableGames", &[]).unwrap().unwrap();
        assert_eq!(games[0]["id"], "test.game");
        assert_eq!(games[0]["gamebanana"]["id"], 7);
        assert_eq!(
            runtime
                .dispatch("getGameInfo", &[json!("test.game")])
                .unwrap(),
            Some(games[0].clone())
        );
        assert_eq!(
            runtime
                .dispatch("getGameInfo", &[json!("missing")])
                .unwrap(),
            Some(Value::Null)
        );
        assert_eq!(
            runtime.dispatch("getCurrentGameInfo", &[]).unwrap(),
            Some(games[0].clone())
        );
        assert_eq!(
            runtime.dispatch("loadedDeltarune", &[]).unwrap(),
            Some(json!({"loaded": true, "path": "test.game"}))
        );
    }

    #[test]
    fn start_game_uses_exact_executable_argv_and_cwd_and_is_reaped() {
        let (_root, config) = game_fixture();
        let spawner = Arc::new(CaptureSpawner::default());
        let runtime = GameRuntime::with_adapters(
            config.clone(),
            spawner.clone(),
            Arc::new(RejectSteam),
            Arc::new(NoopGameLifecycle),
        );
        assert_eq!(
            runtime.dispatch("startGame", &[json!("ignored")]).unwrap(),
            Some(json!(true))
        );
        let specs = spawner.0.lock().unwrap();
        let cwd = fs::canonicalize(config.store_path.parent().unwrap().join("install")).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].platform, Platform::Windows);
        assert_eq!(specs[0].args, Vec::<String>::new());
        assert_eq!(specs[0].cwd, cwd);
        assert_eq!(specs[0].executable, cwd.join("GAME.exe"));
        drop(specs);
        for _ in 0..20 {
            if !runtime.is_running() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!runtime.is_running());
    }

    #[test]
    fn bounded_noop_and_unsupported_channels_are_explicit() {
        let (_root, config) = game_fixture();
        let runtime = GameRuntime::new(config);
        assert_eq!(
            runtime
                .dispatch("executeArgumentCmd", &[json!("ignored")])
                .unwrap(),
            Some(Value::Null)
        );
        assert_eq!(
            runtime.dispatch("startGameVanilla", &[]),
            Err(GameError::UnsupportedChannel)
        );
        assert_eq!(runtime.dispatch("not-a-game-channel", &[]).unwrap(), None);
    }

    #[test]
    fn inherited_environment_is_an_allowlist() {
        assert!(allowed_inherited_key("PATH"));
        assert!(allowed_inherited_key("LC_ALL"));
        assert!(!allowed_inherited_key("AWS_ACCESS_KEY_ID"));
        assert!(!allowed_inherited_key("RANDOM_APPLICATION_SETTING"));
    }
}
