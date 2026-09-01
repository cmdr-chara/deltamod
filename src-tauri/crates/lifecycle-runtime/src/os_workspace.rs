use crate::{
    retention::{
        PendingRecoveryDeletion, RecoveryDeletionPhase, RecoveryGenerationStorage,
        RecoveryPurgeReceipt, RecoveryRemovalReceipt, RECOVERY_QUARANTINE_PREFIX,
    },
    DurableMutationGuard, LifecycleWorkspace, StagingSource, WorkspaceRoots,
};
use deltamod_product_contracts::{
    FilesystemBoundaryError, LifecycleFilesystemBoundary, LifecycleMutationGuard,
    MutationFenceError, ObservationSnapshot, ObservedFileState, PublicationReceipt,
    RootBoundObservation, RootIdentity, ValidatedMutationReconciliation,
    ValidatedMutationTransition, ValidatedRelativePath,
};
use deltamod_tools_runtime::{
    copy_relative_regular_file_to_open_file_verified, copy_relative_regular_file_verified,
    inspect_regular_file,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(unix)]
use crate::store_identity::{
    configure_no_follow, inspect_opened, verify_opened_path, IdentityError, StableObjectIdentity,
};
use crate::store_identity::{inspect_path, StoreObjectKind};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const WORKSPACE_MARKER: &str = ".deltamod-workspace.json";

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceMarker {
    schema_version: u8,
    operation_id: String,
    recovery_generation_id: String,
}

#[derive(Clone, Debug)]
enum RegisteredSource {
    File { root: PathBuf, relative: PathBuf },
    Tree { root: PathBuf },
}

#[derive(Debug)]
struct ActiveWorkspace {
    operation_root: PathBuf,
    staging: PathBuf,
    backup: PathBuf,
    roots: WorkspaceRoots,
    #[cfg(unix)]
    operation_pin: UnixDirectoryPin,
    #[cfg(unix)]
    staging_pin: UnixDirectoryPin,
}

#[derive(Debug)]
struct RecoveryBackup {
    operation_id: String,
    operation_root: PathBuf,
    operation_identity: crate::store_identity::StableObjectIdentity,
    workspace_name: String,
    path: PathBuf,
    identity: RootIdentity,
    #[cfg(unix)]
    pin: UnixDirectoryPin,
}

type ScannedWorkspaces = (
    HashMap<String, ActiveWorkspace>,
    HashMap<String, RecoveryBackup>,
);

#[cfg(unix)]
#[derive(Debug)]
struct UnixDirectoryPin {
    opened: fs::File,
    identity: StableObjectIdentity,
}

#[cfg(unix)]
impl UnixDirectoryPin {
    fn open(path: &Path) -> Result<Self, FilesystemBoundaryError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| FilesystemBoundaryError::Io)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        let mut options = fs::OpenOptions::new();
        options.read(true);
        configure_no_follow(&mut options);
        let opened = options
            .open(path)
            .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?;
        let identity =
            verify_opened_path(path, &opened, StoreObjectKind::Directory).map_err(map_identity)?;
        Ok(Self { opened, identity })
    }

    fn verify(&self, path: &Path) -> Result<(), FilesystemBoundaryError> {
        let current = verify_opened_path(path, &self.opened, StoreObjectKind::Directory)
            .map_err(map_root_identity)?;
        if current == self.identity {
            Ok(())
        } else {
            Err(FilesystemBoundaryError::RootIdentityChanged)
        }
    }

    fn root_identity(&self, canonical: &Path) -> RootIdentity {
        root_identity_from_object(canonical, &self.identity)
    }
}

#[derive(Debug)]
pub struct OsObservation {
    snapshot: ObservationSnapshot,
    root: PathBuf,
    #[cfg(unix)]
    parent_pin: Option<UnixDirectoryPin>,
}

impl RootBoundObservation for OsObservation {
    fn snapshot(&self) -> &ObservationSnapshot {
        &self.snapshot
    }
}

/// Production filesystem adapter for lifecycle transactions on Windows,
/// Linux, and macOS. Every platform pins stable root identities, rejects link
/// aliases, and revalidates observations immediately before mutation. Windows
/// uses capability-relative mutation handles; see `OS_WORKSPACE_INTEGRATION.md`
/// for the explicitly bounded Unix pathname-mutation limitation.
pub struct OsLifecycleWorkspace {
    transaction_root: PathBuf,
    workspace_root: PathBuf,
    transaction_identity: RootIdentity,
    workspace_identity: RootIdentity,
    sources: HashMap<String, RegisteredSource>,
    active: HashMap<String, ActiveWorkspace>,
    recovery_backups: HashMap<String, RecoveryBackup>,
    workspace_object_identity: crate::store_identity::StableObjectIdentity,
    observation_sequence: AtomicU64,
    #[cfg(unix)]
    transaction_root_pin: UnixDirectoryPin,
    #[cfg(unix)]
    workspace_root_pin: UnixDirectoryPin,
}

impl OsLifecycleWorkspace {
    pub fn open(
        transaction_root: PathBuf,
        workspace_root: PathBuf,
    ) -> Result<Self, FilesystemBoundaryError> {
        let transaction_root = canonical_directory(&transaction_root)?;
        let workspace_root = canonical_directory(&workspace_root)?;
        #[cfg(unix)]
        let transaction_root_pin = UnixDirectoryPin::open(&transaction_root)?;
        #[cfg(unix)]
        let workspace_root_pin = UnixDirectoryPin::open(&workspace_root)?;
        #[cfg(unix)]
        let transaction_identity = transaction_root_pin.root_identity(&transaction_root);
        #[cfg(unix)]
        let workspace_identity = workspace_root_pin.root_identity(&workspace_root);
        #[cfg(windows)]
        let transaction_identity = root_identity(&transaction_root)?;
        #[cfg(windows)]
        let workspace_identity = root_identity(&workspace_root)?;
        if transaction_identity.same_filesystem_object(&workspace_identity) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        let workspace_object_identity =
            inspect_path(&workspace_root, StoreObjectKind::Directory).map_err(map_identity)?;
        let (active, recovery_backups) = scan_workspaces(&workspace_root, &transaction_identity)?;
        Ok(Self {
            transaction_root,
            workspace_root,
            transaction_identity,
            workspace_identity,
            sources: HashMap::new(),
            active,
            recovery_backups,
            workspace_object_identity,
            observation_sequence: AtomicU64::new(1),
            #[cfg(unix)]
            transaction_root_pin,
            #[cfg(unix)]
            workspace_root_pin,
        })
    }

    pub fn register_artifact_source(
        &mut self,
        source_id: impl Into<String>,
        path: &Path,
    ) -> Result<(), FilesystemBoundaryError> {
        let source_id = source_id.into();
        if !valid_workspace_id(&source_id, 256) || self.sources.contains_key(&source_id) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        inspect_regular_file(path, MAX_FILE_BYTES).map_err(map_secure)?;
        let root = path
            .parent()
            .ok_or(FilesystemBoundaryError::UnsafeAlias)?
            .to_owned();
        let relative = PathBuf::from(
            path.file_name()
                .ok_or(FilesystemBoundaryError::UnsafeAlias)?,
        );
        self.sources
            .insert(source_id, RegisteredSource::File { root, relative });
        Ok(())
    }

