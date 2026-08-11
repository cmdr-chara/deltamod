# Runtime Integration

`deltamod-mods-themes-runtime` is the filesystem adapter between Tauri/channel code and
`deltamod-mods-themes-domain`. It does not open dialogs, report fake success, or perform
network/archive extraction.

## Layout

- `data_root/mods-state.json`: `{ "enabled": [], "selectedVariants": {} }`
- `data_root/preferences.json`: legacy unique-flag map
- `data_root/active-theme.json`: `{ "id": "theme-id" }`
- `data_root/shared.json` and `data_root/sponsors.json`
- Each mod directory contains `manifest.json`.
- Each theme directory contains `theme.json` and validated asset files.

## Channel wiring

Construct `Runtime::open(RuntimeConfig::new(...))` during app initialization. Expose
`mods()`, `themes()`, `preferences()`, `shared()`, and `sponsors()` from channel handlers;
serialize returned domain records with the existing serde/JSON bridge. Drain
`Runtime::drain_events()` after successful mutations and translate each `EventIntent` to
the frontend event name used by the shell.

Archive import and theme asset ingestion must implement `ArchiveAssetValidator` and
`ThemeAssetValidator`. Validate archive entries before staging, copy into a transaction
staging directory, then atomically publish. The adapter intentionally leaves extraction
policy to the integration layer.

All managed paths are root-constrained and reject symlinks/reparse points. Writes are
temporary-file plus rename and are serialized per runtime instance.
