use crate::{error, state::AppState};
use serde_json::{json, Value};

use deltamod_archive_import_runtime::{stage_mod_archive, Limits as ArchiveLimits};
use deltamod_lifecycle_runtime::{
    file_plan_fingerprint, CachedArchiveSource, DurableLifecycleStore, ExecutionIdentity,
    InstallFilePlan, InstallMetadata, LifecycleOutcome, LockedProfileMod, OsLifecycleWorkspace,
    ProfileDefinition, ProfileLockfile, ProfileModDefinition, ProviderArtifactSource,
    ReleaseARuntime, RepairPlanDisposition, RepairSourceCatalog, StagingSource,
    StartupRecoveryOutcome, ValidatedInstallPlan, ValidatedUninstallPlan, VerificationScope,
};
use deltamod_product_contracts::{
    LifecycleOperationKind, OperationIntent, OperationPhase, OperationRequest,
    ProviderArtifactKind, ProviderId, ProviderItemKind, ProviderRef, ProviderResourceId,
    ValidatedRelativePath,
};
use deltamod_tauri_os_adapters::DialogBackend;
use deltamod_tauri_os_adapters::{validate_dialog_selection, DialogFilter, DialogRequest};
use deltamod_tools_runtime::{copy_relative_regular_file_verified, inspect_regular_file};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MODS_INSTALLATION: &str = "local-mod-library";
const PACKETS_INSTALLATION: &str = "local-packet-library";
const MAX_PACKAGE_FILES: usize = 20_000;
const MAX_PACKAGE_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const ARTIFACT_CACHE_DIRECTORY: &str = "lifecycle-artifact-cache";

pub(crate) fn recover_startup(state: &AppState) -> Result<usize, String> {
    let mut runtime = ReleaseARuntime::new(open_store(state)?);
    let mut recovered = 0;
    for installation_id in [MODS_INSTALLATION, PACKETS_INSTALLATION] {
        let root = library_root(state, installation_id)?;
        let workspace_root = library_workspace_root(state, installation_id);
        fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
        let mut workspace =
            OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
        let outcomes = runtime.recover_startup_installation(
            &format!("tauri-startup-{}", std::process::id()),
            installation_id,
            now_ms(),
            5 * 60 * 1_000,
            |operation| {
                stable_id(
                    "startup-lease",
                    &format!("{installation_id}:{}", operation.request.operation_id()),
                )
            },
            &mut workspace,
        );
        for outcome in outcomes {
            match outcome {
                StartupRecoveryOutcome::Recovered { .. } => recovered += 1,
                StartupRecoveryOutcome::Active { .. } => {}
                StartupRecoveryOutcome::Blocked { .. }
                | StartupRecoveryOutcome::StoreBlocked { .. } => {
                    return Err("LIFECYCLE_STARTUP_RECOVERY_BLOCKED".into());
                }
            }
        }
    }
    Ok(recovered)
}

pub fn dispatch(state: &AppState, channel: &str, data: &[Value]) -> Result<Option<Value>, String> {
    if !channel.starts_with("lifecycle:") {
        return Ok(None);
    }
    match channel {
        "lifecycle:listProfiles" => {
            let profiles = state
                .profile_registry
                .lock()
                .map_err(|_| error::internal())?
                .list();
            return serde_json::to_value(json!({ "profiles": profiles }))
                .map(Some)
                .map_err(|_| error::internal());
        }
        "lifecycle:importProfileLockfile" => {
            let canonical_json = data
                .first()
                .and_then(Value::as_str)
                .ok_or_else(|| error::invalid(channel))?;
            let profile = state
                .profile_registry
                .lock()
                .map_err(|_| error::internal())?
                .import(canonical_json.as_bytes())
                .map_err(|_| error::invalid(channel))?;
            return serde_json::to_value(json!({ "profile": profile }))
                .map(Some)
                .map_err(|_| error::internal());
        }
        "lifecycle:exportProfileLockfile" => {
            let profile_id = data
                .first()
                .and_then(Value::as_str)
                .filter(|value| valid_id(value, 128))
                .ok_or_else(|| error::invalid(channel))?;
            let profile = state
                .profile_registry
                .lock()
                .map_err(|_| error::internal())?
                .get(profile_id)
                .ok_or_else(|| error::invalid(channel))?;
            let canonical_json = profile.to_canonical_json().map_err(|_| error::internal())?;
            return Ok(Some(json!({
                "canonicalJson": canonical_json,
                "fileName": format!("{profile_id}.deltamod-profile.json"),
                "profile": profile
            })));
        }
        "lifecycle:getActiveProfile" => {
            let installation_id = data
                .first()
                .and_then(Value::as_str)
                .filter(|value| valid_id(value, 256))
                .ok_or_else(|| error::invalid(channel))?;
            let active = open_store(state)?
                .active_profile(installation_id)
                .map_err(|_| error::internal())?;
            return serde_json::to_value(json!({ "activeProfile": active }))
                .map(Some)
                .map_err(|_| error::internal());
        }
        _ => {}
    }
    match channel {
        "lifecycle:getInstalledMods" => authoritative_catalog(state).map(Some),
        "lifecycle:createProfileFromCurrent" => create_profile_from_current(state, data).map(Some),
        "lifecycle:switchProfile" => switch_profile(state, data).map(Some),
        "lifecycle:getOperationStatus" => {
            let operation_id = data
                .first()
                .and_then(Value::as_str)
                .filter(|value| valid_id(value, 128))
                .ok_or_else(|| error::invalid(channel))?;
            operation_status(state, operation_id).map(Some)
        }
        "lifecycle:verifyMod" => {
            let (installation_id, instance_id) = lifecycle_target(data, channel)?;
            verify_mod(state, &installation_id, &instance_id).map(Some)
        }
        "lifecycle:repairMod" => {
            let (installation_id, instance_id) = lifecycle_target(data, channel)?;
            let operation_id = data
                .get(2)
                .and_then(Value::as_str)
                .filter(|value| valid_id(value, 128))
                .ok_or_else(|| error::invalid(channel))?;
            repair_mod(state, &installation_id, &instance_id, operation_id).map(Some)
        }
        "lifecycle:restoreLastWorkingState" => {
            let installation_id = data
                .first()
                .and_then(Value::as_str)
                .filter(|value| matches!(*value, MODS_INSTALLATION | PACKETS_INSTALLATION))
                .ok_or_else(|| error::invalid(channel))?;
            let operation_id = data
                .get(1)
                .and_then(Value::as_str)
                .filter(|value| valid_id(value, 128))
                .ok_or_else(|| error::invalid(channel))?;
            restore_last_working_state(state, installation_id, operation_id).map(Some)
        }
        "lifecycle:uninstallMod" => {
            let (installation_id, instance_id) = lifecycle_target(data, channel)?;
            let operation_id = data
                .get(2)
                .and_then(Value::as_str)
                .filter(|value| valid_id(value, 128))
                .ok_or_else(|| error::invalid(channel))?;
            uninstall_mod(state, &installation_id, &instance_id, operation_id).map(Some)
        }
        _ => Ok(None),
    }
}

