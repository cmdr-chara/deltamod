use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StablePathIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    volume_serial: u64,
    #[cfg(windows)]
    file_id: [u8; 16],
    #[cfg(not(any(unix, windows)))]
    unavailable: (),
}

impl StablePathIdentity {
    pub(crate) fn token(&self) -> String {
        #[cfg(unix)]
        {
            format!("{:016x}:{:016x}", self.device, self.inode)
        }
        #[cfg(windows)]
        {
            format!("{:016x}:{}", self.volume_serial, hex::encode(self.file_id))
        }
        #[cfg(not(any(unix, windows)))]
        {
            String::new()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedFile {
    identity: StablePathIdentity,
    size: u64,
    sha256: String,
}

impl VerifiedFile {
    pub const fn identity(&self) -> &StablePathIdentity {
        &self.identity
    }

    pub const fn size(&self) -> u64 {
        self.size
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, Error)]
pub enum SecurePathError {
    #[error("Secure path inspection is unavailable on this platform.")]
    Unsupported,
    #[error("The path is not an ordinary single-link object.")]
    Unsafe,
    #[error("The path identity changed during inspection.")]
    Changed,
    #[error("The file exceeds the configured size limit.")]
    TooLarge,
    #[error("Secure path I/O failed.")]
    Io(#[source] io::Error),
}

impl From<io::Error> for SecurePathError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn inspect_directory_identity(path: &Path) -> Result<StablePathIdentity, SecurePathError> {
    inspect_directory_identity_impl(path)
}

pub fn inspect_regular_file(path: &Path, max_bytes: u64) -> Result<VerifiedFile, SecurePathError> {
    hash_opened(open_regular(path, max_bytes)?, max_bytes)
}

pub(crate) struct PinnedRegularFile {
    #[cfg_attr(windows, allow(dead_code))]
    opened: OpenedRegular,
    verified: VerifiedFile,
}

pub(crate) fn pin_regular_file(
    path: &Path,
    max_bytes: u64,
) -> Result<PinnedRegularFile, SecurePathError> {
    let mut opened = open_regular(path, max_bytes)?;
    let verified = hash_opened_ref(&mut opened, max_bytes)?;
    Ok(PinnedRegularFile { opened, verified })
}

impl PinnedRegularFile {
    pub(crate) const fn verified(&self) -> &VerifiedFile {
        &self.verified
    }

    #[cfg(not(windows))]
    pub(crate) fn verify_unchanged(&mut self, max_bytes: u64) -> Result<(), SecurePathError> {
        let current = hash_opened_ref(&mut self.opened, max_bytes)?;
        if current != self.verified {
            return Err(SecurePathError::Changed);
        }
        Ok(())
    }

    pub(crate) fn launch_path(&self, original: &Path) -> Result<PathBuf, SecurePathError> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd as _;

            let path = PathBuf::from(format!("/proc/self/fd/{}", self.opened.file.as_raw_fd()));
            fs::metadata(&path)?;
            Ok(path)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(original.to_owned())
        }
    }
}

pub fn copy_relative_regular_file_verified(
    trusted_root: &Path,
    relative_source: &Path,
    destination: &Path,
    max_bytes: u64,
) -> Result<VerifiedFile, SecurePathError> {
    let mut source = open_relative_regular(trusted_root, relative_source, max_bytes)?;
    source.verify()?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut digest = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .filter(|value| *value <= max_bytes)
            .ok_or(SecurePathError::TooLarge)?;
        output.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
    }
    output.flush()?;
    output.sync_all()?;
    source.verify()?;
    if copied != source.size {
        return Err(SecurePathError::Changed);
    }
    drop(output);
    let copied_file = inspect_regular_file(destination, max_bytes)?;
    if copied_file.size != copied || copied_file.sha256 != hex::encode(digest.finalize()) {
        return Err(SecurePathError::Changed);
    }
    Ok(copied_file)
}

/// Copies a no-follow, single-link source into a caller-owned newly-created
/// destination handle. This lets capability-based mutation roots create the
/// destination without reopening it by pathname.
pub fn copy_relative_regular_file_to_open_file_verified(
    trusted_root: &Path,
    relative_source: &Path,
    destination: &mut File,
    max_bytes: u64,
) -> Result<(u64, String), SecurePathError> {
    let mut source = open_relative_regular(trusted_root, relative_source, max_bytes)?;
    source.verify()?;
    let mut digest = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .filter(|value| *value <= max_bytes)
            .ok_or(SecurePathError::TooLarge)?;
        destination.write_all(&buffer[..read])?;
        digest.update(&buffer[..read]);
    }
    destination.flush()?;
    destination.sync_all()?;
    source.verify()?;
    if copied != source.size {
        return Err(SecurePathError::Changed);
    }
    Ok((copied, hex::encode(digest.finalize())))
}

