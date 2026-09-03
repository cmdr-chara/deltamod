use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::Arc;
use std::time::SystemTime;

#[cfg(windows)]
use std::collections::BTreeMap;

use thiserror::Error;

use crate::path_security::validate_relative_path;

pub const JS_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_SOURCE_ENTRIES: usize = 50_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Directory,
    File,
}

#[derive(Clone, Debug)]
pub struct CopyEntry {
    pub kind: EntryKind,
    pub relative: String,
    pub size: u64,
    snapshot: Snapshot,
    #[cfg(windows)]
    windows_entry: fence_windows::DirectoryEntry,
    #[cfg(windows)]
    windows_parent: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CopyInventory {
    pub source_root: PathBuf,
    pub entries: Vec<CopyEntry>,
    pub file_count: u64,
    pub total_bytes: u64,
    root_snapshot: Snapshot,
    #[cfg(windows)]
    source_directories: BTreeMap<PathBuf, WindowsSourceDirectory>,
}

#[cfg(windows)]
#[derive(Clone, Debug)]
struct WindowsSourceDirectory {
    handle: Arc<fence_windows::DirectoryHandle>,
    entries: BTreeMap<std::ffi::OsString, fence_windows::DirectoryEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Snapshot {
    identity: Identity,
    len: u64,
    modified: Option<SystemTime>,
    kind: EntryKind,
    links: u64,
}

#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(windows)]
type Identity = fence_windows::FileIdentity;
#[cfg(not(any(unix, windows)))]
type Identity = ();

#[derive(Debug, Error)]
pub enum StagedCopyError {
    #[error("The import was cancelled.")]
    Cancelled,
    #[error("The selected source is not a directory.")]
    SourceNotDirectory,
    #[error("Cannot read the source directory safely.")]
    SourceUnreadable(#[source] io::Error),
    #[error("Linked or reparse-point source entries are not imported.")]
    SourceLinkBlocked,
    #[error("Unsupported source entry.")]
    SourceEntryUnsupported,
    #[error("Unsafe source entry name.")]
    SourceUnsafeName,
    #[error("The source changed during copying.")]
    SourceChanged,
    #[error("Source size or count exceeds the supported limit.")]
    SourceOverflow,
    #[error("The destination already exists.")]
    DestinationExists,
    #[error("The destination parent changed during copying.")]
    DestinationParentChanged,
    #[error("The staging path already exists or is unsafe.")]
    StagingCollision,
    #[error("The staged directory cannot be committed atomically.")]
    NonAtomicCommit,
    #[error("Failed to copy source data.")]
    CopyFailed(#[source] io::Error),
}

impl StagedCopyError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "COPY_CANCELLED",
            Self::SourceNotDirectory => "SOURCE_NOT_DIRECTORY",
            Self::SourceUnreadable(_) => "SOURCE_UNREADABLE",
            Self::SourceLinkBlocked => "SOURCE_LINK_BLOCKED",
            Self::SourceEntryUnsupported => "SOURCE_ENTRY_UNSUPPORTED",
            Self::SourceUnsafeName => "SOURCE_ENTRY_UNSUPPORTED",
            Self::SourceChanged => "SOURCE_CHANGED",
            Self::SourceOverflow => "SOURCE_OVERFLOW",
            Self::DestinationExists => "DESTINATION_EXISTS",
            Self::DestinationParentChanged => "DESTINATION_PARENT_CHANGED",
            Self::StagingCollision => "STAGING_COLLISION",
            Self::NonAtomicCommit => "NON_ATOMIC_COMMIT",
            Self::CopyFailed(_) => "COPY_FAILED",
        }
    }
}

#[cfg(not(windows))]
pub fn inspect_source_tree(source_root: &Path) -> Result<CopyInventory, StagedCopyError> {
    let source_root = source_root.to_path_buf();
    let root_metadata =
        fs::symlink_metadata(&source_root).map_err(StagedCopyError::SourceUnreadable)?;
    if root_metadata.file_type().is_symlink()
        || is_reparse_point(&root_metadata)
        || !root_metadata.is_dir()
    {
        return Err(StagedCopyError::SourceNotDirectory);
    }
    let root_snapshot = snapshot(&source_root, &root_metadata, EntryKind::Directory)?;
    let mut entries = Vec::new();
    let mut queue = vec![(source_root.clone(), PathBuf::new())];
    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;

    while let Some((directory, relative_dir)) = queue.pop() {
        let children = fs::read_dir(&directory).map_err(StagedCopyError::SourceUnreadable)?;
        for child in children {
            let child = child.map_err(StagedCopyError::SourceUnreadable)?;
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| StagedCopyError::SourceUnsafeName)?;
            validate_name(&name)?;
            let relative_path = relative_dir.join(&name);
            let relative = relative_path
                .to_str()
                .ok_or(StagedCopyError::SourceUnsafeName)?
                .replace('\\', "/");
            if relative.len() > 32_768 || entries.len() >= MAX_SOURCE_ENTRIES {
                return Err(StagedCopyError::SourceOverflow);
            }
            let path = child.path();
            let metadata = metadata_without_links(&path)?;
            if metadata.is_dir() {
                let item_snapshot = snapshot(&path, &metadata, EntryKind::Directory)?;
                entries.push(CopyEntry {
                    kind: EntryKind::Directory,
                    relative,
                    size: 0,
                    snapshot: item_snapshot,
                });
                queue.push((path, relative_path));
            } else if metadata.is_file() {
                if link_count(&path, &metadata)? > 1 {
                    return Err(StagedCopyError::SourceLinkBlocked);
                }
                (file_count, total_bytes) =
                    checked_file_totals(file_count, total_bytes, metadata.len())?;
                let size = metadata.len();
                entries.push(CopyEntry {
                    kind: EntryKind::File,
                    relative,
                    size,
                    snapshot: snapshot(&path, &metadata, EntryKind::File)?,
                });
            } else {
                return Err(StagedCopyError::SourceEntryUnsupported);
            }
        }
    }
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(CopyInventory {
        source_root,
        entries,
        file_count,
        total_bytes,
        root_snapshot,
    })
}

