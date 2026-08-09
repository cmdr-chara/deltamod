# Deltamod Asset Runtime

This crate is intentionally independent of the repository and has `#![forbid(unsafe_code)]`.

## Core

Construct `Roots` from trusted application-owned directories and call `AssetRuntime::new`. The constructor canonicalizes every root. `resolve` accepts only `app://`, `theme://`, and `packet://` requests, validates the decoded relative path, applies the per-kind extension policy, checks every component for symlinks/reparse points and Unix hardlinks, then re-canonicalizes and checks containment. Built-in themes are tried before user themes. Packet requests are restricted to `image/` and image formats.

Use `headers`, `plan_range`, `AssetRuntime::open`, and `Body` to produce bounded streaming responses. Multiple ranges are rejected. Errors intentionally contain no filesystem paths.

`DeepLinkState` is a FIFO state machine: enqueue startup/second-instance URLs before `renderer_ready`, then consume them with `next`. `DeepLinkEvents` is an OS-independent interface for application event delivery.

## Tauri 2.11

`tauri_adapter.rs` is an exact integration reference and is not compiled by the default crate because the core must remain standalone. Copy it into the Tauri application crate, add `deltamod-asset-runtime` and `http = "1"`, then register the scheme during setup with `register_asset_scheme`. The handler returns generic empty responses for rejected requests and never exposes paths.

For desktop deep links, register `tauri-plugin-single-instance` before `tauri-plugin-deep-link` and pass its `argv` through `single_instance_callback`. The plugin callback is only an input adapter; validation and queuing remain in this crate. Also configure the schemes in `tauri.conf.json` and use the deep-link plugin's `get_current`/`on_open_url` APIs for startup and live events.

## Verification

Run explicitly:

```text
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```
