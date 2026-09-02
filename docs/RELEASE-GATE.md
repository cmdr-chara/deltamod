# Tauri Release Gate

This gate applies to the stable Tauri release. The existing Electron release remains the rollback artifact until every check below is green. The current unsigned Electron release workflow must not be used to publish the stable release.

## Required parity checks

1. `npm ci` succeeds with the committed `package-lock.json`.
2. `npm run build:boot` succeeds and the Tauri frontend points at the generated `web/` output.
3. `npm test`, `npm run typecheck`, and `npm run security:audit` pass.
4. Formatting, strict all-target Clippy, and locked workspace tests pass for both
   `src-tauri/Cargo.toml` and `native/Cargo.toml`; the Tauri workspace command must
   include `cargo test --workspace --all-targets --locked --manifest-path src-tauri/Cargo.toml`.
5. `npm run verify:g3mtool-manifest` and `npm run verify:undertale-mod-tool-manifest` pass; bundled trees retain their upstream license files and the matching source archives are attached to the release.
6. Each target stages five sidecars, each sidecar is non-empty, target-matched, executable on Unix, and invoked by its real JSON smoke test.
   The installed unsigned Windows NSIS package passes archive validation, atomic
   import, exact hashing, empty-plan validation, and exact patch rollback through
   those workers. Evidence is retained at
   `benchmarks/packaged-smoke/tauri-windows-installed-nsis-sidecars.json`; the same
   smoke runs in the installed Linux and macOS package jobs.
7. The packaged app starts, shows the main window, reports the exact package version, persists one unique flag, loads the base theme, lists an installation, and returns a bounded error for an unknown IPC channel.
8. The protocol smoke test registers `deltamod-community://`, launches a cold process with one deep link, confirms `protocol:rendererReady` receives the queued action, and confirms a second instance forwards the link instead of creating a second data root.
   The unsigned Windows NSIS candidate passes this gate; bounded evidence is retained at `benchmarks/packaged-smoke/tauri-windows-installed-nsis-protocol.json`. Signing remains a separate requirement.
   The installed Linux job dispatches through `xdg-open`; both installed macOS
   architectures declare the scheme in `Info.plist`, run from `/Applications`, and
   dispatch through Launch Services. Their evidence must be produced by native CI.
9. Windows x64: install and uninstall the NSIS package, verify its updater signature and a signed update from the previous stable version, run a mod import and CSX patch smoke test, and verify all five sidecars.
10. Linux x64: install the `.deb`, confirm updater status is `unsupported-package`, import a mod, run a G3MTool patch smoke test, and verify all five sidecars.
11. macOS x64 and arm64: verify the app bundle architecture, updater archive signature, and a signed update from the previous stable version; run the same persistence/protocol smoke tests, verify G3MTool, and mark UndertaleModTool CSX unavailable on arm64 rather than silently falling back.
12. Compare the Tauri smoke results against the Electron release on the same fixture set. Any changed result is a release blocker unless documented in the release notes.
13. Capture seven measured launches of the packaged Tauri candidate on the same
    Windows host and protocol as `benchmarks/desktop/electron-9e6f8af.json`, retain
    the immutable raw result, and require `scripts/desktop-benchmark/compare.js` to
    accept the pair before reporting readiness, memory, or artifact-size deltas.
14. Confirm updater artifacts are enabled only in the Windows/macOS platform overrides, `latest.json` contains exactly those three signed targets, and Linux `.deb` is absent.
15. Generate `SHA256SUMS.txt` over every release asset, including signatures and `latest.json`, attach GitHub attestations, and state that checksums verify integrity but not publisher identity.

## External signing prerequisites

The stable and rehearsal workflows fail before compilation unless the repository
provides the Tauri updater key, an exportable Windows code-signing PFX, and an Apple
Developer ID Application certificate plus notarization credentials. Secret names are
validated without printing their values. Windows imports the PFX into the disposable
runner certificate store, signs with SHA-256 and a timestamp, then verifies the shell,
NSIS package, and branded bootstrapper against the imported thumbprint. macOS imports
the Developer ID certificate into a disposable keychain and requires `codesign`,
Gatekeeper, and stapled-ticket validation on both architectures.

Certificates, private keys, Apple credentials, and their passwords are external
release authority and must never be committed. If the Windows certificate is hardware-
backed or cloud-held rather than exportable, replace the PFX import with the issuer's
Tauri `signCommand` integration and retain the same post-build publisher checks.

## Artifact and license checks

The artifact must contain EUPL-1.2 metadata, `NOTICE.md`, `THIRD_PARTY_NOTICES.md`, and the complete unmodified G3MTool and UndertaleModTool trees for the selected target. The five native workers are packaged as separate EUPL-1.2 compatibility executables and share EUPL Rust implementation code with the authoritative in-process Tauri lifecycle boundary. G3MTool and UndertaleModTool remain separate GPL-3.0-only processes; do not link either into Rust crates or copy only its executable without its release license files.

## Rollback plan

1. Do not delete or replace the prior Electron assets, release tag, or update metadata.
2. If any gate fails after publication, mark the Tauri assets as withdrawn in the GitHub release notes and remove them from manual distribution.
3. Repoint the download table and release links to the last passing Electron artifact; keep the failed Tauri files available only to maintainers for diagnosis.
4. Re-run the failed target from the exact commit using the committed Cargo and npm locks. Never repair a release by rebuilding without the lock files.
5. If a user installed the Tauri release, direct them to uninstall it and install the prior verified Electron release; user data must remain in the documented application data directory and must not be deleted by uninstall.