#[cfg(windows)]
pub fn inspect_source_tree(source_root: &Path) -> Result<CopyInventory, StagedCopyError> {
    use fence_windows::{NodeKind, RootHandle};

    let source_root = source_root.to_path_buf();
    let root_metadata =
        fs::symlink_metadata(&source_root).map_err(StagedCopyError::SourceUnreadable)?;
    if root_metadata.file_type().is_symlink()
        || is_reparse_point(&root_metadata)
        || !root_metadata.is_dir()
    {
        return Err(StagedCopyError::SourceNotDirectory);
    }
    let root = RootHandle::open(&source_root).map_err(map_windows_source_error)?;
    let root_native = root.directory().metadata();
    let root_snapshot = snapshot_with_native(
        &root_metadata,
        EntryKind::Directory,
        root_native.identity,
        root_native.link_count,
        root_native.size,
    );
    let root_handle = Arc::new(root.into_directory());
    let mut entries = Vec::new();
    let mut source_directories = BTreeMap::new();
    let mut queue = vec![(PathBuf::new(), root_handle)];
    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;

    while let Some((relative_dir, directory)) = queue.pop() {
        let children = directory.entries().map_err(map_windows_source_error)?;
        let mut indexed_children = BTreeMap::new();
        for child in children {
            let name = child
                .name
                .clone()
                .into_string()
                .map_err(|_| StagedCopyError::SourceUnsafeName)?;
            validate_name(&name)?;
            let relative_path = relative_dir.join(&name);
            let relative = relative_path
                .to_str()
                .ok_or(StagedCopyError::SourceUnsafeName)?
                .replace('\\', "/");
            if relative.len() > 32_768 || entries.len() >= MAX_SOURCE_ENTRIES {
                return Err(StagedCopyError::SourceOverflow);
            }

            let node = directory
                .open_child(&child)
                .map_err(map_windows_source_error)?;
            let native = node.metadata();
            if native.kind == NodeKind::ReparsePoint {
                return Err(StagedCopyError::SourceLinkBlocked);
            }
            let path = source_root.join(&relative_path);
            let metadata =
                fs::symlink_metadata(&path).map_err(StagedCopyError::SourceUnreadable)?;
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                return Err(StagedCopyError::SourceLinkBlocked);
            }

            let kind = match native.kind {
                NodeKind::Directory if metadata.is_dir() => EntryKind::Directory,
                NodeKind::RegularFile if metadata.is_file() => EntryKind::File,
                _ => return Err(StagedCopyError::SourceEntryUnsupported),
            };
            if kind == EntryKind::File && native.link_count > 1 {
                return Err(StagedCopyError::SourceLinkBlocked);
            }
            if kind == EntryKind::File {
                (file_count, total_bytes) =
                    checked_file_totals(file_count, total_bytes, native.size)?;
            }
            let size = if kind == EntryKind::File {
                native.size
            } else {
                0
            };
            let snapshot = snapshot_with_native(
                &metadata,
                kind.clone(),
                native.identity,
                native.link_count,
                native.size,
            );
            entries.push(CopyEntry {
                kind: kind.clone(),
                relative,
                size,
                snapshot,
                windows_entry: child.clone(),
                windows_parent: relative_dir.clone(),
            });
            indexed_children.insert(child.name.clone(), child);
            if kind == EntryKind::Directory {
                let child_directory = node.into_directory().map_err(map_windows_source_error)?;
                queue.push((relative_path, Arc::new(child_directory)));
            }
        }
        source_directories.insert(
            relative_dir,
            WindowsSourceDirectory {
                handle: directory,
                entries: indexed_children,
            },
        );
    }
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(CopyInventory {
        source_root,
        entries,
        file_count,
        total_bytes,
        root_snapshot,
        source_directories,
    })
}

