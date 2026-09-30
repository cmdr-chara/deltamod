// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
//! Read-only presentation services. Only interface preferences write to the
//! separately owned preview state directory. No source profile is accepted here.
use super::{contained, linked, read_json, text, Result};
use deltamod_mods_themes_domain::ThemeId;
use deltamod_network_runtime::{Client, Provider};
use deltamod_storage_domain::save_json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}, time::Duration};

const MAX_ID: u64 = 9_007_199_254_740_991;
const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
const INTERFACE_FILE: &str = "interface.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InterfacePreferences {
    schema_version: u32,
    locale: String,
    theme_images: bool,
}
impl Default for InterfacePreferences {
    fn default() -> Self {
        Self { schema_version: 1, locale: "en".into(), theme_images: false }
    }
}
impl InterfacePreferences {
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || !matches!(self.locale.as_str(), "en" | "it" | "de" | "es" | "fr" | "ja" | "pl" | "pt-br") {
            return Err("Invalid interface preferences or unsupported language".into());
        }
        Ok(())
    }
}

fn empty_args(args: &Value) -> Result<()> {
    if args.as_object().is_some_and(|args| args.is_empty()) { Ok(()) }
    else { Err("This command does not accept arguments".into()) }
}

fn interface_preferences(state: &Path, args: &Value, write: bool) -> Result<Value> {
    let preferences = if write {
        serde_json::from_value::<InterfacePreferences>(args.clone())
            .map_err(|_| "Invalid interface preferences")?
    } else {
        empty_args(args)?;
        match read_json(state, Path::new(INTERFACE_FILE))? {
            Some(value) => serde_json::from_value(value).map_err(|_| "Invalid saved interface preferences")?,
            None => InterfacePreferences::default(),
        }
    };
    preferences.validate()?;
    if write {
        let file = contained(state, Path::new(INTERFACE_FILE))?;
        save_json(&file, &preferences, true).map_err(|_| "Could not save interface preferences")?;
    }
    Ok(json!(preferences))
}

fn theme_color(value: &Value) -> Option<String> {
    let value = value.as_str()?.trim();
    if value.len() == 7 && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit) {
        return Some(value.to_ascii_lowercase());
    }
    let rgb = value.strip_prefix("rgb(")?.strip_suffix(')')?;
    let parts = rgb.split(',').map(|part| part.trim().parse::<u8>())
        .collect::<std::result::Result<Vec<_>, _>>().ok()?;
    if parts.len() != 3 { return None; }
    Some(format!("#{:02x}{:02x}{:02x}", parts[0], parts[1], parts[2]))
}

// Explicit, bounded theme semantics. CSS and media filenames are never passed
// to a browser, shell, URL loader or arbitrary frontend filesystem bridge.
fn theme_metadata(theme: &Value) -> Value {
    let credits: Vec<Value> = theme.get("credits").and_then(Value::as_array)
        .into_iter().flatten().take(32).filter(|credit| credit.is_object())
        .map(|credit| json!({ "name": text(credit, "name", "", 240),
            "role": text(credit, "role", "", 240) })).collect();
    json!({ "name": text(theme, "name", "", 240),
        "description": text(theme, "description", "", 4096),
        "musicTrack": text(theme, "musicTrack", "", 240),
        "accent": theme.get("color").and_then(theme_color),
        "soulColor": theme.get("soulColor").and_then(theme_color),
        "bootSyncTime": theme.get("bootSyncTime").and_then(Value::as_f64)
            .filter(|value| value.is_finite() && (0.0..=3600.0).contains(value)),
        "videoHasAudio": theme.get("videoHasAudio").and_then(Value::as_bool).unwrap_or(false),
        "credits": credits })
}

