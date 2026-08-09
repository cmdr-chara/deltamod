use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use thiserror::Error;

use crate::path_security::{validate_relative_path, CaseSensitivity};

const JS_MAX_SAFE_INTEGER: i128 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveLimits {
    pub max_files: usize,
    pub max_expanded_bytes: u64,
    pub max_archive_bytes: u64,
    pub max_depth: usize,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_expanded_bytes: 4 * 1024 * 1024 * 1024,
            max_archive_bytes: 2 * 1024 * 1024 * 1024,
            max_depth: 32,
        }
    }
}

impl ArchiveLimits {
    pub fn try_from_signed(
        max_files: i128,
        max_expanded_bytes: i128,
        max_archive_bytes: i128,
        max_depth: i128,
    ) -> Result<Self, UnsafeArchiveError> {
        Ok(Self {
            max_files: usize::try_from(max_files).map_err(|_| UnsafeArchiveError::InvalidLimit)?,
            max_expanded_bytes: u64::try_from(max_expanded_bytes)
                .map_err(|_| UnsafeArchiveError::InvalidLimit)?,
            max_archive_bytes: u64::try_from(max_archive_bytes)
                .map_err(|_| UnsafeArchiveError::InvalidLimit)?,
            max_depth: usize::try_from(max_depth).map_err(|_| UnsafeArchiveError::InvalidLimit)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: i128,
    pub attributes: String,
}

impl ArchiveEntry {
    pub fn new(name: impl Into<String>, size: i128, attributes: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size,
            attributes: attributes.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArchiveInventory {
    pub file_count: usize,
    pub expanded_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum UnsafeArchiveError {
    #[error("Invalid archive limit.")]
    InvalidLimit,
    #[error("The archive is empty or could not be listed.")]
    Empty,
    #[error("The archive contains too many entries.")]
    FileLimit,
    #[error("Unsafe archive entry path.")]
    UnsafePath,
    #[error("Archive entry exceeds the nesting limit.")]
    NestingLimit,
    #[error("The archive contains duplicate destination paths.")]
    DuplicatePath,
    #[error("Archive links are not allowed.")]
    LinkBlocked,
    #[error("Invalid archive entry size.")]
    InvalidSize,
    #[error("The archive expands beyond its byte limit.")]
    SizeLimit,
    #[error("Extracted path escaped staging.")]
    PathEscape,
    #[error("Unsupported extracted entry type.")]
    EntryType,
}

impl UnsafeArchiveError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidLimit => "ARCHIVE_INVALID_LIMIT",
            Self::Empty => "ARCHIVE_EMPTY",
            Self::FileLimit => "ARCHIVE_FILE_LIMIT",
            Self::UnsafePath => "ARCHIVE_UNSAFE_PATH",
            Self::NestingLimit => "ARCHIVE_NESTING_LIMIT",
            Self::DuplicatePath => "ARCHIVE_DUPLICATE_PATH",
            Self::LinkBlocked => "ARCHIVE_LINK_BLOCKED",
            Self::InvalidSize => "ARCHIVE_INVALID_SIZE",
            Self::SizeLimit => "ARCHIVE_SIZE_LIMIT",
            Self::PathEscape => "ARCHIVE_PATH_ESCAPE",
            Self::EntryType => "ARCHIVE_ENTRY_TYPE",
        }
    }
}

#[derive(Debug, Error)]
pub enum ExtractedTreeError {
    #[error(transparent)]
    Unsafe(#[from] UnsafeArchiveError),
    #[error("Extracted tree could not be read safely.")]
    Io(#[source] io::Error),
}

impl ExtractedTreeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unsafe(error) => error.code(),
            Self::Io(_) => "ARCHIVE_IO_ERROR",
        }
    }
}

impl From<io::Error> for ExtractedTreeError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn validate_extracted_tree(
    root: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveInventory, ExtractedTreeError> {
    let root_metadata = fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || is_reparse_point(&root_metadata) {
        return Err(UnsafeArchiveError::LinkBlocked.into());
    }
    if !root_metadata.is_dir() {
        return Err(UnsafeArchiveError::EntryType.into());
    }

    let real_root = fs::canonicalize(root)?;
    let mut queue = vec![(root.to_path_buf(), 0_usize)];
    let mut file_count = 0_usize;
    let mut expanded_bytes = 0_u64;

    while let Some((directory, depth)) = queue.pop() {
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                return Err(UnsafeArchiveError::LinkBlocked.into());
            }

            let canonical = fs::canonicalize(&path)?;
            if !crate::path_security::is_within(&real_root, &canonical, false) {
                return Err(UnsafeArchiveError::PathEscape.into());
            }

            if metadata.is_dir() {
                let child_depth = depth
                    .checked_add(1)
                    .ok_or(UnsafeArchiveError::NestingLimit)?;
                if child_depth > limits.max_depth {
                    return Err(UnsafeArchiveError::NestingLimit.into());
                }
                queue.push((path, child_depth));
            } else if metadata.is_file() {
                if link_count(&path, &metadata)? > 1 {
                    return Err(UnsafeArchiveError::LinkBlocked.into());
                }
                file_count = file_count
                    .checked_add(1)
                    .ok_or(UnsafeArchiveError::FileLimit)?;
                if file_count > limits.max_files {
                    return Err(UnsafeArchiveError::FileLimit.into());
                }
                expanded_bytes = expanded_bytes
                    .checked_add(metadata.len())
                    .ok_or(UnsafeArchiveError::SizeLimit)?;
                if expanded_bytes > limits.max_expanded_bytes {
                    return Err(UnsafeArchiveError::SizeLimit.into());
                }
            } else {
                return Err(UnsafeArchiveError::EntryType.into());
            }
        }
    }

    Ok(ArchiveInventory {
        file_count,
        expanded_bytes,
    })
}

#[cfg(unix)]
fn link_count(_path: &Path, metadata: &fs::Metadata) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink())
}