fn checked_file_totals(
    file_count: u64,
    total_bytes: u64,
    file_size: u64,
) -> Result<(u64, u64), StagedCopyError> {
    let file_count = file_count
        .checked_add(1)
        .filter(|value| *value <= JS_MAX_SAFE_INTEGER)
        .ok_or(StagedCopyError::SourceOverflow)?;
    let total_bytes = total_bytes
        .checked_add(file_size)
        .filter(|value| *value <= JS_MAX_SAFE_INTEGER)
        .ok_or(StagedCopyError::SourceOverflow)?;
    Ok((file_count, total_bytes))
}

pub fn copy_directory_staged<F, C>(
    inventory: &CopyInventory,
    destination: &Path,
    staging: &Path,
    retries: u32,
    mut progress: F,
    mut committing: impl FnMut() -> Result<(), StagedCopyError>,
    cancelled: C,
) -> Result<(), StagedCopyError>
where
    F: FnMut(u64, &str) -> Result<(), StagedCopyError>,
    C: Fn() -> bool,
{
    if cancelled() {
        return Err(StagedCopyError::Cancelled);
    }
    ensure_missing(destination, StagedCopyError::DestinationExists)?;
    ensure_missing(staging, StagedCopyError::StagingCollision)?;
    let parent = destination
        .parent()
        .ok_or(StagedCopyError::DestinationParentChanged)?;
    let parent_metadata =
        metadata_without_links(parent).map_err(|_| StagedCopyError::DestinationParentChanged)?;
    if !parent_metadata.is_dir() {
        return Err(StagedCopyError::DestinationParentChanged);
    }
    let parent_snapshot = snapshot(parent, &parent_metadata, EntryKind::Directory)?;
    #[cfg(windows)]
    let commit_parent = fence_windows::MutationRoot::open(parent)
        .map_err(|error| StagedCopyError::CopyFailed(io::Error::other(error.to_string())))?;
    #[cfg(unix)]
    let commit_parent = rustix::fs::open(
        parent,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| {
        StagedCopyError::CopyFailed(io::Error::from_raw_os_error(error.raw_os_error()))
    })?;
    if staging.parent() != Some(parent) {
        return Err(StagedCopyError::NonAtomicCommit);
    }
    fs::create_dir(staging).map_err(map_staging_create)?;
    let staging_metadata = metadata_without_links(staging)?;
    let staging_snapshot = snapshot(staging, &staging_metadata, EntryKind::Directory)?;
    let mut staged_directories = Vec::<(PathBuf, Snapshot)>::new();
    let mut guard = StagingGuard {
        path: staging,
        live: true,
    };
    let mut completed = 0_u64;

    for entry in &inventory.entries {
        if cancelled() {
            return Err(StagedCopyError::Cancelled);
        }
        verify_directory_identity(staging, &staging_snapshot)
            .map_err(|_| StagedCopyError::StagingCollision)?;
        verify_source_ancestors(inventory, entry)?;
        verify_staged_ancestors(staging, &entry.relative, &staged_directories)?;
        let target = staging.join(&entry.relative);
        match entry.kind {
            EntryKind::Directory => {
                verify_source_entry(inventory, entry)?;
                fs::create_dir(&target).map_err(StagedCopyError::CopyFailed)?;
                let metadata = metadata_without_links(&target)
                    .map_err(|_| StagedCopyError::StagingCollision)?;
                staged_directories.push((
                    PathBuf::from(&entry.relative),
                    snapshot(&target, &metadata, EntryKind::Directory)?,
                ));
            }
            EntryKind::File => {
                let attempts = retries.max(1);
                let mut last_error = None;
                for attempt in 0..attempts {
                    match copy_one_file(
                        inventory,
                        entry,
                        &target,
                        completed,
                        &mut progress,
                        &cancelled,
                    ) {
                        Ok(()) => {
                            last_error = None;
                            break;
                        }
                        Err(error @ StagedCopyError::Cancelled)
                        | Err(error @ StagedCopyError::SourceChanged)
                        | Err(error @ StagedCopyError::SourceLinkBlocked) => return Err(error),
                        Err(error) => {
                            last_error = Some(error);
                            let _ = fs::remove_file(&target);
                            if attempt + 1 < attempts {
                                std::thread::sleep(std::time::Duration::from_millis(
                                    150 * u64::from(attempt + 1),
                                ));
                            }
                        }
                    }
                }
                if let Some(error) = last_error {
                    return Err(error);
                }
                completed = completed
                    .checked_add(entry.size)
                    .ok_or(StagedCopyError::SourceOverflow)?;
            }
        }
    }

    verify_source_tree(inventory)?;
    verify_directory_identity(staging, &staging_snapshot)
        .map_err(|_| StagedCopyError::StagingCollision)?;
    for (relative, expected) in &staged_directories {
        verify_directory_identity(&staging.join(relative), expected)
            .map_err(|_| StagedCopyError::StagingCollision)?;
    }
    let current_parent =
        metadata_without_links(parent).map_err(|_| StagedCopyError::DestinationParentChanged)?;
    if !current_parent.is_dir()
        || identity(parent, &current_parent)
            .map_err(|_| StagedCopyError::DestinationParentChanged)?
            != parent_snapshot.identity
    {
        return Err(StagedCopyError::DestinationParentChanged);
    }
    ensure_missing(destination, StagedCopyError::DestinationExists)?;
    for (relative, _) in staged_directories.iter().rev() {
        sync_directory(&staging.join(relative))?;
    }
    sync_directory(staging)?;
    sync_directory(parent)?;
    committing()?;
    if cancelled() {
        return Err(StagedCopyError::Cancelled);
    }
    atomic_rename_noreplace(parent, staging, destination, &commit_parent)?;
    guard.live = false;
    sync_directory(parent)?;
    Ok(())
}

