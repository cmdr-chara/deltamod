# Tauri Release Gate

This gate applies to the stable Tauri release. The existing Electron release remains the rollback artifact until every check below is green. The current unsigned Electron release workflow must not be used to publish the stable release.

## Required parity checks

1. `npm ci` succeeds with the committed `package-lock.json`.
2. `npm run build:boot` succeeds and the Tauri frontend points at the generated `web/` output.
3. `npm test`, `npm run typecheck`, and `npm run security:audit` pass.
4. `cargo fmt --all --check --manifest-path native/Cargo.toml`, `cargo clippy --workspace --all-targets --locked --manifest-path native/Cargo.toml -- -D warnings`, and `cargo test --workspace --locked --manifest-path native/Cargo.toml` pass.
5. `npm run verify:g3mtool-manifest` and `npm run verify:undertale-mod-tool-manifest` pass; bundled trees retain their upstream license files and the matching source archives are attached to the release.
6. Each target stages five sidecars, each sidecar is non-empty, target-matched, executable on Unix, and invoked by its real JSON smoke test.
7. The packaged app starts, shows the main window, reports the exact package version, persists one unique flag, loads the base theme, lists an installation, and returns a bounded error for an unknown IPC channel.
8. The protocol smoke test registers `deltamod-community://`, launches a cold process with one deep link, confirms `protocol:rendererReady` receives the queued action, and confirms a second instance forwards the link instead of creating a second data root.
9. Windows x64: clearly label the NSIS package unsigned, install and uninstall it, run a mod import and CSX patch smoke test, and verify all five sidecars.
10. Linux x64: run the AppImage without installation, verify executable permissions, import a mod, run a G3MTool patch smoke test, and verify all five sidecars.
11. macOS x64 and arm64: clearly label the DMG unsigned and unnotarized, verify the app bundle architecture, run the same persistence/protocol smoke tests, verify G3MTool, and mark UndertaleModTool CSX unavailable on arm64 rather than silently falling back.
12. Compare the Tauri smoke results against the Electron release on the same fixture set. Any changed result is a release blocker unless documented in the release notes.
13. Confirm `createUpdaterArtifacts` remains `false`, no updater endpoint is advertised, and release notes direct users to manually download future versions. Automatic updates remain blocked until a signed updater and key-management process are configured and tested.
14. Generate `SHA256SUMS.txt` from the final seven release assets and attach it to the same GitHub release. State that checksums verify integrity but not publisher identity.

## Artifact and license checks

The artifact must contain EUPL-1.2 metadata, `NOTICE.md`, `THIRD_PARTY_NOTICES.md`, and the complete unmodified G3MTool and UndertaleModTool trees for the selected target. Native workers are separate EUPL-1.2 executables and are not linked into the Tauri shell. G3MTool and UndertaleModTool remain separate GPL-3.0-only processes; do not link either into Rust crates or copy only its executable without its release license files.

## Rollback plan

1. Do not delete or replace the prior Electron assets, release tag, or update metadata.
2. If any gate fails after publication, mark the Tauri assets as withdrawn in the GitHub release notes and remove them from manual distribution.
3. Repoint the download table and release links to the last passing Electron artifact; keep the failed Tauri files available only to maintainers for diagnosis.
4. Re-run the failed target from the exact commit using the committed Cargo and npm locks. Never repair a release by rebuilding without the lock files.
5. If a user installed the Tauri release, direct them to uninstall it and install the prior verified Electron release; user data must remain in the documented application data directory and must not be deleted by uninstall.
