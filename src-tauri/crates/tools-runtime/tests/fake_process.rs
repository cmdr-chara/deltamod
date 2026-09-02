use deltamod_tools_runtime::{
    run_bounded, run_bounded_with_cancel, verify_tool, CancellationToken, CommandSpec,
    RuntimeError, ToolKind,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::{Duration, Instant},
};

struct SharedFakeExecutable {
    _directory: tempfile::TempDir,
    path: PathBuf,
}

static SHARED_FAKE_EXECUTABLE: OnceLock<SharedFakeExecutable> = OnceLock::new();
static PROCESS_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serial_process_test() -> MutexGuard<'static, ()> {
    PROCESS_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn compile_fake_executable(dir: &Path) -> PathBuf {
    let source = dir.join("fake.rs");
    let output = dir.join(if cfg!(windows) { "fake.exe" } else { "fake" });
    fs::write(
        &source,
        r#"
use std::{
    env,
    fs,
    io::{self, Write as _},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn spawn_descendant(marker: &str) {
    let child = Command::new(env::current_exe().unwrap())
        .arg("descendant")
        .arg(marker)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    println!("descendant={}", child.id());
    io::stdout().flush().unwrap();
}

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("sleep") => {
            println!("ready");
            io::stdout().flush().unwrap();
            thread::sleep(Duration::from_secs(30));
        }
        Some("output") => {
            let stdout_bytes = args[1].parse::<usize>().unwrap();
            let stderr_bytes = args[2].parse::<usize>().unwrap();
            io::stdout().write_all(&vec![b'o'; stdout_bytes]).unwrap();
            io::stdout().flush().unwrap();
            io::stderr().write_all(&vec![b'e'; stderr_bytes]).unwrap();
            io::stderr().flush().unwrap();
        }
        Some("spawn-descendant") => {
            spawn_descendant(&args[1]);
            thread::sleep(Duration::from_secs(30));
        }
        Some("orphan-descendant") => spawn_descendant(&args[1]),
        Some("descendant") => {
            thread::sleep(Duration::from_millis(500));
            fs::write(&args[1], b"survived").unwrap();
            thread::sleep(Duration::from_secs(30));
        }
        _ => {
            println!("cwd={}", env::current_dir().unwrap().display());
            println!("args={:?}", args);
            println!(
                "marker={}",
                env::var("DELTAMOD_FAKE_MARKER").unwrap_or_default()
            );
            println!(
                "runtime_pin={}",
                env::var("__DELTAMOD_TOOLS_RUNTIME_EXECUTABLE_SHA256").unwrap_or_default()
            );
        }
    }
}
"#,
    )
    .unwrap();
    let rustc = option_env!("RUSTC").unwrap_or("rustc");
    let status = Command::new(rustc)
        .arg(&source)
        .arg("-o")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    output
}

fn fake_executable(dir: &Path) -> PathBuf {
    let shared = SHARED_FAKE_EXECUTABLE.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let path = compile_fake_executable(directory.path());
        SharedFakeExecutable {
            _directory: directory,
            path,
        }
    });
    let output = dir.join(if cfg!(windows) { "fake.exe" } else { "fake" });
    fs::copy(&shared.path, &output).unwrap();
    output
}

fn pinned_spec(executable: PathBuf, cwd: &Path, argv: Vec<OsString>) -> CommandSpec {
    let tool = verify_tool(&executable, ToolKind::G3mTool, None).unwrap();
    let mut spec = CommandSpec {
        kind: ToolKind::G3mTool,
        executable: tool.path.clone(),
        argv,
        cwd: cwd.to_owned(),
        env: BTreeMap::new(),
    };
    spec.pin_to(&tool).unwrap();
    spec
}

#[test]
fn fake_process_observes_exact_launch_context() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let mut spec = pinned_spec(
        executable,
        dir.path(),
        vec!["one".into(), "two words".into()],
    );
    spec.env.insert(
        OsString::from("DELTAMOD_FAKE_MARKER"),
        OsString::from("present"),
    );
    let result = run_bounded(&spec, Duration::from_secs(5), 4096).unwrap();
    assert!(
        result.status.success(),
        "status={:?}, stdout={:?}, stderr={:?}",
        result.status,
        result.stdout,
        result.stderr
    );
    assert!(result.stdout.contains(r#"args=["one", "two words"]"#));
    assert!(result.stdout.contains("marker=present"));
    assert!(result.stdout.lines().any(|line| line == "runtime_pin="));
    let reported_cwd = result
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("cwd="))
        .expect("fake process must report its working directory");
    assert_eq!(
        fs::canonicalize(reported_cwd).unwrap(),
        fs::canonicalize(dir.path()).unwrap()
    );
}

#[test]
fn timeout_is_structured_and_reaped() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(executable, dir.path(), vec!["sleep".into()]);
    let started = Instant::now();
    let error = run_bounded(&spec, Duration::from_millis(50), 4096).unwrap_err();
    assert!(matches!(error, RuntimeError::Timeout { kind: "G3MTool" }));
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[test]
fn explicit_cancellation_is_structured_and_reaped() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(executable, dir.path(), vec!["sleep".into()]);
    let cancellation = CancellationToken::new();
    let canceller = cancellation.clone();
    let cancellation_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        canceller.cancel();
    });
    let started = Instant::now();
    let error =
        run_bounded_with_cancel(&spec, Duration::from_secs(10), 4096, &cancellation).unwrap_err();
    cancellation_thread.join().unwrap();
    assert!(matches!(error, RuntimeError::Cancelled { kind: "G3MTool" }));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn stdout_and_stderr_share_one_exact_budget() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(
        executable,
        dir.path(),
        vec!["output".into(), "600".into(), "400".into()],
    );
    let output = run_bounded(&spec, Duration::from_secs(5), 1000).unwrap();
    assert_eq!(output.stdout.len() + output.stderr.len(), 1000);
    assert!(!output.truncated);
}

