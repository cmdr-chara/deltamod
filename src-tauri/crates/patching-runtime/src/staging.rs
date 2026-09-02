use super::{
    checked_relative, progress_event, validate_operation_id, Error, PatchPlan, Progress, Runtime,
};
use deltamod_native_core::patch_plan::{
    validate_patch_plan, PatchPlanRequest, PatchPlatform, PatchType,
};
use deltamod_tools_runtime::{
    copy_relative_regular_file_verified, inspect_directory_identity, inspect_regular_file,
    SecurePathError, StablePathIdentity, VerifiedFile,
};
use serde::Serialize;
use std::{
    collections::HashSet,
    fmt, fs, io,
    path::{Component, Path, PathBuf},
};

pub const MAX_STAGED_ARTIFACT_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_STAGED_TOTAL_BYTES: u64 = 32 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StagingErrorCode {
    InvalidRequest,
    Cancelled,
    SandboxUnavailable,
    ToolUnavailable,
    ToolTimeout,
    ToolFailed,
    InputChanged,
    InvalidOutput,
    OutputTooLarge,
    TargetChanged,
    WorkspaceChanged,
    Io,
}

impl StagingErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "PATCH_STAGING_INVALID_REQUEST",
            Self::Cancelled => "PATCH_STAGING_CANCELLED",
            Self::SandboxUnavailable => "PATCH_STAGING_SANDBOX_UNAVAILABLE",
            Self::ToolUnavailable => "PATCH_STAGING_TOOL_UNAVAILABLE",
            Self::ToolTimeout => "PATCH_STAGING_TOOL_TIMEOUT",
            Self::ToolFailed => "PATCH_STAGING_TOOL_FAILED",
            Self::InputChanged => "PATCH_STAGING_INPUT_CHANGED",
            Self::InvalidOutput => "PATCH_STAGING_INVALID_OUTPUT",
            Self::OutputTooLarge => "PATCH_STAGING_OUTPUT_TOO_LARGE",
            Self::TargetChanged => "PATCH_STAGING_TARGET_CHANGED",
            Self::WorkspaceChanged => "PATCH_STAGING_WORKSPACE_CHANGED",
            Self::Io => "PATCH_STAGING_IO",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PatchMechanism {
    Internal,
    G3m,
    Csx,
}