fn create_profile_from_current(state: &AppState, data: &[Value]) -> Result<Value, String> {
    let profile_id = data
        .first()
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 128))
        .ok_or_else(|| error::invalid("lifecycle:createProfileFromCurrent"))?;
    let installation_id = data
        .get(1)
        .and_then(Value::as_str)
        .unwrap_or(PACKETS_INSTALLATION);
    if !matches!(installation_id, MODS_INSTALLATION | PACKETS_INSTALLATION) {
        return Err(error::invalid("lifecycle:createProfileFromCurrent"));
    }
    let manifest = open_store(state)?
        .manifest(installation_id)
        .map_err(|_| error::internal())?
        .ok_or_else(|| "PROFILE_CAPTURE_UNAVAILABLE:empty_installation".to_owned())?;
    let game = state
        .game
        .dispatch("getCurrentGameInfo", &[])
        .map_err(|_| error::internal())?
        .ok_or_else(|| "PROFILE_CAPTURE_UNAVAILABLE:game_unknown".to_owned())?;
    let game_id = game
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 256))
        .ok_or_else(|| "PROFILE_CAPTURE_UNAVAILABLE:game_unknown".to_owned())?;
    let mut definitions = Vec::with_capacity(manifest.records.len());
    let mut locked = Vec::with_capacity(manifest.records.len());
    for (index, record) in manifest.records.iter().enumerate() {
        let order = u32::try_from(index)
            .map_err(|_| "PROFILE_CAPTURE_UNAVAILABLE:too_many_mods".to_owned())?;
        let archive_sha256 = record.archive_sha256.clone().ok_or_else(|| {
            format!(
                "PROFILE_CAPTURE_UNAVAILABLE:archive_hash_missing:{}",
                record.instance_id
            )
        })?;
        definitions.push(ProfileModDefinition {
            order,
            instance_id: record.instance_id.clone(),
            mod_id: record.mod_id.clone(),
            display_name: record.display_name.clone(),
            provider: record.provider.clone(),
            configuration_fingerprint: None,
        });
        locked.push(LockedProfileMod {
            order,
            instance_id: record.instance_id.clone(),
            mod_id: record.mod_id.clone(),
            display_name: record.display_name.clone(),
            version: record.version.clone(),
            provider: record.provider.clone(),
            archive_sha256,
            file_plan_fingerprint: record.file_plan_fingerprint.clone(),
            configuration_fingerprint: None,
        });
    }
    let definition = ProfileDefinition::new(profile_id, game_id, installation_id, definitions)
        .map_err(|_| "PROFILE_CAPTURE_UNAVAILABLE:invalid_manifest".to_owned())?;
    let profile = ProfileLockfile::new(&definition, locked)
        .map_err(|_| "PROFILE_CAPTURE_UNAVAILABLE:invalid_manifest".to_owned())?;
    state
        .profile_registry
        .lock()
        .map_err(|_| error::internal())?
        .save(&profile)
        .map_err(|_| error::internal())?;
    Ok(json!({ "profile": profile }))
}

fn switch_profile(state: &AppState, data: &[Value]) -> Result<Value, String> {
    let profile_id = data
        .first()
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 128))
        .ok_or_else(|| error::invalid("lifecycle:switchProfile"))?;
    let operation_id = data
        .get(1)
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 128))
        .ok_or_else(|| error::invalid("lifecycle:switchProfile"))?;
    let target = state
        .profile_registry
        .lock()
        .map_err(|_| error::internal())?
        .get(profile_id)
        .ok_or_else(|| error::invalid("lifecycle:switchProfile"))?;
    if !matches!(
        target.installation_id.as_str(),
        MODS_INSTALLATION | PACKETS_INSTALLATION
    ) {
        return Err(error::invalid("lifecycle:switchProfile"));
    }
    let store = open_store(state)?;
    let current = store
        .manifest(&target.installation_id)
        .map_err(|_| error::internal())?;
    let previous = store
        .active_profile(&target.installation_id)
        .map_err(|_| error::internal())?
        .and_then(|pointer| {
            state
                .profile_registry
                .lock()
                .ok()?
                .get(pointer.profile_id())
                .filter(|profile| {
                    profile.fingerprint().is_ok_and(|fingerprint| {
                        fingerprint.eq_ignore_ascii_case(pointer.lock_fingerprint())
                    })
                })
        });
    let plan = deltamod_lifecycle_runtime::ProfileSwitchPlan::build(
        current.as_ref(),
        previous.as_ref(),
        &target,
    )
    .map_err(|_| error::invalid("lifecycle:switchProfile"))?;
    let mut resolved = Vec::new();
    let root = library_root(state, &target.installation_id)?;
    let workspace_root = library_workspace_root(state, &target.installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let mut workspace =
        OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
    let mut registered_sources = BTreeSet::new();
    for operation in plan.operations() {
        if operation.kind() == LifecycleOperationKind::Uninstall {
            continue;
        }
        let locked = target
            .mods
            .iter()
            .find(|item| item.instance_id == operation.instance_id())
            .ok_or_else(error::internal)?;
        let snapshot = cached_snapshot(state, &locked.archive_sha256)
            .map_err(|_| "PROFILE_SOURCE_UNAVAILABLE:exact_archive".to_owned())?;
        let entries = package_files(&snapshot.root, &snapshot.root)?;
        if entries.is_empty() {
            return Err("PROFILE_SOURCE_UNAVAILABLE:empty_archive".into());
        }
        if registered_sources.insert(snapshot.source_id.clone()) {
            workspace
                .register_artifact_tree_source(&snapshot.source_id, &snapshot.root)
                .map_err(|_| error::internal())?;
        }
        let files = entries
            .into_iter()
            .map(|entry| InstallFilePlan {
                path_identity_key: entry.relative.as_str().to_ascii_lowercase(),
                expected_previous_sha256: None,
                path: entry.relative.clone(),
                sha256: entry.sha256,
                size_bytes: entry.size,
                source: StagingSource::ArtifactTree {
                    source_id: snapshot.source_id.clone(),
                    source_path: entry.relative,
                },
            })
            .collect::<Vec<_>>();
        if !file_plan_fingerprint(&files).eq_ignore_ascii_case(&locked.file_plan_fingerprint) {
            return Err("PROFILE_SOURCE_UNAVAILABLE:file_plan_mismatch".into());
        }
        let child_intent = OperationIntent {
            installation_id: target.installation_id.clone(),
            kind: operation.kind(),
            mod_instance_id: Some(locked.instance_id.clone()),
            provider: Some(locked.provider.clone()),
            archive_sha256: Some(locked.archive_sha256.clone()),
            file_plan_fingerprint: Some(locked.file_plan_fingerprint.clone()),
            profile_id: Some(target.profile_id.clone()),
        };
        let child_request = OperationRequest::new(
            operation.operation_id(),
            operation.idempotency_key(),
            child_intent,
        )
        .map_err(|_| error::internal())?;
        resolved.push(
            ValidatedInstallPlan::new(
                child_request,
                InstallMetadata {
                    instance_id: locked.instance_id.clone(),
                    mod_id: locked.mod_id.clone(),
                    display_name: locked.display_name.clone(),
                    version: locked.version.clone(),
                    provider: locked.provider.clone(),
                    archive_sha256: Some(locked.archive_sha256.clone()),
                },
                files,
            )
            .map_err(|_| error::internal())?,
        );
    }
    let request = OperationRequest::new(
        operation_id,
        operation_id,
        OperationIntent {
            installation_id: target.installation_id.clone(),
            kind: LifecycleOperationKind::ProfileSwitch,
            mod_instance_id: None,
            provider: None,
            archive_sha256: None,
            file_plan_fingerprint: Some(plan.fingerprint().to_owned()),
            profile_id: Some(target.profile_id.clone()),
        },
    )
    .map_err(|_| error::invalid("lifecycle:switchProfile"))?;
    let mut runtime = ReleaseARuntime::new(store);
    require_success(runtime.switch_profile(
        request,
        plan,
        target.clone(),
        resolved,
        identity(operation_id),
        &mut workspace,
    ))?;
    Ok(json!({ "ok": true, "operationId": operation_id, "profile": target }))
}

