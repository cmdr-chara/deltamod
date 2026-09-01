use super::error::{io_failure, ErrorCode, ErrorReport, ExecutionPhase, Result};
use deltamod_theme_recipes::{Sha256Digest, MAX_SOURCE_BYTES};
use sha2::{Digest, Sha256};
use std::{
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, File, Metadata},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::io;

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct FileIdentity {
    volume: u64,
    object: [u8; 16],
}

impl fmt::Debug for FileIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FileIdentity(<redacted>)")
    }
}

impl FileIdentity {
    pub(crate) fn binding_sha256(self) -> Sha256Digest {
        let mut bytes = b"deltamod:theme-recipe-staging-root:v1\0".to_vec();
        bytes.extend_from_slice(&self.volume.to_le_bytes());
        bytes.extend_from_slice(&self.object);
        Sha256Digest::from_bytes(&bytes)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NodeKind {
    File,
    Directory,
    #[cfg(windows)]
    Reparse,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct FileEvidence {
    pub(crate) identity: FileIdentity,
    pub(crate) kind: NodeKind,
    pub(crate) links: u64,
    pub(crate) length: u64,
    modified: i128,
    changed: i128,
}

impl fmt::Debug for FileEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileEvidence")
            .field("identity", &"<redacted>")
            .field("kind", &self.kind)
            .field("links", &self.links)
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

pub(crate) struct GuardedRoot {
    canonical: PathBuf,
    mutable: bool,
    identity: FileIdentity,
    #[cfg(windows)]
    directory: fence_windows::DirectoryHandle,
    #[cfg(unix)]
    directory: File,
}

impl GuardedRoot {
    pub(crate) fn open_private_operation(path: &Path, phase: ExecutionPhase) -> Result<Self> {
        if contains_live_theme_component(path) {
            return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
        }
        let root = Self::open(path, true, true, phase)?;
        // Reapply the boundary after canonicalization and handle identity verification so
        // namespace aliases cannot hide a live theme-tree component.
        if contains_live_theme_component(root.canonical()) {
            return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
        }
        Ok(root)
    }

    pub(crate) fn open_read(path: &Path, phase: ExecutionPhase) -> Result<Self> {
        Self::open(path, false, false, phase)
    }

    fn open(
        path: &Path,
        mutable: bool,
        require_private: bool,
        phase: ExecutionPhase,
    ) -> Result<Self> {
        if !path.is_absolute() {
            return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
        }
        validate_directory_path(path, phase)?;
        let before = io_failure(fs::symlink_metadata(path), phase)?;
        if !before.is_dir() || metadata_is_unsafe(&before) {
            return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
        }
        let canonical = io_failure(fs::canonicalize(path), phase)?;

        #[cfg(windows)]
        let directory = open_windows_selected_directory(&canonical, mutable, phase)?;
        #[cfg(windows)]
        let identity = windows_evidence(directory.metadata()).identity;

        #[cfg(unix)]
        let directory = open_unix_directory_path(&canonical, phase)?;
        #[cfg(unix)]
        let identity = io_failure(file_evidence(&directory), phase)?.identity;

        #[cfg(not(any(windows, unix)))]
        return Err(ErrorReport::new(ErrorCode::UnsupportedFilesystem, phase));

        let root = Self {
            canonical,
            mutable,
            identity,
            directory,
        };
        if require_private {
            ensure_private_directory(root.canonical(), phase)?;
        }
        root.check_identity(phase)?;
        Ok(root)
    }

    pub(crate) fn canonical(&self) -> &Path {
        &self.canonical
    }

    pub(crate) const fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub(crate) fn binding_sha256(&self) -> Sha256Digest {
        self.identity.binding_sha256()
    }

    pub(crate) fn check(&self, phase: ExecutionPhase) -> Result<()> {
        self.check_identity(phase)?;
        if self.mutable {
            ensure_private_directory(&self.canonical, phase)?;
            self.check_identity(phase)?;
        }
        Ok(())
    }

    fn check_identity(&self, phase: ExecutionPhase) -> Result<()> {
        validate_directory_path(&self.canonical, phase)?;

        #[cfg(windows)]
        let current = open_windows_selected_directory(&self.canonical, self.mutable, phase)?;
        #[cfg(windows)]
        let identity = windows_evidence(current.metadata()).identity;

        #[cfg(unix)]
        let current = open_unix_directory_path(&self.canonical, phase)?;
        #[cfg(unix)]
        let identity = io_failure(file_evidence(&current), phase)?.identity;

        if identity != self.identity {
            return Err(ErrorReport::new(ErrorCode::RootChanged, phase));
        }
        Ok(())
    }

    pub(crate) fn names(&self, phase: ExecutionPhase) -> Result<Vec<OsString>> {
        #[cfg(windows)]
        {
            self.directory
                .entries()
                .map(|entries| entries.into_iter().map(|entry| entry.name).collect())
                .map_err(|_| ErrorReport::new(ErrorCode::Io, phase))
        }

        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let directory = rustix::fs::Dir::read_from(&self.directory)
                .map_err(|_| ErrorReport::new(ErrorCode::Io, phase))?;
            let mut names = Vec::new();
            for entry in directory {
                let entry = entry.map_err(|_| ErrorReport::new(ErrorCode::Io, phase))?;
                let bytes = entry.file_name().to_bytes();
                if bytes != b"." && bytes != b".." {
                    names.push(OsString::from_vec(bytes.to_vec()));
                }
            }
            Ok(names)
        }
    }

    pub(crate) fn child_exists(&self, name: &str, phase: ExecutionPhase) -> Result<bool> {
        validate_component(name, phase)?;
        let names = self.names(phase)?;
        if names.iter().any(|candidate| candidate == OsStr::new(name)) {
            return Ok(true);
        }
        #[cfg(windows)]
        if names.iter().any(|candidate| {
            candidate
                .to_str()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        }) {
            return Err(ErrorReport::new(ErrorCode::IncompleteStaging, phase));
        }
        Ok(false)
    }

    pub(crate) fn open_child_dir(
        &self,
        name: &str,
        expected: Option<FileIdentity>,
        phase: ExecutionPhase,
    ) -> Result<GuardedRoot> {
        validate_component(name, phase)?;
        if !self.child_exists(name, phase)? {
            return Err(ErrorReport::new(ErrorCode::UnsafeSource, phase));
        }

        #[cfg(windows)]
        let directory = {
            let node = self
                .directory
                .open_named_child(OsStr::new(name))
                .map_err(|_| ErrorReport::new(ErrorCode::UnsafeSource, phase))?;
            ensure_windows_streams(&node, ErrorCode::NamedStreams, phase)?;
            let evidence = windows_evidence(node.metadata());
            if evidence.kind != NodeKind::Directory
                || expected.is_some_and(|identity| identity != evidence.identity)
            {
                return Err(ErrorReport::new(ErrorCode::UnsafeSource, phase));
            }
            node.into_directory()
                .map_err(|_| ErrorReport::new(ErrorCode::UnsafeSource, phase))?
        };
        #[cfg(windows)]
        let identity = windows_evidence(directory.metadata()).identity;

        #[cfg(unix)]
        let directory = open_unix_directory_at(&self.directory, name, phase)?;
        #[cfg(unix)]
        let identity = {
            let evidence = io_failure(file_evidence(&directory), phase)?;
            if evidence.kind != NodeKind::Directory
                || expected.is_some_and(|identity| identity != evidence.identity)
            {
                return Err(ErrorReport::new(ErrorCode::UnsafeSource, phase));
            }
            evidence.identity
        };

        Ok(GuardedRoot {
            canonical: self.canonical.join(name),
            mutable: false,
            identity,
            directory,
        })
    }

    pub(crate) fn create_new_file(&self, name: &str, phase: ExecutionPhase) -> Result<File> {
        if !self.mutable {
            return Err(ErrorReport::new(ErrorCode::UnsupportedFilesystem, phase));
        }
        validate_component(name, phase)?;
        if self.child_exists(name, phase)? {
            return Err(ErrorReport::new(ErrorCode::IncompleteStaging, phase));
        }

        #[cfg(windows)]
        {
            self.directory
                .create_new_file(OsStr::new(name))
                .map_err(|_| ErrorReport::new(ErrorCode::Io, phase))
        }

        #[cfg(unix)]
        {
            let descriptor = rustix::fs::openat(
                &self.directory,
                name,
                rustix::fs::OFlags::RDWR
                    | rustix::fs::OFlags::CREATE
                    | rustix::fs::OFlags::EXCL
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            )
            .map_err(|_| ErrorReport::new(ErrorCode::Io, phase))?;
            Ok(File::from(descriptor))
        }
    }

    pub(crate) fn open_regular_file(
        &self,
        name: &str,
        expected: Option<FileIdentity>,
        error_code: ErrorCode,
        phase: ExecutionPhase,
    ) -> Result<OpenedFile> {
        validate_component(name, phase)?;
        if !self.child_exists(name, phase)? {
            return Err(ErrorReport::new(error_code, phase));
        }

        #[cfg(windows)]
        {
            let node = self
                .directory
                .open_named_child(OsStr::new(name))
                .map_err(|_| ErrorReport::new(error_code, phase))?;
            ensure_windows_streams(&node, ErrorCode::NamedStreams, phase)?;
            let evidence = windows_evidence(node.metadata());
            if evidence.links != 1 && error_code == ErrorCode::UnsafeSource {
                return Err(ErrorReport::new(ErrorCode::SourceHardlinked, phase));
            }
            if evidence.kind != NodeKind::File
                || evidence.links != 1
                || expected.is_some_and(|identity| identity != evidence.identity)
            {
                return Err(ErrorReport::new(error_code, phase));
            }
            let file = node
                .try_clone_file()
                .map_err(|_| ErrorReport::new(error_code, phase))?;
            Ok(OpenedFile {
                file,
                evidence,
                node,
            })
        }

        #[cfg(unix)]
        {
            let descriptor = rustix::fs::openat(
                &self.directory,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|_| ErrorReport::new(error_code, phase))?;
            let file = File::from(descriptor);
            let evidence = io_failure(file_evidence(&file), phase)?;
            if evidence.links != 1 && error_code == ErrorCode::UnsafeSource {
                return Err(ErrorReport::new(ErrorCode::SourceHardlinked, phase));
            }
            if evidence.kind != NodeKind::File
                || evidence.links != 1
                || expected.is_some_and(|identity| identity != evidence.identity)
            {
                return Err(ErrorReport::new(error_code, phase));
            }
            Ok(OpenedFile { file, evidence })
        }
    }

    pub(crate) fn write_new_bytes(
        &self,
        name: &str,
        bytes: &[u8],
        phase: ExecutionPhase,
    ) -> Result<()> {
        self.check(phase)?;
        let mut file = self.create_new_file(name, phase)?;
        io_failure(file.write_all(bytes), phase)?;
        io_failure(file.sync_all(), phase)?;
        drop(file);
        let opened = self.open_regular_file(name, None, ErrorCode::InvalidStagedPackage, phase)?;
        opened.finish(self, name, ErrorCode::InvalidStagedPackage, phase)?;
        self.check(phase)
    }

    pub(crate) fn read_bounded(
        &self,
        name: &str,
        maximum: u64,
        error_code: ErrorCode,
        phase: ExecutionPhase,
    ) -> Result<Vec<u8>> {
        let mut opened = self.open_regular_file(name, None, error_code, phase)?;
        let bytes = opened.read_bounded(maximum, error_code, phase)?;
        opened.finish(self, name, error_code, phase)?;
        Ok(bytes)
    }
}

pub(crate) struct OpenedFile {
    file: File,
    evidence: FileEvidence,
    #[cfg(windows)]
    node: fence_windows::NodeHandle,
}

impl OpenedFile {
    pub(crate) const fn evidence(&self) -> FileEvidence {
        self.evidence
    }

    pub(crate) fn reader(&mut self) -> &mut File {
        &mut self.file
    }

    fn read_bounded(
        &mut self,
        maximum: u64,
        error_code: ErrorCode,
        phase: ExecutionPhase,
    ) -> Result<Vec<u8>> {
        if self.evidence.length > maximum {
            return Err(ErrorReport::new(error_code, phase));
        }
        let capacity = usize::try_from(self.evidence.length)
            .map_err(|_| ErrorReport::new(error_code, phase))?;
        let mut bytes = Vec::with_capacity(capacity);
        io_failure(
            Read::by_ref(&mut self.file)
                .take(maximum.saturating_add(1))
                .read_to_end(&mut bytes),
            phase,
        )?;
        if bytes.len() as u64 != self.evidence.length || bytes.len() as u64 > maximum {
            return Err(ErrorReport::new(error_code, phase));
        }
        Ok(bytes)
    }

    pub(crate) fn finish(
        self,
        parent: &GuardedRoot,
        name: &str,
        error_code: ErrorCode,
        phase: ExecutionPhase,
    ) -> Result<()> {
        #[cfg(windows)]
        {
            ensure_windows_streams(&self.node, ErrorCode::NamedStreams, phase)?;
            if windows_evidence(
                self.node
                    .refresh_metadata()
                    .map_err(|_| ErrorReport::new(error_code, phase))?,
            ) != self.evidence
            {
                return Err(ErrorReport::new(error_code, phase));
            }
        }

        #[cfg(unix)]
        if io_failure(file_evidence(&self.file), phase)? != self.evidence {
            return Err(ErrorReport::new(error_code, phase));
        }

        let current = parent.open_regular_file_identity(name, error_code, phase)?;
        if current != self.evidence.identity {
            return Err(ErrorReport::new(error_code, phase));
        }
        Ok(())
    }
}

impl GuardedRoot {
    fn open_regular_file_identity(
        &self,
        name: &str,
        error_code: ErrorCode,
        phase: ExecutionPhase,
    ) -> Result<FileIdentity> {
        validate_component(name, phase)?;

        #[cfg(windows)]
        {
            let node = self
                .directory
                .open_named_child(OsStr::new(name))
                .map_err(|_| ErrorReport::new(error_code, phase))?;
            ensure_windows_streams(&node, ErrorCode::NamedStreams, phase)?;
            let evidence = windows_evidence(node.metadata());
            if evidence.kind != NodeKind::File || evidence.links != 1 {
                return Err(ErrorReport::new(error_code, phase));
            }
            Ok(evidence.identity)
        }

        #[cfg(unix)]
        {
            let descriptor = rustix::fs::openat(
                &self.directory,
                name,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NOFOLLOW
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(|_| ErrorReport::new(error_code, phase))?;
            let file = File::from(descriptor);
            let evidence = io_failure(file_evidence(&file), phase)?;
            if evidence.kind != NodeKind::File || evidence.links != 1 {
                return Err(ErrorReport::new(error_code, phase));
            }
            Ok(evidence.identity)
        }
    }
}

pub(crate) struct OpenedSource {
    parent: GuardedRoot,
    file_name: String,
    opened: OpenedFile,
    ancestors: Vec<GuardedRoot>,
    planned_root: File,
    planned_ancestors: Vec<File>,
    planned_source: File,
}

impl OpenedSource {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open(
        root_path: &Path,
        planned_root: File,
        planned_ancestors: Vec<File>,
        relative: &str,
        planned_source: File,
        planned_length: u64,
        phase: ExecutionPhase,
    ) -> Result<Self> {
        let planned_root_identity = planned_directory_identity(&planned_root, phase)?;
        let mut current = open_source_root(root_path, planned_root_identity, phase)?;

        let components = relative.split('/').collect::<Vec<_>>();
        if components.is_empty() || planned_ancestors.len() + 1 != components.len() {
            return Err(ErrorReport::new(ErrorCode::UnsafeSource, phase));
        }
        let mut ancestors = Vec::with_capacity(components.len());
        for (component, planned_ancestor) in components[..components.len() - 1]
            .iter()
            .zip(&planned_ancestors)
        {
            validate_component(component, phase)
                .map_err(|_| ErrorReport::new(ErrorCode::UnsafeSource, phase))?;
            let planned_identity = planned_directory_identity(planned_ancestor, phase)?;
            let next = current.open_child_dir(component, Some(planned_identity), phase)?;
            ancestors.push(current);
            current = next;
        }
        let file_name = components[components.len() - 1].to_owned();
        validate_planned_source(&planned_source, planned_length, phase)?;

        #[cfg(unix)]
        let expected_identity = Some(io_failure(file_evidence(&planned_source), phase)?.identity);
        #[cfg(windows)]
        let expected_identity = None;

        let opened = current.open_regular_file(
            &file_name,
            expected_identity,
            ErrorCode::UnsafeSource,
            phase,
        )?;
        let evidence = opened.evidence();
        if evidence.links != 1 {
            return Err(ErrorReport::new(ErrorCode::SourceHardlinked, phase));
        }
        if evidence.length > MAX_SOURCE_BYTES {
            return Err(ErrorReport::new(ErrorCode::SourceTooLarge, phase));
        }
        if evidence.length != planned_length {
            return Err(ErrorReport::new(ErrorCode::SourceChanged, phase));
        }
        Ok(Self {
            parent: current,
            file_name,
            opened,
            ancestors,
            planned_root,
            planned_ancestors,
            planned_source,
        })
    }

    pub(crate) fn copy_hashing(
        &mut self,
        destination: &mut File,
        phase: ExecutionPhase,
    ) -> Result<[u8; 32]> {
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = io_failure(self.opened.file.read(&mut buffer), phase)?;
            if read == 0 {
                break;
            }
            copied = copied
                .checked_add(read as u64)
                .ok_or_else(|| ErrorReport::new(ErrorCode::SourceTooLarge, phase))?;
            if copied > MAX_SOURCE_BYTES {
                return Err(ErrorReport::new(ErrorCode::SourceTooLarge, phase));
            }
            hasher.update(&buffer[..read]);
            io_failure(destination.write_all(&buffer[..read]), phase)?;
        }
        if copied != self.opened.evidence.length {
            return Err(ErrorReport::new(ErrorCode::SourceChanged, phase));
        }
        Ok(hasher.finalize().into())
    }

    pub(crate) fn finish(self, phase: ExecutionPhase) -> Result<()> {
        validate_planned_source(&self.planned_source, self.opened.evidence.length, phase)?;
        self.opened.finish(
            &self.parent,
            &self.file_name,
            ErrorCode::SourceChanged,
            phase,
        )?;
        self.parent
            .check(phase)
            .map_err(|_| ErrorReport::new(ErrorCode::SourceChanged, phase))?;
        for ancestor in &self.ancestors {
            ancestor
                .check(phase)
                .map_err(|_| ErrorReport::new(ErrorCode::SourceChanged, phase))?;
        }
        planned_directory_identity(&self.planned_root, phase)?;
        for ancestor in &self.planned_ancestors {
            planned_directory_identity(ancestor, phase)?;
        }
        Ok(())
    }
}

fn open_source_root(
    root_path: &Path,
    planned_identity: FileIdentity,
    phase: ExecutionPhase,
) -> Result<GuardedRoot> {
    let parent_path = root_path
        .parent()
        .ok_or_else(|| ErrorReport::new(ErrorCode::UnsupportedFilesystem, phase))?;
    let root_name = root_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| ErrorReport::new(ErrorCode::UnsupportedFilesystem, phase))?;
    validate_component(root_name, phase)
        .map_err(|_| ErrorReport::new(ErrorCode::UnsafeSource, phase))?;
    let parent = GuardedRoot::open_read(parent_path, phase)
        .map_err(|error| preserve_named_stream_error(error, ErrorCode::UnsafeSource))?;
    parent
        .open_child_dir(root_name, Some(planned_identity), phase)
        .map_err(|error| preserve_named_stream_error(error, ErrorCode::SourceChanged))
}

fn preserve_named_stream_error(error: ErrorReport, fallback: ErrorCode) -> ErrorReport {
    if error.code == ErrorCode::NamedStreams {
        error
    } else {
        ErrorReport::new(fallback, error.phase)
    }
}

fn validate_planned_source(file: &File, expected_length: u64, phase: ExecutionPhase) -> Result<()> {
    let metadata = io_failure(file.metadata(), phase)?;
    if metadata.len() > MAX_SOURCE_BYTES || expected_length > MAX_SOURCE_BYTES {
        return Err(ErrorReport::new(ErrorCode::SourceTooLarge, phase));
    }
    if !metadata.is_file() || metadata_is_unsafe(&metadata) || metadata.len() != expected_length {
        return Err(ErrorReport::new(ErrorCode::SourceChanged, phase));
    }
    Ok(())
}

fn planned_directory_identity(file: &File, phase: ExecutionPhase) -> Result<FileIdentity> {
    #[cfg(windows)]
    {
        let cloned = io_failure(file.try_clone(), phase)?;
        let directory = fence_windows::DirectoryHandle::from_directory_file(cloned)
            .map_err(|_| ErrorReport::new(ErrorCode::UnsafeSource, phase))?;
        Ok(windows_evidence(directory.metadata()).identity)
    }

    #[cfg(unix)]
    {
        let evidence = io_failure(file_evidence(file), phase)?;
        if evidence.kind != NodeKind::Directory {
            return Err(ErrorReport::new(ErrorCode::UnsafeSource, phase));
        }
        Ok(evidence.identity)
    }
}

pub(crate) fn metadata_is_unsafe(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn validate_component(component: &str, phase: ExecutionPhase) -> Result<()> {
    if component.is_empty()
        || component.len() > 255
        || matches!(component, "." | "..")
        || component.contains(['/', '\\', ':', '\0'])
    {
        return Err(ErrorReport::new(ErrorCode::IncompleteStaging, phase));
    }
    Ok(())
}

fn validate_directory_path(path: &Path, phase: ExecutionPhase) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(_) => {
                current.push(component.as_os_str());
                let metadata = io_failure(fs::symlink_metadata(&current), phase)?;
                if !metadata.is_dir() || metadata_is_unsafe(&metadata) {
                    return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
                }
            }
            Component::CurDir | Component::ParentDir => {
                return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
            }
        }
    }
    Ok(())
}

