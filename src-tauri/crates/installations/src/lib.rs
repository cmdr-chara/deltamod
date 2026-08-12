#![forbid(unsafe_code)]
//! Pure installation-domain contracts. Filesystem, dialogs, and processes belong in adapters.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LegacyIpcRequest {
    ListInstallations,
    SelectInstallation { id: Option<String> },
    SetInstallationName { id: String, name: String },
    SetInstallationEdition { id: String, edition: String },
    ImportInstallation { path: String, linked: bool },
    ReimportInstallation { id: String },
    RepairInstallation { id: String },
    RemoveInstallation { id: String, delete_files: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    List,
    Select {
        id: Option<InstallationId>,
    },
    Name {
        id: InstallationId,
        name: String,
    },
    Edition {
        id: InstallationId,
        edition: Edition,
    },
    Import {
        path: PathBuf,
        ownership: Ownership,
    },
    Reimport {
        id: InstallationId,
    },
    Repair {
        id: InstallationId,
    },
    Remove {
        id: InstallationId,
        delete_files: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]
#[serde(transparent)]
pub struct InstallationId(pub String);

impl InstallationId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let v = value.into();
        if v.is_empty()
            || v.len() > 128
            || !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(DomainError::InvalidId);
        }
        Ok(Self(v))
    }
}

