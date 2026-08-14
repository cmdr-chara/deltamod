#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

mod channels;
mod controller;
mod error;
mod state;

use base64::Engine;
use deltamod_asset_runtime::{headers, plan_range, Body, Error as AssetError, Range};
use http::{Request, Response, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, UriSchemeContext, WebviewWindow};

#[cfg(any(target_os = "windows", target_os = "android"))]
const MAIN_ORIGIN: &str = "http://tauri.localhost";
#[cfg(not(any(target_os = "windows", target_os = "android")))]
const MAIN_ORIGIN: &str = "tauri://localhost";

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
            "showWindow" => Self::ShowWindow,
            "toggleFullscreen" => Self::ToggleFullscreen,
            "version" => Self::Version,
            "getUniqueFlag"
            | "setUniqueFlag"
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
            | "dlmodURL" => Self::Implemented(channel.to_owned()),
            "undertaleModTool:status" | "fireUpdate" => Self::Implemented(channel.to_owned()),
            "htmlAlert_outwin"
            | "shouldGoIM"
            | "sampleError"
            | "rebootDev"
            | "setSponsor"
            | "shakeCommunityWindowForEasterEgg"
            | "createInstallLink"
            | "undertaleModTool:openInstallation"
            | "gamebanana_downloadAllInCollection"
            | "start-update"
            | "ignore-update"
            | "getEditionByIndex"
            | "openFlagDatabase"
            | "deltamoddersDiscord"
            | "canReportError"
            | "npsCallback"
            | "executeArgumentCmd"
            | "initialize"
            | "modalTest"
            | "openElectronTracer"
            | "installDeltamodCLI" => Self::Unsupported(channel.to_owned()),
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
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        if restart {
            app.restart();
        } else {
            app.exit(0);
        }
    });
}

fn set_app_icon(window: &WebviewWindow, data: &[Value]) -> Result<Value, String> {
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
        .set_icon(icon)
        .map(|_| Value::Null)
        .map_err(|_| error::internal())
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
        BackendChannel::SetAppIcon => set_app_icon(window, data),
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
    use deltamod_updater_launch_runtime::UpdateEvent;
    let events = state
        .updater_events
        .lock()
        .map(|mut events| events.drain(..).collect::<Vec<_>>())
        .unwrap_or_default();
    for event in events {
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

fn main() {
    tauri::Builder::default()
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
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|_| "state root unavailable")?;
            let resource_dir = app
                .path()
                .resource_dir()
                .map_err(|_| "state root unavailable")?;
            if !is_installer_mode() {
                app.manage(
                    state::AppState::initialize_with_app(
                        data_dir,
                        resource_dir.clone(),
                        app.handle().clone(),
                    )
                    .map_err(|_| "state root unavailable")?,
                );
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
                }
            }
            app.manage(controller);
            Ok(())
        })
        .on_window_event(|window, event| {
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
        .run(tauri::generate_context!())
        .expect("failed to run Deltamod Tauri shell");
}

#[cfg(test)]
mod tests {
    use super::*;
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
        for channel in ["executeArgumentCmd", "start-update"] {
            assert!(matches!(
                BackendChannel::from_str(channel),
                Ok(BackendChannel::Unsupported(name)) if name == channel
            ));
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
