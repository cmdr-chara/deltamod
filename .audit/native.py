from pathlib import Path
import re

def read(p): return Path(p).read_text()
def write(p,s):
    p=Path(p); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(s)
def replace(p,a,b):
    s=read(p)
    if s.count(a)!=1: raise RuntimeError(f'{p}: anchor count {s.count(a)} for {a[:100]!r}')
    write(p,s.replace(a,b))

# Preserve the validated InstallationId invariant during persistence deserialization.
p='src-tauri/crates/installations/src/lib.rs'
replace(p,'#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Ord, PartialOrd)]\n#[serde(transparent)]\npub struct InstallationId', '#[derive(Debug, Clone, PartialEq, Eq, Serialize, Ord, PartialOrd)]\n#[serde(transparent)]\npub struct InstallationId')
replace(p,'impl InstallationId {', '''pub mod steam;

impl<'de> Deserialize<'de> for InstallationId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?)
            .map_err(|_| serde::de::Error::custom("invalid installation ID"))
    }
}

impl InstallationId {''')
write('src-tauri/crates/installations/src/steam.rs',r'''//! Bounded Steam VDF discovery. Filesystem access belongs to the shell adapter.
use crate::HostOs;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: usize = 1024 * 1024;
const MAX_TOKENS: usize = 65_536;
const MAX_TOKEN_BYTES: usize = 4096;
const MAX_DEPTH: usize = 16;
const MAX_LIBRARIES: usize = 128;

pub fn unix_roots(host: HostOs, home: &Path, xdg_data_home: Option<&Path>) -> Vec<PathBuf> {
    if !home.is_absolute() { return Vec::new(); }
    if host == HostOs::Macos { return vec![home.join("Library/Application Support/Steam")]; }
    if host != HostOs::Linux { return Vec::new(); }
    let mut roots = Vec::new();
    if let Some(xdg) = xdg_data_home.filter(|path| path.is_absolute()) { roots.push(xdg.join("Steam")); }
    for relative in [".local/share/Steam", ".steam/steam", ".steam/root", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
        let path = home.join(relative);
        if !roots.contains(&path) { roots.push(path); }
    }
    roots
}

#[derive(Debug)]
enum Token { Text(String), Open, Close }
fn tokens(input: &str) -> Option<Vec<Token>> {
    if input.len() > MAX_BYTES || input.contains('\0') { return None; }
    let mut chars = input.chars().peekable();
    let mut result = Vec::new();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() { continue; }
        if ch == '/' && chars.peek() == Some(&'/') {
            for ch in chars.by_ref() { if ch == '\n' { break; } }
            continue;
        }
        let token = match ch {
            '{' => Token::Open,
            '}' => Token::Close,
            '"' => {
                let mut value = String::new();
                let mut closed = false;
                while let Some(ch) = chars.next() {
                    if ch == '"' { closed = true; break; }
                    if ch == '\\' {
                        let escaped = chars.next()?;
                        if escaped != '\\' && escaped != '"' { value.push('\\'); }
                        value.push(escaped);
                    } else { value.push(ch); }
                    if value.len() > MAX_TOKEN_BYTES { return None; }
                }
                if !closed { return None; }
                Token::Text(value)
            }
            _ => {
                let mut value = String::from(ch);
                while let Some(ch) = chars.peek().copied() {
                    if ch.is_whitespace() || matches!(ch, '{' | '}' | '"') { break; }
                    value.push(ch); chars.next();
                    if value.len() > MAX_TOKEN_BYTES { return None; }
                }
                Token::Text(value)
            }
        };
        if result.len() == MAX_TOKENS { return None; }
        result.push(token);
    }
    Some(result)
}
fn numeric(value: &str) -> bool { !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) }
fn parse(input: &str) -> Option<Vec<PathBuf>> {
    let tokens = tokens(input)?;
    let mut stack: Vec<&str> = Vec::new();
    let mut roots = Vec::new();
    let mut cursor = 0;
    while cursor < tokens.len() {
        match &tokens[cursor] {
            Token::Close => { stack.pop()?; cursor += 1; }
            Token::Open => return None,
            Token::Text(key) => {
                match tokens.get(cursor + 1)? {
                    Token::Open => {
                        if stack.len() == MAX_DEPTH { return None; }
                        stack.push(key);
                    }
                    Token::Close => return None,
                    Token::Text(value) => {
                        let modern = key.eq_ignore_ascii_case("path") && (stack.is_empty()
                            || (stack.len() == 2 && stack[0].eq_ignore_ascii_case("libraryfolders") && numeric(stack[1])));
                        let legacy = numeric(key) && stack.len() == 1 && stack[0].eq_ignore_ascii_case("libraryfolders");
                        if modern || legacy {
                            if value.is_empty() || value.chars().any(char::is_control) { return None; }
                            let path = PathBuf::from(value);
                            if !roots.contains(&path) {
                                if roots.len() == MAX_LIBRARIES { return None; }
                                roots.push(path);
                            }
                        }
                    }
                }
                cursor += 2;
            }
        }
    }
    stack.is_empty().then_some(roots)
}
/// Invalid or truncated manifests yield no partial path set.
pub fn library_roots(input: &str) -> Vec<PathBuf> { parse(input).unwrap_or_default() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_legacy_and_metadata_paths() {
        let input = r#""libraryfolders" { "0" { "path" "/one" "apps" { "path" "/not-a-library" } } "1" "/two" "2" { "path" "/one" } }"#;
        assert_eq!(library_roots(input), vec![PathBuf::from("/one"), PathBuf::from("/two")]);
        assert_eq!(library_roots(r#""path" "E:\\SteamLibrary""#), vec![PathBuf::from(r"E:\SteamLibrary")]);
        assert_eq!(library_roots(r#""path" "/a\"{b}" // ignored braces { }
"#), vec![PathBuf::from("/a\"{b}")]);
    }
    #[test]
    fn malformed_and_bounded_inputs_fail_without_partial_results() {
        for suffix in ["{", "}", "\"unfinished", "key", "key}", "\0"] {
            assert!(library_roots(&format!("\"path\" \"/valid\" {suffix}")).is_empty());
        }
        assert!(library_roots(&"x".repeat(MAX_BYTES + 1)).is_empty());
        assert!(library_roots(&format!("\"path\" \"/{}\"", "a".repeat(MAX_TOKEN_BYTES))).is_empty());
        assert!(library_roots(&format!("{}{}", "x { ".repeat(MAX_DEPTH + 1), "}".repeat(MAX_DEPTH + 1))).is_empty());
        let roots = (0..=MAX_LIBRARIES).map(|i| format!("\"path\" \"/{i}\" ")).collect::<String>();
        assert!(library_roots(&roots).is_empty());
        assert!(library_roots("\"path\" \"/line\nbreak\"").is_empty());
    }
    #[test]
    fn unix_candidates_are_absolute_and_deduplicated() {
        let home = std::env::temp_dir().join("steam-home");
        let xdg = home.join(".local/share");
        let linux = unix_roots(HostOs::Linux, &home, Some(&xdg));
        assert_eq!(linux.len(), 4);
        assert!(linux.contains(&home.join(".steam/root")));
        assert!(linux.contains(&home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")));
        assert_eq!(unix_roots(HostOs::Macos, &home, None), vec![home.join("Library/Application Support/Steam")]);
        assert!(unix_roots(HostOs::Linux, Path::new("relative"), None).is_empty());
    }
    #[test]
    fn deterministic_malformed_corpus_is_bounded() {
        let mut state = 0x51ea_u32;
        let alphabet = b"abc012 {}\"\\/\n\t";
        for i in 0..10_000 {
            let mut input = String::new();
            for _ in 0..i % 256 {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                input.push(alphabet[(state as usize) % alphabet.len()] as char);
            }
            assert!(library_roots(&input).len() <= MAX_LIBRARIES);
        }
    }
    #[test]
    fn deserialization_cannot_bypass_installation_id_validation() {
        use crate::InstallationId;
        for invalid in ["", "../outside", "a/b", "a\\b", "x:y", "with spaces", "ümlaut", "a\0b"] {
            assert!(serde_json::from_value::<InstallationId>(serde_json::json!(invalid)).is_err());
        }
        assert!(serde_json::from_value::<InstallationId>(serde_json::json!("a".repeat(129))).is_err());
        let valid = InstallationId::new("install-1_ok").unwrap();
        assert_eq!(serde_json::from_str::<InstallationId>(&serde_json::to_string(&valid).unwrap()).unwrap(), valid);
    }
}
''')

