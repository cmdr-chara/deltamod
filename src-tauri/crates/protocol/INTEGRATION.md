# Deltamod protocol domain

This directory is standalone and was built against the current repository contract. It does not modify `src-tauri`.

## Contracts

Deep links are deliberately narrow:

* `deltamod-community://gb/launch?item=123`
* `deltamod-community://gb/import?item=123&file=456&source=https%3A%2F%2Fgamebanana.com%2Fmods%2F123`

`item` and `file` are nonzero decimal `u32` values no greater than `2_000_000_000`. Import sources must be HTTPS on exactly `gamebanana.com` or a subdomain; credentials, fragments, localhost, IP literals, alternate ports, suffix lookalikes, malformed percent encoding, duplicate keys, and unknown keys are rejected. The parser has a 4096-byte URI limit.

Asset requests are `app://<host>/<relative-path>`, `theme://<host>/<relative-path>`, and `packet://<packet-id>/image/<relative-path>`. Host is an opaque nonempty identifier. Paths are decoded once, limited to 512 bytes, and must consist only of normal relative components. `plan_asset` canonicalizes both root and target before checking containment, so symlinks and Windows reparse points cannot escape a validated root. Built-in themes are searched before user themes. Packet requests can only address allowlisted image extensions below `image/`.

`Range` supports one `bytes=` range only: bounded, open-ended, or suffix. Multiple ranges and malformed values are rejected. `plan_range` produces either full, partial, or unsatisfiable (416) planning, including empty files and overflow-safe clamping.

`PendingQueue` is bounded at 256 items, FIFO, and retains startup, deep-link, second-instance, and file-open requests until `mark_renderer_ready`. Never replace it with a single `Option`.

## Recommended dependencies

The domain library itself needs no runtime dependency. In the owning crate, use:

```toml
tauri = { version = "2", features = [] }
tauri-plugin-deep-link = "2"
tauri-plugin-single-instance = { version = "2", features = ["deep-link"] }
```

`tempfile = "3"` is only used by the standalone tests. These versions are stable-Rust compatible; keep the repository lockfile authoritative when integrating.

## Tauri v2 hooks

Configure desktop deep links in `tauri.conf.json`:

```json
{
  "plugins": { "deep-link": { "desktop": { "schemes": ["deltamod-community"] } } }
}
```

Register single-instance before deep-link. The single-instance callback receives `app`, `argv`, and `cwd`; enqueue every relevant argv URL, focus/show `main`, and let the deep-link plugin process its event. In setup, call `app.deep_link().get_current()` for startup URLs and `app.deep_link().on_open_url(|event| ...)` for later URLs. Parse every URL with `parse_deep_link` before enqueueing. The deep-link plugin documentation explicitly recommends manually validating URL format.

For an app-owned custom protocol, register it on the builder with Tauri v2's `register_asynchronous_uri_scheme_protocol` (or the synchronous equivalent only if the handler is nonblocking). The callback receives a `Request`; pass its URI and `Range` header to `parse_asset_request` and `plan_range`. Construct the Tauri `Response` from the resulting status, `Content-Type`, `Content-Length`, `Content-Range`, and bounded file reader. Do not expose a catch-all filesystem protocol or use the generic asset scope for packet roots.

The handler should capture a small immutable domain state, not the broad application state:

```rust
struct ProtocolState {
    roots: deltamod_protocol_domain::AssetRoots,
    pending: deltamod_protocol_domain::PendingQueue,
}
```

Resolve roots once during setup. `app.path().app_data_dir()` is the userData root; append only application-owned fixed subdirectories after creating and canonicalizing them. Packet roots must enter through validated state built from the packet manager's approved directories, keyed by a bounded packet identifier. Do not accept roots from a URI, renderer argument, or arbitrary command payload. If a root is replaced, rebuild state and canonicalize again.

The current repository has no deep-link, single-instance, or custom protocol registration, so the owning agent must add those plugins and their generated capability permissions. Keep the existing `#![forbid(unsafe_code)]`.

## Verification

From this directory run:

```text
cargo test
cargo clippy --all-targets -- -D warnings
```

Errors intentionally contain no filesystem paths or attacker-controlled values.