pub(crate) fn update_mod<D: DialogBackend>(
    state: &AppState,
    dialogs: &D,
    data: &[Value],
) -> Result<Value, String> {
    let (installation_id, instance_id) = lifecycle_target(data, "lifecycle:updateMod")?;
    if installation_id != PACKETS_INSTALLATION {
        return Err(error::invalid("lifecycle:updateMod"));
    }
    let operation_id = data
        .get(2)
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 128))
        .ok_or_else(|| error::invalid("lifecycle:updateMod"))?;
    let request = DialogRequest::file("Choose the exact replacement mod archive").filter(
        DialogFilter::new("Deltamod compatible archive", ["zip", "7z", "gz", "lzma"])
            .map_err(|_| error::internal())?,
    );
    let Some(selected) = dialogs.pick(&request).map_err(|_| error::internal())? else {
        return Ok(json!({"ok": false, "cancelled": true}));
    };
    let selected = validate_dialog_selection(&request, selected)
        .map_err(|_| error::invalid("lifecycle:updateMod"))?;
    execute_archive_update(
        state,
        &installation_id,
        &instance_id,
        operation_id,
        &selected,
    )
}

fn lifecycle_target(data: &[Value], channel: &str) -> Result<(String, String), String> {
    let installation_id = data
        .first()
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, MODS_INSTALLATION | PACKETS_INSTALLATION))
        .ok_or_else(|| error::invalid(channel))?;
    let instance_id = data
        .get(1)
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 256))
        .ok_or_else(|| error::invalid(channel))?;
    Ok((installation_id.into(), instance_id.into()))
}

