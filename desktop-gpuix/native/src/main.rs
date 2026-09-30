// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
#![forbid(unsafe_code)]

use deltamod_mods_themes_domain::ThemeId;
use deltamod_network_runtime::{Client, Provider};
use deltamod_storage_domain::{load_json, save_json, ProfileStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    fs,
    io::{self, BufRead, Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

const PROTOCOL: u32 = 1;
const MAX_REQUEST: usize = 64 * 1024;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
const MAX_JSON: u64 = 1024 * 1024;
const OWNER_MARKER: &str = ".deltamod-gpuix-preview";
type Result<T> = std::result::Result<T, String>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u32,
    id: u64,
    command: String,
    args: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    theme_id: String,
    accent: String,
    reduced_motion: bool,
    opaque: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme_id: "base".into(),
            accent: "#cd4451".into(),
            reduced_motion: true,
            opaque: true,
        }
    }
}
impl Preferences {
    fn validate(&self) -> Result<()> {
        ThemeId::new(&self.theme_id).map_err(|_| "Invalid theme ID".to_owned())?;
        if !is_color(&self.accent) {
            return Err("A six-digit hexadecimal accent color is required".into());
        }
        Ok(())
    }
}

fn is_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}
fn text(value: &Value, field: &str, fallback: &str, limit: usize) -> String {
    value.get(field).and_then(Value::as_str).unwrap_or(fallback)
        .chars().filter(|c| !c.is_control()).take(limit).collect()
}
fn linked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() { return true; }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return true; }
    }
    false
}
fn root(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() { return Err("An absolute directory is required".into()); }
    let metadata = fs::symlink_metadata(path).map_err(|_| "Directory is unavailable")?;
    if !metadata.is_dir() || linked(&metadata) { return Err("Linked or non-directory root refused".into()); }
    fs::canonicalize(path).map_err(|_| "Directory is unavailable".into())
}
fn contained(base: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else { return Err("Invalid relative path".into()); };
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if linked(&metadata) => return Err("Linked profile entry refused".into()),
            Ok(_) => {},
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(_) => return Err("Profile entry unavailable".into()),
        }
    }
    if path == base { return Err("A profile file is required".into()); }
    Ok(path)
}
fn read_json(base: &Path, relative: &Path) -> Result<Option<Value>> {
    let file = contained(base, relative)?;
    match fs::symlink_metadata(&file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Profile data cannot be read".into()),
        Ok(metadata) => {
            if !metadata.is_file() || metadata.len() > MAX_JSON { return Err("Invalid or oversized profile record".into()); }
            // Reuse the production bounded, root-relative regular-file reader.
            load_json(&file).map(Some).map_err(|_| "Invalid or unsafe profile JSON".into())
        }
    }
}
fn entries(base: &Path, relative: &str, limit: usize) -> Result<Vec<String>> {
    let directory = contained(base, Path::new(relative))?;
    if !directory.exists() { return Ok(Vec::new()); }
    let mut names = Vec::new();
    for entry in fs::read_dir(directory).map_err(|_| "Cannot inspect profile directory")?.take(limit + 1) {
        let entry = entry.map_err(|_| "Cannot inspect profile entry")?;
        let name = entry.file_name().into_string().map_err(|_| "Unsupported profile filename")?;
        if names.len() >= limit { return Err("Profile directory exceeds the preview's bounded listing limit".into()); }
        names.push(name);
    }
    names.sort();
    Ok(names)
}
fn prepare_state(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() { return Err("Absolute preview state root required".into()); }
    if !path.exists() { fs::create_dir_all(path).map_err(|_| "Cannot create preview state")?; }
    let state = root(path)?;
    let marker = state.join(OWNER_MARKER);
    if marker.exists() {
        let metadata = fs::symlink_metadata(&marker).map_err(|_| "Invalid ownership marker")?;
        if !metadata.is_file() || linked(&metadata) || metadata.len() != 2 {
            return Err("Invalid preview ownership marker".into());
        }
        let contents = fs::read(&marker).map_err(|_| "Cannot read ownership marker")?;
        if contents != b"1\n" { return Err("Unknown preview state schema".into()); }
    } else {
        if fs::read_dir(&state).map_err(|_| "Cannot read preview state")?.next().is_some() {
            return Err("Refusing to use an existing non-preview data directory".into());
        }
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(marker)
            .map_err(|_| "Cannot establish preview state ownership")?;
        file.write_all(b"1\n").and_then(|_| file.sync_all()).map_err(|_| "Cannot save preview ownership")?;
    }
    Ok(state)
}
fn validate_source(state: &Path, path: &Path) -> Result<PathBuf> {
    let source = root(path)?;
    if source.starts_with(state) || state.starts_with(&source) {
        return Err("Source profile and preview state must be separate directories".into());
    }
    let recognizable = source.join("profiles").join("installations.json").is_file()
        || source.join("deltamod_system-0").is_dir()
        || source.join("packets").is_dir()
        || source.join("runtime").is_dir();
    if !recognizable { return Err("Select a Deltamod data folder, not the game or program folder".into()); }
    Ok(source)
}

