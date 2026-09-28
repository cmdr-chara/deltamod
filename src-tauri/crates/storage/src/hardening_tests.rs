// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
use super::tests::tempdir;
use super::*;

#[test]
fn legacy_marker_inside_json_strings_is_not_a_suffix() {
    for value in [
        r###"{"name":"a##b","##key":["escaped\"##tail"]}"###,
        r###""##""###,
        r###"["##",42]"###,
    ] {
        let expected: Value = serde_json::from_str(value).unwrap();
        assert_eq!(
            parse_legacy_json::<Value>(value.as_bytes()).unwrap(),
            expected
        );
        assert_eq!(
            parse_legacy_json::<Value>(format!("{value} \r\n## legacy").as_bytes()).unwrap(),
            expected
        );
    }
    for text in ["", "{}{}", "{} garbage", "{}#", "##", "{\"a\":##}"] {
        assert!(
            parse_legacy_json::<Value>(text.as_bytes()).is_err(),
            "{text}"
        );
    }
}

#[test]
fn generated_legacy_json_round_trips_without_string_corruption() {
    let mut seed = 47_u64;
    for n in 0..10_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = serde_json::json!({"name": format!("{seed}##{n}\"\\\n"), "##": [n, null]});
        let encoded = serde_json::to_vec(&value).unwrap();
        assert_eq!(parse_legacy_json::<Value>(&encoded).unwrap(), value);
    }
}

struct CollisionOwner;
impl FaultIo for CollisionOwner {
    fn create_temp(&self, path: &Path) -> Result<File, StorageError> {
        fs::write(path, b"not owned by the attempted writer")?;
        Err(StorageError::TempCollision)
    }
    fn sync_file(&self, _: &File) -> Result<(), StorageError> {
        unreachable!()
    }
    fn replace(&self, _: &Path, _: &Path) -> Result<(), StorageError> {
        unreachable!()
    }
    fn sync_parent(&self, _: &Path) -> Result<(), StorageError> {
        unreachable!()
    }
}
#[test]
fn temp_collision_never_deletes_another_writers_file() {
    let dir = tempdir();
    assert!(matches!(
        atomic_write_with(&CollisionOwner, &dir.0.join("state"), b"new", false),
        Err(StorageError::TempCollision)
    ));
    let files: Vec<_> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(
        fs::read(&files[0]).unwrap(),
        b"not owned by the attempted writer"
    );
}

#[test]
fn backup_hardlinks_cannot_truncate_original_or_foreign_data() {
    for source_alias in [true, false] {
        let dir = tempdir();
        let target = dir.0.join("state.json");
        let backup = dir.0.join("state.json.backup");
        let outside = dir.0.join("original");
        fs::write(&target, b"current").unwrap();
        fs::write(&outside, b"foreign").unwrap();
        fs::hard_link(if source_alias { &target } else { &outside }, &backup).unwrap();
        assert!(atomic_write_bytes(&target, b"next", true).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"current");
        assert_eq!(fs::read(&outside).unwrap(), b"foreign");
        assert_eq!(
            fs::read(&backup).unwrap(),
            if source_alias {
                b"current".as_slice()
            } else {
                b"foreign".as_slice()
            }
        );
    }
}

#[cfg(unix)]
#[test]
fn backup_symlink_and_target_symlink_are_not_followed() {
    let dir = tempdir();
    let target = dir.0.join("state");
    let backup = dir.0.join("state.backup");
    let outside = dir.0.join("outside");
    fs::write(&target, b"current").unwrap();
    fs::write(&outside, b"preserve").unwrap();
    std::os::unix::fs::symlink(&outside, &backup).unwrap();
    assert!(atomic_write_bytes(&target, b"next", true).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"preserve");
    assert_eq!(fs::read(&target).unwrap(), b"current");
    fs::remove_file(&target).unwrap();
    std::os::unix::fs::symlink(&outside, &target).unwrap();
    assert!(atomic_write_bytes(&target, b"next", false).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"preserve");
}

