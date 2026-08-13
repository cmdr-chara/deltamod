#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("{kind} executable is unavailable for {platform}-{arch}")]
    Unsupported {
        kind: &'static str,
        platform: String,
        arch: String,
    },
    #[error("{kind} executable is not a regular, non-linked file: {path}")]
    InvalidExecutable { kind: &'static str, path: PathBuf },
    #[error("{kind} executable hash mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        kind: &'static str,
        expected: String,
        actual: String,
    },
    #[error("{kind} timed out")]
    Timeout { kind: &'static str },
    #[error("{kind} failed to start: {source}")]
    Start {
        kind: &'static str,
        source: io::Error,
    },
    #[error("{kind} exited unsuccessfully: {status}; output: {output}")]
    Failed {
        kind: &'static str,
        status: String,
        output: String,
    },
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("workspace validation failed: {0}")]
    Workspace(String),
    #[error("process registry error")]
    Registry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    G3mTool,
    UndertaleModCli,
    WinUi,
    ControllerMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPath {
    pub kind: ToolKind,
    pub path: PathBuf,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub kind: ToolKind,
    pub executable: PathBuf,
    pub argv: Vec<OsString>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
}

fn regular_file(path: &Path, kind: &'static str) -> Result<PathBuf, RuntimeError> {
    let link = fs::symlink_metadata(path).map_err(|_| RuntimeError::InvalidExecutable {
        kind,
        path: path.to_owned(),
    })?;
    let meta = fs::metadata(path).map_err(|_| RuntimeError::InvalidExecutable {
        kind,
        path: path.to_owned(),
    })?;
    if !link.file_type().is_file()
        || link.file_type().is_symlink()
        || hard_link_count(&link) != 1
        || !meta.is_file()
    {
        return Err(RuntimeError::InvalidExecutable {
            kind,
            path: path.to_owned(),
        });
    }
    fs::canonicalize(path).map_err(|_| RuntimeError::InvalidExecutable {
        kind,
        path: path.to_owned(),
    })
}

#[cfg(unix)]
fn hard_link_count(meta: &fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::nlink(meta)
}
#[cfg(windows)]
fn hard_link_count(_: &fs::Metadata) -> u64 {
    1
}
#[cfg(not(any(unix, windows)))]
fn hard_link_count(_: &fs::Metadata) -> u64 {
    1
}

pub fn sha256_file(path: &Path) -> Result<String, RuntimeError> {
    let mut file =
        File::open(path).map_err(|_| RuntimeError::InvalidPath(path.display().to_string()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| RuntimeError::InvalidPath(path.display().to_string()))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}

pub fn verify_tool(
    path: &Path,
    kind: ToolKind,
    expected_sha256: Option<&str>,
) -> Result<ToolPath, RuntimeError> {
    let name = kind.name();
    let canonical = regular_file(path, name)?;
    let actual = sha256_file(&canonical)?;
    if let Some(expected) = expected_sha256 {
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(RuntimeError::HashMismatch {
                kind: name,
                expected: expected.to_owned(),
                actual,
            });
        }
    }
    Ok(ToolPath {
        kind,
        path: canonical,
        sha256: Some(actual),
    })
}

impl ToolKind {
    fn name(self) -> &'static str {
        match self {
            Self::G3mTool => "G3MTool",
            Self::UndertaleModCli => "UndertaleModCli",
            Self::WinUi => "WinUI",
            Self::ControllerMode => "ControllerMode",
        }
    }
    pub fn packaged_path(
        self,
        root: &Path,
        platform: &str,
        arch: &str,
    ) -> Result<PathBuf, RuntimeError> {
        let suffix = match (self, platform, arch) {
            (Self::G3mTool, "win32", "x64") => ["g3mtool", "win-x64", "G3MTool.exe"].as_slice(),
            (Self::G3mTool, "linux", "x64") => ["g3mtool", "linux-x64", "G3MTool"].as_slice(),
            (Self::G3mTool, "darwin", "x64") => ["g3mtool", "mac-x64", "G3MTool"].as_slice(),
            (Self::G3mTool, "darwin", "arm64") => ["g3mtool", "mac-arm64", "G3MTool"].as_slice(),
            (Self::UndertaleModCli, "win32", "x64") => {
                ["undertale-mod-tool", "win-x64", "UndertaleModCli.exe"].as_slice()
            }
            (Self::UndertaleModCli, "linux", "x64") => {
                ["undertale-mod-tool", "linux-x64", "UndertaleModCli"].as_slice()
            }
            (Self::UndertaleModCli, "darwin", "x64") => {
                ["undertale-mod-tool", "mac-x64", "UndertaleModCli"].as_slice()
            }
            (Self::WinUi, "win32", "x64") => ["winui", "win-x64", "Deltamod.WinUI.exe"].as_slice(),
            (Self::ControllerMode, "win32", _) => ["tools", "cmodeutil.exe"].as_slice(),
            _ => {
                return Err(RuntimeError::Unsupported {
                    kind: self.name(),
                    platform: platform.to_owned(),
                    arch: arch.to_owned(),
                })
            }
        };
        Ok(suffix.iter().fold(root.to_owned(), |p, part| p.join(part)))
    }
}

