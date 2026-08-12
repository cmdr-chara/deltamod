# Deltamod Network Runtime

This crate is intentionally independent from the repository. It is the I/O adapter for the existing network-domain contract.

## Security properties

- `reqwest` uses Rustls, a finite timeout, no automatic redirects, and a bounded redirect loop.
- The initial URL and every resolved `Location` must be HTTPS, credential-free, non-local, and provider-allowed.
- GameBanana cookies are attached only to the exact `gamebanana.com` host and Nexus OAuth Bearer tokens only to exact `api.nexusmods.com`; redirects are revalidated and credentials are never forwarded to provider CDN/public subdomains.
- API response bodies are capped at 4 MiB using both `Content-Length` and observed-byte accounting.
- Downloads stream into a uniquely-created temporary file, enforce both `Content-Length` and observed-byte limits, emit legacy progress envelopes, and remove partial files on cancel/failure.
- No environment credential fallback exists. Production adapters should implement `SecretStore` with the OS keyring.
- Nexus account authorization is implemented by the Tauri channel through a fixed loopback OAuth PKCE callback; this runtime accepts only the resulting Bearer access token.

## Adapter mapping

`Client::download` maps to the legacy `operation_id`, `ProgressEnvelope`, and bounded download result. `Nexus::{validate,resolve_primary_download,browse,status}`, `ModDb::browse`, and the GameBanana validation/comment/like/collection methods provide the confirmed Node endpoint contracts. HTTP failures retain status, retry-after, and `x-rl-*` quota data in `RuntimeError::Http`.

## Explicit limitations

Interactive GameBanana login uses a restricted Tauri authentication webview that exports only cookies applicable to `https://gamebanana.com/`, validates `Member/UiConfig`, and then writes through the OS keyring adapter. Nexus OAuth remains disabled until a registered public client ID is configured. Neither flow reports fake success when required configuration is absent.

## Channel wiring

- `loginGamebanana`: unavailable until the restricted webview above exists; never accept renderer-supplied cookies.
- `logoutGamebanana`: delete `GameBananaCookies`; report keyring failure rather than success.
- `eraseGamebananaCache`: clear only an in-memory validated UI-config cache. It must not delete the keyring credential.
- `validateGamebananaToken`: load `GameBananaCookies`, call `GameBanana::validate`, and return whether `_idMemberRow > 0`; missing/rejected credentials return `false` without exposing the cookie.
- `leaveCommentGamebanana`: run domain target/text normalization and escaping, then `GameBanana::leave_comment`.
- `gbLikeMod`: use `GameBanana::like_target`; its `{status,data}` shape preserves `already_liked` responses.
- Collection list/create/add/delete map to the fixed-endpoint GameBanana methods. Collection inspection/download-all additionally needs the mod installation/download orchestration owned outside this crate.
- `modSources:startNexusSso` and cancellation use one process-wide bounded OAuth controller: one active request, five-minute timeout, exact authorization/token endpoints, fixed IPv4 loopback callback, verified `state`, PKCE S256, and API validation before keyring storage. Until the OAuth client ID exists, return `NEXUS_SSO_NOT_REGISTERED`.
- `modSources:clearNexusKey`: cancel OAuth, delete `NexusOAuthTokens` and the retired legacy SSO entry, then return `true`.
- `modSources:downloadNexus`: validate domain/mod/operation/source-page inputs, load a current access token, call `Nexus::resolve_primary_download`, download with no Bearer token attached to the CDN request, and hand the bounded temporary archive to the existing native mod import runtime. Emit progress with the unchanged operation ID.
