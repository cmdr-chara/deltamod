# GameBanana: concrete WebView-free sign-in blocker

Reviewed on 2026-09-30. Native account sign-in is **not implemented**. This is a
missing approved authentication contract, not a claim that GameBanana has no API
or that native sign-in is inherently impossible.

## What the existing implementation actually needs

`src-tauri/src/channels/auth.rs` opens a restricted GameBanana login WebView,
reads its cookies with `cookies_for_url`, clears that browsing session, validates
those cookies through `GameBanana::validate`, requires a nonzero `_idMemberRow`,
and stores the resulting secret as `GameBananaCookies` in the OS credential store.
The GPUIX process has no equivalent browser-owned cookie jar. Opening the regular
system browser does not provide that application's session cookies to GPUIX.
Existing keyring credentials and Nexus PKCE sign-in are not a new GameBanana login.

## Provider evidence and the unresolved boundary

The [documented Core/App/Authenticate endpoint](https://api.gamebanana.com/docs/endpoints/Core/App/Authenticate)
accepts an application API password, application ID and user ID and produces an
authentication token. That is a different credential contract from this backend's
validated cookie header. The endpoint documentation does not establish a
public-client PKCE/browser callback flow or show that this token can replace the
cookie credential used by the current provider adapter.

The repository implementation contains no completed, provider-approved native
client registration, callback/token exchange, or adapter for that separate app
authentication flow. This review did not access the maintainer's GameBanana
account and makes no claim about registrations outside the repository.

## What must be obtained and implemented

Obtain a provider-approved native/public-client authorization flow and its client
registration, callback rules, token scopes, validation and revocation contract.
Alternatively, approve and deploy an HTTPS broker that retains any confidential
application password server-side. A broker is a new operated service, not a
secret that can safely be embedded in a downloadable desktop executable.

Then implement bounded cancellation/timeouts, state binding, PKCE where the
provider supports it, strict redirect validation, identity verification, OS
keyring persistence, logout/revocation, and the corresponding provider adapter.
Test login, cancellation, expired credentials and incorrect-account responses.
Do not reinterpret an arbitrary app token as a cookie header.

No password collection, browser cookie extraction, exported-cookie paste box,
hardcoded application password, hidden WebView or fabricated callback success was
added to bypass this blocker. Tauri's current login remains available while this
contract is unresolved.

Source implementation at the inspected revision:
[auth.rs](https://github.com/cmdr-chara/deltamod/blob/f308372b2e4ef530c5a9fd7f5fa4fa0fa62064b4/src-tauri/src/channels/auth.rs).