fn verify_source_ancestors(
    inventory: &CopyInventory,
    entry: &CopyEntry,
) -> Result<(), StagedCopyError> {
    #[cfg(windows)]
    {
        // Every ancestor is held open without delete sharing for the inventory lifetime.
        // The entry itself is opened from its original record immediately after this check.
        let _ = (inventory, entry);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        verify_snapshot(&inventory.source_root, &inventory.root_snapshot)?;
        let relative = Path::new(&entry.relative);
        for ancestor in relative.ancestors().skip(1) {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let normalized = ancestor.to_string_lossy().replace('\\', "/");
            let expected = inventory
                .entries
                .iter()
                .find(|candidate| {
                    candidate.kind == EntryKind::Directory && candidate.relative == normalized
                })
                .ok_or(StagedCopyError::SourceChanged)?;
            verify_snapshot(&inventory.source_root.join(ancestor), &expected.snapshot)?;
        }
        Ok(())
    }
}

fn verify_staged_ancestors(
    staging: &Path,
    relative: &str,
    directories: &[(PathBuf, Snapshot)],
) -> Result<(), StagedCopyError> {
    for ancestor in Path::new(relative).ancestors().skip(1) {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let (_, expected) = directories
            .iter()
            .find(|(candidate, _)| candidate == ancestor)
            .ok_or(StagedCopyError::StagingCollision)?;
        verify_directory_identity(&staging.join(ancestor), expected)
            .map_err(|_| StagedCopyError::StagingCollision)?;
    }
    Ok(())
}

