#![forbid(unsafe_code)]
#![allow(dead_code)]

mod error {
    pub fn invalid(channel: &str) -> String {
        format!("TAURI_INVALID_PAYLOAD:{channel}")
    }

    pub fn internal() -> String {
        "TAURI_INTERNAL_ERROR".into()
    }
}

mod state {
    use deltamod_game_download_runtime::{ButlerdAdapter, CancellationToken, Runtime};
    use deltamod_network_runtime::Client;
    use std::{collections::HashMap, path::PathBuf, sync::Mutex};

    pub struct DataRoot {
        pub root: PathBuf,
    }

    pub struct AppState {
        pub data_root: DataRoot,
        pub network: Client,
        pub network_runtime: Mutex<tokio::runtime::Runtime>,
        pub game_download: Runtime,
        pub butlerd: Option<ButlerdAdapter>,
        pub game_download_cancellations: Mutex<HashMap<String, CancellationToken>>,
    }
}

#[path = "../../../src/channels/import_download.rs"]
mod import_download;

#[test]
fn production_channel_boundary_compiles_against_runtime_apis() {
    assert!(std::mem::size_of::<state::AppState>() > 0);
}