fn authoritative_catalog(state: &AppState) -> Result<Value, String> {
    let source = super::runtime::mod_list(state, "lifecycle:getInstalledMods")?;
    let records = source.as_array().ok_or_else(error::internal)?;
    let mut errors = Vec::new();
    errors.extend(
        state
            .startup_recovery_errors
            .lock()
            .map_err(|_| error::internal())?
            .iter()
            .map(|code| json!({"code": code})),
    );
    let mut folders = BTreeMap::new();
    for record in records {
        match adopt_record(state, record) {
            Ok(Some((instance_id, folder))) => {
                folders.insert(instance_id, folder);
            }
            Ok(None) => {}
            Err(_) => errors.push(json!({"code": "legacy_adoption_failed"})),
        }
    }

    let store = open_store(state)?;
    let mut installed = Vec::new();
    let mut verification = Vec::new();
    let mut health = Vec::new();
    for installation_id in [MODS_INSTALLATION, PACKETS_INSTALLATION] {
        let Some(manifest) = store
            .manifest(installation_id)
            .map_err(|_| error::internal())?
        else {
            continue;
        };
        let root = library_root(state, installation_id)?;
        let workspace_root = library_workspace_root(state, installation_id);
        fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
        let workspace =
            OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
        let runtime = ReleaseARuntime::new(open_store(state)?);
        let installation_scope =
            VerificationScope::new(installation_id, None).map_err(|_| error::internal())?;
        let installation_report = runtime
            .verify_installation(&installation_scope, &workspace)
            .map_err(|failure| failure.error.code.as_str().to_owned())?;
        health.push(
            serde_json::to_value(&installation_report.health).map_err(|_| error::internal())?,
        );
        for record in manifest.records {
            let scope = VerificationScope::new(installation_id, Some(record.instance_id.clone()))
                .map_err(|_| error::internal())?;
            let report = runtime
                .verify_installation(&scope, &workspace)
                .map_err(|failure| failure.error.code.as_str().to_owned())?;
            verification
                .push(serde_json::to_value(&report.verification).map_err(|_| error::internal())?);
            installed.push(serde_json::to_value(&record).map_err(|_| error::internal())?);
        }
    }
    let operation_records = store
        .operations()
        .map_err(|_| error::internal())?
        .into_iter()
        .take(100)
        .map(|record| serde_json::to_value(record).map_err(|_| error::internal()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut completed_installations = BTreeSet::new();
    let lifecycle_journals = store
        .journals()
        .map_err(|_| error::internal())?
        .into_iter()
        .filter(|journal| {
            journal.phase != OperationPhase::Complete
                || (journal.operation != LifecycleOperationKind::Recover
                    && completed_installations.insert(journal.installation_id.clone()))
        })
        .take(100)
        .map(|journal| serde_json::to_value(journal).map_err(|_| error::internal()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "installedMods": installed,
        "verificationResults": verification,
        "gameHealthReports": health,
        "conflictReports": [],
        "operationRecords": operation_records,
        "lifecycleJournals": lifecycle_journals,
        "foldersByInstanceId": folders,
        "errors": errors,
        "runtime": {"shell": "tauri", "platform": std::env::consts::OS}
    }))
}

fn adopt_record(state: &AppState, record: &Value) -> Result<Option<(String, String)>, String> {
    let uid = record
        .get("uid")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .ok_or_else(error::internal)?;
    let folder = record
        .get("folder")
        .and_then(Value::as_str)
        .filter(|value| safe_segment(value))
        .ok_or_else(error::internal)?;
    let Some((installation_id, root, package_root)) = resolve_package_root(state, folder, uid)?
    else {
        return Ok(None);
    };
    let instance_id = contract_id("mod", uid, 256);
    let store = open_store(state)?;
    let existing = store
        .manifest(installation_id)
        .map_err(|_| error::internal())?
        .and_then(|manifest| {
            manifest
                .records
                .into_iter()
                .find(|installed| installed.instance_id == instance_id)
        });

    let workspace_root = library_workspace_root(state, installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let mut workspace =
        OsLifecycleWorkspace::open(root.clone(), workspace_root).map_err(|_| error::internal())?;
    let entries = package_files(&root, &package_root)?;
    if entries.is_empty() {
        return Err(error::internal());
    }
    let snapshot = ensure_package_snapshot(state, &root, &entries)?;
    workspace
        .register_artifact_tree_source(&snapshot.source_id, &snapshot.root)
        .map_err(|_| error::internal())?;
    let mut files = Vec::with_capacity(entries.len());
    for entry in entries {
        files.push(InstallFilePlan {
            path: entry.relative.clone(),
            path_identity_key: entry.relative.as_str().to_ascii_lowercase(),
            sha256: entry.sha256.clone(),
            size_bytes: entry.size,
            expected_previous_sha256: Some(entry.sha256),
            source: StagingSource::ArtifactTree {
                source_id: snapshot.source_id.clone(),
                source_path: entry.relative.clone(),
            },
        });
    }
    if let Some(installed) = existing {
        if installed.archive_sha256.as_deref() == Some(snapshot.sha256.as_str()) {
            return Ok(Some((instance_id, folder.into())));
        }
        if installed.archive_sha256.is_some() {
            return Err(error::internal());
        }
        for file in &mut files {
            file.expected_previous_sha256 = None;
        }
        let operation_id = stable_id(
            "adopt-source",
            &format!("{installation_id}\0{instance_id}\0{}", snapshot.sha256),
        );
        let request = OperationRequest::new(
            &operation_id,
            &operation_id,
            OperationIntent {
                installation_id: installation_id.into(),
                kind: LifecycleOperationKind::Update,
                mod_instance_id: Some(instance_id.clone()),
                provider: Some(installed.provider.clone()),
                archive_sha256: Some(snapshot.sha256.clone()),
                file_plan_fingerprint: Some(file_plan_fingerprint(&files)),
                profile_id: None,
            },
        )
        .map_err(|_| error::internal())?;
        let plan = ValidatedInstallPlan::new(
            request,
            InstallMetadata {
                instance_id: instance_id.clone(),
                mod_id: installed.mod_id.clone(),
                display_name: installed.display_name.clone(),
                version: installed.version.clone(),
                provider: installed.provider.clone(),
                archive_sha256: Some(snapshot.sha256),
            },
            files,
        )
        .map_err(|_| error::internal())?;
        let mut runtime = ReleaseARuntime::new(store);
        require_success(runtime.update(plan, identity(&operation_id), &mut workspace))?;
        return Ok(Some((instance_id, folder.into())));
    }
    let provider = provider_ref(record, uid)?;
    let operation_id = stable_id(
        "adopt",
        &format!(
            "{installation_id}\0{instance_id}\0{}",
            file_plan_fingerprint(&files)
        ),
    );
    let request = OperationRequest::new(
        &operation_id,
        &operation_id,
        OperationIntent {
            installation_id: installation_id.into(),
            kind: LifecycleOperationKind::Install,
            mod_instance_id: Some(instance_id.clone()),
            provider: Some(provider.clone()),
            archive_sha256: Some(snapshot.sha256.clone()),
            file_plan_fingerprint: Some(file_plan_fingerprint(&files)),
            profile_id: None,
        },
    )
    .map_err(|_| error::internal())?;
    let display_name = record
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(uid)
        .chars()
        .take(512)
        .collect();
    let version = record
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(256).collect());
    let mod_id = record
        .get("packageID")
        .and_then(Value::as_str)
        .filter(|value| *value != "und.und.und")
        .map_or_else(
            || contract_id("package", uid, 256),
            |value| contract_id("package", value, 256),
        );
    let plan = ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: instance_id.clone(),
            mod_id,
            display_name,
            version,
            provider,
            archive_sha256: Some(snapshot.sha256),
        },
        files,
    )
    .map_err(|_| error::internal())?;
    let mut runtime = ReleaseARuntime::new(store);
    require_success(runtime.install(plan, identity(&operation_id), &mut workspace))?;
    Ok(Some((instance_id, folder.into())))
}