pub fn g3m_apply(
    tool: &ToolPath,
    game_root: &Path,
    backup: &Path,
    source: &Path,
    target_relative: &Path,
) -> CommandSpec {
    CommandSpec {
        kind: ToolKind::G3mTool,
        executable: tool.path.clone(),
        argv: vec![
            "patch".into(),
            "apply".into(),
            backup.to_owned().into(),
            source.to_owned().into(),
            target_relative.as_os_str().to_owned(),
        ],
        cwd: game_root.to_owned(),
        env: scrubbed_env(),
    }
}
pub fn g3m_merge(
    tool: &ToolPath,
    game_root: &Path,
    backup: &Path,
    sources: &[PathBuf],
    target: &Path,
) -> CommandSpec {
    let mut argv = vec!["patch".into(), "merge".into(), backup.to_owned().into()];
    argv.extend(sources.iter().cloned().map(Into::into));
    argv.extend(["-a".into(), target.to_owned().into()]);
    CommandSpec {
        kind: ToolKind::G3mTool,
        executable: tool.path.clone(),
        argv,
        cwd: game_root.to_owned(),
        env: scrubbed_env(),
    }
}
pub fn undertale_mod_cli(
    tool: &ToolPath,
    input: &Path,
    output: &Path,
    scripts: &[PathBuf],
) -> CommandSpec {
    let mut argv = vec![
        "load".into(),
        input.to_owned().into(),
        "--verbose".into(),
        "--output".into(),
        output.to_owned().into(),
        "--scripts".into(),
    ];
    argv.extend(scripts.iter().cloned().map(Into::into));
    let cwd = tool.path.parent().unwrap_or(Path::new(".")).to_owned();
    CommandSpec {
        kind: ToolKind::UndertaleModCli,
        executable: tool.path.clone(),
        argv,
        cwd,
        env: scrubbed_env(),
    }
}
pub fn winui_launch(tool: &ToolPath, data_file: &Path) -> CommandSpec {
    CommandSpec {
        kind: ToolKind::WinUi,
        executable: tool.path.clone(),
        argv: vec!["--open".into(), data_file.to_owned().into()],
        cwd: tool.path.parent().unwrap_or(Path::new(".")).to_owned(),
        env: scrubbed_env(),
    }
}

pub fn controller_mode_launch(tool: &ToolPath) -> CommandSpec {
    CommandSpec {
        kind: ToolKind::ControllerMode,
        executable: tool.path.clone(),
        argv: Vec::new(),
        cwd: tool.path.parent().unwrap_or(Path::new(".")).to_owned(),
        env: scrubbed_env(),
    }
}

pub fn scrubbed_env() -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::new();
    for key in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "TEMP",
        "TMP",
        "SystemRoot",
        "WINDIR",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            env.insert(key.into(), value);
        }
    }
    env
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub timed_out: bool,
}

pub struct OwnedProcess {
    id: u64,
    child: Arc<Mutex<Child>>,
    registry: ProcessRegistry,
    #[cfg(windows)]
    job: Option<Arc<fence_windows::KillOnCloseJob>>,
}
impl OwnedProcess {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn terminate(self) -> Result<(), RuntimeError> {
        let mut child = self.child.lock().map_err(|_| RuntimeError::Registry)?;
        terminate_child(&mut child);
        child.wait().map(|_| ()).map_err(|_| RuntimeError::Registry)
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _job = &self.job;
        if let Ok(mut processes) = self.registry.inner.lock() {
            processes.remove(&self.id);
        }
    }
}

#[derive(Clone, Default)]
pub struct ProcessRegistry {
    inner: Arc<Mutex<BTreeMap<u64, Arc<Mutex<Child>>>>>,
}
impl ProcessRegistry {
    pub fn spawn(&self, spec: &CommandSpec) -> Result<OwnedProcess, RuntimeError> {
        self.spawn_with_output(spec, true)
    }