    /// Registers an immutable artifact tree whose paths mirror the transaction
    /// root. Repair plans can then address multiple files through one exact
    /// artifact identity without weakening path validation.
    pub fn register_artifact_tree_source(
        &mut self,
        source_id: impl Into<String>,
        path: &Path,
    ) -> Result<(), FilesystemBoundaryError> {
        let source_id = source_id.into();
        if !valid_workspace_id(&source_id, 256) || self.sources.contains_key(&source_id) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        let root = canonical_directory(path)?;
        let identity = root_identity(&root)?;
        if identity.same_filesystem_object(&self.transaction_identity)
            || identity.same_filesystem_object(&self.workspace_identity)
        {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        self.sources
            .insert(source_id, RegisteredSource::Tree { root });
        Ok(())
    }

    /// Measures and identity-binds one immutable completed recovery workspace.
    /// Symlinks, reparse points, hardlinks, special files, and object
    /// replacement all fail closed.
    pub fn recovery_generation_storage(
        &self,
        generation_id: &str,
        operation_id: &str,
    ) -> Result<RecoveryGenerationStorage, FilesystemBoundaryError> {
        if !valid_workspace_id(generation_id, 128) || !valid_workspace_id(operation_id, 128) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        self.verify_root_pin(&self.workspace_root, &self.workspace_identity)?;
        let backup = self
            .recovery_backups
            .get(generation_id)
            .ok_or(FilesystemBoundaryError::VerificationFailed)?;
        if backup.operation_id != operation_id
            || inspect_path(&backup.operation_root, StoreObjectKind::Directory)
                .map_err(map_identity)?
                != backup.operation_identity
        {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        verify_workspace_marker(&backup.operation_root, operation_id, generation_id)?;
        self.verify_root_pin(&backup.path, &backup.identity)?;
        let size_bytes = measure_generated_tree(&backup.path)?;
        if inspect_path(&backup.operation_root, StoreObjectKind::Directory).map_err(map_identity)?
            != backup.operation_identity
        {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        Ok(RecoveryGenerationStorage::new(
            generation_id,
            operation_id,
            backup.workspace_name.clone(),
            self.workspace_object_identity.clone(),
            backup.operation_identity.clone(),
            size_bytes,
        ))
    }

    /// Atomically moves the exact tombstoned workspace to its deterministic
    /// quarantine name. Replays accept an already-quarantined exact object and
    /// never replace a colliding destination.
    pub fn quarantine_recovery_generation(
        &mut self,
        tombstone: &PendingRecoveryDeletion,
    ) -> Result<(), FilesystemBoundaryError> {
        let Some(storage) = tombstone.storage() else {
            return Ok(());
        };
        if storage.workspace_root_identity() != &self.workspace_object_identity {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        self.verify_root_pin(&self.workspace_root, &self.workspace_identity)?;
        let source = self.workspace_root.join(storage.workspace_name());
        let quarantine = self.workspace_root.join(tombstone.quarantine_name());
        match inspect_optional_directory(&quarantine)? {
            Some(identity) if &identity == storage.workspace_object_identity() => {
                self.recovery_backups.remove(tombstone.generation_id());
                return Ok(());
            }
            Some(_) => return Err(FilesystemBoundaryError::ObservationChanged),
            None => {}
        }
        match inspect_optional_directory(&source)? {
            Some(identity) if &identity == storage.workspace_object_identity() => {}
            _ => return Err(FilesystemBoundaryError::ObservationChanged),
        }
        verify_workspace_marker(&source, tombstone.operation_id(), tombstone.generation_id())?;

        #[cfg(windows)]
        {
            let root = fence_windows::MutationRoot::open(&self.workspace_root)
                .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?;
            root.into_directory()
                .rename_child_noreplace(
                    OsStr::new(storage.workspace_name()),
                    OsStr::new(tombstone.quarantine_name()),
                )
                .map_err(|_| FilesystemBoundaryError::ObservationChanged)?;
        }
        #[cfg(unix)]
        {
            self.workspace_root_pin.verify(&self.workspace_root)?;
            rustix::fs::renameat_with(
                &self.workspace_root_pin.opened,
                storage.workspace_name(),
                &self.workspace_root_pin.opened,
                tombstone.quarantine_name(),
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|_| FilesystemBoundaryError::ObservationChanged)?;
            self.workspace_root_pin
                .opened
                .sync_all()
                .map_err(|_| FilesystemBoundaryError::Io)?;
        }
        if inspect_optional_directory(&quarantine)?.as_ref()
            != Some(storage.workspace_object_identity())
        {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        self.recovery_backups.remove(tombstone.generation_id());
        Ok(())
    }

    /// Idempotently purges quarantined data while retaining the exact marker
    /// shell as a witness for the durable `Purged` transition.
    pub fn purge_quarantined_recovery_generation(
        &mut self,
        tombstone: &PendingRecoveryDeletion,
    ) -> Result<RecoveryPurgeReceipt, FilesystemBoundaryError> {
        let Some(storage) = tombstone.storage() else {
            return Ok(RecoveryPurgeReceipt::new(tombstone));
        };
        if storage.workspace_root_identity() != &self.workspace_object_identity {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        let quarantine = self.workspace_root.join(tombstone.quarantine_name());
        match inspect_optional_directory(&quarantine)? {
            Some(identity) if &identity == storage.workspace_object_identity() => {}
            None if tombstone.phase() == RecoveryDeletionPhase::Purged => {
                return Ok(RecoveryPurgeReceipt::new(tombstone));
            }
            _ => return Err(FilesystemBoundaryError::ObservationChanged),
        }
        verify_workspace_marker(
            &quarantine,
            tombstone.operation_id(),
            tombstone.generation_id(),
        )?;
        purge_workspace_to_marker(&quarantine)?;
        if inspect_optional_directory(&quarantine)?.as_ref()
            != Some(storage.workspace_object_identity())
        {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        verify_workspace_marker(
            &quarantine,
            tombstone.operation_id(),
            tombstone.generation_id(),
        )?;
        Ok(RecoveryPurgeReceipt::new(tombstone))
    }

    /// Removes the empty exact quarantine shell after the store has durably
    /// acknowledged the purge. Absence is success, making a crash between shell
    /// removal and final metadata removal replayable.
    pub fn remove_purged_recovery_quarantine(
        &mut self,
        tombstone: &PendingRecoveryDeletion,
    ) -> Result<RecoveryRemovalReceipt, FilesystemBoundaryError> {
        if tombstone.phase() != RecoveryDeletionPhase::Purged {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        let Some(storage) = tombstone.storage() else {
            return Ok(RecoveryRemovalReceipt::new(tombstone));
        };
        if storage.workspace_root_identity() != &self.workspace_object_identity {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        let quarantine = self.workspace_root.join(tombstone.quarantine_name());
        match inspect_optional_directory(&quarantine)? {
            None => return Ok(RecoveryRemovalReceipt::new(tombstone)),
            Some(identity) if &identity == storage.workspace_object_identity() => {}
            Some(_) => return Err(FilesystemBoundaryError::ObservationChanged),
        }
        ensure_marker_shell(
            &quarantine,
            tombstone.operation_id(),
            tombstone.generation_id(),
        )?;
        remove_generated_tree(&quarantine)?;
        if inspect_optional_directory(&quarantine)?.is_some() {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        Ok(RecoveryRemovalReceipt::new(tombstone))
    }

    fn next_sequence(&self) -> u64 {
        self.observation_sequence.fetch_add(1, Ordering::Relaxed)
    }

    fn root_path(&self, expected: &RootIdentity) -> Result<&Path, FilesystemBoundaryError> {
        if expected == &self.transaction_identity {
            return Ok(&self.transaction_root);
        }
        self.active
            .values()
            .find_map(|workspace| {
                if expected == &workspace.roots.staging {
                    Some(workspace.staging.as_path())
                } else if expected == &workspace.roots.backup {
                    Some(workspace.backup.as_path())
                } else {
                    None
                }
            })
            .or_else(|| {
                self.recovery_backups
                    .values()
                    .find(|backup| &backup.identity == expected)
                    .map(|backup| backup.path.as_path())
            })
            .ok_or(FilesystemBoundaryError::RootIdentityChanged)
    }

    #[cfg(unix)]
    fn verify_root_pin(
        &self,
        root: &Path,
        expected: &RootIdentity,
    ) -> Result<(), FilesystemBoundaryError> {
        if expected == &self.transaction_identity {
            return self.transaction_root_pin.verify(root);
        }
        if expected == &self.workspace_identity {
            return self.workspace_root_pin.verify(root);
        }
        if let Some(workspace) = self.active.values().find(|workspace| {
            expected == &workspace.roots.staging || expected == &workspace.roots.backup
        }) {
            if expected == &workspace.roots.staging {
                return workspace.staging_pin.verify(root);
            }
            let backup = self
                .recovery_backups
                .values()
                .find(|backup| &backup.identity == expected)
                .ok_or(FilesystemBoundaryError::RootIdentityChanged)?;
            return backup.pin.verify(root);
        }
        self.recovery_backups
            .values()
            .find(|backup| &backup.identity == expected)
            .ok_or(FilesystemBoundaryError::RootIdentityChanged)?
            .pin
            .verify(root)
    }

    #[cfg(windows)]
    fn verify_root_pin(
        &self,
        root: &Path,
        expected: &RootIdentity,
    ) -> Result<(), FilesystemBoundaryError> {
        if root_identity(root)? == *expected {
            Ok(())
        } else {
            Err(FilesystemBoundaryError::RootIdentityChanged)
        }
    }

    fn observe_at(
        &self,
        root: &Path,
        expected: &RootIdentity,
        relative: &ValidatedRelativePath,
    ) -> Result<OsObservation, FilesystemBoundaryError> {
        self.verify_root_pin(root, expected)?;
        let relative_path = Path::new(relative.as_str());
        validate_relative_components(relative_path)?;
        verify_ancestors(root, relative_path)?;
        let absolute = root.join(relative_path);
        let state = match fs::symlink_metadata(&absolute) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => ObservedFileState::Missing,
            Err(_) => ObservedFileState::Unreadable,
            Ok(metadata) if metadata.file_type().is_symlink() || is_reparse(&metadata) => {
                return Err(FilesystemBoundaryError::UnsafeAlias)
            }
            Ok(metadata) if !metadata.is_file() => ObservedFileState::NonRegular,
            Ok(_) => {
                let verified =
                    inspect_regular_file(&absolute, MAX_FILE_BYTES).map_err(map_secure)?;
                let identity =
                    inspect_path(&absolute, StoreObjectKind::RegularFile).map_err(map_identity)?;
                ObservedFileState::Regular {
                    sha256: verified.sha256().to_owned(),
                    file_id: format!("{}-{}", identity.volume_id, identity.file_id),
                    link_count: 1,
                }
            }
        };
        Ok(OsObservation {
            snapshot: ObservationSnapshot {
                root_identity: expected.clone(),
                path: relative.clone(),
                path_identity_key: identity_key(relative.as_str()),
                state,
                observation_sequence: self.next_sequence(),
            },
            root: root.to_owned(),
            #[cfg(unix)]
            parent_pin: pin_existing_parent(root, relative_path)?,
        })
    }

    fn ensure_current(&self, observation: &OsObservation) -> Result<(), FilesystemBoundaryError> {
        let current = self.observe_at(
            &observation.root,
            &observation.snapshot.root_identity,
            &observation.snapshot.path,
        )?;
        #[cfg(unix)]
        if let Some(parent_pin) = &observation.parent_pin {
            let parent = observation.root.join(
                Path::new(observation.snapshot.path.as_str())
                    .parent()
                    .unwrap_or(Path::new("")),
            );
            parent_pin.verify(&parent)?;
        }
        if current.snapshot.state == observation.snapshot.state
            && current.snapshot.path_identity_key == observation.snapshot.path_identity_key
        {
            Ok(())
        } else {
            Err(FilesystemBoundaryError::ObservationChanged)
        }
    }

    fn receipt(
        guard: &impl LifecycleMutationGuard,
        destination: &OsObservation,
        sha256: Option<String>,
    ) -> PublicationReceipt {
        PublicationReceipt {
            root_identity: destination.snapshot.root_identity.clone(),
            path: destination.snapshot.path.clone(),
            path_identity_key: destination.snapshot.path_identity_key.clone(),
            sha256,
            operation_id: guard.operation_id().to_owned(),
            lease_id: guard.lease_id().to_owned(),
            fencing_token: guard.fencing_token(),
            operation_revision: guard.operation_revision(),
            journal_sequence: guard.journal_sequence(),
        }
    }
}

impl LifecycleWorkspace for OsLifecycleWorkspace {
    fn transaction_root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError> {
        #[cfg(unix)]
        {
            self.transaction_root_pin.verify(&self.transaction_root)?;
            return Ok(self.transaction_identity.clone());
        }
        #[cfg(windows)]
        let current = root_identity(&self.transaction_root)?;
        #[cfg(windows)]
        (current == self.transaction_identity)
            .then_some(current)
            .ok_or(FilesystemBoundaryError::RootIdentityChanged)
    }

    fn observe_preflight_no_follow(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError> {
        Ok(self
            .observe_at(&self.transaction_root, expected_root, path)?
            .snapshot)
    }

    fn prepare_workspace(
        &mut self,
        operation_id: &str,
        recovery_generation_id: &str,
        expected_transaction_root: &RootIdentity,
    ) -> Result<WorkspaceRoots, FilesystemBoundaryError> {
        if !valid_workspace_id(operation_id, 128)
            || !valid_workspace_id(recovery_generation_id, 128)
            || self.transaction_root_identity()? != *expected_transaction_root
            || self.active.contains_key(operation_id)
        {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        self.verify_root_pin(&self.workspace_root, &self.workspace_identity)?;
        let operation_root = self
            .workspace_root
            .join(format!("{operation_id}-{recovery_generation_id}"));
        fs::create_dir(&operation_root).map_err(|_| FilesystemBoundaryError::Io)?;
        let workspace_name = operation_root
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or(FilesystemBoundaryError::UnsafeAlias)?
            .to_owned();
        #[cfg(unix)]
        let operation_pin = UnixDirectoryPin::open(&operation_root)?;
        let operation_identity =
            inspect_path(&operation_root, StoreObjectKind::Directory).map_err(map_identity)?;
        let staging = operation_root.join("staging-root");
        let backup = operation_root.join("backup-root");
        fs::create_dir(&staging).map_err(|_| FilesystemBoundaryError::Io)?;
        fs::create_dir(&backup).map_err(|_| FilesystemBoundaryError::Io)?;
        #[cfg(unix)]
        let staging_pin = UnixDirectoryPin::open(&staging)?;
        #[cfg(unix)]
        let backup_pin = UnixDirectoryPin::open(&backup)?;
        write_workspace_marker(
            &operation_root,
            &WorkspaceMarker {
                schema_version: 1,
                operation_id: operation_id.to_owned(),
                recovery_generation_id: recovery_generation_id.to_owned(),
            },
        )?;
        #[cfg(unix)]
        let staging_identity = staging_pin.root_identity(&staging);
        #[cfg(unix)]
        let backup_identity = backup_pin.root_identity(&backup);
        #[cfg(windows)]
        let staging_identity = root_identity(&staging)?;
        #[cfg(windows)]
        let backup_identity = root_identity(&backup)?;
        let roots = WorkspaceRoots {
            transaction: expected_transaction_root.clone(),
            staging: staging_identity,
            backup: backup_identity.clone(),
        };
        self.recovery_backups.insert(
            recovery_generation_id.to_owned(),
            RecoveryBackup {
                operation_id: operation_id.to_owned(),
                operation_root: operation_root.clone(),
                operation_identity,
                workspace_name,
                path: backup.clone(),
                identity: backup_identity,
                #[cfg(unix)]
                pin: backup_pin,
            },
        );
        self.active.insert(
            operation_id.to_owned(),
            ActiveWorkspace {
                operation_root,
                staging,
                backup,
                roots: roots.clone(),
                #[cfg(unix)]
                operation_pin,
                #[cfg(unix)]
                staging_pin,
            },
        );
        Ok(roots)
    }

    fn stage_file(
        &mut self,
        staging_root: &RootIdentity,
        staging_path: &ValidatedRelativePath,
        source: &StagingSource,
        expected_sha256: &str,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError> {
        let staging = self.root_path(staging_root)?.to_owned();
        let registered = match source {
            StagingSource::Artifact { source_id } => self
                .sources
                .get(source_id)
                .cloned()
                .ok_or(FilesystemBoundaryError::VerificationFailed)?,
            StagingSource::ArtifactTree {
                source_id,
                source_path,
            } => match self
                .sources
                .get(source_id)
                .cloned()
                .ok_or(FilesystemBoundaryError::VerificationFailed)?
            {
                RegisteredSource::Tree { root } => RegisteredSource::File {
                    root,
                    relative: PathBuf::from(source_path.as_str()),
                },
                RegisteredSource::File { .. } => return Err(FilesystemBoundaryError::UnsafeAlias),
            },
            StagingSource::RecoveryBackup {
                generation_id,
                backup_path,
            } => RegisteredSource::File {
                root: self
                    .recovery_backups
                    .get(generation_id)
                    .map(|backup| backup.path.clone())
                    .ok_or(FilesystemBoundaryError::VerificationFailed)?,
                relative: PathBuf::from(backup_path.as_str()),
            },
        };
        let (source_root, source_relative) = match registered {
            RegisteredSource::File { root, relative } => (root, relative),
            RegisteredSource::Tree { root } => (root, PathBuf::from(staging_path.as_str())),
        };
        let destination = staging.join(staging_path.as_str());
        create_safe_parents(&staging, Path::new(staging_path.as_str()))?;
        let copied = copy_relative_regular_file_verified(
            &source_root,
            &source_relative,
            &destination,
            MAX_FILE_BYTES,
        )
        .map_err(map_secure)?;
        if !copied.sha256().eq_ignore_ascii_case(expected_sha256) {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        Ok(self
            .observe_at(&staging, staging_root, staging_path)?
            .snapshot)
    }

    fn backup_file(
        &mut self,
        transaction_root: &RootIdentity,
        destination_path: &ValidatedRelativePath,
        backup_root: &RootIdentity,
        backup_path: &ValidatedRelativePath,
        expected_sha256: &str,
    ) -> Result<ObservationSnapshot, FilesystemBoundaryError> {
        let transaction = self.root_path(transaction_root)?.to_owned();
        let backup = self.root_path(backup_root)?.to_owned();
        create_safe_parents(&backup, Path::new(backup_path.as_str()))?;
        let copied = copy_relative_regular_file_verified(
            &transaction,
            Path::new(destination_path.as_str()),
            &backup.join(backup_path.as_str()),
            MAX_FILE_BYTES,
        )
        .map_err(map_secure)?;
        if !copied.sha256().eq_ignore_ascii_case(expected_sha256) {
            return Err(FilesystemBoundaryError::ObservationChanged);
        }
        Ok(self.observe_at(&backup, backup_root, backup_path)?.snapshot)
    }

    fn cleanup_workspace(
        &mut self,
        roots: &WorkspaceRoots,
        operation_id: &str,
        _recovery_generation_id: &str,
        retain_backup: bool,
    ) -> Result<(), FilesystemBoundaryError> {
        let workspace = self
            .active
            .get(operation_id)
            .ok_or(FilesystemBoundaryError::RootIdentityChanged)?;
        if workspace.roots != *roots {
            return Err(FilesystemBoundaryError::RootIdentityChanged);
        }
        #[cfg(unix)]
        {
            workspace.operation_pin.verify(&workspace.operation_root)?;
            workspace.staging_pin.verify(&workspace.staging)?;
            self.recovery_backups
                .get(_recovery_generation_id)
                .ok_or(FilesystemBoundaryError::RootIdentityChanged)?
                .pin
                .verify(&workspace.backup)?;
        }
        remove_generated_tree(&workspace.staging)?;
        if !retain_backup {
            remove_generated_tree(&workspace.backup)?;
            fs::remove_file(workspace.operation_root.join(WORKSPACE_MARKER))
                .map_err(|_| FilesystemBoundaryError::Io)?;
            fs::remove_dir(&workspace.operation_root).map_err(|_| FilesystemBoundaryError::Io)?;
        }
        self.active.remove(operation_id);
        if !retain_backup {
            self.recovery_backups.remove(_recovery_generation_id);
        }
        Ok(())
    }
}

impl<'guard> LifecycleFilesystemBoundary<DurableMutationGuard<'guard>> for OsLifecycleWorkspace {
    type Observation = OsObservation;

    fn root_identity(&self) -> Result<RootIdentity, FilesystemBoundaryError> {
        self.transaction_root_identity()
    }

    fn observe_no_follow(
        &self,
        expected_root: &RootIdentity,
        path: &ValidatedRelativePath,
    ) -> Result<Self::Observation, FilesystemBoundaryError> {
        let root = self.root_path(expected_root)?.to_owned();
        self.observe_at(&root, expected_root, path)
    }

    fn publish_verified(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        staged: &Self::Observation,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(map_fence)?;
        self.ensure_current(staged)?;
        self.ensure_current(destination)?;
        let sha256 = transition.validate_publication(&staged.snapshot, &destination.snapshot)?;
        publish_from_observation(
            staged,
            destination,
            transition.mutation().action == deltamod_product_contracts::MutationAction::Replace,
            &sha256,
        )?;
        let published = self.observe_at(
            &destination.root,
            &destination.snapshot.root_identity,
            &destination.snapshot.path,
        )?;
        if !matches!(&published.snapshot.state, ObservedFileState::Regular { sha256: actual, link_count: 1, .. } if actual.eq_ignore_ascii_case(&sha256))
        {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        guard
            .checkpoint_after_side_effect(now_ms, transition)
            .map_err(map_fence)?;
        Ok(Self::receipt(guard, destination, Some(sha256)))
    }

    fn remove_verified(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(map_fence)?;
        self.ensure_current(destination)?;
        transition.validate_removal(&destination.snapshot)?;
        remove_observation(destination)?;
        guard
            .checkpoint_after_side_effect(now_ms, transition)
            .map_err(map_fence)?;
        Ok(Self::receipt(guard, destination, None))
    }

    fn restore_verified_backup(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        backup: &Self::Observation,
        destination: &Self::Observation,
        transition: &ValidatedMutationTransition,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(map_fence)?;
        self.ensure_current(backup)?;
        self.ensure_current(destination)?;
        let sha256 =
            transition.validate_backup_restoration(&backup.snapshot, &destination.snapshot)?;
        publish_from_observation(
            backup,
            destination,
            transition.mutation().action == deltamod_product_contracts::MutationAction::Replace,
            &sha256,
        )?;
        guard
            .checkpoint_after_side_effect(now_ms, transition)
            .map_err(map_fence)?;
        Ok(Self::receipt(guard, destination, Some(sha256)))
    }

    fn reconcile_uncertain_effect(
        &mut self,
        guard: &mut DurableMutationGuard<'guard>,
        now_ms: u64,
        destination: &Self::Observation,
        reconciliation: &ValidatedMutationReconciliation,
    ) -> Result<PublicationReceipt, FilesystemBoundaryError> {
        guard.assert_current(now_ms).map_err(map_fence)?;
        self.ensure_current(destination)?;
        reconciliation.validate_destination_observation(&destination.snapshot)?;
        let sha256 = match &destination.snapshot.state {
            ObservedFileState::Regular { sha256, .. } => Some(sha256.clone()),
            _ => None,
        };
        guard
            .checkpoint_after_reconciliation(now_ms, reconciliation)
            .map_err(map_fence)?;
        Ok(Self::receipt(guard, destination, sha256))
    }
}

#[cfg(windows)]
fn publish_from_observation(
    source: &OsObservation,
    destination: &OsObservation,
    replace: bool,
    expected_sha256: &str,
) -> Result<(), FilesystemBoundaryError> {
    let relative = Path::new(destination.snapshot.path.as_str());
    let parent_relative = relative.parent().unwrap_or(Path::new(""));
    let name = relative
        .file_name()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    let root = fence_windows::MutationRoot::open(&destination.root)
        .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?;
    let mut parent = root.into_directory();
    for component in parent_relative.components() {
        let Component::Normal(component) = component else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        parent = parent
            .open_mutation_directory(component)
            .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?;
    }
    let temp_name = format!(
        ".deltamod-publish-{}-{}.tmp",
        std::process::id(),
        destination.snapshot.observation_sequence
    );
    let mut output = parent
        .create_new_file(OsStr::new(&temp_name))
        .map_err(|_| FilesystemBoundaryError::Io)?;
    let result = copy_relative_regular_file_to_open_file_verified(
        &source.root,
        Path::new(source.snapshot.path.as_str()),
        &mut output,
        MAX_FILE_BYTES,
    )
    .map_err(map_secure);
    drop(output);
    let (_size, sha256) = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = parent.remove_child(OsStr::new(&temp_name));
            return Err(error);
        }
    };
    if !sha256.eq_ignore_ascii_case(expected_sha256) {
        let _ = parent.remove_child(OsStr::new(&temp_name));
        return Err(FilesystemBoundaryError::VerificationFailed);
    }
    let mutation = if replace {
        parent.replace_child(OsStr::new(&temp_name), name)
    } else {
        parent.rename_child_noreplace(OsStr::new(&temp_name), name)
    };
    mutation.map_err(|_| FilesystemBoundaryError::ObservationChanged)
}

#[cfg(unix)]
fn publish_from_observation(
    source: &OsObservation,
    destination: &OsObservation,
    replace: bool,
    expected_sha256: &str,
) -> Result<(), FilesystemBoundaryError> {
    let source_relative = Path::new(source.snapshot.path.as_str());
    let destination_relative = Path::new(destination.snapshot.path.as_str());
    validate_relative_components(source_relative)?;
    validate_relative_components(destination_relative)?;
    let destination_parent = destination
        .root
        .join(destination_relative.parent().unwrap_or(Path::new("")));
    let parent_pin = destination
        .parent_pin
        .as_ref()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    ensure_observation_path_current(source)?;
    ensure_observation_path_current(destination)?;
    parent_pin.verify(&destination_parent)?;

    let temp_name = format!(
        ".deltamod-publish-{}-{}.tmp",
        std::process::id(),
        destination.snapshot.observation_sequence
    );
    let destination_path = destination.root.join(destination_relative);
    let descriptor = rustix::fs::openat(
        &parent_pin.opened,
        temp_name.as_str(),
        rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .map_err(|_| FilesystemBoundaryError::ObservationChanged)?;
    let mut output = fs::File::from(descriptor);
    let temp_identity =
        inspect_opened(&output, StoreObjectKind::RegularFile).map_err(map_identity)?;
    if let Err(error) = parent_pin.verify(&destination_parent) {
        drop(output);
        remove_owned_temp(parent_pin, &temp_name, &temp_identity);
        return Err(error);
    }

    let copied = copy_relative_regular_file_to_open_file_verified(
        &source.root,
        source_relative,
        &mut output,
        MAX_FILE_BYTES,
    )
    .map_err(map_secure);
    let (_size, sha256) = match copied {
        Ok(value) => value,
        Err(error) => {
            drop(output);
            remove_owned_temp(parent_pin, &temp_name, &temp_identity);
            return Err(error);
        }
    };
    if !sha256.eq_ignore_ascii_case(expected_sha256) {
        drop(output);
        remove_owned_temp(parent_pin, &temp_name, &temp_identity);
        return Err(FilesystemBoundaryError::VerificationFailed);
    }
    output.sync_all().map_err(|_| FilesystemBoundaryError::Io)?;
    if inspect_opened(&output, StoreObjectKind::RegularFile).map_err(map_identity)? != temp_identity
    {
        drop(output);
        remove_owned_temp(parent_pin, &temp_name, &temp_identity);
        return Err(FilesystemBoundaryError::ObservationChanged);
    }

    // Copying may be long-running. Reassert the exact destination snapshot and
    // parent identity immediately before the externally visible mutation.
    ensure_observation_path_current(destination)?;
    parent_pin.verify(&destination_parent)?;
    let name = destination_relative
        .file_name()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    let mutation = if replace {
        rustix::fs::renameat(
            &parent_pin.opened,
            temp_name.as_str(),
            &parent_pin.opened,
            name,
        )
    } else {
        rustix::fs::renameat_with(
            &parent_pin.opened,
            temp_name.as_str(),
            &parent_pin.opened,
            name,
            rustix::fs::RenameFlags::NOREPLACE,
        )
    };
    if mutation.is_err() {
        drop(output);
        remove_owned_temp(parent_pin, &temp_name, &temp_identity);
        return Err(FilesystemBoundaryError::ObservationChanged);
    }
    let published_identity =
        inspect_opened(&output, StoreObjectKind::RegularFile).map_err(map_identity)?;
    if published_identity != temp_identity {
        return Err(FilesystemBoundaryError::ObservationChanged);
    }
    let published = inspect_regular_file(&destination_path, MAX_FILE_BYTES).map_err(map_secure)?;
    if !published.sha256().eq_ignore_ascii_case(expected_sha256) {
        return Err(FilesystemBoundaryError::VerificationFailed);
    }
    parent_pin
        .opened
        .sync_all()
        .map_err(|_| FilesystemBoundaryError::Io)
}

#[cfg(windows)]
fn remove_observation(destination: &OsObservation) -> Result<(), FilesystemBoundaryError> {
    let relative = Path::new(destination.snapshot.path.as_str());
    let name = relative
        .file_name()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    let root = fence_windows::MutationRoot::open(&destination.root)
        .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?;
    let mut parent = root.into_directory();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(component) = component else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        parent = parent
            .open_mutation_directory(component)
            .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?;
    }
    parent
        .remove_child(name)
        .map_err(|_| FilesystemBoundaryError::ObservationChanged)
}

#[cfg(unix)]
fn remove_observation(destination: &OsObservation) -> Result<(), FilesystemBoundaryError> {
    let relative = Path::new(destination.snapshot.path.as_str());
    validate_relative_components(relative)?;
    let parent_path = destination
        .root
        .join(relative.parent().unwrap_or(Path::new("")));
    let parent_pin = destination
        .parent_pin
        .as_ref()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    ensure_observation_path_current(destination)?;
    parent_pin.verify(&parent_path)?;
    let name = relative
        .file_name()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    rustix::fs::unlinkat(&parent_pin.opened, name, rustix::fs::AtFlags::empty())
        .map_err(|_| FilesystemBoundaryError::ObservationChanged)?;
    parent_pin
        .opened
        .sync_all()
        .map_err(|_| FilesystemBoundaryError::Io)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, FilesystemBoundaryError> {
    let canonical = fs::canonicalize(path).map_err(|_| FilesystemBoundaryError::Io)?;
    inspect_path(&canonical, StoreObjectKind::Directory).map_err(map_identity)?;
    Ok(canonical)
}

fn scan_workspaces(
    workspace_root: &Path,
    transaction_identity: &RootIdentity,
) -> Result<ScannedWorkspaces, FilesystemBoundaryError> {
    let mut active = HashMap::new();
    let mut recovery = HashMap::new();
    for entry in fs::read_dir(workspace_root).map_err(|_| FilesystemBoundaryError::Io)? {
        let path = entry.map_err(|_| FilesystemBoundaryError::Io)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| FilesystemBoundaryError::Io)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        let workspace_name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or(FilesystemBoundaryError::UnsafeAlias)?
            .to_owned();
        if workspace_name.starts_with(RECOVERY_QUARANTINE_PREFIX) {
            inspect_path(&path, StoreObjectKind::Directory).map_err(map_identity)?;
            continue;
        }
        let marker_path = path.join(WORKSPACE_MARKER);
        let marker_metadata = fs::symlink_metadata(&marker_path)
            .map_err(|_| FilesystemBoundaryError::VerificationFailed)?;
        if !marker_metadata.is_file()
            || marker_metadata.file_type().is_symlink()
            || is_reparse(&marker_metadata)
            || marker_metadata.len() > 16 * 1024
        {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        let marker: WorkspaceMarker = serde_json::from_slice(
            &fs::read(&marker_path).map_err(|_| FilesystemBoundaryError::Io)?,
        )
        .map_err(|_| FilesystemBoundaryError::VerificationFailed)?;
        if marker.schema_version != 1
            || !valid_workspace_id(&marker.operation_id, 128)
            || !valid_workspace_id(&marker.recovery_generation_id, 128)
            || active.contains_key(&marker.operation_id)
            || recovery.contains_key(&marker.recovery_generation_id)
        {
            return Err(FilesystemBoundaryError::VerificationFailed);
        }
        let staging = path.join("staging-root");
        let backup = path.join("backup-root");
        let operation_identity =
            inspect_path(&path, StoreObjectKind::Directory).map_err(map_identity)?;
        #[cfg(unix)]
        let operation_pin = UnixDirectoryPin::open(&path)?;
        #[cfg(unix)]
        let backup_pin = UnixDirectoryPin::open(&backup)?;
        #[cfg(unix)]
        let backup_identity = backup_pin.root_identity(&backup);
        #[cfg(windows)]
        let backup_identity = root_identity(&backup)?;
        recovery.insert(
            marker.recovery_generation_id,
            RecoveryBackup {
                operation_id: marker.operation_id.clone(),
                operation_root: path.clone(),
                operation_identity,
                workspace_name,
                path: backup.clone(),
                identity: backup_identity.clone(),
                #[cfg(unix)]
                pin: backup_pin,
            },
        );
        if staging.is_dir() {
            #[cfg(unix)]
            let staging_pin = UnixDirectoryPin::open(&staging)?;
            #[cfg(unix)]
            let staging_identity = staging_pin.root_identity(&staging);
            #[cfg(windows)]
            let staging_identity = root_identity(&staging)?;
            let roots = WorkspaceRoots {
                transaction: transaction_identity.clone(),
                staging: staging_identity,
                backup: backup_identity,
            };
            active.insert(
                marker.operation_id,
                ActiveWorkspace {
                    operation_root: path,
                    staging,
                    backup,
                    roots,
                    #[cfg(unix)]
                    operation_pin,
                    #[cfg(unix)]
                    staging_pin,
                },
            );
        }
    }
    Ok((active, recovery))
}

fn write_workspace_marker(
    operation_root: &Path,
    marker: &WorkspaceMarker,
) -> Result<(), FilesystemBoundaryError> {
    use std::io::Write as _;
    let path = operation_root.join(WORKSPACE_MARKER);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| FilesystemBoundaryError::Io)?;
    let bytes =
        serde_json::to_vec(marker).map_err(|_| FilesystemBoundaryError::VerificationFailed)?;
    file.write_all(&bytes)
        .map_err(|_| FilesystemBoundaryError::Io)?;
    file.flush().map_err(|_| FilesystemBoundaryError::Io)?;
    file.sync_all().map_err(|_| FilesystemBoundaryError::Io)
}

fn read_workspace_marker(path: &Path) -> Result<WorkspaceMarker, FilesystemBoundaryError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FilesystemBoundaryError::Io)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || is_reparse(&metadata)
        || metadata.len() > 16 * 1024
    {
        return Err(FilesystemBoundaryError::UnsafeAlias);
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| FilesystemBoundaryError::Io)?)
        .map_err(|_| FilesystemBoundaryError::VerificationFailed)
}

fn verify_workspace_marker(
    operation_root: &Path,
    operation_id: &str,
    generation_id: &str,
) -> Result<(), FilesystemBoundaryError> {
    let marker = read_workspace_marker(&operation_root.join(WORKSPACE_MARKER))?;
    if marker.schema_version == 1
        && marker.operation_id == operation_id
        && marker.recovery_generation_id == generation_id
    {
        Ok(())
    } else {
        Err(FilesystemBoundaryError::VerificationFailed)
    }
}

fn inspect_optional_directory(
    path: &Path,
) -> Result<Option<crate::store_identity::StableObjectIdentity>, FilesystemBoundaryError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(FilesystemBoundaryError::Io),
        Ok(metadata)
            if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse(&metadata) =>
        {
            Err(FilesystemBoundaryError::UnsafeAlias)
        }
        Ok(_) => inspect_path(path, StoreObjectKind::Directory)
            .map(Some)
            .map_err(map_identity),
    }
}

fn measure_generated_tree(root: &Path) -> Result<u64, FilesystemBoundaryError> {
    let expected_root = inspect_path(root, StoreObjectKind::Directory).map_err(map_identity)?;
    let size = measure_generated_tree_inner(root)?;
    if inspect_path(root, StoreObjectKind::Directory).map_err(map_identity)? != expected_root {
        return Err(FilesystemBoundaryError::ObservationChanged);
    }
    Ok(size)
}

fn measure_generated_tree_inner(root: &Path) -> Result<u64, FilesystemBoundaryError> {
    let mut total = 0_u64;
    for entry in fs::read_dir(root).map_err(|_| FilesystemBoundaryError::Io)? {
        let path = entry.map_err(|_| FilesystemBoundaryError::Io)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| FilesystemBoundaryError::Io)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        let size = if metadata.is_dir() {
            inspect_path(&path, StoreObjectKind::Directory).map_err(map_identity)?;
            measure_generated_tree_inner(&path)?
        } else if metadata.is_file() {
            inspect_path(&path, StoreObjectKind::RegularFile).map_err(map_identity)?;
            metadata.len()
        } else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        total = total
            .checked_add(size)
            .ok_or(FilesystemBoundaryError::VerificationFailed)?;
    }
    Ok(total)
}

fn purge_workspace_to_marker(root: &Path) -> Result<(), FilesystemBoundaryError> {
    for entry in fs::read_dir(root).map_err(|_| FilesystemBoundaryError::Io)? {
        let path = entry.map_err(|_| FilesystemBoundaryError::Io)?.path();
        if path.file_name() == Some(OsStr::new(WORKSPACE_MARKER)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|_| FilesystemBoundaryError::Io)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        if metadata.is_dir() {
            remove_generated_tree(&path)?;
        } else if metadata.is_file() {
            inspect_path(&path, StoreObjectKind::RegularFile).map_err(map_identity)?;
            fs::remove_file(&path).map_err(|_| FilesystemBoundaryError::Io)?;
        } else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
    }
    sync_directory(root)
}

fn ensure_marker_shell(
    root: &Path,
    operation_id: &str,
    generation_id: &str,
) -> Result<(), FilesystemBoundaryError> {
    verify_workspace_marker(root, operation_id, generation_id)?;
    let mut entries = fs::read_dir(root).map_err(|_| FilesystemBoundaryError::Io)?;
    let first = entries
        .next()
        .transpose()
        .map_err(|_| FilesystemBoundaryError::Io)?
        .ok_or(FilesystemBoundaryError::VerificationFailed)?;
    if first.file_name() != OsStr::new(WORKSPACE_MARKER)
        || entries
            .next()
            .transpose()
            .map_err(|_| FilesystemBoundaryError::Io)?
            .is_some()
    {
        return Err(FilesystemBoundaryError::VerificationFailed);
    }
    Ok(())
}

fn root_identity(path: &Path) -> Result<RootIdentity, FilesystemBoundaryError> {
    let canonical = canonical_directory(path)?;
    let identity = inspect_path(&canonical, StoreObjectKind::Directory).map_err(map_identity)?;
    Ok(root_identity_from_object(&canonical, &identity))
}

fn root_identity_from_object(
    canonical: &Path,
    identity: &crate::store_identity::StableObjectIdentity,
) -> RootIdentity {
    #[cfg(windows)]
    let canonical_bytes = canonical.to_string_lossy().to_lowercase().into_bytes();
    #[cfg(unix)]
    let canonical_bytes = {
        use std::os::unix::ffi::OsStrExt as _;

        canonical.as_os_str().as_bytes().to_vec()
    };
    RootIdentity {
        canonical_path_sha256: hex_digest(&canonical_bytes),
        volume_id: identity.volume_id.to_string(),
        file_id: identity.file_id.to_string(),
    }
}

#[cfg(unix)]
fn pin_existing_parent(
    root: &Path,
    relative: &Path,
) -> Result<Option<UnixDirectoryPin>, FilesystemBoundaryError> {
    let mut current = UnixDirectoryPin::open(root)?;
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(component) = component else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        let descriptor = match rustix::fs::openat(
            &current.opened,
            component,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        ) {
            Ok(descriptor) => descriptor,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(_) => return Err(FilesystemBoundaryError::UnsafeAlias),
        };
        let opened = fs::File::from(descriptor);
        let identity = inspect_opened(&opened, StoreObjectKind::Directory).map_err(map_identity)?;
        current = UnixDirectoryPin { opened, identity };
    }
    Ok(Some(current))
}

#[cfg(unix)]
fn ensure_observation_path_current(
    observation: &OsObservation,
) -> Result<(), FilesystemBoundaryError> {
    let relative = Path::new(observation.snapshot.path.as_str());
    validate_relative_components(relative)?;
    verify_ancestors(&observation.root, relative)?;
    if let Some(parent_pin) = &observation.parent_pin {
        parent_pin.verify(
            &observation
                .root
                .join(relative.parent().unwrap_or(Path::new(""))),
        )?;
    }
    let parent = observation
        .parent_pin
        .as_ref()
        .ok_or(FilesystemBoundaryError::ObservationChanged)?;
    let name = relative
        .file_name()
        .ok_or(FilesystemBoundaryError::UnsafeAlias)?;
    let opened = rustix::fs::openat(
        &parent.opened,
        name,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    );
    match (&observation.snapshot.state, opened) {
        (ObservedFileState::Missing, Err(rustix::io::Errno::NOENT)) => Ok(()),
        (ObservedFileState::Regular { file_id, .. }, Ok(descriptor)) => {
            let file = fs::File::from(descriptor);
            let identity =
                inspect_opened(&file, StoreObjectKind::RegularFile).map_err(map_identity)?;
            if format!("{}-{}", identity.volume_id, identity.file_id) == *file_id {
                Ok(())
            } else {
                Err(FilesystemBoundaryError::ObservationChanged)
            }
        }
        _ => Err(FilesystemBoundaryError::ObservationChanged),
    }
}

#[cfg(unix)]
fn remove_owned_temp(parent: &UnixDirectoryPin, name: &str, identity: &StableObjectIdentity) {
    let opened = rustix::fs::openat(
        &parent.opened,
        name,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    );
    if let Ok(descriptor) = opened {
        let file = fs::File::from(descriptor);
        if inspect_opened(&file, StoreObjectKind::RegularFile)
            .is_ok_and(|current| &current == identity)
        {
            let _ = rustix::fs::unlinkat(&parent.opened, name, rustix::fs::AtFlags::empty());
        }
    }
}

fn verify_ancestors(root: &Path, relative: &Path) -> Result<(), FilesystemBoundaryError> {
    let mut current = root.to_owned();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(component) = component else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && !is_reparse(&metadata) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            _ => return Err(FilesystemBoundaryError::UnsafeAlias),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn create_safe_parents(root: &Path, relative: &Path) -> Result<(), FilesystemBoundaryError> {
    let mut current = root.to_owned();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(component) = component else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata)
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && !is_reparse(&metadata) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| FilesystemBoundaryError::Io)?
            }
            _ => return Err(FilesystemBoundaryError::UnsafeAlias),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_safe_parents(root: &Path, relative: &Path) -> Result<(), FilesystemBoundaryError> {
    validate_relative_components(relative)?;
    let mut current = UnixDirectoryPin::open(root)?.opened;
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(component) = component else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        };
        let flags = rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC;
        let descriptor =
            match rustix::fs::openat(&current, component, flags, rustix::fs::Mode::empty()) {
                Ok(descriptor) => descriptor,
                Err(rustix::io::Errno::NOENT) => {
                    rustix::fs::mkdirat(
                        &current,
                        component,
                        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR | rustix::fs::Mode::XUSR,
                    )
                    .or_else(|error| {
                        (error == rustix::io::Errno::EXIST)
                            .then_some(())
                            .ok_or(error)
                    })?;
                    rustix::fs::openat(&current, component, flags, rustix::fs::Mode::empty())
                        .map_err(|_| FilesystemBoundaryError::UnsafeAlias)?
                }
                Err(_) => return Err(FilesystemBoundaryError::UnsafeAlias),
            };
        let opened = fs::File::from(descriptor);
        inspect_opened(&opened, StoreObjectKind::Directory).map_err(map_identity)?;
        current = opened;
    }
    Ok(())
}

fn remove_generated_tree(root: &Path) -> Result<(), FilesystemBoundaryError> {
    let root_identity = inspect_path(root, StoreObjectKind::Directory).map_err(map_identity)?;
    for entry in fs::read_dir(root).map_err(|_| FilesystemBoundaryError::Io)? {
        let path = entry.map_err(|_| FilesystemBoundaryError::Io)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| FilesystemBoundaryError::Io)?;
        if metadata.file_type().is_symlink() || is_reparse(&metadata) {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
        if metadata.is_dir() {
            inspect_path(&path, StoreObjectKind::Directory).map_err(map_identity)?;
            remove_generated_tree(&path)?;
        } else if metadata.is_file() {
            inspect_path(&path, StoreObjectKind::RegularFile).map_err(map_identity)?;
            fs::remove_file(&path).map_err(|_| FilesystemBoundaryError::Io)?;
        } else {
            return Err(FilesystemBoundaryError::UnsafeAlias);
        }
    }
    if inspect_path(root, StoreObjectKind::Directory).map_err(map_identity)? != root_identity {
        return Err(FilesystemBoundaryError::ObservationChanged);
    }
    fs::remove_dir(root).map_err(|_| FilesystemBoundaryError::Io)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), FilesystemBoundaryError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| FilesystemBoundaryError::Io)
}

