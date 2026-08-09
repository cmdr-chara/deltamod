# Deltamod Network Runtime

This crate is intentionally independent from the repository. It is the I/O adapter for the existing network-domain contract.

## Security properties

- `reqwest` uses Rustls, a finite timeout, no automatic redirects, and a bounded redirect loop.
- The initial URL and every resolved `Location` must be HTTPS, credential-free, non-local, and provider-allowed.
- GameBanana cookies are attached only to the exact `gamebanana.com` host and Nexus API keys only to exact `api.nexusmods.com`; redirects are revalidated and credentials are never forwarded to provider CDN/public subdomains.
- API response bodies are capped at 4 MiB using both `Content-Length` and observed-byte accounting.
- Downloads stream into a uniquely-created temporary file, enforce both `Content-Length` and observed-byte limits, emit legacy progress envelopes, and remove partial files on cancel/failure.
- No environment credential fallback exists. Production adapters should implement `SecretStore` with the OS keyring.
- `SsoWebSocket`/`SsoSession` are seams for a pinned, authenticated Nexus SSO implementation; this crate does not fake browser or SSO success.

## Adapter mapping

`Client::download` maps to the legacy `operation_id`, `ProgressEnvelope`, and bounded download result. `Nexus::{validate,resolve_primary_download,browse,status}`, `ModDb::browse`, and the GameBanana validation/comment/like/collection methods provide the confirmed Node endpoint contracts. HTTP failures retain status, retry-after, and `x-rl-*` quota data in `RuntimeError::Http`.

## Explicit limitations

Interactive GameBanana login still requires a restricted Tauri authentication webview that exports only cookies applicable to `https://gamebanana.com/`, validates `Member/UiConfig`, and then writes through the OS keyring adapter. Nexus SSO still requires a bounded WebSocket adapter and registered application slug. Neither flow reports fake success when those adapters are absent.

## Channel wiring

- `loginGamebanana`: unavailable until the restricted webview above exists; never accept renderer-supplied cookies.
- `logoutGamebanana`: delete `GameBananaCookies`; report keyring failure rather than success.
- `eraseGamebananaCache`: clear only an in-memory validated UI-config cache. It must not delete the keyring credential.
- `validateGamebananaToken`: load `GameBananaCookies`, call `GameBanana::validate`, and return whether `_idMemberRow > 0`; missing/rejected credentials return `false` without exposing the cookie.
- `leaveCommentGamebanana`: run domain target/text normalization and escaping, then `GameBanana::leave_comment`.
- `gbLikeMod`: use `GameBanana::like_target`; its `{status,data}` shape preserves `already_liked` responses.
- Collection list/create/add/delete map to the fixed-endpoint GameBanana methods. Collection inspection/download-all additionally needs the mod installation/download orchestration owned outside this crate.
- `modSources:startNexusSso` and cancellation require one process-wide bounded SSO controller: one active request, five-minute timeout, heartbeat, socket close on cancel/window destruction, exact endpoint, parsed credential, API validation before keyring storage. Until the app slug and WebSocket adapter exist, return `NEXUS_SSO_NOT_REGISTERED`.
- `modSources:clearNexusKey`: cancel SSO, delete `NexusSsoKey`, then return `true`.
- `modSources:downloadNexus`: validate domain/mod/operation/source-page inputs, load the key, call `Nexus::resolve_primary_download`, download with no API key attached to the CDN request, and hand the bounded temporary archive to the existing native mod import runtime. Emit progress with the unchanged operation ID.