#[test]
fn aggregate_output_overflow_is_structured_and_terminates() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(
        executable,
        dir.path(),
        vec!["output".into(), "600".into(), "600".into()],
    );
    let error = run_bounded(&spec, Duration::from_secs(5), 1000).unwrap_err();
    assert!(
        matches!(
            error,
            RuntimeError::OutputOverflow {
                kind: "G3MTool",
                limit: 1000
            }
        ),
        "unexpected overflow result: {error:?}"
    );
}

#[test]
fn aggregate_output_overflow_terminates_descendants() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("overflow-descendant-survived");
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(
        executable,
        dir.path(),
        vec!["spawn-descendant".into(), marker.as_os_str().to_owned()],
    );
    let error = run_bounded(&spec, Duration::from_secs(5), 1).unwrap_err();
    assert!(matches!(error, RuntimeError::OutputOverflow { .. }));
    thread::sleep(Duration::from_millis(750));
    assert!(
        !marker.exists(),
        "a descendant survived the output-overflow barrier"
    );
}

#[test]
fn timeout_terminates_the_entire_process_tree() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("descendant-survived");
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(
        executable,
        dir.path(),
        vec!["spawn-descendant".into(), marker.as_os_str().to_owned()],
    );
    let error = run_bounded(&spec, Duration::from_millis(100), 4096).unwrap_err();
    assert!(
        matches!(error, RuntimeError::Timeout { .. }),
        "unexpected timeout result: {error:?}"
    );
    thread::sleep(Duration::from_millis(750));
    assert!(
        !marker.exists(),
        "a descendant survived the timeout barrier"
    );
}

#[test]
fn cancellation_terminates_the_entire_process_tree() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("cancelled-descendant-survived");
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(
        executable,
        dir.path(),
        vec!["spawn-descendant".into(), marker.as_os_str().to_owned()],
    );
    let cancellation = CancellationToken::new();
    let canceller = cancellation.clone();
    let cancellation_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        canceller.cancel();
    });
    let error =
        run_bounded_with_cancel(&spec, Duration::from_secs(5), 4096, &cancellation).unwrap_err();
    cancellation_thread.join().unwrap();
    assert!(
        matches!(error, RuntimeError::Cancelled { .. }),
        "unexpected cancellation result: {error:?}"
    );
    thread::sleep(Duration::from_millis(750));
    assert!(
        !marker.exists(),
        "a descendant survived the cancellation barrier"
    );
}

#[test]
fn successful_parent_exit_reaps_residual_descendants_before_returning() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("orphan-survived");
    let executable = fake_executable(dir.path());
    let spec = pinned_spec(
        executable,
        dir.path(),
        vec!["orphan-descendant".into(), marker.as_os_str().to_owned()],
    );
    let started = Instant::now();
    let output = run_bounded(&spec, Duration::from_secs(5), 4096).unwrap();
    assert!(output.status.success());
    assert!(started.elapsed() < Duration::from_secs(4));
    thread::sleep(Duration::from_millis(750));
    assert!(
        !marker.exists(),
        "a residual descendant survived the reap barrier"
    );
}

#[test]
fn executable_digest_is_rechecked_at_launch() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let tool = verify_tool(&executable, ToolKind::G3mTool, None).unwrap();
    let mut spec = CommandSpec {
        kind: tool.kind,
        executable: tool.path.clone(),
        argv: Vec::new(),
        cwd: dir.path().to_owned(),
        env: BTreeMap::new(),
    };
    spec.pin_to(&tool).unwrap();
    fs::write(&spec.executable, b"replaced after verification").unwrap();
    let error = run_bounded(&spec, Duration::from_secs(5), 4096).unwrap_err();
    assert!(
        matches!(error, RuntimeError::HashMismatch { .. }),
        "unexpected digest result: {error:?}"
    );
}

#[test]
fn executable_identity_is_rechecked_at_launch() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let tool = verify_tool(&executable, ToolKind::G3mTool, None).unwrap();
    let mut spec = CommandSpec {
        kind: tool.kind,
        executable: tool.path.clone(),
        argv: Vec::new(),
        cwd: dir.path().to_owned(),
        env: BTreeMap::new(),
    };
    spec.pin_to(&tool).unwrap();
    let replacement = dir.path().join("replacement");
    fs::copy(&executable, &replacement).unwrap();
    fs::remove_file(&executable).unwrap();
    fs::rename(&replacement, &executable).unwrap();
    let error = run_bounded(&spec, Duration::from_secs(5), 4096).unwrap_err();
    assert!(matches!(error, RuntimeError::ExecutableChanged { .. }));
}

#[test]
fn unpinned_commands_fail_closed() {
    let _serial = serial_process_test();
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let mut spec = pinned_spec(executable, dir.path(), Vec::new());
    spec.env.clear();
    let error = run_bounded(&spec, Duration::from_secs(5), 4096).unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::UnpinnedExecutable { kind: "G3MTool" }
    ));
}