p='src-tauri/src/channels/workflows.rs'; s=read(p)
start=s.index('fn steam_library_roots('); end=s.index('fn schedule_restart(',start)
s=s[:start]+r'''fn steam_library_roots(contents: &str) -> Vec<PathBuf> {
    deltamod_installations_domain::steam::library_roots(contents)
}

fn push_existing_root(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if let Ok(canonical) = fs::canonicalize(path) {
        if canonical.is_dir() { push_unique(paths, canonical); }
    }
}

fn steam_common_folders() -> Vec<PathBuf> {
    use std::io::Read;
    let mut candidates = Vec::new();
    #[cfg(target_os = "windows")]
    {
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(variable) { candidates.push(PathBuf::from(root).join("Steam")); }
        }
        for letter in b'A'..=b'Z' {
            let drive = PathBuf::from(format!("{}:\\", char::from(letter)));
            if drive.is_dir() {
                for relative in ["Steam", "SteamLibrary", "Program Files (x86)\\Steam"] { candidates.push(drive.join(relative)); }
            }
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(home) = std::env::var_os("HOME") {
        use deltamod_installations_domain::{steam, HostOs};
        let host = if cfg!(target_os = "macos") { HostOs::Macos } else { HostOs::Linux };
        let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        candidates = steam::unix_roots(host, &PathBuf::from(home), xdg.as_deref());
    }
    let mut steam_roots = Vec::new();
    for candidate in candidates { push_existing_root(&mut steam_roots, candidate); }
    let mut library_roots = steam_roots.clone();
    for root in steam_roots {
        let manifest = root.join("steamapps/libraryfolders.vdf");
        let Ok(metadata) = fs::symlink_metadata(&manifest) else { continue; };
        let limit = deltamod_installations_domain::steam::MAX_BYTES as u64;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit { continue; }
        let Ok(file) = fs::File::open(manifest) else { continue; };
        let mut contents = String::new();
        if file.take(limit + 1).read_to_string(&mut contents).is_err() || contents.len() as u64 > limit { continue; }
        for library in steam_library_roots(&contents) {
            if library.is_absolute() { push_existing_root(&mut library_roots, library); }
        }
    }
    let mut common = Vec::new();
    for root in library_roots { push_existing_root(&mut common, root.join("steamapps/common")); }
    common
}

fn steam_source(game: &Value) -> Option<(PathBuf, String)> {
    let data = game.get("availableFeatures")?.as_array()?.iter()
        .find(|feature| feature.get("feat").and_then(Value::as_str) == Some("steam"))?.get("data")?;
    let folder = data.get("folder")?.as_str()?;
    let app_id = data.get("appid")?.as_str()?;
    if !safe_relative(folder) || app_id.is_empty() || app_id.len() > 10
        || !app_id.bytes().all(|byte| byte.is_ascii_digit())
        || app_id.parse::<u32>().ok().filter(|id| *id > 0).is_none() { return None; }
    let source = steam_common_folders().into_iter().map(|common| common.join(folder)).find(|candidate| candidate.is_dir())?;
    Some((source, app_id.to_owned()))
}

'''+s[end:]
s=s.replace('/// Dialog-owned Electron channels remain unavailable until a Tauri dialog integration exists.','/// Dialog and filesystem authority stay in the native adapter.')
write(p,s)

