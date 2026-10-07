//! Windows reference files: unmodified `data.win` files from the user's own
//! Windows copy of the game. Mods are made against these files, so platforms
//! whose data file differs (DELTARUNE's Mac `game.ios`) apply `.xdelta` mods to
//! the reference copy and publish the result in place of their own data file.

use crate::{error, state::AppState};
use deltamod_tauri_os_adapters::{DialogBackend, DialogRequest};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_CHAPTERS: u8 = 7;
const MAX_REFERENCE_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const NO_FILES_MESSAGE: &str = "That folder has no unmodified Windows data.win files. Choose the Windows game folder that contains DELTARUNE.exe and the chapter1_windows, chapter2_windows, … folders.";

/// Every reference file a mod can target, relative to the Windows game folder.
fn reference_candidates() -> Vec<String> {
    std::iter::once("data.win".to_owned())
        .chain((1..=MAX_CHAPTERS).map(|chapter| format!("chapter{chapter}_windows/data.win")))
        .collect()
}

fn current_root(state: &AppState) -> Option<PathBuf> {
    let game = super::runtime::active_game_id(state)?;
    crate::state::reference_files_root(&state.data_root, &game)
}

/// True when this platform renames the game's data file, so `.xdelta` mods
/// cannot be applied to the installed game directly.
fn needed(state: &AppState) -> bool {
    state
        .patching
        .definition
        .data_files
        .first()
        .is_some_and(|file| !file.eq_ignore_ascii_case("data.win"))
}

fn present_files(root: &Path) -> Vec<String> {
    reference_candidates()
        .into_iter()
        .filter(|relative| is_game_data_file(&root.join(relative)))
        .collect()
}

/// A regular, non-linked GameMaker data file (IFF `FORM` header).
fn is_game_data_file(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() < 8 || metadata.len() > MAX_REFERENCE_FILE_BYTES {
        return false;
    }
    let mut magic = [0_u8; 4];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok_and(|()| &magic == b"FORM")
}

fn status(state: &AppState) -> Value {
    let files = current_root(state)
        .map(|root| present_files(&root))
        .unwrap_or_default();
    json!({
        "needed": needed(state),
        "files": files,
    })
}

fn choose<D: DialogBackend>(dialogs: &D, state: &AppState) -> Result<Value, String> {
    // Only platforms that rename the game's data file (macOS) use reference files.
    if !needed(state) {
        return Err(error::unavailable("referenceFiles:choose"));
    }
    let root = current_root(state).ok_or_else(|| error::unavailable("referenceFiles:choose"))?;
    let request = DialogRequest::folder("Choose your Windows DELTARUNE folder");
    let Some(selected) = dialogs.pick(&request).map_err(|_| error::internal())? else {
        return Ok(Value::Null);
    };
    let found = present_files(&selected);
    if found.is_empty() {
        return Ok(json!({ "ok": false, "message": NO_FILES_MESSAGE }));
    }
    let parent = root.parent().ok_or_else(error::internal)?;
    fs::create_dir_all(parent).map_err(|_| error::internal())?;
    let staging = parent.join(format!(".reference-{}", uuid::Uuid::new_v4()));
    let result = stage_and_publish(&selected, &found, &staging, &root);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    let mut response = status(state);
    response["ok"] = json!(true);
    Ok(response)
}

/// Copies into a private sibling folder first, so a failed or partial copy never
/// replaces a working set. On APFS `fs::copy` clones, using almost no space.
fn stage_and_publish(
    source: &Path,
    files: &[String],
    staging: &Path,
    root: &Path,
) -> Result<(), String> {
    for relative in files {
        let from = source.join(relative);
        let to = staging.join(relative);
        fs::create_dir_all(to.parent().ok_or_else(error::internal)?)
            .map_err(|_| error::internal())?;
        let copied = fs::copy(&from, &to).map_err(|_| error::internal())?;
        if copied != fs::metadata(&from).map_err(|_| error::internal())?.len()
            || !is_game_data_file(&to)
        {
            return Err(error::internal());
        }
    }
    if root.exists() {
        let retired = staging.with_extension("old");
        fs::rename(root, &retired).map_err(|_| error::internal())?;
        if fs::rename(staging, root).is_err() {
            let _ = fs::rename(&retired, root);
            return Err(error::internal());
        }
        let _ = fs::remove_dir_all(retired);
    } else {
        fs::rename(staging, root).map_err(|_| error::internal())?;
    }
    Ok(())
}

pub fn dispatch<D: DialogBackend>(
    state: &AppState,
    dialogs: &D,
    channel: &str,
) -> Result<Option<Value>, String> {
    Ok(Some(match channel {
        "referenceFiles:status" => status(state),
        "referenceFiles:choose" => choose(dialogs, state)?,
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_file(path: &Path, body: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = b"FORM".to_vec();
        bytes.extend_from_slice(body);
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn only_gamemaker_data_files_in_known_places_are_references() {
        let dir = std::env::temp_dir().join(format!("deltamod-ref-{}", uuid::Uuid::new_v4()));
        data_file(&dir.join("data.win"), b"root");
        data_file(&dir.join("chapter3_windows/data.win"), b"three");
        fs::create_dir_all(dir.join("chapter4_windows")).unwrap();
        fs::write(dir.join("chapter4_windows/data.win"), b"not gamemaker").unwrap();
        data_file(&dir.join("chapter9_windows/data.win"), b"unknown chapter");
        assert_eq!(
            present_files(&dir),
            ["data.win", "chapter3_windows/data.win"]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn publishing_replaces_the_previous_set_atomically() {
        let dir = std::env::temp_dir().join(format!("deltamod-ref-{}", uuid::Uuid::new_v4()));
        let source = dir.join("windows");
        let root = dir.join("reference-files/toby.deltarune");
        data_file(&source.join("chapter3_windows/data.win"), b"three");
        data_file(&root.join("chapter1_windows/data.win"), b"old");
        let files = present_files(&source);
        stage_and_publish(&source, &files, &dir.join("reference-files/.stage"), &root).unwrap();
        assert_eq!(present_files(&root), ["chapter3_windows/data.win"]);
        assert_eq!(
            fs::read_dir(dir.join("reference-files")).unwrap().count(),
            1,
            "no staging or retired folders are left behind"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
