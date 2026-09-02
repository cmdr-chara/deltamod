#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod channels;
mod controller;
mod error;
mod profile_registry;
mod provider_cache;
mod state;

use base64::Engine;
use deltamod_asset_runtime::{headers, plan_range, Body, Error as AssetError, Range};
use http::{Request, Response, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::Mutex,
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, UriSchemeContext, WebviewWindow};

#[derive(Default)]
struct TaskbarIconState {
    #[cfg(windows)]
    icon: Mutex<Option<deltamod_windows_taskbar_icon::TaskbarIcon>>,
}

#[cfg(any(target_os = "windows", target_os = "android"))]
const MAIN_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(any(target_os = "windows", target_os = "android")))]
const MAIN_ORIGIN: &str = "tauri://localhost";

const SMOKE_DATA_ROOT_ENV: &str = "DELTAMOD_SMOKE_DATA_ROOT";
const SMOKE_CAPABILITY_FILE_ENV: &str = "DELTAMOD_SMOKE_CAPABILITY_FILE";
const SMOKE_CAPABILITY_FILE: &str = ".deltamod-capability-evidence.json";
const SMOKE_PROTOCOL_FILE_ENV: &str = "DELTAMOD_SMOKE_PROTOCOL_FILE";
const SMOKE_PROTOCOL_FILE: &str = ".deltamod-protocol-evidence.json";
const SMOKE_PROTOCOL_QUEUE_FILE: &str = ".deltamod-protocol-queue-evidence.json";

fn configured_data_dir<I>(
    default: PathBuf,
    args: I,
    expected_override: Option<OsString>,
) -> Result<PathBuf, &'static str>
where
    I: IntoIterator<Item = OsString>,
{
    let mut selected: Option<PathBuf> = None;
    let mut arguments = args.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--data-root") {
            let value = arguments.next().ok_or("data root argument missing")?;
            if selected.replace(PathBuf::from(value)).is_some() {
                return Err("duplicate data root argument");
            }
            continue;
        }
        if let Some(argument) = argument.to_str() {
            if let Some(value) = argument.strip_prefix("--data-root=") {
                if value.is_empty() {
                    return Err("data root argument missing");
                }
                if selected.replace(PathBuf::from(value)).is_some() {
                    return Err("duplicate data root argument");
                }
            }
        }
    }

    let Some(selected) = selected else {
        return Ok(default);
    };
    let expected = expected_override.ok_or("data root override unavailable")?;
    let expected = PathBuf::from(expected);
    if !selected.is_absolute() || !expected.is_absolute() {
        return Err("data root override must be absolute");
    }
    let selected = selected
        .canonicalize()
        .map_err(|_| "data root override unavailable")?;
    let expected = expected
        .canonicalize()
        .map_err(|_| "data root override unavailable")?;
    if selected != expected {
        return Err("data root override mismatch");
    }
    let metadata = selected
        .symlink_metadata()
        .map_err(|_| "data root override unavailable")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("data root override must be a directory");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("data root override cannot be a reparse point");
        }
    }
    Ok(selected)
}

fn smoke_capability_path(state: &state::AppState) -> Result<Option<PathBuf>, String> {
    let Some(raw) = std::env::var_os(SMOKE_CAPABILITY_FILE_ENV) else {
        return Ok(None);
    };
    let expected_root = std::env::var_os(SMOKE_DATA_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| "TAURI_INVALID_SMOKE_CAPABILITY_PATH".to_owned())?;
    validate_smoke_evidence_path(
        &state.data_root.root,
        &expected_root,
        &PathBuf::from(raw),
        SMOKE_CAPABILITY_FILE,
    )
    .map(Some)
}

fn smoke_protocol_path(state: &state::AppState) -> Result<Option<PathBuf>, String> {
    let Some(raw) = std::env::var_os(SMOKE_PROTOCOL_FILE_ENV) else {
        return Ok(None);
    };
    let expected_root = std::env::var_os(SMOKE_DATA_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| "TAURI_INVALID_SMOKE_PROTOCOL_PATH".to_owned())?;
    validate_smoke_evidence_path(
        &state.data_root.root,
        &expected_root,
        &PathBuf::from(raw),
        SMOKE_PROTOCOL_FILE,
    )
    .map_err(|_| "TAURI_INVALID_SMOKE_PROTOCOL_PATH".to_owned())
    .map(Some)
}

fn smoke_protocol_queue_path(state: &state::AppState) -> Result<Option<PathBuf>, String> {
    let Some(protocol_path) = smoke_protocol_path(state)? else {
        return Ok(None);
    };
    let queue_path = protocol_path.with_file_name(SMOKE_PROTOCOL_QUEUE_FILE);
    let expected_root = std::env::var_os(SMOKE_DATA_ROOT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| "TAURI_INVALID_SMOKE_PROTOCOL_PATH".to_owned())?;
    validate_smoke_evidence_path(
        &state.data_root.root,
        &expected_root,
        &queue_path,
        SMOKE_PROTOCOL_QUEUE_FILE,
    )
    .map_err(|_| "TAURI_INVALID_SMOKE_PROTOCOL_PATH".to_owned())
    .map(Some)
}

fn validate_smoke_evidence_path(
    data_root: &Path,
    expected_root: &Path,
    candidate: &Path,
    file_name: &str,
) -> Result<PathBuf, String> {
    if candidate.file_name() != Some(OsStr::new(file_name)) {
        return Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into());
    }
    let parent = candidate
        .parent()
        .ok_or_else(|| "TAURI_INVALID_SMOKE_CAPABILITY_PATH".to_owned())?;
    let root = data_root
        .canonicalize()
        .map_err(|_| "TAURI_INVALID_SMOKE_CAPABILITY_PATH".to_owned())?;
    let expected_root = expected_root
        .canonicalize()
        .map_err(|_| "TAURI_INVALID_SMOKE_CAPABILITY_PATH".to_owned())?;
    let parent = parent
        .canonicalize()
        .map_err(|_| "TAURI_INVALID_SMOKE_CAPABILITY_PATH".to_owned())?;
    let contains_relative_component = candidate.components().any(|component| {
        matches!(
            component,
            std::path::Component::CurDir | std::path::Component::ParentDir
        )
    });
    if expected_root != root || parent != root || contains_relative_component || candidate.exists()
    {
        return Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into());
    }
    let metadata = fs::symlink_metadata(&root)
        .map_err(|_| "TAURI_INVALID_SMOKE_CAPABILITY_PATH".to_owned())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into());
        }
    }
    Ok(candidate.to_path_buf())
}

