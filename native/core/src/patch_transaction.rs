use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_OPERATIONS: usize = 10_000;
const MAX_STRING_BYTES: usize = 32_768;

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("The patch recovery journal is invalid; no game files were changed.")]
    Invalid,
    #[error("The patch recovery journal could not be read or written safely.")]
    Io(#[from] io::Error),
    #[error("The transaction backup is missing: {0}")]
    MissingBackup(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u8,
    #[serde(rename = "transactionId")]
    pub transaction_id: String,
    pub state: String,
    #[serde(rename = "startedAt", default)]
    pub started_at: Option<String>,
    #[serde(rename = "completedAt", default)]
    pub completed_at: Option<String>,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    #[serde(rename = "type")]
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub backup: Option<String>,
    pub state: String,
}

fn relative(root: &Path, value: &str) -> Result<PathBuf, TransactionError> {
    if value.is_empty() || value.len() > MAX_STRING_BYTES || value.contains('\0') {
        return Err(TransactionError::Invalid);
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(TransactionError::Invalid);
    }
    let result = root.join(path);
    if !result.starts_with(root) {
        return Err(TransactionError::Invalid);
    }
    Ok(result)
}

fn regular(path: &Path) -> Result<fs::Metadata, TransactionError> {
    let metadata = fs::symlink_metadata(path).map_err(TransactionError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransactionError::Invalid);
    }
    #[cfg(unix)]
    if std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
        return Err(TransactionError::Invalid);
    }
    Ok(metadata)
}

fn safe_parent(path: &Path) -> Result<(), TransactionError> {
    let parent = path.parent().ok_or(TransactionError::Invalid)?;
    let metadata = fs::symlink_metadata(parent).map_err(TransactionError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TransactionError::Invalid);
    }
    Ok(())
}

pub fn validate(journal: &Journal, game_root: &Path) -> Result<(), TransactionError> {
    if journal.schema_version != 1
        || journal.transaction_id.len() > MAX_STRING_BYTES
        || journal.transaction_id.matches('-').count() != 1
        || !journal
            .transaction_id
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-')
        || journal.transaction_id.is_empty()
        || journal.operations.len() > MAX_OPERATIONS
        || !["patching", "patched"].contains(&journal.state.as_str())
    {
        return Err(TransactionError::Invalid);
    }
    let backup_root = relative(
        game_root,
        &format!(
            ".deltamod-community-patch-backups/{}",
            journal.transaction_id
        ),
    )?;
    let mut targets = std::collections::HashSet::new();
    let mut backups = std::collections::HashSet::new();
    for operation in &journal.operations {
        if !["restore", "remove"].contains(&operation.kind.as_str())
            || !["pending", "applied"].contains(&operation.state.as_str())
        {
            return Err(TransactionError::Invalid);
        }
        let target = relative(game_root, &operation.target)?;
        let target_key = if cfg!(windows) {
            target.to_string_lossy().to_lowercase()
        } else {
            target.to_string_lossy().into_owned()
        };
        if !targets.insert(target_key) {
            return Err(TransactionError::Invalid);
        }
        safe_parent(&target)?;
        if operation.kind == "restore" {
            let backup_name = operation
                .backup
                .as_deref()
                .ok_or(TransactionError::Invalid)?;
            let backup = relative(&backup_root, backup_name)?;
            let backup_key = if cfg!(windows) {
                backup.to_string_lossy().to_lowercase()
            } else {
                backup.to_string_lossy().into_owned()
            };
            if !backups.insert(backup_key) {
                return Err(TransactionError::Invalid);
            }
            if let Some(parent) = backup.parent() {
                match fs::symlink_metadata(parent) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                        return Err(TransactionError::Invalid)
                    }
                    Ok(_) => {}
                    Err(error)
                        if error.kind() == io::ErrorKind::NotFound
                            && operation.state == "pending" => {}
                    Err(error) => return Err(TransactionError::Io(error)),
                }
            }
        } else if operation.backup.is_some() {
            return Err(TransactionError::Invalid);
        }
    }
    Ok(())
}

fn journal_backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), TransactionError> {
    let parent = path.parent().ok_or(TransactionError::Invalid)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), TransactionError> {
    Ok(())
}

fn read_journal_file(path: &Path) -> Result<Journal, TransactionError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(TransactionError::Invalid);
    }
    serde_json::from_slice(&fs::read(path)?).map_err(|_| TransactionError::Invalid)
}

pub fn load_journal(path: &Path) -> Result<Option<Journal>, TransactionError> {
    let backup = journal_backup_path(path);
    if !path.exists() {
        if !backup.exists() {
            return Ok(None);
        }
        let metadata = fs::symlink_metadata(&backup)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(TransactionError::Invalid);
        }
        fs::rename(&backup, path)?;
        sync_parent(path)?;
    }
    let journal = read_journal_file(path)?;
    if backup.exists() {
        let _ = fs::remove_file(&backup);
        let _ = sync_parent(path);
    }
    Ok(Some(journal))
}

