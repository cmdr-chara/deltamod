use crate::{error, state::AppState};
use deltamod_mods_themes_domain::AssetInput;
use deltamod_mods_themes_runtime::{ThemeAssetValidator, ThemeJson};
use deltamod_tauri_os_adapters::{
    legacy_path_result, tool_choice_result, validate_dialog_selection, validate_windows_executable,
    ChoiceBackend, DialogBackend, DialogFilter, DialogRequest, ThemeImportCancel,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter};

const MAX_THEME_ASSET_BYTES: u64 = 64 * 1024 * 1024;

fn invalid(channel: &str) -> String {
    error::invalid(channel)
}

fn pick<D: DialogBackend>(dialogs: &D, request: &DialogRequest) -> Result<Option<PathBuf>, String> {
    dialogs.pick(request).map_err(|_| error::internal())
}

fn browse_file<D: DialogBackend>(dialogs: &D, data: &[Value]) -> Result<Value, String> {
    let name = data.first().and_then(Value::as_str).unwrap_or("Files");
    let name = name
        .replace(['\r', '\n', '\0'], " ")
        .chars()
        .take(80)
        .collect::<String>();
    let extension = data
        .get(1)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("browseFile"))?
        .to_ascii_lowercase();
    let filter = DialogFilter::new(name, [extension]).map_err(|_| invalid("browseFile"))?;
    let request = DialogRequest::file("Choose a file").filter(filter);
    Ok(json!(legacy_path_result(pick(dialogs, &request)?)))
}

#[derive(Deserialize)]
struct GameDefinition {
    platforms: std::collections::BTreeMap<String, PlatformDefinition>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformDefinition {
    executable: Option<String>,
    #[serde(default)]
    data_files: Vec<String>,
    bundle: Option<String>,
}

fn safe_game_relative(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !value.contains('\\')
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

fn valid_game_folder(resource_root: &Path, selected: &Path, game_id: &str) -> bool {
    if game_id.is_empty()
        || game_id.len() > 80
        || !game_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return false;
    }
    let definition = resource_root.join("games").join(format!("{game_id}.json"));
    let Ok(bytes) = fs::read(definition) else {
        return false;
    };
    let Ok(game) = serde_json::from_slice::<GameDefinition>(&bytes) else {
        return false;
    };
    let host = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        value => value,
    };
    let mut candidates = vec![host];
    if host == "linux" {
        candidates.push("win32");
    }
    candidates.into_iter().any(|platform| {
        let Some(definition) = game.platforms.get(platform) else {
            return false;
        };
        definition
            .executable
            .iter()
            .chain(definition.data_files.iter())
            .chain(definition.bundle.iter())
            .all(|relative| safe_game_relative(relative) && selected.join(relative).exists())
    })
}

fn locate_delta<D: DialogBackend>(
    dialogs: &D,
    state: &AppState,
    data: &[Value],
) -> Result<Value, String> {
    let game_id = data
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("locateDelta"))?;
    let request = DialogRequest::folder("Choose the game folder");
    let Some(selected) = pick(dialogs, &request)? else {
        return Ok(Value::Null);
    };
    if valid_game_folder(&state._assets.app, &selected, game_id) {
        Ok(json!(selected.to_string_lossy()))
    } else {
        Ok(json!("Invalid"))
    }
}

fn choose_theme<D: ChoiceBackend>(dialogs: &D, state: &AppState) -> Result<Value, String> {
    let themes = state
        .mods_themes
        .themes()
        .list()
        .map_err(|_| error::internal())?;
    let names = themes
        .iter()
        .map(|theme| theme.name.clone())
        .collect::<Vec<_>>();
    let selected = dialogs
        .choose(
            "Select a theme",
            "Select a theme from the list below:",
            &names,
        )
        .map_err(|_| error::internal())?;
    let Some(index) = selected else {
        return Ok(Value::Null);
    };
    let theme = themes.get(index).ok_or_else(error::internal)?;
    state
        .mods_themes
        .themes()
        .set_active(&theme.id)
        .map_err(|_| error::internal())?;
    Ok(Value::Null)
}

