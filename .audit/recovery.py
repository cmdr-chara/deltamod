from pathlib import Path
import re

def read(p): return Path(p).read_text()
def write(p,s):
    p=Path(p); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(s)

p='src-tauri/crates/profile-install/src/lib.rs'; s=read(p)
s=s.replace('#![forbid(unsafe_code)]', '#![forbid(unsafe_code)]\n\nmod recovery_guard;')
s=s.replace('    copy: Arc<dyn CopyBackend>,\n    events:', '    copy: Arc<dyn CopyBackend>,\n    lifecycle: Arc<Mutex<()>>,\n    registry: Arc<Mutex<()>>,\n    events:')
s=s.replace('            copy,\n            events:', '            copy,\n            lifecycle: Arc::new(Mutex::new(())),\n            registry: Arc::new(Mutex::new(())),\n            events:')
# Lifecycle serialization begins before ID allocation or reading mutable installation state.
# The metadata lock is separate: cancellation, progress and unrelated reads remain responsive.
def guard(source,name,lock):
    match=re.search(r'\bfn '+re.escape(name)+r'\s*\(',source)
    if not match: raise RuntimeError(f'missing method {name}')
    brace=source.index('{\n',match.end())
    return source[:brace+2]+f'        let _{lock} = self.{lock}.lock().map_err(|_| RuntimeError::Journal("{lock} lock poisoned".into()))?;\n'+source[brace+2:]
for name in ['import_official_profile','create_installation','copy_installation','reimport_installation','repair_installation','delete_installation','legacy_create_installation','legacy_reimport_installation','legacy_delete_installation']:
    s=guard(s,name,'lifecycle')
for name in ['set_name','set_edition','select','persist_added','commit_state','delete_installation','legacy_set_name','legacy_remove_steam_integration','legacy_change_system_index','legacy_delete_installation']:
    s=guard(s,name,'registry')
# The last-added registry guard in delete must follow lifecycle to avoid lock inversion.
for name in ['delete_installation','legacy_delete_installation']:
    match=re.search(r'\bfn '+name+r'\s*\(',s); brace=s.index('{\n',match.end())
    tail=s[brace+2:]; lines=tail.splitlines(keepends=True)
    assert '_registry' in lines[0] and '_lifecycle' in lines[1]
    s=s[:brace+2]+lines[1]+lines[0]+''.join(lines[2:])
# Do not hold the registry lock during long copies. Keep the existing late snapshot semantics.
a='''            store.insert(
                "gamePath".into(),'''
assert s.count(a)==2
# First occurrence is legacy_create; second is reimport (which receives its own guard below).
idx=s.index(a,s.index('fn legacy_create_installation'))
s=s[:idx]+'''            let _registry = self.registry.lock().map_err(|_| RuntimeError::Journal("registry lock poisoned".into()))?;
'''+s[idx:]
a='''        // The copy can take minutes. Snapshot only when publishing metadata so'''
assert a in s
s=s.replace(a,'''        let _registry = self.registry.lock().map_err(|_| RuntimeError::Journal("registry lock poisoned".into()))?;
'''+a)
# Mutation paths are validated before target creation, not only during restart recovery.
a='''        let id = self.start(kind)?;
        // The staged copier'''
assert a in s
s=s.replace(a,'''        recovery_guard::validate_destination(&self.root, kind, destination)?;
        let id = self.start(kind)?;
        // The staged copier''')
# Read installation state using the existing bounded storage parser.
s=s.replace('Ok(serde_json::from_slice(&fs::read(&self.state_path)?)?)','Ok(load_json(&self.state_path)?)')
# Never discard the journal when replacement rollback or cleanup could not be completed.
a='''                    })() {
                        self.abort(id, &journal);
                        let _ = fs::remove_dir_all(&target);
                        return Err(error.into());
                    }'''
assert a in s
s=s.replace(a,'''                    })() {
                        // Keep all remaining recovery paths and the journal. Startup recovery
                        // validates ownership and refuses to guess between two usable copies.
                        return Err(error.into());
                    }''')