/// Allocates IDs from canonical adapter input without depending on process or map order.
pub fn allocate_installation_id(
    source: &Path,
    platform: GamePlatform,
    occupied: &std::collections::BTreeSet<InstallationId>,
) -> InstallationId {
    let input = format!("{}\0{:?}", source.to_string_lossy(), platform);
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let base = format!("install-{hash:016x}");
    if let Ok(id) = InstallationId::new(&base) {
        if !occupied.contains(&id) {
            return id;
        }
    }
    for suffix in 2.. {
        let candidate =
            InstallationId::new(format!("{base}-{suffix}")).expect("generated ID is valid");
        if !occupied.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("finite ID space exhausted")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Original,
    Expanded,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ownership {
    ManagedCopy,
    LinkedExternal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GamePlatform {
    Windows,
    Linux,
    Macos,
    Wine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    Linux,
    Macos,
}

impl GamePlatform {
    pub fn resolve(host: HostOs, wine_prefix: bool) -> Self {
        match (host, wine_prefix) {
            (HostOs::Windows, _) => Self::Windows,
            (_, true) => Self::Wine,
            (HostOs::Linux, false) => Self::Linux,
            (HostOs::Macos, false) => Self::Macos,
        }
    }
    pub fn executable(self) -> &'static str {
        match self {
            Self::Windows | Self::Wine => "DELTARUNE.exe",
            Self::Linux => "DELTARUNE",
            Self::Macos => "DELTARUNE.app",
        }
    }
    pub fn patch_targets(self) -> &'static [&'static str] {
        match self {
            Self::Windows | Self::Wine => &["data.win"],
            Self::Linux => &["data.win"],
            Self::Macos => &["Contents/Resources/data.win"],
        }
    }
    pub fn patch_target(self, relative: &str) -> Result<PathBuf, DomainError> {
        let path = strict_relative(relative)?;
        if self.patch_targets().contains(&relative) {
            Ok(path)
        } else {
            Err(DomainError::UnsupportedPatchTarget)
        }
    }
}

fn strict_relative(value: &str) -> Result<PathBuf, DomainError> {
    if value.is_empty()
        || value.contains('\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(value).is_absolute()
    {
        return Err(DomainError::InvalidRelativePath);
    }
    Ok(value.split('/').collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub id: InstallationId,
    pub name: String,
    pub edition: Edition,
    pub platform: GamePlatform,
    pub source: PathBuf,
    pub install_path: PathBuf,
    pub ownership: Ownership,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationListResponse {
    pub installations: Vec<Installation>,
    pub selected_id: Option<InstallationId>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationResponse {
    pub installation: Installation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResponse {
    pub operation_id: u64,
    pub accepted: bool,
}

pub fn convert_request(request: LegacyIpcRequest) -> Result<Request, DomainError> {
    Ok(match request {
        LegacyIpcRequest::ListInstallations => Request::List,
        LegacyIpcRequest::SelectInstallation { id } => Request::Select {
            id: id.map(InstallationId::new).transpose()?,
        },
        LegacyIpcRequest::SetInstallationName { id, name } => Request::Name {
            id: InstallationId::new(id)?,
            name: validate_name(name)?,
        },
        LegacyIpcRequest::SetInstallationEdition { id, edition } => Request::Edition {
            id: InstallationId::new(id)?,
            edition: parse_edition(&edition)?,
        },
        LegacyIpcRequest::ImportInstallation { path, linked } => Request::Import {
            path: validate_source(PathBuf::from(path))?,
            ownership: if linked {
                Ownership::LinkedExternal
            } else {
                Ownership::ManagedCopy
            },
        },
        LegacyIpcRequest::ReimportInstallation { id } => Request::Reimport {
            id: InstallationId::new(id)?,
        },
        LegacyIpcRequest::RepairInstallation { id } => Request::Repair {
            id: InstallationId::new(id)?,
        },
        LegacyIpcRequest::RemoveInstallation { id, delete_files } => Request::Remove {
            id: InstallationId::new(id)?,
            delete_files,
        },
    })
}
fn validate_name(name: String) -> Result<String, DomainError> {
    let n = name.trim().to_string();
    if n.is_empty() || n.len() > 100 {
        Err(DomainError::InvalidName)
    } else {
        Ok(n)
    }
}
fn parse_edition(value: &str) -> Result<Edition, DomainError> {
    if value.trim().is_empty() {
        Err(DomainError::InvalidEdition)
    } else {
        Ok(match value {
            "original" => Edition::Original,
            "expanded" => Edition::Expanded,
            other => Edition::Other(other.to_string()),
        })
    }
}
fn validate_source(path: PathBuf) -> Result<PathBuf, DomainError> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        Err(DomainError::InvalidSource)
    } else {
        Ok(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    DeleteManaged(PathBuf),
    PreserveLinked(PathBuf),
    RestorePatch(PathBuf),
    ReimportFrom(PathBuf),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPlan {
    pub actions: Vec<PlanAction>,
}
pub fn plan_delete(i: &Installation, delete_files: bool) -> OperationPlan {
    OperationPlan {
        actions: if delete_files && i.ownership == Ownership::ManagedCopy {
            vec![PlanAction::DeleteManaged(i.install_path.clone())]
        } else {
            vec![PlanAction::PreserveLinked(i.source.clone())]
        },
    }
}
pub fn plan_repair(i: &Installation) -> OperationPlan {
    OperationPlan {
        actions: vec![PlanAction::RestorePatch(i.install_path.clone())],
    }
}
pub fn plan_reimport(i: &Installation) -> OperationPlan {
    OperationPlan {
        actions: vec![PlanAction::ReimportFrom(i.source.clone())],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationState {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Committing,
    Completed,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: u64,
    pub state: OperationState,
}
#[derive(Debug, Default)]
pub struct OperationRegistry {
    next_id: u64,
    operations: BTreeMap<u64, OperationState>,
}
impl OperationRegistry {
    pub fn start(&mut self) -> Operation {
        self.next_id += 1;
        self.operations
            .insert(self.next_id, OperationState::Running);
        Operation {
            id: self.next_id,
            state: OperationState::Running,
        }
    }
    pub fn state(&self, id: u64) -> Option<OperationState> {
        self.operations.get(&id).copied()
    }
    pub fn cancel(&mut self, id: u64) -> Result<(), DomainError> {
        match self.operations.get_mut(&id) {
            Some(state @ OperationState::Running) => {
                *state = OperationState::Cancelling;
                Ok(())
            }
            Some(OperationState::Queued) => {
                *self.operations.get_mut(&id).unwrap() = OperationState::Cancelled;
                Ok(())
            }
            Some(
                OperationState::Cancelling
                | OperationState::Committing
                | OperationState::Completed
                | OperationState::Cancelled
                | OperationState::Failed,
            ) => Err(DomainError::InvalidOperationState),
            None => Err(DomainError::UnknownOperation),
        }
    }
    pub fn checkpoint(&self, id: u64) -> Result<(), DomainError> {
        match self.state(id) {
            Some(OperationState::Cancelling | OperationState::Cancelled) => {
                Err(DomainError::Cancelled)
            }
            Some(OperationState::Running) => Ok(()),
            _ => Err(DomainError::InvalidOperationState),
        }
    }
    pub fn commit(&mut self, id: u64) -> Result<(), DomainError> {
        match self.operations.get_mut(&id) {
            Some(state @ OperationState::Running) => {
                *state = OperationState::Committing;
                Ok(())
            }
            Some(OperationState::Cancelling) => {
                *self.operations.get_mut(&id).unwrap() = OperationState::Committing;
                Ok(())
            }
            _ => Err(DomainError::InvalidOperationState),
        }
    }
    pub fn finish(&mut self, id: u64, success: bool) -> Result<(), DomainError> {
        match self.operations.get_mut(&id) {
            Some(state @ OperationState::Committing) => {
                *state = if success {
                    OperationState::Completed
                } else {
                    OperationState::Failed
                };
                Ok(())
            }
            _ => Err(DomainError::InvalidOperationState),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ProgressEvent {
    Started {
        operation_id: u64,
        operation: String,
    },
    Phase {
        operation_id: u64,
        phase: String,
        completed: u64,
        total: Option<u64>,
    },
    Warning {
        operation_id: u64,
        message: String,
    },
    Finished {
        operation_id: u64,
        success: bool,
        message: Option<String>,
    },
}

pub trait DialogAdapter {
    fn choose_directory(&self) -> Result<Option<PathBuf>, AdapterError>;
}
pub trait ProcessAdapter {
    fn launch(&self, executable: &Path, args: &[String]) -> Result<(), AdapterError>;
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError(pub String);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    InvalidId,
    InvalidName,
    InvalidEdition,
    InvalidSource,
    InvalidRelativePath,
    UnsupportedPatchTarget,
    UnknownOperation,
    InvalidOperationState,
    Cancelled,
}
impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(ownership: Ownership) -> Installation {
        Installation {
            id: InstallationId::new("a1").unwrap(),
            name: "Game".into(),
            edition: Edition::Original,
            platform: GamePlatform::Windows,
            source: PathBuf::from("C:/external/game"),
            install_path: PathBuf::from("C:/managed/game"),
            ownership,
        }
    }
    #[test]
    fn legacy_conversion_is_typed() {
        assert_eq!(
            convert_request(LegacyIpcRequest::SetInstallationEdition {
                id: "x".into(),
                edition: "expanded".into()
            })
            .unwrap(),
            Request::Edition {
                id: InstallationId::new("x").unwrap(),
                edition: Edition::Expanded
            }
        );
        assert!(convert_request(LegacyIpcRequest::ImportInstallation {
            path: "relative".into(),
            linked: false
        })
        .is_err());
    }
    #[test]
    fn installation_ids_are_stable_and_collision_safe() {
        let source = Path::new("C:/games/deltarune");
        let empty = std::collections::BTreeSet::new();
        let first = allocate_installation_id(source, GamePlatform::Windows, &empty);
        assert_eq!(
            first,
            allocate_installation_id(source, GamePlatform::Windows, &empty)
        );
        let mut occupied = std::collections::BTreeSet::new();
        occupied.insert(first.clone());
        assert_ne!(
            first,
            allocate_installation_id(source, GamePlatform::Windows, &occupied)
        );
        assert_ne!(
            first,
            allocate_installation_id(source, GamePlatform::Wine, &empty)
        );
    }
    #[test]
    fn platform_parity_and_paths() {
        assert_eq!(
            GamePlatform::resolve(HostOs::Windows, false),
            GamePlatform::Windows
        );
        assert_eq!(
            GamePlatform::resolve(HostOs::Linux, true),
            GamePlatform::Wine
        );
        assert_eq!(
            GamePlatform::resolve(HostOs::Macos, false),
            GamePlatform::Macos
        );
        for p in [
            GamePlatform::Windows,
            GamePlatform::Linux,
            GamePlatform::Wine,
        ] {
            assert_eq!(
                p.patch_target("data.win").unwrap(),
                PathBuf::from("data.win")
            );
        }
        assert_eq!(
            GamePlatform::Macos
                .patch_target("Contents/Resources/data.win")
                .unwrap(),
            PathBuf::from("Contents/Resources/data.win")
        );
    }
    #[test]
    fn paths_are_strict() {
        for p in ["../data.win", "/data.win", "a//b", "a\\b", "./data.win", ""] {
            assert!(strict_relative(p).is_err(), "{p}");
        }
    }
    #[test]
    fn plans_never_delete_external_sources() {
        assert!(matches!(
            plan_delete(&sample(Ownership::LinkedExternal), true).actions[0],
            PlanAction::PreserveLinked(_)
        ));
        assert!(matches!(
            plan_delete(&sample(Ownership::ManagedCopy), true).actions[0],
            PlanAction::DeleteManaged(_)
        ));
        assert!(matches!(
            plan_reimport(&sample(Ownership::LinkedExternal)).actions[0],
            PlanAction::ReimportFrom(_)
        ));
    }
    #[test]
    fn cancellation_cannot_cross_commit_boundary_wrongly() {
        let mut r = OperationRegistry::default();
        let op = r.start();
        r.cancel(op.id).unwrap();
        assert_eq!(r.checkpoint(op.id), Err(DomainError::Cancelled));
        r.commit(op.id).unwrap();
        r.finish(op.id, true).unwrap();
        assert_eq!(r.state(op.id), Some(OperationState::Completed));
    }
    #[test]
    fn progress_is_renderer_serializable() {
        let event = ProgressEvent::Phase {
            operation_id: 2,
            phase: "copy".into(),
            completed: 1,
            total: Some(2),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("phase"));
    }
}