# Native keyring metadata failures must not destroy a previously valid credential.
p='src-tauri/crates/credentials/src/lib.rs'; s=read(p)
s=s.replace('use std::{collections::BTreeMap, fmt, sync::Arc};','use std::{collections::BTreeMap, fmt, sync::{Arc, Mutex}};')
s=s.replace('#[error("migration failed")]\n    Migration,','#[error("credential rollback failed; secure-store reconciliation is required")]\n    Recovery,')
start=s.index('pub struct CredentialStore<B: Backend>'); end=s.index('#[cfg(test)]',start)
s=s[:start]+r'''pub struct CredentialStore<B: Backend> {
    backend: Arc<B>,
    operation: Mutex<()>,
}
impl<B: Backend> CredentialStore<B> {
    pub fn new(backend: Arc<B>) -> Result<Self, Error> {
        let store = Self { backend, operation: Mutex::new(()) };
        store.ensure_metadata()?;
        Ok(store)
    }
    pub fn clear(&self, kind: CredentialKind) -> Result<(), Error> {
        let _guard = self.operation.lock().map_err(|_| Error::Backend)?;
        let mut metadata = self.status_unlocked()?;
        metadata.present.insert(kind.user().to_owned(), false);
        let encoded = serde_json::to_string(&metadata).map_err(|_| Error::Metadata)?;
        self.backend.delete(kind.user())?;
        // Never restore a token after an explicit logout, even if metadata fails.
        self.backend.set(METADATA_USER, &encoded)
    }
    pub fn status(&self) -> Result<CredentialMetadata, Error> {
        let _guard = self.operation.lock().map_err(|_| Error::Backend)?;
        self.status_unlocked()
    }
    fn status_unlocked(&self) -> Result<CredentialMetadata, Error> {
        let raw = self.backend.get(METADATA_USER)?.ok_or(Error::Metadata)?;
        if raw.len() > MAX_SECRET_BYTES { return Err(Error::Metadata); }
        let metadata: CredentialMetadata = serde_json::from_str(&raw).map_err(|_| Error::Metadata)?;
        if metadata.schema_version != SCHEMA_VERSION || metadata.present.keys().any(|key| !matches!(key.as_str(), "gamebanana-cookies" | "nexus-oauth-tokens" | "nexus-sso-key")) { return Err(Error::Metadata); }
        Ok(metadata)
    }
    pub fn store(&self, kind: CredentialKind, secret: Secret) -> Result<(), Error> {
        let _guard = self.operation.lock().map_err(|_| Error::Backend)?;
        let mut metadata = self.status_unlocked()?;
        metadata.present.insert(kind.user().to_owned(), true);
        let encoded = serde_json::to_string(&metadata).map_err(|_| Error::Metadata)?;
        let previous = self.backend.get(kind.user())?.map(Zeroizing::new);
        self.backend.set(kind.user(), secret.expose())?;
        if let Err(error) = self.backend.set(METADATA_USER, &encoded) {
            let restored = match previous {
                Some(previous) => self.backend.set(kind.user(), previous.as_str()),
                None => self.backend.delete(kind.user()),
            };
            return Err(if restored.is_ok() { error } else { Error::Recovery });
        }
        Ok(())
    }
    pub fn load(&self, kind: CredentialKind) -> Result<Option<Secret>, Error> {
        let _guard = self.operation.lock().map_err(|_| Error::Backend)?;
        self.status_unlocked()?;
        self.backend.get(kind.user())?.map(Secret::new).transpose()
    }
    fn ensure_metadata(&self) -> Result<(), Error> {
        if self.backend.get(METADATA_USER)?.is_none() {
            let metadata = CredentialMetadata { schema_version: SCHEMA_VERSION, present: BTreeMap::new() };
            let encoded = serde_json::to_string(&metadata).map_err(|_| Error::Metadata)?;
            self.backend.set(METADATA_USER, &encoded)?;
        }
        self.status_unlocked()?;
        Ok(())
    }
}

'''+s[end:]
s+='\n#[cfg(test)]\n#[path = "hardening_tests.rs"]\nmod hardening_tests;\n'; write(p,s)
write('src-tauri/crates/credentials/src/hardening_tests.rs',r'''use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Default)]
struct Store { values: Mutex<BTreeMap<String, String>>, fail_metadata: AtomicBool }
impl Backend for Store {
    fn get(&self, key: &str) -> Result<Option<String>, Error> { Ok(self.values.lock().unwrap().get(key).cloned()) }
    fn set(&self, key: &str, value: &str) -> Result<(), Error> {
        if key == METADATA_USER && self.fail_metadata.swap(false, Ordering::SeqCst) { return Err(Error::Unavailable); }
        self.values.lock().unwrap().insert(key.into(), value.into()); Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), Error> { self.values.lock().unwrap().remove(key); Ok(()) }
}
#[test]
fn failed_metadata_update_restores_existing_secret() {
    let backend = Arc::new(Store::default());
    let store = CredentialStore::new(Arc::clone(&backend)).unwrap();
    store.store(CredentialKind::GameBananaCookies, Secret::new("previous").unwrap()).unwrap();
    backend.fail_metadata.store(true, Ordering::SeqCst);
    assert!(store.store(CredentialKind::GameBananaCookies, Secret::new("replacement").unwrap()).is_err());
    assert_eq!(store.load(CredentialKind::GameBananaCookies).unwrap().unwrap().expose(), "previous");
    assert!(store.status().unwrap().present["gamebanana-cookies"]);
}
#[test]
fn failed_first_store_does_not_leave_an_unindexed_secret() {
    let backend = Arc::new(Store::default());
    let store = CredentialStore::new(Arc::clone(&backend)).unwrap();
    backend.fail_metadata.store(true, Ordering::SeqCst);
    assert!(store.store(CredentialKind::NexusOAuthTokens, Secret::new("new").unwrap()).is_err());
    assert!(store.load(CredentialKind::NexusOAuthTokens).unwrap().is_none());
}
#[test]
fn invalid_or_newer_metadata_is_not_rewritten() {
    for raw in [r#"{"schema_version":2,"present":{}}"#.to_owned(), r#"{"schema_version":1,"present":{"unknown":true}}"#.into(), "invalid".into(), " ".repeat(MAX_SECRET_BYTES + 1)] {
        let backend = Arc::new(Store::default());
        backend.set(METADATA_USER, &raw).unwrap();
        assert!(matches!(CredentialStore::new(Arc::clone(&backend)), Err(Error::Metadata)));
        assert_eq!(backend.get(METADATA_USER).unwrap().unwrap(), raw);
    }
}
#[test]
fn concurrent_provider_updates_preserve_both_metadata_entries() {
    let store = Arc::new(CredentialStore::new(Arc::new(Store::default())).unwrap());
    let handles = [CredentialKind::GameBananaCookies, CredentialKind::NexusOAuthTokens].map(|kind| {
        let store = Arc::clone(&store);
        std::thread::spawn(move || { for i in 0..100 { store.store(kind, Secret::new(format!("value-{i}")).unwrap()).unwrap(); } })
    });
    for handle in handles { handle.join().unwrap(); }
    let status = store.status().unwrap();
    assert!(status.present["gamebanana-cookies"] && status.present["nexus-oauth-tokens"]);
}
#[test]
fn logout_never_restores_a_deleted_token_when_metadata_fails() {
    let backend = Arc::new(Store::default());
    let store = CredentialStore::new(Arc::clone(&backend)).unwrap();
    store.store(CredentialKind::GameBananaCookies, Secret::new("token").unwrap()).unwrap();
    backend.fail_metadata.store(true, Ordering::SeqCst);
    assert!(store.clear(CredentialKind::GameBananaCookies).is_err());
    assert!(store.load(CredentialKind::GameBananaCookies).unwrap().is_none());
}
#[test]
fn unavailable_secure_store_does_not_write_a_fallback() {
    struct Unavailable;
    impl Backend for Unavailable {
        fn get(&self, _: &str) -> Result<Option<String>, Error> { Err(Error::Unavailable) }
        fn set(&self, _: &str, _: &str) -> Result<(), Error> { panic!("must not fall back") }
        fn delete(&self, _: &str) -> Result<(), Error> { panic!("must not mutate") }
    }
    assert!(matches!(CredentialStore::new(Arc::new(Unavailable)), Err(Error::Unavailable)));
}
''')