fn copy_one_file<F, C>(
    inventory: &CopyInventory,
    entry: &CopyEntry,
    target: &Path,
    completed: u64,
    progress: &mut F,
    cancelled: &C,
) -> Result<(), StagedCopyError>
where
    F: FnMut(u64, &str) -> Result<(), StagedCopyError>,
    C: Fn() -> bool,
{
    let expected = &entry.snapshot;
    let relative = &entry.relative;
    #[cfg(windows)]
    let source_node = open_source_node(inventory, entry)?;
    #[cfg(windows)]
    let mut input = source_node
        .try_clone_file()
        .map_err(map_windows_source_error)?;
    #[cfg(not(windows))]
    let source_path = inventory.source_root.join(relative);
    #[cfg(not(windows))]
    let mut input = File::open(&source_path).map_err(StagedCopyError::CopyFailed)?;
    let opened = input.metadata().map_err(StagedCopyError::CopyFailed)?;
    #[cfg(windows)]
    let opened_snapshot = snapshot_with_native(
        &opened,
        EntryKind::File,
        source_node.metadata().identity,
        source_node.metadata().link_count,
        source_node.metadata().size,
    );
    #[cfg(not(windows))]
    let opened_snapshot = snapshot(&source_path, &opened, EntryKind::File)?;
    if opened_snapshot != *expected {
        return Err(StagedCopyError::SourceChanged);
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(StagedCopyError::CopyFailed)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_u64;
    loop {
        if cancelled() {
            return Err(StagedCopyError::Cancelled);
        }
        let read = input
            .read(&mut buffer)
            .map_err(StagedCopyError::CopyFailed)?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(StagedCopyError::CopyFailed)?;
        copied = copied
            .checked_add(read as u64)
            .ok_or(StagedCopyError::SourceOverflow)?;
        if copied > expected.len {
            return Err(StagedCopyError::SourceChanged);
        }
        let current = completed
            .checked_add(copied)
            .filter(|value| *value <= JS_MAX_SAFE_INTEGER)
            .ok_or(StagedCopyError::SourceOverflow)?;
        progress(current, relative)?;
    }
    output.flush().map_err(StagedCopyError::CopyFailed)?;
    if copied != expected.len {
        return Err(StagedCopyError::SourceChanged);
    }
    let after = input.metadata().map_err(StagedCopyError::CopyFailed)?;
    #[cfg(windows)]
    let after_snapshot = {
        let native = source_node
            .refresh_metadata()
            .map_err(map_windows_source_error)?;
        source_node
            .verify_path_identity()
            .map_err(map_windows_source_error)?;
        snapshot_with_native(
            &after,
            EntryKind::File,
            native.identity,
            native.link_count,
            native.size,
        )
    };
    #[cfg(not(windows))]
    let after_snapshot = snapshot(&source_path, &after, EntryKind::File)?;
    if after_snapshot != *expected {
        return Err(StagedCopyError::SourceChanged);
    }
    Ok(())
}

#[cfg(windows)]
fn open_source_node(
    inventory: &CopyInventory,
    entry: &CopyEntry,
) -> Result<fence_windows::NodeHandle, StagedCopyError> {
    let directory = inventory
        .source_directories
        .get(&entry.windows_parent)
        .ok_or(StagedCopyError::SourceChanged)?;
    directory
        .handle
        .open_child(&entry.windows_entry)
        .map_err(|_| StagedCopyError::SourceChanged)
}

#[cfg(windows)]
fn verify_source_entry(
    inventory: &CopyInventory,
    entry: &CopyEntry,
) -> Result<(), StagedCopyError> {
    use fence_windows::NodeKind;

    let node = open_source_node(inventory, entry)?;
    let native = node.metadata();
    let expected_kind = match entry.kind {
        EntryKind::Directory => NodeKind::Directory,
        EntryKind::File => NodeKind::RegularFile,
    };
    if native.kind != expected_kind {
        return Err(StagedCopyError::SourceChanged);
    }
    let metadata = fs::symlink_metadata(inventory.source_root.join(&entry.relative))
        .map_err(|_| StagedCopyError::SourceChanged)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(StagedCopyError::SourceChanged);
    }
    let actual = snapshot_with_native(
        &metadata,
        entry.kind.clone(),
        native.identity,
        native.link_count,
        native.size,
    );
    if actual == entry.snapshot {
        Ok(())
    } else {
        Err(StagedCopyError::SourceChanged)
    }
}

#[cfg(not(windows))]
fn verify_source_entry(
    inventory: &CopyInventory,
    entry: &CopyEntry,
) -> Result<(), StagedCopyError> {
    verify_snapshot(
        &inventory.source_root.join(&entry.relative),
        &entry.snapshot,
    )
}

