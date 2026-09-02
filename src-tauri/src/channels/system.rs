use crate::{error, state::AppState};
use deltamod_lifecycle_runtime::{
    plan_operation_history_with_sources, plan_recovery_retention, CrashSafeRetentionRuntime,
    DurableLifecycleStore, OsLifecycleWorkspace, RetentionBackend,
};
use deltamod_product_contracts::RetentionPolicy;
use deltamod_tauri_os_adapters::{validate_https_external, ValidatedFolder};
use serde_json::{json, Value};
use std::{
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

const DELTAMOD_CLI_RELEASES_URL: &str = "https://github.com/deltamodders/deltamodCLI/releases";
const UNIQUE_SYSTEM_DIRECTORY: &str = "deltamod_system-unique";
const FLAG_DATABASE_FILE: &str = "flagDB.config";
const BENCHMARK_READY_FILE: &str = ".deltamod-benchmark-ready";
const BENCHMARK_READY_ENV: &str = "DELTAMOD_BENCHMARK_READY_FILE";
const BENCHMARK_PROFILE_ENV: &str = "DELTAMOD_BENCHMARK_PROFILE";
const MODS_INSTALLATION: &str = "local-mod-library";
const PACKETS_INSTALLATION: &str = "local-packet-library";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixedPathError {
    Unavailable,
    Unsafe,
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn canonical_non_link_directory(path: &Path) -> Result<PathBuf, FixedPathError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FixedPathError::Unavailable)?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(FixedPathError::Unsafe);
    }
    fs::canonicalize(path).map_err(|_| FixedPathError::Unavailable)
}

fn validate_flag_database_candidate(
    data_root: &Path,
    candidate: &Path,
) -> Result<PathBuf, FixedPathError> {
    if candidate.file_name() != Some(OsStr::new(FLAG_DATABASE_FILE)) {
        return Err(FixedPathError::Unsafe);
    }
    let parent = candidate.parent().ok_or(FixedPathError::Unsafe)?;
    if parent.file_name() != Some(OsStr::new(UNIQUE_SYSTEM_DIRECTORY)) {
        return Err(FixedPathError::Unsafe);
    }

    let canonical_root = canonical_non_link_directory(data_root)?;
    if canonical_root != data_root {
        return Err(FixedPathError::Unsafe);
    }
    let canonical_parent = canonical_non_link_directory(parent)?;
    if canonical_parent.parent() != Some(canonical_root.as_path())
        || canonical_parent.file_name() != Some(OsStr::new(UNIQUE_SYSTEM_DIRECTORY))
    {
        return Err(FixedPathError::Unsafe);
    }

    let metadata = fs::symlink_metadata(candidate).map_err(|_| FixedPathError::Unavailable)?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(FixedPathError::Unsafe);
    }
    let canonical_file = fs::canonicalize(candidate).map_err(|_| FixedPathError::Unavailable)?;
    if canonical_file.parent() != Some(canonical_parent.as_path())
        || canonical_file.file_name() != Some(OsStr::new(FLAG_DATABASE_FILE))
        || !canonical_file.starts_with(&canonical_root)
    {
        return Err(FixedPathError::Unsafe);
    }

    // Re-check every published component after canonicalization. The opener call
    // follows this helper immediately, without renderer or asynchronous work.
    if canonical_non_link_directory(data_root)? != canonical_root
        || canonical_non_link_directory(parent)? != canonical_parent
    {
        return Err(FixedPathError::Unsafe);
    }
    let final_metadata =
        fs::symlink_metadata(candidate).map_err(|_| FixedPathError::Unavailable)?;
    if !final_metadata.is_file() || is_link_or_reparse(&final_metadata) {
        return Err(FixedPathError::Unsafe);
    }
    let final_file = fs::canonicalize(candidate).map_err(|_| FixedPathError::Unavailable)?;
    if final_file != canonical_file {
        return Err(FixedPathError::Unsafe);
    }
    Ok(final_file)
}

fn require_no_renderer_arguments(data: &[Value], channel: &'static str) -> Result<(), String> {
    if data.is_empty() {
        Ok(())
    } else {
        Err(error::invalid(channel))
    }
}