# Bounded native reads and owned launch finalization.
p='src-tauri/crates/updater-launch-runtime/src/lib.rs'; s=read(p)
s=s.replace('env::vars()\n        .filter', 'env::vars_os()\n        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))\n        .filter')
start=s.index('        let id = rest.split'); end=s.index('        Ok(Self(raw))',start)
s=s[:start]+'''        if rest.is_empty() || rest.len() > 10 || !rest.bytes().all(|byte| byte.is_ascii_digit())
            || rest.parse::<u32>().ok().filter(|id| *id > 0).is_none() {
            return Err(SteamError::InvalidUri);
        }
'''+s[end:]
a='''        command
            .status()
            .map(|_| ())
            .map_err(|e| SteamError::Io(e.to_string()))'''
assert a in s
s=s.replace(a,'''        let status = command.status().map_err(|e| SteamError::Io(e.to_string()))?;
        // Explorer can hand off successfully while returning a nonzero status.
        if cfg!(target_os = "windows") { Ok(()) } else { require_steam_success(status) }''')
s=s.replace('/// Host platform names intentionally match Node\'s `process.platform` values.', '''fn require_steam_success(status: ExitStatus) -> Result<(), SteamError> {
    if status.success() { Ok(()) } else { Err(SteamError::Io(format!("Steam opener exited with {status}"))) }
}

/// Host platform names retain the stable renderer wire values.''')
s=s.replace('.map_err(|_| GameError::CatalogUnavailable)?\n            .collect::<Result<Vec<_>, _>>()', '.map_err(|_| GameError::CatalogUnavailable)?\n            .take(MAX_CATALOG_FILES + 1)\n            .collect::<Result<Vec<_>, _>>()')
s=s.replace('    fn start_game(&self) -> Result<bool, GameError> {', '''    /// Steam handoffs have no owned child process and cannot finalize patch transactions.
    pub fn uses_external_launcher(&self) -> Result<bool, GameError> {
        Ok(self.store()?.get("isSteam").and_then(Value::as_bool) == Some(true))
    }

    fn start_game(&self) -> Result<bool, GameError> {''')