s=s.replace('''                        self.abort(id, &journal);
                        let _ = fs::remove_dir_all(&target);
                        return Err(RuntimeError::Cancelled(id));''','''                        recovery_guard::remove_directory_if_present(&target)?;
                        self.abort(id, &journal);
                        return Err(RuntimeError::Cancelled(id));''')
# Replace recovery as one bounded, ownership-checked unit; no ignored deletion errors.
start=s.index('    fn recover(&self) -> Result<(), RuntimeError> {')
end=s.index('\n}\n\nimpl From<PersistedState>',start)
s=s[:start]+'''    fn recover(&self) -> Result<(), RuntimeError> {
        recovery_guard::recover(self)
    }
'''+s[end:]
# Keep positive stable operation identities for metadata-only operations as well.
a='''        if ownership == Ownership::LinkedExternal {
            self.persist_added(installation)?;
            return Ok(self.accepted("link"));
        }'''
assert a in s
s=s.replace(a,'''        if ownership == Ownership::LinkedExternal {
            let id = self.start("link")?;
            self.persist_added(installation)?;
            self.complete_operation(id);
            return Ok(OperationResponse { operation_id: id, accepted: true });
        }''')
# Deletion journal must get its own ID rather than reusing the last copy operation's ID.
start=s.index('    pub fn delete_installation('); end=s.index('    pub fn set_name(',start)
block=s[start:end]
block=block.replace('        let mut state = self.state()?;', '        let operation_id = self.start("installation-delete")?;\n        let mut state = self.state()?;',1)
block=block.replace('        Ok(self.accepted("installation-delete"))','        self.complete_operation(operation_id);\n        Ok(OperationResponse { operation_id, accepted: true })')
s=s[:start]+block+s[end:]
s=s.replace('        o.next += 1;', '        o.next = o.next.checked_add(1).ok_or_else(|| RuntimeError::Journal("operation ID exhausted".into()))?;')
s+='\n#[cfg(test)]\n#[path = "recovery_tests.rs"]\nmod recovery_tests;\n'
write(p,s)