fn contains_live_theme_component(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(value) = component else {
            return false;
        };
        value.to_str().is_some_and(|value| {
            value.eq_ignore_ascii_case("customThemes") || value.eq_ignore_ascii_case("themes")
        })
    })
}

#[cfg(windows)]
fn ensure_private_directory(path: &Path, phase: ExecutionPhase) -> Result<()> {
    match fence_windows::private_directory_is_hardened(path) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ErrorReport::new(ErrorCode::RootNotPrivate, phase)),
        Err(_) => Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase)),
    }
}

#[cfg(unix)]
fn ensure_private_directory(path: &Path, phase: ExecutionPhase) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = io_failure(fs::symlink_metadata(path), phase)?;
    if metadata.permissions().mode() & 0o077 == 0 {
        Ok(())
    } else {
        Err(ErrorReport::new(ErrorCode::RootNotPrivate, phase))
    }
}

#[cfg(windows)]
fn open_windows_selected_directory(
    path: &Path,
    mutable: bool,
    phase: ExecutionPhase,
) -> Result<fence_windows::DirectoryHandle> {
    let parent_path = path
        .parent()
        .ok_or_else(|| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))?;
    let name = path
        .file_name()
        .ok_or_else(|| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))?;
    let parent = open_windows_directory_direct(parent_path, phase)?;
    let node = parent
        .open_named_child(name)
        .map_err(|_| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))?;
    ensure_windows_streams(&node, ErrorCode::NamedStreams, phase)?;
    let evidence = windows_evidence(node.metadata());
    if evidence.kind != NodeKind::Directory {
        return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
    }
    let directory = if mutable {
        parent
            .open_mutation_directory(name)
            .map_err(|_| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))?
    } else {
        node.into_directory()
            .map_err(|_| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))?
    };
    if windows_evidence(directory.metadata()).identity != evidence.identity {
        return Err(ErrorReport::new(ErrorCode::RootChanged, phase));
    }
    Ok(directory)
}

