use crate::{error, state::AppState};
use serde_json::{json, Value};

pub fn dispatch(state: &AppState, channel: &str, _data: &[Value]) -> Result<Option<Value>, String> {
    if !matches!(
        channel,
        "fireUpdate" | "start-update" | "ignore-update" | "updater-status"
    ) {
        return Ok(None);
    }
    let mut updater = state.updater.lock().map_err(|_| error::internal())?;
    let value = match channel {
        "fireUpdate" => json!(updater.fire_update().map_err(|_| error::internal())?),
        "start-update" => {
            updater.start_update().map_err(|_| error::internal())?;
            Value::Null
        }
        "ignore-update" => {
            updater.ignore_update().map_err(|_| error::internal())?;
            Value::Null
        }
        "updater-status" => {
            let status = updater.status();
            json!({
                "state": status.state,
                "available": status.available,
                "supported": status.supported,
                "version": status.version,
                "reason": status.reason,
            })
        }
        _ => unreachable!(),
    };
    Ok(Some(value))
}