impl PatchMechanism {
    const fn label(self) -> &'static str {
        match self {
            Self::Internal => "internal patcher",
            Self::G3m => "G3MTool",
            Self::Csx => "UndertaleModCli",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingDiagnostic {
    mechanism: PatchMechanism,
    classification: String,
}

impl StagingDiagnostic {
    fn stable(mechanism: PatchMechanism, code: StagingErrorCode) -> Self {
        Self {
            mechanism,
            classification: code.as_str().to_owned(),
        }
    }

    pub const fn mechanism(&self) -> PatchMechanism {
        self.mechanism
    }

    /// Stable classification only; arbitrary tool output and paths are never retained.
    pub fn detail(&self) -> &str {
        &self.classification
    }

    pub const fn truncated(&self) -> bool {
        false
    }
}

#[derive(Debug)]
pub struct StagingError {
    code: StagingErrorCode,
    mechanism: Option<PatchMechanism>,
    diagnostic: Option<StagingDiagnostic>,
}

impl StagingError {
    fn new(code: StagingErrorCode, mechanism: Option<PatchMechanism>) -> Self {
        Self {
            code,
            mechanism,
            diagnostic: mechanism.map(|value| StagingDiagnostic::stable(value, code)),
        }
    }

    fn from_runtime(error: Error) -> Self {
        let code = match error {
            Error::Cancelled => StagingErrorCode::Cancelled,
            Error::Io(_) => StagingErrorCode::Io,
            _ => StagingErrorCode::InvalidRequest,
        };
        Self::new(code, Some(PatchMechanism::Internal))
    }

    fn from_secure(error: SecurePathError, fallback: StagingErrorCode) -> Self {
        let code = match error {
            SecurePathError::TooLarge => StagingErrorCode::OutputTooLarge,
            SecurePathError::Unsupported => StagingErrorCode::SandboxUnavailable,
            SecurePathError::Unsafe | SecurePathError::Changed => fallback,
            SecurePathError::Io(_) => StagingErrorCode::Io,
        };
        Self::new(code, Some(PatchMechanism::Internal))
    }

    fn io(_: io::Error) -> Self {
        Self::new(StagingErrorCode::Io, Some(PatchMechanism::Internal))
    }

    pub const fn code(&self) -> StagingErrorCode {
        self.code
    }

    pub const fn mechanism(&self) -> Option<PatchMechanism> {
        self.mechanism
    }

    pub fn diagnostic(&self) -> Option<&StagingDiagnostic> {
        self.diagnostic.as_ref()
    }
}

impl fmt::Display for StagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mechanism = self.mechanism.unwrap_or(PatchMechanism::Internal).label();
        match self.code {
            StagingErrorCode::InvalidRequest => {
                formatter.write_str("The patch staging request is invalid.")
            }
            StagingErrorCode::Cancelled => formatter.write_str("Patch staging was cancelled."),
            StagingErrorCode::SandboxUnavailable => {
                write!(
                    formatter,
                    "A confinement boundary for {mechanism} is unavailable."
                )
            }
            StagingErrorCode::ToolUnavailable => {
                write!(formatter, "The {mechanism} is unavailable.")
            }
            StagingErrorCode::ToolTimeout => write!(formatter, "The {mechanism} timed out."),
            StagingErrorCode::ToolFailed => write!(formatter, "The {mechanism} failed."),
            StagingErrorCode::InputChanged => {
                formatter.write_str("A patch input changed during staging.")
            }
            StagingErrorCode::InvalidOutput => {
                formatter.write_str("The staged patch output is invalid.")
            }
            StagingErrorCode::OutputTooLarge => {
                formatter.write_str("A staged patch output exceeds its size limit.")
            }
            StagingErrorCode::TargetChanged => {
                formatter.write_str("A game target changed after staging.")
            }
            StagingErrorCode::WorkspaceChanged => {
                formatter.write_str("The owned staging workspace changed identity.")
            }
            StagingErrorCode::Io => {
                formatter.write_str("The patch staging workspace is unavailable.")
            }
        }
    }
}

impl std::error::Error for StagingError {}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchTargetIdentity {
    relative_path: String,
}

