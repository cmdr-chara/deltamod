#![forbid(unsafe_code)]
//! Safe, crash-resistant storage primitives. Windows replacement uses the
//! standard-library rename operation; unlike MoveFileEx with replace semantics,
//! this cannot guarantee replacement when another process holds the destination.

use deltamod_tools_runtime::{
    copy_relative_regular_file_to_open_file_verified, inspect_regular_file_size,
    read_relative_regular_file, SecurePathError,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("invalid journal: {0}")]
    InvalidJournal(String),
    #[error("unsafe storage file: {0}")]
    UnsafeFile(#[from] SecurePathError),
    #[error("temporary file collision")]
    TempCollision,
    #[error("replacement is not supported by this platform/filesystem: {0}")]
    Replacement(String),
}

pub trait FaultIo {
    fn create_temp(&self, path: &Path) -> Result<File, StorageError>;
    fn sync_file(&self, file: &File) -> Result<(), StorageError>;
    fn replace(&self, from: &Path, to: &Path) -> Result<(), StorageError>;
    fn sync_parent(&self, parent: &Path) -> Result<(), StorageError>;
}

#[derive(Default)]
pub struct StdFaultIo;
impl FaultIo for StdFaultIo {
    fn create_temp(&self, path: &Path) -> Result<File, StorageError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                StorageError::TempCollision
            } else {
                e.into()
            }
        })
    }

    fn sync_file(&self, file: &File) -> Result<(), StorageError> {
        file.sync_all().map_err(Into::into)
    }
    fn replace(&self, from: &Path, to: &Path) -> Result<(), StorageError> {
        fs::rename(from, to).map_err(Into::into)
    }
    fn sync_parent(&self, parent: &Path) -> Result<(), StorageError> {
        // Directory fsync is not portable on Windows. Best effort is deliberate.
        #[cfg(not(windows))]
        {
            File::open(parent)?.sync_all()?;
        }
        #[cfg(windows)]
        let _ = parent;
        Ok(())
    }
}

pub fn parse_legacy_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StorageError> {
    let text = std::str::from_utf8(bytes).map_err(|e| StorageError::InvalidPath(e.to_string()))?;
    // A legacy suffix starts only after a complete JSON value. Splitting on
    // the first marker corrupts otherwise valid user strings containing ##.
    let mut values = serde_json::Deserializer::from_str(text).into_iter::<T>();
    let value = match values.next() {
        Some(value) => value?,
        None => return Ok(serde_json::from_str(text)?),
    };
    let suffix = text[values.byte_offset()..].trim_start();
    if !suffix.starts_with("##") {
        serde_json::Deserializer::from_str(suffix).end()?;
    }
    Ok(value)
}

pub fn atomic_write_bytes(path: &Path, data: &[u8], backup: bool) -> Result<(), StorageError> {
    atomic_write_with(&StdFaultIo, path, data, backup)
}
pub fn atomic_write_json<T: Serialize>(
    path: &Path,
    value: &T,
    backup: bool,
) -> Result<(), StorageError> {
    atomic_write_bytes(path, &serde_json::to_vec_pretty(value)?, backup)
}
pub fn atomic_write_with<I: FaultIo>(
    io: &I,
    path: &Path,
    data: &[u8],
    backup: bool,
) -> Result<(), StorageError> {
    let absolute = absolute_storage_path(path)?;
    let path = absolute.as_path();
    let parent = storage_parent(path)?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| StorageError::InvalidPath("missing filename".into()))?;
    let source_size = ordinary_size(path)?;
    let backup_path = {
        let mut name = path.as_os_str().to_owned();
        name.push(".backup");
        PathBuf::from(name)
    };
    if backup && source_size.is_some() {
        ordinary_size(&backup_path)?;
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".{}.tmp-{}-{nonce}",
        name.to_string_lossy(),
        std::process::id()
    ));
    stage_file(
        io,
        &temp,
        |file| {
            file.write_all(data)?;
            Ok(())
        },
        || {
            if let Some(size) = source_size.filter(|_| backup) {
                let backup_temp = parent.join(format!(
                    ".{}.backup-tmp-{}-{nonce}",
                    name.to_string_lossy(),
                    std::process::id()
                ));
                // Never copy into the live backup path: it may alias the original
                // file or another user's data. Publish an independently owned inode.
                stage_file(
                    io,
                    &backup_temp,
                    |file| {
                        copy_relative_regular_file_to_open_file_verified(
                            parent,
                            Path::new(name),
                            file,
                            size,
                        )?;
                        Ok(())
                    },
                    || {
                        ordinary_size(&backup_path)?;
                        io.replace(&backup_temp, &backup_path)?;
                        io.sync_parent(parent)
                    },
                )?;
            }
            ordinary_size(path)?;
            io.replace(&temp, path)?;
            io.sync_parent(parent)
        },
    )
}

