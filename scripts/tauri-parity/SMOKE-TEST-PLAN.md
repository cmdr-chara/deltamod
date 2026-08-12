# Tauri Smoke-Test Plan

Run after building the Tauri app with a disposable app-data directory and a fixture game/profile. Capture the invoke channel, arguments, result/error, emitted events, and UI state for every step. A green launch is not evidence that an unsupported command succeeded: require the expected result shape and event.

| Area | Steps and assertions |
| --- | --- |
| Launch | Start packaged and dev builds; main window is labeled `main`, renderer loads, `version`, `getOS`, `isDevMode`, `isPackaged`, and `diagnosticInfo` return values. |
| Mods | List mods, import a fixture mod, list full metadata, toggle state, set variant, remove it; assert `refresh` events and persisted state. |
| Themes | List/get themes, import and rename a fixture theme, activate it, delete it; assert `themeChange` and persisted active theme. |
| Install | Create/import a disposable installation, list installations, rename it, change system index, repair/reimport, and delete; assert no writes outside disposable data. |
| Patch | Run patch with a fixture installation and mod set; assert progress events, completion/failure result, and atomic rollback on a forced failure. |
| Game | Query available/current game info, launch a fixture or mocked game, verify process lifecycle and `loadedDeltarune`; do not use a user's real install. |
| Network | Browse providers against a deterministic mock server, exercise Nexus failure/cancel paths, and verify bounded errors plus `mod-source-progress`. |
| Updater | Check, ignore, and install using a signed local test update or stub; assert `updateAvailable`, `updateProgress`, restart/exit behavior. |

For each area, first run the contract fixtures through `compare-contract.js`; then run the live Tauri scenario. Record skipped platform-specific capabilities explicitly.
