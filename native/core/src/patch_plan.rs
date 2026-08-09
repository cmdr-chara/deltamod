use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use thiserror::Error;

use crate::path_security::{is_within_with_case, validate_relative_path, CaseSensitivity};

pub const MAX_PATCHES: usize = 10_000;
pub const MAX_STRING_BYTES: usize = 32_768;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchPlatform {
    Win32,
    Linux,
    Darwin,
}

impl PatchPlatform {
    const fn case_sensitivity(self) -> CaseSensitivity {
        match self {
            Self::Win32 => CaseSensitivity::Insensitive,
            Self::Linux | Self::Darwin => CaseSensitivity::Sensitive,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchType {
    Override,
    Copy,
    Xdelta,
    G3mPatch,
    Csx,
}

impl PatchType {
    const fn is_direct(self) -> bool {
        matches!(self, Self::Override | Self::Copy)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchCandidate {
    pub patch_type: PatchType,
    pub patch: String,
    pub to: String,
    pub mapped_target: String,
    pub mod_name: String,
    pub mod_id: String,
    pub mod_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchPlanRequest {
    pub game_root: PathBuf,
    pub platform: PatchPlatform,
    pub patches: Vec<PatchCandidate>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatchPlanApproval {
    pub operation_count: usize,
    pub patch_count: usize,
    pub snapshot_count: usize,
}

#[derive(Debug, Error)]
pub enum PatchPlanError {
    #[error("{0}")]
    Invalid(String),
    #[error("Patch plan paths could not be read safely.")]
    Io(#[source] io::Error),
}

impl PatchPlanError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "PATCH_PLAN_INVALID",
            Self::Io(_) => "PATCH_PLAN_IO",
        }
    }
}

impl From<io::Error> for PatchPlanError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    path: PathBuf,
    identity: Identity,
    length: u64,
    modified: Option<SystemTime>,
    links: u64,
    regular_file: bool,
}

#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(windows)]
type Identity = fence_windows::FileIdentity;
#[cfg(not(any(unix, windows)))]
type Identity = ();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerKind {
    Direct,
    Merge,
    Csx,
}

pub fn validate_patch_plan(
    request: &PatchPlanRequest,
) -> Result<PatchPlanApproval, PatchPlanError> {
    if request.patches.len() > MAX_PATCHES {
        return invalid("Patch plan exceeds the supported patch count.");
    }
    validate_root_argument(&request.game_root, "Game root")?;

    let case = request.platform.case_sensitivity();
    let game_root = validate_root(&request.game_root, "Game root")?;
    let mut snapshots = Vec::new();
    snapshots.push(snapshot(&request.game_root)?);
    let mut snapshotted = HashSet::new();
    snapshotted.insert(request.game_root.clone());
    let mut owners = HashMap::<String, (OwnerKind, String)>::new();
    let mut operation_targets = HashSet::<String>::new();

    for candidate in &request.patches {
        validate_candidate_strings(candidate)?;
        validate_root_argument(&candidate.mod_root, "Mod root")?;
        let mod_root = validate_root(&candidate.mod_root, "Mod root")?;
        push_snapshot(&candidate.mod_root, &mut snapshots, &mut snapshotted)?;

        let source_relative = checked_relative(&candidate.patch, "Patch source")?;
        let source = candidate.mod_root.join(&source_relative);
        validate_existing_path(&candidate.mod_root, &source_relative)?;
        let canonical_source = fs::canonicalize(&source)?;
        if !is_within_with_case(&mod_root, &canonical_source, false, case) {
            return invalid("Patch source escapes its mod root.");
        }
        require_regular_file(&source, &format!("Patch source \"{}\"", candidate.patch))?;
        push_snapshot(&source, &mut snapshots, &mut snapshotted)?;

        checked_relative(&candidate.to, "Patch target")?;
        let target_relative = checked_relative(&candidate.mapped_target, "Mapped patch target")?;
        let target = request.game_root.join(&target_relative);
        validate_target_ancestry(&request.game_root, &target_relative)?;
        if let Ok(canonical_target) = fs::canonicalize(&target) {
            if !is_within_with_case(&game_root, &canonical_target, false, case) {
                return invalid("Patch target escapes the game root.");
            }
        }
        let target_exists = target.try_exists()?;
        if target_exists {
            require_regular_file(&target, &format!("Patch target \"{}\"", candidate.to))?;
            push_snapshot(&target, &mut snapshots, &mut snapshotted)?;
        }

        let target_key = comparison_key(&target, case);
        let prior = owners.get(&target_key).cloned();
        if candidate.patch_type == PatchType::Csx {
            validate_csx(candidate, &target, target_exists)?;
            if let Some((kind, _)) = prior {
                if kind != OwnerKind::Csx {
                    return invalid(format!(
                        "Patch conflict: \"{}\" has both CSX and non-CSX patches.",
                        candidate.to
                    ));
                }
            }
            owners
                .entry(target_key.clone())
                .or_insert((OwnerKind::Csx, candidate.mod_name.clone()));
            operation_targets.insert(target_key);
            continue;
        }

        if candidate.patch_type.is_direct() {
            if let Some((kind, owner)) = prior {
                if kind == OwnerKind::Direct {
                    return invalid(format!(
                        "Patch conflict: \"{}\" is modified by both \"{}\" and \"{}\".",
                        candidate.to, owner, candidate.mod_name
                    ));
                }
                return invalid(format!(
                    "Patch conflict: \"{}\" has both direct and non-direct patches.",
                    candidate.to
                ));
            }
            owners.insert(
                target_key.clone(),
                (OwnerKind::Direct, candidate.mod_name.clone()),
            );
            operation_targets.insert(target_key);
            continue;
        }

        if let Some((kind, _)) = prior {
            if kind == OwnerKind::Direct || kind == OwnerKind::Csx {
                return invalid(format!(
                    "Patch conflict: \"{}\" has both direct and merge patches.",
                    candidate.to
                ));
            }
        }
        if !target_exists {
            return invalid(format!(
                "Merge target \"{}\" required by \"{}\" does not exist.",
                candidate.to, candidate.mod_name
            ));
        }
        owners
            .entry(target_key.clone())
            .or_insert((OwnerKind::Merge, candidate.mod_name.clone()));
        operation_targets.insert(target_key);
    }

    for expected in &snapshots {
        verify_snapshot(expected)?;
    }

    Ok(PatchPlanApproval {
        operation_count: operation_targets.len(),
        patch_count: request.patches.len(),
        snapshot_count: snapshots.len(),
    })
}

fn validate_root_argument(path: &Path, description: &str) -> Result<(), PatchPlanError> {
    if !path.is_absolute() {
        return invalid(format!("{description} must be absolute."));
    }
    let length = path.as_os_str().to_string_lossy().len();
    if length == 0 || length > MAX_STRING_BYTES {
        return invalid(format!("{description} exceeds the supported path length."));
    }
    Ok(())
}

fn validate_candidate_strings(candidate: &PatchCandidate) -> Result<(), PatchPlanError> {
    for (name, value) in [
        ("patch", candidate.patch.as_str()),
        ("to", candidate.to.as_str()),
        ("mapped target", candidate.mapped_target.as_str()),
        ("mod name", candidate.mod_name.as_str()),
        ("mod ID", candidate.mod_id.as_str()),
    ] {
        if value.is_empty() || value.len() > MAX_STRING_BYTES || value.contains('\0') {
            return invalid(format!("Patch plan contains an invalid {name}."));
        }
    }
    Ok(())
}

fn checked_relative(value: &str, description: &str) -> Result<PathBuf, PatchPlanError> {
    let relative = validate_relative_path(value)
        .map_err(|error| PatchPlanError::Invalid(format!("{description}: {error}")))?;
    if relative == Path::new(".") {
        return invalid(format!("{description} must name a file."));
    }
    Ok(relative)
}

fn validate_root(path: &Path, description: &str) -> Result<PathBuf, PatchPlanError> {
    let metadata = metadata_without_links(path, description)?;
    if !metadata.is_dir() {
        return invalid(format!("{description} is not a regular directory."));
    }
    Ok(fs::canonicalize(path)?)
}

fn validate_existing_path(root: &Path, relative: &Path) -> Result<(), PatchPlanError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        metadata_without_links(&current, "Patch source path")?;
    }
    Ok(())
}

fn validate_target_ancestry(root: &Path, relative: &Path) -> Result<(), PatchPlanError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                    return invalid("Patch target path contains a link or reparse point.");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn require_regular_file(path: &Path, description: &str) -> Result<(), PatchPlanError> {
    let metadata = metadata_without_links(path, description)?;
    if !metadata.is_file() {
        return invalid(format!("{description} is not a regular file."));
    }
    if link_count(path, &metadata)? > 1 {
        return invalid(format!(
            "{description} is a hardlink and cannot be patched safely."
        ));
    }
    Ok(())
}

fn validate_csx(
    candidate: &PatchCandidate,
    target: &Path,
    target_exists: bool,
) -> Result<(), PatchPlanError> {
    if Path::new(&candidate.patch)
        .extension()
        .and_then(|value| value.to_str())
        .is_none_or(|value| !value.eq_ignore_ascii_case("csx"))
    {
        return invalid(format!(
            "CSX patch \"{}\" from \"{}\" must use the .csx extension.",
            candidate.patch, candidate.mod_name
        ));
    }
    let basename = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !["data.win", "game.ios", "game.unx", "game.droid"]
        .iter()
        .any(|value| basename.eq_ignore_ascii_case(value))
    {
        return invalid(format!(
            "CSX patch from \"{}\" must target a supported GameMaker data file.",
            candidate.mod_name
        ));
    }
    if !target_exists {
        return invalid(format!(
            "CSX target \"{}\" required by \"{}\" does not exist.",
            candidate.to, candidate.mod_name
        ));
    }
    Ok(())
}

fn metadata_without_links(path: &Path, description: &str) -> Result<fs::Metadata, PatchPlanError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return invalid(format!("{description} contains a link or reparse point."));
    }
    Ok(metadata)
}

