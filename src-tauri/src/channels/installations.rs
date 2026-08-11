use crate::{error, state::AppState};
use serde_json::{json, Value};

pub fn dispatch(state: &AppState, channel: &str, _data: &[Value]) -> Result<Option<Value>, String> {
    let profile = match channel {
        "getInstallations"
        | "getSystemIndex"
        | "getMaxExistingIndex"
        | "isCurrentIndexSteam"
        | "removeSteamIntegration" => state.profile()?,
        _ => return Ok(None),
    };
    match channel {
        "getInstallations" => {
            let records = profile
                .installations
                .into_iter()
                .take(256)
                .collect::<Vec<_>>();
            Ok(Some(
                serde_json::to_value(records).map_err(|_| error::internal())?,
            ))
        }
        "getSystemIndex" => Ok(Some(json!(profile.current_index.unwrap_or(0)))),
        "getMaxExistingIndex" => Ok(Some(json!(profile
            .installations
            .iter()
            .filter_map(|x| x.index)
            .max()
            .unwrap_or(0)))),
        "isCurrentIndexSteam" => {
            let current = profile.current_index;
            Ok(Some(json!(profile
                .installations
                .iter()
                .any(|x| x.index == current && x.steam == Some(true)))))
        }
        "removeSteamIntegration" => {
            let index = profile.current_index.unwrap_or(0);
            let removed = state
                .profile_runtime
                .legacy_remove_steam_integration(index)
                .map_err(|_| error::internal())?;
            Ok(Some(json!(removed)))
        }
        _ => unreachable!(),
    }
}
