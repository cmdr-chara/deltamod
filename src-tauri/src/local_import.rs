use crate::{channels, state};
use deltamod_profile_install_runtime::MAX_LEGACY_INSTALLATION_INDEX;
use deltamod_tauri_os_adapters::ChoiceBackend;
use serde_json::{json, Value};
use std::{ffi::OsString, fs, io::Read, path::PathBuf};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const LAUNCH_MARKER: &[u8] = b"deltamod-community-open-v1\n";
const SELECT_MARKER_PREFIX: &[u8] = b"deltamod-community-select-v1\n";
const HANDOFF_DIRECTORY: &str = "Deltamod Community CLI";

#[derive(Debug, Eq, PartialEq)]
enum HandoffIntent {
    Launch(PathBuf),
    SelectInstallation { index: u32, marker: PathBuf },
    Import(PathBuf),
    Ignore,
}

fn parse_handoff_arg(value: OsString) -> Result<HandoffIntent, &'static str> {
    let path = PathBuf::from(value);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("modarchive") => validate_archive(path).map(HandoffIntent::Import),
        Some("deltamod-open") => validate_launch_marker(path),
        _ => Ok(HandoffIntent::Ignore),
    }
}

fn validate_archive(path: PathBuf) -> Result<PathBuf, &'static str> {
    if !path.is_absolute() {
        return Err("The requested mod package path is not absolute.");
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "The requested mod package is no longer available.")?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > MAX_ARCHIVE_BYTES
    {
        return Err("The requested mod package is not a safe regular archive.");
    }
    fs::canonicalize(path).map_err(|_| "The requested mod package could not be resolved.")
}

fn validate_launch_marker(path: PathBuf) -> Result<HandoffIntent, &'static str> {
    if !path.is_absolute() {
        return Err("The Deltamod launch marker path is not absolute.");
    }
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "The Deltamod launch marker is no longer available.")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 128 {
        return Err("The Deltamod launch marker is invalid.");
    }
    let marker =
        fs::canonicalize(&path).map_err(|_| "The Deltamod launch marker could not be resolved.")?;
    let mut file =
        fs::File::open(&marker).map_err(|_| "The Deltamod launch marker could not be read.")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(129)
        .read_to_end(&mut bytes)
        .map_err(|_| "The Deltamod launch marker could not be read.")?;
    if bytes.len() > 128 {
        return Err("The Deltamod launch marker is invalid.");
    }
    if bytes == LAUNCH_MARKER {
        let handoff_root = std::env::temp_dir().join(HANDOFF_DIRECTORY);
        let handoff_root = fs::canonicalize(handoff_root)
            .map_err(|_| "The Deltamod CLI handoff directory is unavailable.")?;
        if marker.parent() != Some(handoff_root.as_path()) {
            return Err("The Deltamod launch marker is not trusted.");
        }
        return Ok(HandoffIntent::Launch(marker));
    }
    if let Some(payload) = bytes.strip_prefix(SELECT_MARKER_PREFIX) {
        let text = std::str::from_utf8(payload)
            .map_err(|_| "The Deltamod installation marker is invalid.")?;
        let index_text = text
            .strip_suffix('\n')
            .ok_or("The Deltamod installation marker is invalid.")?;
        let index = index_text
            .parse::<u32>()
            .map_err(|_| "The Deltamod installation marker is invalid.")?;
        if index > MAX_LEGACY_INSTALLATION_INDEX {
            return Err("The Deltamod installation marker is out of range.");
        }
        return Ok(HandoffIntent::SelectInstallation { index, marker });
    }
    Err("The Deltamod launch marker is not trusted.")
}

pub(crate) fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn handle_args<I>(app: &AppHandle, args: I)
where
    I: IntoIterator<Item = OsString>,
{
    for arg in args {
        match parse_handoff_arg(arg) {
            Ok(HandoffIntent::Launch(marker)) => {
                focus_main(app);
                let _ = fs::remove_file(marker);
            }
            Ok(HandoffIntent::SelectInstallation { index, marker: _ }) => {
                let state = app.state::<state::AppState>();
                let current = state
                    .profile_runtime
                    .legacy_system_index()
                    .ok()
                    .and_then(|value| value.as_u64())
                    .and_then(|value| u32::try_from(value).ok());
                if current == Some(index) {
                    // A selector only changes the active installation; it never
                    // launches the game as a side effect.
                    let _ = app.emit("refresh", json!({}));
                    let _ = app.emit("page", "main");
                    focus_main(app);
                    continue;
                }
                match state.profile_runtime.legacy_change_system_index(index) {
                    Ok(()) => {
                        // `game_store_path` and the patch runtime are initialized from
                        // the selected profile at startup. Restart once so the selected
                        // profile becomes authoritative; on restart this guard sees the
                        // same index and avoids a restart loop. The desktop marker is
                        // intentionally retained for future reuse.
                        app.restart();
                    }
                    Err(error) => {
                        let message = error.to_string();
                        show_error(app, &message);
                    }
                }
            }
            Ok(HandoffIntent::Import(path)) => schedule_import(app.clone(), path),
            Ok(HandoffIntent::Ignore) => {}
            Err(message) => show_error(app, message),
        }
    }
}