write('src-tauri/crates/profile-install/src/recovery_guard.rs',r'''use super::*;
use std::io::Read;
use std::path::Component;
const MAX_JOURNAL_BYTES: u64 = 256 * 1024;
const MAX_JOURNALS: usize = 4096;
fn invalid(message: &str) -> RuntimeError { RuntimeError::Journal(message.into()) }

fn linked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() { return true; }
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return true; }
    }
    false
}
/// Validate every existing component without following links. This does not claim
/// to defeat hostile local path swaps after validation; OS-relative handles remain
/// the boundary for that stronger guarantee.
fn owned_directory(root: &Path, path: &Path) -> Result<(), RuntimeError> {
    let relative = path.strip_prefix(root).map_err(|_| invalid("recovery path escaped root"))?;
    if relative.as_os_str().is_empty() || !relative.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(invalid("invalid recovery path components"));
    }
    let mut current = root.to_path_buf();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component { current.push(component.as_os_str()); }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !linked(&metadata) && metadata.is_dir() => {}
            Ok(_) => return Err(invalid("recovery path contains a link or non-directory")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
fn profile_index(value: &str) -> Option<u32> {
    let digits = value.strip_prefix("deltamod_system-")?;
    let index = digits.parse::<u32>().ok()?;
    (index <= MAX_LEGACY_INSTALLATION_INDEX && digits == index.to_string()).then_some(index)
}
pub(super) fn validate_destination(root: &Path, kind: &str, path: &Path) -> Result<(), RuntimeError> {
    owned_directory(root, path)?;
    let relative = path.strip_prefix(root).map_err(|_| invalid("invalid destination"))?;
    let parts = relative.iter().map(|p| p.to_str().ok_or_else(|| invalid("non-UTF-8 recovery path"))).collect::<Result<Vec<_>, _>>()?;
    let allowed = match (kind, parts.as_slice()) {
        ("official-profile-import", ["official-profile"]) => true,
        ("installation-create" | "installation-copy" | "installation-reimport" | "installation-repair" | "installation-delete", ["installations", id]) => InstallationId::new(*id).is_ok(),
        ("legacy-profile-delete", [profile]) => profile_index(profile).is_some(),
        ("legacy-installation-create", [profile, "deltaruneInstall"]) => profile_index(profile).is_some(),
        ("legacy-installation-reimport", [profile, name]) => profile_index(profile).is_some() && (*name == "deltaruneInstall" || name.strip_prefix("deltaruneInstall-reimport-").is_some_and(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) && value.parse::<u64>().is_ok())),
        _ => false,
    };
    if !allowed { return Err(invalid("journal destination does not belong to its operation")); }
    Ok(())
}
fn same_optional(actual: Option<&str>, expected: Option<&Path>) -> bool {
    actual.map(Path::new) == expected
}
fn validate(root: &Path, file: &Path, journal: &Journal) -> Result<(), RuntimeError> {
    if journal.version != 1 || file.file_name().and_then(|p| p.to_str()) != Some(format!("{}.json", journal.operation_id).as_str()) {
        return Err(invalid("unsupported or mismatched journal identity"));
    }
    let destination = Path::new(journal.destination.as_deref().ok_or_else(|| invalid("missing destination"))?);
    validate_destination(root, &journal.kind, destination)?;
    let id = journal.operation_id;
    if journal.kind == "legacy-profile-delete" {
        let trash = root.join(".runtime-replacements").join(format!("legacy-delete-{id}"));
        if journal.staging.is_some() || journal.backup.is_some() || !same_optional(journal.replacement.as_deref(), Some(&trash)) {
            return Err(invalid("invalid delete recovery paths"));
        }
        owned_directory(root, &trash)?;
    } else if journal.kind == "installation-delete" {
        if journal.staging.is_some() || journal.replacement.is_some() || journal.backup.is_some() {
            return Err(invalid("unexpected installation-delete recovery paths"));
        }
    } else {
        let replacement = root.join(".runtime-replacements").join(id.to_string());
        let backup = destination.with_extension(format!("deltamod-replacing-{id}"));
        let replacing = journal.replacement.is_some();
        if !same_optional(journal.replacement.as_deref(), replacing.then_some(replacement.as_path()))
            || !same_optional(journal.backup.as_deref(), replacing.then_some(backup.as_path())) {
            return Err(invalid("invalid replacement recovery paths"));
        }
        let target = if replacing { replacement.as_path() } else { destination };
        let staging = target.with_extension(format!("importing-{id}"));
        if !same_optional(journal.staging.as_deref(), Some(&staging)) { return Err(invalid("invalid staging recovery path")); }
        owned_directory(root, &staging)?;
        if replacing { owned_directory(root, &replacement)?; owned_directory(root, &backup)?; }
    }
    Ok(())
}
pub(super) fn remove_directory_if_present(path: &Path) -> Result<(), RuntimeError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
pub(super) fn recover(runtime: &Runtime) -> Result<(), RuntimeError> {
    let root = &runtime.root;
    let directory = root.join(".runtime-journals");
    owned_directory(root, &directory)?;
    let entries = fs::read_dir(&directory)?.take(MAX_JOURNALS + 1).collect::<Result<Vec<_>, _>>()?;
    if entries.len() > MAX_JOURNALS { return Err(invalid("too many recovery journals")); }
    let mut journals = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|p| p.to_str()) != Some("json") { continue; }
        let metadata = fs::symlink_metadata(&path)?;
        if linked(&metadata) || !metadata.is_file() || metadata.len() > MAX_JOURNAL_BYTES { return Err(invalid("invalid journal file")); }
        let file = fs::File::open(&path)?;
        if !file.metadata()?.is_file() { return Err(invalid("journal changed type")); }
        let mut bytes = Vec::new();
        file.take(MAX_JOURNAL_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES { return Err(invalid("journal too large")); }
        let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| invalid("malformed recovery journal"))?;
        validate(root, &path, &journal)?;
        journals.push((path, journal));
    }
    journals.sort_by_key(|(_, journal)| journal.operation_id);
    for (path, journal) in journals {
        // Revalidate just before mutation as another completed recovery may have changed paths.
        validate(root, &path, &journal)?;
        let destination = PathBuf::from(journal.destination.as_deref().expect("validated destination"));
        if journal.kind == "legacy-profile-delete" {
            let trash = PathBuf::from(journal.replacement.as_deref().expect("validated replacement"));
            let index = profile_index(destination.file_name().and_then(|p| p.to_str()).expect("validated profile")).expect("validated index");
            let indexed = runtime.load_legacy_profiles()?.installations.iter().any(|record| record.index == Some(index));
            if indexed && trash.exists() {
                if destination.exists() { return Err(invalid("ambiguous profile recovery; both copies preserved")); }
                fs::rename(&trash, &destination)?;
            } else if !indexed {
                remove_directory_if_present(&trash)?;
            }
        } else {
            if let Some(backup) = journal.backup.as_deref().map(Path::new) {
                if backup.exists() {
                    if destination.exists() { return Err(invalid("ambiguous replacement recovery; both copies preserved")); }
                    fs::rename(backup, &destination)?;
                }
            }
            if let Some(staging) = journal.staging.as_deref() { remove_directory_if_present(Path::new(staging))?; }
            if let Some(replacement) = journal.replacement.as_deref() { remove_directory_if_present(Path::new(replacement))?; }
        }
        runtime.finish_journal(&path)?;
    }
    Ok(())
}
''')
write('src-tauri/crates/profile-install/src/recovery_tests.rs',r'''use super::*;
fn journal(root: &Path, destination: &Path, replacing: bool) -> PathBuf {
    let replacement = root.join(".runtime-replacements/1");
    let target = if replacing { replacement.as_path() } else { destination };
    let value = serde_json::json!({
        "version":1,"operation_id":1,"kind":"installation-reimport","source":null,
        "destination":destination,"staging":target.with_extension("importing-1"),
        "replacement":replacing.then_some(&replacement),
        "backup":replacing.then(|| destination.with_extension("deltamod-replacing-1")),
        "status":"prepared"
    });
    let path = root.join(".runtime-journals/1.json");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap(); path
}
#[test]
fn traversal_in_journal_never_deletes_external_files() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = Runtime::open(temp.path().join("community")).unwrap();
    let outside = temp.path().join("outside"); fs::create_dir(&outside).unwrap(); fs::write(outside.join("sentinel"), b"keep").unwrap();
    let dest = runtime.root.join("../outside");
    let path = journal(&runtime.root, &dest, false);
    assert!(runtime.recover().is_err()); assert!(path.exists());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"keep");
}
#[test]
fn ambiguous_backup_is_preserved_with_its_journal() {
    let temp = tempfile::tempdir().unwrap(); let runtime = Runtime::open(temp.path()).unwrap();
    let dest = runtime.root.join("installations/install-safe");
    let backup = dest.with_extension("deltamod-replacing-1");
    fs::create_dir(&dest).unwrap(); fs::create_dir(&backup).unwrap();
    fs::write(dest.join("data"), b"new").unwrap(); fs::write(backup.join("data"), b"old").unwrap();
    let path = journal(&runtime.root, &dest, true);
    assert!(runtime.recover().is_err()); assert!(path.exists());
    assert_eq!(fs::read(dest.join("data")).unwrap(), b"new"); assert_eq!(fs::read(backup.join("data")).unwrap(), b"old");
}
#[test]
fn missing_destination_restores_validated_backup_and_cleans_staging() {
    let temp = tempfile::tempdir().unwrap(); let runtime = Runtime::open(temp.path()).unwrap();
    let dest = runtime.root.join("installations/install-safe"); let backup = dest.with_extension("deltamod-replacing-1");
    fs::create_dir(&backup).unwrap(); fs::write(backup.join("data"), b"old").unwrap();
    let replacement = runtime.root.join(".runtime-replacements/1"); fs::create_dir_all(&replacement).unwrap();
    fs::create_dir(replacement.with_extension("importing-1")).unwrap();
    let path = journal(&runtime.root, &dest, true);
    runtime.recover().unwrap();
    assert_eq!(fs::read(dest.join("data")).unwrap(), b"old"); assert!(!path.exists()); assert!(!replacement.exists());
}
#[test]
fn journal_size_and_future_versions_fail_without_cleanup() {
    let temp = tempfile::tempdir().unwrap(); let runtime = Runtime::open(temp.path()).unwrap();
    let dest = runtime.root.join("installations/install-safe"); let path = journal(&runtime.root, &dest, false);
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap(); value["version"] = json!(2);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap(); assert!(runtime.recover().is_err()); assert!(path.exists());
    fs::write(&path, vec![b' '; 256 * 1024 + 1]).unwrap(); assert!(runtime.recover().is_err()); assert!(path.exists());
}
#[cfg(unix)]
#[test]
fn symlinked_recovery_parent_is_not_followed() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap(); let runtime = Runtime::open(temp.path().join("community")).unwrap();
    let outside = temp.path().join("outside"); fs::create_dir(&outside).unwrap();
    fs::remove_dir(runtime.root.join("installations")).unwrap(); symlink(&outside, runtime.root.join("installations")).unwrap();
    let dest = runtime.root.join("installations/install-safe"); let path = journal(&runtime.root, &dest, false);
    assert!(runtime.recover().is_err()); assert!(path.exists()); assert!(outside.exists());
}
#[test]
fn linked_or_tampered_reimport_cannot_replace_an_external_destination() {
    let temp = tempfile::tempdir().unwrap(); let source = temp.path().join("source"); fs::create_dir(&source).unwrap();
    fs::write(source.join("data"), b"original").unwrap(); let runtime = Runtime::open(temp.path().join("community")).unwrap();
    runtime.create_installation(&source, "linked".into(), GamePlatform::Windows, Ownership::LinkedExternal).unwrap();
    let mut state = runtime.state().unwrap(); state.installations[0].install_path = source.clone(); runtime.save_state(&state).unwrap();
    assert!(runtime.reimport_installation(&state.installations[0].id).is_err());
    assert_eq!(fs::read(source.join("data")).unwrap(), b"original");
}
#[test]
fn concurrent_creates_allocate_distinct_ids_and_preserve_both_records() {
    let temp = tempfile::tempdir().unwrap(); let source = temp.path().join("source"); fs::create_dir(&source).unwrap(); fs::write(source.join("data"), b"original").unwrap();
    let runtime = Runtime::open(temp.path().join("community")).unwrap(); let barrier = Arc::new(std::sync::Barrier::new(3));
    let workers = (0..2).map(|index| { let runtime = runtime.clone(); let source = source.clone(); let barrier = barrier.clone();
        std::thread::spawn(move || { barrier.wait(); runtime.create_installation(&source, format!("copy-{index}"), GamePlatform::Windows, Ownership::ManagedCopy).unwrap() })
    }).collect::<Vec<_>>();
    barrier.wait(); let results = workers.into_iter().map(|thread| thread.join().unwrap()).collect::<Vec<_>>();
    assert_ne!(results[0].operation_id, results[1].operation_id);
    let list = runtime.list_installations().unwrap(); assert_eq!(list.installations.len(), 2); assert_ne!(list.installations[0].id, list.installations[1].id);
    for installation in list.installations { assert_eq!(fs::read(installation.install_path.join("data")).unwrap(), b"original"); }
}
''')
print('Recovery journals are bounded and ownership-validated; ambiguous recovery copies are retained; lifecycle mutations serialized.')