fn execute_archive_update(
    state: &AppState,
    installation_id: &str,
    instance_id: &str,
    operation_id: &str,
    archive: &Path,
) -> Result<Value, String> {
    let store = open_store(state)?;
    let installed = store
        .manifest(installation_id)
        .map_err(|_| error::internal())?
        .and_then(|manifest| {
            manifest
                .records
                .into_iter()
                .find(|record| record.instance_id == instance_id)
        })
        .ok_or_else(|| error::invalid("lifecycle:updateMod"))?;
    let folder = installed_package_folder(&installed.files)?;
    let staging_parent = state.data_root.root.join("lifecycle-import-staging");
    let staged = stage_mod_archive(archive, &staging_parent, ArchiveLimits::default(), || false)
        .map_err(|_| "archive_invalid".to_owned())?;
    if contract_id("package", staged.package_id(), 256) != installed.mod_id {
        return Err("source_identity_mismatch".into());
    }
    staged
        .bind_identity(instance_id)
        .map_err(|_| "source_identity_mismatch".to_owned())?;
    let mut entries = package_files(staged.root(), staged.root())?;
    for entry in &mut entries {
        entry.source_relative = PathBuf::from(entry.relative.as_str());
        entry.relative =
            ValidatedRelativePath::parse(&format!("{folder}/{}", entry.relative.as_str()))
                .map_err(|_| error::internal())?;
    }
    if entries.is_empty() {
        return Err("archive_invalid".into());
    }
    let snapshot = ensure_package_snapshot(state, staged.root(), &entries)?;
    let root = library_root(state, installation_id)?;
    let workspace_root = library_workspace_root(state, installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let mut workspace =
        OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
    workspace
        .register_artifact_tree_source(&snapshot.source_id, &snapshot.root)
        .map_err(|_| error::internal())?;
    let files = entries
        .into_iter()
        .map(|entry| InstallFilePlan {
            path_identity_key: entry.relative.as_str().to_ascii_lowercase(),
            expected_previous_sha256: None,
            path: entry.relative.clone(),
            sha256: entry.sha256,
            size_bytes: entry.size,
            source: StagingSource::ArtifactTree {
                source_id: snapshot.source_id.clone(),
                source_path: entry.relative,
            },
        })
        .collect::<Vec<_>>();
    let provider =
        local_artifact_provider(staged.package_id(), staged.version(), &snapshot.sha256)?;
    let request = OperationRequest::new(
        operation_id,
        operation_id,
        OperationIntent {
            installation_id: installation_id.into(),
            kind: LifecycleOperationKind::Update,
            mod_instance_id: Some(instance_id.into()),
            provider: Some(provider.clone()),
            archive_sha256: Some(snapshot.sha256.clone()),
            file_plan_fingerprint: Some(file_plan_fingerprint(&files)),
            profile_id: None,
        },
    )
    .map_err(|_| error::invalid("lifecycle:updateMod"))?;
    let plan = ValidatedInstallPlan::new(
        request,
        InstallMetadata {
            instance_id: instance_id.into(),
            mod_id: installed.mod_id.clone(),
            display_name: installed.display_name.clone(),
            version: staged.version().map(str::to_owned),
            provider,
            archive_sha256: Some(snapshot.sha256),
        },
        files,
    )
    .map_err(|_| error::invalid("lifecycle:updateMod"))?;
    let mut runtime = ReleaseARuntime::new(store);
    require_success(runtime.update(plan, identity(operation_id), &mut workspace))?;
    let scope = VerificationScope::new(installation_id, Some(instance_id.into()))
        .map_err(|_| error::internal())?;
    let report = runtime
        .verify_installation(&scope, &workspace)
        .map_err(|failure| failure.error.code.as_str().to_owned())?;
    Ok(json!({
        "ok": true,
        "cancelled": false,
        "operationId": operation_id,
        "verificationResult": serde_json::to_value(report.verification).map_err(|_| error::internal())?,
        "gameHealthReport": serde_json::to_value(report.health).map_err(|_| error::internal())?
    }))
}

fn verify_mod(state: &AppState, installation_id: &str, instance_id: &str) -> Result<Value, String> {
    let root = library_root(state, installation_id)?;
    let workspace_root = library_workspace_root(state, installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let workspace =
        OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
    let runtime = ReleaseARuntime::new(open_store(state)?);
    let scope = VerificationScope::new(installation_id, Some(instance_id.into()))
        .map_err(|_| error::invalid("lifecycle:verifyMod"))?;
    let report = runtime
        .verify_installation(&scope, &workspace)
        .map_err(|failure| failure.error.code.as_str().to_owned())?;
    Ok(json!({
        "verificationResult": serde_json::to_value(report.verification).map_err(|_| error::internal())?,
        "gameHealthReport": serde_json::to_value(report.health).map_err(|_| error::internal())?
    }))
}

fn operation_status(state: &AppState, operation_id: &str) -> Result<Value, String> {
    let runtime = ReleaseARuntime::new(open_store(state)?);
    let status = runtime
        .operation_status(operation_id)
        .map_err(|failure| failure.error.code.as_str().to_owned())?;
    let Some(status) = status else {
        return Ok(Value::Null);
    };
    Ok(json!({
        "operation": serde_json::to_value(status.operation).map_err(|_| error::internal())?,
        "progress": serde_json::to_value(status.progress).map_err(|_| error::internal())?,
        "recoveryDisposition": status.recovery_disposition
    }))
}

fn repair_mod(
    state: &AppState,
    installation_id: &str,
    instance_id: &str,
    operation_id: &str,
) -> Result<Value, String> {
    let store = open_store(state)?;
    let installed = store
        .manifest(installation_id)
        .map_err(|_| error::internal())?
        .and_then(|manifest| {
            manifest
                .records
                .into_iter()
                .find(|record| record.instance_id == instance_id)
        })
        .ok_or_else(|| error::invalid("lifecycle:repairMod"))?;
    let archive_sha256 = installed
        .archive_sha256
        .clone()
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "recovery_unavailable".to_owned())?;
    let source = cached_snapshot(state, &archive_sha256)?;
    let root = library_root(state, installation_id)?;
    let workspace_root = library_workspace_root(state, installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let mut workspace =
        OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
    workspace
        .register_artifact_tree_source(&source.source_id, &source.root)
        .map_err(|_| error::internal())?;
    let catalog = LocalRepairCatalog {
        source_id: source.source_id,
        archive_sha256: archive_sha256.clone(),
    };
    let request = OperationRequest::new(
        operation_id,
        operation_id,
        OperationIntent {
            installation_id: installation_id.into(),
            kind: LifecycleOperationKind::Repair,
            mod_instance_id: Some(instance_id.into()),
            provider: Some(installed.provider.clone()),
            archive_sha256: Some(archive_sha256),
            file_plan_fingerprint: Some(installed.file_plan_fingerprint.clone()),
            profile_id: None,
        },
    )
    .map_err(|_| error::invalid("lifecycle:repairMod"))?;
    let mut runtime = ReleaseARuntime::new(store);
    let plan = runtime
        .plan_repair(request, &workspace, &catalog)
        .map_err(|failure| failure.error.code.as_str().to_owned())?;
    match plan {
        RepairPlanDisposition::NotNeeded(report) => Ok(json!({
            "ok": true,
            "repaired": false,
            "operationId": operation_id,
            "verificationResult": serde_json::to_value(report.verification).map_err(|_| error::internal())?,
            "gameHealthReport": serde_json::to_value(report.health).map_err(|_| error::internal())?
        })),
        RepairPlanDisposition::Ready(plan) => {
            require_success(runtime.repair(*plan, identity(operation_id), &mut workspace))?;
            let scope = VerificationScope::new(installation_id, Some(instance_id.into()))
                .map_err(|_| error::internal())?;
            let report = runtime
                .verify_installation_with_sources(&scope, &workspace, &catalog)
                .map_err(|failure| failure.error.code.as_str().to_owned())?;
            Ok(json!({
                "ok": true,
                "repaired": true,
                "operationId": operation_id,
                "verificationResult": serde_json::to_value(report.verification).map_err(|_| error::internal())?,
                "gameHealthReport": serde_json::to_value(report.health).map_err(|_| error::internal())?
            }))
        }
    }
}

fn restore_last_working_state(
    state: &AppState,
    installation_id: &str,
    operation_id: &str,
) -> Result<Value, String> {
    let root = library_root(state, installation_id)?;
    let workspace_root = library_workspace_root(state, installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let mut workspace =
        OsLifecycleWorkspace::open(root, workspace_root).map_err(|_| error::internal())?;
    let request = OperationRequest::new(
        operation_id,
        operation_id,
        OperationIntent {
            installation_id: installation_id.into(),
            kind: LifecycleOperationKind::Recover,
            mod_instance_id: None,
            provider: None,
            archive_sha256: None,
            file_plan_fingerprint: None,
            profile_id: None,
        },
    )
    .map_err(|_| error::invalid("lifecycle:restoreLastWorkingState"))?;
    let mut runtime = ReleaseARuntime::new(open_store(state)?);
    require_success(runtime.restore_last_working_state(
        request,
        identity(operation_id),
        &mut workspace,
    ))?;
    Ok(json!({"ok": true, "operationId": operation_id}))
}

fn uninstall_mod(
    state: &AppState,
    installation_id: &str,
    instance_id: &str,
    operation_id: &str,
) -> Result<Value, String> {
    let store = open_store(state)?;
    let manifest = store
        .manifest(installation_id)
        .map_err(|_| error::internal())?
        .ok_or_else(|| error::invalid("lifecycle:uninstallMod"))?;
    let installed = manifest
        .records
        .iter()
        .find(|record| record.instance_id == instance_id)
        .cloned()
        .ok_or_else(|| error::invalid("lifecycle:uninstallMod"))?;
    let root = library_root(state, installation_id)?;
    let workspace_root = library_workspace_root(state, installation_id);
    fs::create_dir_all(&workspace_root).map_err(|_| error::internal())?;
    let mut workspace =
        OsLifecycleWorkspace::open(root.clone(), workspace_root).map_err(|_| error::internal())?;
    let request = OperationRequest::new(
        operation_id,
        operation_id,
        OperationIntent {
            installation_id: installation_id.into(),
            kind: LifecycleOperationKind::Uninstall,
            mod_instance_id: Some(instance_id.into()),
            provider: None,
            archive_sha256: None,
            file_plan_fingerprint: None,
            profile_id: None,
        },
    )
    .map_err(|_| error::invalid("lifecycle:uninstallMod"))?;
    let plan = ValidatedUninstallPlan::new(request)
        .map_err(|_| error::invalid("lifecycle:uninstallMod"))?;
    let mut runtime = ReleaseARuntime::new(store);
    require_success(runtime.uninstall(plan, identity(operation_id), &mut workspace))?;
    remove_empty_package_directories(&root, &installed.files)?;
    Ok(json!({"ok": true, "operationId": operation_id}))
}

struct PackageFile {
    relative: ValidatedRelativePath,
    source_relative: PathBuf,
    sha256: String,
    size: u64,
}

struct PackageSnapshot {
    root: PathBuf,
    source_id: String,
    sha256: String,
}

struct LocalRepairCatalog {
    source_id: String,
    archive_sha256: String,
}

impl RepairSourceCatalog for LocalRepairCatalog {
    fn cached_archive(&self, expected_archive_sha256: &str) -> Option<CachedArchiveSource> {
        self.archive_sha256
            .eq_ignore_ascii_case(expected_archive_sha256)
            .then(|| CachedArchiveSource {
                source_id: self.source_id.clone(),
                archive_sha256: self.archive_sha256.clone(),
            })
    }

    fn exact_provider_artifact(
        &self,
        _expected_provider: &ProviderRef,
    ) -> Option<ProviderArtifactSource> {
        None
    }
}

fn package_files(root: &Path, package_root: &Path) -> Result<Vec<PackageFile>, String> {
    let mut pending = vec![package_root.to_owned()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let metadata = fs::symlink_metadata(&directory).map_err(|_| error::internal())?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(error::internal());
        }
        for entry in fs::read_dir(&directory).map_err(|_| error::internal())? {
            let path = entry.map_err(|_| error::internal())?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| error::internal())?;
            if is_link_or_reparse(&metadata) {
                return Err(error::internal());
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            if !metadata.is_file() || files.len() >= MAX_PACKAGE_FILES {
                return Err(error::internal());
            }
            let relative = path.strip_prefix(root).map_err(|_| error::internal())?;
            let normalized = relative.to_string_lossy().replace('\\', "/");
            let relative =
                ValidatedRelativePath::parse(&normalized).map_err(|_| error::internal())?;
            let verified = inspect_regular_file(&path, MAX_PACKAGE_FILE_BYTES)
                .map_err(|_| error::internal())?;
            files.push(PackageFile {
                source_relative: PathBuf::from(relative.as_str()),
                relative,
                sha256: verified.sha256().into(),
                size: verified.size(),
            });
        }
    }
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}

fn ensure_package_snapshot(
    state: &AppState,
    library_root: &Path,
    entries: &[PackageFile],
) -> Result<PackageSnapshot, String> {
    let sha256 = package_snapshot_sha(entries);
    let cache_root = state.data_root.root.join(ARTIFACT_CACHE_DIRECTORY);
    fs::create_dir_all(&cache_root).map_err(|_| error::internal())?;
    require_plain_directory(&cache_root)?;
    let target = cache_root.join(&sha256);
    if !target.exists() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let temporary = cache_root.join(format!(".{sha256}.{}.{}", std::process::id(), nonce));
        fs::create_dir(&temporary).map_err(|_| error::internal())?;
        for entry in entries {
            let destination = temporary.join(entry.relative.as_str());
            create_plain_parents(&temporary, Path::new(entry.relative.as_str()))?;
            let copied = copy_relative_regular_file_verified(
                library_root,
                &entry.source_relative,
                &destination,
                MAX_PACKAGE_FILE_BYTES,
            )
            .map_err(|_| error::internal())?;
            if copied.size() != entry.size || !copied.sha256().eq_ignore_ascii_case(&entry.sha256) {
                return Err(error::internal());
            }
        }
        match fs::rename(&temporary, &target) {
            Ok(()) => {}
            Err(_) if target.is_dir() => {}
            Err(_) => return Err(error::internal()),
        }
    }
    require_plain_directory(&target)?;
    for entry in entries {
        let cached = inspect_regular_file(
            &target.join(entry.relative.as_str()),
            MAX_PACKAGE_FILE_BYTES,
        )
        .map_err(|_| error::internal())?;
        if cached.size() != entry.size || !cached.sha256().eq_ignore_ascii_case(&entry.sha256) {
            return Err(error::internal());
        }
    }
    Ok(PackageSnapshot {
        root: target,
        source_id: format!("artifact-{sha256}"),
        sha256,
    })
}

fn cached_snapshot(state: &AppState, sha256: &str) -> Result<PackageSnapshot, String> {
    let root = state
        .data_root
        .root
        .join(ARTIFACT_CACHE_DIRECTORY)
        .join(sha256);
    require_plain_directory(&root).map_err(|_| "recovery_unavailable".to_owned())?;
    Ok(PackageSnapshot {
        root,
        source_id: format!("artifact-{sha256}"),
        sha256: sha256.into(),
    })
}

fn package_snapshot_sha(entries: &[PackageFile]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"deltamod-package-snapshot-v1\0");
    for entry in entries {
        hasher.update(entry.relative.as_str().as_bytes());
        hasher.update([0]);
        hasher.update(entry.sha256.as_bytes());
        hasher.update([0]);
        hasher.update(entry.size.to_le_bytes());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn require_plain_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| error::internal())?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(error::internal());
    }
    Ok(())
}