fn flag_database_path(data_root: &Path, data: &[Value]) -> Result<PathBuf, String> {
    require_no_renderer_arguments(data, "openFlagDatabase")?;
    let candidate = data_root
        .join(UNIQUE_SYSTEM_DIRECTORY)
        .join(FLAG_DATABASE_FILE);
    validate_flag_database_candidate(data_root, &candidate).map_err(|path_error| match path_error {
        FixedPathError::Unavailable => error::unavailable("openFlagDatabase"),
        FixedPathError::Unsafe => error::invalid("openFlagDatabase"),
    })
}

fn deltamod_cli_releases_url() -> Result<url::Url, String> {
    validate_https_external(DELTAMOD_CLI_RELEASES_URL, &["github.com"])
        .map_err(|_| error::invalid("installDeltamodCLI"))
}

fn mark_benchmark_renderer_ready(state: &AppState, data: &[Value]) -> Result<Value, String> {
    let status = data
        .first()
        .and_then(Value::as_object)
        .ok_or_else(|| error::invalid("benchmark:rendererReady"))?;
    if status.get("page").and_then(Value::as_str) != Some("main")
        || status.get("routeGuardCleared").and_then(Value::as_bool) != Some(true)
    {
        return Err(error::invalid("benchmark:rendererReady"));
    }
    if std::env::var_os(BENCHMARK_PROFILE_ENV).is_none() {
        return Ok(json!(false));
    }
    let Some(raw) = std::env::var_os(BENCHMARK_READY_ENV) else {
        return Ok(json!(false));
    };
    let target = PathBuf::from(raw);
    if target.file_name() != Some(OsStr::new(BENCHMARK_READY_FILE)) {
        return Err(error::invalid("benchmark:rendererReady"));
    }
    let root = canonical_non_link_directory(&state.data_root.root)
        .map_err(|_| error::unavailable("benchmark:rendererReady"))?;
    let parent = target
        .parent()
        .ok_or_else(|| error::invalid("benchmark:rendererReady"))?;
    let parent = canonical_non_link_directory(parent)
        .map_err(|_| error::invalid("benchmark:rendererReady"))?;
    if parent != root {
        return Err(error::invalid("benchmark:rendererReady"));
    }
    if target.exists() {
        return Ok(json!(true));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|_| error::internal())?;
    file.write_all(b"main\n").map_err(|_| error::internal())?;
    file.sync_all().map_err(|_| error::internal())?;
    Ok(json!(true))
}

fn open_url(app: &AppHandle, raw: &str, hosts: &[&str]) -> Result<Value, String> {
    let url = validate_https_external(raw, hosts).map_err(|_| error::invalid("open"))?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map(|_| Value::Null)
        .map_err(|_| error::internal())
}

fn open_folder(
    app: &AppHandle,
    path: &std::path::Path,
    root: &std::path::Path,
) -> Result<Value, String> {
    let folder = ValidatedFolder::from_backend(path, &[root.to_path_buf()])
        .map_err(|_| error::unavailable("openFolder"))?;
    app.opener()
        .open_path(folder.path().to_string_lossy(), None::<&str>)
        .map(|_| Value::Null)
        .map_err(|_| error::internal())
}

fn flag_name(value: &str) -> Option<String> {
    if (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b.is_ascii_digit() || b == b'_')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        Some(value.to_ascii_uppercase())
    } else {
        None
    }
}