struct FailedBackup;
impl FaultIo for FailedBackup {
    fn create_temp(&self, path: &Path) -> Result<File, StorageError> {
        StdFaultIo.create_temp(path)
    }
    fn sync_file(&self, file: &File) -> Result<(), StorageError> {
        StdFaultIo.sync_file(file)
    }
    fn replace(&self, from: &Path, to: &Path) -> Result<(), StorageError> {
        if to.extension().and_then(|v| v.to_str()) == Some("backup") {
            return Err(StorageError::Replacement(
                "injected backup publication failure".into(),
            ));
        }
        StdFaultIo.replace(from, to)
    }
    fn sync_parent(&self, parent: &Path) -> Result<(), StorageError> {
        StdFaultIo.sync_parent(parent)
    }
}
#[test]
fn failed_backup_publication_retains_both_prior_versions() {
    let dir = tempdir();
    let target = dir.0.join("state");
    let backup = dir.0.join("state.backup");
    fs::write(&target, b"current").unwrap();
    fs::write(&backup, b"previous").unwrap();
    assert!(atomic_write_with(&FailedBackup, &target, b"new", true).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"current");
    assert_eq!(fs::read(&backup).unwrap(), b"previous");
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
}

#[test]
fn persisted_json_reads_are_bounded_and_reject_aliases() {
    let dir = tempdir();
    let target = dir.0.join("state");
    assert!(
        matches!(load_json::<Value>(&target), Err(StorageError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound)
    );
    File::create(&target)
        .unwrap()
        .set_len(MAX_STORED_JSON_BYTES + 1)
        .unwrap();
    assert!(matches!(
        load_json::<Value>(&target),
        Err(StorageError::UnsafeFile(SecurePathError::TooLarge))
    ));
    fs::write(&target, b"{}").unwrap();
    fs::hard_link(&target, dir.0.join("alias")).unwrap();
    assert!(load_json::<Value>(&target).is_err());
}

#[cfg(unix)]
#[test]
fn new_state_and_backups_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir();
    let target = dir.0.join("state");
    atomic_write_bytes(&target, b"current", true).unwrap();
    atomic_write_bytes(&target, b"next", true).unwrap();
    for path in [target, dir.0.join("state.backup")] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
#[ignore = "opt-in comparable local tooling benchmark, not a release gate"]
fn benchmark_oversized_json_rejection() {
    use std::{hint::black_box, time::Instant};
    let dir = tempdir();
    let path = dir.0.join("oversized.json");
    let size = MAX_STORED_JSON_BYTES * 2;
    File::create(&path).unwrap().set_len(size).unwrap();
    let mut baseline = Vec::new();
    let mut candidate = Vec::new();
    for round in 0..11 {
        // Both reject the same invalid sparse record. The old implementation
        // allocated/read the entire file before parsing. Alternate trial order.
        for mode in [round % 2, 1 - round % 2] {
            let started = Instant::now();
            if mode == 0 {
                let bytes = fs::read(&path).unwrap();
                assert!(parse_legacy_json::<Value>(black_box(&bytes)).is_err());
            } else {
                assert!(load_json::<Value>(black_box(&path)).is_err());
            }
            if round > 0 {
                let sample = started.elapsed().as_secs_f64() * 1000.0;
                if mode == 0 {
                    baseline.push(sample);
                } else {
                    candidate.push(sample);
                }
            }
        }
    }
    baseline.sort_by(f64::total_cmp);
    candidate.sort_by(f64::total_cmp);
    println!(
        "BOUNDED_JSON_BENCHMARK={}",
        serde_json::json!({
            "schemaVersion": 1, "platform": std::env::consts::OS, "architecture": std::env::consts::ARCH,
            "inputBytes": size, "configuredLimitBytes": MAX_STORED_JSON_BYTES,
            "baselineMs": baseline, "candidateMs": candidate,
            "baselineMedianMs": (baseline[4]+baseline[5])/2.0,
            "candidateMedianMs": (candidate[4]+candidate[5])/2.0,
            "note": "Debug build, warm sparse invalid JSON, 10 samples each, alternating order. Not an application benchmark."
        })
    );
}

#[test]
fn relative_paths_still_support_replace_backup_and_load() {
    let name = format!(
        ".storage-relative-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = PathBuf::from(&name);
    // Own this exact file before invoking the writer, without changing the
    // process working directory shared by concurrently running tests.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let mut backup = self.0.as_os_str().to_owned();
            backup.push(".backup");
            let _ = fs::remove_file(PathBuf::from(backup));
        }
    }
    let _cleanup = Cleanup(path.clone());
    atomic_write_json(&path, &serde_json::json!({"name":"first##name"}), false).unwrap();
    atomic_write_json(&path, &serde_json::json!({"name":"next"}), true).unwrap();
    assert_eq!(load_json::<Value>(&path).unwrap()["name"], "next");
    assert_eq!(
        load_json::<Value>(&PathBuf::from(format!("{name}.backup"))).unwrap()["name"],
        "first##name"
    );
}