fn checked_relative_names(relative: &Path) -> Result<Vec<OsString>, SecurePathError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(SecurePathError::Unsafe);
    }
    let names = relative
        .components()
        .map(|component| match component {
            Component::Normal(name) if !name.is_empty() => Ok(name.to_owned()),
            _ => Err(SecurePathError::Unsafe),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if names.is_empty() {
        return Err(SecurePathError::Unsafe);
    }
    Ok(names)
}

fn hash_opened(mut opened: OpenedRegular, max_bytes: u64) -> Result<VerifiedFile, SecurePathError> {
    hash_opened_ref(&mut opened, max_bytes)
}

fn hash_opened_ref(
    opened: &mut OpenedRegular,
    max_bytes: u64,
) -> Result<VerifiedFile, SecurePathError> {
    opened.verify()?;
    opened.file.seek(SeekFrom::Start(0))?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = opened.file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .filter(|value| *value <= max_bytes)
            .ok_or(SecurePathError::TooLarge)?;
        digest.update(&buffer[..read]);
    }
    opened.verify()?;
    if total != opened.size {
        return Err(SecurePathError::Changed);
    }
    Ok(VerifiedFile {
        identity: opened.identity.clone(),
        size: total,
        sha256: hex::encode(digest.finalize()),
    })
}

struct OpenedRegular {
    file: File,
    identity: StablePathIdentity,
    size: u64,
    #[cfg(unix)]
    path: PathBuf,
    #[cfg(windows)]
    node: fence_windows::NodeHandle,
}

impl OpenedRegular {
    fn verify(&self) -> Result<(), SecurePathError> {
        verify_opened(self)
    }
}