fn push_snapshot(
    path: &Path,
    snapshots: &mut Vec<Snapshot>,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), PatchPlanError> {
    if seen.insert(path.to_path_buf()) {
        snapshots.push(snapshot(path)?);
    }
    Ok(())
}

fn snapshot(path: &Path) -> Result<Snapshot, PatchPlanError> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(Snapshot {
        path: path.to_path_buf(),
        identity: identity(path, &metadata)?,
        length: metadata.len(),
        modified: metadata.modified().ok(),
        links: link_count(path, &metadata)?,
        regular_file: metadata.is_file(),
    })
}

fn verify_snapshot(expected: &Snapshot) -> Result<(), PatchPlanError> {
    let metadata = metadata_without_links(&expected.path, "Patch plan path")?;
    let current = Snapshot {
        path: expected.path.clone(),
        identity: identity(&expected.path, &metadata)?,
        length: metadata.len(),
        modified: metadata.modified().ok(),
        links: link_count(&expected.path, &metadata)?,
        regular_file: metadata.is_file(),
    };
    if current.identity != expected.identity
        || current.length != expected.length
        || current.modified != expected.modified
        || current.links != expected.links
        || current.regular_file != expected.regular_file
        || (current.regular_file && current.links > 1)
    {
        return invalid("Patch plan paths changed during validation.");
    }
    Ok(())
}

