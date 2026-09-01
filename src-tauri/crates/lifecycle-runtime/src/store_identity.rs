use serde::{Deserialize, Serialize};
use std::{
    fs::{File, Metadata, OpenOptions},
    io,
    path::Path,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoreObjectKind {
    Directory,
    RegularFile,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StableObjectIdentity {
    pub(crate) volume_id: u128,
    pub(crate) file_id: u128,
}

#[derive(Debug)]
pub(crate) enum IdentityError {
    Io(io::Error),
    Unsafe,
    WrongKind,
    #[cfg(not(any(unix, windows)))]
    Unavailable,
    Replaced,
}

impl From<io::Error> for IdentityError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub(crate) fn configure_no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        options.custom_flags(unix_no_follow_flag());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;

        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
}

pub(crate) fn inspect_path(
    path: &Path,
    expected_kind: StoreObjectKind,
) -> Result<StableObjectIdentity, IdentityError> {
    let opened = open_existing_no_follow(path, expected_kind)?;
    verify_opened_path(path, &opened, expected_kind)
}

#[cfg(unix)]
pub(crate) fn inspect_opened(
    file: &File,
    expected_kind: StoreObjectKind,
) -> Result<StableObjectIdentity, IdentityError> {
    let metadata = file.metadata()?;
    validate_metadata(&metadata, expected_kind, true)?;
    stable_opened_identity(file, expected_kind)
}

pub(crate) fn verify_opened_path(
    path: &Path,
    file: &File,
    expected_kind: StoreObjectKind,
) -> Result<StableObjectIdentity, IdentityError> {
    let handle_metadata = file.metadata()?;
    validate_metadata(&handle_metadata, expected_kind, true)?;
    let retained_identity = stable_opened_identity(file, expected_kind)?;
    let current = open_existing_no_follow(path, expected_kind)?;
    let current_identity = stable_opened_identity(&current, expected_kind)?;
    if retained_identity != current_identity {
        Err(IdentityError::Replaced)
    } else {
        Ok(retained_identity)
    }
}

fn open_existing_no_follow(
    path: &Path,
    expected_kind: StoreObjectKind,
) -> Result<File, IdentityError> {
    let path_metadata = std::fs::symlink_metadata(path)?;
    validate_metadata(&path_metadata, expected_kind, true)?;

    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    #[cfg(windows)]
    if expected_kind == StoreObjectKind::Directory {
        use std::os::windows::fs::OpenOptionsExt as _;

        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
    }
    let file = options.open(path)?;
    let handle_metadata = file.metadata()?;
    validate_metadata(&handle_metadata, expected_kind, true)?;
    stable_opened_identity(&file, expected_kind)?;
    Ok(file)
}

fn validate_metadata(
    metadata: &Metadata,
    expected_kind: StoreObjectKind,
    reject_reparse: bool,
) -> Result<(), IdentityError> {
    if metadata.file_type().is_symlink() || (reject_reparse && platform_is_reparse(metadata)) {
        return Err(IdentityError::Unsafe);
    }
    let right_kind = match expected_kind {
        StoreObjectKind::Directory => metadata.is_dir(),
        StoreObjectKind::RegularFile => metadata.is_file(),
    };
    if !right_kind {
        return Err(IdentityError::WrongKind);
    }
    #[cfg(unix)]
    if expected_kind == StoreObjectKind::RegularFile {
        use std::os::unix::fs::MetadataExt as _;

        if metadata.nlink() != 1 {
            return Err(IdentityError::Unsafe);
        }
    }
    Ok(())
}

#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    )
))]
const fn unix_no_follow_flag() -> i32 {
    0x8000
}

#[cfg(all(
    target_os = "linux",
    not(any(
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "powerpc",
        target_arch = "powerpc64"
    ))
))]
const fn unix_no_follow_flag() -> i32 {
    0x20000
}