fn write_smoke_capability_evidence(app: &AppHandle, state: &state::AppState) -> Result<(), String> {
    let Some(path) = smoke_capability_path(state)? else {
        return Ok(());
    };
    let probe = (|| -> Result<Value, String> {
        let fixture_game = state.data_root.root.join("smoke-fixture-game");
        fs::create_dir(&fixture_game).map_err(|_| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?;
        #[cfg(target_os = "windows")]
        for relative in ["UNDERTALE.exe", "data.win"] {
            fs::write(
                fixture_game.join(relative),
                b"deltamod packaged smoke fixture\n",
            )
            .map_err(|_| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?;
        }
        #[cfg(target_os = "linux")]
        for relative in ["run.sh", "assets/game.unx"] {
            let target = fixture_game.join(relative);
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?,
            )
            .map_err(|_| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?;
            fs::write(target, b"deltamod packaged smoke fixture\n")
                .map_err(|_| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?;
        }
        #[cfg(target_os = "macos")]
        for relative in [
            "UNDERTALE.app/Contents/MacOS/Mac_Runner",
            "UNDERTALE.app/Contents/Resources/game.ios",
        ] {
            let target = fixture_game.join(relative);
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?,
            )
            .map_err(|_| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?;
            fs::write(target, b"deltamod packaged smoke fixture\n")
                .map_err(|_| "SMOKE_FIXTURE_CREATE_FAILED".to_owned())?;
        }
        let existing = state
            .profile_runtime
            .legacy_installations()
            .map_err(|_| "SMOKE_INSTALLATION_LIST_FAILED".to_owned())?;
        if existing.as_array().is_none_or(Vec::is_empty) {
            let mut store = serde_json::Map::new();
            store.insert("gamePid".into(), json!("toby.undertale"));
            store.insert("loadedDeltarune".into(), json!(true));
            state
                .profile_runtime
                .legacy_create_installation(0, &fixture_game, "Smoke fixture".into(), false, store)
                .map_err(|_| "SMOKE_INSTALLATION_CREATE_FAILED".to_owned())?;
        }

        let set_flag = dispatch_domain(
            app,
            state,
            "setUniqueFlag",
            &[json!("PACKAGED_SMOKE"), json!(true)],
        )?;
        let get_flag = dispatch_domain(app, state, "getUniqueFlag", &[json!("PACKAGED_SMOKE")])?;
        let persisted: state::Preferences = serde_json::from_slice(
            &fs::read(state.data_root.root.join("preferences.json"))
                .map_err(|_| "SMOKE_PERSISTENCE_READ_FAILED".to_owned())?,
        )
        .map_err(|_| "SMOKE_PERSISTENCE_PARSE_FAILED".to_owned())?;
        let themes = dispatch_domain(app, state, "getThemes", &[])?;
        let theme_ids = themes
            .as_array()
            .ok_or_else(|| "SMOKE_THEME_LIST_INVALID".to_owned())?
            .iter()
            .filter_map(|theme| theme.get("id").and_then(Value::as_str))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let active_theme = dispatch_domain(app, state, "getTheme", &[])?;
        let installations = dispatch_domain(app, state, "getInstallations", &[])?;
        let loaded_game = dispatch_domain(app, state, "loadedDeltarune", &[])?;
        let installation_count = installations
            .as_array()
            .ok_or_else(|| "SMOKE_INSTALLATION_LIST_INVALID".to_owned())?
            .len();
        let unknown_channel_error = match BackendChannel::from_str("smoke:unknown") {
            Err(()) => error::unavailable("unknown"),
            Ok(_) => return Err("SMOKE_UNKNOWN_CHANNEL_ACCEPTED".into()),
        };
        let checks = json!({
            "packaged": !cfg!(debug_assertions),
            "flagSet": set_flag == json!(true),
            "flagRead": get_flag == json!(true),
            "flagPersisted": persisted.unique_flags.get("PACKAGED_SMOKE") == Some(&true),
            "baseThemeAvailable": theme_ids.iter().any(|id| id == "base"),
            "baseThemeActive": active_theme == json!("base"),
            "installationListed": installation_count >= 1,
            "gameLoaded": loaded_game.get("loaded").and_then(Value::as_bool) == Some(true),
            "unknownChannelRejected": unknown_channel_error == "TAURI_COMMAND_UNAVAILABLE:unknown"
        });
        if checks
            .as_object()
            .is_none_or(|values| values.values().any(|value| value != &json!(true)))
        {
            return Err("SMOKE_CAPABILITY_CHECK_FAILED".into());
        }
        Ok(json!({
            "schemaVersion": 1,
            "status": "passed",
            "ok": true,
            "packageVersion": app.package_info().version.to_string(),
            "platform": platform_name(),
            "diagnostic": format!(
                "Deltamod Community {} - Tauri shell on {} - packaged mode",
                app.package_info().version,
                platform_name()
            ),
            "checks": checks,
            "themeCount": theme_ids.len(),
            "installationCount": installation_count,
            "unknownChannelError": unknown_channel_error
        }))
    })();
    let evidence = match probe {
        Ok(evidence) => evidence,
        Err(code) => json!({
            "schemaVersion": 1,
            "status": "failed",
            "ok": false,
            "error": code.chars().take(128).collect::<String>()
        }),
    };
    deltamod_storage_domain::save_json(&path, &evidence, false)
        .map_err(|_| "SMOKE_CAPABILITY_WRITE_FAILED".to_owned())
}

pub(crate) fn write_protocol_smoke_evidence(
    app: &AppHandle,
    item_id: u32,
    operation_id: u64,
    renderer_generation: u64,
) -> Result<(), String> {
    let state = app
        .try_state::<state::AppState>()
        .ok_or_else(|| "SMOKE_PROTOCOL_STATE_UNAVAILABLE".to_owned())?;
    let Some(path) = smoke_protocol_path(&state)? else {
        return Ok(());
    };
    let evidence = json!({
        "schemaVersion": 1,
        "status": "passed",
        "ok": true,
        "packageVersion": app.package_info().version.to_string(),
        "processId": std::process::id(),
        "action": "launch",
        "itemId": item_id,
        "operationId": operation_id,
        "rendererGeneration": renderer_generation,
        "checks": {
            "rendererReady": true,
            "singleInstanceWorker": true,
            "strictProtocolAction": true
        }
    });
    deltamod_storage_domain::save_json(&path, &evidence, false)
        .map_err(|_| "SMOKE_PROTOCOL_WRITE_FAILED".to_owned())
}

pub(crate) fn write_protocol_queue_smoke_evidence(
    app: &AppHandle,
    item_id: u32,
    renderer_ready: bool,
) -> Result<(), String> {
    let state = app
        .try_state::<state::AppState>()
        .ok_or_else(|| "SMOKE_PROTOCOL_STATE_UNAVAILABLE".to_owned())?;
    let Some(path) = smoke_protocol_queue_path(&state)? else {
        return Ok(());
    };
    let evidence = json!({
        "schemaVersion": 1,
        "status": "queued",
        "ok": true,
        "processId": std::process::id(),
        "action": "launch",
        "itemId": item_id,
        "rendererReadyAtQueue": renderer_ready
    });
    deltamod_storage_domain::save_json(&path, &evidence, false)
        .map_err(|_| "SMOKE_PROTOCOL_QUEUE_WRITE_FAILED".to_owned())
}

fn protocol_error(error: AssetError) -> StatusCode {
    match error {
        AssetError::NotFound => StatusCode::NOT_FOUND,
        AssetError::Io => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::FORBIDDEN,
    }
}

fn empty_response(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("content-length", "0")
        .body(Vec::new())
        .expect("static response headers are valid")
}

fn normalize_asset_uri(uri: &str, scheme: &str) -> Option<String> {
    let internal_scheme = scheme.trim_end_matches("prot");
    if uri.starts_with(scheme) {
        return Some(uri.replacen(scheme, internal_scheme, 1));
    }
    let path = uri.strip_prefix(&format!("http://{scheme}.localhost/"))?;
    let (host, path) = path.split_once('/')?;
    Some(format!("{internal_scheme}://{host}/{path}"))
}

fn protocol_origin_allowed(request: &Request<Vec<u8>>) -> bool {
    request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| origin == MAIN_ORIGIN)
}

fn serve_asset<R: tauri::Runtime>(
    context: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    scheme: &'static str,
) -> Response<Vec<u8>> {
    if context.webview_label() != "main" || !protocol_origin_allowed(&request) {
        return empty_response(StatusCode::FORBIDDEN);
    }
    let Some(raw) = normalize_asset_uri(&request.uri().to_string(), scheme) else {
        return empty_response(StatusCode::FORBIDDEN);
    };
    let state = context.app_handle().state::<state::AppState>();
    let plan = match state.assets.resolve(&raw) {
        Ok(plan) => plan,
        Err(error) => return empty_response(protocol_error(error)),
    };
    let range_header = request.headers().get("range").and_then(|v| v.to_str().ok());
    let response_headers = match headers(&plan, range_header) {
        Ok(value) => value,
        Err(error) => return empty_response(protocol_error(error)),
    };
    if response_headers.status == 416 {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header("content-length", "0")
            .header(
                "content-range",
                response_headers.content_range.unwrap_or_default(),
            )
            .body(Vec::new())
            .expect("static response headers are valid");
    }
    let file = match state.assets.open(&plan) {
        Ok(file) => file,
        Err(error) => return empty_response(protocol_error(error)),
    };
    let range = match plan_range(range_header, plan.length) {
        Ok(range @ (Range::Full | Range::Partial { .. })) => range,
        _ => return empty_response(StatusCode::RANGE_NOT_SATISFIABLE),
    };
    let mut body = match Body::new(file, range, plan.length) {
        Ok(body) => body,
        Err(error) => return empty_response(protocol_error(error)),
    };
    let mut bytes = Vec::new();
    if body
        .by_ref()
        .take(response_headers.content_length)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return empty_response(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let mut builder = Response::builder()
        .status(StatusCode::from_u16(response_headers.status).unwrap_or(StatusCode::OK))
        .header("content-type", response_headers.content_type)
        .header("content-length", response_headers.content_length)
        .header("accept-ranges", response_headers.accept_ranges)
        .header("access-control-allow-origin", MAIN_ORIGIN);
    if let Some(value) = response_headers.content_range {
        builder = builder.header("content-range", value);
    }
    builder
        .body(bytes)
        .expect("static response headers are valid")
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BackendChannel {
    DiagnosticInfo,
    InstallerMode,
    InstallerInfo,
    InstallerInstall,
    InstallerLaunch,
    InstallerMinimize,
    InstallerQuit,
    GetOs,
    IsDevMode,
    IsPackaged,
    Log,
    LoginGamebanana,
    Minimize,
    Quit,
    Restart,
    IsControllerMode,
    ControllerModeOn,
    ControllerModeOff,
    SetAppIcon,
    ShakeEasterEggWindow,
    ShowWindow,
    ToggleFullscreen,
    Version,
    Implemented(String),
    Unsupported(String),
}

impl FromStr for BackendChannel {
    type Err = ();
    fn from_str(channel: &str) -> Result<Self, Self::Err> {
        let known = match channel {
            "diagnosticInfo" => Self::DiagnosticInfo,
            "isInstallerMode" => Self::InstallerMode,
            "installerInfo" => Self::InstallerInfo,
            "installerInstall" => Self::InstallerInstall,
            "installerLaunch" => Self::InstallerLaunch,
            "installerMinimize" => Self::InstallerMinimize,
            "installerQuit" => Self::InstallerQuit,
            "getOS" => Self::GetOs,
            "isDevMode" => Self::IsDevMode,
            "isPackaged" => Self::IsPackaged,
            "log" => Self::Log,
            "loginGamebanana" => Self::LoginGamebanana,
            "minimizeMe" => Self::Minimize,
            "quitCommunityForEasterEgg" => Self::Quit,
            "restartCommunity" => Self::Restart,
            "isCMode" => Self::IsControllerMode,
            "cmode-on" => Self::ControllerModeOn,
            "cmode-off" => Self::ControllerModeOff,
            "setAppIcon" => Self::SetAppIcon,
            "shakeCommunityWindowForEasterEgg" => Self::ShakeEasterEggWindow,
            "showWindow" => Self::ShowWindow,
            "toggleFullscreen" => Self::ToggleFullscreen,
            "version" => Self::Version,
            "getUniqueFlag"
            | "setUniqueFlag"
            | "benchmark:rendererReady"
            | "storage:getUsage"
            | "storage:clearCache"
            | "storage:deleteRecoveryData"
            | "htmlAlert_outwin"
            | "isBaked"
            | "modSources:getProviders"
            | "modSources:browse"
            | "modSources:nexusStatus"
            | "modSources:validateUrl"
            | "modSources:startNexusSso"
            | "modSources:cancelNexusSso"
            | "modSources:clearNexusKey"
            | "modSources:open"
            | "modSources:downloadNexus"
            | "getInstallations"
            | "getSystemIndex"
            | "getMaxExistingIndex"
            | "isCurrentIndexSteam"
            | "removeSteamIntegration"
            | "protocol:parseDeepLink"
            | "protocol:planRange"
            | "protocol:queueDeepLink"
            | "protocol:rendererReady"
            | "getModList"
            | "getModListFull"
            | "lifecycle:getInstalledMods"
            | "lifecycle:getOperationStatus"
            | "lifecycle:listProfiles"
            | "lifecycle:importProfileLockfile"
            | "lifecycle:exportProfileLockfile"
            | "lifecycle:createProfileFromCurrent"
            | "lifecycle:getActiveProfile"
            | "lifecycle:switchProfile"
            | "lifecycle:updateMod"
            | "lifecycle:verifyMod"
            | "lifecycle:repairMod"
            | "lifecycle:restoreLastWorkingState"
            | "lifecycle:uninstallMod"
            | "howManyMods"
            | "getModState"
            | "toggleModState"
            | "setModVariant"
            | "removeMod"
            | "getThemes"
            | "getTheme"
            | "setTheme"
            | "renameCustomTheme"
            | "deleteCustomTheme"
            | "getSponsor"
            | "fetchSharedVariable"
            | "getOfficialProfileSummary"
            | "cancelOfficialProfileImport"
            | "getModImage"
            | "logoutGamebanana"
            | "eraseGamebananaCache"
            | "openCommunityMaintainerProfile"
            | "openSysFolder"
            | "openModFolder"
            | "validateGamebananaToken"
            | "getGamebananaPic"
            | "getGamebananaID"
            | "getGamebananaUserinfo"
            | "startGame"
            | "getCurrentGameInfo"
            | "getGameInfo"
            | "getAvailableGames"
            | "loadedDeltarune"
            | "leaveCommentGamebanana"
            | "gbLikeMod"
            | "gamebanana_getCollections"
            | "gamebanana_createCollection"
            | "gamebanana_deleteCollection"
            | "gamebanana_importToCollection"
            | "chooseTheme"
            | "importOfficialProfile"
            | "undertaleModTool:choose"
            | "browseFile"
            | "locateDelta"
            | "importTheme"
            | "createNewInstallation"
            | "repairInstallation"
            | "reimportInstallation"
            | "deleteSystemIndex"
            | "setInstallationCName"
            | "changeSystemIndex"
            | "openInstallationFolder"
            | "downloadGame"
            | "cancelGameImport"
            | "patchAndRun"
            | "precalcGameHashes"
            | "importMod"
            | "shouldGoIM"
            | "getEditionByIndex"
            | "executeArgumentCmd"
            | "installDeltamodCLI"
            | "openFlagDatabase"
            | "dlmodURL" => Self::Implemented(channel.to_owned()),
            "undertaleModTool:status"
            | "fireUpdate"
            | "start-update"
            | "ignore-update"
            | "updater-status" => Self::Implemented(channel.to_owned()),
            "rebootDev"
            | "createInstallLink"
            | "undertaleModTool:openInstallation"
            | "gamebanana_downloadAllInCollection"
            | "npsCallback"
            | "initialize" => Self::Unsupported(channel.to_owned()),
            _ => return Err(()),
        };
        Ok(known)
    }
}

fn platform_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

fn is_installer_mode() -> bool {
    option_env!("DELTAMOD_INSTALLER_MODE") == Some("1")
        || std::env::args().any(|arg| arg == "--installer")
}

fn installer_asset_url() -> String {
    option_env!("DELTAMOD_INSTALLER_ASSET_URL")
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "https://github.com/cmdr-chara/deltamod/releases/download/community-v{version}/Deltamod.Community_{version}_x64-setup.exe",
                version = env!("CARGO_PKG_VERSION")
            )
        })
}

fn installer_asset_sha256() -> Option<&'static str> {
    option_env!("DELTAMOD_INSTALLER_SHA256").filter(|value| !value.trim().is_empty())
}

