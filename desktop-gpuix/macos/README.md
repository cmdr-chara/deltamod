# Native macOS desktop delivery

The app bundle starts `deltamod-gpuix-launcher`, a windowless AppKit delegate.
The launcher owns one GPUIX renderer child. It receives cold/warm open-file,
open-URL and reopen events without a WebView or GPUIX delegate swizzling.

Up to 16 requests wait for a private readiness nonce written after frontend
initialization. The nonce is removed from the frontend environment before its
Rust host and media processes are spawned. This is a delivery handshake, not a
paint benchmark. Separate forwarders use `--forward-only`, so the death of the
primary cannot elect an accidental replacement writer. A failed/unknown receipt
is never retried. Archives still require review in the normal inbox.

A forwarder has an eight-second lifetime bound. The launcher's bootstrap timeout
never kills a renderer that could be performing recovery. An explicit OS quit
uses the frontend's existing SIGTERM/owned-shutdown path. Diagnostics omit paths,
URLs and the nonce. Error dialogs are bounded to one per launcher session.

Build with `npm run build:launcher` on a supported macOS arm64 host before staging.
`stage:runtime` runs that build automatically. Packaging verifies the launcher's
architecture and binds its hash to the staged runtime manifest. Sign the launcher,
renderer, backend and native addon as part of the complete app bundle. No signing
or notarization authority is embedded here.

## Required native validation

This source has not been compiled or executed on macOS in the preparation
environment. The new launcher must pass cold/warm `open -a` URL and archive tests,
Finder reopen/foreground tests, ready-timeout and process-exit tests, bundle/Dock
identity checks, and signed installed package validation. Include the case where
a developer-launched renderer already owns the same data root. Verify closure
leaves no launcher/forwarder processes. Node transport tests are not these tests.

The launcher adds a native process. No lower-memory claim is made. Native failure
alerts use the OS language with the same eight-language set and an English
fallback. Localized native layout still requires platform validation. Only the
preview protocol is registered. Production `.modarchive` and community protocol
ownership remain with Tauri until all cutover gates pass.

Primary API references:
- https://developer.apple.com/documentation/appkit/nsapplicationdelegate/application(_:openfiles:)
- https://developer.apple.com/documentation/appkit/nsapplicationdelegate/applicationshouldhandlereopen(_:hasvisiblewindows:)
- https://developer.apple.com/documentation/foundation/process/terminationhandler