fn theme_preview(resources: &Path, args: &Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request { theme_id: String }
    let request: Request = serde_json::from_value(args.clone()).map_err(|_| "Invalid theme preview request")?;
    ThemeId::new(&request.theme_id).map_err(|_| "Invalid theme ID")?;
    let theme = read_json(resources, &PathBuf::from("web/themes/data")
        .join(format!("{}.theme.json", request.theme_id)))?
        .ok_or("The built-in theme is unavailable")?;
    if !theme.is_object() { return Err("Invalid built-in theme record".into()); }
    let mut image_path = None;
    if let Some(name) = theme.get("previewBackground").and_then(Value::as_str).filter(|name| !name.is_empty())
        .or_else(|| theme.get("background").and_then(Value::as_str).filter(|name| !name.is_empty())) {
        // A theme image is a single named, bundled raster file. Never accept a
        // URL, path traversal, SVG script, user media path or a network share.
        if name.len() > 255 || name.contains(['/', '\\', ':', '\0']) || name == "." || name == ".." {
            return Err("Unsafe theme image filename".into());
        }
        let extension = Path::new(name).extension().and_then(|part| part.to_str())
            .unwrap_or("").to_ascii_lowercase();
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
            return Err("This theme background format is not supported in the preview".into());
        }
        let path = contained(resources, &PathBuf::from("web/themes/img").join(name))?;
        let metadata = fs::symlink_metadata(&path).map_err(|_| "Theme background image is missing")?;
        if !metadata.is_file() || linked(&metadata) || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            return Err("Theme image is linked, empty or exceeds the 12 MiB limit".into());
        }
        let canonical = fs::canonicalize(&path).map_err(|_| "Theme background image is unavailable")?;
        let directory = fs::canonicalize(contained(resources, Path::new("web/themes/img"))?)
            .map_err(|_| "Theme image directory is unavailable")?;
        if !directory.starts_with(resources) || canonical.parent() != Some(directory.as_path()) {
            return Err("Theme image escapes the bundled image directory".into());
        }
        image_path = Some(canonical.to_str().ok_or("Theme image path is not valid UTF-8")?.to_owned());
    }
    Ok(json!({ "themeId": request.theme_id, "imagePath": image_path,
        "hasVideo": theme.get("backgroundVideo").and_then(Value::as_str).is_some_and(|value| !value.is_empty()),
        "hasAudio": theme.get("mainSong").and_then(Value::as_str).is_some_and(|value| !value.is_empty())
            || theme.get("videoHasAudio").and_then(Value::as_bool).unwrap_or(false),
        "metadata": theme_metadata(&theme) }))
}

fn mod_id(value: &str) -> Result<u64> {
    if value.is_empty() || value.len() > 16 || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Invalid public mod ID".into());
    }
    value.parse::<u64>().ok().filter(|id| *id > 0 && *id <= MAX_ID)
        .ok_or_else(|| "Invalid public mod ID".into())
}

fn normalize_detail(value: &Value, id: u64) -> Result<Value> {
    if value.get("_idRow").and_then(Value::as_u64) != Some(id)
        || !value.get("_sName").is_some_and(Value::is_string)
        || value.get("_sModelName").and_then(Value::as_str).is_some_and(|model| model != "Mod") {
        return Err("GameBanana returned a different or malformed mod".into());
    }
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for file in value.get("_aFiles").and_then(Value::as_array).into_iter().flatten().take(100) {
        let Some(file_id) = file.get("_idRow").and_then(Value::as_u64).filter(|id| *id > 0 && *id <= MAX_ID) else { continue; };
        if !seen.insert(file_id) { continue; }
        if files.len() == 20 { break; }
        files.push(json!({ "id": file_id.to_string(), "name": text(file, "_sFile", "", 240),
            "version": text(file, "_sVersion", "", 80),
            "bytes": file.get("_nFilesize").and_then(Value::as_u64).filter(|size| *size <= MAX_ID) }));
    }
    let description = value.get("_sText").and_then(Value::as_str).filter(|value| !value.is_empty())
        .or_else(|| value.get("_sDescription").and_then(Value::as_str)).unwrap_or("")
        .chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).take(16000).collect::<String>();
    // Deliberately omit download URLs, tokens, executable commands and arbitrary
    // provider links. Remote image fetches are not delegated to GPUI's URL loader.
    Ok(json!({ "id": id.to_string(), "name": text(value, "_sName", "", 240),
        "author": value.pointer("/_aSubmitter/_sName").and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control()).take(240).collect::<String>(),
        "description": description,
        "game": value.pointer("/_aGame/_sName").and_then(Value::as_str).unwrap_or("").chars().filter(|c| !c.is_control()).take(240).collect::<String>(),
        "url": format!("https://gamebanana.com/mods/{id}"), "files": files,
        "hasContentRatings": value.get("_bHasContentRatings").and_then(Value::as_bool).unwrap_or(false) }))
}

fn mod_detail(network: &mut Option<(tokio::runtime::Runtime, Client)>, args: &Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request { id: String }
    let request: Request = serde_json::from_value(args.clone()).map_err(|_| "Invalid mod detail request")?;
    let id = mod_id(&request.id)?;
    if network.is_none() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|_| "Network runtime unavailable")?;
        let client = Client::new(Duration::from_secs(12), 1, Duration::from_millis(100))
            .map_err(|_| "Network client unavailable")?;
        *network = Some((runtime, client));
    }
    let (runtime, client) = network.as_ref().ok_or("Network client unavailable")?;
    let url = format!("https://gamebanana.com/apiv11/Mod/{id}/ProfilePage");
    let response: Value = runtime.block_on(client.json(Provider::GameBanana, &url, None))
        .map_err(|_| "GameBanana details could not be loaded. Check the connection and retry.")?;
    normalize_detail(&response, id)
}

