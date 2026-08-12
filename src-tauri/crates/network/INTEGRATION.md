# Integration

This crate is the pure policy/domain boundary. It performs no network, keyring, or filesystem I/O.

## Existing channels

- `modSources:browse` maps to `BrowseRequest` and `CurrentEnvelope`; enforce one bounded page per request.
- `modSources:downloadNexus` maps to `DownloadRequest`, `ProgressEnvelope`, `ErrorEnvelope`, and `SizeAccounting`.
- `modSources:startNexusSso` retains its compatibility channel name but drives OAuth Authorization Code with PKCE S256 through the fixed `http://127.0.0.1:52817/callback` listener.
- `modSources:cancelNexusSso` cancels the active local callback wait and must never select a dynamic fallback port.
- GameBanana comment submission uses `normalize_comment_target`, `normalize_comment`, and `escape_comment_html` before JSON encoding.
- Existing `mod-source-progress`, `hash-progress`, and import progress events should carry `operation_id` unchanged.

## Required adapters

Implement `HttpsTransport` with `reqwest` using manual redirects. Validate the initial URL and every `Location` with `validate_https_url`/`validate_redirect`; never let the client follow redirects automatically. Stream response chunks through `SizeAccounting` before writing.

OAuth requests use only the fixed Nexus authorization/token endpoints. Bind the callback only to IPv4 loopback, validate `state`, use PKCE S256, reject redirects during token exchange, and store the validated token bundle in the OS keyring.

Implement `SecretStore` with the platform keyring adapter. Do not pass the process environment wholesale to child processes; use `filter_secret_environment`. Store Nexus credentials only after OAuth token exchange and API validation.

HTTP adapters should parse `Retry-After` and `x-rl-*` headers into `ErrorEnvelope`, then use `pause_for_retry` for bounded scheduling. No fake adapter belongs in this crate.