fn html_alert<D: ChoiceBackend>(dialogs: &D, data: &[Value]) -> Result<Value, String> {
    let title = data
        .first()
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 160)
        .ok_or_else(|| invalid("htmlAlert_outwin"))?;
    let message = data
        .get(1)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 8 * 1024)
        .ok_or_else(|| invalid("htmlAlert_outwin"))?;
    let buttons = data
        .get(2)
        .and_then(Value::as_array)
        .filter(|buttons| !buttons.is_empty() && buttons.len() <= 32)
        .ok_or_else(|| invalid("htmlAlert_outwin"))?;
    let choices = buttons
        .iter()
        .map(|button| {
            button
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty() && text.len() <= 120)
                .map(str::to_owned)
                .ok_or_else(|| invalid("htmlAlert_outwin"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let selected = dialogs
        .choose(title, message, &choices)
        .map_err(|_| error::internal())?
        .unwrap_or(choices.len() - 1);
    Ok(json!(selected))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeImportRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    include_music: bool,
    #[serde(default)]
    color: String,
    #[serde(default)]
    soul_color: String,
}

fn validated_theme_color(value: &str, fallback: &str) -> Result<String, String> {
    let value = if value.is_empty() { fallback } else { value };
    if value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(value.to_ascii_uppercase())
    } else {
        Err(invalid("importTheme"))
    }
}

struct ThemeValidator;
impl ThemeAssetValidator for ThemeValidator {
    fn validate_assets(&self, assets: &[AssetInput]) -> deltamod_mods_themes_runtime::Result<()> {
        if assets
            .iter()
            .any(|asset| asset.bytes.is_empty() || asset.bytes.len() as u64 > MAX_THEME_ASSET_BYTES)
        {
            return Err(deltamod_mods_themes_runtime::RuntimeError::Invalid(
                "theme asset size".into(),
            ));
        }
        Ok(())
    }
}

fn image_signature(extension: &str, bytes: &[u8]) -> bool {
    match extension {
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

fn read_asset(request: &DialogRequest, path: &Path) -> Result<(String, Vec<u8>), String> {
    let path = validate_dialog_selection(request, path).map_err(|_| error::internal())?;
    let metadata = fs::metadata(&path).map_err(|_| error::internal())?;
    if metadata.len() > MAX_THEME_ASSET_BYTES {
        return Err(error::internal());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(error::internal)?
        .to_ascii_lowercase();
    let bytes = fs::read(path).map_err(|_| error::internal())?;
    Ok((extension, bytes))
}

fn import_theme<D: DialogBackend>(
    dialogs: &D,
    state: &AppState,
    data: &[Value],
) -> Result<Value, String> {
    let mut request: ThemeImportRequest =
        serde_json::from_value(data.first().cloned().unwrap_or_else(|| json!({})))
            .map_err(|_| invalid("importTheme"))?;
    request.name = request.name.trim().chars().take(100).collect();
    request.description = request.description.trim().chars().take(500).collect();
    if request.name.is_empty() {
        return Err(invalid("importTheme"));
    }
    request.color = validated_theme_color(&request.color, "#CD4451")?;
    request.soul_color = validated_theme_color(&request.soul_color, "#FF0000")?;

    let image_request = DialogRequest::file("Choose the theme background").filter(
        DialogFilter::new("Image files", ["png", "jpg", "jpeg", "webp", "gif"])
            .map_err(|_| error::internal())?,
    );
    let Some(image_path) = pick(dialogs, &image_request)? else {
        return serde_json::to_value(ThemeImportCancel::at("background"))
            .map_err(|_| error::internal());
    };
    let (image_extension, image_bytes) = read_asset(&image_request, &image_path)?;
    if !image_signature(&image_extension, &image_bytes) {
        return Err(invalid("importTheme"));
    }

    let music_request = DialogRequest::file("Choose the optional theme music")
        .filter(DialogFilter::new("Song files", ["mp3", "ogg"]).map_err(|_| error::internal())?);
    let music = if request.include_music {
        let Some(path) = pick(dialogs, &music_request)? else {
            return serde_json::to_value(ThemeImportCancel::at("music"))
                .map_err(|_| error::internal());
        };
        Some(read_asset(&music_request, &path)?)
    } else {
        None
    };

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| error::internal())?
        .as_nanos();
    let theme_id = format!("custom_{nonce:x}");
    let background = format!("background.{image_extension}");
    let mut assets = vec![AssetInput {
        name: background.clone(),
        bytes: image_bytes,
    }];
    let music_name = music.map(|(extension, bytes)| {
        let name = format!("music.{extension}");
        assets.push(AssetInput {
            name: name.clone(),
            bytes,
        });
        name
    });
    state
        .mods_themes
        .themes()
        .import(
            ThemeJson {
                id: theme_id.clone(),
                name: request.name,
                description: Some(request.description),
                built_in: false,
                icon: Some(background),
                music: music_name,
                color: Some(request.color),
                soul_color: Some(request.soul_color),
            },
            assets,
            &ThemeValidator,
        )
        .map_err(|_| error::internal())?;
    Ok(json!({"created":true,"themeId":theme_id}))
}

fn choose_undertale_mod_tool<D: DialogBackend>(
    dialogs: &D,
    state: &AppState,
) -> Result<Value, String> {
    if !cfg!(target_os = "windows") {
        return Err(error::unavailable("undertaleModTool:choose"));
    }
    let request = DialogRequest::file("Choose UndertaleModTool").filter(
        DialogFilter::new("UndertaleModTool executable", ["exe"]).map_err(|_| error::internal())?,
    );
    let selected = pick(dialogs, &request)?;
    let selected = selected
        .as_deref()
        .map(validate_windows_executable)
        .transpose()
        .map_err(|_| invalid("undertaleModTool:choose"))?;
    if let Some(path) = &selected {
        let config = state.data_root.root.join("undertale-mod-tool.json");
        let temporary = state.data_root.root.join(".undertale-mod-tool.json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&json!({"schemaVersion":1,"executable":path}))
                .map_err(|_| error::internal())?,
        )
        .map_err(|_| error::internal())?;
        fs::rename(temporary, config).map_err(|_| error::internal())?;
    }
    serde_json::to_value(tool_choice_result(selected.as_deref())).map_err(|_| error::internal())
}

fn official_source(state: &AppState) -> Option<PathBuf> {
    let source = std::env::var_os("APPDATA")
        .map(PathBuf::from)?
        .join("deltamod");
    let source = fs::canonicalize(source).ok()?;
    let destination = fs::canonicalize(&state.data_root.root).ok()?;
    (source != destination && source.is_dir()).then_some(source)
}

fn import_official_profile(
    app: &AppHandle,
    state: &AppState,
    data: &[Value],
) -> Result<Value, String> {
    let requested = data.first().and_then(Value::as_str).unwrap_or("");
    let valid_uuid = requested.len() == 36
        && requested.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 8 | 13 | 18 | 23) && byte == b'-'
                || !matches!(index, 8 | 13 | 18 | 23) && byte.is_ascii_hexdigit()
        });
    let operation_id = if valid_uuid {
        requested.to_owned()
    } else {
        format!(
            "{:08x}-0000-4000-8000-{:012x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| error::internal())?
                .as_micros()
                & 0xffffffffffff
        )
    };
    let source =
        official_source(state).ok_or_else(|| error::unavailable("importOfficialProfile"))?;
    let manifest = state
        .profile_runtime
        .import_official_profile(&source)
        .map_err(|_| error::internal())?;
    for progress in state
        .profile_runtime
        .events()
        .map_err(|_| error::internal())?
    {
        let _ = app.emit("profile-import-progress", progress);
    }
    state
        .profile_runtime
        .clear_events()
        .map_err(|_| error::internal())?;
    Ok(json!({"operationId":operation_id,"manifest":manifest,"restartRequired":true}))
}