pub fn dispatch(
    app: &AppHandle,
    state: &AppState,
    channel: &str,
    data: &[Value],
) -> Result<Option<Value>, String> {
    match channel {
        "benchmark:rendererReady" => mark_benchmark_renderer_ready(state, data).map(Some),
        "storage:getUsage" => {
            let cache_bytes = state
                .provider_cache
                .lock()
                .map_err(|_| error::internal())?
                .usage_bytes();
            let journal_bytes = directory_usage(&state.data_root.root.join("lifecycle-store"))?;
            let recovery_bytes =
                directory_usage(&state.data_root.root.join("lifecycle-workspaces"))?
                    .saturating_add(directory_usage(
                        &state.data_root.root.join("lifecycle-library-workspaces"),
                    )?);
            Ok(Some(json!({
                "cacheBytes": cache_bytes,
                "journalBytes": journal_bytes,
                "recoveryBytes": recovery_bytes,
                "totalBytes": cache_bytes.saturating_add(journal_bytes).saturating_add(recovery_bytes)
            })))
        }
        "storage:clearCache" => {
            let removed_bytes = state
                .provider_cache
                .lock()
                .map_err(|_| error::internal())?
                .clear_redownloadable()
                .map_err(|_| error::internal())?;
            Ok(Some(json!({ "removedBytes": removed_bytes })))
        }
        "storage:deleteRecoveryData" => {
            require_no_renderer_arguments(data, "storage:deleteRecoveryData")?;
            delete_recovery_data(state).map(Some)
        }
        "openCommunityMaintainerProfile" => {
            open_url(app, "https://github.com/cmdr-chara", &["github.com"]).map(Some)
        }
        "installDeltamodCLI" => {
            require_no_renderer_arguments(data, "installDeltamodCLI")?;
            let url = deltamod_cli_releases_url()?;
            app.opener()
                .open_url(url.as_str(), None::<&str>)
                .map_err(|_| error::internal())?;
            Ok(Some(json!(true)))
        }
        "openFlagDatabase" => {
            let path = flag_database_path(&state.data_root.root, data)?;
            let path = path
                .to_str()
                .ok_or_else(|| error::unavailable("openFlagDatabase"))?;
            app.opener()
                .open_path(path, None::<&str>)
                .map_err(|_| error::internal())?;
            Ok(Some(json!("")))
        }
        "getUniqueFlag" => {
            let name = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("getUniqueFlag"))?;
            let key = flag_name(name).ok_or_else(|| error::invalid("getUniqueFlag"))?;
            let prefs = state.preferences.lock().map_err(|_| error::internal())?;
            Ok(Some(json!(prefs
                .unique_flags
                .get(&key)
                .copied()
                .unwrap_or(false))))
        }
        "setUniqueFlag" => {
            let name = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid("setUniqueFlag"))?;
            let value = data
                .get(1)
                .and_then(Value::as_bool)
                .ok_or_else(|| error::invalid("setUniqueFlag"))?;
            let key = flag_name(name).ok_or_else(|| error::invalid("setUniqueFlag"))?;
            let mut prefs = state.preferences.lock().map_err(|_| error::internal())?;
            prefs.unique_flags.insert(key, value);
            state.save_preferences(&prefs)?;
            Ok(Some(json!(value)))
        }
        "isBaked" => Ok(Some(json!(false))),
        "openSysFolder" => open_folder(app, &state.data_root.root, &state.data_root.root).map(Some),
        "openModFolder" => open_folder(
            app,
            &data
                .first()
                .and_then(Value::as_str)
                .filter(|folder| {
                    !folder.is_empty()
                        && folder.len() <= 128
                        && folder.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
                        })
                })
                .map(|folder| state.data_root.root.join("packets").join(folder))
                .unwrap_or_else(|| state.data_root.root.join("packets")),
            &state.data_root.root,
        )
        .map(Some),
        _ => Ok(None),
    }
}

fn recovery_roots(state: &AppState, installation_id: &str) -> Result<(PathBuf, PathBuf), String> {
    match installation_id {
        MODS_INSTALLATION | PACKETS_INSTALLATION => {
            let library = if installation_id == MODS_INSTALLATION {
                "mods"
            } else {
                "packets"
            };
            Ok((
                state.data_root.root.join(library),
                state
                    .data_root
                    .root
                    .join("lifecycle-library-workspaces")
                    .join(installation_id),
            ))
        }
        _ => Ok((
            state.patching.game_root.clone(),
            state.data_root.root.join("lifecycle-workspaces"),
        )),
    }
}

fn delete_recovery_data(state: &AppState) -> Result<Value, String> {
    let store_root = state.data_root.root.join("lifecycle-store");
    fs::create_dir_all(&store_root).map_err(|_| error::internal())?;
    let mut store = DurableLifecycleStore::open(store_root).map_err(|_| error::internal())?;
    reconcile_pending_recovery_deletions(state, &mut store)?;
    bind_recovery_storage(state, &mut store)?;

    delete_recovery_with_policy(state, &mut store, 0)
}