impl PatchTargetIdentity {
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryPrecondition {
    path: PathBuf,
    identity: StablePathIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExpectedTarget {
    Absent,
    Present(VerifiedFile),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TargetPrecondition {
    path: PathBuf,
    ancestors: Vec<DirectoryPrecondition>,
    first_missing_ancestor: Option<PathBuf>,
    expected: ExpectedTarget,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedArtifact {
    target: PatchTargetIdentity,
    path: PathBuf,
    sha256: String,
    size: u64,
    expected_target_sha256: Option<String>,
    #[serde(skip)]
    output_directory: PathBuf,
    #[serde(skip)]
    output_identity: StablePathIdentity,
    #[serde(skip)]
    target_precondition: TargetPrecondition,
}

impl StagedArtifact {
    pub const fn target(&self) -> &PatchTargetIdentity {
        &self.target
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub const fn size(&self) -> u64 {
        self.size
    }

    /// `None` means the target was absent when the plan was staged.
    pub fn expected_target_sha256(&self) -> Option<&str> {
        self.expected_target_sha256.as_deref()
    }
}

#[derive(Debug)]
struct OwnedWorkspace {
    path: PathBuf,
    identity: StablePathIdentity,
}

impl OwnedWorkspace {
    fn create() -> Result<Self, StagingError> {
        let temporary = tempfile::Builder::new()
            .prefix("deltamod-patch-stage-")
            .tempdir()
            .map_err(StagingError::io)?;
        Self::from_temporary(temporary)
    }

    #[cfg(test)]
    fn create_in(parent: &Path) -> Result<Self, StagingError> {
        let temporary = tempfile::Builder::new()
            .prefix("deltamod-patch-stage-")
            .tempdir_in(parent)
            .map_err(StagingError::io)?;
        Self::from_temporary(temporary)
    }

    fn from_temporary(temporary: tempfile::TempDir) -> Result<Self, StagingError> {
        let path = temporary.keep();
        let identity = inspect_directory_identity(&path).map_err(|error| {
            StagingError::from_secure(error, StagingErrorCode::WorkspaceChanged)
        })?;
        Ok(Self { path, identity })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn verify(&self) -> Result<(), StagingError> {
        let current = inspect_directory_identity(&self.path).map_err(|error| {
            StagingError::from_secure(error, StagingErrorCode::WorkspaceChanged)
        })?;
        if current != self.identity {
            return Err(StagingError::new(
                StagingErrorCode::WorkspaceChanged,
                Some(PatchMechanism::Internal),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct StagedPatchSet {
    workspace: OwnedWorkspace,
    game_root: PathBuf,
    game_root_identity: StablePathIdentity,
    platform: PatchPlatform,
    artifacts: Vec<StagedArtifact>,
    diagnostics: Vec<StagingDiagnostic>,
}

impl StagedPatchSet {
    pub fn workspace_path(&self) -> &Path {
        self.workspace.path()
    }

    pub fn artifacts(&self) -> &[StagedArtifact] {
        &self.artifacts
    }

    pub fn diagnostics(&self) -> &[StagingDiagnostic] {
        &self.diagnostics
    }

    /// Revalidates workspace identity, output handles, and every game target
    /// precondition immediately before lifecycle handoff.
    pub fn verify(&self) -> Result<(), StagingError> {
        self.verify_workspace()?;
        for artifact in &self.artifacts {
            verify_target_precondition(
                &self.game_root,
                &artifact.target,
                &artifact.target_precondition,
            )?;
        }
        Ok(())
    }

    fn verify_workspace(&self) -> Result<(), StagingError> {
        self.workspace.verify()?;
        let current_root = inspect_directory_identity(&self.game_root)
            .map_err(|error| StagingError::from_secure(error, StagingErrorCode::TargetChanged))?;
        if current_root != self.game_root_identity {
            return Err(StagingError::new(
                StagingErrorCode::TargetChanged,
                Some(PatchMechanism::Internal),
            ));
        }

        let mut targets = HashSet::new();
        let mut total = 0_u64;
        for artifact in &self.artifacts {
            if !targets.insert(target_key(&artifact.target.relative_path, self.platform)) {
                return Err(StagingError::new(
                    StagingErrorCode::InvalidOutput,
                    Some(PatchMechanism::Internal),
                ));
            }
            verify_workspace_path(
                self.workspace.path(),
                &artifact.output_directory,
                &artifact.path,
            )?;
            let output = inspect_regular_file(&artifact.path, MAX_STAGED_ARTIFACT_BYTES).map_err(
                |error| StagingError::from_secure(error, StagingErrorCode::InvalidOutput),
            )?;
            if output.identity() != &artifact.output_identity
                || output.size() != artifact.size
                || output.sha256() != artifact.sha256
            {
                return Err(StagingError::new(
                    StagingErrorCode::InvalidOutput,
                    Some(PatchMechanism::Internal),
                ));
            }
            total = total
                .checked_add(output.size())
                .ok_or_else(|| StagingError::new(StagingErrorCode::OutputTooLarge, None))?;
        }
        if total > MAX_STAGED_TOTAL_BYTES {
            return Err(StagingError::new(StagingErrorCode::OutputTooLarge, None));
        }
        Ok(())
    }

    /// Removes only the verified files and empty directories owned by this
    /// staging set. Unexpected entries make cleanup fail closed; recursive
    /// pathname deletion is deliberately avoided.
    pub fn discard_verified(self) -> Result<(), StagingError> {
        self.verify_workspace()?;
        let workspace = self.workspace.path().to_owned();
        let mut operation_directories = Vec::with_capacity(self.artifacts.len());
        for artifact in &self.artifacts {
            fs::remove_file(&artifact.path).map_err(StagingError::io)?;
            fs::remove_dir(&artifact.output_directory).map_err(StagingError::io)?;
            let operation = artifact
                .output_directory
                .parent()
                .ok_or_else(|| {
                    StagingError::new(
                        StagingErrorCode::WorkspaceChanged,
                        Some(PatchMechanism::Internal),
                    )
                })?
                .to_owned();
            if !operation_directories.contains(&operation) {
                operation_directories.push(operation);
            }
        }
        for operation in operation_directories {
            fs::remove_dir(operation).map_err(StagingError::io)?;
        }
        fs::remove_dir(workspace).map_err(StagingError::io)
    }
}

pub(super) fn stage_patch_outputs(
    runtime: &Runtime,
    selected: &[String],
    operation_id: &str,
    mut emit: impl FnMut(Progress),
    cancelled: impl Fn() -> bool,
) -> Result<StagedPatchSet, StagingError> {
    validate_operation_id(operation_id).map_err(StagingError::from_runtime)?;
    check_cancel(&cancelled)?;
    if let Some(mechanism) = runtime
        .unsupported_staging_mechanism(selected)
        .map_err(StagingError::from_runtime)?
    {
        return Err(StagingError::new(
            StagingErrorCode::SandboxUnavailable,
            Some(mechanism),
        ));
    }
    let plan = runtime
        .build_staging_plan(selected)
        .map_err(StagingError::from_runtime)?;
    if let Some(mechanism) = unsupported_mechanism(&plan) {
        return Err(StagingError::new(
            StagingErrorCode::SandboxUnavailable,
            Some(mechanism),
        ));
    }
    check_cancel(&cancelled)?;
    revalidate_plan(runtime, &plan)?;

    let game_root_identity = inspect_directory_identity(&plan.game_root)
        .map_err(|error| StagingError::from_secure(error, StagingErrorCode::TargetChanged))?;
    let workspace = OwnedWorkspace::create()?;
    let mut artifacts = Vec::with_capacity(plan.operation_count);
    let mut completed = 0_usize;

    for patch in &plan.patches {
        check_cancel(&cancelled)?;
        let (output_directory, output) = operation_paths(workspace.path(), completed)?;
        let patch_relative = checked_relative(&patch.candidate.patch).map_err(|_| {
            StagingError::new(
                StagingErrorCode::InvalidRequest,
                Some(PatchMechanism::Internal),
            )
        })?;
        if patch.candidate.mod_root.join(&patch_relative) != patch.source {
            return Err(StagingError::new(
                StagingErrorCode::InputChanged,
                Some(PatchMechanism::Internal),
            ));
        }
        let copied = copy_relative_regular_file_verified(
            &patch.candidate.mod_root,
            &patch_relative,
            &output,
            MAX_STAGED_ARTIFACT_BYTES,
        )
        .map_err(|error| StagingError::from_secure(error, StagingErrorCode::InputChanged))?;
        if copied.sha256() != patch.source_sha256 {
            return Err(StagingError::new(
                StagingErrorCode::InputChanged,
                Some(PatchMechanism::Internal),
            ));
        }
        let target_precondition =
            capture_target_precondition(&plan.game_root, &patch.candidate.mapped_target)?;
        let expected_target_sha256 = match &target_precondition.expected {
            ExpectedTarget::Absent => None,
            ExpectedTarget::Present(file) => Some(file.sha256().to_owned()),
        };
        artifacts.push(StagedArtifact {
            target: PatchTargetIdentity {
                relative_path: patch.candidate.mapped_target.replace('\\', "/"),
            },
            path: output,
            sha256: copied.sha256().to_owned(),
            size: copied.size(),
            expected_target_sha256,
            output_directory,
            output_identity: copied.identity().clone(),
            target_precondition,
        });
        completed += 1;
        emit(progress_event(
            operation_id,
            "staging",
            completed,
            plan.operation_count,
            Some(patch.candidate.mapped_target.clone()),
            None,
        ));
    }

    artifacts.sort_by(|left, right| left.target.cmp(&right.target));
    let result = StagedPatchSet {
        workspace,
        game_root: plan.game_root,
        game_root_identity,
        platform: runtime.platform,
        artifacts,
        diagnostics: Vec::new(),
    };
    result.verify()?;
    Ok(result)
}

fn revalidate_plan(runtime: &Runtime, plan: &PatchPlan) -> Result<(), StagingError> {
    validate_patch_plan(&PatchPlanRequest {
        game_root: plan.game_root.clone(),
        platform: runtime.platform,
        patches: plan
            .patches
            .iter()
            .map(|patch| patch.candidate.clone())
            .collect(),
    })
    .map(|_| ())
    .map_err(|_| {
        StagingError::new(
            StagingErrorCode::InvalidRequest,
            Some(PatchMechanism::Internal),
        )
    })
}

fn unsupported_mechanism(plan: &PatchPlan) -> Option<PatchMechanism> {
    plan.patches
        .iter()
        .find_map(|patch| match patch.candidate.patch_type {
            PatchType::Override | PatchType::Copy => None,
            PatchType::Xdelta | PatchType::G3mPatch => Some(PatchMechanism::G3m),
            PatchType::Csx => Some(PatchMechanism::Csx),
        })
}

fn operation_paths(workspace: &Path, index: usize) -> Result<(PathBuf, PathBuf), StagingError> {
    let operation = workspace.join(format!("operation-{index:05}"));
    let output_directory = operation.join("output");
    fs::create_dir(&operation).map_err(StagingError::io)?;
    fs::create_dir(&output_directory).map_err(StagingError::io)?;
    inspect_directory_identity(&operation)
        .map_err(|error| StagingError::from_secure(error, StagingErrorCode::WorkspaceChanged))?;
    inspect_directory_identity(&output_directory)
        .map_err(|error| StagingError::from_secure(error, StagingErrorCode::WorkspaceChanged))?;
    let output = output_directory.join("artifact");
    Ok((output_directory, output))
}

fn capture_target_precondition(
    game_root: &Path,
    target: &str,
) -> Result<TargetPrecondition, StagingError> {
    let relative = checked_relative(target).map_err(|_| {
        StagingError::new(
            StagingErrorCode::InvalidRequest,
            Some(PatchMechanism::Internal),
        )
    })?;
    let path = game_root.join(&relative);
    let mut ancestors = Vec::new();
    let mut current = game_root.to_path_buf();
    let parent = relative.parent().unwrap_or(Path::new(""));
    let mut first_missing_ancestor = None;
    for component in parent.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(StagingError::new(
                StagingErrorCode::InvalidRequest,
                Some(PatchMechanism::Internal),
            ));
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(_) => {
                let identity = inspect_directory_identity(&current).map_err(|error| {
                    StagingError::from_secure(error, StagingErrorCode::TargetChanged)
                })?;
                ancestors.push(DirectoryPrecondition {
                    path: current.clone(),
                    identity,
                });
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                first_missing_ancestor = Some(current.clone());
                break;
            }
            Err(error) => return Err(StagingError::io(error)),
        }
    }
    let expected = if first_missing_ancestor.is_some() {
        ExpectedTarget::Absent
    } else {
        match fs::symlink_metadata(&path) {
            Ok(_) => ExpectedTarget::Present(
                inspect_regular_file(&path, MAX_STAGED_ARTIFACT_BYTES).map_err(|error| {
                    StagingError::from_secure(error, StagingErrorCode::TargetChanged)
                })?,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => ExpectedTarget::Absent,
            Err(error) => return Err(StagingError::io(error)),
        }
    };
    Ok(TargetPrecondition {
        path,
        ancestors,
        first_missing_ancestor,
        expected,
    })
}

fn verify_target_precondition(
    game_root: &Path,
    target: &PatchTargetIdentity,
    precondition: &TargetPrecondition,
) -> Result<(), StagingError> {
    let relative = checked_relative(&target.relative_path).map_err(|_| target_changed())?;
    if game_root.join(relative) != precondition.path
        || !is_confined_descendant(game_root, &precondition.path)
        || precondition
            .ancestors
            .iter()
            .any(|ancestor| !is_confined_descendant(game_root, &ancestor.path))
        || precondition
            .first_missing_ancestor
            .as_deref()
            .is_some_and(|missing| !is_confined_descendant(game_root, missing))
    {
        return Err(target_changed());
    }
    for ancestor in &precondition.ancestors {
        let current = inspect_directory_identity(&ancestor.path)
            .map_err(|error| StagingError::from_secure(error, StagingErrorCode::TargetChanged))?;
        if current != ancestor.identity {
            return Err(target_changed());
        }
    }
    if let Some(missing) = &precondition.first_missing_ancestor {
        return match fs::symlink_metadata(missing) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err(target_changed()),
        };
    }
    match &precondition.expected {
        ExpectedTarget::Absent => match fs::symlink_metadata(&precondition.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            _ => Err(target_changed()),
        },
        ExpectedTarget::Present(expected) => {
            let current = inspect_regular_file(&precondition.path, MAX_STAGED_ARTIFACT_BYTES)
                .map_err(|error| {
                    StagingError::from_secure(error, StagingErrorCode::TargetChanged)
                })?;
            if &current == expected {
                Ok(())
            } else {
                Err(target_changed())
            }
        }
    }
}

fn is_confined_descendant(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
    })
}

fn target_changed() -> StagingError {
    StagingError::new(
        StagingErrorCode::TargetChanged,
        Some(PatchMechanism::Internal),
    )
}

fn verify_workspace_path(
    workspace: &Path,
    output_directory: &Path,
    expected: &Path,
) -> Result<(), StagingError> {
    let relative = expected.strip_prefix(workspace).map_err(|_| {
        StagingError::new(
            StagingErrorCode::WorkspaceChanged,
            Some(PatchMechanism::Internal),
        )
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(StagingError::new(
            StagingErrorCode::WorkspaceChanged,
            Some(PatchMechanism::Internal),
        ));
    }
    let mut current = workspace.to_path_buf();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        current.push(component.as_os_str());
        inspect_directory_identity(&current).map_err(|error| {
            StagingError::from_secure(error, StagingErrorCode::WorkspaceChanged)
        })?;
    }
    if current != output_directory {
        return Err(StagingError::new(
            StagingErrorCode::WorkspaceChanged,
            Some(PatchMechanism::Internal),
        ));
    }
    let entries = fs::read_dir(output_directory)
        .map_err(StagingError::io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StagingError::io)?;
    if entries.len() != 1 || entries[0].path() != expected {
        return Err(StagingError::new(
            StagingErrorCode::InvalidOutput,
            Some(PatchMechanism::Internal),
        ));
    }
    Ok(())
}

fn target_key(target: &str, platform: PatchPlatform) -> String {
    let normalized = target.replace('\\', "/");
    if platform == PatchPlatform::Win32 {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn check_cancel(cancelled: &impl Fn() -> bool) -> Result<(), StagingError> {
    if cancelled() {
        Err(StagingError::new(StagingErrorCode::Cancelled, None))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::PlatformDefinition;
    use super::*;

    fn fixture(target: &str, patch_type: &str) -> (tempfile::TempDir, Runtime) {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("game");
        let mods = root.path().join("mods");
        let packet = mods.join("one");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&packet).unwrap();
        if let Some(parent) = Path::new(target).parent() {
            fs::create_dir_all(game.join(parent)).unwrap();
        }
        if patch_type != "override-absent" {
            fs::write(game.join(target), b"original").unwrap();
        }
        let (kind, source) = match patch_type {
            "csx" => ("csx", "script.csx"),
            "g3m" => ("g3mpatch", "patch.bin"),
            _ => ("override", "new.bin"),
        };
        fs::write(packet.join(source), b"patched").unwrap();
        fs::write(packet.join("__deltaID.json"), r#"{"uniqueId":"id"}"#).unwrap();
        fs::write(packet.join("meta.toml"), "[metadata]\nname='Test'\n").unwrap();
        fs::write(
            packet.join("modding.xml"),
            format!(r#"<root><patch type="{kind}" patch="{source}" to="{target}"/></root>"#),
        )
        .unwrap();
        let runtime = Runtime {
            game_root: game,
            mod_root: mods,
            tools_root: root.path().join("missing-tools"),
            hash_cache_path: root.path().join("hash.json"),
            platform: PatchPlatform::Linux,
            platform_name: "linux".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        (root, runtime)
    }

    #[test]
    fn internal_stage_does_not_mutate_game_and_is_deterministic() {
        let (_root, runtime) = fixture("data.win", "override");
        let first = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        assert_eq!(
            fs::read(runtime.game_root.join("data.win")).unwrap(),
            b"original"
        );
        assert!(!runtime.game_root.join(super::super::JOURNAL_NAME).exists());
        let second = runtime
            .stage_patch_outputs(&["id".into()], "stage-2", |_| {}, || false)
            .unwrap();
        assert_eq!(
            first.artifacts()[0].target(),
            second.artifacts()[0].target()
        );
        assert_eq!(
            first.artifacts()[0].sha256(),
            second.artifacts()[0].sha256()
        );
        assert!(first.verify().is_ok());
    }

    #[test]
    fn all_external_mechanisms_fail_closed_before_tool_lookup() {
        for (kind, source, mechanism) in [
            ("g3m", "patch.bin", PatchMechanism::G3m),
            ("csx", "script.csx", PatchMechanism::Csx),
        ] {
            let (_root, runtime) = fixture("data.win", kind);
            fs::remove_file(runtime.mod_root.join("one").join(source)).unwrap();
            let error = runtime
                .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
                .unwrap_err();
            assert_eq!(error.code(), StagingErrorCode::SandboxUnavailable);
            assert_eq!(error.mechanism(), Some(mechanism));
            assert_eq!(
                error.diagnostic().unwrap().detail(),
                "PATCH_STAGING_SANDBOX_UNAVAILABLE"
            );
            assert!(!runtime.game_root.join(super::super::JOURNAL_NAME).exists());
        }
    }

    #[test]
    fn verify_rejects_same_content_output_and_target_replacements() {
        let (_root, runtime) = fixture("data.win", "override");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        let output = staged.artifacts()[0].path().to_owned();
        fs::remove_file(&output).unwrap();
        fs::write(&output, b"patched").unwrap();
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::InvalidOutput
        );

        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-2", |_| {}, || false)
            .unwrap();
        let target = runtime.game_root.join("data.win");
        fs::remove_file(&target).unwrap();
        fs::write(&target, b"original").unwrap();
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::TargetChanged
        );
    }

    #[test]
    fn verify_rejects_created_absent_target_and_replaced_ancestor() {
        let (_root, runtime) = fixture("new/path.bin", "override-absent");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        fs::write(runtime.game_root.join("new/path.bin"), b"surprise").unwrap();
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::TargetChanged
        );

        let (_root, runtime) = fixture("nested/data.win", "override");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-2", |_| {}, || false)
            .unwrap();
        let nested = runtime.game_root.join("nested");
        let moved = runtime.game_root.join("nested-old");
        fs::rename(&nested, &moved).unwrap();
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("data.win"), b"original").unwrap();
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::TargetChanged
        );
    }

    #[test]
    fn verify_rejects_replaced_game_root_identity() {
        let (root, runtime) = fixture("data.win", "override");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        let moved = root.path().join("game-old");
        fs::rename(&runtime.game_root, &moved).unwrap();
        fs::create_dir(&runtime.game_root).unwrap();
        fs::write(runtime.game_root.join("data.win"), b"original").unwrap();
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::TargetChanged
        );
    }

    #[test]
    fn verify_rejects_target_precondition_outside_private_game_root() {
        let (root, runtime) = fixture("data.win", "override");
        let mut staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        staged.artifacts[0].target_precondition.path = root.path().join("outside.bin");
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::TargetChanged
        );
    }

    #[test]
    fn drop_retains_workspace_for_parent_owned_safe_cleanup() {
        let parent = tempfile::tempdir().unwrap();
        let workspace = OwnedWorkspace::create_in(parent.path()).unwrap();
        let original = workspace.path().to_owned();
        fs::write(original.join("owned"), b"retain").unwrap();
        drop(workspace);
        assert_eq!(fs::read(original.join("owned")).unwrap(), b"retain");

        let parent = tempfile::tempdir().unwrap();
        let workspace = OwnedWorkspace::create_in(parent.path()).unwrap();
        let original = workspace.path().to_owned();
        let moved = parent.path().join("moved-workspace");
        fs::rename(&original, &moved).unwrap();
        fs::create_dir(&original).unwrap();
        fs::write(original.join("sentinel"), b"keep").unwrap();
        drop(workspace);
        assert_eq!(fs::read(original.join("sentinel")).unwrap(), b"keep");
    }

    #[test]
    fn verified_discard_removes_only_the_owned_empty_workspace() {
        let (_root, runtime) = fixture("data.win", "override");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        let workspace = staged.workspace_path().to_owned();
        staged.discard_verified().unwrap();
        assert!(!workspace.exists());

        let (_root, runtime) = fixture("data.win", "override");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-2", |_| {}, || false)
            .unwrap();
        let workspace = staged.workspace_path().to_owned();
        fs::write(workspace.join("unexpected"), b"retain").unwrap();
        assert_eq!(
            staged.discard_verified().unwrap_err().code(),
            StagingErrorCode::Io
        );
        assert_eq!(fs::read(workspace.join("unexpected")).unwrap(), b"retain");
    }

    #[test]
    fn target_keys_follow_windows_case_semantics() {
        assert_eq!(
            target_key("Folder/Data.WIN", PatchPlatform::Win32),
            target_key("folder/data.win", PatchPlatform::Win32)
        );
        assert_ne!(
            target_key("Folder/Data.WIN", PatchPlatform::Linux),
            target_key("folder/data.win", PatchPlatform::Linux)
        );
    }

    #[test]
    fn staged_output_hardlinks_are_rejected() {
        let (_root, runtime) = fixture("data.win", "override");
        let staged = runtime
            .stage_patch_outputs(&["id".into()], "stage-1", |_| {}, || false)
            .unwrap();
        let artifact = staged.artifacts()[0].path();
        let alias = staged.workspace_path().join("alias");
        match fs::hard_link(artifact, &alias) {
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
        assert_eq!(
            staged.verify().unwrap_err().code(),
            StagingErrorCode::InvalidOutput
        );
    }

    #[test]
    fn public_diagnostics_are_stable_classifications_only() {
        let error = StagingError::new(
            StagingErrorCode::SandboxUnavailable,
            Some(PatchMechanism::Csx),
        );
        assert_eq!(
            error.diagnostic().unwrap().detail(),
            "PATCH_STAGING_SANDBOX_UNAVAILABLE"
        );
        assert!(!error.to_string().contains(['\\', '/']));
    }
}
