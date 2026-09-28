// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
//! The legacy adapter's recovery boundary. Journals are untrusted descriptions,
//! never filesystem authority. Only operation-derived paths may be acted upon.

use super::*;
use deltamod_tools_runtime::{
    read_relative_regular_file, try_lock_relative_file, ExclusiveFileLease,
};
use std::{fs::OpenOptions, io, path::Component};

const MAX_JOURNAL_BYTES: u64 = 64 * 1024;
const MAX_JOURNALS: usize = 1024;

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::Journal(message.into())
}

fn same_path(actual: &Path, expected: &Path) -> bool {
    // Path equality normalizes `.`. Persisted paths must be the exact generated
    // representation, not an alias which happened to compare equal.
    actual.as_os_str() == expected.as_os_str()
}

enum RecoveryAction {
    Remove(PathBuf),
    Restore { from: PathBuf, to: PathBuf },
    Finish(PathBuf),
}

impl Runtime {
    pub(super) fn mutation_lease(&self) -> Result<ExclusiveFileLease, RuntimeError> {
        if inspect_directory_identity(&self.root)? != self.root_identity {
            return Err(invalid("data root identity changed"));
        }
        let relative = Path::new(".runtime-operation.lock");
        // create_new never follows an existing symlink. The following handle-
        // relative open also rejects hardlinks, reparses and non-regular files.
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.root.join(relative))
        {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let lease = try_lock_relative_file(&self.root, relative)?;
        if inspect_directory_identity(&self.root)? != self.root_identity {
            return Err(invalid("data root identity changed"));
        }
        Ok(lease)
    }

    /// Check each existing ancestor without canonicalizing away links. Missing
    /// descendants are allowed, but all existing objects must be directories.
    pub(super) fn owned_directory_exists(&self, path: &Path) -> Result<bool, RuntimeError> {
        if inspect_directory_identity(&self.root)? != self.root_identity {
            return Err(invalid("data root identity changed"));
        }
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid("path escaped root"))?;
        let mut current = self.root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(invalid("invalid path component"));
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(_) => {
                    inspect_directory_identity(&current)?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(true)
    }

    pub(super) fn ensure_owned_directory(&self, path: &Path) -> Result<(), RuntimeError> {
        if !self.owned_directory_exists(path)? {
            let parent = path
                .parent()
                .ok_or_else(|| invalid("directory parent missing"))?;
            self.ensure_owned_directory(parent)?;
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        if !self.owned_directory_exists(path)? {
            return Err(invalid("directory disappeared"));
        }
        Ok(())
    }

    pub(super) fn require_missing(&self, path: &Path) -> Result<(), RuntimeError> {
        let parent = path.parent().ok_or_else(|| invalid("missing parent"))?;
        self.owned_directory_exists(parent)?;
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
            Ok(_) => Err(invalid("transaction path already exists")),
        }
    }

    pub(super) fn remove_owned_directory(&self, path: &Path) -> Result<(), RuntimeError> {
        if same_path(path, &self.root) {
            return Err(invalid("cannot remove root"));
        }
        if self.owned_directory_exists(path)? {
            deltamod_tools_runtime::remove_owned_tree(
                &self.root,
                path.strip_prefix(&self.root)
                    .map_err(|_| invalid("cleanup escaped root"))?,
            )?;
        }
        Ok(())
    }

    pub(super) fn rename_owned_directory(
        &self,
        from: &Path,
        to: &Path,
    ) -> Result<(), RuntimeError> {
        self.owned_directory_exists(from)?;
        self.require_missing(to)?;
        deltamod_tools_runtime::rename_owned_tree(
            &self.root,
            from.strip_prefix(&self.root)
                .map_err(|_| invalid("rename source escaped root"))?,
            to.strip_prefix(&self.root)
                .map_err(|_| invalid("rename destination escaped root"))?,
        )?;
        Ok(())
    }

