use crate::{error, state::AppState};
use serde_json::{json, Value};

pub fn dispatch(state: &AppState, channel: &str, _data: &[Value]) -> Result<Option<Value>, String> {
    if channel != "fireUpdate" {
        return Ok(None);
    }
    let mut updater = state.updater.lock().map_err(|_| error::internal())?;
    Ok(Some(json!(updater
        .fire_update()
        .map_err(|_| error::internal())?)))
}