fn create_plain_parents(root: &Path, relative: &Path) -> Result<(), String> {
    let mut current = root.to_owned();
    for component in relative.parent().unwrap_or(Path::new("")).components() {
        let Component::Normal(component) = component else {
            return Err(error::internal());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|_| crate::error::internal())?;
            }
            _ => return Err(error::internal()),
        }
    }
    Ok(())
}

fn resolve_package_root(
    state: &AppState,
    folder: &str,
    uid: &str,
) -> Result<Option<(&'static str, PathBuf, PathBuf)>, String> {
    let candidates = [
        (
            PACKETS_INSTALLATION,
            state.data_root.root.join("packets"),
            "__deltaID.json",
        ),
        (
            MODS_INSTALLATION,
            state.data_root.root.join("mods"),
            "manifest.json",
        ),
    ];
    for (installation, root, identity_file) in candidates {
        let package = root.join(folder);
        let identity = package.join(identity_file);
        let Ok(metadata) = fs::symlink_metadata(&identity) else {
            continue;
        };
        if !metadata.is_file() || is_link_or_reparse(&metadata) || metadata.len() > 1024 * 1024 {
            continue;
        }
        let value: Value =
            serde_json::from_slice(&fs::read(identity).map_err(|_| error::internal())?)
                .map_err(|_| error::internal())?;
        let recorded = value
            .get("uniqueId")
            .or_else(|| value.get("uid"))
            .and_then(Value::as_str);
        if recorded == Some(uid) {
            return Ok(Some((installation, root, package)));
        }
    }
    Ok(None)
}

