// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
#![forbid(unsafe_code)]

use deltamod_mods_themes_domain::ThemeId;
use deltamod_network_runtime::{Client, Provider};
use deltamod_storage_domain::{load_json, save_json};
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
        let backend = Self { state, resources, source, prefs, network: None };
        backend.prefs.validate()?;
        Ok(backend)
    }
    fn snapshot(&self) -> Result<Value> {
        let mut warnings = Vec::<String>::new();
        let mut installations = Vec::new();
        let mut mods = Vec::new();
        if let Some(source) = &self.source {
            let profile = read_json(source, Path::new("profiles/installations.json"))?.unwrap_or(Value::Null);
            let current = profile.get("current_index").and_then(Value::as_u64).unwrap_or(0);
            let records = profile.get("installations").and_then(Value::as_array).cloned().unwrap_or_default();
            if records.len() > 128 { warnings.push("Only the first 128 installations are shown".into()); }
            let indices: Vec<u64> = if records.is_empty() { vec![current] } else {
                records.iter().take(128).enumerate().map(|(i, record)| record.get("index")
                    .and_then(Value::as_u64).unwrap_or(i as u64)).collect()
            };
            for index in indices {
                if index > u32::MAX as u64 { continue; }
                let relative = PathBuf::from(format!("deltamod_system-{index}/store.json"));
                if let Some(store) = read_json(source, &relative)? {
                    let game_id = text(&store, "gamePid", "", 120);
                    let game_path = text(&store, "gamePath", "", 4096);
                    installations.push(json!({
                        "id": index.to_string(), "name": text(&store, "customName", &game_id, 240),
                        "gameId": game_id, "path": game_path,
                        "current": index == current,
                        // Saved game paths are display-only here. Do not probe arbitrary
                        // paths (including network shares) supplied by a profile.
                        "available": Value::Null
                    }));
                }
            }
            let enabled = read_json(source, Path::new("runtime/mods-state.json"))?.unwrap_or(Value::Null);
            let enabled_known = enabled.get("enabled").is_some_and(Value::is_array);
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
            "preferences": self.prefs, "warnings": warnings.into_iter().take(12).collect::<Vec<_>>() }))
    }
    fn browse(&mut self, args: Value) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Browse { query: String, page: u32 }
        let request: Browse = serde_json::from_value(args).map_err(|_| "Invalid browse request")?;
        if request.query.len() > 512 || request.query.chars().count() > 128 || request.query.chars().any(char::is_control)
            || !(1..=100).contains(&request.page) { return Err("Invalid browse query or page".into()); }
        let game = read_json(&self.resources, Path::new("games/toby.deltarune.json"))?
            .ok_or("Packaged DELTARUNE catalogue is missing")?;
        let game_id = game.pointer("/gamebanana/id").and_then(Value::as_u64).filter(|id| *id > 0)
            .ok_or("GameBanana mapping is missing")?;
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
        Ok(json!({ "items": items, "hasMore": rows.len() == 24 }))
    }
    fn dispatch(&mut self, request: Request) -> Result<Value> {
        if !request.args.is_object() { return Err("Object arguments required".into()); }
        match request.command.as_str() {
            "hello" => Ok(json!({ "protocol": PROTOCOL, "readOnly": true,
                "capabilities": ["snapshot", "profile.attach", "shop.browse", "preferences.set"],
                "runtime": "Rust stdio, no Tauri or WebView", "version": env!("CARGO_PKG_VERSION") })),
            "snapshot" => self.snapshot(),
            "profile.attach" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Attach { path: String }
                let args: Attach = serde_json::from_value(request.args).map_err(|_| "Invalid profile request")?;
                if args.path.is_empty() || args.path.len() > 16384 { return Err("Invalid profile directory".into()); }
                let source = validate_source(&self.state, Path::new(&args.path))?;
                let previous = self.source.replace(source);
                match self.snapshot() {
                    Ok(value) => Ok(value),
                    Err(error) => { self.source = previous; Err(error) }
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
}
