// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
use super::*;
use tempfile::tempdir;

fn copy_journal(runtime: &Runtime, id: u64) -> (PathBuf, Journal) {
    let destination = runtime
        .root
        .join("installations")
        .join(format!("install-test-{id}"));
    let target = runtime
        .root
        .join(".runtime-replacements")
        .join(id.to_string());
    (
        runtime
            .root
            .join(".runtime-journals")
            .join(format!("{id}.json")),
        Journal {
            version: 1,
            operation_id: id,
            kind: "installation-reimport".into(),
            source: Some("not used for recovery".into()),
            destination: Some(destination.to_str().unwrap().into()),
            staging: Some(
                target
                    .with_extension(format!("importing-{id}"))
                    .to_str()
                    .unwrap()
                    .into(),
            ),
            replacement: Some(target.to_str().unwrap().into()),
            backup: Some(
                destination
                    .with_extension(format!("deltamod-replacing-{id}"))
                    .to_str()
                    .unwrap()
                    .into(),
            ),
            status: JournalStatus::Prepared,
        },
    )
}
fn store(path: &Path, journal: &Journal) {
    fs::write(path, serde_json::to_vec(journal).unwrap()).unwrap();
}
fn directory_with_data(path: &Path, data: &[u8]) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("data.win"), data).unwrap();
}

#[test]
fn validates_all_journals_before_cleaning_any_staging() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path()).unwrap();
    let (good_path, good) = copy_journal(&runtime, 1);
    let stage = Path::new(good.staging.as_ref().unwrap());
    directory_with_data(stage, b"partial");
    store(&good_path, &good);
    let (bad_path, mut bad) = copy_journal(&runtime, 2);
    bad.version = 999;
    store(&bad_path, &bad);
    assert!(Runtime::open(dir.path()).is_err());
    assert_eq!(fs::read(stage.join("data.win")).unwrap(), b"partial");
    assert!(good_path.exists() && bad_path.exists());
}

#[test]
fn rejects_forged_paths_kinds_and_operation_identities() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    let outside = dir.path().join("outside");
    directory_with_data(&outside, b"original");
    let (path, original) = copy_journal(&runtime, 1);
    let attacks = [
        runtime.root.clone(),
        outside.clone(),
        runtime.root.join("..").join("outside"),
        runtime.root.join("profiles"),
        runtime.root.join(".runtime-replacements/2"),
        runtime.root.join(".runtime-replacements/./1"),
    ];
    for attack in attacks {
        for field in ["destination", "staging", "replacement", "backup"] {
            let mut value = serde_json::to_value(&original).unwrap();
            value[field] = json!(attack);
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(
                Runtime::open(&runtime.root).is_err(),
                "{field}: {}",
                attack.display()
            );
            assert!(path.exists());
            assert_eq!(fs::read(outside.join("data.win")).unwrap(), b"original");
        }
    }
    for (field, value) in [
        ("kind", json!("cleanup-anything")),
        ("operation_id", json!(2)),
        ("operation_id", json!(0)),
        ("version", json!(2)),
        ("unexpected", json!(true)),
    ] {
        let mut value_with_error = serde_json::to_value(&original).unwrap();
        value_with_error[field] = value;
        fs::write(&path, serde_json::to_vec(&value_with_error).unwrap()).unwrap();
        assert!(Runtime::open(&runtime.root).is_err());
    }
}

#[test]
fn recovery_restores_the_only_backup_before_cleaning_replacement() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path()).unwrap();
    let (path, journal) = copy_journal(&runtime, 4);
    directory_with_data(Path::new(journal.backup.as_ref().unwrap()), b"original");
    directory_with_data(Path::new(journal.replacement.as_ref().unwrap()), b"new");
    store(&path, &journal);
    let reopened = Runtime::open(dir.path()).unwrap();
    assert_eq!(
        fs::read(Path::new(journal.destination.as_ref().unwrap()).join("data.win")).unwrap(),
        b"original"
    );
    assert!(!path.exists());
    assert!(!Path::new(journal.replacement.as_ref().unwrap()).exists());
    assert_eq!(reopened.next_operation_id().unwrap(), 5);
}

#[test]
fn ambiguous_backups_survive_even_a_committed_v1_journal() {
    for status in [JournalStatus::Prepared, JournalStatus::Committed] {
        let dir = tempdir().unwrap();
        let runtime = Runtime::open(dir.path()).unwrap();
        let (path, mut journal) = copy_journal(&runtime, 1);
        journal.status = status;
        directory_with_data(Path::new(journal.backup.as_ref().unwrap()), b"original");
        directory_with_data(
            Path::new(journal.destination.as_ref().unwrap()),
            b"externally changed",
        );
        store(&path, &journal);
        assert!(Runtime::open(dir.path()).is_err());
        assert!(path.exists());
        assert_eq!(
            fs::read(Path::new(journal.backup.as_ref().unwrap()).join("data.win")).unwrap(),
            b"original"
        );
        assert_eq!(
            fs::read(Path::new(journal.destination.as_ref().unwrap()).join("data.win")).unwrap(),
            b"externally changed"
        );
    }
}