pub(crate) fn enforce_storage_retention(state: &AppState) -> Result<(), String> {
    let store_root = state.data_root.root.join("lifecycle-store");
    fs::create_dir_all(&store_root).map_err(|_| error::internal())?;
    let mut store = DurableLifecycleStore::open(store_root).map_err(|_| error::internal())?;
    reconcile_pending_recovery_deletions(state, &mut store)?;
    bind_recovery_storage(state, &mut store)?;

    let policy = RetentionPolicy::default();
    delete_recovery_with_policy(state, &mut store, policy.recovery_limit_bytes)?;

    let snapshot = store.retention_snapshot().map_err(|_| error::internal())?;
    let history = plan_operation_history_with_sources(
        &snapshot.operations,
        &snapshot.active_journal_operation_ids,
        &snapshot.recovery_source_operation_ids,
        policy,
        now_ms(),
    )
    .map_err(|_| error::internal())?;
    for operation_id in history.evict_operation_ids {
        let entry = snapshot
            .operations
            .iter()
            .find(|entry| entry.operation_id == operation_id)
            .ok_or_else(error::internal)?;
        store
            .delete_operation_history(entry)
            .map_err(|_| error::internal())?;
    }
    Ok(())
}

fn reconcile_pending_recovery_deletions(
    state: &AppState,
    store: &mut DurableLifecycleStore,
) -> Result<(), String> {
    for tombstone in store
        .pending_recovery_deletions()
        .map_err(|_| error::internal())?
    {
        let (game_root, workspace_root) = recovery_roots(state, tombstone.installation_id())?;
        if !game_root.is_dir() || !workspace_root.is_dir() {
            return Err(error::unavailable("storage:recoveryRetention"));
        }
        let mut workspace =
            OsLifecycleWorkspace::open(game_root, workspace_root).map_err(|_| error::internal())?;
        CrashSafeRetentionRuntime::new(store, &mut workspace)
            .reconcile_deletion(tombstone)
            .map_err(|_| error::internal())?;
    }
    Ok(())
}

fn bind_recovery_storage(
    state: &AppState,
    store: &mut DurableLifecycleStore,
) -> Result<(), String> {
    // Bind every retained generation to its exact workspace before planning.
    // The durable store rejects changed identities, links, and reparse escapes.
    for (generation_id, operation_id, installation_id) in store
        .recovery_generation_storage_requests()
        .map_err(|_| error::internal())?
    {
        let (game_root, workspace_root) = recovery_roots(state, &installation_id)?;
        if !game_root.is_dir() {
            return Err(error::unavailable("storage:deleteRecoveryData"));
        }
        fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
        let workspace =
            OsLifecycleWorkspace::open(game_root, workspace_root).map_err(|_| error::internal())?;
        let storage = workspace
            .recovery_generation_storage(&generation_id, &operation_id)
            .map_err(|_| error::internal())?;
        store
            .bind_recovery_generation_storage(storage)
            .map_err(|_| error::internal())?;
    }
    Ok(())
}

