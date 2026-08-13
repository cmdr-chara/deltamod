#![forbid(unsafe_code)]

use serde::Serialize;
use sha2::{Digest, Sha256};
#[cfg(not(windows))]
use std::fs::File;
use std::fs::{self, Metadata};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::time::UNIX_EPOCH;

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    File {
        relative: String,
        signature: String,
        sha256: String,
        completed: usize,
        total: usize,
    },
    Done {
        file_count: usize,
    },
}

pub fn run(root: &Path, mut emit: impl FnMut(&Event) -> io::Result<()>) -> io::Result<()> {
    let root_metadata = fs::symlink_metadata(root)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(invalid_data(
            "The game hash source is not a safe directory.",
        ));
    }

    let files = list_files(root)?;
    let total = files.len();
    for (index, path) in files.iter().enumerate() {
        let relative_path = path
            .strip_prefix(root)
            .map_err(|_| invalid_data("A game file escaped the hash source."))?;
        let relative = relative_path.to_string_lossy().replace('\\', "/");
        let (signature, sha256) = hash_file(path)?;
        emit(&Event::File {
            relative,
            signature,
            sha256,
            completed: index + 1,
            total,
        })?;
    }
    emit(&Event::Done { file_count: total })
}

/// Hashes one root-relative regular file using the same link, identity, and
/// mutation checks as the full-tree worker.
pub fn hash_relative_file(root: &Path, relative: &Path) -> io::Result<(String, String)> {
    hash_file(&safe_relative_file(root, relative)?)
}

/// Returns the cache signature only after validating every path component and
/// rejecting linked or non-regular files.
pub fn relative_file_signature(root: &Path, relative: &Path) -> io::Result<String> {
    let path = safe_relative_file(root, relative)?;
    let metadata = fs::symlink_metadata(&path)?;
    if link_count_at(&path, &metadata)? != 1 {
        return Err(invalid_data("Game file cannot be hashed safely."));
    }
    file_signature(&metadata)
}

fn safe_relative_file(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let root_metadata = fs::symlink_metadata(root)?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(invalid_data(
            "The game hash source is not a safe directory.",
        ));
    }
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid_data("The required game file path is unsafe."));
    }

    let mut current = root.to_path_buf();
    let component_count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink()
            || (index + 1 < component_count && !metadata.is_dir())
            || (index + 1 == component_count && !metadata.is_file())
        {
            return Err(invalid_data("Game file cannot be hashed safely."));
        }
    }
    Ok(current)
}

#[cfg(not(windows))]
fn file_signature(metadata: &Metadata) -> io::Result<String> {
    Ok(format!("{}:{}", metadata.len(), modified_millis(metadata)?))
}

#[cfg(windows)]
fn file_signature(metadata: &Metadata) -> io::Result<String> {
    use std::os::windows::fs::MetadataExt;
    Ok(format!(
        "{}:{}",
        metadata.file_size(),
        windows_mtime_millis(metadata.last_write_time() as i64)?
    ))
}

fn list_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let before = fs::symlink_metadata(&directory)?;
        if !before.is_dir() || before.file_type().is_symlink() {
            return Err(invalid_data(
                "A directory changed while it was being scanned.",
            ));
        }

        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                directories.push(path);
            } else if metadata.is_file() && link_count_at(&path, &metadata)? == 1 {
                files.push(path);
            }
        }

        let after = fs::symlink_metadata(&directory)?;
        if metadata_changed(&before, &after) {
            return Err(invalid_data(
                "A directory changed while it was being scanned.",
            ));
        }
    }
    Ok(files)
}

#[cfg(not(windows))]
fn hash_file(path: &Path) -> io::Result<(String, String)> {
    hash_file_with_hook(path, || {})
}