fn provider_ref(record: &Value, uid: &str) -> Result<ProviderRef, String> {
    let gamebanana_id = record
        .get("gamebanana")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .filter(|value| valid_id(value, 256));
    let (provider, resource) = if let Some(id) = gamebanana_id {
        ("gamebanana", id)
    } else {
        ("local", uid)
    };
    ProviderRef::new(
        ProviderId::parse(provider).map_err(|_| error::internal())?,
        if provider == "local" {
            ProviderItemKind::LocalArchive
        } else {
            ProviderItemKind::Mod
        },
        ProviderResourceId::parse(&contract_id("resource", resource, 256))
            .map_err(|_| error::internal())?,
        None,
        None,
        ProviderArtifactKind::Unknown,
        None,
        None,
    )
    .map_err(|_| error::internal())
}

fn local_artifact_provider(
    package_id: &str,
    version: Option<&str>,
    archive_sha256: &str,
) -> Result<ProviderRef, String> {
    ProviderRef::new(
        ProviderId::parse("local").map_err(|_| error::internal())?,
        ProviderItemKind::LocalArchive,
        ProviderResourceId::parse(&contract_id("resource", package_id, 256))
            .map_err(|_| error::internal())?,
        None,
        Some(
            ProviderResourceId::parse(&contract_id("artifact", archive_sha256, 256))
                .map_err(|_| error::internal())?,
        ),
        ProviderArtifactKind::Archive,
        version
            .map(|value| ProviderResourceId::parse(&contract_id("version", value, 256)))
            .transpose()
            .map_err(|_| error::internal())?,
        None,
    )
    .map_err(|_| error::internal())
}

fn installed_package_folder(
    files: &[deltamod_product_contracts::InstalledFileRef],
) -> Result<String, String> {
    let mut folder = None::<&str>;
    for file in files {
        let Some((candidate, _)) = file.path.as_str().split_once('/') else {
            return Err(error::internal());
        };
        if !safe_segment(candidate) || folder.is_some_and(|current| current != candidate) {
            return Err(error::internal());
        }
        folder = Some(candidate);
    }
    folder.map(str::to_owned).ok_or_else(error::internal)
}

fn open_store(state: &AppState) -> Result<DurableLifecycleStore, String> {
    let root = state.data_root.root.join("lifecycle-store");
    fs::create_dir_all(&root).map_err(|_| error::internal())?;
    DurableLifecycleStore::open(root).map_err(|_| error::internal())
}

fn library_root(state: &AppState, installation_id: &str) -> Result<PathBuf, String> {
    let root = match installation_id {
        MODS_INSTALLATION => state.data_root.root.join("mods"),
        PACKETS_INSTALLATION => state.data_root.root.join("packets"),
        _ => return Err(error::invalid("lifecycle")),
    };
    fs::create_dir_all(&root).map_err(|_| error::internal())?;
    Ok(root)
}

fn library_workspace_root(state: &AppState, installation_id: &str) -> PathBuf {
    state
        .data_root
        .root
        .join("lifecycle-library-workspaces")
        .join(installation_id)
}