#[cfg(unix)]
fn open_regular(path: &Path, max_bytes: u64) -> Result<OpenedRegular, SecurePathError> {
    use std::os::unix::fs::MetadataExt as _;

    let path_metadata = fs::symlink_metadata(path)?;
    if !path_metadata.is_file()
        || path_metadata.file_type().is_symlink()
        || path_metadata.nlink() != 1
    {
        return Err(SecurePathError::Unsafe);
    }
    if path_metadata.len() > max_bytes {
        return Err(SecurePathError::TooLarge);
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let identity = StablePathIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.dev() != path_metadata.dev()
        || metadata.ino() != path_metadata.ino()
        || metadata.len() != path_metadata.len()
    {
        return Err(SecurePathError::Changed);
    }
    let opened = OpenedRegular {
        file,
        identity,
        size: metadata.len(),
        path: path.to_owned(),
    };
    opened.verify()?;
    Ok(opened)
}

#[cfg(unix)]
fn open_relative_regular(
    trusted_root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> Result<OpenedRegular, SecurePathError> {
    use std::os::unix::fs::MetadataExt as _;

    let names = checked_relative_names(relative)?;
    let root = rustix::fs::open(
        trusted_root,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(unix_error)?;
    let mut directory = File::from(root);
    for name in &names[..names.len() - 1] {
        let child = rustix::fs::openat(
            &directory,
            name,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(unix_error)?;
        directory = File::from(child);
    }
    let descriptor = rustix::fs::openat(
        &directory,
        &names[names.len() - 1],
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(unix_error)?;
    let file = File::from(descriptor);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(SecurePathError::Unsafe);
    }
    if metadata.len() > max_bytes {
        return Err(SecurePathError::TooLarge);
    }
    let opened = OpenedRegular {
        file,
        identity: StablePathIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
        size: metadata.len(),
        path: trusted_root.join(relative),
    };
    opened.verify()?;
    Ok(opened)
}

#[cfg(unix)]
fn verify_opened(opened: &OpenedRegular) -> Result<(), SecurePathError> {
    use std::os::unix::fs::MetadataExt as _;

    let handle = opened.file.metadata()?;
    let path = fs::symlink_metadata(&opened.path)?;
    if !handle.is_file()
        || !path.is_file()
        || path.file_type().is_symlink()
        || handle.nlink() != 1
        || path.nlink() != 1
    {
        return Err(SecurePathError::Unsafe);
    }
    let handle_identity = StablePathIdentity {
        device: handle.dev(),
        inode: handle.ino(),
    };
    let path_identity = StablePathIdentity {
        device: path.dev(),
        inode: path.ino(),
    };
    if handle_identity != opened.identity
        || path_identity != opened.identity
        || handle.len() != opened.size
        || path.len() != opened.size
    {
        return Err(SecurePathError::Changed);
    }
    Ok(())
}

#[cfg(windows)]
fn open_regular(path: &Path, max_bytes: u64) -> Result<OpenedRegular, SecurePathError> {
    use fence_windows::{NodeKind, RootHandle};
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let path_metadata = fs::symlink_metadata(path)?;
    if !path_metadata.is_file()
        || path_metadata.file_type().is_symlink()
        || path_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(SecurePathError::Unsafe);
    }
    let parent = path.parent().ok_or(SecurePathError::Unsafe)?;
    let name = path.file_name().ok_or(SecurePathError::Unsafe)?;
    let root = RootHandle::open(parent).map_err(windows_error)?;
    let node = root
        .directory()
        .open_named_child(name)
        .map_err(windows_error)?;
    let metadata = node.metadata();
    if metadata.kind != NodeKind::RegularFile || metadata.link_count != 1 {
        return Err(SecurePathError::Unsafe);
    }
    if metadata.size > max_bytes {
        return Err(SecurePathError::TooLarge);
    }
    node.verify_path_identity().map_err(windows_error)?;
    let file = node.try_clone_file().map_err(windows_error)?;
    let opened = OpenedRegular {
        file,
        identity: StablePathIdentity {
            volume_serial: metadata.identity.volume_serial,
            file_id: metadata.identity.file_id,
        },
        size: metadata.size,
        node,
    };
    opened.verify()?;
    Ok(opened)
}

#[cfg(windows)]
fn open_relative_regular(
    trusted_root: &Path,
    relative: &Path,
    max_bytes: u64,
) -> Result<OpenedRegular, SecurePathError> {
    use fence_windows::{NodeKind, RootHandle};

    let names = checked_relative_names(relative)?;
    let mut directory = RootHandle::open(trusted_root)
        .map_err(windows_error)?
        .into_directory();
    for name in &names[..names.len() - 1] {
        directory = directory
            .open_named_child(name)
            .map_err(windows_error)?
            .into_directory()
            .map_err(windows_error)?;
    }
    let node = directory
        .open_named_child(&names[names.len() - 1])
        .map_err(windows_error)?;
    let metadata = node.metadata();
    if metadata.kind != NodeKind::RegularFile || metadata.link_count != 1 {
        return Err(SecurePathError::Unsafe);
    }
    if metadata.size > max_bytes {
        return Err(SecurePathError::TooLarge);
    }
    node.verify_path_identity().map_err(windows_error)?;
    let file = node.try_clone_file().map_err(windows_error)?;
    let opened = OpenedRegular {
        file,
        identity: StablePathIdentity {
            volume_serial: metadata.identity.volume_serial,
            file_id: metadata.identity.file_id,
        },
        size: metadata.size,
        node,
    };
    opened.verify()?;
    Ok(opened)
}

#[cfg(windows)]
fn verify_opened(opened: &OpenedRegular) -> Result<(), SecurePathError> {
    use fence_windows::NodeKind;

    let metadata = opened.node.refresh_metadata().map_err(windows_error)?;
    let identity = StablePathIdentity {
        volume_serial: metadata.identity.volume_serial,
        file_id: metadata.identity.file_id,
    };
    if metadata.kind != NodeKind::RegularFile || metadata.link_count != 1 {
        return Err(SecurePathError::Unsafe);
    }
    if identity != opened.identity || metadata.size != opened.size {
        return Err(SecurePathError::Changed);
    }
    opened
        .node
        .verify_path_identity()
        .map_err(|_| SecurePathError::Changed)
}

#[cfg(unix)]
fn inspect_directory_identity_impl(path: &Path) -> Result<StablePathIdentity, SecurePathError> {
    use std::os::unix::fs::MetadataExt as _;

    let before = fs::symlink_metadata(path)?;
    if !before.is_dir() || before.file_type().is_symlink() {
        return Err(SecurePathError::Unsafe);
    }
    let file = File::open(path)?;
    let opened = file.metadata()?;
    let after = fs::symlink_metadata(path)?;
    if !opened.is_dir()
        || !after.is_dir()
        || after.file_type().is_symlink()
        || opened.dev() != before.dev()
        || opened.ino() != before.ino()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
    {
        return Err(SecurePathError::Changed);
    }
    Ok(StablePathIdentity {
        device: opened.dev(),
        inode: opened.ino(),
    })
}

#[cfg(windows)]
fn inspect_directory_identity_impl(path: &Path) -> Result<StablePathIdentity, SecurePathError> {
    use fence_windows::{NodeKind, RootHandle};
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let before = fs::symlink_metadata(path)?;
    if !before.is_dir()
        || before.file_type().is_symlink()
        || before.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(SecurePathError::Unsafe);
    }
    let first = RootHandle::open(path).map_err(windows_error)?;
    let metadata = first.directory().metadata();
    if metadata.kind != NodeKind::Directory {
        return Err(SecurePathError::Unsafe);
    }
    let identity = StablePathIdentity {
        volume_serial: metadata.identity.volume_serial,
        file_id: metadata.identity.file_id,
    };
    let after = fs::symlink_metadata(path)?;
    if !after.is_dir()
        || after.file_type().is_symlink()
        || after.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(SecurePathError::Unsafe);
    }
    let second = RootHandle::open(path).map_err(windows_error)?;
    let current = second.directory().metadata();
    let current_identity = StablePathIdentity {
        volume_serial: current.identity.volume_serial,
        file_id: current.identity.file_id,
    };
    if current.kind != NodeKind::Directory || current_identity != identity {
        return Err(SecurePathError::Changed);
    }
    Ok(identity)
}

#[cfg(not(any(unix, windows)))]
fn open_regular(_: &Path, _: u64) -> Result<OpenedRegular, SecurePathError> {
    Err(SecurePathError::Unsupported)
}

#[cfg(not(any(unix, windows)))]
fn open_relative_regular(_: &Path, _: &Path, _: u64) -> Result<OpenedRegular, SecurePathError> {
    Err(SecurePathError::Unsupported)
}

#[cfg(not(any(unix, windows)))]
fn verify_opened(_: &OpenedRegular) -> Result<(), SecurePathError> {
    Err(SecurePathError::Unsupported)
}

#[cfg(not(any(unix, windows)))]
fn inspect_directory_identity_impl(_: &Path) -> Result<StablePathIdentity, SecurePathError> {
    Err(SecurePathError::Unsupported)
}

#[cfg(windows)]
fn windows_error(error: fence_windows::WindowsError) -> SecurePathError {
    SecurePathError::Io(io::Error::other(error.to_string()))
}

#[cfg(unix)]
fn unix_error(error: rustix::io::Errno) -> SecurePathError {
    SecurePathError::Io(io::Error::from_raw_os_error(error.raw_os_error()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_copy_preserves_content_and_identity_is_stable() {
        let root = tempfile::tempdir().unwrap();
        let source_root = root.path().join("source-root");
        fs::create_dir(&source_root).unwrap();
        let source = source_root.join("source");
        let destination = root.path().join("destination");
        fs::write(&source, b"content").unwrap();
        let copied = copy_relative_regular_file_verified(
            &source_root,
            Path::new("source"),
            &destination,
            64,
        )
        .unwrap();
        assert_eq!(copied.sha256(), hex::encode(Sha256::digest(b"content")));
        assert_eq!(copied, inspect_regular_file(&destination, 64).unwrap());
    }

    #[test]
    fn verified_copy_can_write_through_a_caller_owned_handle() {
        let root = tempfile::tempdir().unwrap();
        let source_root = root.path().join("source-root");
        fs::create_dir(&source_root).unwrap();
        fs::write(source_root.join("source"), b"content").unwrap();
        let destination = root.path().join("destination");
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .unwrap();
        let (size, sha256) = copy_relative_regular_file_to_open_file_verified(
            &source_root,
            Path::new("source"),
            &mut output,
            64,
        )
        .unwrap();
        assert_eq!(size, 7);
        assert_eq!(sha256, hex::encode(Sha256::digest(b"content")));
        assert_eq!(fs::read(destination).unwrap(), b"content");
    }

    #[test]
    fn relative_copy_rejects_parent_link_escape() {
        let root = tempfile::tempdir().unwrap();
        let trusted = root.path().join("trusted");
        let outside = root.path().join("outside");
        fs::create_dir(&trusted).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret"), b"secret").unwrap();
        let link = trusted.join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(&outside, &link) {
            if error.raw_os_error() == Some(1314)
                || matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                )
            {
                return;
            }
            panic!("directory-link setup failed unexpectedly: {error}");
        }

        let destination = root.path().join("destination");
        assert!(copy_relative_regular_file_verified(
            &trusted,
            Path::new("link/secret"),
            &destination,
            64,
        )
        .is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn regular_file_inspection_rejects_hardlinks() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let alias = root.path().join("alias");
        fs::write(&original, b"content").unwrap();
        match fs::hard_link(&original, &alias) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                ) =>
            {
                return;
            }
            Err(error) => panic!("hard-link setup failed unexpectedly: {error}"),
        }
        assert!(matches!(
            inspect_regular_file(&original, 64),
            Err(SecurePathError::Unsafe)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_replacement_changes_verified_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("file");
        fs::write(&path, b"same").unwrap();
        let before = inspect_regular_file(&path, 64).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"same").unwrap();
        let after = inspect_regular_file(&path, 64).unwrap();
        assert_ne!(before.identity(), after.identity());
    }
}