#[cfg(windows)]
fn sync_directory(path: &Path) -> Result<(), FilesystemBoundaryError> {
    use std::os::windows::fs::OpenOptionsExt as _;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    options
        .open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| FilesystemBoundaryError::Io)
}

fn identity_key(path: &str) -> String {
    #[cfg(windows)]
    {
        path.replace('\\', "/").to_lowercase()
    }
    #[cfg(unix)]
    {
        path.to_owned()
    }
}

fn valid_workspace_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_relative_components(relative: &Path) -> Result<(), FilesystemBoundaryError> {
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        return Err(FilesystemBoundaryError::UnsafeAlias);
    }
    let mut saw_component = false;
    for component in relative.components() {
        match component {
            Component::Normal(name) if !name.is_empty() => saw_component = true,
            _ => return Err(FilesystemBoundaryError::UnsafeAlias),
        }
    }
    if saw_component {
        Ok(())
    } else {
        Err(FilesystemBoundaryError::UnsafeAlias)
    }
}
fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn map_secure(_: deltamod_tools_runtime::SecurePathError) -> FilesystemBoundaryError {
    FilesystemBoundaryError::UnsafeAlias
}
fn map_identity(error: crate::store_identity::IdentityError) -> FilesystemBoundaryError {
    match error {
        crate::store_identity::IdentityError::Io(_) => FilesystemBoundaryError::Io,
        _ => FilesystemBoundaryError::UnsafeAlias,
    }
}
#[cfg(unix)]
fn map_root_identity(error: IdentityError) -> FilesystemBoundaryError {
    match error {
        IdentityError::Replaced => FilesystemBoundaryError::RootIdentityChanged,
        IdentityError::Io(error) if error.kind() == io::ErrorKind::NotFound => {
            FilesystemBoundaryError::RootIdentityChanged
        }
        IdentityError::Io(_) => FilesystemBoundaryError::Io,
        _ => FilesystemBoundaryError::UnsafeAlias,
    }
}
fn map_fence(error: MutationFenceError) -> FilesystemBoundaryError {
    match error {
        MutationFenceError::StaleJournalSequence | MutationFenceError::InvalidJournal => {
            FilesystemBoundaryError::StaleJournal
        }
        _ => FilesystemBoundaryError::LostLease,
    }
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(unix)]
fn is_reparse(_metadata: &fs::Metadata) -> bool {
    false
}
