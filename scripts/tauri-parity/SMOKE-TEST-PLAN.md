# Tauri Smoke-Test Plan

The executable contract is
[`fixtures/packaged-smoke.json`](./fixtures/packaged-smoke.json). Validate it with:

```text
node scripts/tauri-parity/validate-smoke-contract.js
```

Validation proves that the scenario is complete and safely isolated; it does not
claim that a packaged binary was launched. The release workflow additionally
installs the generated NSIS, `.deb`, or `.dmg` artifact, runs the bounded in-app
capability probe from that installed location, and uninstalls it while preserving
the disposable data root. On Windows it also runs the installed protocol smoke,
which verifies that the NSIS registration targets the installed Tauri executable,
opens a strict `deltamod-community://` action, and proves that the already-running
process handles it only after the renderer handshake. A live run must supply an explicit
Tauri executable (argument `--executable` or `DELTAMOD_TAURI_EXECUTABLE`) and a
disposable app-data root (argument `--data-root` or
`DELTAMOD_SMOKE_DATA_ROOT`). Never point the smoke at a user's real game or app
data.

The native Linux package additionally verifies its registered desktop handler and
dispatches the protocol with `xdg-open`. The macOS jobs copy the signed candidate to
`/Applications`, verify `CFBundleURLTypes`, and dispatch with Launch Services. Both
must prove that the queued and renderer-consumed action belongs to the already-running
process; static bundle metadata alone is insufficient.

Every installed target also runs `run-installed-sidecar-smoke.js` from the
directory containing the packaged executable. It rejects a host/target mismatch,
requires exactly five non-link workers (and executable mode on Unix), then exercises
archive-tree validation, atomic import publication, exact hashing, patch-plan
validation, and backup/restore through their real bounded protocols. Windows x64
has passed this gate locally; Linux x64 and macOS x64/arm64 evidence is produced only
by their native package jobs.

`--capability-probe --expected-version <x.y.z>` requires the Rust shell itself to
write bounded evidence inside that exact non-link data root. The runner accepts
the launch only when the process remains live and the app proves packaged mode,
exact version, flag persistence, base-theme loading/activation, installation
listing, and bounded rejection of an unknown channel. The current freshly built
unsigned Windows NSIS package has passed installation, this capability probe,
protocol forwarding, installed worker smoke, uninstallation, and user-data preservation; its evidence is retained under
`benchmarks/packaged-smoke`. Linux and macOS evidence must come from their native
release jobs; Windows signing and updater checks remain separate gates.

Run both the dev and packaged modes with a fixture game/profile. Capture the
invoke channel, arguments, result/error, emitted events, and UI state for every
step. A green launch is not evidence that an unsupported command succeeded:
require the expected result shape and event, and require unavailable commands to
fail explicitly.

| Area | Steps and assertions |
| --- | --- |
| Launch | Start packaged and dev builds; main window is labeled `main`, renderer loads, `version`, `getOS`, `isDevMode`, `isPackaged`, and `diagnosticInfo` return values. |
| Mods | List mods, import a fixture mod, list full metadata, toggle state, set variant, remove it; assert `refresh` events and persisted state. |
| Themes | List/get themes, import and rename a fixture theme, activate it, delete it; assert `themeChange` and persisted active theme. |
| Install | Create/import a disposable installation, list installations, rename it, change system index, repair/reimport, and delete; assert no writes outside disposable data. |
| Patch | Run patch with a fixture installation and mod set; assert progress events, completion/failure result, and atomic rollback on a forced failure. |
| Game | Query available/current game info, launch a fixture or mocked game, verify process lifecycle and `loadedDeltarune`; do not use a user's real install. |
| Network | Browse providers against a deterministic mock server, exercise Nexus failure/cancel paths, and verify bounded errors plus `mod-source-progress`. |
| Updater | Check, ignore, and install using a signed local test update or stub; assert `updateAvailable`, `updater-status`, `updater-progress`, and restart/exit behavior. |

For each area, first run the independent legacy/Rust fixtures through
`compare-contract.js`; then run the live Tauri scenario. Record skipped
platform-specific capabilities explicitly. A skipped required mode or missing
packaged executable keeps the release gate open.