/// Dialog/import dispatch hook. Integrate it before `runtime::dispatch` so the UUID cancel
/// contract here is not shadowed by the temporary numeric-only compatibility handler.
pub fn dispatch<D: DialogBackend + ChoiceBackend>(
    app: &AppHandle,
    state: &AppState,
    dialogs: &D,
    channel: &str,
    data: &[Value],
) -> Result<Option<Value>, String> {
    let value = match channel {
        "htmlAlert_outwin" => html_alert(dialogs, data)?,
        "browseFile" => browse_file(dialogs, data)?,
        "lifecycle:updateMod" => super::lifecycle::update_mod(state, dialogs, data)?,
        "locateDelta" => locate_delta(dialogs, state, data)?,
        "chooseTheme" => choose_theme(dialogs, state)?,
        "importTheme" => import_theme(dialogs, state, data)?,
        "undertaleModTool:choose" => choose_undertale_mod_tool(dialogs, state)?,
        "importOfficialProfile" => import_official_profile(app, state, data)?,
        // The current profile runtime cannot expose its numeric operation ID until its
        // synchronous copy returns. UUID cancellation therefore truthfully reports false.
        "cancelOfficialProfileImport" if data.first().and_then(Value::as_str).is_some() => {
            json!(false)
        }
        _ => return Ok(None),
    };
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::html_alert;
    use deltamod_tauri_os_adapters::{AdapterError, ChoiceBackend};
    use serde_json::{json, Value};

    struct Choices(Option<usize>);

    impl ChoiceBackend for Choices {
        fn choose(
            &self,
            _title: &str,
            _message: &str,
            choices: &[String],
        ) -> Result<Option<usize>, AdapterError> {
            assert_eq!(choices, ["Continue", "Cancel"]);
            Ok(self.0)
        }
    }

    fn payload() -> Vec<Value> {
        vec![
            json!("Confirm"),
            json!("Continue with this operation?"),
            json!([{"text":"Continue"},{"text":"Cancel"}]),
        ]
    }

    #[test]
    fn native_html_alert_returns_the_selected_button_index() {
        assert_eq!(html_alert(&Choices(Some(0)), &payload()).unwrap(), json!(0));
    }

    #[test]
    fn native_html_alert_maps_window_close_to_the_last_button() {
        assert_eq!(html_alert(&Choices(None), &payload()).unwrap(), json!(1));
    }

    #[test]
    fn native_html_alert_rejects_unbounded_or_malformed_buttons() {
        let malformed = vec![json!("Confirm"), json!("Message"), json!([{}])];
        assert!(html_alert(&Choices(Some(0)), &malformed).is_err());
        let oversized = vec![
            json!("Confirm"),
            json!("Message"),
            Value::Array((0..33).map(|_| json!({"text":"Choice"})).collect()),
        ];
        assert!(html_alert(&Choices(Some(0)), &oversized).is_err());
    }
}