#[cfg(windows)]
fn open_windows_directory_direct(
    path: &Path,
    phase: ExecutionPhase,
) -> Result<fence_windows::DirectoryHandle> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_LIST_DIRECTORY: u32 = 0x0000_0001;
    const FILE_TRAVERSE: u32 = 0x0000_0020;
    const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    let file = io_failure(
        OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path),
        phase,
    )?;
    let metadata = io_failure(file.metadata(), phase)?;
    if !metadata.is_dir() || metadata_is_unsafe(&metadata) {
        return Err(ErrorReport::new(ErrorCode::InvalidHostRoot, phase));
    }
    fence_windows::DirectoryHandle::from_directory_file(file)
        .map_err(|_| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))
}

#[cfg(windows)]
fn windows_evidence(metadata: fence_windows::NodeMetadata) -> FileEvidence {
    FileEvidence {
        identity: FileIdentity {
            volume: metadata.identity.volume_serial,
            object: metadata.identity.file_id,
        },
        kind: match metadata.kind {
            fence_windows::NodeKind::RegularFile => NodeKind::File,
            fence_windows::NodeKind::Directory => NodeKind::Directory,
            fence_windows::NodeKind::ReparsePoint => NodeKind::Reparse,
        },
        links: u64::from(metadata.link_count),
        length: metadata.size,
        modified: i128::from(metadata.last_write_time),
        changed: i128::from(metadata.change_time),
    }
}