s=s.replace(' == Some(true)\n            && self.config.host == HostPlatform::Win32\n        {',' == Some(true) {')
s=s.replace('!id.is_empty() && id.len() <= 12', '!id.is_empty() && id.len() <= 10')
a='            self.steam.open(&uri).map_err(|_| GameError::LaunchFailed)?;'
assert a in s
s=s.replace(a,'''            let state = self.process.lock().map_err(|_| GameError::RuntimeUnavailable)?;
            if state.running { return Err(GameError::AlreadyRunning); }
            self.steam.open(&uri).map_err(|_| GameError::LaunchFailed)?;
            drop(state);''')
s=s.replace('''                if let Ok(mut state) = process.lock() {
                    if state.generation == generation {
                        state.running = false;
                    }
                }
                lifecycle.finished(success);''','''                // Finalization may restore patched files. Keep launch exclusion until it finishes.
                lifecycle.finished(success);
                if let Ok(mut state) = process.lock() {
                    if state.generation == generation { state.running = false; }
                }''')
a='''        if spawn_result.is_err() {
            if let Ok(mut state) = self.process.lock() {
                state.running = false;
            }
            if let Ok(mut slot) = child.lock() {
                if let Some(child) = slot.as_mut() {
                    let _ = child.kill();
                }
            }
            return Err(GameError::LaunchFailed);
        }'''
