#![forbid(unsafe_code)]

mod secure_path;

pub use secure_path::{
    copy_relative_regular_file_to_open_file_verified, copy_relative_regular_file_verified,
    inspect_directory_identity, inspect_regular_file, SecurePathError, StablePathIdentity,
    VerifiedFile,
};

use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const EXECUTABLE_IDENTITY_ENV: &str = "__DELTAMOD_TOOLS_RUNTIME_EXECUTABLE_IDENTITY";
const EXECUTABLE_SHA256_ENV: &str = "__DELTAMOD_TOOLS_RUNTIME_EXECUTABLE_SHA256";

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
    #[error("{kind} execution was cancelled")]
    Cancelled { kind: &'static str },
    #[error("{kind} exceeded the aggregate stdout/stderr limit of {limit} bytes")]
    OutputOverflow { kind: &'static str, limit: usize },
    #[error("{kind} executable is not pinned to a stable identity and SHA-256 digest")]
    UnpinnedExecutable { kind: &'static str },
    #[error("{kind} executable changed while it was being launched")]
    ExecutableChanged { kind: &'static str },
    #[error("{kind} failed to start: {source}")]
    Start {
        kind: &'static str,
        source: io::Error,
    },
    #[error("{kind} output capture failed: {source}")]
    Output {
        kind: &'static str,
        source: io::Error,
    },
    #[error("{kind} process could not be reaped: {source}")]
    Reap {
        kind: &'static str,
        source: io::Error,
    },
    #[error("{kind} process containment failed: {detail}")]
    Containment { kind: &'static str, detail: String },
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

impl CommandSpec {
    pub fn pin_to(&mut self, tool: &ToolPath) -> Result<(), RuntimeError> {
        if self.kind != tool.kind || self.executable != tool.path {
            return Err(RuntimeError::InvalidExecutable {
                kind: self.kind.name(),
                path: self.executable.clone(),
            });
        }
        let sha256 = tool
            .sha256
            .as_deref()
            .filter(|value| valid_sha256(value))
            .ok_or(RuntimeError::UnpinnedExecutable {
                kind: self.kind.name(),
            })?;
        let inspected = inspect_regular_file(&tool.path, u64::MAX)
            .map_err(|error| executable_inspection_error(tool.kind, &tool.path, error))?;
        if !inspected.sha256().eq_ignore_ascii_case(sha256) {
            return Err(RuntimeError::HashMismatch {
                kind: self.kind.name(),
                expected: sha256.to_owned(),
                actual: inspected.sha256().to_owned(),
            });
        }
        self.env.insert(
            EXECUTABLE_IDENTITY_ENV.into(),
            inspected.identity().token().into(),
        );
        self.env.insert(EXECUTABLE_SHA256_ENV.into(), sha256.into());
        Ok(())
    }
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
    let inspected = inspect_regular_file(&canonical, u64::MAX)
        .map_err(|error| executable_inspection_error(kind, &canonical, error))?;
    let actual = inspected.sha256().to_owned();
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
        env: pinned_env(tool),
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
        env: pinned_env(tool),
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
        env: pinned_env(tool),
    }
}

pub fn winui_launch(tool: &ToolPath, data_file: &Path) -> CommandSpec {
    CommandSpec {
        kind: ToolKind::WinUi,
        executable: tool.path.clone(),
        argv: vec!["--open".into(), data_file.to_owned().into()],
        cwd: tool.path.parent().unwrap_or(Path::new(".")).to_owned(),
        env: pinned_env(tool),
    }
}

pub fn controller_mode_launch(tool: &ToolPath) -> CommandSpec {
    CommandSpec {
        kind: ToolKind::ControllerMode,
        executable: tool.path.clone(),
        argv: Vec::new(),
        cwd: tool.path.parent().unwrap_or(Path::new(".")).to_owned(),
        env: pinned_env(tool),
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

fn pinned_env(tool: &ToolPath) -> BTreeMap<OsString, OsString> {
    let mut env = scrubbed_env();
    if let Some(sha256) = tool.sha256.as_deref().filter(|value| valid_sha256(value)) {
        if let Ok(inspected) = inspect_regular_file(&tool.path, u64::MAX) {
            env.insert(
                EXECUTABLE_IDENTITY_ENV.into(),
                inspected.identity().token().into(),
            );
            env.insert(EXECUTABLE_SHA256_ENV.into(), sha256.into());
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

#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

struct ManagedProcess {
    kind: &'static str,
    child: Mutex<Child>,
    reaped: AtomicBool,
    #[cfg(windows)]
    job: Mutex<Option<fence_windows::KillOnCloseJob>>,
}

impl ManagedProcess {
    fn try_wait(&self) -> Result<Option<ExitStatus>, RuntimeError> {
        self.child
            .lock()
            .map_err(|_| RuntimeError::Registry)?
            .try_wait()
            .map_err(|source| RuntimeError::Reap {
                kind: self.kind,
                source,
            })
    }

    fn terminate_and_reap(&self) -> Result<(), RuntimeError> {
        if self.reaped.load(Ordering::Acquire) {
            return Ok(());
        }

        let mut child = self.child.lock().map_err(|_| RuntimeError::Registry)?;
        if self.reaped.load(Ordering::Acquire) {
            return Ok(());
        }

        #[cfg(unix)]
        let mut containment_error = None;
        #[cfg(not(unix))]
        let containment_error: Option<String> = None;
        #[cfg(windows)]
        {
            let job = self.job.lock().map_err(|_| RuntimeError::Registry)?.take();
            drop(job);
        }
        #[cfg(unix)]
        {
            if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
                if let Err(error) =
                    rustix::process::kill_process_group(pid, rustix::process::Signal::KILL)
                {
                    if error != rustix::io::Errno::SRCH {
                        containment_error = Some(error.to_string());
                    }
                }
            }
        }
        let _ = child.kill();
        child.wait().map_err(|source| RuntimeError::Reap {
            kind: self.kind,
            source,
        })?;

        #[cfg(unix)]
        if containment_error.is_none() {
            if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
                let mut group_gone = false;
                for _ in 0..100 {
                    match rustix::process::test_kill_process_group(pid) {
                        Err(rustix::io::Errno::SRCH) => {
                            group_gone = true;
                            break;
                        }
                        Ok(()) => thread::sleep(Duration::from_millis(1)),
                        Err(error) => {
                            containment_error = Some(error.to_string());
                            break;
                        }
                    }
                }
                if containment_error.is_none() && !group_gone {
                    containment_error =
                        Some("process group remained live after termination".into());
                }
            }
        }

        if let Some(detail) = containment_error {
            return Err(RuntimeError::Containment {
                kind: self.kind,
                detail,
            });
        }
        self.reaped.store(true, Ordering::Release);
        Ok(())
    }
}

pub struct OwnedProcess {
    id: u64,
    process: Arc<ManagedProcess>,
    registry: ProcessRegistry,
}
impl OwnedProcess {
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn terminate(self) -> Result<(), RuntimeError> {
        self.process.terminate_and_reap()
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.process.terminate_and_reap();
        if let Ok(mut processes) = self.registry.inner.lock() {
            processes.remove(&self.id);
        }
    }
}

#[derive(Clone, Default)]
pub struct ProcessRegistry {
    inner: Arc<Mutex<BTreeMap<u64, Arc<ManagedProcess>>>>,
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
        let (executable, command_env) = pin_command_executable(spec)?;
        #[cfg(windows)]
        let executable_identity = executable.verified().clone();
        let launch_path = executable
            .launch_path(&spec.executable)
            .map_err(|error| executable_inspection_error(spec.kind, &spec.executable, error))?;
        #[cfg(windows)]
        drop(executable);
        #[cfg(not(windows))]
        let mut executable = executable;
        let mut command = Command::new(launch_path);
        command.args(&spec.argv).current_dir(&spec.cwd).env_clear();
        for (k, v) in &command_env {
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
        #[cfg(windows)]
        let job =
            fence_windows::KillOnCloseJob::new().map_err(|error| RuntimeError::Containment {
                kind: spec.kind.name(),
                detail: error.to_string(),
            })?;
        let mut child = command.spawn().map_err(|source| RuntimeError::Start {
            kind: spec.kind.name(),
            source,
        })?;
        #[cfg(windows)]
        if let Err(error) = job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RuntimeError::Containment {
                kind: spec.kind.name(),
                detail: error.to_string(),
            });
        }
        let id = u64::from(child.id());
        let process = Arc::new(ManagedProcess {
            kind: spec.kind.name(),
            child: Mutex::new(child),
            reaped: AtomicBool::new(false),
            #[cfg(windows)]
            job: Mutex::new(Some(job)),
        });
        #[cfg(windows)]
        let executable_verification =
            inspect_regular_file(&spec.executable, u64::MAX).and_then(|current| {
                if current == executable_identity {
                    Ok(())
                } else {
                    Err(SecurePathError::Changed)
                }
            });
        #[cfg(not(windows))]
        let executable_verification = executable.verify_unchanged(u64::MAX);
        if let Err(error) = executable_verification {
            let launch_error = executable_inspection_error(spec.kind, &spec.executable, error);
            process.terminate_and_reap()?;
            return Err(launch_error);
        }
        let mut processes = match self.inner.lock() {
            Ok(processes) => processes,
            Err(_) => {
                process.terminate_and_reap()?;
                return Err(RuntimeError::Registry);
            }
        };
        processes.insert(id, Arc::clone(&process));
        drop(processes);
        Ok(OwnedProcess {
            id,
            process,
            registry: self.clone(),
        })
    }
    pub fn terminate_all(&self) {
        let processes = if let Ok(mut registry) = self.inner.lock() {
            let processes = registry.values().cloned().collect::<Vec<_>>();
            registry.clear();
            processes
        } else {
            Vec::new()
        };
        for process in processes {
            let _ = process.terminate_and_reap();
        }
    }
}

pub fn run_bounded(
    spec: &CommandSpec,
    timeout: Duration,
    max_output: usize,
) -> Result<ProcessOutput, RuntimeError> {
    run_bounded_with_cancel_probe(spec, timeout, max_output, || false)
}

pub fn run_bounded_with_cancel(
    spec: &CommandSpec,
    timeout: Duration,
    max_output: usize,
    cancellation: &CancellationToken,
) -> Result<ProcessOutput, RuntimeError> {
    run_bounded_with_cancel_probe(spec, timeout, max_output, || cancellation.is_cancelled())
}

pub fn run_bounded_with_cancel_probe(
    spec: &CommandSpec,
    timeout: Duration,
    max_output: usize,
    mut cancellation_requested: impl FnMut() -> bool,
) -> Result<ProcessOutput, RuntimeError> {
    if cancellation_requested() {
        return Err(RuntimeError::Cancelled {
            kind: spec.kind.name(),
        });
    }
    let registry = ProcessRegistry::default();
    let owned = registry.spawn(spec)?;
    let (stdout_pipe, stderr_pipe) = {
        let mut child = owned
            .process
            .child
            .lock()
            .map_err(|_| RuntimeError::Registry)?;
        (child.stdout.take(), child.stderr.take())
    };
    let output_state = Arc::new(OutputState::new(max_output));
    let stdout_state = Arc::clone(&output_state);
    let stdout_thread = thread::Builder::new()
        .name("tools-runtime-stdout".into())
        .spawn(move || {
            stdout_pipe
                .map(|pipe| read_bounded(pipe, &stdout_state))
                .transpose()
        })
        .map_err(|source| RuntimeError::Output {
            kind: spec.kind.name(),
            source,
        })?;
    let stderr_state = Arc::clone(&output_state);
    let stderr_thread = match thread::Builder::new()
        .name("tools-runtime-stderr".into())
        .spawn(move || {
            stderr_pipe
                .map(|pipe| read_bounded(pipe, &stderr_state))
                .transpose()
        }) {
        Ok(handle) => handle,
        Err(source) => {
            let reap_result = owned.process.terminate_and_reap();
            let stdout_result = join_output(stdout_thread, spec.kind.name());
            reap_result?;
            stdout_result?;
            return Err(RuntimeError::Output {
                kind: spec.kind.name(),
                source,
            });
        }
    };
    let start = Instant::now();
    let outcome = loop {
        if cancellation_requested() {
            break RunOutcome::Stopped(StopReason::Cancelled);
        }
        if output_state.overflowed.load(Ordering::Acquire) {
            break RunOutcome::Stopped(StopReason::OutputOverflow);
        }
        if output_state.read_failed.load(Ordering::Acquire) {
            break RunOutcome::Stopped(StopReason::OutputRead);
        }
        if start.elapsed() >= timeout {
            break RunOutcome::Stopped(StopReason::Timeout);
        }
        match owned.process.try_wait() {
            Ok(Some(status)) => break RunOutcome::Completed(status),
            Ok(None) => {}
            Err(error) => break RunOutcome::Failed(error),
        }
        thread::sleep(Duration::from_millis(5));
    };

    let reap_result = owned.process.terminate_and_reap();
    let output_result = join_outputs(stdout_thread, stderr_thread, spec.kind.name());
    reap_result?;
    let (stdout, stderr) = output_result?;

    if output_state.overflowed.load(Ordering::Acquire) {
        return Err(RuntimeError::OutputOverflow {
            kind: spec.kind.name(),
            limit: max_output,
        });
    }

    match outcome {
        RunOutcome::Completed(status) => Ok(ProcessOutput {
            status,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            truncated: false,
            timed_out: false,
        }),
        RunOutcome::Stopped(reason) => Err(reason.error(spec.kind.name(), max_output)),
        RunOutcome::Failed(error) => Err(error),
    }
}

struct OutputState {
    remaining: Mutex<usize>,
    overflowed: AtomicBool,
    read_failed: AtomicBool,
}

impl OutputState {
    fn new(limit: usize) -> Self {
        Self {
            remaining: Mutex::new(limit),
            overflowed: AtomicBool::new(false),
            read_failed: AtomicBool::new(false),
        }
    }
}

enum RunOutcome {
    Completed(ExitStatus),
    Stopped(StopReason),
    Failed(RuntimeError),
}

enum StopReason {
    Cancelled,
    Timeout,
    OutputOverflow,
    OutputRead,
}

impl StopReason {
    fn error(self, kind: &'static str, limit: usize) -> RuntimeError {
        match self {
            Self::Cancelled => RuntimeError::Cancelled { kind },
            Self::Timeout => RuntimeError::Timeout { kind },
            Self::OutputOverflow => RuntimeError::OutputOverflow { kind, limit },
            Self::OutputRead => RuntimeError::Registry,
        }
    }
}

fn read_bounded(mut reader: impl Read, state: &OutputState) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let amount = match reader.read(&mut buffer) {
            Ok(amount) => amount,
            Err(error) => {
                state.read_failed.store(true, Ordering::Release);
                return Err(error);
            }
        };
        if amount == 0 {
            break;
        }
        let kept = {
            let mut remaining = state
                .remaining
                .lock()
                .map_err(|_| io::Error::other("output budget lock poisoned"))?;
            let kept = amount.min(*remaining);
            *remaining -= kept;
            kept
        };
        bytes.extend_from_slice(&buffer[..kept]);
        if kept != amount {
            state.overflowed.store(true, Ordering::Release);
        }
    }
    Ok(bytes)
}

fn join_outputs(
    stdout: thread::JoinHandle<io::Result<Option<Vec<u8>>>>,
    stderr: thread::JoinHandle<io::Result<Option<Vec<u8>>>>,
    kind: &'static str,
) -> Result<(Vec<u8>, Vec<u8>), RuntimeError> {
    let stdout = join_output(stdout, kind);
    let stderr = join_output(stderr, kind);
    Ok((stdout?, stderr?))
}

fn join_output(
    handle: thread::JoinHandle<io::Result<Option<Vec<u8>>>>,
    kind: &'static str,
) -> Result<Vec<u8>, RuntimeError> {
    handle
        .join()
        .map_err(|_| RuntimeError::Registry)?
        .map_err(|source| RuntimeError::Output { kind, source })
        .map(|value| value.unwrap_or_default())
}

fn pin_command_executable(
    spec: &CommandSpec,
) -> Result<(secure_path::PinnedRegularFile, BTreeMap<OsString, OsString>), RuntimeError> {
    let mut command_env = spec.env.clone();
    let expected_identity = command_env
        .remove(std::ffi::OsStr::new(EXECUTABLE_IDENTITY_ENV))
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
        .ok_or(RuntimeError::UnpinnedExecutable {
            kind: spec.kind.name(),
        })?;
    let expected = command_env
        .remove(std::ffi::OsStr::new(EXECUTABLE_SHA256_ENV))
        .and_then(|value| value.into_string().ok())
        .filter(|value| valid_sha256(value))
        .ok_or(RuntimeError::UnpinnedExecutable {
            kind: spec.kind.name(),
        })?;
    let pinned = secure_path::pin_regular_file(&spec.executable, u64::MAX)
        .map_err(|error| executable_inspection_error(spec.kind, &spec.executable, error))?;
    if pinned.verified().identity().token() != expected_identity {
        return Err(RuntimeError::ExecutableChanged {
            kind: spec.kind.name(),
        });
    }
    let actual = pinned.verified().sha256();
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(RuntimeError::HashMismatch {
            kind: spec.kind.name(),
            expected,
            actual: actual.to_owned(),
        });
    }
    Ok((pinned, command_env))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn executable_inspection_error(
    kind: ToolKind,
    path: &Path,
    error: SecurePathError,
) -> RuntimeError {
    if matches!(error, SecurePathError::Changed) {
        RuntimeError::ExecutableChanged { kind: kind.name() }
    } else {
        RuntimeError::InvalidExecutable {
            kind: kind.name(),
            path: path.to_owned(),
        }
    }
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
