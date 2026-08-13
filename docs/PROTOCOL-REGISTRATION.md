# Protocol Registration

`deltamod-community://` is an application protocol, not a file association. The Tauri shell must register it before the first release build with `tauri-plugin-deep-link` and forward every launch URL to the existing `protocol:queueDeepLink` domain channel.

The startup sequence is fixed:

1. Register `deltamod-community` with the deep-link plugin during `tauri::Builder` construction.
2. On a cold start, collect plugin URLs before the webview is ready and retain at most 256 validated URLs.
3. After the renderer invokes `protocol:rendererReady`, deliver the retained URLs through the existing pending queue.
4. On Windows, the plugin's single-instance callback forwards subsequent URLs to the first process. On Linux and macOS, the same callback handles the desktop activation event.
5. Parse and validate each URL with `deltamod-protocol-domain`; reject non-HTTPS payloads, malformed percent encoding, unknown actions, and payloads above the domain limit.

The required dependency is `tauri-plugin-deep-link` with its platform integration enabled in `src-tauri/Cargo.toml`. The plugin's generated platform registration must be included in the bundle; a JavaScript-only `window.location` check is not sufficient. The release gate must execute a cold launch and a second-instance launch on every desktop target.