fn schedule_import(app: AppHandle, archive: PathBuf) {
    focus_main(&app);
    let task_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(message) = import_archive(&task_app, archive) {
            show_error(&task_app, &message);
        }
    });
}

fn import_archive(app: &AppHandle, archive: PathBuf) -> Result<(), String> {
    let state = app.state::<state::AppState>();
    let dialogs = deltamod_tauri_os_adapters::tauri_adapter::TauriDialogBackend::new(app);
    let filename = archive
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Deltamod package")
        .chars()
        .take(160)
        .collect::<String>();
    let decision = dialogs
        .choose(
            "Import Deltamod package",
            &format!(
                "Deltamod Community CLI requested an import of:\n\n{filename}\n\nThe package will be validated and staged before it is installed."
            ),
            &["Import package".to_owned()],
        )
        .map_err(|_| "The import confirmation dialog could not be opened.".to_owned())?;
    if decision != Some(0) {
        return Ok(());
    }

    let result = channels::import_download::run_import(
        &dialogs,
        &archive,
        &state.data_root.root.join("packets"),
        None,
        || false,
    )?;
    if result == json!(true) {
        let _ = app.emit("refresh", Value::Null);
        crate::emit_runtime_events(app, &state);
    }
    Ok(())
}

fn show_error(app: &AppHandle, message: &str) {
    focus_main(app);
    let bounded = message.chars().take(512).collect::<String>();
    let _ = app.emit("gplog", json!({"log": bounded.clone(), "percent": -1.0}));
    app.dialog()
        .message(bounded)
        .title("Deltamod import failed")
        .blocking_show();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "deltamod-file-handoff-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("package.modarchive");
        fs::write(&archive, b"PK\x05\x06fixture").unwrap();
        (root, archive)
    }

    #[test]
    fn accepts_safe_absolute_modarchive_paths() {
        let (root, archive) = archive_fixture();
        assert!(matches!(
            parse_handoff_arg(archive.clone().into_os_string()),
            Ok(HandoffIntent::Import(_))
        ));
        let text = root.join("package.txt");
        fs::write(&text, b"not an archive").unwrap();
        assert_eq!(
            parse_handoff_arg(text.into_os_string()).unwrap(),
            HandoffIntent::Ignore
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_or_empty_archive_paths() {
        let root = std::env::temp_dir().join(format!(
            "deltamod-missing-handoff-test-{}",
            std::process::id()
        ));
        let missing = root.join("missing.modarchive");
        assert!(parse_handoff_arg(missing.into_os_string()).is_err());
        fs::create_dir_all(&root).unwrap();
        let empty = root.join("empty.modarchive");
        fs::write(&empty, b"").unwrap();
        assert!(parse_handoff_arg(empty.into_os_string()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "deltamod-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn valid_launch_marker_is_accepted_only_from_cli_temp_scope() {
        let root = std::env::temp_dir().join(HANDOFF_DIRECTORY);
        fs::create_dir_all(&root).unwrap();
        let marker = root.join(format!("test-{}.deltamod-open", std::process::id()));
        fs::write(&marker, LAUNCH_MARKER).unwrap();
        assert_eq!(
            parse_handoff_arg(marker.clone().into_os_string()).unwrap(),
            HandoffIntent::Launch(marker.clone())
        );
        fs::remove_file(marker).unwrap();
    }

    #[test]
    fn launch_marker_outside_cli_temp_scope_is_rejected() {
        let root = test_root("outside-launch");
        let marker = root.join("test.deltamod-open");
        fs::write(&marker, LAUNCH_MARKER).unwrap();
        assert!(parse_handoff_arg(marker.into_os_string()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installation_marker_selects_a_bounded_index() {
        let root = test_root("select");
        let marker = root.join("test.deltamod-open");
        fs::write(&marker, b"deltamod-community-select-v1\n7\n").unwrap();
        assert_eq!(
            parse_handoff_arg(marker.clone().into_os_string()).unwrap(),
            HandoffIntent::SelectInstallation { index: 7, marker }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installation_marker_rejects_out_of_range_indices() {
        let root = test_root("select-range");
        let marker = root.join("test.deltamod-open");
        fs::write(&marker, b"deltamod-community-select-v1\n256\n").unwrap();
        assert!(parse_handoff_arg(marker.into_os_string()).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