struct Backend {
    state: PathBuf,
    resources: PathBuf,
    source: Option<PathBuf>,
    selected_installation: Option<String>,
    prefs: Preferences,
    network: Option<(tokio::runtime::Runtime, Client)>,
}
impl Backend {
    fn new(state: PathBuf, resources: PathBuf, source: Option<PathBuf>) -> Result<Self> {
        let state = prepare_state(&state)?;
        let resources = root(&resources)?;
        let source = source.map(|path| validate_source(&state, &path)).transpose()?;
        let prefs = match read_json(&state, Path::new("preferences.json"))? {
            Some(value) => serde_json::from_value(value).map_err(|_| "Invalid preview preferences")?,
            None => Preferences::default(),
        };
        let backend = Self { state, resources, source, selected_installation: None, prefs, network: None };
        backend.prefs.validate()?;
        Ok(backend)
    }
    fn snapshot(&self) -> Result<Value> {
        let mut warnings = Vec::<String>::new();
        let mut installations = Vec::new();
        let mut mods = Vec::new();
        if let Some(source) = &self.source {
            let profile = read_json(source, Path::new("profiles/installations.json"))?;
            // Deserialize the same registry schema as Tauri. Corrupt registries
            // must not silently select a different installation.
            let profile: ProfileStore = match profile {
                Some(value) => serde_json::from_value(value).map_err(|_| "Invalid installation registry")?,
                None => ProfileStore::default(),
            };
            let current = profile.current_index.unwrap_or(0);
            if profile.installations.len() > 128 { return Err("Installation registry exceeds the 128-record preview limit".into()); }
            let indices: Vec<u32> = if profile.installations.is_empty() { vec![current] } else {
                profile.installations.iter().enumerate().map(|(i, record)| record.index.unwrap_or(i as u32)).collect()
            };
            let mut unique = HashSet::new();
            if indices.iter().any(|index| !unique.insert(*index)) {
                return Err("Installation registry contains duplicate indices".into());
            }
            for index in indices {
                let relative = PathBuf::from(format!("deltamod_system-{index}/store.json"));
                if let Some(store) = read_json(source, &relative)? {
                    if !store.is_object() { return Err("Invalid installation store".into()); }
                    let game_id = text(&store, "gamePid", "", 120);
                    let game_path = text(&store, "gamePath", "", 4096);
                    installations.push(json!({
                        "id": index.to_string(), "name": text(&store, "customName",
                            profile.installations.iter().find(|record| record.index == Some(index))
                                .and_then(|record| record.name.as_deref()).unwrap_or(&game_id), 240),
                        "gameId": game_id, "path": game_path,
                        "current": index == current,
                        // Saved game paths are display-only here. Do not probe arbitrary
                        // paths (including network shares) supplied by a profile.
                        "available": Value::Null
                    }));
                }
            }
            let enabled = read_json(source, Path::new("runtime/mods-state.json"))?.unwrap_or(Value::Null);
            let enabled_known = enabled.get("enabled").and_then(Value::as_array)
                .is_some_and(|values| values.iter().all(Value::is_string));
            if !enabled.is_null() && !enabled_known {
                warnings.push("Invalid enabled-state record. Mod enabled states are unavailable.".into());
            }
            let enabled: HashSet<String> = enabled.get("enabled").and_then(Value::as_array)
                .into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
            for (directory, manifest, format) in [("mods", "manifest.json", "runtime"), ("packets", "__deltaID.json", "legacy packet")] {
                match entries(source, directory, 2000) {
                    Err(error) => warnings.push(error),
                    Ok(names) => for name in names {
                        if mods.len() >= 500 { warnings.push("Only the first 500 mods are shown".into()); break; }
                        let relative = PathBuf::from(directory).join(&name).join(manifest);
                        match read_json(source, &relative) {
                            Ok(Some(record)) => {
                                let id = text(&record, if directory == "mods" { "uid" } else { "uniqueId" }, &name, 240);
                                mods.push(json!({ "id": id, "name": text(&record, "name", &name, 240),
                                    "description": text(&record, "description", "", 600), "enabled": if enabled_known && directory == "mods" { Some(enabled.contains(&id)) } else { None }, "format": format }));
                            }
                            Ok(None) => {},
                            Err(_) => if warnings.len() < 10 { warnings.push("Skipped an invalid or unsafe mod record".into()); },
                        }
                    }
                }
            }
        }
        let selected_id = match &self.selected_installation {
            Some(id) if installations.iter().any(|item| item["id"].as_str() == Some(id.as_str())) => Some(id.clone()),
            Some(_) => { warnings.push("The selected installation is no longer in this profile. Choose another installation.".into()); None },
            None => installations.iter().find(|item| item["current"] == true)
                .and_then(|item| item["id"].as_str()).map(str::to_owned),
        };
        for item in &mut installations {
            item["selected"] = json!(item["id"].as_str() == selected_id.as_deref());
        }
        let mut themes = Vec::new();
        for name in entries(&self.resources, "web/themes/data", 256)? {
            let Some(id) = name.strip_suffix(".theme.json") else { continue; };
            if ThemeId::new(id).is_err() { continue; }
            if let Some(theme) = read_json(&self.resources, &PathBuf::from("web/themes/data").join(&name))? {
                let accent = text(&theme, "soulColor", &text(&theme, "color", "#cd4451", 20), 20);
                themes.push(json!({ "id": id, "name": text(&theme, "name", id, 240),
                    "accent": if is_color(&accent) { accent } else { "#cd4451".into() } }));
            }
        }
        if themes.is_empty() { themes.push(json!({ "id": "base", "name": "Deltamod", "accent": "#cd4451" })); }
        Ok(json!({ "sourceAttached": self.source.is_some(), "readOnly": true,
            "installations": installations, "mods": mods, "themes": themes,
            "selectedInstallationId": selected_id, "games": self.game_catalog()?,
            "preferences": self.prefs, "warnings": warnings.into_iter().take(12).collect::<Vec<_>>() }))
    }
    fn game_catalog(&self) -> Result<Vec<Value>> {
        let mut games = Vec::new();
        for name in entries(&self.resources, "games", 256)? {
            let Some(id) = name.strip_suffix(".json") else { continue; };
            if !valid_game_id(id) { continue; }
            let game = read_json(&self.resources, &PathBuf::from("games").join(&name))?
                .ok_or("Packaged game catalogue changed during reading")?;
            if game.get("id").and_then(Value::as_str) != Some(id) {
                return Err("Packaged game catalogue ID does not match its filename".into());
            }
            let provider = game.pointer("/gamebanana/id").and_then(Value::as_u64)
                .filter(|id| *id > 0 && *id <= 9_007_199_254_740_991);
            games.push(json!({ "id": id, "name": text(&game, "name", id, 240),
                "gamebanana": provider.is_some() }));
        }
        Ok(games)
    }
    fn select_installation(&mut self, args: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Select { id: String }
        let args: Select = serde_json::from_value(args).map_err(|_| "Invalid installation selection")?;
        if self.source.is_none() { return Err("Attach a profile before choosing an installation".into()); }
        if args.id.len() > 10 || args.id.parse::<u32>().is_err() {
            return Err("Invalid installation ID".into());
        }
        let mut snapshot = self.snapshot()?;
        let records = snapshot["installations"].as_array_mut().ok_or("Invalid installation snapshot")?;
        if !records.iter().any(|item| item["id"].as_str() == Some(args.id.as_str())) {
            return Err("Installation is not present in the attached profile".into());
        }
        for item in records {
            item["selected"] = json!(item["id"].as_str() == Some(args.id.as_str()));
        }
        // A preview selection does not call changeSystemIndex or persist a store.
        self.selected_installation = Some(args.id.clone());
        snapshot["selectedInstallationId"] = json!(args.id);
        Ok(snapshot)
    }
    fn browse(&mut self, args: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Browse { query: String, page: u32, game_id: String }
        let request: Browse = serde_json::from_value(args).map_err(|_| "Invalid browse request")?;
        if request.query.len() > 512 || request.query.chars().count() > 128 || request.query.chars().any(char::is_control)
            || !(1..=100).contains(&request.page) { return Err("Invalid browse query or page".into()); }
        if !valid_game_id(&request.game_id) { return Err("Invalid catalogue game ID".into()); }
        let game = read_json(&self.resources, &PathBuf::from("games").join(format!("{}.json", request.game_id)))?
            .ok_or("Selected game is not in the packaged catalogue")?;
        if game.get("id").and_then(Value::as_str) != Some(request.game_id.as_str()) {
            return Err("Packaged game catalogue ID mismatch".into());
        }
        let game_id = game.pointer("/gamebanana/id").and_then(Value::as_u64).filter(|id| *id > 0 && *id <= 9_007_199_254_740_991)
            .ok_or("GameBanana is not configured for this game")?;
        let query = request.query.trim();
        let raw = if query.is_empty() { format!("https://gamebanana.com/apiv11/Game/{game_id}/Subfeed") }
            else { "https://gamebanana.com/apiv11/Util/Search/Results".into() };
        let mut url = url::Url::parse(&raw).map_err(|_| "Invalid provider endpoint")?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("_nPage", &request.page.to_string()).append_pair("_nPerpage", "24");
            if query.is_empty() { pairs.append_pair("_aModelName", "Mod"); }
            else { pairs.append_pair("_idGameRow", &game_id.to_string()).append_pair("_sModelName", "Mod").append_pair("_sSearchString", query); }
        }
        if self.network.is_none() {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "Network runtime unavailable")?;
            let client = Client::new(Duration::from_secs(12), 1, Duration::from_millis(100)).map_err(|_| "Network client unavailable")?;
            self.network = Some((runtime, client));
        }
        let (runtime, client) = self.network.as_ref().ok_or("Network client unavailable")?;
        // Production URL containment, redirect checks, timeouts and 4 MiB response
        // bound remain owned by the existing Rust network runtime.
        let response: Value = runtime.block_on(client.json(Provider::GameBanana, url.as_str(), None))
            .map_err(|_| "GameBanana could not be loaded. Check the connection and retry.")?;
        let rows = response.get("_aRecords").and_then(Value::as_array).or_else(|| response.as_array())
            .ok_or("GameBanana returned an unsupported catalogue format")?;
        let items: Vec<Value> = rows.iter().take(24).filter_map(|row| {
            let id = row.get("_idRow").and_then(Value::as_u64).filter(|id| *id > 0)?;
            if row.get("_sModelName").and_then(Value::as_str).is_some_and(|model| model != "Mod") { return None; }
            Some(json!({ "id": id.to_string(), "name": text(row, "_sName", "Untitled mod", 240),
                "author": row.pointer("/_aSubmitter/_sName").and_then(Value::as_str).unwrap_or("").chars().take(240).collect::<String>(),
                "description": text(row, "_sDescription", "", 400), "url": format!("https://gamebanana.com/mods/{id}") }))
        }).collect();
        Ok(json!({ "items": items, "hasMore": rows.len() == 24 && request.page < 100 }))
    }
    fn dispatch(&mut self, request: Request) -> Result<Value> {
        if !request.args.is_object() { return Err("Object arguments required".into()); }
        match request.command.as_str() {
            "hello" => Ok(json!({ "protocol": PROTOCOL, "readOnly": true,
                "workspaceVersion": 2,
                "capabilities": ["snapshot", "profile.attach", "profile.detach", "installation.select", "shop.browse", "preferences.set"],
                "runtime": "Rust stdio, no Tauri or WebView", "version": env!("CARGO_PKG_VERSION") })),
            "snapshot" => self.snapshot(),
            "installation.select" => self.select_installation(request.args),
            "profile.detach" => {
                if request.args.as_object().is_none_or(|args| !args.is_empty()) {
                    return Err("Profile detach does not accept arguments".into());
                }
                let previous = self.source.take();
                let selected = self.selected_installation.take();
                match self.snapshot() {
                    Ok(value) => Ok(value),
                    Err(error) => { self.source = previous; self.selected_installation = selected; Err(error) }
                }
            }
            "profile.attach" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Attach { path: String }
                let args: Attach = serde_json::from_value(request.args).map_err(|_| "Invalid profile request")?;
                if args.path.is_empty() || args.path.len() > 16384 { return Err("Invalid profile directory".into()); }
                let source = validate_source(&self.state, Path::new(&args.path))?;
                let previous = self.source.replace(source);
                let selected = self.selected_installation.take();
                match self.snapshot() {
                    Ok(value) => Ok(value),
                    Err(error) => { self.source = previous; self.selected_installation = selected; Err(error) }
                }
            }
            "preferences.set" => {
                let preferences: Preferences = serde_json::from_value(request.args).map_err(|_| "Invalid preferences")?;
                preferences.validate()?;
                let file = contained(&self.state, Path::new("preferences.json"))?;
                save_json(&file, &preferences, true).map_err(|_| "Could not save preview preferences")?;
                self.prefs = preferences;
                Ok(json!(self.prefs))
            }
            "shop.browse" => self.browse(request.args),
            _ => Err("Command is not available in the read-only GPUIX preview".into()),
        }
    }
}