fn default_installer_directory(app: &AppHandle) -> PathBuf {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("Deltamod Community"))
        .unwrap_or_else(|_| std::env::temp_dir().join("Deltamod Community"))
}

fn validate_installer_directory(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err("Choose an installation folder.".to_owned());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("The installation folder must be an absolute local path.".to_owned());
    }
    if path.parent().is_none() {
        return Err("The installation folder cannot be a drive root.".to_owned());
    }
    Ok(path)
}

fn emit_installer_progress(app: &AppHandle, progress: f64, phase: &str, detail: &str) {
    let _ = app.emit(
        "installer-progress",
        json!({
            "progress": progress.clamp(0.0, 1.0),
            "phase": phase,
            "detail": detail,
        }),
    );
}

fn run_silent_installer(installer: &Path) -> Result<i32, String> {
    let mut command = Command::new(installer);
    command.arg("/S");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let status = command
        .status()
        .map_err(|_| "Deltamod could not start the installation engine.".to_owned())?;
    Ok(status.code().unwrap_or(1))
}

fn copy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(source).map_err(|_| {
        "The installed package could not be located in its staging folder.".to_owned()
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|_| "The setup could not read the staged installation files.".to_owned())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|_| "The setup could not inspect the staged installation files.".to_owned())?;
        if file_type.is_dir() {
            std::fs::create_dir_all(&destination_path).map_err(|_| {
                "The setup could not create the selected installation folder.".to_owned()
            })?;
            copy_directory_contents(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&source_path, &destination_path)
                .map_err(|_| "The setup could not copy the installation files.".to_owned())?;
        }
    }
    Ok(())
}