#[cfg(not(windows))]
fn hash_file_with_hook(path: &Path, after_read: impl FnOnce()) -> io::Result<(String, String)> {
    let path_before = fs::symlink_metadata(path)?;
    if !path_before.is_file()
        || path_before.file_type().is_symlink()
        || link_count_at(path, &path_before)? != 1
    {
        return Err(invalid_data("Game file cannot be hashed safely."));
    }

    let file = File::open(path)?;
    let opened_before = file.metadata()?;
    if metadata_changed(&path_before, &opened_before) {
        return Err(invalid_data(
            "A game file changed before it could be hashed.",
        ));
    }

    let mut reader = BufReader::new(&file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    after_read();

    let opened_after = file.metadata()?;
    let path_after = fs::symlink_metadata(path)?;
    if metadata_changed(&opened_before, &opened_after)
        || metadata_changed(&opened_after, &path_after)
        || link_count_at(path, &opened_after)? != 1
    {
        return Err(invalid_data(
            "A game file changed while it was being hashed.",
        ));
    }

    let signature = format!("{}:{}", opened_after.len(), modified_millis(&opened_after)?);
    Ok((signature, hex::encode(hasher.finalize())))
}

#[cfg(windows)]
fn hash_file(path: &Path) -> io::Result<(String, String)> {
    hash_file_with_hook(path, || {})
}

#[cfg(windows)]
fn hash_file_with_hook(path: &Path, after_read: impl FnOnce()) -> io::Result<(String, String)> {
    use fence_windows::{NodeKind, RootHandle};

    let parent = path
        .parent()
        .ok_or_else(|| invalid_data("Game file has no parent directory."))?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid_data("Game file has no name."))?;
    let root = RootHandle::open(parent).map_err(windows_error)?;
    let node = root
        .directory()
        .open_named_child(name)
        .map_err(windows_error)?;
    let before = node.metadata();
    if before.kind != NodeKind::RegularFile || before.link_count != 1 {
        return Err(invalid_data("Game file cannot be hashed safely."));
    }

    let mut reader = BufReader::new(node.try_clone_file().map_err(windows_error)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    after_read();

    let after = node.refresh_metadata().map_err(windows_error)?;
    node.verify_path_identity().map_err(windows_error)?;
    if before != after || after.link_count != 1 {
        return Err(invalid_data(
            "A game file changed while it was being hashed.",
        ));
    }

    let signature = format!(
        "{}:{}",
        after.size,
        windows_mtime_millis(after.last_write_time)?
    );
    Ok((signature, hex::encode(hasher.finalize())))
}

#[cfg(windows)]
fn windows_mtime_millis(file_time: i64) -> io::Result<String> {
    const UNIX_EPOCH_FILE_TIME: i64 = 116_444_736_000_000_000;
    let ticks = file_time
        .checked_sub(UNIX_EPOCH_FILE_TIME)
        .ok_or_else(|| invalid_data("A game file has an invalid modification time."))?;
    let seconds = ticks.div_euclid(10_000_000);
    let subsecond_ticks = ticks.rem_euclid(10_000_000);
    let millis = seconds as f64 * 1000.0 + subsecond_ticks as f64 / 10_000.0;
    Ok(millis.to_string())
}

#[cfg(windows)]
fn windows_error(error: fence_windows::WindowsError) -> io::Error {
    io::Error::other(error.to_string())
}

#[cfg(not(windows))]
fn modified_millis(metadata: &Metadata) -> io::Result<String> {
    let duration = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_data("A game file has an invalid modification time."))?;
    let millis = duration.as_secs_f64() * 1000.0;
    Ok(millis.to_string())
}

fn metadata_changed(before: &Metadata, after: &Metadata) -> bool {
    before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || file_identity(before) != file_identity(after)
}

#[cfg(unix)]
fn link_count(metadata: &Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink()
}

#[cfg(not(windows))]
fn link_count_at(_path: &Path, metadata: &Metadata) -> io::Result<u64> {
    Ok(link_count(metadata))
}

#[cfg(windows)]
fn link_count_at(path: &Path, _metadata: &Metadata) -> io::Result<u64> {
    use fence_windows::RootHandle;
    let parent = path
        .parent()
        .ok_or_else(|| invalid_data("Game file has no parent directory."))?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid_data("Game file has no name."))?;
    let root = RootHandle::open(parent).map_err(windows_error)?;
    let node = root
        .directory()
        .open_named_child(name)
        .map_err(windows_error)?;
    Ok(node.metadata().link_count as u64)
}

#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn file_identity(_metadata: &Metadata) -> (u64, u64) {
    (0, 0)
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn hashes_files_and_emits_progress() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("data.win"), b"game data").unwrap();
        let mut events = Vec::new();
        run(directory.path(), |event| {
            events.push(serde_json::to_value(event)?);
            Ok(())
        })
        .unwrap();

        assert_eq!(events[0]["relative"], "data.win");
        assert_eq!(
            events[0]["sha256"],
            "f0b21bf007b642ee3fcfaaa207ceeec9e5d57f74d6cc7d1d63c2d7d200994048"
        );
        assert_eq!(events[1]["fileCount"], 1);
    }

    #[test]
    fn hashes_only_a_safe_required_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("chapter1")).unwrap();
        fs::write(directory.path().join("chapter1/data.win"), b"game data").unwrap();
        let (_, digest) =
            hash_relative_file(directory.path(), Path::new("chapter1/data.win")).unwrap();
        assert_eq!(
            digest,
            "f0b21bf007b642ee3fcfaaa207ceeec9e5d57f74d6cc7d1d63c2d7d200994048"
        );
        assert!(hash_relative_file(directory.path(), Path::new("../outside")).is_err());
    }

    #[test]
    fn rejects_hard_linked_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::write(&source, b"data").unwrap();
        fs::hard_link(&source, directory.path().join("second")).unwrap();
        let mut events = Vec::new();
        run(directory.path(), |event| {
            events.push(serde_json::to_value(event)?);
            Ok(())
        })
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["fileCount"], 0);
    }

    #[test]
    fn detects_mutation_during_hashing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.win");
        fs::write(&path, b"before").unwrap();
        let result = hash_file_with_hook(&path, || {
            let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(b" after").unwrap();
            file.sync_all().unwrap();
        });
        assert!(result.is_err());
    }
}
