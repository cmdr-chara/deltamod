from pathlib import Path
p=Path('src-tauri/crates/updater-launch-runtime/src/hardening_tests.rs'); s=p.read_text()
s=s.replace('tempfile::TempDir','TestDirectory').replace('tempfile::tempdir()','test_directory()')
marker='fn status(success: bool) -> ExitStatus {'
assert marker in s
s=s.replace(marker,'''struct TestDirectory(PathBuf);
impl TestDirectory {
    fn path(&self) -> &Path { &self.0 }
}
impl Drop for TestDirectory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}
fn test_directory() -> io::Result<TestDirectory> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..100 {
        let path = env::temp_dir().join(format!("deltamod-launch-hardening-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(TestDirectory(path)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("cannot allocate an isolated test directory"))
}

'''+marker)
p.write_text(s)
print('Native test directories use exclusive creation and RAII cleanup; no new dependency or Rust lockfile changes.')