#[cfg(windows)]
fn verify_source_tree(inventory: &CopyInventory) -> Result<(), StagedCopyError> {
    verify_snapshot(&inventory.source_root, &inventory.root_snapshot)?;
    for directory in inventory.source_directories.values() {
        let current = directory
            .handle
            .entries()
            .map_err(|_| StagedCopyError::SourceChanged)?
            .into_iter()
            .map(|entry| (entry.name.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        if !same_directory_structure(&current, &directory.entries) {
            return Err(StagedCopyError::SourceChanged);
        }
    }
    for entry in &inventory.entries {
        verify_source_entry(inventory, entry)?;
    }
    Ok(())
}

#[cfg(windows)]
fn same_directory_structure(
    current: &BTreeMap<std::ffi::OsString, fence_windows::DirectoryEntry>,
    expected: &BTreeMap<std::ffi::OsString, fence_windows::DirectoryEntry>,
) -> bool {
    current.len() == expected.len()
        && current.iter().all(|(name, actual)| {
            expected.get(name).is_some_and(|original| {
                actual.file_id == original.file_id
                    && actual.attributes == original.attributes
                    && actual.reparse_tag == original.reparse_tag
            })
        })
}

#[cfg(not(windows))]
fn verify_source_tree(inventory: &CopyInventory) -> Result<(), StagedCopyError> {
    verify_snapshot(&inventory.source_root, &inventory.root_snapshot)?;
    for entry in &inventory.entries {
        verify_source_entry(inventory, entry)?;
    }
    let current =
        inspect_source_tree(&inventory.source_root).map_err(|_| StagedCopyError::SourceChanged)?;
    if !same_source_inventory(&current, inventory) {
        return Err(StagedCopyError::SourceChanged);
    }
    Ok(())
}

#[cfg(not(windows))]
fn same_source_inventory(current: &CopyInventory, expected: &CopyInventory) -> bool {
    current.root_snapshot == expected.root_snapshot
        && current.file_count == expected.file_count
        && current.total_bytes == expected.total_bytes
        && current.entries.len() == expected.entries.len()
        && current
            .entries
            .iter()
            .zip(&expected.entries)
            .all(|(actual, original)| {
                actual.kind == original.kind
                    && actual.relative == original.relative
                    && actual.size == original.size
                    && actual.snapshot == original.snapshot
            })
}

#[cfg(windows)]
fn map_windows_source_error(error: fence_windows::WindowsError) -> StagedCopyError {
    StagedCopyError::SourceUnreadable(io::Error::other(error.to_string()))
}

fn validate_name(name: &str) -> Result<(), StagedCopyError> {
    let normalized = validate_relative_path(name).map_err(|_| StagedCopyError::SourceUnsafeName)?;
    if normalized.components().count() != 1 || normalized.as_os_str() != name {
        return Err(StagedCopyError::SourceUnsafeName);
    }
    Ok(())
}

fn ensure_missing(path: &Path, error: StagedCopyError) -> Result<(), StagedCopyError> {
    match fs::symlink_metadata(path) {
        Err(io_error) if io_error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(io_error) => Err(StagedCopyError::CopyFailed(io_error)),
        Ok(_) => Err(error),
    }
}

fn metadata_without_links(path: &Path) -> Result<fs::Metadata, StagedCopyError> {
    let metadata = fs::symlink_metadata(path).map_err(StagedCopyError::SourceUnreadable)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(StagedCopyError::SourceLinkBlocked);
    }
    Ok(metadata)
}

fn verify_snapshot(path: &Path, expected: &Snapshot) -> Result<(), StagedCopyError> {
    let metadata = metadata_without_links(path).map_err(|_| StagedCopyError::SourceChanged)?;
    let actual = snapshot(path, &metadata, expected.kind.clone())
        .map_err(|_| StagedCopyError::SourceChanged)?;
    if actual == *expected {
        Ok(())
    } else {
        Err(StagedCopyError::SourceChanged)
    }
}

fn verify_directory_identity(path: &Path, expected: &Snapshot) -> Result<(), StagedCopyError> {
    let metadata = metadata_without_links(path).map_err(|_| StagedCopyError::SourceChanged)?;
    if metadata.is_dir() && identity(path, &metadata)? == expected.identity {
        Ok(())
    } else {
        Err(StagedCopyError::SourceChanged)
    }
}

#[cfg(not(windows))]
fn snapshot(
    path: &Path,
    metadata: &fs::Metadata,
    kind: EntryKind,
) -> Result<Snapshot, StagedCopyError> {
    Ok(Snapshot {
        identity: identity(path, metadata)?,
        len: metadata.len(),
        modified: metadata.modified().ok(),
        kind,
        links: link_count(path, metadata)?,
    })
}

#[cfg(windows)]
fn snapshot(
    path: &Path,
    metadata: &fs::Metadata,
    kind: EntryKind,
) -> Result<Snapshot, StagedCopyError> {
    let native = windows_node_metadata(path)?;
    Ok(snapshot_with_native(
        metadata,
        kind,
        native.identity,
        native.link_count,
        native.size,
    ))
}

#[cfg(windows)]
fn snapshot_with_native(
    metadata: &fs::Metadata,
    kind: EntryKind,
    identity: Identity,
    link_count: u32,
    size: u64,
) -> Snapshot {
    Snapshot {
        identity,
        len: size,
        modified: metadata.modified().ok(),
        kind,
        links: u64::from(link_count),
    }
}

#[cfg(unix)]
fn identity(_path: &Path, metadata: &fs::Metadata) -> Result<Identity, StagedCopyError> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn identity(path: &Path, _metadata: &fs::Metadata) -> Result<Identity, StagedCopyError> {
    Ok(windows_node_metadata(path)?.identity)
}

#[cfg(windows)]
fn windows_node_metadata(path: &Path) -> Result<fence_windows::NodeMetadata, StagedCopyError> {
    use fence_windows::RootHandle;
    let parent = path.parent().ok_or(StagedCopyError::SourceChanged)?;
    let name = path.file_name().ok_or(StagedCopyError::SourceChanged)?;
    let root = RootHandle::open(parent)
        .map_err(|error| StagedCopyError::SourceUnreadable(io::Error::other(error.to_string())))?;
    let node = root
        .directory()
        .open_named_child(name)
        .map_err(|error| StagedCopyError::SourceUnreadable(io::Error::other(error.to_string())))?;
    Ok(node.metadata())
}

#[cfg(not(any(unix, windows)))]
fn identity(_path: &Path, _metadata: &fs::Metadata) -> Result<Identity, StagedCopyError> {
    Ok(())
}

#[cfg(unix)]
fn link_count(_path: &Path, metadata: &fs::Metadata) -> Result<u64, StagedCopyError> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink())
}

