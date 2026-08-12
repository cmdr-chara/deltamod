# Integration

This crate does not execute arbitrary commands or accept renderer-supplied filesystem paths. It provides policy types and narrow traits. The application owns the concrete Tauri handles and backend-derived roots.

## Cargo

The workspace member already exists. Enable the compile-tested native implementation and add
the official dialog plugin to the application crate in `src-tauri/Cargo.toml`:

```toml
[dependencies]
deltamod-tauri-os-adapters = { path = "crates/os-adapters", features = ["tauri-adapter"] }
tauri-plugin-dialog = "2.7.2"
```

The crate pins the compile-tested Tauri 2.11 line and official plugins `tauri-plugin-dialog = 2.7.2` and `tauri-plugin-opener = 2.5.4`.

## Registration

Register the official dialog plugin beside the existing opener plugin in `main`:

```rust
tauri::Builder::default().plugin(tauri_plugin_dialog::init())
```

`TauriDialogBackend::new(app)` uses `DialogExt` for native file/folder/message selection and
revalidates every selected path after the picker returns. Success is a path string and cancel is
`null` for `browseFile` and `locateDelta`. Theme import preserves the Electron
`{ created: false, canceled: true, stage: "background" | "music" }` cancel objects and UMT
selection preserves `{ configured, executableName, canceled }`.

Add `pub mod dialogs;` to `src/channels/mod.rs`. In `dispatch_domain`, instantiate the backend
and call the new dispatcher before `runtime::dispatch`:

```rust
let dialogs = deltamod_tauri_os_adapters::tauri_adapter::TauriDialogBackend::new(app);
if let Some(value) = channels::dialogs::dispatch(app, state, &dialogs, name, data)? {
    return Ok(value);
}
```

Make `backend_invoke` an `async fn` before wiring this dispatcher. The official plugin documents
its `blocking_pick_*`/`blocking_show_with_result` methods for async commands or other non-main
thread contexts; do not run them from Tauri's main event-loop thread.

Move `chooseTheme`, `importTheme`, `importOfficialProfile`, `undertaleModTool:choose`,
`browseFile`, and `locateDelta` from `BackendChannel::Unsupported` to `Implemented`.
`cancelOfficialProfileImport` is already allowlisted. Remove its old branch from
`channels/runtime.rs` after the profile runtime exposes a cancellable UUID operation before copy;
until then the dialog dispatcher truthfully returns `false` for UUID cancellation rather than
claiming cancellation occurred.

Use `validate_https_external` before `app.opener().open_url`; do not expose shell commands or a
generic path opener.

`ValidatedFolder::from_backend` must receive only paths computed by Rust state (for example installation/mod/theme roots) and the corresponding approved canonical roots. Use `FolderRevealer` to reveal that validated folder through the opener plugin. Never pass a renderer argument directly to either operation.

Map `LifecycleAction::Restart` to `AppHandle::restart`, `Quit` to `AppHandle::exit(0)`, and `WindowMode` to an allowlisted window-state operation. Keep these commands on the existing `main` window allowlist. Shortcut creation should first produce `ShortcutPlan`; an OS-specific installer may consume it after validating executable ownership. No `Command`, shell, or arbitrary shortcut target belongs in an IPC payload.

## Capabilities

Add only narrow permissions to the `default` capability for `main`:

```json
"permissions": [
  "core:event:default",
  "dialog:allow-open",
  "dialog:allow-message",
  "opener:allow-open-url"
]
```

The Rust-side dialogs do not require renderer filesystem permissions, but the explicit dialog
permissions keep policy accurate if the official plugin commands are retained. Do not add
`shell:allow-execute`, broad `shell:default`, unrestricted filesystem scopes, or a wildcard opener
URL rule. The opener URL scope should be narrowed to the hosts passed to
`validate_https_external`.

## Desktop shortcuts

`ShortcutPlan` is data-only and now rejects relative executables, path separators, control
characters, Windows-reserved filename characters, trailing spaces/dots, and oversized arguments.
Build plans only from `app.current_exe()` plus a backend-derived installation index. Do not accept
a target, working directory, or raw argument vector from the renderer. No shortcut installer or
shell execution permission is required by the dialog parity work.

## Verification

Run from this crate directory:

```text
cargo fmt --check
cargo test --all-targets
cargo check --all-targets --features tauri-adapter
cargo clippy --all-targets --all-features -- -D warnings
```
