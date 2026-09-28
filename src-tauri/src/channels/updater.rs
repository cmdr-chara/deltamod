use crate::{error, state::AppState};
use serde_json::{json, Value};
use std::sync::TryLockError;

pub fn dispatch(state: &AppState, channel: &str, _data: &[Value]) -> Result<Option<Value>, String> {
    if !matches!(
        channel,
        "fireUpdate" | "start-update" | "cancel-update" | "ignore-update" | "updater-status"
    ) {
        return Ok(None);
    }
    if !_data.is_empty() {
        return Err(error::invalid(channel));
    }
    if channel == "cancel-update" {
        return Ok(Some(json!(state.updater_control.cancel())));
    }
    let mut updater = match state.updater.try_lock() {
        Ok(updater) => updater,
        Err(TryLockError::WouldBlock) if channel == "updater-status" => {
            return Ok(Some(json!({
                "state": state.updater_control.active_phase(), "available": false,
                "supported": true, "version": Value::Null, "reason": Value::Null,
            })));
        }
        Err(TryLockError::WouldBlock) => return Err("TAURI_UPDATER_BUSY".into()),
        Err(TryLockError::Poisoned(_)) => return Err(error::internal()),
    };
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
