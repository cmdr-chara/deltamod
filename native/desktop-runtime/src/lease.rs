// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
//! Cooperative, kernel-held exclusion for one managed data root.
//!
//! The file stays in place after exit. Never unlink it to reclaim a "stale PID":
//! the operating system releases the lock when the owning handle closes. This
//! protects cooperating versions, not older binaries that never take the lease.
use std::{fs::{self, File, OpenOptions}, io, path::Path};

const NAME: &str = ".deltamod-runtime.lock";

pub struct RuntimeLease {
    _file: File,
}

impl RuntimeLease {
    /// The caller must establish the managed-root ownership contract first.
    /// Lock acquisition occurs before recovery, profile migration or mutation.
    pub fn acquire(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid managed root"));
        }
        let path = root.join(NAME);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
            // O_NOFOLLOW: platform headers differ. Supported desktop targets
            // are Linux and macOS. Never follow a final-component symlink.
            #[cfg(target_os = "linux")]
            options.custom_flags(0x20000);
            #[cfg(target_os = "macos")]
            options.custom_flags(0x100);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // OPEN_REPARSE_POINT, and FILE_SHARE_READ | FILE_SHARE_WRITE only.
            // Do not allow deletion/replacement while the lease handle is open.
            options.custom_flags(0x00200000).share_mode(0x00000003);
        }
        let file = options.open(&path)?;
        let metadata = file.metadata()?;
        let link = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || link.file_type().is_symlink() || metadata.len() != 0 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "invalid runtime lease file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let owner = fs::metadata(&root)?.uid();
            if metadata.uid() != owner || metadata.nlink() != 1 || metadata.mode() & 0o077 != 0
                || metadata.dev() != link.dev() || metadata.ino() != link.ino() {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "runtime lease is not private"));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "runtime lease is a reparse point"));
            }
        }
        file.try_lock().map_err(|_| io::Error::new(io::ErrorKind::WouldBlock,
            "managed data root is already in use or cannot be locked"))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let root = std::env::temp_dir().join(format!("deltamod-lease-{}-{time}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Root { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn excludes_a_second_handle_and_releases_without_unlinking() {
        let root = Root::new();
        let first = RuntimeLease::acquire(&root.0).unwrap();
        assert!(RuntimeLease::acquire(&root.0).is_err());
        drop(first);
        assert!(root.0.join(NAME).is_file());
        let _again = RuntimeLease::acquire(&root.0).unwrap();
    }

    #[test]
    fn never_truncates_an_existing_file() {
        let root = Root::new();
        fs::write(root.0.join(NAME), b"not a runtime lease").unwrap();
        assert!(RuntimeLease::acquire(&root.0).is_err());
        assert_eq!(fs::read(root.0.join(NAME)).unwrap(), b"not a runtime lease");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_links_and_group_readable_lease_files() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = Root::new();
        fs::write(root.0.join("other"), b"").unwrap();
        symlink(root.0.join("other"), root.0.join(NAME)).unwrap();
        assert!(RuntimeLease::acquire(&root.0).is_err());
        fs::remove_file(root.0.join(NAME)).unwrap();
        fs::write(root.0.join(NAME), b"").unwrap();
        fs::set_permissions(root.0.join(NAME), fs::Permissions::from_mode(0o640)).unwrap();
        assert!(RuntimeLease::acquire(&root.0).is_err());
    }

    #[test]
    fn lease_child_probe() {
        if let Some(root) = std::env::var_os("DELTAMOD_LEASE_TEST_ROOT") {
            assert!(RuntimeLease::acquire(Path::new(&root)).is_err());
        }
    }

    #[test]
    fn another_process_is_excluded() {
        let root = Root::new();
        let _lease = RuntimeLease::acquire(&root.0).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("lease_child_probe")
            .env("DELTAMOD_LEASE_TEST_ROOT", &root.0)
            .status().unwrap();
        assert!(status.success());
    }
}