#[cfg(windows)]
fn link_count(path: &Path, _metadata: &fs::Metadata) -> io::Result<u64> {
    use fence_windows::RootHandle;

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing parent."))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Missing name."))?;
    let root = RootHandle::open(parent).map_err(|error| io::Error::other(error.to_string()))?;
    let node = root
        .directory()
        .open_named_child(name)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(node.metadata().link_count.into())
}

#[cfg(not(any(unix, windows)))]
fn link_count(_path: &Path, _metadata: &fs::Metadata) -> io::Result<u64> {
    Ok(1)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

pub fn validate_archive_entries(
    entries: &[ArchiveEntry],
    limits: ArchiveLimits,
) -> Result<ArchiveInventory, UnsafeArchiveError> {
    validate_archive_entries_with_case(entries, limits, CaseSensitivity::host())
}

pub fn validate_archive_entries_with_case(
    entries: &[ArchiveEntry],
    limits: ArchiveLimits,
    case_sensitivity: CaseSensitivity,
) -> Result<ArchiveInventory, UnsafeArchiveError> {
    if entries.is_empty() {
        return Err(UnsafeArchiveError::Empty);
    }
    if entries.len() > limits.max_files {
        return Err(UnsafeArchiveError::FileLimit);
    }

    let mut expanded_bytes = 0_u64;
    let mut seen = HashSet::new();
    for entry in entries {
        let normalized =
            validate_relative_path(&entry.name).map_err(|_| UnsafeArchiveError::UnsafePath)?;
        if normalized == std::path::Path::new(".")
            || normalized == std::path::Path::new(std::path::MAIN_SEPARATOR_STR)
        {
            return Err(UnsafeArchiveError::UnsafePath);
        }
        let depth = normalized.components().count();
        if depth > limits.max_depth {
            return Err(UnsafeArchiveError::NestingLimit);
        }

        let normalized = normalized.to_string_lossy().into_owned();
        let key = match case_sensitivity {
            CaseSensitivity::Sensitive => normalized,
            CaseSensitivity::Insensitive => normalized.to_lowercase(),
        };
        if !seen.insert(key) {
            return Err(UnsafeArchiveError::DuplicatePath);
        }

        let attributes = entry.attributes.to_lowercase();
        if entry.attributes.starts_with('l')
            || contains_link_attribute(&attributes, "symbolic")
            || contains_link_attribute(&attributes, "hard")
            || attributes.contains("reparse")
        {
            return Err(UnsafeArchiveError::LinkBlocked);
        }

        if entry.size > JS_MAX_SAFE_INTEGER {
            return Err(UnsafeArchiveError::InvalidSize);
        }
        let size = u64::try_from(entry.size).map_err(|_| UnsafeArchiveError::InvalidSize)?;
        expanded_bytes = expanded_bytes
            .checked_add(size)
            .ok_or(UnsafeArchiveError::SizeLimit)?;
        if expanded_bytes > limits.max_expanded_bytes {
            return Err(UnsafeArchiveError::SizeLimit);
        }
    }

    Ok(ArchiveInventory {
        file_count: entries.len(),
        expanded_bytes,
    })
}

fn contains_link_attribute(attributes: &str, kind: &str) -> bool {
    let mut remainder = attributes;
    while let Some(index) = remainder.find(kind) {
        let after_kind = &remainder[index + kind.len()..];
        let after_space = after_kind.trim_start_matches(char::is_whitespace);
        if after_space.starts_with("link") {
            return true;
        }
        remainder = after_kind;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn entry(name: &str, size: i128, attributes: &str) -> ArchiveEntry {
        ArchiveEntry::new(name, size, attributes)
    }

    #[test]
    fn accepts_a_bounded_inventory() {
        let inventory = validate_archive_entries(
            &[
                entry("mod/meta.toml", 100, "A"),
                entry("mod/data/file.bin", 900, "A"),
            ],
            ArchiveLimits::default(),
        )
        .unwrap();
        assert_eq!(
            inventory,
            ArchiveInventory {
                file_count: 2,
                expanded_bytes: 1000
            }
        );
    }

    #[test]
    fn returns_stable_codes_for_inventory_failures() {
        let cases = [
            (vec![], UnsafeArchiveError::Empty),
            (
                vec![entry("../outside", 1, "A")],
                UnsafeArchiveError::UnsafePath,
            ),
            (
                vec![entry("linked", 1, "lrwxrwxrwx")],
                UnsafeArchiveError::LinkBlocked,
            ),
            (
                vec![entry("linked", 1, "symbolic link")],
                UnsafeArchiveError::LinkBlocked,
            ),
            (
                vec![entry("linked", 1, "HARD LINK")],
                UnsafeArchiveError::LinkBlocked,
            ),
            (
                vec![entry("linked", 1, "reparse point")],
                UnsafeArchiveError::LinkBlocked,
            ),
            (vec![entry("bad", -1, "A")], UnsafeArchiveError::InvalidSize),
            (
                vec![entry("bad", i128::MAX, "A")],
                UnsafeArchiveError::InvalidSize,
            ),
        ];
        for (entries, expected) in cases {
            let error = validate_archive_entries(&entries, ArchiveLimits::default()).unwrap_err();
            assert_eq!(error, expected);
            assert!(error.code().starts_with("ARCHIVE_"));
        }
    }

    #[test]
    fn enforces_file_size_and_depth_limits() {
        let mut limits = ArchiveLimits {
            max_files: 1,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_archive_entries(&[entry("one", 1, "A"), entry("two", 1, "A")], limits),
            Err(UnsafeArchiveError::FileLimit)
        );
        limits = ArchiveLimits {
            max_expanded_bytes: 10,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_archive_entries(&[entry("large", 11, "A")], limits),
            Err(UnsafeArchiveError::SizeLimit)
        );
        limits = ArchiveLimits {
            max_expanded_bytes: u64::MAX,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_archive_entries(
                &[
                    entry("one", i128::from(u64::MAX), "A"),
                    entry("two", 1, "A")
                ],
                limits
            ),
            Err(UnsafeArchiveError::InvalidSize)
        );
        limits = ArchiveLimits {
            max_depth: 2,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_archive_entries(&[entry("one/two/three", 1, "A")], limits),
            Err(UnsafeArchiveError::NestingLimit)
        );
        limits = ArchiveLimits {
            max_files: 0,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_archive_entries(&[entry("one", 1, "A")], limits),
            Err(UnsafeArchiveError::FileLimit)
        );
    }

    #[test]
    fn rejects_invalid_signed_limits_before_validation() {
        assert_eq!(
            ArchiveLimits::try_from_signed(-1, 1, 1, 1),
            Err(UnsafeArchiveError::InvalidLimit)
        );
        assert_eq!(
            ArchiveLimits::try_from_signed(1, i128::MAX, 1, 1),
            Err(UnsafeArchiveError::InvalidLimit)
        );
        assert_eq!(
            UnsafeArchiveError::InvalidLimit.code(),
            "ARCHIVE_INVALID_LIMIT"
        );
    }

    #[test]
    fn duplicate_case_behavior_is_selectable() {
        let entries = [entry("Mod/File", 1, "A"), entry("mod/file", 1, "A")];
        assert!(validate_archive_entries_with_case(
            &entries,
            ArchiveLimits::default(),
            CaseSensitivity::Sensitive
        )
        .is_ok());
        assert_eq!(
            validate_archive_entries_with_case(
                &entries,
                ArchiveLimits::default(),
                CaseSensitivity::Insensitive
            ),
            Err(UnsafeArchiveError::DuplicatePath)
        );
        let aliases = [entry("mod//file", 1, "A"), entry("mod/./file", 1, "A")];
        assert_eq!(
            validate_archive_entries_with_case(
                &aliases,
                ArchiveLimits::default(),
                CaseSensitivity::Sensitive
            ),
            Err(UnsafeArchiveError::DuplicatePath)
        );
    }

    #[test]
    fn rejects_root_and_encoded_paths() {
        for name in [
            ".",
            "./",
            "/",
            "C:\\outside",
            "\\\\server\\share",
            "%252e%252e%252foutside",
            "%zz",
        ] {
            assert_eq!(
                validate_archive_entries(&[entry(name, 1, "A")], ArchiveLimits::default()),
                Err(UnsafeArchiveError::UnsafePath),
                "{name:?}"
            );
        }
    }

    #[test]
    fn validates_an_extracted_tree_and_enforces_limits() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        fs::write(directory.path().join("one"), b"123").unwrap();
        fs::write(directory.path().join("nested/two"), b"4567").unwrap();
        assert_eq!(
            validate_extracted_tree(directory.path(), ArchiveLimits::default()).unwrap(),
            ArchiveInventory {
                file_count: 2,
                expanded_bytes: 7,
            }
        );

        let limits = ArchiveLimits {
            max_files: 1,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_extracted_tree(directory.path(), limits)
                .unwrap_err()
                .code(),
            "ARCHIVE_FILE_LIMIT"
        );
        let limits = ArchiveLimits {
            max_expanded_bytes: 6,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_extracted_tree(directory.path(), limits)
                .unwrap_err()
                .code(),
            "ARCHIVE_SIZE_LIMIT"
        );
        let limits = ArchiveLimits {
            max_depth: 0,
            ..ArchiveLimits::default()
        };
        assert_eq!(
            validate_extracted_tree(directory.path(), limits)
                .unwrap_err()
                .code(),
            "ARCHIVE_NESTING_LIMIT"
        );
    }

    #[test]
    fn rejects_hard_links_and_non_directories() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::write(&source, b"data").unwrap();
        fs::hard_link(&source, directory.path().join("second")).unwrap();
        assert_eq!(
            validate_extracted_tree(directory.path(), ArchiveLimits::default())
                .unwrap_err()
                .code(),
            "ARCHIVE_LINK_BLOCKED"
        );
        assert_eq!(
            validate_extracted_tree(&source, ArchiveLimits::default())
                .unwrap_err()
                .code(),
            "ARCHIVE_ENTRY_TYPE"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_and_unsupported_entries() {
        use std::os::unix::fs::{symlink, FileTypeExt};

        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), directory.path().join("escape")).unwrap();
        assert_eq!(
            validate_extracted_tree(directory.path(), ArchiveLimits::default())
                .unwrap_err()
                .code(),
            "ARCHIVE_LINK_BLOCKED"
        );
        fs::remove_file(directory.path().join("escape")).unwrap();

        let fifo = directory.path().join("fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo());
        assert_eq!(
            validate_extracted_tree(directory.path(), ArchiveLimits::default())
                .unwrap_err()
                .code(),
            "ARCHIVE_ENTRY_TYPE"
        );
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_junctions() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let junction = directory.path().join("junction");
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&junction)
            .arg(outside.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            validate_extracted_tree(directory.path(), ArchiveLimits::default())
                .unwrap_err()
                .code(),
            "ARCHIVE_LINK_BLOCKED"
        );
    }

    #[test]
    fn distinguishes_io_failures() {
        let missing = PathBuf::from("definitely-missing-extracted-tree");
        assert_eq!(
            validate_extracted_tree(&missing, ArchiveLimits::default())
                .unwrap_err()
                .code(),
            "ARCHIVE_IO_ERROR"
        );
    }
}