fn remove_empty_package_directories(
    root: &Path,
    files: &[deltamod_product_contracts::InstalledFileRef],
) -> Result<(), String> {
    let mut directories = BTreeSet::new();
    for file in files {
        let mut parent = Path::new(file.path.as_str()).parent();
        while let Some(relative) = parent {
            if relative.as_os_str().is_empty() {
                break;
            }
            directories.insert(relative.to_owned());
            parent = relative.parent();
        }
    }
    for relative in directories.into_iter().rev() {
        let directory = root.join(relative);
        let Ok(metadata) = fs::symlink_metadata(&directory) else {
            continue;
        };
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(error::internal());
        }
        match fs::remove_dir(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(_) => return Err(crate::error::internal()),
        }
    }
    Ok(())
}

fn require_success(outcome: LifecycleOutcome) -> Result<(), String> {
    match outcome {
        LifecycleOutcome::Succeeded { .. } | LifecycleOutcome::Existing { .. } => Ok(()),
        LifecycleOutcome::Busy { error, .. }
        | LifecycleOutcome::Rejected { error, .. }
        | LifecycleOutcome::RecoveryRequired { error, .. } => Err(error.code.as_str().into()),
    }
}

fn identity(operation_id: &str) -> ExecutionIdentity {
    let digest = digest(operation_id.as_bytes());
    ExecutionIdentity {
        owner_instance_id: format!("tauri-{}", std::process::id()),
        lease_id: format!("lease-{}", &digest[..32]),
        recovery_generation_id: format!("generation-{}", &digest[32..]),
        now_ms: now_ms(),
        lease_ttl_ms: 5 * 60 * 1_000,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn stable_id(prefix: &str, material: &str) -> String {
    format!("{prefix}-{}", &digest(material.as_bytes())[..40])
}

fn contract_id(prefix: &str, value: &str, max: usize) -> String {
    if valid_id(value, max) {
        value.into()
    } else {
        stable_id(prefix, value)
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !value.contains(['/', '\\', ':'])
        && !value.chars().any(char::is_control)
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn state() -> (AppState, PathBuf) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("deltamod-lifecycle-channel-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        let resources = root.join("resources");
        fs::create_dir(&resources).unwrap();
        (
            AppState::initialize(root.join("data"), resources).unwrap(),
            root,
        )
    }

    fn replacement_archive(root: &Path) -> PathBuf {
        let path = root.join("replacement.zip");
        let file = fs::File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, bytes) in [
            (
                "meta.toml",
                b"[metadata]\nname='Example'\nversion='2.0'\npackageID='example.mod'\ngame='deltarune'\n"
                    .as_slice(),
            ),
            ("modding.xml", b"<mod version='2'/>".as_slice()),
            ("new.dat", b"new-version".as_slice()),
        ] {
            archive
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    #[test]
    fn legacy_packet_is_adopted_verified_and_uninstalled_transactionally() {
        let (state, root) = state();
        let packet = state.data_root.root.join("packets").join("example.mod");
        fs::create_dir_all(&packet).unwrap();
        fs::write(
            packet.join("__deltaID.json"),
            r#"{"uniqueId":"packet-uid"}"#,
        )
        .unwrap();
        let metadata = b"[metadata]\nname='Example'\nversion='1.0'\npackageID='example.mod'\n";
        fs::write(packet.join("meta.toml"), metadata).unwrap();
        fs::write(packet.join("modding.xml"), "<mod/>").unwrap();

        let catalog = dispatch(&state, "lifecycle:getInstalledMods", &[])
            .unwrap()
            .unwrap();
        assert_eq!(
            catalog["installedMods"].as_array().unwrap().len(),
            1,
            "legacy adoption catalog: {catalog}"
        );
        assert_eq!(catalog["installedMods"][0]["instanceId"], "packet-uid");
        assert_eq!(
            catalog["installedMods"][0]["archiveSha256"]
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(catalog["verificationResults"][0]["state"], "healthy");
        assert_eq!(catalog["foldersByInstanceId"]["packet-uid"], "example.mod");

        fs::remove_file(packet.join("modding.xml")).unwrap();
        let repair = dispatch(
            &state,
            "lifecycle:repairMod",
            &[
                json!(PACKETS_INSTALLATION),
                json!("packet-uid"),
                json!("repair-missing"),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(repair["repaired"], true);
        assert_eq!(repair["verificationResult"]["state"], "healthy");
        assert_eq!(
            fs::read_to_string(packet.join("modding.xml")).unwrap(),
            "<mod/>"
        );

        let update = execute_archive_update(
            &state,
            PACKETS_INSTALLATION,
            "packet-uid",
            "update-v2",
            &replacement_archive(&root),
        )
        .unwrap();
        assert_eq!(update["verificationResult"]["state"], "healthy");
        assert!(fs::read_to_string(packet.join("meta.toml"))
            .unwrap()
            .contains("version='2.0'"));
        assert_eq!(fs::read(packet.join("new.dat")).unwrap(), b"new-version");
        assert_eq!(
            fs::read_to_string(packet.join("modding.xml")).unwrap(),
            "<mod version='2'/>"
        );

        let restored = dispatch(
            &state,
            "lifecycle:restoreLastWorkingState",
            &[json!(PACKETS_INSTALLATION), json!("restore-v1")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(restored["ok"], true);
        assert_eq!(
            fs::read_to_string(packet.join("modding.xml")).unwrap(),
            "<mod/>"
        );
        assert!(!packet.join("new.dat").exists());

        fs::write(packet.join("meta.toml"), b"externally changed").unwrap();
        let verified = dispatch(
            &state,
            "lifecycle:verifyMod",
            &[json!(PACKETS_INSTALLATION), json!("packet-uid")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(verified["verificationResult"]["state"], "hash_mismatch");
        assert!(dispatch(
            &state,
            "lifecycle:uninstallMod",
            &[
                json!(PACKETS_INSTALLATION),
                json!("packet-uid"),
                json!("uninstall-blocked")
            ],
        )
        .is_err());
        assert!(packet.is_dir());

        fs::write(packet.join("meta.toml"), metadata).unwrap();
        let result = dispatch(
            &state,
            "lifecycle:uninstallMod",
            &[
                json!(PACKETS_INSTALLATION),
                json!("packet-uid"),
                json!("uninstall-success"),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(result["ok"], true);
        assert!(!packet.exists());
        let _ = fs::remove_dir_all(root);
    }
}
