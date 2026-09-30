// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2

// Reuse the production channel implementations in the GPUIX host without
// starting Tauri, a WebView, or a second copy of lifecycle logic.
#[path = "channels/runtime.rs"]
pub(crate) mod runtime;
#[path = "channels/lifecycle.rs"]
pub(crate) mod lifecycle;
#[path = "channels/game.rs"]
pub(crate) mod game;
#[path = "channels/installations.rs"]
pub(crate) mod installations;
#[path = "channels/protocol.rs"]
pub(crate) mod protocol;
#[path = "channels/import_download.rs"]
pub(crate) mod import_download;