pub(super) fn dispatch(command: &str, args: &Value, state: &Path, resources: &Path,
    network: &mut Option<(tokio::runtime::Runtime, Client)>) -> Option<Result<Value>> {
    match command {
        "ui.preferences.get" => Some(interface_preferences(state, args, false)),
        "ui.preferences.set" => Some(interface_preferences(state, args, true)),
        "theme.preview" => Some(theme_preview(resources, args)),
        "shop.detail" => Some(mod_detail(network, args)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mod_identifiers_cannot_change_the_endpoint() {
        for id in ["", "0", "01", "../42", "42?x=1", "-1", "9007199254740992"] {
            assert!(mod_id(id).is_err());
        }
        assert_eq!(mod_id("42").unwrap(), 42);
    }
    #[test]
    fn details_are_bounded_and_do_not_expose_download_tokens() {
        let detail = json!({ "_idRow": 42, "_sName": "Mod", "_sModelName": "Mod",
            "_aFiles": [{ "_idRow": 1, "_sFile": "mod.zip", "_sDownloadUrl": "https://example.com/?token=SECRET" },
                { "_idRow": 1, "_sFile": "duplicate" }], "_sText": "x".repeat(20000) });
        let result = normalize_detail(&detail, 42).unwrap();
        assert_eq!(result["files"].as_array().unwrap().len(), 1);
        assert_eq!(result["description"].as_str().unwrap().len(), 16000);
        assert!(!result.to_string().contains("SECRET"));
        assert!(normalize_detail(&detail, 43).is_err());
    }
    #[test]
    fn locale_schema_is_explicit_and_unsupported_values_are_rejected() {
        let mut prefs = InterfacePreferences::default();
        assert!(prefs.validate().is_ok());
        prefs.locale = "it".into(); assert!(prefs.validate().is_ok());
        prefs.locale = "../../file".into(); assert!(prefs.validate().is_err());
        assert!(serde_json::from_value::<InterfacePreferences>(json!({"schemaVersion":1,"locale":"en","themeImages":false,"extra":true})).is_err());
    }
    #[test]
    fn unknown_commands_are_not_a_generic_file_or_network_bridge() {
        let mut network = None;
        assert!(dispatch("download", &json!({}), Path::new("/state"), Path::new("/resources"), &mut network).is_none());
        assert!(dispatch("shop.detail", &json!({"id":"../42"}), Path::new("/state"), Path::new("/resources"), &mut network).unwrap().is_err());
        assert!(network.is_none());
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!("deltamod-gpuix-presentation-{}-{}",
                std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    #[test]
    fn interface_preferences_are_separate_and_bad_writes_preserve_saved_values() {
        let state = Fixture::new();
        fs::write(state.0.join("preferences.json"), b"original appearance preferences").unwrap();
        let value = json!({"schemaVersion":1,"locale":"it","themeImages":true});
        assert_eq!(interface_preferences(&state.0, &value, true).unwrap(), value);
        assert_eq!(interface_preferences(&state.0, &json!({}), false).unwrap(), value);
        assert!(interface_preferences(&state.0, &json!({"schemaVersion":1,"locale":"bad","themeImages":false}), true).is_err());
        assert_eq!(interface_preferences(&state.0, &json!({}), false).unwrap(), value);
        assert_eq!(fs::read(state.0.join("preferences.json")).unwrap(), b"original appearance preferences");
        assert!(interface_preferences(&state.0, &json!({"path":"/outside"}), false).is_err());
    }

    #[test]
    fn theme_preview_is_confined_to_named_bounded_built_in_images() {
        let resources = Fixture::new();
        let data = resources.0.join("web/themes/data");
        let images = resources.0.join("web/themes/img");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir(&images).unwrap();
        let theme_file = data.join("base.theme.json");
        fs::write(images.join("base.png"), b"bounded test raster placeholder").unwrap();
        fs::write(&theme_file, br#"{"background":"base.png","mainSong":"base.mp3"}"#).unwrap();
        let result = theme_preview(&resources.0, &json!({"themeId":"base"})).unwrap();
        assert_eq!(PathBuf::from(result["imagePath"].as_str().unwrap()), fs::canonicalize(images.join("base.png")).unwrap());
        assert_eq!(result["hasAudio"], true);
        for background in ["../base.png", "https://example.test/a.png", "file.svg"] {
            fs::write(&theme_file, serde_json::to_vec(&json!({"background":background})).unwrap()).unwrap();
            assert!(theme_preview(&resources.0, &json!({"themeId":"base"})).is_err());
        }
        fs::write(&theme_file, br#"{"background":"base.png"}"#).unwrap();
        fs::File::options().write(true).open(images.join("base.png")).unwrap().set_len(MAX_IMAGE_BYTES + 1).unwrap();
        assert!(theme_preview(&resources.0, &json!({"themeId":"base"})).is_err());
        assert!(theme_preview(&resources.0, &json!({"themeId":"../base"})).is_err());
    }

    #[test]
    fn detail_fallback_preserves_plain_text_paragraphs() {
        let result = normalize_detail(&json!({"_idRow":42,"_sName":"Mod","_sText":"","_sDescription":"First\nSecond"}), 42).unwrap();
        assert_eq!(result["description"], "First\nSecond");
    }

}