    pub fn spawn_silent(&self, spec: &CommandSpec) -> Result<OwnedProcess, RuntimeError> {
        self.spawn_with_output(spec, false)
    }

    fn spawn_with_output(
        &self,
        spec: &CommandSpec,
        capture_output: bool,
    ) -> Result<OwnedProcess, RuntimeError> {
        let mut command = Command::new(&spec.executable);
        command.args(&spec.argv).current_dir(&spec.cwd).env_clear();
        for (k, v) in &spec.env {
            command.env(k, v);
        }
        command.stdin(Stdio::null());
        if capture_output {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = Arc::new(Mutex::new(command.spawn().map_err(|source| {
            RuntimeError::Start {
                kind: spec.kind.name(),
                source,
            }
        })?));
        #[cfg(windows)]
        let job = {
            let job =
                Arc::new(fence_windows::KillOnCloseJob::new().map_err(|_| RuntimeError::Registry)?);
            {
                let guard = child.lock().map_err(|_| RuntimeError::Registry)?;
                job.assign(&guard).map_err(|_| RuntimeError::Registry)?;
            }
            Some(job)
        };
        let id = u64::from(child.lock().map_err(|_| RuntimeError::Registry)?.id());
        self.inner
            .lock()
            .map_err(|_| RuntimeError::Registry)?
            .insert(id, Arc::clone(&child));
        Ok(OwnedProcess {
            id,
            child,
            registry: self.clone(),
            #[cfg(windows)]
            job,
        })
    }
    pub fn terminate_all(&self) {
        if let Ok(mut processes) = self.inner.lock() {
            for child in processes.values_mut() {
                if let Ok(mut child) = child.lock() {
                    terminate_child(&mut child);
                }
            }
            processes.clear();
        }
    }
}

pub fn run_bounded(
    spec: &CommandSpec,
    timeout: Duration,
    max_output: usize,
) -> Result<ProcessOutput, RuntimeError> {
    let registry = ProcessRegistry::default();
    let owned = registry.spawn(spec)?;
    let (stdout_pipe, stderr_pipe) = {
        let mut child = owned.child.lock().map_err(|_| RuntimeError::Registry)?;
        (child.stdout.take(), child.stderr.take())
    };
    let stdout_thread = thread::spawn(move || {
        stdout_pipe
            .map(|pipe| read_bounded(pipe, max_output))
            .transpose()
    });
    let stderr_thread = thread::spawn(move || {
        stderr_pipe
            .map(|pipe| read_bounded(pipe, max_output))
            .transpose()
    });
    let start = Instant::now();
    loop {
        let mut child = owned.child.lock().map_err(|_| RuntimeError::Registry)?;
        if let Some(status) = child.try_wait().map_err(|_| RuntimeError::Registry)? {
            drop(child);
            let stdout = join_output(stdout_thread)?;
            let stderr = join_output(stderr_thread)?;
            let truncated = stdout.len() + stderr.len() > max_output;
            let split = stdout.len().min(max_output);
            return Ok(ProcessOutput {
                status,
                stdout: String::from_utf8_lossy(&stdout[..split]).into_owned(),
                stderr: String::from_utf8_lossy(
                    &stderr[..stderr.len().min(max_output.saturating_sub(split))],
                )
                .into_owned(),
                truncated,
                timed_out: false,
            });
        }
        if start.elapsed() >= timeout {
            terminate_child(&mut child);
            let status = child.wait().map_err(|_| RuntimeError::Registry)?;
            drop(child);
            let stdout = join_output(stdout_thread)?;
            let stderr = join_output(stderr_thread)?;
            return Ok(ProcessOutput {
                status,
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
                truncated: stdout.len() >= max_output || stderr.len() >= max_output,
                timed_out: true,
            });
        }
        drop(child);
        thread::sleep(Duration::from_millis(10));
    }
}
fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    while bytes.len() < limit {
        let read_limit = buffer.len().min(limit - bytes.len());
        let amount = reader.read(&mut buffer[..read_limit])?;
        if amount == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..amount]);
    }
    Ok(bytes)
}
fn join_output(
    handle: thread::JoinHandle<io::Result<Option<Vec<u8>>>>,
) -> Result<Vec<u8>, RuntimeError> {
    handle
        .join()
        .map_err(|_| RuntimeError::Registry)?
        .map_err(|_| RuntimeError::Registry)
        .map(|value| value.unwrap_or_default())
}
fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
    let _ = child.kill();
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePlan {
    pub staging: PathBuf,
    pub final_root: PathBuf,
    pub data_file: PathBuf,
    pub source_sha256: String,
    pub size: u64,
}
pub fn plan_workspace(root: &Path, source: &Path, id: &str) -> Result<WorkspacePlan, RuntimeError> {
    if !root.is_absolute()
        || !source.is_absolute()
        || id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(RuntimeError::Workspace(
            "absolute paths and a safe id are required".into(),
        ));
    }
    let source = regular_file(source, "workspace source")?;
    let meta =
        fs::metadata(&source).map_err(|_| RuntimeError::Workspace("source unavailable".into()))?;
    let final_root = root.join(id);
    let staging = root.join(format!("{id}.staging"));
    Ok(WorkspacePlan {
        data_file: staging
            .join("editor")
            .join(source.file_name().unwrap_or_default()),
        staging,
        final_root,
        source_sha256: sha256_file(&source)?,
        size: meta.len(),
    })
}
pub fn materialize_workspace(plan: &WorkspacePlan, source: &Path) -> Result<(), RuntimeError> {
    fs::create_dir_all(plan.data_file.parent().unwrap())
        .map_err(|e| RuntimeError::Workspace(e.to_string()))?;
    fs::copy(source, &plan.data_file).map_err(|e| RuntimeError::Workspace(e.to_string()))?;
    let copied =
        fs::metadata(&plan.data_file).map_err(|e| RuntimeError::Workspace(e.to_string()))?;
    if copied.len() != plan.size || sha256_file(&plan.data_file)? != plan.source_sha256 {
        return Err(RuntimeError::Workspace(
            "workspace copy verification failed".into(),
        ));
    }
    fs::create_dir_all(plan.staging.join("exports"))
        .map_err(|e| RuntimeError::Workspace(e.to_string()))?;
    fs::rename(&plan.staging, &plan.final_root).map_err(|e| RuntimeError::Workspace(e.to_string()))
}