fn launch_installed_application(raw_directory: &str) -> Result<(), String> {
    let install_dir = validate_installer_directory(raw_directory)?;
    let candidates = [
        install_dir.join("deltamod-tauri-shell.exe"),
        install_dir.join("Deltamod Community.exe"),
        install_dir.join("Deltamod.exe"),
    ];
    let executable = candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "Deltamod Community was installed, but its launcher was not found.".to_owned()
        })?;
    let mut command = Command::new(executable);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .spawn()
        .map_err(|_| "Deltamod Community could not be launched.".to_owned())?;
    Ok(())
}

async fn download_and_install(app: AppHandle, raw_directory: String) -> Result<Value, String> {
    let install_dir = validate_installer_directory(&raw_directory)?;
    std::fs::create_dir_all(&install_dir)
        .map_err(|_| "The setup could not create the selected installation folder.".to_owned())?;
    let url = installer_asset_url();
    if !url.starts_with("https://github.com/") {
        return Err("The setup download source is not trusted.".to_owned());
    }

    emit_installer_progress(
        &app,
        0.08,
        "Connecting",
        "Contacting the Deltamod release server",
    );
    let client = reqwest::Client::builder()
        .user_agent("Deltamod Community Setup")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|_| "The setup could not initialize its secure download client.".to_owned())?;
    emit_installer_progress(
        &app,
        0.09,
        "Connecting",
        "Secure channel ready; waiting for the release server",
    );
    let response =
        match tokio::time::timeout(Duration::from_secs(20), client.get(&url).send()).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                return Err("The Deltamod setup could not reach the release server.".to_owned());
            }
            Err(_) => {
                return Err(
                "The release server took too long to respond. Check your connection and try again."
                    .to_owned(),
            );
            }
        };
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(
            "The stable Deltamod release package is not available yet. Try again after the release is published."
                .to_owned(),
        );
    }
    if !response.status().is_success() {
        return Err(format!(
            "The Deltamod release server returned HTTP {}.",
            response.status()
        ));
    }
    emit_installer_progress(
        &app,
        0.10,
        "Downloading",
        "Release package found; starting transfer",
    );
    let total = response.content_length().unwrap_or(0);
    if total > 1_024 * 1_024 * 1_024 {
        return Err("The setup package is larger than the supported limit.".to_owned());
    }

    let temporary = std::env::temp_dir().join(format!(
        "deltamod-community-setup-{}.exe",
        std::process::id()
    ));
    let mut file = File::create(&temporary)
        .map_err(|_| "The setup could not create its temporary download.".to_owned())?;
    let mut digest = Sha256::new();
    let mut downloaded = 0_u64;
    let mut stream = response;
    while let Some(chunk) = stream
        .chunk()
        .await
        .map_err(|_| "The setup download was interrupted.".to_owned())?
    {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > 1_024 * 1_024 * 1_024 {
            let _ = std::fs::remove_file(&temporary);
            return Err("The setup package is larger than the supported limit.".to_owned());
        }
        file.write_all(&chunk)
            .map_err(|_| "The setup could not save the downloaded package.".to_owned())?;
        digest.update(&chunk);
        let download_progress = if total == 0 {
            0.42
        } else {
            0.10 + (downloaded as f64 / total as f64) * 0.38
        };
        emit_installer_progress(
            &app,
            download_progress,
            "Downloading",
            &format!("{} MB received", downloaded / (1024 * 1024)),
        );
    }
    file.flush()
        .map_err(|_| "The setup could not finish saving the package.".to_owned())?;
    drop(file);

    if let Some(expected) = installer_asset_sha256() {
        let actual = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if expected.len() != 64
            || !expected
                .chars()
                .all(|character| character.is_ascii_hexdigit())
            || !actual.eq_ignore_ascii_case(expected)
        {
            let _ = std::fs::remove_file(&temporary);
            return Err("The downloaded setup failed its integrity check.".to_owned());
        }
    }

    emit_installer_progress(
        &app,
        0.55,
        "Installing",
        "Applying Deltamod Community to the selected folder",
    );
    let installer_path = temporary.clone();
    let exit_code =
        tauri::async_runtime::spawn_blocking(move || run_silent_installer(&installer_path))
            .await
            .map_err(|_| "The installation engine stopped unexpectedly.".to_owned())??;
    let _ = std::fs::remove_file(&temporary);

    if exit_code != 0 {
        return Err(format!(
            "The installation engine exited with code {exit_code}."
        ));
    }
    let staging_dir = default_installer_directory(&app);
    if install_dir != staging_dir {
        emit_installer_progress(
            &app,
            0.78,
            "Finalizing",
            "Moving Deltamod Community to the selected folder",
        );
        copy_directory_contents(&staging_dir, &install_dir)?;
    }
    if !install_dir.join("deltamod-tauri-shell.exe").is_file() {
        return Err(
            "The installation engine finished, but the Deltamod launcher was not found in the selected folder."
                .to_owned(),
        );
    }
    emit_installer_progress(
        &app,
        1.0,
        "Ready",
        "Deltamod Community is installed and ready to launch",
    );
    Ok(Value::Null)
}

fn os_details() -> Value {
    let info = os_info::get();
    json!({"platform": platform_name(), "release": info.version().to_string(), "version": info.to_string()})
}

fn schedule_exit(app: AppHandle, restart: bool) {
    controller::protocol_shutdown(&app);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        if restart {
            app.restart();
        } else {
            app.exit(0);
        }
    });
}

fn set_app_icon(_app: &AppHandle, window: &WebviewWindow, data: &[Value]) -> Result<Value, String> {
    const PREFIX: &str = "data:image/png;base64,";
    const MAX_ENCODED_BYTES: usize = 96 * 1024;
    let encoded = data
        .first()
        .and_then(Value::as_str)
        .and_then(|value| value.strip_prefix(PREFIX))
        .filter(|value| !value.is_empty() && value.len() <= MAX_ENCODED_BYTES)
        .ok_or_else(|| error::invalid("setAppIcon"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| error::invalid("setAppIcon"))?;
    if bytes.len() < 24 || bytes[..8] != *b"\x89PNG\r\n\x1a\n" {
        return Err(error::invalid("setAppIcon"));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().map_err(|_| error::internal())?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().map_err(|_| error::internal())?);
    if !(16..=512).contains(&width) || !(16..=512).contains(&height) {
        return Err(error::invalid("setAppIcon"));
    }
    let icon = tauri::image::Image::from_bytes(&bytes).map_err(|_| error::invalid("setAppIcon"))?;
    window
        .set_icon(icon.clone())
        .map_err(|_| error::internal())?;
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|_| error::internal())?.0 as isize;
        let taskbar_icon = deltamod_windows_taskbar_icon::TaskbarIcon::set_for_window(
            hwnd,
            icon.rgba(),
            icon.width(),
            icon.height(),
        )
        .map_err(|_| error::internal())?;
        let icon_state = _app.state::<TaskbarIconState>();
        let mut current = icon_state.icon.lock().map_err(|_| error::internal())?;
        *current = Some(taskbar_icon);
    }
    Ok(Value::Null)
}

fn dispatch(
    app: &AppHandle,
    window: &WebviewWindow,
    channel: BackendChannel,
    data: &[Value],
) -> Result<Value, String> {
    let state = app.state::<state::AppState>();
    match channel {
        BackendChannel::Version => Ok(json!(app.package_info().version.to_string())),
        BackendChannel::GetOs => Ok(os_details()),
        BackendChannel::IsPackaged => Ok(json!(!cfg!(debug_assertions))),
        BackendChannel::IsDevMode => Ok(json!(cfg!(debug_assertions))),
        BackendChannel::DiagnosticInfo => Ok(json!(format!(
            "Deltamod Community {} - Tauri shell on {} - {} mode",
            app.package_info().version,
            platform_name(),
            if cfg!(debug_assertions) {
                "development"
            } else {
                "packaged"
            }
        ))),
        BackendChannel::InstallerMode
        | BackendChannel::InstallerInfo
        | BackendChannel::InstallerInstall
        | BackendChannel::InstallerLaunch
        | BackendChannel::InstallerMinimize
        | BackendChannel::InstallerQuit => Err(error::unavailable("installer")),
        BackendChannel::Minimize => window
            .minimize()
            .map(|_| Value::Null)
            .map_err(|_| error::internal()),
        BackendChannel::ToggleFullscreen => {
            let full = window.is_fullscreen().map_err(|_| error::internal())?;
            window
                .set_fullscreen(!full)
                .map(|_| Value::Null)
                .map_err(|_| error::internal())
        }
        BackendChannel::ShowWindow => window
            .show()
            .map(|_| Value::Null)
            .map_err(|_| error::internal()),
        BackendChannel::Restart => {
            schedule_exit(app.clone(), true);
            Ok(Value::Null)
        }
        BackendChannel::IsControllerMode => {
            Ok(json!(app.state::<controller::ControllerMode>().enabled()))
        }
        BackendChannel::ControllerModeOn => {
            controller::relaunch(app, true)?;
            Ok(Value::Null)
        }
        BackendChannel::ControllerModeOff => {
            controller::relaunch(app, false)?;
            Ok(Value::Null)
        }
        BackendChannel::SetAppIcon => set_app_icon(app, window, data),
        BackendChannel::ShakeEasterEggWindow => shake_easter_egg_window(window, &state, data),
        BackendChannel::Quit => {
            schedule_exit(app.clone(), false);
            Ok(json!({"closing":true}))
        }
        BackendChannel::Log => {
            let message = data
                .first()
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(2048)
                .collect::<String>();
            let level = data
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or("LOG")
                .chars()
                .take(32)
                .collect::<String>();
            eprintln!("[{level}] [renderer] {message}");
            Ok(Value::Null)
        }
        BackendChannel::LoginGamebanana => Err(error::internal()),
        BackendChannel::Implemented(name) => {
            let value = dispatch_domain(app, &state, &name, data)?;
            emit_runtime_events(app, &state);
            Ok(value)
        }
        BackendChannel::Unsupported(name) => Err(error::unavailable(&name)),
    }
}