fn delete_recovery_with_policy(
    state: &AppState,
    store: &mut DurableLifecycleStore,
    recovery_limit_bytes: u64,
) -> Result<Value, String> {
    let snapshot = store.retention_snapshot().map_err(|_| error::internal())?;
    let policy = RetentionPolicy {
        recovery_limit_bytes,
        ..RetentionPolicy::default()
    };
    let plan = plan_recovery_retention(
        &snapshot.recovery_generations,
        &snapshot.active_journal_operation_ids,
        policy,
    )
    .map_err(|_| error::internal())?;

    let mut removed_bytes = 0_u64;
    let mut removed_generation_ids = Vec::new();
    for generation_id in &plan.eviction_order {
        let entry = snapshot
            .recovery_generations
            .iter()
            .find(|entry| entry.generation.generation_id == *generation_id)
            .ok_or_else(error::internal)?;
        let (game_root, workspace_root) = recovery_roots(state, &entry.generation.installation_id)?;
        let mut workspace =
            OsLifecycleWorkspace::open(game_root, workspace_root).map_err(|_| error::internal())?;
        let mut runtime = CrashSafeRetentionRuntime::new(store, &mut workspace);
        RetentionBackend::delete_recovery_generation(&mut runtime, entry)
            .map_err(|_| error::internal())?;
        removed_bytes = removed_bytes.saturating_add(entry.generation.size_bytes);
        removed_generation_ids.push(generation_id.clone());
    }

    Ok(json!({
        "removedBytes": removed_bytes,
        "removedGenerations": removed_generation_ids.len(),
        "protectedGenerations": plan.decision.keep_generation_ids.len()
    }))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn directory_usage(root: &Path) -> Result<u64, String> {
    let Ok(root_metadata) = fs::symlink_metadata(root) else {
        return Ok(0);
    };
    if !root_metadata.is_dir() || is_link_or_reparse(&root_metadata) {
        return Err(error::internal());
    }
    let mut total = 0u64;
    let mut visited = 0usize;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| error::internal())? {
            let entry = entry.map_err(|_| error::internal())?;
            visited = visited.saturating_add(1);
            if visited > 200_000 {
                return Err(error::internal());
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|_| error::internal())?;
            if is_link_or_reparse(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::{
        deltamod_cli_releases_url, flag_database_path, flag_name, require_no_renderer_arguments,
        validate_flag_database_candidate, FixedPathError, DELTAMOD_CLI_RELEASES_URL,
        FLAG_DATABASE_FILE, UNIQUE_SYSTEM_DIRECTORY,
    };
    use serde_json::json;
    use std::{
        fs, io,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn sandbox(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "deltamod-system-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn data_root(sandbox: &Path) -> PathBuf {
        let root = sandbox.join("data");
        fs::create_dir(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    fn create_flag_database(root: &Path) -> PathBuf {
        let parent = root.join(UNIQUE_SYSTEM_DIRECTORY);
        fs::create_dir(&parent).unwrap();
        let file = parent.join(FLAG_DATABASE_FILE);
        fs::write(&file, b"AUDIO = 1\n").unwrap();
        file
    }

    #[cfg(unix)]
    fn symlink_file(source: &Path, destination: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(source, destination)
    }

    #[cfg(windows)]
    fn symlink_file(source: &Path, destination: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_file(source, destination)
    }

    #[cfg(not(any(unix, windows)))]
    fn symlink_file(_source: &Path, _destination: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "file symlinks are unavailable",
        ))
    }

    #[cfg(unix)]
    fn symlink_directory(source: &Path, destination: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(source, destination)
    }

    #[cfg(windows)]
    fn symlink_directory(source: &Path, destination: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(source, destination)
    }

    #[cfg(not(any(unix, windows)))]
    fn symlink_directory(_source: &Path, _destination: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory symlinks are unavailable",
        ))
    }

    #[cfg(unix)]
    fn remove_directory_symlink(path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    #[cfg(windows)]
    fn remove_directory_symlink(path: &Path) -> io::Result<()> {
        fs::remove_dir(path)
    }

    #[cfg(not(any(unix, windows)))]
    fn remove_directory_symlink(_path: &Path) -> io::Result<()> {
        Ok(())
    }

    #[test]
    fn legacy_flag_names_are_case_insensitive() {
        assert_eq!(flag_name("audio").as_deref(), Some("AUDIO"));
        assert_eq!(flag_name("hashchecks").as_deref(), Some("HASHCHECKS"));
        assert_eq!(flag_name("CONTROLLER").as_deref(), Some("CONTROLLER"));
        assert_eq!(flag_name("1invalid"), None);
    }

    #[test]
    fn deltamod_cli_url_is_fixed_https_and_host_validated() {
        let url = deltamod_cli_releases_url().unwrap();
        assert_eq!(url.as_str(), DELTAMOD_CLI_RELEASES_URL);
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("github.com"));
        assert_eq!(url.path(), "/deltamodders/deltamodCLI/releases");
    }

    #[test]
    fn flag_database_path_accepts_only_the_fixed_regular_file() {
        let sandbox = sandbox("valid-flag-database");
        let root = data_root(&sandbox);
        let file = create_flag_database(&root);
        assert_eq!(
            flag_database_path(&root, &[]).unwrap(),
            fs::canonicalize(&file).unwrap()
        );

        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        assert_eq!(
            validate_flag_database_candidate(&root, &file),
            Err(FixedPathError::Unsafe)
        );
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn flag_database_path_reports_a_bounded_missing_file_error() {
        let sandbox = sandbox("missing-flag-database");
        let root = data_root(&sandbox);
        fs::create_dir(root.join(UNIQUE_SYSTEM_DIRECTORY)).unwrap();
        assert_eq!(
            flag_database_path(&root, &[]),
            Err("TAURI_COMMAND_UNAVAILABLE:openFlagDatabase".into())
        );
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn flag_database_path_rejects_symlink_or_reparse_escapes_when_supported() {
        let sandbox = sandbox("flag-database-links");
        let root = data_root(&sandbox);
        let foreign = sandbox.join("foreign");
        fs::create_dir(&foreign).unwrap();
        let foreign_file = foreign.join(FLAG_DATABASE_FILE);
        fs::write(&foreign_file, b"foreign").unwrap();

        let parent = root.join(UNIQUE_SYSTEM_DIRECTORY);
        if symlink_directory(&foreign, &parent).is_ok() {
            assert_eq!(
                validate_flag_database_candidate(&root, &parent.join(FLAG_DATABASE_FILE)),
                Err(FixedPathError::Unsafe)
            );
            remove_directory_symlink(&parent).unwrap();
        }

        fs::create_dir(&parent).unwrap();
        let linked_file = parent.join(FLAG_DATABASE_FILE);
        if symlink_file(&foreign_file, &linked_file).is_ok() {
            assert_eq!(
                validate_flag_database_candidate(&root, &linked_file),
                Err(FixedPathError::Unsafe)
            );
            fs::remove_file(&linked_file).unwrap();
        }
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn flag_database_path_rejects_foreign_candidates_and_replaced_roots() {
        let sandbox = sandbox("flag-database-root-replacement");
        let root = data_root(&sandbox);
        create_flag_database(&root);
        let foreign_root = sandbox.join("foreign-root");
        fs::create_dir(&foreign_root).unwrap();
        let foreign_file = create_flag_database(&foreign_root);
        assert_eq!(
            validate_flag_database_candidate(&root, &foreign_file),
            Err(FixedPathError::Unsafe)
        );

        let displaced_root = sandbox.join("displaced-root");
        fs::rename(&root, &displaced_root).unwrap();
        assert_eq!(
            flag_database_path(&root, &[]),
            Err("TAURI_COMMAND_UNAVAILABLE:openFlagDatabase".into())
        );
        if symlink_directory(&foreign_root, &root).is_ok() {
            assert_eq!(
                flag_database_path(&root, &[]),
                Err("TAURI_INVALID_PAYLOAD:openFlagDatabase".into())
            );
            remove_directory_symlink(&root).unwrap();
        }
        if !root.exists() {
            fs::rename(&displaced_root, &root).unwrap();
        }
        fs::remove_dir_all(sandbox).unwrap();
    }

    #[test]
    fn legacy_openers_reject_renderer_controlled_arguments() {
        let renderer_path = json!("C:/foreign/flagDB.config");
        assert_eq!(
            flag_database_path(
                Path::new("renderer-data-root-must-not-be-read"),
                std::slice::from_ref(&renderer_path)
            ),
            Err("TAURI_INVALID_PAYLOAD:openFlagDatabase".into())
        );
        assert_eq!(
            require_no_renderer_arguments(std::slice::from_ref(&renderer_path), "openFlagDatabase"),
            Err("TAURI_INVALID_PAYLOAD:openFlagDatabase".into())
        );
        assert_eq!(
            require_no_renderer_arguments(&[renderer_path], "installDeltamodCLI"),
            Err("TAURI_INVALID_PAYLOAD:installDeltamodCLI".into())
        );
    }
}
