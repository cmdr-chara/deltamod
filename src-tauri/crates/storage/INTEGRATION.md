# Integration Notes

The current repository exposes storage-adjacent behavior through the Tauri IPC bridge. The install manager calls `getInstallations`, `getSystemIndex`, `setInstallationCName`, `changeSystemIndex`, `deleteSystemIndex`, `repairInstallation`, `reimportInstallation`, `openInstallationFolder`, and `createInstallLink`. Records observed in the frontend include `index`, `pid`, `name`, `steam`, `valid`, `issues`, and `canOpenInUndertaleModTool`; the Rust schema keeps those typed and preserves unknown fields with `flatten`.

The web layer also uses browser `localStorage` for language, shop provider, filters, and contributor cache. Those are intentionally not folded into the profile file. GameBanana collections remain remote IPC operations.

Suggested native/core reuse: call `DataRoot::new` once from the Tauri state layer, use `load_json`/`save_json` for profile migration and writes, and expose the existing IPC response shapes by serializing `InstallationRecord`. `recovery_plan` should be followed by a separate executor only after all journal validation succeeds; planning itself performs no mutation.

The standard-library Windows path uses same-directory temporary creation and rename. Windows does not provide a portable standard-library guarantee for replacing an open destination, and parent-directory fsync is unavailable here; callers should surface replacement errors and retry/repair rather than assume POSIX-level durability. No `unsafe`, `windows-sys`, or global configuration is used.
