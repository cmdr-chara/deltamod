# Integration Notes

This crate is intentionally pure. A host owns filesystem/network mutation and implements `DomainStore` or a `FileMutationPlan` adapter after validating a plan.

## Legacy mapping

- Legacy mod folders and enabled-mod preferences map to `ModFolderId`, `ModUid`, `ModRecord`, and `ModListState`; call `normalize_enabled` before persistence.
- Legacy variant declarations map to `ModVariant`; `validate_declared_variants` rejects duplicate IDs and unknown selections.
- Theme JSON records map to `ThemeRecord`; built-in JSON entries must be treated as immutable. Imported records must pass `validate_theme_import`.
- Theme image/audio/video files map to `AssetInput`; both extension and magic bytes are required for PNG, JPEG, WebP, GIF, MP3, OGG, WAV, and MP4.
- Theme generated files should use `generated_asset_basename`, which is deterministic and collision-aware.
- Sponsor metadata maps to `SponsorManifest`; `root: None` is a valid missing-root state and must not be interpreted as the process root.
- Exact legacy renderer flags are represented by `Flag` and the allowlist `AUDIO SFX CONTROLLER SETUP`; parse and write with the exact functions.
- Shared renderer variables map to `SharedVariable` and `VariableType`; asset-valued variables use `AssetRef`.

## Tauri/event/protocol boundary

The existing browser adapter uses one Rust command, `backend_invoke`, with `{ channel, data }`, where `data` is always an array. Keep this crate below that boundary: decode a channel request into typed domain inputs, validate it, then return a typed result or a stable error. Do not accept arbitrary paths from the channel.

The current application protocol is `deltamod-community://`; the legacy `deltamod://` scheme must remain unclaimed. Existing event names include `themeChange` and `hash-progress`. Event payloads should be serialized from typed state/result shapes rather than forwarded arbitrary objects.

Filesystem adapters should apply only returned plans, in particular `ModPlan::DeleteFolder`, after the adapter verifies the packet root it supplied is the intended root. This crate performs no deletion or import itself.
