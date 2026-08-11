use crate::{error, state::AppState};
use deltamod_tauri_os_adapters::{validate_https_external, ValidatedFolder};
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

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
        "openCommunityMaintainerProfile" => {
            open_url(app, "https://github.com/cmdr-chara", &["github.com"]).map(Some)
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

#[cfg(test)]
mod tests {
    use super::flag_name;

    #[test]
    fn legacy_flag_names_are_case_insensitive() {
        assert_eq!(flag_name("audio").as_deref(), Some("AUDIO"));
        assert_eq!(flag_name("hashchecks").as_deref(), Some("HASHCHECKS"));
        assert_eq!(flag_name("CONTROLLER").as_deref(), Some("CONTROLLER"));
        assert_eq!(flag_name("1invalid"), None);
    }
}