#[cfg(not(any(unix, windows)))]
fn link_count(_path: &Path, _metadata: &fs::Metadata) -> Result<u64, StagedCopyError> {
    Ok(1)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn sync_directory(path: &Path) -> Result<(), StagedCopyError> {
    match File::open(path).and_then(|directory| directory.sync_all()) {
        Ok(()) => Ok(()),
        Err(error) if cfg!(windows) && error.kind() == io::ErrorKind::PermissionDenied => Ok(()),
        Err(error) => Err(StagedCopyError::CopyFailed(error)),
    }
}

fn map_staging_create(error: io::Error) -> StagedCopyError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        StagedCopyError::StagingCollision
    } else {
        StagedCopyError::CopyFailed(error)
    }
}

#[cfg(windows)]
fn atomic_rename_noreplace(
    _parent: &Path,
    staging: &Path,
    destination: &Path,
    commit_parent: &fence_windows::MutationRoot,
) -> Result<(), StagedCopyError> {
    let source = staging
        .file_name()
        .ok_or(StagedCopyError::NonAtomicCommit)?;
    let target = destination
        .file_name()
        .ok_or(StagedCopyError::NonAtomicCommit)?;
    commit_parent
        .directory()
        .rename_child_noreplace(source, target)
        .map_err(|error| {
            if fs::symlink_metadata(destination).is_ok() {
                StagedCopyError::DestinationExists
            } else {
                StagedCopyError::CopyFailed(io::Error::other(error.to_string()))
            }
        })
}

#[cfg(unix)]
fn atomic_rename_noreplace(
    _parent: &Path,
    staging: &Path,
    destination: &Path,
    commit_parent: &std::os::fd::OwnedFd,
) -> Result<(), StagedCopyError> {
    let source = staging
        .file_name()
        .ok_or(StagedCopyError::NonAtomicCommit)?;
    let target = destination
        .file_name()
        .ok_or(StagedCopyError::NonAtomicCommit)?;
    rustix::fs::renameat_with(
        commit_parent,
        source,
        commit_parent,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| match error {
        rustix::io::Errno::EXIST => StagedCopyError::DestinationExists,
        rustix::io::Errno::XDEV => StagedCopyError::NonAtomicCommit,
        _ => StagedCopyError::CopyFailed(io::Error::from_raw_os_error(error.raw_os_error())),
    })
}

#[cfg(not(any(unix, windows)))]
fn atomic_rename_noreplace(
    _parent: &Path,
    _staging: &Path,
    _destination: &Path,
    _commit_parent: &(),
) -> Result<(), StagedCopyError> {
    Err(StagedCopyError::NonAtomicCommit)
}

struct StagingGuard<'a> {
    path: &'a Path,
    live: bool,
}

