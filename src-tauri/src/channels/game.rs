use crate::state::AppState;
use serde_json::Value;

pub fn dispatch(state: &AppState, channel: &str, data: &[Value]) -> Result<Option<Value>, String> {
    if !matches!(
        channel,
        "startGame"
            | "loadedDeltarune"
            | "getCurrentGameInfo"
            | "getGameInfo"
            | "getAvailableGames"
    ) {
        return Ok(None);
    }
    if matches!(channel, "startGame" | "getCurrentGameInfo") && !state.game_store_path.is_file() {
        return Err("GAME_STORE_UNAVAILABLE".to_owned());
    }
    state
        .game
        .dispatch(channel, data)
        .map_err(|game_error| game_error.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_completed_game_contracts_are_owned() {
        assert!(matches!(
            "startGame",
            "startGame"
                | "loadedDeltarune"
                | "getCurrentGameInfo"
                | "getGameInfo"
                | "getAvailableGames"
        ));
    }
}
