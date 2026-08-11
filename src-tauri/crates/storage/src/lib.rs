#![forbid(unsafe_code)]
//! Safe, crash-resistant storage primitives. Windows replacement uses the
//! standard-library rename operation; unlike MoveFileEx with replace semantics,
//! this cannot guarantee replacement when another process holds the destination.

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
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
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
    let json = text
        .split_once("##")
        .map_or(text, |(head, _)| head)
        .trim_end();
    Ok(serde_json::from_str(json)?)
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
    let parent = path
        .parent()
        .ok_or_else(|| StorageError::InvalidPath(path.display().to_string()))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| StorageError::InvalidPath(path.display().to_string()))?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(".{name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut f = io.create_temp(&temp)?;
        f.write_all(data)?;
        f.flush()?;
        io.sync_file(&f)?;
        drop(f);
        if backup && path.exists() {
            fs::copy(path, PathBuf::from(format!("{}.backup", path.display())))?;
        }
        io.replace(&temp, path)?;
        io.sync_parent(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

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
    parse_legacy_json(&fs::read(path)?)
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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TestDir {
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