#[cfg(target_os = "macos")]
const fn unix_no_follow_flag() -> i32 {
    0x0100
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
const fn unix_no_follow_flag() -> i32 {
    compile_error!("Unix lifecycle object pinning currently supports Linux and macOS only");
}

#[cfg(windows)]
fn stable_opened_identity(
    file: &File,
    expected_kind: StoreObjectKind,
) -> Result<StableObjectIdentity, IdentityError> {
    let information = winapi_util::file::information(file)?;
    if expected_kind == StoreObjectKind::RegularFile && information.number_of_links() != 1 {
        return Err(IdentityError::Unsafe);
    }
    Ok(StableObjectIdentity {
        volume_id: u128::from(information.volume_serial_number()),
        file_id: u128::from(information.file_index()),
    })
}

#[cfg(unix)]
fn stable_opened_identity(
    file: &File,
    expected_kind: StoreObjectKind,
) -> Result<StableObjectIdentity, IdentityError> {
    use std::os::unix::fs::MetadataExt as _;

    let metadata = file.metadata()?;
    if expected_kind == StoreObjectKind::RegularFile && metadata.nlink() != 1 {
        return Err(IdentityError::Unsafe);
    }
    Ok(StableObjectIdentity {
        volume_id: u128::from(metadata.dev()),
        file_id: u128::from(metadata.ino()),
    })
}

#[cfg(not(any(unix, windows)))]
fn stable_opened_identity(
    _file: &File,
    _expected_kind: StoreObjectKind,
) -> Result<StableObjectIdentity, IdentityError> {
    Err(IdentityError::Unavailable)
}

#[cfg(windows)]
fn platform_is_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn platform_is_reparse(_metadata: &Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn shared_identity_validator_distinguishes_objects_and_matches_open_handle() {
        let directory = tempfile::tempdir().unwrap();
        let first_path = directory.path().join("first");
        let second_path = directory.path().join("second");
        File::create(&first_path)
            .unwrap()
            .write_all(b"one")
            .unwrap();
        File::create(&second_path)
            .unwrap()
            .write_all(b"two")
            .unwrap();

        let first = File::open(&first_path).unwrap();
        let first_identity =
            verify_opened_path(&first_path, &first, StoreObjectKind::RegularFile).unwrap();
        let second_identity = inspect_path(&second_path, StoreObjectKind::RegularFile).unwrap();

        assert_ne!(first_identity, second_identity);
        assert_eq!(
            first_identity,
            verify_opened_path(&first_path, &first, StoreObjectKind::RegularFile).unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_identity_survives_object_mutation_and_rejects_copy_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let renamed = directory.path().join("renamed");
        let replacement = directory.path().join("replacement");
        File::create(&original)
            .unwrap()
            .write_all(b"identity")
            .unwrap();
        let retained = File::open(&original).unwrap();
        let identity = inspect_path(&original, StoreObjectKind::RegularFile).unwrap();

        OpenOptions::new()
            .append(true)
            .open(&original)
            .unwrap()
            .write_all(b"-append")
            .unwrap();
        assert_eq!(
            inspect_path(&original, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        assert_eq!(
            verify_opened_path(&original, &retained, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        OpenOptions::new()
            .write(true)
            .open(&original)
            .unwrap()
            .set_len(3)
            .unwrap();
        assert_eq!(
            inspect_path(&original, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        std::fs::rename(&original, &renamed).unwrap();
        assert_eq!(
            inspect_path(&renamed, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        assert_eq!(
            verify_opened_path(&renamed, &retained, StoreObjectKind::RegularFile).unwrap(),
            identity
        );

        std::fs::copy(&renamed, &replacement).unwrap();
        assert_ne!(
            inspect_path(&replacement, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        std::fs::remove_file(&renamed).unwrap();
        std::fs::rename(&replacement, &renamed).unwrap();
        assert_ne!(
            inspect_path(&renamed, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        let replacement_check =
            verify_opened_path(&renamed, &retained, StoreObjectKind::RegularFile);
        assert!(
            matches!(
                &replacement_check,
                Err(IdentityError::Replaced | IdentityError::Unsafe | IdentityError::Io(_))
            ),
            "replacement verification returned {replacement_check:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_regular_file_identity_rejects_hardlinks() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let alias = directory.path().join("alias");
        File::create(&original).unwrap();
        match std::fs::hard_link(&original, &alias) {
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
            inspect_path(&original, StoreObjectKind::RegularFile),
            Err(IdentityError::Unsafe)
        ));
        assert!(matches!(
            inspect_path(&alias, StoreObjectKind::RegularFile),
            Err(IdentityError::Unsafe)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unix_identity_survives_object_mutation_and_rejects_copy_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let renamed = directory.path().join("renamed");
        let replacement = directory.path().join("replacement");
        File::create(&original)
            .unwrap()
            .write_all(b"identity")
            .unwrap();
        let retained = File::open(&original).unwrap();
        let identity = inspect_path(&original, StoreObjectKind::RegularFile).unwrap();

        OpenOptions::new()
            .append(true)
            .open(&original)
            .unwrap()
            .write_all(b"-append")
            .unwrap();
        assert_eq!(
            inspect_path(&original, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        assert_eq!(
            verify_opened_path(&original, &retained, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        std::fs::rename(&original, &renamed).unwrap();
        assert_eq!(
            inspect_path(&renamed, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        assert_eq!(
            verify_opened_path(&renamed, &retained, StoreObjectKind::RegularFile).unwrap(),
            identity
        );

        std::fs::copy(&renamed, &replacement).unwrap();
        std::fs::remove_file(&renamed).unwrap();
        std::fs::rename(&replacement, &renamed).unwrap();
        assert_ne!(
            inspect_path(&renamed, StoreObjectKind::RegularFile).unwrap(),
            identity
        );
        assert!(matches!(
            verify_opened_path(&renamed, &retained, StoreObjectKind::RegularFile),
            Err(IdentityError::Replaced | IdentityError::Unsafe | IdentityError::Io(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unix_regular_file_identity_rejects_symlinks_and_hardlinks() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let hardlink = directory.path().join("hardlink");
        let symlink_path = directory.path().join("symlink");
        File::create(&original).unwrap();
        std::fs::hard_link(&original, &hardlink).unwrap();
        symlink(&original, &symlink_path).unwrap();

        assert!(matches!(
            inspect_path(&original, StoreObjectKind::RegularFile),
            Err(IdentityError::Unsafe)
        ));
        assert!(matches!(
            inspect_path(&hardlink, StoreObjectKind::RegularFile),
            Err(IdentityError::Unsafe)
        ));
        assert!(matches!(
            inspect_path(&symlink_path, StoreObjectKind::RegularFile),
            Err(IdentityError::Unsafe)
        ));
    }
}
