// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
//! Handle-relative mutations for adapter-owned transaction directories. Never
//! recurse through links, follow a replaced ancestor, or replace a destination.
use crate::secure_path::{checked_relative_names, SecurePathError};
use std::{io, path::Path};

pub fn remove_owned_tree(root: &Path, relative: &Path) -> Result<(), SecurePathError> {
    platform::remove(root, relative)
}
pub fn rename_owned_tree(root: &Path, from: &Path, to: &Path) -> Result<(), SecurePathError> {
    platform::rename(root, from, to)
}

#[cfg(unix)]
mod platform {
    use super::*;
    use rustix::{
        fd::OwnedFd,
        fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags},
    };
    use std::ffi::{CStr, OsString};
    fn error(e: rustix::io::Errno) -> SecurePathError {
        io::Error::from_raw_os_error(e.raw_os_error()).into()
    }
    fn directory_at(fd: &OwnedFd, name: &std::ffi::OsStr) -> Result<OwnedFd, SecurePathError> {
        fs::openat(
            fd,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(error)
    }
    fn parent(root: &Path, relative: &Path) -> Result<(OwnedFd, OsString), SecurePathError> {
        let names = checked_relative_names(relative)?;
        let mut directory = fs::open(
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(error)?;
        for name in &names[..names.len() - 1] {
            directory = directory_at(&directory, name)?;
        }
        Ok((directory, names.last().unwrap().clone()))
    }
    fn empty(
        directory: &OwnedFd,
        depth: usize,
        remaining: &mut usize,
        device: fs::Dev,
    ) -> Result<(), SecurePathError> {
        if depth > 128 {
            return Err(SecurePathError::TooLarge);
        }
        let entries = fs::Dir::read_from(directory).map_err(error)?;
        for entry in entries {
            let entry = entry.map_err(error)?;
            let name = entry.file_name();
            if name == c"." || name == c".." {
                continue;
            }
            *remaining = remaining.checked_sub(1).ok_or(SecurePathError::TooLarge)?;
            let metadata = fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(error)?;
            if metadata.st_dev != device {
                return Err(SecurePathError::Unsafe);
            }
            if FileType::from_raw_mode(metadata.st_mode) == FileType::Directory {
                let child = fs::openat(
                    directory,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(error)?;
                let opened = fs::fstat(&child).map_err(error)?;
                if opened.st_dev != metadata.st_dev || opened.st_ino != metadata.st_ino {
                    return Err(SecurePathError::Changed);
                }
                empty(&child, depth + 1, remaining, device)?;
                ensure_same(directory, name, &opened)?;
                fs::unlinkat(directory, name, AtFlags::REMOVEDIR).map_err(error)?;
            } else {
                // unlinkat removes the entry itself, including a symlink, never its target.
                fs::unlinkat(directory, name, AtFlags::empty()).map_err(error)?;
            }
        }
        Ok(())
    }
    fn ensure_same(
        parent: &OwnedFd,
        name: &CStr,
        expected: &fs::Stat,
    ) -> Result<(), SecurePathError> {
        let current = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(error)?;
        if current.st_dev != expected.st_dev || current.st_ino != expected.st_ino {
            return Err(SecurePathError::Changed);
        }
        Ok(())
    }
    pub(super) fn remove(root: &Path, relative: &Path) -> Result<(), SecurePathError> {
        let (parent, name) = parent(root, relative)?;
        let child = directory_at(&parent, &name)?;
        let opened = fs::fstat(&child).map_err(error)?;
        let root = fs::fstat(&parent).map_err(error)?;
        if opened.st_dev != root.st_dev {
            return Err(SecurePathError::Unsafe);
        }
        empty(&child, 0, &mut 1_000_000, opened.st_dev)?;
        let current = fs::statat(&parent, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(error)?;
        if current.st_dev != opened.st_dev || current.st_ino != opened.st_ino {
            return Err(SecurePathError::Changed);
        }
        fs::unlinkat(&parent, name, AtFlags::REMOVEDIR).map_err(error)?;
        fs::fsync(&parent).map_err(error)
    }
    pub(super) fn rename(root: &Path, from: &Path, to: &Path) -> Result<(), SecurePathError> {
        let (source, source_name) = parent(root, from)?;
        let (target, target_name) = parent(root, to)?;
        let _ordinary_directory = directory_at(&source, &source_name)?;
        fs::renameat_with(
            &source,
            source_name,
            &target,
            target_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(error)?;
        fs::fsync(&source).map_err(error)?;
        fs::fsync(&target).map_err(error)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn replaced_parent_never_redirects_descriptor_relative_cleanup() {
            let dir = tempfile::tempdir().unwrap();
            let managed = dir.path().join("managed");
            let outside = dir.path().join("outside");
            std::fs::create_dir(&managed).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::create_dir(managed.join("stage")).unwrap();
            std::fs::write(managed.join("stage/work"), b"partial").unwrap();
            std::fs::write(outside.join("preserved"), b"original").unwrap();
            let (pinned, _) = parent(dir.path(), Path::new("managed/stage")).unwrap();
            std::fs::rename(&managed, dir.path().join("moved")).unwrap();
            std::os::unix::fs::symlink(&outside, &managed).unwrap();
            let child = directory_at(&pinned, std::ffi::OsStr::new("stage")).unwrap();
            let dev = fs::fstat(&child).unwrap().st_dev;
            empty(&child, 0, &mut 100, dev).unwrap();
            assert_eq!(
                std::fs::read(outside.join("preserved")).unwrap(),
                b"original"
            );
            assert!(dir
                .path()
                .join("moved/stage")
                .read_dir()
                .unwrap()
                .next()
                .is_none());
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use fence_windows::{DirectoryHandle, MutationRoot, NodeKind, RootHandle};
    use std::{ffi::OsString, fs};
    fn error(e: fence_windows::WindowsError) -> SecurePathError {
        io::Error::other(e.to_string()).into()
    }
    // Pin every ordinary parent without FILE_SHARE_DELETE. This also protects
    // the cross-parent std rename from ancestor replacement on Windows.
    fn parents(
        root: &Path,
        relative: &Path,
    ) -> Result<(Vec<RootHandle>, std::path::PathBuf, OsString), SecurePathError> {
        let names = checked_relative_names(relative)?;
        let mut pins = vec![RootHandle::open(root).map_err(error)?];
        let mut path = root.to_owned();
        for name in &names[..names.len() - 1] {
            let child = pins
                .last()
                .unwrap()
                .directory()
                .open_named_child(name)
                .map_err(error)?
                .into_directory()
                .map_err(error)?;
            path.push(name);
            // Keep the no-follow child open until the path-based pin is checked.
            // RootHandle intentionally resolves its selected root, so discarding
            // child here would leave a junction-replacement race between opens.
            let pinned = RootHandle::open(&path).map_err(error)?;
            if child.metadata().identity != pinned.directory().metadata().identity {
                return Err(SecurePathError::Changed);
            }
            pins.push(pinned);
        }
        Ok((pins, path, names.last().unwrap().clone()))
    }
    fn empty(
        directory: &DirectoryHandle,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<(), SecurePathError> {
        if depth > 128 {
            return Err(SecurePathError::TooLarge);
        }
        for entry in directory.entries().map_err(error)? {
            *remaining = remaining.checked_sub(1).ok_or(SecurePathError::TooLarge)?;
            if entry.reparse_tag.is_none() && entry.attributes & 0x10 != 0 {
                let child = directory
                    .open_mutation_directory(&entry.name)
                    .map_err(error)?;
                empty(&child, depth + 1, remaining)?;
                drop(child);
            }
            directory.remove_child(&entry.name).map_err(error)?;
        }
        Ok(())
    }
    pub(super) fn remove(root: &Path, relative: &Path) -> Result<(), SecurePathError> {
        let (_pins, parent, name) = parents(root, relative)?;
        let directory = MutationRoot::open(&parent).map_err(error)?;
        let child = directory
            .directory()
            .open_mutation_directory(&name)
            .map_err(error)?;
        empty(&child, 0, &mut 1_000_000)?;
        drop(child);
        directory.directory().remove_child(&name).map_err(error)
    }
    pub(super) fn rename(root: &Path, from: &Path, to: &Path) -> Result<(), SecurePathError> {
        let (source_pins, source_parent, source_name) = parents(root, from)?;
        let (_target_pins, target_parent, target_name) = parents(root, to)?;
        let source = source_pins
            .last()
            .unwrap()
            .directory()
            .open_named_child(&source_name)
            .map_err(error)?;
        if source.metadata().kind != NodeKind::Directory {
            return Err(SecurePathError::Unsafe);
        }
        drop(source);
        let target = target_parent.join(&target_name);
        match fs::symlink_metadata(&target) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(_) => return Err(SecurePathError::Unsafe),
        }
        if source_parent == target_parent {
            MutationRoot::open(&source_parent)
                .map_err(error)?
                .directory()
                .rename_child_noreplace(&source_name, &target_name)
                .map_err(error)
        } else {
            // Windows directory renames do not replace an existing directory.
            fs::rename(source_parent.join(source_name), target).map_err(Into::into)
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;
    pub(super) fn remove(_: &Path, _: &Path) -> Result<(), SecurePathError> {
        Err(SecurePathError::Unsupported)
    }
    pub(super) fn rename(_: &Path, _: &Path, _: &Path) -> Result<(), SecurePathError> {
        Err(SecurePathError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn renames_never_overwrite_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["from", "to", "nested"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        fs::write(dir.path().join("from/preserved"), b"original").unwrap();
        assert!(rename_owned_tree(dir.path(), Path::new("from"), Path::new("to")).is_err());
        rename_owned_tree(dir.path(), Path::new("from"), Path::new("nested/moved")).unwrap();
        assert_eq!(
            fs::read(dir.path().join("nested/moved/preserved")).unwrap(),
            b"original"
        );
        remove_owned_tree(dir.path(), Path::new("nested/moved")).unwrap();
        assert!(dir.path().join("nested").exists());
        assert!(remove_owned_tree(dir.path(), Path::new(".")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn links_in_the_tree_are_unlinked_but_never_traversed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("stage")).unwrap();
        fs::create_dir(dir.path().join("outside")).unwrap();
        fs::write(dir.path().join("outside/preserved"), b"original").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), dir.path().join("stage/link"))
            .unwrap();
        remove_owned_tree(dir.path(), Path::new("stage")).unwrap();
        assert_eq!(
            fs::read(dir.path().join("outside/preserved")).unwrap(),
            b"original"
        );
    }
}
