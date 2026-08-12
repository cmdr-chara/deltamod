# Integration

This crate is the pure policy/domain boundary. It performs no network, keyring, filesystem, or websocket I/O.

## Existing channels

- `modSources:browse` maps to `BrowseRequest` and `CurrentEnvelope`; enforce one bounded page per request.
- `modSources:downloadNexus` maps to `DownloadRequest`, `ProgressEnvelope`, `ErrorEnvelope`, and `SizeAccounting`.
- `modSources:startNexusSso` drives `sso_transition`; websocket messages pass through `parse_sso_message`.
- `modSources:cancelNexusSso` emits `SsoEvent::Cancel` and must close the active socket.
- GameBanana comment submission uses `normalize_comment_target`, `normalize_comment`, and `escape_comment_html` before JSON encoding.
- Existing `mod-source-progress`, `hash-progress`, and import progress events should carry `operation_id` unchanged.

## Required adapters

Implement `HttpsTransport` with `reqwest` using manual redirects. Validate the initial URL and every `Location` with `validate_https_url`/`validate_redirect`; never let the client follow redirects automatically. Stream response chunks through `SizeAccounting` before writing.

Implement `WebSocketTransport` with `tokio-tungstenite` or an equivalent websocket client. Connect only to `SSO_ENDPOINT`, send the generated request id and app id, ping while awaiting authorization, and map close/error/timeout/cancel events to the state machine.

Implement `SecretStore` with the platform keyring adapter. Do not pass the process environment wholesale to child processes; use `filter_secret_environment`. Store Nexus credentials only after SSO validation.

HTTP adapters should parse `Retry-After` and `x-rl-*` headers into `ErrorEnvelope`, then use `pause_for_retry` for bounded scheduling. No fake adapter belongs in this crate.