fn comparison_key(path: &Path, case: CaseSensitivity) -> String {
    let value = path.to_string_lossy().into_owned();
    match case {
        CaseSensitivity::Sensitive => value,
        CaseSensitivity::Insensitive => value.to_lowercase(),
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T, PatchPlanError> {
    Err(PatchPlanError::Invalid(message.into()))
}

#[cfg(unix)]
fn identity(_path: &Path, metadata: &fs::Metadata) -> Result<Identity, PatchPlanError> {
    use std::os::unix::fs::MetadataExt;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn identity(path: &Path, _metadata: &fs::Metadata) -> Result<Identity, PatchPlanError> {
    Ok(windows_metadata(path)?.identity)
}

#[cfg(not(any(unix, windows)))]
fn identity(_path: &Path, _metadata: &fs::Metadata) -> Result<Identity, PatchPlanError> {
    Ok(())
}

#[cfg(unix)]
fn link_count(_path: &Path, metadata: &fs::Metadata) -> Result<u64, PatchPlanError> {
    use std::os::unix::fs::MetadataExt;
    Ok(metadata.nlink())
}

#[cfg(windows)]
fn link_count(path: &Path, _metadata: &fs::Metadata) -> Result<u64, PatchPlanError> {
    Ok(u64::from(windows_metadata(path)?.link_count))
}

#[cfg(not(any(unix, windows)))]
fn link_count(_path: &Path, _metadata: &fs::Metadata) -> Result<u64, PatchPlanError> {
    Ok(1)
}

#[cfg(windows)]
fn windows_metadata(path: &Path) -> Result<fence_windows::NodeMetadata, PatchPlanError> {
    use fence_windows::RootHandle;
    let parent = path
        .parent()
        .ok_or_else(|| PatchPlanError::Invalid("Path has no parent.".to_owned()))?;
    let name = path
        .file_name()
        .ok_or_else(|| PatchPlanError::Invalid("Path has no file name.".to_owned()))?;
    let root = RootHandle::open(parent).map_err(|error| io::Error::other(error.to_string()))?;
    let node = root
        .directory()
        .open_named_child(name)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(node.metadata())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct Fixture {
        _root: tempfile::TempDir,
        game: PathBuf,
        mod_root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempdir().unwrap();
            let game = root.path().join("game");
            let mod_root = root.path().join("mod");
            fs::create_dir(&game).unwrap();
            fs::create_dir(&mod_root).unwrap();
            fs::write(mod_root.join("source.bin"), b"patch").unwrap();
            Self {
                _root: root,
                game,
                mod_root,
            }
        }

        fn candidate(&self, patch_type: PatchType, target: &str) -> PatchCandidate {
            PatchCandidate {
                patch_type,
                patch: "source.bin".to_owned(),
                to: target.to_owned(),
                mapped_target: target.to_owned(),
                mod_name: "Example".to_owned(),
                mod_id: "example-id".to_owned(),
                mod_root: self.mod_root.clone(),
            }
        }

        fn request(
            &self,
            platform: PatchPlatform,
            patches: Vec<PatchCandidate>,
        ) -> PatchPlanRequest {
            PatchPlanRequest {
                game_root: self.game.clone(),
                platform,
                patches,
            }
        }
    }

    #[test]
    fn validates_all_patch_types_and_mapped_targets() {
        let fixture = Fixture::new();
        fs::write(fixture.game.join("merge-a"), b"game").unwrap();
        fs::write(fixture.game.join("merge-b"), b"game").unwrap();
        fs::write(fixture.game.join("game.unx"), b"game").unwrap();
        fs::write(fixture.mod_root.join("script.csx"), b"script").unwrap();
        let mut csx = fixture.candidate(PatchType::Csx, "data.win");
        csx.patch = "script.csx".to_owned();
        csx.mapped_target = "game.unx".to_owned();
        let patches = vec![
            fixture.candidate(PatchType::Override, "direct-a"),
            fixture.candidate(PatchType::Copy, "direct-b"),
            fixture.candidate(PatchType::Xdelta, "merge-a"),
            fixture.candidate(PatchType::G3mPatch, "merge-b"),
            csx,
        ];
        let approval =
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, patches)).unwrap();
        assert_eq!(approval.operation_count, 5);
        assert_eq!(approval.patch_count, 5);
        assert!(approval.snapshot_count >= 5);
    }

    #[test]
    fn groups_merge_and_csx_operations_without_reordering() {
        let fixture = Fixture::new();
        fs::write(fixture.game.join("merge"), b"game").unwrap();
        fs::write(fixture.game.join("data.win"), b"game").unwrap();
        fs::write(fixture.mod_root.join("script.csx"), b"script").unwrap();
        let mut script_a = fixture.candidate(PatchType::Csx, "data.win");
        script_a.patch = "script.csx".to_owned();
        let script_b = script_a.clone();
        let patches = vec![
            fixture.candidate(PatchType::Xdelta, "merge"),
            fixture.candidate(PatchType::G3mPatch, "merge"),
            script_a,
            script_b,
        ];
        let approval =
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, patches)).unwrap();
        assert_eq!(approval.operation_count, 2);
    }

    #[test]
    fn selects_windows_case_insensitive_and_posix_sensitive_conflicts() {
        let fixture = Fixture::new();
        let patches = vec![
            fixture.candidate(PatchType::Override, "Data/File"),
            fixture.candidate(PatchType::Copy, "data/file"),
        ];
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, patches.clone())).is_ok()
        );
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Darwin, patches.clone())).is_ok()
        );
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Win32, patches))
                .unwrap_err()
                .to_string()
                .contains("modified by both")
        );
    }

    #[test]
    fn rejects_absolute_traversal_encoded_and_empty_paths() {
        let fixture = Fixture::new();
        for unsafe_path in [
            "",
            "../outside",
            "%252e%252e%252foutside",
            "/absolute",
            "C:\\absolute",
            "\\\\server\\share",
            "%zz",
        ] {
            let patch = fixture.candidate(PatchType::Override, unsafe_path);
            assert!(
                validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![patch])).is_err()
            );
        }
    }

    #[test]
    fn rejects_conflicting_patch_kinds_and_missing_merge_targets() {
        let fixture = Fixture::new();
        let direct_merge = vec![
            fixture.candidate(PatchType::Override, "same"),
            fixture.candidate(PatchType::Xdelta, "same"),
        ];
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, direct_merge))
                .unwrap_err()
                .to_string()
                .contains("direct and merge")
        );

        let missing = fixture.candidate(PatchType::G3mPatch, "missing");
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![missing]))
                .unwrap_err()
                .to_string()
                .contains("does not exist")
        );
    }

    #[test]
    fn enforces_csx_extension_target_and_existing_regular_file() {
        let fixture = Fixture::new();
        let wrong_extension = fixture.candidate(PatchType::Csx, "data.win");
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![wrong_extension]))
                .unwrap_err()
                .to_string()
                .contains(".csx extension")
        );

        fs::write(fixture.mod_root.join("script.csx"), b"script").unwrap();
        let mut missing = fixture.candidate(PatchType::Csx, "data.win");
        missing.patch = "script.csx".to_owned();
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![missing.clone()]))
                .unwrap_err()
                .to_string()
                .contains("does not exist")
        );

        fs::write(fixture.game.join("target.txt"), b"game").unwrap();
        missing.to = "target.txt".to_owned();
        missing.mapped_target = "target.txt".to_owned();
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![missing]))
                .unwrap_err()
                .to_string()
                .contains("GameMaker data file")
        );
    }

    #[test]
    fn rejects_source_and_target_hardlinks() {
        let fixture = Fixture::new();
        fs::hard_link(
            fixture.mod_root.join("source.bin"),
            fixture.mod_root.join("second"),
        )
        .unwrap();
        let patch = fixture.candidate(PatchType::Override, "target");
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![patch]))
                .unwrap_err()
                .to_string()
                .contains("hardlink")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_sources_targets_and_ancestors() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        symlink(
            fixture.mod_root.join("source.bin"),
            fixture.mod_root.join("linked"),
        )
        .unwrap();
        let mut source_link = fixture.candidate(PatchType::Override, "target");
        source_link.patch = "linked".to_owned();
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![source_link])).is_err()
        );

        symlink(
            fixture.mod_root.join("source.bin"),
            fixture.game.join("linked-target"),
        )
        .unwrap();
        let target_link = fixture.candidate(PatchType::Override, "linked-target");
        assert!(
            validate_patch_plan(&fixture.request(PatchPlatform::Linux, vec![target_link])).is_err()
        );
    }
}