    pub(super) fn validate_copy_destination(
        &self,
        kind: &str,
        path: &Path,
    ) -> Result<(), RuntimeError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid("destination escaped root"))?;
        let parts: Vec<_> = relative
            .components()
            .map(|part| match part {
                Component::Normal(value) => value
                    .to_str()
                    .ok_or_else(|| invalid("non-Unicode destination")),
                _ => Err(invalid("invalid destination component")),
            })
            .collect::<Result<_, _>>()?;
        let expected = match (kind, parts.as_slice()) {
            ("official-profile-import", ["official-profile"]) => self.root.join("official-profile"),
            (
                "installation-create"
                | "installation-copy"
                | "installation-reimport"
                | "installation-repair",
                ["installations", id],
            ) => {
                InstallationId::new(*id).map_err(|_| invalid("invalid installation identity"))?;
                self.root.join("installations").join(id)
            }
            ("legacy-installation-create" | "legacy-installation-reimport", [profile, game]) => {
                let index = legacy_index(profile)?;
                let valid_game = *game == "deltaruneInstall"
                    || (kind == "legacy-installation-reimport"
                        && game
                            .strip_prefix("deltaruneInstall-reimport-")
                            .and_then(|n| n.parse::<u64>().ok())
                            .is_some_and(|id| {
                                id > 0 && *game == format!("deltaruneInstall-reimport-{id}")
                            }));
                if !valid_game {
                    return Err(invalid("invalid managed game directory"));
                }
                self.root
                    .join(format!("deltamod_system-{index}"))
                    .join(game)
            }
            _ => return Err(invalid("invalid copy kind or destination")),
        };
        if !same_path(path, &expected) {
            return Err(invalid("aliased destination"));
        }
        self.owned_directory_exists(path)?;
        Ok(())
    }

    pub(super) fn read_journal(&self, path: &Path) -> Result<Journal, RuntimeError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid("journal escaped root"))?;
        let bytes = read_relative_regular_file(&self.root, relative, MAX_JOURNAL_BYTES)?;
        // Inspect schema version before the full record. Unknown versions cannot
        // trigger cleanup merely because their old-looking fields deserialize.
        let value: Value = serde_json::from_slice(&bytes)?;
        if value.get("version").and_then(Value::as_u64) != Some(1) {
            return Err(invalid("unsupported version"));
        }
        // Deserialize the original bytes, so duplicate fields are not erased by Value.
        let journal: Journal = serde_json::from_slice(&bytes)?;
        self.validate_journal(path, &journal)?;
        Ok(journal)
    }

    pub(super) fn validate_journal(
        &self,
        path: &Path,
        journal: &Journal,
    ) -> Result<(), RuntimeError> {
        let id = journal.operation_id;
        if journal.version != 1
            || id == 0
            || id > deltamod_native_core::staged_copy::JS_MAX_SAFE_INTEGER
        {
            return Err(invalid("invalid journal identity or version"));
        }
        if !same_path(
            path,
            &self
                .root
                .join(".runtime-journals")
                .join(format!("{id}.json")),
        ) {
            return Err(invalid("journal filename does not match operation"));
        }
        let destination = journal
            .destination
            .as_deref()
            .map(Path::new)
            .ok_or_else(|| invalid("destination missing"))?;
        if matches!(
            journal.kind.as_str(),
            "legacy-profile-delete" | "installation-delete"
        ) {
            if journal.source.is_some() || journal.staging.is_some() || journal.backup.is_some() {
                return Err(invalid("unexpected delete fields"));
            }
            let (expected, trash) = if journal.kind == "legacy-profile-delete" {
                let name = destination
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or_else(|| invalid("invalid profile"))?;
                (
                    self.root
                        .join(format!("deltamod_system-{}", legacy_index(name)?)),
                    format!("legacy-delete-{id}"),
                )
            } else {
                let name = destination
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or_else(|| invalid("invalid installation"))?;
                InstallationId::new(name).map_err(|_| invalid("invalid installation identity"))?;
                (
                    self.root.join("installations").join(name),
                    format!("delete-{id}"),
                )
            };
            if !same_path(destination, &expected) {
                return Err(invalid("delete escaped owned directory"));
            }
            if let Some(replacement) = &journal.replacement {
                if !same_path(
                    Path::new(replacement),
                    &self.root.join(".runtime-replacements").join(trash),
                ) {
                    return Err(invalid("invalid delete quarantine"));
                }
            } else if journal.kind == "legacy-profile-delete" {
                return Err(invalid("delete quarantine missing"));
            }
        } else {
            self.validate_copy_destination(&journal.kind, destination)?;
            let replacement = self.root.join(".runtime-replacements").join(id.to_string());
            let target = if let Some(value) = &journal.replacement {
                if !same_path(Path::new(value), &replacement) {
                    return Err(invalid("invalid replacement"));
                }
                let expected_backup =
                    destination.with_extension(format!("deltamod-replacing-{id}"));
                if !journal
                    .backup
                    .as_deref()
                    .is_some_and(|b| same_path(Path::new(b), &expected_backup))
                {
                    return Err(invalid("invalid backup"));
                }
                replacement.as_path()
            } else {
                if journal.backup.is_some() {
                    return Err(invalid("backup without replacement"));
                }
                destination
            };
            let expected_stage = target.with_extension(format!("importing-{id}"));
            if !journal
                .staging
                .as_deref()
                .is_some_and(|s| same_path(Path::new(s), &expected_stage))
            {
                return Err(invalid("invalid staging path"));
            }
            if journal.kind == "legacy-installation-reimport" {
                let name = destination
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default();
                if name != "deltaruneInstall" && name != format!("deltaruneInstall-reimport-{id}") {
                    return Err(invalid("reimport path does not match operation"));
                }
            }
        }
        for value in [
            &journal.destination,
            &journal.staging,
            &journal.replacement,
            &journal.backup,
        ]
        .into_iter()
        .flatten()
        {
            self.owned_directory_exists(Path::new(value))?;
        }
        Ok(())
    }

    pub(super) fn begin_commit(&self, id: u64) -> Result<(), RuntimeError> {
        let mut operations = self
            .operations
            .lock()
            .map_err(|_| invalid("operation lock poisoned"))?;
        let entry = operations
            .entries
            .get_mut(&id)
            .ok_or(RuntimeError::UnknownOperation(id))?;
        if entry.cancel {
            return Err(RuntimeError::Cancelled(id));
        }
        entry.state = OperationState::Committing;
        Ok(())
    }

    fn plan_recovery(
        &self,
        path: &Path,
        journal: &Journal,
    ) -> Result<Vec<RecoveryAction>, RuntimeError> {
        let destination =
            PathBuf::from(journal.destination.as_ref().expect("validated destination"));
        let mut actions = Vec::new();
        if matches!(
            journal.kind.as_str(),
            "legacy-profile-delete" | "installation-delete"
        ) {
            let indexed = if journal.kind == "legacy-profile-delete" {
                let index = legacy_index(
                    destination
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default(),
                )?;
                self.load_legacy_profiles()?
                    .installations
                    .iter()
                    .any(|i| i.index == Some(index))
            } else {
                self.state()?
                    .installations
                    .iter()
                    .any(|i| same_path(&i.install_path, &destination))
            };
            let live = self.owned_directory_exists(&destination)?;
            let trash = journal.replacement.as_ref().map(PathBuf::from);
            let quarantined = match &trash {
                Some(path) => self.owned_directory_exists(path)?,
                None => false,
            };
            match (indexed, live, quarantined) {
                (true, false, true) => actions.push(RecoveryAction::Restore {
                    from: trash.unwrap(),
                    to: destination,
                }),
                (false, false, true) => actions.push(RecoveryAction::Remove(trash.unwrap())),
                (_, true, true) | (true, false, false) => {
                    return Err(invalid("ambiguous deletion; recovery data retained"))
                }
                _ => {}
            }
        } else {
            if let Some(backup) = journal.backup.as_ref().map(PathBuf::from) {
                if self.owned_directory_exists(&backup)? {
                    if self.owned_directory_exists(&destination)? {
                        // Even a committed v1 journal has no content fingerprint.
                        // Do not guess whether either copy was externally changed.
                        return Err(invalid(
                            "both live and backup copies exist; recovery data retained",
                        ));
                    }
                    actions.push(RecoveryAction::Restore {
                        from: backup,
                        to: destination,
                    });
                }
            }
            for transient in [&journal.staging, &journal.replacement]
                .into_iter()
                .flatten()
            {
                actions.push(RecoveryAction::Remove(PathBuf::from(transient)));
            }
        }
        actions.push(RecoveryAction::Finish(path.to_owned()));
        Ok(actions)
    }

    pub(super) fn recover(&self) -> Result<(), RuntimeError> {
        let directory = self.root.join(".runtime-journals");
        self.owned_directory_exists(&directory)?;
        let mut journals = Vec::new();
        for (count, entry) in fs::read_dir(&directory)?.enumerate() {
            if count == MAX_JOURNALS {
                return Err(invalid("too many recovery directory entries"));
            }
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let journal = self.read_journal(&path)?;
            journals.push((path, journal));
        }
        journals.sort_by_key(|(_, j)| j.operation_id);
        let mut destinations: Vec<PathBuf> = Vec::new();
        let mut plan = Vec::new();
        // Validate the entire recovery set before the first cleanup. One malformed
        // or newer journal must not leave an order-dependent partially cleaned root.
        for (path, journal) in &journals {
            let destination = PathBuf::from(journal.destination.as_ref().unwrap());
            if destinations
                .iter()
                .any(|other| destination.starts_with(other) || other.starts_with(&destination))
            {
                return Err(invalid("overlapping recovery operations"));
            }
            destinations.push(destination);
            plan.extend(self.plan_recovery(path, journal)?);
        }
        if let Some((_, last)) = journals.last() {
            let next = self.next_operation_id()?;
            if last.operation_id >= next {
                atomic_write_json(
                    &self.root.join(".runtime-sequence.json"),
                    &last.operation_id,
                    false,
                )?;
            }
        }
        for action in plan {
            match action {
                RecoveryAction::Remove(path) => self.remove_owned_directory(&path)?,
                RecoveryAction::Restore { from, to } => {
                    self.owned_directory_exists(&from)?;
                    self.require_missing(&to)?;
                    self.rename_owned_directory(&from, &to)?;
                }
                RecoveryAction::Finish(path) => self.finish_journal(&path)?,
            }
        }
        Ok(())
    }
}

fn legacy_index(name: &str) -> Result<u32, RuntimeError> {
    name.strip_prefix("deltamod_system-")
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|index| {
            *index <= MAX_LEGACY_INSTALLATION_INDEX && name == format!("deltamod_system-{index}")
        })
        .ok_or_else(|| invalid("invalid legacy profile index"))
}

#[cfg(test)]
mod tests;
