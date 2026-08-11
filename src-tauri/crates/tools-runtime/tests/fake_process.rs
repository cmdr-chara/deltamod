use deltamod_tools_runtime::{run_bounded, CommandSpec, ToolKind};
use std::{
    collections::BTreeMap, ffi::OsString, fs, path::PathBuf, process::Command, time::Duration,
};

fn fake_executable(dir: &std::path::Path) -> PathBuf {
    let source = dir.join("fake.rs");
    let output = dir.join(if cfg!(windows) { "fake.exe" } else { "fake" });
    fs::write(&source, r#"
fn main() {
    println!("cwd={}", std::env::current_dir().unwrap().display());
    println!("args={:?}", std::env::args().skip(1).collect::<Vec<_>>());
    println!("marker={}", std::env::var("DELTAMOD_FAKE_MARKER").unwrap_or_default());
    if std::env::args().any(|arg| arg == "sleep") { std::thread::sleep(std::time::Duration::from_secs(30)); }
}
"#).unwrap();
    let rustc = option_env!("RUSTC").unwrap_or("rustc");
    let status = Command::new(rustc)
        .args([
            source.as_os_str(),
            OsString::from("-o").as_os_str(),
            output.as_os_str(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    output
}

#[test]
fn fake_process_observes_exact_launch_context() {
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let mut env = BTreeMap::new();
    env.insert(
        OsString::from("DELTAMOD_FAKE_MARKER"),
        OsString::from("present"),
    );
    let spec = CommandSpec {
        kind: ToolKind::G3mTool,
        executable,
        argv: vec!["one".into(), "two words".into()],
        cwd: dir.path().to_owned(),
        env,
    };
    let result = run_bounded(&spec, Duration::from_secs(5), 4096).unwrap();
    assert!(result.status.success());
    assert!(result.stdout.contains("args=[\"one\", \"two words\"]"));
    assert!(result.stdout.contains("marker=present"));
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
fn fake_process_timeout_is_reported_and_terminated() {
    let dir = tempfile::tempdir().unwrap();
    let executable = fake_executable(dir.path());
    let spec = CommandSpec {
        kind: ToolKind::G3mTool,
        executable,
        argv: vec!["sleep".into()],
        cwd: dir.path().to_owned(),
        env: BTreeMap::new(),
    };
    let result = run_bounded(&spec, Duration::from_millis(50), 4096).unwrap();
    assert!(result.timed_out);
}
