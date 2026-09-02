#![cfg(any(unix, windows))]

use deltamod_lifecycle_runtime::{
    file_plan_fingerprint, DurableLifecycleStore, ExecutionIdentity, InstallFilePlan,
    InstallMetadata, LifecycleOutcome, LifecycleWorkspace, OsLifecycleWorkspace, ReleaseARuntime,
    StagingSource, ValidatedInstallPlan, ValidatedUninstallPlan,
};
use deltamod_product_contracts::{
    fixtures::provider_ref, LifecycleOperationKind, OperationIntent, OperationRequest,
    ValidatedRelativePath,
};
use sha2::{Digest, Sha256};
use std::fs;

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn identity(lease: &str, generation: &str) -> ExecutionIdentity {
    ExecutionIdentity {
        owner_instance_id: "os-workspace-test".into(),
        lease_id: lease.into(),
        recovery_generation_id: generation.into(),
        now_ms: 1,
        lease_ttl_ms: 60_000,
    }
}

fn install_plan(
    operation: &str,
    key: &str,
    kind: LifecycleOperationKind,
    version: &str,
    source: &str,
    sha256: &str,
    size: u64,
) -> ValidatedInstallPlan {
    let files = vec![InstallFilePlan {
        path: ValidatedRelativePath::parse("mods/a.dat").unwrap(),
        path_identity_key: "mods/a.dat".into(),
        sha256: sha256.into(),
        size_bytes: size,
        expected_previous_sha256: None,
        source: StagingSource::Artifact {
            source_id: source.into(),
        },
    }];
    let provider = provider_ref();
    let request = OperationRequest::new(
        operation,
        key,
        OperationIntent {
            installation_id: "game".into(),
            kind,
            mod_instance_id: Some("active-patch-set".into()),
            provider: Some(provider.clone()),
            archive_sha256: None,
            file_plan_fingerprint: Some(file_plan_fingerprint(&files)),
            profile_id: None,
        },
    )
    .unwrap();
    ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: "active-patch-set".into(),
            mod_id: "active-patch-set".into(),
            display_name: "Active patch set".into(),
            version: Some(version.into()),
            provider,
            archive_sha256: None,
        },
        files,
    )
    .unwrap()
}

fn uninstall_plan() -> ValidatedUninstallPlan {
    ValidatedUninstallPlan::new(
        OperationRequest::new(
            "op-uninstall",
            "key-uninstall",
            OperationIntent {
                installation_id: "game".into(),
                kind: LifecycleOperationKind::Uninstall,
                mod_instance_id: Some("active-patch-set".into()),
                provider: None,
                archive_sha256: None,
                file_plan_fingerprint: None,
                profile_id: None,
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn success(outcome: LifecycleOutcome) {
    assert!(
        matches!(outcome, LifecycleOutcome::Succeeded { .. }),
        "{outcome:?}"
    );
}

#[test]
fn real_boundary_installs_updates_and_uninstalls_through_journaled_runtime() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    let workspace_root = root.path().join("workspaces");
    let store_root = root.path().join("store");
    let sources = root.path().join("sources");
    fs::create_dir_all(game.join("mods")).unwrap();
    fs::create_dir(&workspace_root).unwrap();
    fs::create_dir(&store_root).unwrap();
    fs::create_dir(&sources).unwrap();
    let v1 = sources.join("v1.bin");
    let v2 = sources.join("v2.bin");
    fs::write(&v1, b"version-one").unwrap();
    fs::write(&v2, b"version-two").unwrap();

    let mut workspace = OsLifecycleWorkspace::open(game.clone(), workspace_root).unwrap();
    workspace
        .register_artifact_source("source-v1", &v1)
        .unwrap();
    workspace
        .register_artifact_source("source-v2", &v2)
        .unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(&store_root).unwrap());

    success(runtime.install(
        install_plan(
            "op-install",
            "key-install",
            LifecycleOperationKind::Install,
            "1",
            "source-v1",
            &digest(b"version-one"),
            11,
        ),
        identity("lease-install", "generation-install"),
        &mut workspace,
    ));
    assert_eq!(fs::read(game.join("mods/a.dat")).unwrap(), b"version-one");

    success(runtime.update(
        install_plan(
            "op-update",
            "key-update",
            LifecycleOperationKind::Update,
            "2",
            "source-v2",
            &digest(b"version-two"),
            11,
        ),
        identity("lease-update", "generation-update"),
        &mut workspace,
    ));
    assert_eq!(fs::read(game.join("mods/a.dat")).unwrap(), b"version-two");

    success(runtime.uninstall(
        uninstall_plan(),
        identity("lease-uninstall", "generation-uninstall"),
        &mut workspace,
    ));
    assert!(!game.join("mods/a.dat").exists());
}

#[test]
fn real_boundary_rejects_a_link_parent_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    let outside = root.path().join("outside");
    let workspace_root = root.path().join("workspaces");
    let store_root = root.path().join("store");
    let sources = root.path().join("sources");
    fs::create_dir(&game).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::create_dir(&workspace_root).unwrap();
    fs::create_dir(&store_root).unwrap();
    fs::create_dir(&sources).unwrap();
    let source = sources.join("source.bin");
    fs::write(&source, b"content").unwrap();
    #[cfg(windows)]
    let link_result = std::os::windows::fs::symlink_dir(&outside, game.join("mods"));
    #[cfg(unix)]
    let link_result = std::os::unix::fs::symlink(&outside, game.join("mods"));
    if let Err(error) = link_result {
        #[cfg(windows)]
        if error.raw_os_error() == Some(1314) {
            return;
        }
        panic!("symlink setup failed: {error}");
    }
    let mut workspace = OsLifecycleWorkspace::open(game, workspace_root).unwrap();
    workspace
        .register_artifact_source("source", &source)
        .unwrap();
    let mut runtime = ReleaseARuntime::new(DurableLifecycleStore::open(&store_root).unwrap());
    let outcome = runtime.install(
        install_plan(
            "op-link",
            "key-link",
            LifecycleOperationKind::Install,
            "1",
            "source",
            &digest(b"content"),
            7,
        ),
        identity("lease-link", "generation-link"),
        &mut workspace,
    );
    assert!(matches!(outcome, LifecycleOutcome::Rejected { .. }));
    assert!(!outside.join("a.dat").exists());
}

#[test]
fn real_boundary_rediscovers_an_interrupted_workspace_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("game");
    let workspace_root = root.path().join("workspaces");
    fs::create_dir(&game).unwrap();
    fs::create_dir(&workspace_root).unwrap();

    let mut first = OsLifecycleWorkspace::open(game.clone(), workspace_root.clone()).unwrap();
    let transaction = first.transaction_root_identity().unwrap();
    let roots = first
        .prepare_workspace("op-restart", "generation-restart", &transaction)
        .unwrap();
    drop(first);

    let mut reopened = OsLifecycleWorkspace::open(game, workspace_root.clone()).unwrap();
    reopened
        .cleanup_workspace(&roots, "op-restart", "generation-restart", false)
        .unwrap();
    assert!(fs::read_dir(workspace_root).unwrap().next().is_none());
}
