# Tauri Release Gate

This gate applies to the stable Tauri release. Electron runtime and packaging have been removed from the source tree. Preserve the last existing Electron release as a historical rollback artifact until every check below is green. Source retirement is not evidence of an updater-signed, platform-verified stable Tauri release. No Electron release workflow remains.

Unsigned Tauri previews use tags named `community-tauri-preview-v*`. They are
GitHub prereleases for manual testing only: updater artifacts and `latest.json`
must be absent, Windows publisher signing is intentionally absent, and macOS
notarization is intentionally absent. A preview never satisfies this stable gate.

`Community Tauri Release` also has a non-publishing `validation` mode. It checks
an exact `DeltaMaster` SHA, builds all four platform packages, installs and smokes
them, verifies that signing/updater artifacts are absent, and retains a checksum
manifest. This mode exists so release packaging can stay continuously verified when
the updater signing key is unavailable. It does not satisfy the updater-signature
or stable-publication requirements below. Optional publisher identity is separate.

## Required parity checks

1. `npm ci` succeeds with the committed `package-lock.json`, and `npm run verify:tauri-only` rejects any reintroduced Electron runtime, dependency, or renderer binding.
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
12. Capture seven measured launches of the packaged Tauri candidate on the same
    Windows host and protocol as `benchmarks/desktop/electron-9e6f8af.json`, retain
    the immutable raw result, and require `scripts/desktop-benchmark/compare.js` to
    accept the pair before reporting readiness, memory, or artifact-size deltas.
13. Confirm updater artifacts are enabled only in the Windows/macOS platform overrides, `latest.json` contains exactly those three signed targets, and Linux `.deb` is absent.
14. Generate `SHA256SUMS.txt` over every release asset, including signatures and `latest.json`, attach GitHub attestations, and state that checksums verify integrity but not publisher identity.

## External signing prerequisites

The maintainer-approved stable policy separates **mandatory updater signatures**
from **optional platform publisher certificates**. Stable builds may ship without
paid Windows/Apple certificates; they must not be mislabeled as unsigned previews.
All package, installed-smoke, target, checksum, provenance, and updater gates remain.

Stable publication and signed rehearsal require `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Main-branch CI checks those credentials before
creating a stable tag. Ordinary release-relevant pushes may fall back to unsigned
validation when they are missing. An explicit stable request (`Community CI` with
`stable_release=true`, or a `[stable-release]` commit marker) fails instead.
The lower-level `Community Tauri Release` workflow defaults to `validation`; its
`stable` mode uses an existing `community-v*` tag from the gated Community CI flow.

Publisher signing is selected independently per platform. An entirely absent group
is optional; a partially configured group or an invalid certificate fails the build
rather than silently falling back to an untrusted identity:

- Windows: `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD`. When configured,
  import the PFX, sign with SHA-256 and a timestamp, and verify the shell, NSIS package,
  and branded bootstrapper against the imported thumbprint. Otherwise verify that
  those executables have no Authenticode publisher signature.
- macOS: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`,
  `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID`. When configured, import the
  Developer ID certificate and require codesign, Gatekeeper, and stapled-ticket
  validation. Otherwise use and verify an ad-hoc signature for stable builds,
  including Apple Silicon, and verify that no Developer ID or notarization is claimed.

Without publisher certificates, release notes must disclose Windows SmartScreen
warnings and macOS Gatekeeper restrictions. Users may need to approve the specific
verified app in Privacy & Security; never recommend globally disabling Gatekeeper.
Ad-hoc signing is not Apple publisher verification. Tauri update signatures remain
mandatory, use the existing trusted public key, and are included for exactly the
Windows x64 and two macOS targets in `latest.json`. Linux `.deb` remains manual.
Stable publication explicitly uses `--latest --prerelease=false`; previews remain
prereleases and never replace the stable updater endpoint.

Certificates, private keys, Apple credentials, and passwords must never be committed
or printed. The `[stable-release]` marker only bypasses the application-changes
filter; it does not bypass any applicable verification. If a configured Windows
certificate is hardware-backed or cloud-held, use the issuer's Tauri `signCommand`
integration and retain the same post-build publisher checks.

## Artifact and license checks

The artifact must contain EUPL-1.2 metadata, `NOTICE.md`, `THIRD_PARTY_NOTICES.md`, and the complete unmodified G3MTool and UndertaleModTool trees for the selected target. The five native workers are packaged as separate EUPL-1.2 compatibility executables and share EUPL Rust implementation code with the authoritative in-process Tauri lifecycle boundary. G3MTool and UndertaleModTool remain separate GPL-3.0-only processes; do not link either into Rust crates or copy only its executable without its release license files.

## Rollback plan

1. Do not delete or replace the prior Electron assets, release tag, or update metadata.
2. If any gate fails after publication, mark the Tauri assets as withdrawn in the GitHub release notes and remove them from manual distribution.
3. Repoint the download table and release links to the last passing Electron artifact; keep the failed Tauri files available only to maintainers for diagnosis.
4. Re-run the failed target from the exact commit using the committed Cargo and npm locks. Never repair a release by rebuilding without the lock files.
5. If a user installed the Tauri release, direct them to uninstall it and install the prior verified Electron release; user data must remain in the documented application data directory and must not be deleted by uninstall.