#[cfg(windows)]
fn ensure_windows_streams(
    node: &fence_windows::NodeHandle,
    error_code: ErrorCode,
    phase: ExecutionPhase,
) -> Result<()> {
    const ERROR_HANDLE_EOF: i32 = 38;
    let streams = match node.streams() {
        Ok(streams) => streams,
        Err(fence_windows::WindowsError::Io { operation, source })
            if node.metadata().kind == fence_windows::NodeKind::Directory
                && operation == "FileStreamInfo"
                && source.raw_os_error() == Some(ERROR_HANDLE_EOF) =>
        {
            Vec::new()
        }
        Err(_) => return Err(ErrorReport::new(error_code, phase)),
    };
    if streams
        .iter()
        .any(|stream| !stream.is_default_data_stream())
    {
        return Err(ErrorReport::new(error_code, phase));
    }
    Ok(())
}

#[cfg(unix)]
fn open_unix_directory_path(path: &Path, phase: ExecutionPhase) -> Result<File> {
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| ErrorReport::new(ErrorCode::InvalidHostRoot, phase))?;
    Ok(File::from(descriptor))
}

#[cfg(unix)]
fn open_unix_directory_at(parent: &File, name: &str, phase: ExecutionPhase) -> Result<File> {
    let descriptor = rustix::fs::openat(
        parent,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| ErrorReport::new(ErrorCode::UnsafeSource, phase))?;
    Ok(File::from(descriptor))
}

#[cfg(unix)]
fn file_evidence(file: &File) -> io::Result<FileEvidence> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    let mut object = [0_u8; 16];
    object[..8].copy_from_slice(&metadata.ino().to_le_bytes());
    Ok(FileEvidence {
        identity: FileIdentity {
            volume: metadata.dev(),
            object,
        },
        kind: if metadata.is_dir() {
            NodeKind::Directory
        } else {
            NodeKind::File
        },
        links: metadata.nlink(),
        length: metadata.len(),
        modified: i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec()),
        changed: i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()),
    })
}