#[test]
fn oversized_and_duplicate_field_journals_are_retained() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path()).unwrap();
    let (path, journal) = copy_journal(&runtime, 1);
    let text = serde_json::to_string(&journal).unwrap();
    for bytes in [
        vec![b' '; MAX_JOURNAL_BYTES as usize + 1],
        text.replacen('{', "{\"version\":1,", 1).into_bytes(),
    ] {
        fs::write(&path, &bytes).unwrap();
        assert!(Runtime::open(dir.path()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn linked_journal_cannot_read_or_delete_its_target() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    let (path, journal) = copy_journal(&runtime, 1);
    let source = dir.path().join("outside.json");
    store(&source, &journal);
    fs::hard_link(&source, &path).unwrap();
    assert!(Runtime::open(&runtime.root).is_err());
    assert!(path.exists() && source.exists());
}

#[test]
fn directory_links_are_rejected_including_ancestors() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    let (path, journal) = copy_journal(&runtime, 1);
    let outside = dir.path().join("outside");
    directory_with_data(&outside, b"original");
    super::super::tests::create_directory_link(
        &outside,
        &runtime.root.join(".runtime-replacements"),
    );
    store(&path, &journal);
    assert!(Runtime::open(&runtime.root).is_err());
    assert_eq!(fs::read(outside.join("data.win")).unwrap(), b"original");
}

#[test]
fn one_kernel_lease_excludes_clones_and_independent_runtime_recovery() {
    let dir = tempdir().unwrap();
    let runtime = Runtime::open(dir.path()).unwrap();
    let independent = Runtime::open(dir.path()).unwrap();
    let lease = runtime.mutation_lease().unwrap();
    assert!(runtime.clone().mutation_lease().is_err());
    assert!(independent.mutation_lease().is_err());
    assert!(Runtime::open(dir.path()).is_err());
    lease.verify().unwrap();
    drop(lease);
    assert!(independent.mutation_lease().is_ok());
    assert!(Runtime::open(dir.path()).is_ok());
}

#[test]
fn metadata_failure_cannot_delete_the_previous_working_copy() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    directory_with_data(&source, b"original");
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    runtime
        .create_installation(
            &source,
            "game".into(),
            GamePlatform::Windows,
            Ownership::ManagedCopy,
        )
        .unwrap();
    let installation = runtime.state().unwrap().installations.remove(0);
    fs::write(source.join("data.win"), b"replacement").unwrap();
    fs::create_dir(runtime.root.join("installations-adapter.json.backup")).unwrap();
    assert!(runtime.reimport_installation(&installation.id).is_err());
    assert_eq!(
        fs::read(installation.install_path.join("data.win")).unwrap(),
        b"replacement"
    );
    let backup = installation
        .install_path
        .with_extension("deltamod-replacing-2");
    assert_eq!(fs::read(backup.join("data.win")).unwrap(), b"original");
    assert!(runtime.root.join(".runtime-journals/2.json").exists());
    assert_eq!(
        runtime
            .operations
            .lock()
            .unwrap()
            .entries
            .get(&2)
            .unwrap()
            .state,
        OperationState::Failed
    );
    assert!(runtime.events.lock().unwrap().iter().any(|event| matches!(
        event,
        ProgressEvent::Finished {
            operation_id: 2,
            success: false,
            ..
        }
    )));
    assert!(Runtime::open(&runtime.root).is_err());
}

#[test]
fn delete_quarantine_rolls_back_when_metadata_write_fails() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    directory_with_data(&source, b"original");
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    runtime
        .create_installation(
            &source,
            "game".into(),
            GamePlatform::Windows,
            Ownership::ManagedCopy,
        )
        .unwrap();
    let installation = runtime.state().unwrap().installations.remove(0);
    fs::create_dir(runtime.root.join("installations-adapter.json.backup")).unwrap();
    assert!(runtime.delete_installation(&installation.id, true).is_err());
    assert_eq!(
        fs::read(installation.install_path.join("data.win")).unwrap(),
        b"original"
    );
    assert_eq!(runtime.state().unwrap().installations.len(), 1);
    assert!(Runtime::open(&runtime.root).is_ok());
}

#[test]
fn linked_installations_cannot_be_reimported_in_place() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    directory_with_data(&source, b"original");
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    runtime
        .create_installation(
            &source,
            "game".into(),
            GamePlatform::Windows,
            Ownership::LinkedExternal,
        )
        .unwrap();
    let installation = runtime.state().unwrap().installations.remove(0);
    assert!(runtime.reimport_installation(&installation.id).is_err());
    assert_eq!(fs::read(source.join("data.win")).unwrap(), b"original");
}

#[test]
fn linked_installation_copies_from_its_actual_external_path() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("source");
    directory_with_data(&source, b"original");
    let runtime = Runtime::open(dir.path().join("data")).unwrap();
    runtime
        .create_installation(
            &source,
            "linked".into(),
            GamePlatform::Windows,
            Ownership::LinkedExternal,
        )
        .unwrap();
    let installation = runtime.state().unwrap().installations.remove(0);
    assert_eq!(
        installation.install_path,
        fs::canonicalize(&source).unwrap()
    );
    runtime
        .copy_installation(&installation.id, "managed".into())
        .unwrap();
    let state = runtime.state().unwrap();
    let managed = state
        .installations
        .iter()
        .find(|i| i.ownership == Ownership::ManagedCopy)
        .unwrap();
    assert_eq!(
        fs::read(managed.install_path.join("data.win")).unwrap(),
        b"original"
    );
    runtime.delete_installation(&installation.id, true).unwrap();
    assert_eq!(fs::read(source.join("data.win")).unwrap(), b"original");
}
