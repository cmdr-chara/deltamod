# Tauri Protocol Adapter

This is an isolated, compile-tested example. The repository was read only; no repository files are required to build it except the path dependency below.

## Explicit cargo path

```text
C:\Users\CharaDreemurr\AppData\Local\Temp\opencode\deltamod-tauri-protocol-adapter
```

Run from that directory:

```text
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

## Exact application changes

1. Add `deltamod-asset-runtime` and `http = "1"` to the Tauri application dependencies.
2. Copy the `AssetState`, `serve`, `register_protocols`, `queue_urls`, and `roots_from_environment` implementations from `src/main.rs` into the application crate.
3. During `setup`, construct `Roots` from the application’s managed/trusted directories, call `AssetRuntime::new`, and `manage(AssetState { runtime })` before registering protocols. Do not derive roots from request URLs.
4. Register only `themeprot` and `packet`; do not register a broad `asset` protocol. `themeprot://<theme-host>/<relative-file>` maps to the runtime’s `theme://` request and `packet://<packet-id>/image/<relative-file>` maps to `packet://`.
5. Keep the `ctx.webview_label() == "main"` and exact `Origin: http://tauri.localhost` checks. If the application enables HTTPS custom protocols, change both the check and CORS response to that exact configured origin.
6. Add the `deep-link` desktop scheme `deltamod-community` to `tauri.conf.json`.
7. If plugin dependencies resolve, register `tauri_plugin_single_instance::init` before `tauri_plugin_deep_link::init`; validate every startup/second-instance URL with `validate_deep_link` and enqueue it in the managed `DeepLinkState`. The renderer calls `renderer_ready` and drains FIFO via the existing application command/event path.

Responses are bounded through the runtime `Body`, include exact `Content-Type`, `Content-Length`, `Accept-Ranges`, and conditional `Content-Range`, and use 403/404/416/500 status mappings without returning filesystem paths.
