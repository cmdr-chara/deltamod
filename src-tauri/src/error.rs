pub const MAX_CHANNEL_BYTES: usize = 64;

pub fn unavailable(channel: &str) -> String {
    let safe = if channel.len() <= MAX_CHANNEL_BYTES && channel.is_ascii() {
        channel
    } else {
        "unknown"
    };
    format!("TAURI_COMMAND_UNAVAILABLE:{safe}")
}

pub fn invalid(channel: &str) -> String {
    let safe = if channel.len() <= MAX_CHANNEL_BYTES && channel.is_ascii() {
        channel
    } else {
        "unknown"
    };
    format!("TAURI_INVALID_PAYLOAD:{safe}")
}

pub fn internal() -> String {
    "TAURI_INTERNAL_ERROR".to_owned()
}
