# Native game download integration

This crate owns resolution and bounded download for the legacy `downloadGame(gameId)` channel. It is independent of Tauri and never executes a shell, scrapes provider pages, or accepts a renderer URL.

## Exact legacy contract

Electron receives only a game ID, looks up packaged `availableFeatures[feat=autodownload].data`, downloads at most 8 GiB, extracts at most 16 GiB/100,000 files, unwraps a single top-level entry, and returns the extracted directory path. It emits no renderer event; its progress window is main-process UI.

Load packaged game JSON with `Catalog::from_legacy_values`. Construct one `Runtime` with `BuiltInResolver` and an app-owned temporary-download directory. Invoke `download_game` with the renderer game ID plus backend-selected host platform and edition. Do not deserialize platform, edition, provider metadata, or URLs from the legacy renderer argument array.

`ImportTransactionPlan` is intentionally not the legacy success value. Pass its archive to the native archive importer using the exact included limits. Validate the extracted tree contains `executable`, apply `unwrapSingleRoot`, then commit `profile` through the profile/install runtime in the same transaction. Delete the archive on every importer success/failure. Only after importer/profile commit succeeds should the channel return the extracted directory path. Resolver failure, importer failure, or profile failure must reject the channel.

Use `CancellationToken` in an application operation registry and call `cancel()` when the owning window closes or a native cancellation command is received. The callback receives typed progress suitable for native UI; there is no legacy renderer event to emit.

## Provider readiness

- GameJolt: production resolver ready. It uses only the fixed build-ID JSON endpoint and revalidates every download redirect against `gamejolt.com`, `gamejolt.net`, and `gjcdn.net`.
- Itch: configuration blocked. Electron scraped a CSRF token from HTML and called an undocumented page endpoint. This runtime deliberately does not reproduce that behavior. Supply a `ProviderResolver` backed by a documented authenticated Itch/Butler API and native credential storage. The adapter must return a `ResolvedArtifact`; this runtime still validates its URL against `itch.io`/`hwcdn.net` and performs the bounded download.
- Integrity: provider/catalog SHA-256 and paired Ed25519 metadata are enforced when available. The current packaged Electron records provide neither, so HTTPS/provider provenance and archive validation remain mandatory.

## Host wiring still required

Add `deltamod-game-download-runtime` to the root Tauri crate, manage a single runtime and operation registry, route `downloadGame` before the current blocker, and connect the returned plan to the native archive importer/profile runtime. The repository's current native archive boundary reports `NATIVE_ARCHIVE_EXTRACTOR_UNAVAILABLE`, so the channel is not end-to-end ready until that importer is linked. No changes to shared routing/configuration files are included here.

## Verification

Run from this directory:

```text
cargo fmt --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
