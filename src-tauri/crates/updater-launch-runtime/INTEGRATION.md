# Integration

This crate is intentionally independent of Tauri, which keeps the domain and process contracts compileable on Windows, Linux, macOS, and Wine.

## Launch

Construct a `LaunchSpec` with an absolute working directory and explicit argv. Pass it to `launch(spec, &SystemProcessSpawner)`. The returned `OwnedChild` owns the OS child, waits or kills it, and runs finalization hooks exactly once, including on drop. Do not pass credentials in child environment variables; secret-looking keys are rejected and `sanitized_environment` removes them.

Wine uses `Platform::Wine` and an exact `wine <canonical-executable>` argv. Persisted custom Linux launchers are intentionally rejected because Electron's arbitrary command setting cannot be reproduced safely. Windows, Linux, and macOS use the same owned lifecycle.

## Game channels

Construct one application-owned `GameRuntime` with the packaged `games` resource directory, the selected legacy installation's `store.json`, and `HostPlatform::current()`. Call `GameRuntime::dispatch(channel, data)` before the generic unavailable fallback. The dispatcher returns `Ok(None)` for channels it does not own.

The following channels are production-ready in the runtime:

- `getCurrentGameInfo`, `getGameInfo`, and `getAvailableGames` return the raw catalog JSON shapes used by Electron.
- `loadedDeltarune` verifies the catalog-defined executable and data files and returns `{ loaded, path }`, where legacy `path` is the game ID.
- `startGame` resolves only catalog-defined paths, canonicalizes them beneath the installation root, launches with exact argv/cwd and a scrubbed environment, rejects overlap, and reaps the child asynchronously.
- `executeArgumentCmd` returns JSON null as the Tauri representation of Electron's resolved `undefined` no-op.
- `startGameVanilla` returns `GAME_CHANNEL_UNSUPPORTED`; Electron defines no handler or semantics for this channel.

Implement `GameLifecycle` in the host. `launched` should hide the window/disable audio, `finished` must restore patched originals and window/controller state exactly once, and `steam_launched` must perform the legacy app shutdown after the validated Steam URI has opened. The runtime deliberately cannot restore patches or control Tauri windows itself.

The selected `store.json` path must be refreshed when the installation index changes. Do not construct a temporary runtime per invocation because its in-flight process guard and lifecycle state are application-owned.

## Steam

Use `SteamUri::run(app_id)` or `SteamUri::parse` before passing the value to a `SteamOpener`. Parsing accepts only validated `steam://run/<id>` and Electron-compatible `steam://rungameid/<id>` URIs. `SystemSteamOpener` maps to `explorer.exe <uri>` on Windows, `open <uri>` on macOS, and `xdg-open <uri>` elsewhere, always as explicit argv rather than a shell string. Replace it with an application opener when the host needs telemetry or Tauri URI handling.

## Updates

`Updater::fire_update`, `Updater::start_update`, and `Updater::ignore_update` implement the Electron channel lifecycle. `fire_update` returns the legacy boolean and emits `UpdateEvent::Available` with the legacy `update`, `version`, and `release_name` fields. Every transition emits `UpdateEvent::Status`; downloads emit bounded `UpdateEvent::Progress` values with operation ID `community-update` and phase `download`. `Updater::status` is suitable for an `updater-status` query channel.

Construct `UpdaterGate` with `UpdaterGate::configured`. It remains unsupported unless the app is packaged, the installed package has an official updater path, updater artifacts are enabled, every configured endpoint is HTTPS, and a public key is present. The host enables this gate only for packaged Windows NSIS and macOS app bundles. Linux `.deb` reports `unsupported-package`. `scripts/verify-secure-updater-config.js` must continue to pass.

The production adapter must use `tauri-plugin-updater` v2. The plugin requires signed updates and does not allow signature checking to be disabled. Implement `tauri_adapter::OfficialUpdaterPlugin` by retaining the checked `tauri_plugin_updater::Update`, invoking its download API, rejecting a declared or cumulative size above the supplied limit, and returning a payload only after the plugin's signature verification succeeds. `install_verified` must call the retained update's install API. The wrapper creates the otherwise opaque `VerifiedArtifact`, binds it to the checked version, and prevents arbitrary paths or unsigned byte buffers from reaching install.

## Tauri

Enable `tauri-adapter` and map events in the host as follows:

- `UpdateEvent::Available` to `app.emit("updateAvailable", { update, version, releaseName })`.
- `UpdateEvent::Status` to `app.emit("updater-status", { state, available, supported, version, reason })`.
- `UpdateEvent::Progress` to `app.emit("updater-progress", { operationId, phase, completed, total, percentage })`.

Register `fireUpdate`, `start-update`, `ignore-update`, and `updater-status` in the channel router. Store the updater behind application-owned synchronization so checks and installs cannot overlap.

Host wiring:

1. Pin `tauri-plugin-updater` and initialize `tauri_plugin_updater::Builder::new().build()`.
2. Set `bundle.createUpdaterArtifacts` to `true` only in the Windows and macOS platform configurations.
3. Configure `plugins.updater.pubkey` with the release public key and the canonical HTTPS `latest.json` endpoint.
4. Source `TAURI_SIGNING_PRIVATE_KEY` only from GitHub Actions secrets; never place the private signing key in the repository or application configuration.
5. Publish Tauri's signed updater artifact and matching signature in the endpoint JSON for each supported target.
6. Keep updater access host-side; no frontend updater plugin permissions are required.
7. Keep Linux manual while the stable package is `.deb`, unless an updater-compatible Linux package and recovery path are explicitly introduced and tested.

Keep the concrete Tauri dependency in the host to avoid feature/version coupling in this crate.

## Verification

Repository policy:

```text
node scripts/verify-secure-updater-config.js --self-test
node scripts/verify-secure-updater-config.js
```

Runtime:

```text
cargo fmt --check
cargo test --all-targets
cargo check --all-targets --features tauri-adapter
cargo clippy --all-targets --all-features -- -D warnings
```

See `docs/SECURE-UPDATES.md` for the staged key-management, platform rollout, recovery, and promotion gates.