assert a in s
s=s.replace(a,'''        if spawn_result.is_err() {
            if let Ok(mut slot) = child.lock() {
                if let Some(mut child) = slot.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
            self.lifecycle.finished(false);
            if let Ok(mut state) = self.process.lock() {
                if state.generation == generation { state.running = false; }
            }
            return Err(GameError::LaunchFailed);
        }''')
a='    let bytes = fs::read(path).map_err(|_| invalid)?;'
assert a in s
s=s.replace(a,'''    use std::io::Read;
    let file = fs::File::open(path).map_err(|_| invalid)?;
    if !file.metadata().map_err(|_| invalid)?.is_file() { return Err(invalid); }
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes).map_err(|_| invalid)?;
    if bytes.len() as u64 > MAX_JSON_BYTES { return Err(invalid); }''')
s=s.replace('''            "linux" => LaunchSpec::new(Platform::Linux, "sh", &self.root)
                .map_err(|_| GameError::InvalidCatalog)
                .map(|spec| spec.arg(self.executable.to_string_lossy())),''','''            "linux" if self.executable.extension().is_some_and(|ext| ext == "sh") =>
                LaunchSpec::new(Platform::Linux, "sh", &self.root)
                    .map_err(|_| GameError::InvalidCatalog)
                    .map(|spec| spec.arg(self.executable.to_string_lossy())),
            "linux" => LaunchSpec::new(Platform::Linux, &self.executable, &self.root)
                .map_err(|_| GameError::InvalidCatalog),''')
s=s.replace('// Electron has no such handler. Do not silently give it startGame semantics.', '// This unrecognized command must not silently gain startGame semantics.')
s=s.replace('// The Electron handler is deliberately a no-op and resolves undefined.', '// Retained wire no-op for existing renderer startup ordering.')
s=s.replace('// Electron permits an arbitrary persisted command here. Reproducing that\n            // behavior would turn profile data into a command-execution primitive.', '// Persisted launcher commands are untrusted data, not executable configuration.')
s+='\n#[cfg(test)]\n#[path = "hardening_tests.rs"]\nmod hardening_tests;\n'; write(p,s)
p='src-tauri/src/state.rs'
replace(p,'''    fn steam_launched(&self) {
        self.0.exit(0);
    }''','''    fn steam_launched(&self) {
        // Keep recovery state and the manager alive after a non-owned Steam handoff.
        let _ = self.0.emit("page", "main");
    }''')
p='src-tauri/crates/patching-runtime/src/lib.rs'; s=read(p)
start=s.index('    pub fn patch_and_run('); brace=s.index('{\n',start)
assert 'game:' in s[start:brace], s[start:brace]
s=s[:brace+2]+'''        if game.uses_external_launcher().map_err(|error| Error::Launch(error.to_string()))? {
            return Err(Error::Launch("Transactional patch-and-run requires an owned game process. Steam launch lifetime cannot yet be observed; no game files were changed.".into()));
        }
'''+s[brace+2:]; write(p,s)
print('Native credentials, bounded Steam VDF discovery, launch exclusion and Linux binary dispatch implemented.')