fn dispatch_domain(
    app: &AppHandle,
    state: &state::AppState,
    name: &str,
    data: &[Value],
) -> Result<Value, String> {
    let dialogs = deltamod_tauri_os_adapters::tauri_adapter::TauriDialogBackend::new(app);
    if let Some(value) = channels::dialogs::dispatch(app, state, &dialogs, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::nexus_download::dispatch(app, state, &dialogs, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::import_download::dispatch(app, state, &dialogs, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::patching::dispatch(app, state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::workflows::dispatch(app, state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::game::dispatch(state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::auth::dispatch(state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::updater::dispatch(state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::lifecycle::dispatch(state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::runtime::dispatch(state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::system::dispatch(app, state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::installations::dispatch(state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::mods::dispatch(app, state, name, data)? {
        return Ok(value);
    }
    if let Some(value) = channels::protocol::dispatch(state, name, data)? {
        return Ok(value);
    }
    Err(error::unavailable(name))
}

fn renderer_event(
    event: deltamod_mods_themes_runtime::EventIntent,
) -> Option<(&'static str, Value)> {
    use deltamod_mods_themes_runtime::EventIntent;
    match event {
        EventIntent::ModsChanged => Some(("refresh", Value::Null)),
        EventIntent::ModStateChanged { .. } => None,
        EventIntent::ThemesChanged | EventIntent::ActiveThemeChanged { .. } => {
            Some(("themeChange", Value::Null))
        }
        EventIntent::PreferencesChanged
        | EventIntent::SharedChanged
        | EventIntent::SponsorsChanged => Some(("refresh", Value::Null)),
    }
}

fn emit_runtime_events(app: &AppHandle, state: &state::AppState) {
    for event in state.mods_themes.drain_events() {
        if let Some((name, payload)) = renderer_event(event) {
            let _ = app.emit(name, payload);
        }
    }
}

fn shake_easter_egg_window(
    window: &WebviewWindow,
    state: &state::AppState,
    data: &[Value],
) -> Result<Value, String> {
    let phase = data
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| error::invalid("shakeCommunityWindowForEasterEgg"))?;
    let native = !(cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE")
                .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))));
    let config: Option<(u64, &'static [(i32, i32)])> = match phase {
        "stop" => None,
        "slash" => Some((46, &[(-10, 0), (8, 0), (-7, 1), (11, -1)])),
        "numbers" => Some((
            38,
            &[(-15, 0), (13, 1), (-11, -1), (16, 0), (-8, 1), (10, -1)],
        )),
        _ => return Err(error::invalid("shakeCommunityWindowForEasterEgg")),
    };
    let mut shake = state
        .easter_egg_window
        .lock()
        .map_err(|_| error::internal())?;
    shake.generation = shake.generation.wrapping_add(1);
    let generation = shake.generation;
    let Some((interval_ms, offsets)) = config else {
        let origin = shake.origin.take();
        drop(shake);
        let restored = origin.is_some_and(|(x, y)| {
            window
                .set_position(tauri::PhysicalPosition::new(x, y))
                .is_ok()
        });
        return Ok(json!({ "phase": phase, "native": native, "restored": restored }));
    };
    if !native {
        shake.origin = None;
        return Ok(json!({ "phase": phase, "native": false }));
    }
    if shake.origin.is_none() {
        let position = window.outer_position().map_err(|_| error::internal())?;
        shake.origin = Some((position.x, position.y));
    }
    let origin = shake.origin.ok_or_else(error::internal)?;
    drop(shake);
    let window = window.clone();
    let app_state = state.easter_egg_window.clone();
    // The bounded worker self-terminates after the encounter's longest visual
    // phase even if renderer cleanup is lost. A later phase invalidates it by
    // generation before the next position write.
    thread::spawn(move || {
        for tick in 0..260usize {
            let Ok(state) = app_state.lock() else {
                return;
            };
            if state.generation != generation || state.origin != Some(origin) {
                return;
            }
            let (x, y) = offsets[tick % offsets.len()];
            if window
                .set_position(tauri::PhysicalPosition::new(origin.0 + x, origin.1 + y))
                .is_err()
            {
                return;
            }
            drop(state);
            thread::sleep(Duration::from_millis(interval_ms));
        }
        if let Ok(mut state) = app_state.lock() {
            if state.generation == generation && state.origin == Some(origin) {
                state.generation = state.generation.wrapping_add(1);
                state.origin = None;
                let _ = window.set_position(tauri::PhysicalPosition::new(origin.0, origin.1));
            }
        }
    });
    Ok(json!({ "phase": phase, "native": true }))
}

#[tauri::command]
async fn backend_invoke(
    app: AppHandle,
    window: WebviewWindow,
    channel: String,
    data: Option<Value>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("TAURI_WINDOW_NOT_ALLOWED".to_owned());
    }
    if channel.len() > error::MAX_CHANNEL_BYTES || !channel.is_ascii() {
        return Err(error::unavailable("unknown"));
    }
    let data = data.unwrap_or_else(|| Value::Array(Vec::new()));
    let data = data.as_array().ok_or_else(|| error::invalid("backend"))?;
    if data.len() > 32 {
        return Err(error::invalid("backend"));
    }
    if channel == "protocol:rendererReady" {
        return controller::protocol_renderer_handshake(&app, data);
    }
    let parsed = BackendChannel::from_str(&channel).map_err(|()| error::unavailable("unknown"))?;
    match parsed {
        BackendChannel::InstallerMode => return Ok(json!(is_installer_mode())),
        BackendChannel::InstallerInfo => {
            if !is_installer_mode() {
                return Err(error::unavailable("installer"));
            }
            return Ok(json!({
                "version": app.package_info().version.to_string(),
                "assetUrl": installer_asset_url(),
                "defaultInstallDir": default_installer_directory(&app).to_string_lossy(),
            }));
        }
        BackendChannel::InstallerInstall => {
            if !is_installer_mode() {
                return Err(error::unavailable("installer"));
            }
            let directory = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("installerInstall"))?;
            return download_and_install(app, directory.to_owned()).await;
        }
        BackendChannel::InstallerLaunch => {
            if !is_installer_mode() {
                return Err(error::unavailable("installer"));
            }
            let directory = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("installerLaunch"))?;
            launch_installed_application(directory)?;
            return Ok(Value::Null);
        }
        BackendChannel::InstallerMinimize => {
            if !is_installer_mode() {
                return Err(error::unavailable("installer"));
            }
            window.minimize().map_err(|_| error::internal())?;
            return Ok(Value::Null);
        }
        BackendChannel::InstallerQuit => {
            if !is_installer_mode() {
                return Err(error::unavailable("installer"));
            }
            app.exit(0);
            return Ok(Value::Null);
        }
        _ => {}
    }
    if parsed == BackendChannel::LoginGamebanana {
        channels::auth::login(app).await
    } else {
        let data = data.clone();
        tauri::async_runtime::spawn_blocking(move || dispatch(&app, &window, parsed, &data))
            .await
            .map_err(|_| error::internal())?
    }
}

const WIN_RES_ALERT_QUIET_PERIOD: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResizeAlertWindowEvent {
    Resized,
    Destroyed,
    Ignore,
}

fn resize_alert_window_event(
    window_label: &str,
    event: &tauri::WindowEvent,
) -> ResizeAlertWindowEvent {
    if window_label != "main" {
        return ResizeAlertWindowEvent::Ignore;
    }
    match event {
        tauri::WindowEvent::Resized(_) => ResizeAlertWindowEvent::Resized,
        tauri::WindowEvent::Destroyed => ResizeAlertWindowEvent::Destroyed,
        _ => ResizeAlertWindowEvent::Ignore,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResizeAlertSchedule {
    worker_id: u64,
    generation: u64,
    start_worker: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResizeAlertCompletion {
    Emit,
    Continue,
    Stop,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResizeAlertMainThreadAction {
    Complete { worker_id: u64, generation: u64 },
    Destroyed,
}

#[derive(Debug, Default)]
struct ResizeAlertTracker {
    generation: u64,
    active_worker: Option<u64>,
}

impl ResizeAlertTracker {
    fn advance(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.generation
    }

    fn resized(&mut self) -> ResizeAlertSchedule {
        let generation = self.advance();
        let (worker_id, start_worker) = match self.active_worker {
            Some(worker_id) => (worker_id, false),
            None => {
                self.active_worker = Some(generation);
                (generation, true)
            }
        };
        ResizeAlertSchedule {
            worker_id,
            generation,
            start_worker,
        }
    }

    fn complete(&mut self, worker_id: u64, generation: u64) -> ResizeAlertCompletion {
        if self.active_worker != Some(worker_id) {
            return ResizeAlertCompletion::Stop;
        }
        if self.generation != generation {
            return ResizeAlertCompletion::Continue;
        }
        self.active_worker = None;
        ResizeAlertCompletion::Emit
    }

    fn cancel(&mut self) {
        self.advance();
        self.active_worker = None;
    }
}

#[derive(Clone, Copy, Debug)]
struct ResizeAlertSignal {
    generation: u64,
    observed_at: tokio::time::Instant,
}

struct ResizeAlertWorker {
    id: u64,
    sender: tokio::sync::watch::Sender<ResizeAlertSignal>,
}

#[derive(Default)]
struct ResizeAlertRuntime {
    tracker: ResizeAlertTracker,
    worker: Option<ResizeAlertWorker>,
}

impl ResizeAlertRuntime {
    fn schedule(
        &mut self,
        observed_at: tokio::time::Instant,
    ) -> Option<(u64, tokio::sync::watch::Receiver<ResizeAlertSignal>)> {
        let mut schedule = self.tracker.resized();
        let mut signal = ResizeAlertSignal {
            generation: schedule.generation,
            observed_at,
        };
        if !schedule.start_worker {
            let reset = self.worker.as_ref().is_some_and(|worker| {
                worker.id == schedule.worker_id && worker.sender.send(signal).is_ok()
            });
            if reset {
                return None;
            }

            self.tracker.cancel();
            self.worker = None;
            schedule = self.tracker.resized();
            signal.generation = schedule.generation;
        }

        let (sender, receiver) = tokio::sync::watch::channel(signal);
        self.worker = Some(ResizeAlertWorker {
            id: schedule.worker_id,
            sender,
        });
        Some((schedule.worker_id, receiver))
    }

    fn complete(&mut self, worker_id: u64, generation: u64) -> ResizeAlertCompletion {
        let completion = self.tracker.complete(worker_id, generation);
        if completion != ResizeAlertCompletion::Continue
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.id == worker_id)
        {
            self.worker = None;
        }
        completion
    }

    fn cancel(&mut self) {
        self.tracker.cancel();
        self.worker = None;
    }

    fn cancel_worker(&mut self, worker_id: u64) {
        if self.tracker.active_worker == Some(worker_id) {
            self.cancel();
        }
    }
}

#[derive(Default)]
struct ResizeAlertCoalescer {
    runtime: Mutex<ResizeAlertRuntime>,
}

impl ResizeAlertCoalescer {
    fn resized(&self, window: &tauri::Window) {
        let task = self
            .runtime
            .lock()
            .ok()
            .and_then(|mut runtime| runtime.schedule(tokio::time::Instant::now()));
        if let Some((worker_id, receiver)) = task {
            let window = window.clone();
            std::mem::drop(tauri::async_runtime::spawn(run_resize_alert_worker(
                window, worker_id, receiver,
            )));
        }
    }

    fn complete(&self, worker_id: u64, generation: u64) -> ResizeAlertCompletion {
        let Ok(mut runtime) = self.runtime.lock() else {
            return ResizeAlertCompletion::Stop;
        };
        runtime.complete(worker_id, generation)
    }

    fn cancel(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.cancel();
        }
    }

    fn cancel_worker(&self, worker_id: u64) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.cancel_worker(worker_id);
        }
    }
}

fn handle_resize_alert_main_thread_action<F>(
    coalescer: &ResizeAlertCoalescer,
    action: ResizeAlertMainThreadAction,
    emit: F,
) -> ResizeAlertCompletion
where
    F: FnOnce(),
{
    let completion = match action {
        ResizeAlertMainThreadAction::Complete {
            worker_id,
            generation,
        } => coalescer.complete(worker_id, generation),
        ResizeAlertMainThreadAction::Destroyed => {
            coalescer.cancel();
            ResizeAlertCompletion::Stop
        }
    };
    if completion == ResizeAlertCompletion::Emit {
        emit();
    }
    completion
}

fn win_res_alert_payload() -> Value {
    Value::Array(Vec::new())
}

async fn run_resize_alert_worker(
    window: tauri::Window,
    worker_id: u64,
    mut receiver: tokio::sync::watch::Receiver<ResizeAlertSignal>,
) {
    loop {
        let signal = *receiver.borrow_and_update();
        let deadline = signal.observed_at + WIN_RES_ALERT_QUIET_PERIOD;
        match tokio::time::timeout_at(deadline, receiver.changed()).await {
            Ok(Ok(())) => continue,
            Ok(Err(_)) => return,
            Err(_) => {
                let emit_window = window.clone();
                let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();
                let queued = window.run_on_main_thread(move || {
                    let completion = emit_window
                        .app_handle()
                        .try_state::<ResizeAlertCoalescer>()
                        .map(|coalescer| {
                            handle_resize_alert_main_thread_action(
                                &coalescer,
                                ResizeAlertMainThreadAction::Complete {
                                    worker_id,
                                    generation: signal.generation,
                                },
                                || {
                                    let _ =
                                        emit_window.emit("winResAlert", win_res_alert_payload());
                                },
                            )
                        })
                        .unwrap_or(ResizeAlertCompletion::Stop);
                    let _ = completion_tx.send(completion);
                });
                if queued.is_err() {
                    if let Some(coalescer) = window.app_handle().try_state::<ResizeAlertCoalescer>()
                    {
                        coalescer.cancel_worker(worker_id);
                    }
                    return;
                }
                let completion = match completion_rx.await {
                    Ok(completion) => completion,
                    Err(_) => {
                        if let Some(coalescer) =
                            window.app_handle().try_state::<ResizeAlertCoalescer>()
                        {
                            coalescer.cancel_worker(worker_id);
                        }
                        return;
                    }
                };
                match completion {
                    ResizeAlertCompletion::Emit => return,
                    ResizeAlertCompletion::Continue => continue,
                    ResizeAlertCompletion::Stop => return,
                }
            }
        }
    }
}

fn main() {
    let app = tauri::Builder::default()
        .manage(ResizeAlertCoalescer::default())
        .manage(TaskbarIconState::default())
        .manage(controller::ProtocolLaunchState::default())
        // Single-instance must be registered before deep-link so a protocol
        // launch aimed at an already-running process is validated and queued
        // by the existing Rust protocol domain instead of opening another data
        // root.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            controller::protocol_second_instance(app, argv);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("themeprot", |context, request| {
            serve_asset(context, request, "themeprot")
        })
        .register_uri_scheme_protocol("packet", |context, request| {
            serve_asset(context, request, "packet")
        })
        .on_page_load(|webview, payload| {
            if webview.label() != "main" {
                return;
            }
            match payload.event() {
                tauri::webview::PageLoadEvent::Started => {
                    controller::protocol_renderer_loading(webview.app_handle())
                }
                tauri::webview::PageLoadEvent::Finished => {
                    controller::protocol_renderer_finished(webview.app_handle());
                    if let Some(state) = webview.app_handle().try_state::<state::AppState>() {
                        if let Err(code) =
                            write_smoke_capability_evidence(webview.app_handle(), &state)
                        {
                            eprintln!("[packaged-smoke] {code}");
                        }
                    }
                }
            }
        })
        .on_menu_event(|app, event| {
            controller::handle_controller_menu_event(app, event.id().as_ref());
        })
        .setup(|app| {
            let default_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|_| "state root unavailable")?;
            let data_dir = configured_data_dir(
                default_data_dir,
                std::env::args_os().skip(1),
                std::env::var_os(SMOKE_DATA_ROOT_ENV),
            )?;
            let resource_dir = app
                .path()
                .resource_dir()
                .map_err(|_| "state root unavailable")?;
            if !is_installer_mode() {
                let app_state = state::AppState::initialize_with_app(
                    data_dir,
                    resource_dir.clone(),
                    app.handle().clone(),
                )
                .map_err(|_| "state root unavailable")?;
                if let Err(code) = channels::lifecycle::recover_startup(&app_state) {
                    app_state
                        .startup_recovery_errors
                        .lock()
                        .map_err(|_| "startup recovery state unavailable")?
                        .push(code);
                }
                if app_state
                    .patching
                    .recover_startup_lifecycle(&deltamod_patching_runtime::LifecycleStorageRoots {
                        store: app_state.data_root.root.join("lifecycle-store"),
                        workspace: app_state.data_root.root.join("lifecycle-workspaces"),
                    })
                    .is_err()
                {
                    app_state
                        .startup_recovery_errors
                        .lock()
                        .map_err(|_| "startup recovery state unavailable")?
                        .push("PATCH_STARTUP_RECOVERY_BLOCKED".into());
                }
                if channels::system::enforce_storage_retention(&app_state).is_err() {
                    app_state
                        .startup_recovery_errors
                        .lock()
                        .map_err(|_| "startup recovery state unavailable")?
                        .push("STORAGE_RETENTION_BLOCKED".into());
                }
                app.manage(app_state);
                controller::protocol_app_ready(app.handle());
            }
            let controller = controller::ControllerMode::new(&resource_dir);
            if !is_installer_mode() && controller.enabled() {
                if let Some(window) = app.get_webview_window("main") {
                    window
                        .set_fullscreen(true)
                        .map_err(|_| "controller mode unavailable")?;
                    if window
                        .is_focused()
                        .map_err(|_| "controller mode unavailable")?
                    {
                        controller
                            .start()
                            .map_err(|_| "controller mode unavailable")?;
                    }
                    controller::install_controller_exit_menu(&window)?;
                }
            }
            app.manage(controller);
            Ok(())
        })
        .on_window_event(|window, event| {
            let resize_event = resize_alert_window_event(window.label(), event);
            if resize_event == ResizeAlertWindowEvent::Destroyed {
                controller::protocol_shutdown(window.app_handle());
            }
            if let Some(coalescer) = window.app_handle().try_state::<ResizeAlertCoalescer>() {
                match resize_event {
                    ResizeAlertWindowEvent::Resized => coalescer.resized(window),
                    ResizeAlertWindowEvent::Destroyed => {
                        handle_resize_alert_main_thread_action(
                            &coalescer,
                            ResizeAlertMainThreadAction::Destroyed,
                            || {},
                        );
                    }
                    ResizeAlertWindowEvent::Ignore => {}
                }
            }
            if let Some(controller) = window
                .app_handle()
                .try_state::<controller::ControllerMode>()
            {
                match event {
                    tauri::WindowEvent::Focused(true) => {
                        let _ = controller.start();
                    }
                    tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Destroyed => {
                        controller.stop()
                    }
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![backend_invoke])
        .build(tauri::generate_context!())
        .expect("failed to build Deltamod Tauri shell");
    app.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            controller::protocol_shutdown(app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDataRoot(PathBuf);

    impl TestDataRoot {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "deltamod-{label}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDataRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn data_root_override_requires_matching_runner_environment() {
        let default = PathBuf::from("default-data");
        let selected = TestDataRoot::new("selected-data");
        let other = TestDataRoot::new("other-data");

        assert_eq!(
            configured_data_dir(default.clone(), Vec::new(), None).unwrap(),
            default
        );
        assert_eq!(
            configured_data_dir(
                PathBuf::from("unused"),
                vec![
                    OsString::from("--data-root"),
                    selected.0.clone().into_os_string()
                ],
                Some(selected.0.clone().into_os_string()),
            )
            .unwrap(),
            selected.0.canonicalize().unwrap()
        );
        assert_eq!(
            configured_data_dir(
                PathBuf::from("unused"),
                vec![
                    OsString::from("--data-root"),
                    selected.0.clone().into_os_string()
                ],
                Some(other.0.clone().into_os_string()),
            ),
            Err("data root override mismatch")
        );
    }

    #[test]
    fn data_root_override_rejects_duplicate_arguments() {
        let selected = TestDataRoot::new("duplicate-data");
        assert_eq!(
            configured_data_dir(
                PathBuf::from("unused"),
                vec![
                    OsString::from("--data-root"),
                    selected.0.clone().into_os_string(),
                    OsString::from(format!("--data-root={}", selected.0.display())),
                ],
                Some(selected.0.clone().into_os_string()),
            ),
            Err("duplicate data root argument")
        );
    }

    #[test]
    fn capability_evidence_is_bound_to_the_exact_non_link_smoke_root() {
        let root = TestDataRoot::new("capability-root");
        let other = TestDataRoot::new("capability-other");
        let candidate = root.0.join(SMOKE_CAPABILITY_FILE);

        assert_eq!(
            validate_smoke_evidence_path(&root.0, &root.0, &candidate, SMOKE_CAPABILITY_FILE,)
                .unwrap(),
            candidate
        );
        assert_eq!(
            validate_smoke_evidence_path(&root.0, &other.0, &candidate, SMOKE_CAPABILITY_FILE,),
            Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into())
        );
        assert_eq!(
            validate_smoke_evidence_path(
                &root.0,
                &root.0,
                &root.0.join("evidence.json"),
                SMOKE_CAPABILITY_FILE,
            ),
            Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into())
        );
        assert_eq!(
            validate_smoke_evidence_path(
                &root.0,
                &root.0,
                &root.0.join("nested").join("..").join(SMOKE_CAPABILITY_FILE),
                SMOKE_CAPABILITY_FILE,
            ),
            Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into())
        );
        assert_eq!(
            validate_smoke_evidence_path(
                &root.0,
                &root.0,
                &root.0.join(SMOKE_PROTOCOL_FILE),
                SMOKE_PROTOCOL_FILE,
            )
            .unwrap(),
            root.0.join(SMOKE_PROTOCOL_FILE)
        );

        fs::write(&candidate, b"occupied").unwrap();
        assert_eq!(
            validate_smoke_evidence_path(&root.0, &root.0, &candidate, SMOKE_CAPABILITY_FILE,),
            Err("TAURI_INVALID_SMOKE_CAPABILITY_PATH".into())
        );
    }

    fn pending_resize_alert(
        coalescer: &ResizeAlertCoalescer,
    ) -> (
        u64,
        ResizeAlertSignal,
        tokio::sync::watch::Receiver<ResizeAlertSignal>,
    ) {
        let (worker_id, mut receiver) = coalescer
            .runtime
            .lock()
            .unwrap()
            .schedule(tokio::time::Instant::now())
            .unwrap();
        let signal = *receiver.borrow_and_update();
        (worker_id, signal, receiver)
    }

    #[test]
    fn resize_alert_burst_completes_only_the_latest_generation() {
        let mut runtime = ResizeAlertRuntime::default();
        let first_at = tokio::time::Instant::now();
        let (worker_id, mut receiver) = runtime.schedule(first_at).unwrap();
        let first = *receiver.borrow_and_update();
        let latest_at = first_at + Duration::from_millis(20);

        assert!(runtime
            .schedule(first_at + Duration::from_millis(10))
            .is_none());
        assert!(runtime.schedule(latest_at).is_none());
        let latest = *receiver.borrow_and_update();
        assert_eq!(latest.generation, first.generation + 2);
        assert_eq!(latest.observed_at, latest_at);
        assert_eq!(
            runtime.complete(worker_id, first.generation),
            ResizeAlertCompletion::Continue
        );
        assert_eq!(
            runtime.complete(worker_id, latest.generation),
            ResizeAlertCompletion::Emit
        );
        assert_eq!(
            runtime.complete(worker_id, latest.generation),
            ResizeAlertCompletion::Stop
        );
    }

    #[test]
    fn separated_resize_alert_bursts_each_complete_once() {
        let mut runtime = ResizeAlertRuntime::default();
        let first_at = tokio::time::Instant::now();
        let (first_worker, mut first_receiver) = runtime.schedule(first_at).unwrap();
        let first = *first_receiver.borrow_and_update();
        let mut emissions = usize::from(
            runtime.complete(first_worker, first.generation) == ResizeAlertCompletion::Emit,
        );
        assert!(first_receiver.has_changed().is_err());

        let (second_worker, mut second_receiver) = runtime
            .schedule(first_at + WIN_RES_ALERT_QUIET_PERIOD + Duration::from_millis(1))
            .unwrap();
        let second = *second_receiver.borrow_and_update();
        assert_ne!(first_worker, second_worker);
        emissions += usize::from(
            runtime.complete(second_worker, second.generation) == ResizeAlertCompletion::Emit,
        );
        assert_eq!(emissions, 2);
    }

    #[test]
    fn resize_alert_non_main_and_non_resize_window_events_schedule_nothing() {
        let resize = tauri::WindowEvent::Resized(tauri::PhysicalSize::new(1100, 720));
        let events = [
            resize_alert_window_event("secondary", &resize),
            resize_alert_window_event("main", &tauri::WindowEvent::Focused(true)),
            resize_alert_window_event("main", &tauri::WindowEvent::Destroyed),
        ];

        assert_eq!(
            events
                .iter()
                .filter(|event| **event == ResizeAlertWindowEvent::Resized)
                .count(),
            0
        );
        assert_eq!(events[2], ResizeAlertWindowEvent::Destroyed);
    }

    #[test]
    fn cancelled_resize_alert_generation_cannot_emit() {
        let mut runtime = ResizeAlertRuntime::default();
        let (worker_id, mut receiver) = runtime.schedule(tokio::time::Instant::now()).unwrap();
        let scheduled = *receiver.borrow_and_update();

        runtime.cancel();
        assert!(receiver.has_changed().is_err());
        assert_eq!(
            runtime.complete(worker_id, scheduled.generation),
            ResizeAlertCompletion::Stop
        );
    }

    #[test]
    fn resize_alert_decision_releases_the_lock_before_emission() {
        let coalescer = ResizeAlertCoalescer::default();
        let (worker_id, signal, _receiver) = pending_resize_alert(&coalescer);
        let emitted = std::cell::Cell::new(false);

        let completion = handle_resize_alert_main_thread_action(
            &coalescer,
            ResizeAlertMainThreadAction::Complete {
                worker_id,
                generation: signal.generation,
            },
            || {
                assert!(coalescer.runtime.try_lock().is_ok());
                emitted.set(true);
            },
        );

        assert_eq!(completion, ResizeAlertCompletion::Emit);
        assert!(emitted.get());
    }

    #[test]
    fn queued_resize_alert_destroy_prevents_emission() {
        let coalescer = ResizeAlertCoalescer::default();
        let (worker_id, signal, receiver) = pending_resize_alert(&coalescer);
        let emissions = std::cell::Cell::new(0);

        assert_eq!(
            handle_resize_alert_main_thread_action(
                &coalescer,
                ResizeAlertMainThreadAction::Destroyed,
                || emissions.set(emissions.get() + 1),
            ),
            ResizeAlertCompletion::Stop
        );
        assert!(receiver.has_changed().is_err());
        assert_eq!(
            handle_resize_alert_main_thread_action(
                &coalescer,
                ResizeAlertMainThreadAction::Complete {
                    worker_id,
                    generation: signal.generation,
                },
                || emissions.set(emissions.get() + 1),
            ),
            ResizeAlertCompletion::Stop
        );
        assert_eq!(emissions.get(), 0);
    }

    #[test]
    fn queued_resize_alert_emit_cannot_run_after_destroy() {
        let coalescer = ResizeAlertCoalescer::default();
        let (worker_id, signal, _receiver) = pending_resize_alert(&coalescer);
        let destroyed = std::cell::Cell::new(false);
        let emissions = std::cell::Cell::new(0);

        assert_eq!(
            handle_resize_alert_main_thread_action(
                &coalescer,
                ResizeAlertMainThreadAction::Complete {
                    worker_id,
                    generation: signal.generation,
                },
                || {
                    assert!(!destroyed.get());
                    emissions.set(emissions.get() + 1);
                },
            ),
            ResizeAlertCompletion::Emit
        );
        destroyed.set(true);
        handle_resize_alert_main_thread_action(
            &coalescer,
            ResizeAlertMainThreadAction::Destroyed,
            || emissions.set(emissions.get() + 1),
        );
        assert_eq!(
            handle_resize_alert_main_thread_action(
                &coalescer,
                ResizeAlertMainThreadAction::Complete {
                    worker_id,
                    generation: signal.generation,
                },
                || emissions.set(emissions.get() + 1),
            ),
            ResizeAlertCompletion::Stop
        );
        assert_eq!(emissions.get(), 1);
    }

    #[test]
    fn resize_alert_preserves_the_legacy_empty_array_payload() {
        assert_eq!(win_res_alert_payload(), Value::Array(Vec::new()));
    }

    #[test]
    fn dispatch_allowlist_is_strict() {
        assert_eq!(
            BackendChannel::from_str("version"),
            Ok(BackendChannel::Version)
        );
        assert!(matches!(
            BackendChannel::from_str("startGame"),
            Ok(BackendChannel::Implemented(name)) if name == "startGame"
        ));
        for channel in [
            "validateGamebananaToken",
            "getGamebananaPic",
            "getGamebananaID",
            "getGamebananaUserinfo",
            "leaveCommentGamebanana",
            "gamebanana_getCollections",
            "fireUpdate",
            "start-update",
            "ignore-update",
            "updater-status",
        ] {
            assert!(matches!(
                BackendChannel::from_str(channel),
                Ok(BackendChannel::Implemented(name)) if name == channel
            ));
        }
        for channel in ["isCMode", "cmode-on", "cmode-off", "setAppIcon"] {
            assert!(!matches!(
                BackendChannel::from_str(channel),
                Ok(BackendChannel::Unsupported(_))
            ));
        }
        assert_eq!(
            BackendChannel::from_str("loginGamebanana"),
            Ok(BackendChannel::LoginGamebanana)
        );
        for channel in [
            "shouldGoIM",
            "getEditionByIndex",
            "executeArgumentCmd",
            "installDeltamodCLI",
            "openFlagDatabase",
        ] {
            assert!(matches!(
                BackendChannel::from_str(channel),
                Ok(BackendChannel::Implemented(name)) if name == channel
            ));
        }
        for channel in [
            "canReportError",
            "sampleError",
            "modalTest",
            "openElectronTracer",
        ] {
            assert!(BackendChannel::from_str(channel).is_err());
        }
        assert!(BackendChannel::from_str("shell:open").is_err());
    }
    #[test]
    fn errors_are_bounded() {
        assert_eq!(
            error::unavailable(&"x".repeat(500)),
            "TAURI_COMMAND_UNAVAILABLE:unknown"
        );
    }

    #[test]
    fn mod_state_changes_do_not_refresh_the_page() {
        use deltamod_mods_themes_runtime::EventIntent;

        assert!(renderer_event(EventIntent::ModStateChanged { uid: "mod".into() }).is_none());
        assert_eq!(
            renderer_event(EventIntent::ModsChanged).map(|event| event.0),
            Some("refresh")
        );
        assert_eq!(
            renderer_event(EventIntent::ThemesChanged).map(|event| event.0),
            Some("themeChange")
        );
    }

    #[test]
    fn windows_custom_protocol_urls_use_internal_asset_schemes() {
        assert_eq!(
            normalize_asset_uri(
                "http://themeprot.localhost/data/base.theme.json",
                "themeprot"
            ),
            Some("theme://data/base.theme.json".to_owned())
        );
        assert_eq!(
            normalize_asset_uri("packet://pack/image/icon.png", "packet"),
            Some("packet://pack/image/icon.png".to_owned())
        );
        let headerless = Request::builder().body(Vec::new()).unwrap();
        assert!(protocol_origin_allowed(&headerless));
        let foreign = Request::builder()
            .header("origin", "https://example.invalid")
            .body(Vec::new())
            .unwrap();
        assert!(!protocol_origin_allowed(&foreign));
    }
}