impl Drop for StagingGuard<'_> {
    fn drop(&mut self) {
        if self.live {
            let _ = fs::remove_dir_all(self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn copies_and_reports_progress() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        let staging = root.path().join(".destination.importing-test");
        fs::create_dir(&source).unwrap();
        fs::create_dir(source.join("nested")).unwrap();
        fs::write(source.join("nested/file"), b"content").unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        let mut progress = Vec::new();
        copy_directory_staged(
            &inventory,
            &destination,
            &staging,
            3,
            |completed, relative| {
                progress.push((completed, relative.to_owned()));
                Ok(())
            },
            || Ok(()),
            || false,
        )
        .unwrap();
        assert_eq!(
            fs::read(destination.join("nested/file")).unwrap(),
            b"content"
        );
        assert_eq!(progress.last().unwrap().0, 7);
        assert!(!staging.exists());
    }

    #[test]
    fn rejects_existing_destination_and_staging() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        let destination = root.path().join("destination");
        fs::write(&destination, b"existing").unwrap();
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &destination,
                &root.path().join("stage"),
                1,
                |_, _| Ok(()),
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::DestinationExists)
        ));
    }

    #[test]
    fn cancellation_removes_staging() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), vec![1_u8; 1024]).unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        let destination = root.path().join("destination");
        let staging = root.path().join("stage");
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &destination,
                &staging,
                1,
                |_, _| Err(StagedCopyError::Cancelled),
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::Cancelled)
        ));
        assert!(!staging.exists());
        assert!(!destination.exists());
    }

    #[test]
    fn detects_source_mutation_after_inventory() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), b"before").unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        fs::write(source.join("file"), b"changed-size").unwrap();
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &root.path().join("destination"),
                &root.path().join("stage"),
                1,
                |_, _| Ok(()),
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::SourceChanged)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn detects_source_mutation_during_copy() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        let source_file = source.join("file");
        fs::write(&source_file, vec![1_u8; 2 * 1024 * 1024]).unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        let destination = root.path().join("destination");
        let staging = root.path().join("stage");
        let mut mutated = false;
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &destination,
                &staging,
                1,
                |_, _| {
                    if !mutated {
                        mutated = true;
                        let mut file = OpenOptions::new().append(true).open(&source_file).unwrap();
                        file.write_all(b"changed").unwrap();
                    }
                    Ok(())
                },
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::SourceChanged)
        ));
        assert!(mutated);
        assert!(!staging.exists());
        assert!(!destination.exists());
    }

    #[test]
    fn detects_source_entry_added_after_inventory() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("first"), b"content").unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        fs::write(source.join("added"), b"content").unwrap();
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &root.path().join("destination"),
                &root.path().join("stage"),
                1,
                |_, _| Ok(()),
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::SourceChanged)
        ));
    }

    #[test]
    fn rejects_hardlink_created_after_inventory() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file"), b"content").unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        fs::hard_link(source.join("file"), root.path().join("outside-link")).unwrap();
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &root.path().join("destination"),
                &root.path().join("stage"),
                1,
                |_, _| Ok(()),
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::SourceChanged)
        ));
    }

    #[test]
    fn rejects_hardlinks() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("one"), b"data").unwrap();
        fs::hard_link(source.join("one"), source.join("two")).unwrap();
        assert!(matches!(
            inspect_source_tree(&source),
            Err(StagedCopyError::SourceLinkBlocked)
        ));
    }

    #[test]
    fn rejects_protocol_count_and_size_overflow() {
        assert!(matches!(
            checked_file_totals(JS_MAX_SAFE_INTEGER, 0, 0),
            Err(StagedCopyError::SourceOverflow)
        ));
        assert!(matches!(
            checked_file_totals(0, JS_MAX_SAFE_INTEGER, 1),
            Err(StagedCopyError::SourceOverflow)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_junction_entries() {
        let source = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let junction = source.path().join("junction");
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(matches!(
            inspect_source_tree(source.path()),
            Err(StagedCopyError::SourceLinkBlocked)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_and_dangling_destination_links() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        symlink(root.path(), source.join("link")).unwrap();
        assert!(matches!(
            inspect_source_tree(&source),
            Err(StagedCopyError::SourceLinkBlocked)
        ));
        fs::remove_file(source.join("link")).unwrap();
        let inventory = inspect_source_tree(&source).unwrap();
        let destination = root.path().join("destination");
        symlink(root.path().join("missing"), &destination).unwrap();
        assert!(matches!(
            copy_directory_staged(
                &inventory,
                &destination,
                &root.path().join("stage"),
                1,
                |_, _| Ok(()),
                || Ok(()),
                || false
            ),
            Err(StagedCopyError::DestinationExists)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_source_root_symlinks() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let source = root.path().join("source-link");
        symlink(root.path(), &source).unwrap();
        assert!(matches!(
            inspect_source_tree(&source),
            Err(StagedCopyError::SourceNotDirectory)
        ));
    }
}