fn valid_game_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 120 && id.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric())
        && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && id.split('.').all(|part| !part.is_empty())
}

fn read_request(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let count = reader.take((MAX_REQUEST + 1) as u64).read_until(b'\n', &mut line)
        .map_err(|_| "Cannot read request")?;
    if count == 0 { return Ok(None); }
    if line.len() > MAX_REQUEST || !line.ends_with(b"\n") { return Err("Request frame exceeded its bound or was incomplete".into()); }
    Ok(Some(line))
}
fn respond(writer: &mut impl Write, id: u64, result: Result<Value>) -> Result<()> {
    let value = match result {
        Ok(result) => json!({ "v": PROTOCOL, "id": id, "ok": true, "result": result }),
        Err(error) => json!({ "v": PROTOCOL, "id": id, "ok": false, "error": error.chars().take(512).collect::<String>() }),
    };
    let mut bytes = serde_json::to_vec(&value).map_err(|_| "Cannot encode response")?;
    if bytes.len() > MAX_RESPONSE {
        bytes = serde_json::to_vec(&json!({ "v": PROTOCOL, "id": id, "ok": false, "error": "Snapshot exceeded the preview response limit" }))
            .map_err(|_| "Cannot encode response")?;
    }
    bytes.push(b'\n');
    writer.write_all(&bytes).and_then(|_| writer.flush()).map_err(|_| "Native response pipe closed".into())
}
fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let (mut state, mut resources, mut source) = (None, None, None);
    while let Some(flag) = args.next() {
        let target = match flag.to_str() {
            Some("--state-root") => &mut state,
            Some("--resources-root") => &mut resources,
            Some("--source-profile") => &mut source,
            _ => return Err("Unknown native backend argument".into()),
        };
        if target.is_some() { return Err("Duplicate native backend argument".into()); }
        *target = Some(PathBuf::from(args.next().ok_or("Missing native backend argument")?));
    }
    let mut backend = Backend::new(state.ok_or("--state-root is required")?, resources.ok_or("--resources-root is required")?, source)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();
    let mut last_id = 0;
    while let Some(line) = read_request(&mut reader)? {
        let request: Request = serde_json::from_slice(&line).map_err(|_| "Invalid request JSON")?;
        if request.v != PROTOCOL || request.id <= last_id || request.id > 9_007_199_254_740_991 || request.command.len() > 64 {
            return Err("Incompatible protocol or request ID".into());
        }
        last_id = request.id;
        respond(&mut writer, last_id, backend.dispatch(request))?;
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("GPUIX backend: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("deltamod-gpuix-test-{}-{}", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn refuses_live_or_nonempty_state_without_ownership() {
        let temp = Temp::new();
        fs::write(temp.0.join("preferences.json"), b"user data").unwrap();
        assert!(prepare_state(&temp.0).is_err());
        assert_eq!(fs::read(temp.0.join("preferences.json")).unwrap(), b"user data");
    }
    #[test]
    fn protocol_is_bounded_and_requires_complete_frames() {
        assert!(read_request(&mut io::Cursor::new(vec![b'x'; MAX_REQUEST + 5])).is_err());
        assert!(read_request(&mut io::Cursor::new(b"{}".to_vec())).is_err());
        assert_eq!(read_request(&mut io::Cursor::new(b"{}\n".to_vec())).unwrap(), Some(b"{}\n".to_vec()));
    }
    #[test]
    fn preferences_validate_the_existing_theme_domain() {
        let mut prefs = Preferences::default();
        assert!(prefs.validate().is_ok());
        prefs.accent = "url(secret)".into();
        assert!(prefs.validate().is_err());
        prefs.accent = "#ffffff".into();
        prefs.theme_id = "../outside".into();
        assert!(prefs.validate().is_err());
    }
    #[test]
    fn source_and_state_must_never_overlap() {
        let temp = Temp::new();
        let state = prepare_state(&temp.0).unwrap();
        assert!(validate_source(&state, &state).is_err());
        assert!(contained(&state, Path::new("../preferences.json")).is_err());
    }
    #[test]
    fn snapshot_and_preferences_do_not_write_to_source() {
        let state = Temp::new();
        let resources = Temp::new();
        let source = Temp::new();
        fs::create_dir(source.0.join("deltamod_system-0")).unwrap();
        let original = br##"{"gamePid":"toby.deltarune","gamePath":"","customName":"My game"}"##;
        let store = source.0.join("deltamod_system-0/store.json");
        fs::write(&store, original).unwrap();
        let mut backend = Backend::new(state.0.clone(), resources.0.clone(), Some(source.0.clone())).unwrap();
        let snapshot = backend.snapshot().unwrap();
        assert_eq!(snapshot["installations"][0]["name"], "My game");
        let request = Request { v: 1, id: 1, command: "preferences.set".into(), args: json!(Preferences::default()) };
        backend.dispatch(request).unwrap();
        assert_eq!(fs::read(store).unwrap(), original);
        assert!(!source.0.join("preferences.json").exists());
        assert!(!source.0.join("runtime").exists());
        assert!(state.0.join("preferences.json").exists());
    }

    fn two_installations(source: &Path) -> Vec<u8> {
        fs::create_dir_all(source.join("profiles")).unwrap();
        let registry = br#"{"current_index":0,"installations":[{"index":0},{"index":1}]}"#.to_vec();
        fs::write(source.join("profiles/installations.json"), &registry).unwrap();
        for (index, game) in [(0, "toby.deltarune"), (1, "toby.undertale")] {
            fs::create_dir_all(source.join(format!("deltamod_system-{index}"))).unwrap();
            fs::write(source.join(format!("deltamod_system-{index}/store.json")),
                serde_json::to_vec(&json!({"gamePid": game, "gamePath": "", "customName": game})).unwrap()).unwrap();
        }
        registry
    }

    #[test]
    fn preview_selection_and_detach_leave_the_source_registry_unchanged() {
        let state = Temp::new(); let resources = Temp::new(); let source = Temp::new();
        let registry = two_installations(&source.0);
        let mut backend = Backend::new(state.0.clone(), resources.0.clone(), Some(source.0.clone())).unwrap();
        assert_eq!(backend.snapshot().unwrap()["selectedInstallationId"], "0");
        let selected = backend.select_installation(json!({ "id": "1" })).unwrap();
        assert_eq!(selected["selectedInstallationId"], "1");
        assert_eq!(selected["installations"][0]["current"], true);
        assert_eq!(selected["installations"][1]["selected"], true);
        assert!(backend.select_installation(json!({ "id": "2" })).is_err());
        assert!(backend.select_installation(json!({ "id": "../0" })).is_err());
        assert_eq!(backend.selected_installation.as_deref(), Some("1"));
        let detached = backend.dispatch(Request { v: 1, id: 1, command: "profile.detach".into(), args: json!({}) }).unwrap();
        assert_eq!(detached["sourceAttached"], false);
        assert_eq!(detached["installations"], json!([]));
        assert_eq!(detached["mods"], json!([]));
        assert!(backend.source.is_none()); assert!(backend.selected_installation.is_none());
        assert_eq!(fs::read(source.0.join("profiles/installations.json")).unwrap(), registry);
        assert!(!source.0.join(OWNER_MARKER).exists());
        assert!(backend.select_installation(json!({ "id": "0" })).is_err());
    }

    #[test]
    fn a_rejected_profile_keeps_the_previous_source_and_selection() {
        let state = Temp::new(); let resources = Temp::new(); let source = Temp::new(); let bad = Temp::new();
        two_installations(&source.0); two_installations(&bad.0);
        fs::write(bad.0.join("profiles/installations.json"), br#"{"installations":"broken"}"#).unwrap();
        let mut backend = Backend::new(state.0.clone(), resources.0.clone(), Some(source.0.clone())).unwrap();
        backend.select_installation(json!({ "id": "1" })).unwrap();
        let result = backend.dispatch(Request { v: 1, id: 1, command: "profile.attach".into(), args: json!({ "path": bad.0 }) });
        assert!(result.is_err());
        assert_eq!(backend.source.as_ref(), Some(&fs::canonicalize(&source.0).unwrap()));
        assert_eq!(backend.selected_installation.as_deref(), Some("1"));
    }

    #[test]
    fn attaching_a_new_profile_resets_only_the_session_selection() {
        let state = Temp::new(); let resources = Temp::new(); let first = Temp::new(); let second = Temp::new();
        two_installations(&first.0); two_installations(&second.0);
        let mut backend = Backend::new(state.0.clone(), resources.0.clone(), Some(first.0.clone())).unwrap();
        backend.select_installation(json!({ "id": "1" })).unwrap();
        let snapshot = backend.dispatch(Request { v: 1, id: 1, command: "profile.attach".into(), args: json!({ "path": second.0 }) }).unwrap();
        assert_eq!(snapshot["selectedInstallationId"], "0");
        assert!(backend.selected_installation.is_none());
        assert!(!first.0.join("preferences.json").exists());
        assert!(!second.0.join("preferences.json").exists());
    }

    #[test]
    fn duplicate_registry_indices_and_non_object_stores_are_errors() {
        let state = Temp::new(); let resources = Temp::new(); let source = Temp::new();
        let original = two_installations(&source.0);
        let backend = Backend::new(state.0.clone(), resources.0.clone(), Some(source.0.clone())).unwrap();
        fs::write(source.0.join("profiles/installations.json"), br#"{"installations":[{"index":0},{"index":0}],"current_index":0}"#).unwrap();
        assert!(backend.snapshot().unwrap_err().contains("duplicate"));
        fs::write(source.0.join("profiles/installations.json"), b"null").unwrap();
        assert!(backend.snapshot().is_err());
        fs::write(source.0.join("profiles/installations.json"), original).unwrap();
        fs::write(source.0.join("deltamod_system-0/store.json"), b"[]").unwrap();
        assert!(backend.snapshot().is_err());
    }

    #[test]
    fn game_catalogue_mappings_are_local_and_game_ids_cannot_escape_resources() {
        let state = Temp::new(); let resources = Temp::new();
        fs::create_dir(resources.0.join("games")).unwrap();
        fs::write(resources.0.join("games/toby.undertale.json"), br#"{"id":"toby.undertale","name":"UNDERTALE","gamebanana":{"id":591}}"#).unwrap();
        fs::write(resources.0.join("games/local.game.json"), br#"{"id":"local.game","name":"Local"}"#).unwrap();
        let mut backend = Backend::new(state.0.clone(), resources.0.clone(), None).unwrap();
        let games = backend.game_catalog().unwrap();
        assert!(games.iter().any(|game| game["id"] == "toby.undertale" && game["gamebanana"] == true));
        assert!(games.iter().any(|game| game["id"] == "local.game" && game["gamebanana"] == false));
        for game_id in ["../outside", "file:///outside", "a..b", "missing.game", "local.game"] {
            assert!(backend.browse(json!({ "query": "", "page": 1, "gameId": game_id })).is_err());
        }
        assert!(backend.network.is_none());
        fs::write(resources.0.join("games/other.json"), br#"{"id":"different"}"#).unwrap();
        assert!(backend.game_catalog().is_err());
    }

    #[test]
    fn malformed_enabled_states_are_unknown_not_disabled() {
        let state = Temp::new(); let resources = Temp::new(); let source = Temp::new();
        two_installations(&source.0);
        fs::create_dir_all(source.0.join("mods/sample")).unwrap();
        fs::create_dir(source.0.join("runtime")).unwrap();
        fs::write(source.0.join("mods/sample/manifest.json"), br#"{"uid":"sample","name":"Sample"}"#).unwrap();
        fs::write(source.0.join("runtime/mods-state.json"), br#"{"enabled":[null]}"#).unwrap();
        let backend = Backend::new(state.0.clone(), resources.0.clone(), Some(source.0.clone())).unwrap();
        let snapshot = backend.snapshot().unwrap();
        assert!(snapshot["mods"][0]["enabled"].is_null());
        assert!(!snapshot["warnings"].as_array().unwrap().is_empty());
    }
}