pub fn write_journal(path: &Path, journal: &Journal) -> Result<(), TransactionError> {
    let bytes = serde_json::to_vec(journal).map_err(|_| TransactionError::Invalid)?;
    let parent = path.parent().ok_or(TransactionError::Invalid)?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension("json.tmp");
    let backup = journal_backup_path(path);

    let mut file = File::create(&temp)?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.sync_all()?;
    drop(file);

    if !path.exists() && backup.exists() {
        fs::rename(&backup, path)?;
        sync_parent(path)?;
    }

    if !path.exists() {
        fs::rename(&temp, path)?;
        if let Err(error) = sync_parent(path) {
            let _ = fs::remove_file(path);
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
        return Ok(());
    }

    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    fs::rename(path, &backup)?;
    if let Err(error) = sync_parent(path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(&temp);
        return Err(TransactionError::Io(error));
    }

    // Once the new primary exists, an fsync failure is no longer a failed
    // transaction: the synced backup remains a recoverable fallback.
    if sync_parent(path).is_ok() {
        let _ = fs::remove_file(&backup);
        let _ = sync_parent(path);
    }
    Ok(())
}

pub fn backup(
    game_root: &Path,
    journal_path: &Path,
    journal: &mut Journal,
    target_name: &str,
) -> Result<(), TransactionError> {
    validate(journal, game_root)?;
    let target = relative(game_root, target_name)?;
    let backup_root = relative(
        game_root,
        &format!(
            ".deltamod-community-patch-backups/{}",
            journal.transaction_id
        ),
    )?;
    let target_exists = match fs::symlink_metadata(&target) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TransactionError::Invalid);
            }
            #[cfg(unix)]
            if std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
                return Err(TransactionError::Invalid);
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(TransactionError::Io(error)),
    };
    let operation = if target_exists {
        let backup_name = target_name.to_owned();
        Operation {
            kind: "restore".into(),
            target: target_name.into(),
            backup: Some(backup_name.clone()),
            state: "pending".into(),
        }
    } else {
        Operation {
            kind: "remove".into(),
            target: target_name.into(),
            backup: None,
            state: "applied".into(),
        }
    };
    journal.operations.push(operation);
    validate(journal, game_root)?;
    fs::create_dir_all(
        backup_root.join(Path::new(target_name).parent().unwrap_or(Path::new("."))),
    )?;
    write_journal(journal_path, journal)?;
    if target_exists {
        let backup = relative(&backup_root, target_name)?;
        if fs::symlink_metadata(&backup).is_ok() {
            return Err(TransactionError::Invalid);
        }
        safe_parent(&target)?;
        fs::rename(&target, &backup)?;
        journal
            .operations
            .last_mut()
            .ok_or(TransactionError::Invalid)?
            .state = "applied".into();
        write_journal(journal_path, journal)?;
    }
    Ok(())
}

pub fn restore(
    game_root: &Path,
    journal_path: &Path,
    journal: &mut Journal,
) -> Result<(), TransactionError> {
    validate(journal, game_root)?;
    let backup_root = relative(
        game_root,
        &format!(
            ".deltamod-community-patch-backups/{}",
            journal.transaction_id
        ),
    )?;
    while let Some(operation) = journal.operations.last().cloned() {
        let target = relative(game_root, &operation.target)?;
        if operation.kind == "restore" {
            let backup_name = operation
                .backup
                .as_deref()
                .ok_or(TransactionError::Invalid)?;
            let backup = relative(&backup_root, backup_name)?;
            if !backup.exists() {
                if operation.state == "pending" {
                    journal.operations.pop();
                    write_journal(journal_path, journal)?;
                    continue;
                }
                return Err(TransactionError::MissingBackup(backup_name.into()));
            }
            regular(&backup)?;
            if let Ok(metadata) = fs::symlink_metadata(&target) {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(TransactionError::Invalid);
                }
                #[cfg(unix)]
                if std::os::unix::fs::MetadataExt::nlink(&metadata) != 1 {
                    return Err(TransactionError::Invalid);
                }
                fs::remove_file(&target)?;
            }
            safe_parent(&target)?;
            fs::rename(&backup, &target)?;
        } else if let Ok(metadata) = fs::symlink_metadata(&target) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(TransactionError::Invalid);
            }
            regular(&target)?;
            fs::remove_file(&target)?;
        }
        journal.operations.pop();
        write_journal(journal_path, journal)?;
    }
    fs::remove_dir_all(&backup_root).ok();
    let fallback = journal_backup_path(journal_path);
    if fallback.exists() {
        fs::remove_file(&fallback)?;
    }
    fs::remove_file(journal_path)?;
    let _ = fs::remove_file(journal_path.with_extension("json.tmp"));
    let _ = sync_parent(journal_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_primary_recovers_from_backup_and_restores() {
        let root = tempfile::tempdir().unwrap();
        let game = root.path();
        let journal_path = game.join(".deltamod-community-patch-journal.json");
        let backup_root = game.join(".deltamod-community-patch-backups").join("1-1");
        fs::create_dir_all(&backup_root).unwrap();
        fs::write(game.join("data.win"), b"patched").unwrap();
        fs::write(backup_root.join("data.win"), b"original").unwrap();

        let journal = Journal {
            schema_version: 1,
            transaction_id: "1-1".into(),
            state: "patched".into(),
            started_at: None,
            completed_at: None,
            operations: vec![Operation {
                kind: "restore".into(),
                target: "data.win".into(),
                backup: Some("data.win".into()),
                state: "applied".into(),
            }],
        };
        write_journal(&journal_path, &journal).unwrap();

        let fallback = journal_backup_path(&journal_path);
        fs::rename(&journal_path, &fallback).unwrap();
        assert!(!journal_path.exists());
        assert!(fallback.exists());

        let mut recovered = load_journal(&journal_path).unwrap().unwrap();
        assert!(journal_path.exists());
        assert!(!fallback.exists());
        restore(game, &journal_path, &mut recovered).unwrap();

        assert_eq!(fs::read(game.join("data.win")).unwrap(), b"original");
        assert!(!journal_path.exists());
        assert!(!journal_backup_path(&journal_path).exists());
    }
}