pub fn reveal_folder(path: &Path) -> CommandSpec {
    #[cfg(windows)]
    let (exe, args) = (
        OsString::from("explorer.exe"),
        vec![path.as_os_str().to_owned()],
    );
    #[cfg(target_os = "macos")]
    let (exe, args) = (OsString::from("open"), vec![path.as_os_str().to_owned()]);
    #[cfg(all(unix, not(target_os = "macos")))]
    let (exe, args) = (
        OsString::from("xdg-open"),
        vec![path.as_os_str().to_owned()],
    );
    CommandSpec {
        kind: ToolKind::WinUi,
        executable: PathBuf::from(exe),
        argv: args,
        cwd: path.to_owned(),
        env: scrubbed_env(),
    }
}

pub mod legacy {
    use super::*;
    pub type LegacyResult<T> = Result<T, String>;
    pub fn map<T>(result: Result<T, RuntimeError>) -> LegacyResult<T> {
        result.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn g3m_argv_is_exact() {
        let t = ToolPath {
            kind: ToolKind::G3mTool,
            path: PathBuf::from("G3MTool"),
            sha256: None,
        };
        let s = g3m_apply(
            &t,
            Path::new("game"),
            Path::new("backup"),
            Path::new("source"),
            Path::new("data.win"),
        );
        assert_eq!(
            s.argv,
            vec!["patch", "apply", "backup", "source", "data.win"]
        );
        assert_eq!(s.cwd, PathBuf::from("game"));
    }
    #[test]
    fn cli_argv_is_exact() {
        let t = ToolPath {
            kind: ToolKind::UndertaleModCli,
            path: PathBuf::from("tools/UndertaleModCli"),
            sha256: None,
        };
        let s = undertale_mod_cli(
            &t,
            Path::new("in"),
            Path::new("out"),
            &[PathBuf::from("a.csx")],
        );
        assert_eq!(
            s.argv,
            vec![
                "load",
                "in",
                "--verbose",
                "--output",
                "out",
                "--scripts",
                "a.csx"
            ]
        );
    }
    #[test]
    fn environment_is_scrubbed() {
        let e = scrubbed_env();
        assert!(!e.contains_key(std::ffi::OsStr::new("SECRET")));
    }

    #[test]
    fn controller_mode_is_windows_only_and_has_no_arguments() {
        let path = ToolKind::ControllerMode
            .packaged_path(Path::new("resources"), "win32", "x64")
            .unwrap();
        assert_eq!(path, PathBuf::from("resources/tools/cmodeutil.exe"));
        assert!(ToolKind::ControllerMode
            .packaged_path(Path::new("resources"), "linux", "x64")
            .is_err());
        let tool = ToolPath {
            kind: ToolKind::ControllerMode,
            path,
            sha256: None,
        };
        assert!(controller_mode_launch(&tool).argv.is_empty());
    }
}