fn absolute_storage_path(path: &Path) -> Result<PathBuf, StorageError> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    if path
        .components()
        .any(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
    {
        return Err(StorageError::InvalidPath(
            "ambiguous drive-relative storage path".into(),
        ));
    }
    Ok(std::env::current_dir()?.join(path))
}

fn storage_parent(path: &Path) -> Result<&Path, StorageError> {
    if path.file_name().is_none() {
        return Err(StorageError::InvalidPath("missing filename".into()));
    }
    Ok(path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new(".")))
}

fn ordinary_size(path: &Path) -> Result<Option<u64>, StorageError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(metadata) => {
            let size = inspect_regular_file_size(path, metadata.len())?;
            Ok(Some(size))
        }
    }
}

fn stage_file<I: FaultIo>(
    io: &I,
    temp: &Path,
    write: impl FnOnce(&mut File) -> Result<(), StorageError>,
    publish: impl FnOnce() -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    // Failure to create is not ownership. In particular, never unlink a file
    // which caused create_new to report an existing-name collision.
    let mut file = io.create_temp(temp)?;
    let result = (|| {
        write(&mut file)?;
        file.flush()?;
        io.sync_file(&file)
    })();
    drop(file);
    let result = result.and_then(|_| publish());
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

/// Persisted JSON is bounded before allocation, including legacy-suffixed JSON.
pub const MAX_STORED_JSON_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct InstallationRecord {
    pub index: Option<u32>,
    pub pid: Option<String>,
    pub name: Option<String>,
    pub steam: Option<bool>,
    pub valid: Option<bool>,
    pub issues: Option<Vec<String>>,
    pub can_open_in_undertale_mod_tool: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ProfileStore {
    pub installations: Vec<InstallationRecord>,
    pub current_index: Option<u32>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T, StorageError> {
    // Preserve the ordinary NotFound error for callers treating missing state
    // as first run, while links, hardlinks and oversized records fail closed.
    let absolute = absolute_storage_path(path)?;
    let path = absolute.as_path();
    fs::symlink_metadata(path)?;
    let parent = storage_parent(path)?;
    let name = Path::new(path.file_name().unwrap());
    parse_legacy_json(&read_relative_regular_file(
        parent,
        name,
        MAX_STORED_JSON_BYTES,
    )?)
}
pub fn save_json<T: Serialize>(path: &Path, value: &T, backup: bool) -> Result<(), StorageError> {
    atomic_write_json(path, value, backup)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRoot {
    pub root: PathBuf,
}
impl DataRoot {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        if fs::symlink_metadata(&root)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(StorageError::InvalidPath("symlink root".into()));
        }
        fs::create_dir_all(&root)?;
        Ok(Self {
            root: fs::canonicalize(root)?,
        })
    }
    pub fn profiles(&self) -> PathBuf {
        self.root.join("profiles")
    }
    pub fn installations(&self) -> PathBuf {
        self.profiles().join("installations.json")
    }
    pub fn journal(&self) -> PathBuf {
        self.root.join("migration.journal.json")
    }
    pub fn contained(&self, relative: &str) -> Result<PathBuf, StorageError> {
        validate_relative_path(relative)?;
        let p = self.root.join(relative);
        let parent = p.parent().unwrap_or(&self.root);
        let c = fs::canonicalize(parent)
            .unwrap_or_else(|_| parent.to_path_buf())
            .join(p.file_name().unwrap());
        if c.starts_with(&self.root) {
            Ok(c)
        } else {
            Err(StorageError::InvalidPath(relative.into()))
        }
    }
}

pub fn validate_relative_path(s: &str) -> Result<(), StorageError> {
    let p = Path::new(s);
    if s.is_empty()
        || p.is_absolute()
        || s.starts_with('\\')
        || s.starts_with("//")
        || s.contains(':')
    {
        return Err(StorageError::InvalidPath(s.into()));
    }
    if p.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StorageError::InvalidPath(s.into()));
    }
    Ok(())
}
pub fn validate_operation_id(s: &str) -> Result<(), StorageError> {
    if s.is_empty()
        || s.len() > 128
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        Err(StorageError::InvalidPath(s.into()))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JournalStatus {
    Prepared,
    Applied,
    Aborted,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationOp {
    pub operation_id: String,
    pub source: String,
    pub destination: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationJournal {
    pub version: u32,
    pub status: JournalStatus,
    pub operations: Vec<MigrationOp>,
}
pub fn validate_journal(j: &MigrationJournal, root: &DataRoot) -> Result<(), StorageError> {
    if j.version != 1 || j.operations.is_empty() {
        return Err(StorageError::InvalidJournal("version or operations".into()));
    }
    for op in &j.operations {
        validate_operation_id(&op.operation_id)?;
        let a = root.contained(&op.source)?;
        let b = root.contained(&op.destination)?;
        if a == b {
            return Err(StorageError::InvalidJournal("same path".into()));
        }
    }
    Ok(())
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    Copy {
        source: PathBuf,
        destination: PathBuf,
    },
}
pub fn recovery_plan(
    j: &MigrationJournal,
    root: &DataRoot,
) -> Result<Vec<RecoveryAction>, StorageError> {
    validate_journal(j, root)?;
    Ok(j.operations
        .iter()
        .map(|o| RecoveryAction::Copy {
            source: root.contained(&o.source).unwrap(),
            destination: root.contained(&o.destination).unwrap(),
        })
        .collect())
}

#[cfg(test)]
mod hardening_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    pub(super) struct TestDir(pub(super) PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    pub(super) fn tempdir() -> TestDir {
        let path = std::env::temp_dir().join(format!(
            "deltamod-storage-test-{}-{}",
            std::process::id(),
            NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        TestDir(path)
    }
    #[test]
    fn suffix_and_malformed() {
        let x: Value = parse_legacy_json(br#"{"a":1}## trailing"#).unwrap();
        assert_eq!(x["a"], 1);
        assert!(parse_legacy_json::<Value>(b"{").is_err());
    }
    #[test]
    fn backup_and_no_backup() {
        let d = tempdir();
        let p = d.0.join("a.json");
        atomic_write_bytes(&p, b"one", true).unwrap();
        atomic_write_bytes(&p, b"two", true).unwrap();
        assert_eq!(fs::read(format!("{}.backup", p.display())).unwrap(), b"one");
        let q = d.0.join("q");
        atomic_write_bytes(&q, b"x", false).unwrap();
        assert!(!PathBuf::from(format!("{}.backup", q.display())).exists());
    }
    #[test]
    fn paths_and_root() {
        for p in [
            "../x",
            "C:\\x",
            "\\\\server\\x",
            "//server/x",
            "\\\\?\\C:\\x",
        ] {
            assert!(validate_relative_path(p).is_err());
        }
        let d = tempdir();
        let r = DataRoot::new(&d.0).unwrap();
        assert!(r.contained("profiles/a").is_ok());
    }
    #[test]
    fn schema_round_trip() {
        let x: ProfileStore =
            serde_json::from_str(r#"{"installations":[{"index":1,"new":true}],"other":"x"}"#)
                .unwrap();
        assert_eq!(x.extra["other"], "x");
        assert_eq!(x.installations[0].extra["new"], true);
    }
    #[test]
    fn journal_is_pure_and_validated() {
        let d = tempdir();
        let r = DataRoot::new(&d.0).unwrap();
        let j = MigrationJournal {
            version: 1,
            status: JournalStatus::Prepared,
            operations: vec![MigrationOp {
                operation_id: "x".into(),
                source: "a".into(),
                destination: "b".into(),
            }],
        };
        assert_eq!(recovery_plan(&j, &r).unwrap().len(), 1);
        assert!(!r.journal().exists());
    }
    #[test]
    fn invalid_status_is_rejected() {
        assert!(serde_json::from_str::<MigrationJournal>(
            r#"{"version":1,"status":"Interrupted","operations":[]}"#
        )
        .is_err());
    }
    struct CollisionIo;
    impl FaultIo for CollisionIo {
        fn create_temp(&self, _: &Path) -> Result<File, StorageError> {
            Err(StorageError::TempCollision)
        }
        fn sync_file(&self, _: &File) -> Result<(), StorageError> {
            Ok(())
        }
        fn replace(&self, _: &Path, _: &Path) -> Result<(), StorageError> {
            Ok(())
        }
        fn sync_parent(&self, _: &Path) -> Result<(), StorageError> {
            Ok(())
        }
    }
    #[test]
    fn injected_temp_collision_leaves_no_target() {
        let d = tempdir();
        let path = d.0.join("target");
        assert!(matches!(
            atomic_write_with(&CollisionIo, &path, b"x", false),
            Err(StorageError::TempCollision)
        ));
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&d.0).unwrap().count(), 0);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_root_is_rejected() {
        let d = tempdir();
        let real = d.0.join("real");
        let link = d.0.join("link");
        fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(DataRoot::new(link).is_err());
    }
}
