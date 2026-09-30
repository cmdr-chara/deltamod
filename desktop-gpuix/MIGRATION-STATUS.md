# GPUIX migration status

GPUIX is a migration preview, not yet the production replacement for Tauri.
The source profile remains read-only. Explicit managed operations continue to use
the existing Rust transaction, recovery and credential implementations.

## Desktop handoffs

The frontend now acquires an instance listener before starting a writable backend.
A second launch with the same canonical managed data root forwards its bounded
requests to the running frontend instead of starting another backend. Development
benchmarks cannot carry open/import requests or bypass instance coordination.

The listener is loopback-only, scoped to the current user and managed root, and
uses a private capability with domain-separated HMACs on requests and receipts.
The capability is never sent over the socket. Each connection has a three-second
deadline and a 160 KiB frame limit. The inbox accepts at most 16 pending requests
and 1024 unique requests per session. Port collisions and unknown acknowledgements
fail closed. There is no automatic forwarding retry or stale PID-lock deletion.

Incoming requests are queued for review. Queue acceptance does **not** mean that an
archive was opened or installed. A pending modal, import, or review cannot be
replaced by another request. Duplicate handoffs are suppressed for the session.
An import already attempted through the inbox cannot silently run a second time,
including after a lost acknowledgement.

Supported request forms:

```sh
# Read-only navigation, reviewed in the application
npm run dev -- --open 'deltamod-gpuix-preview://mod?id=42'

# Local archive, followed by explicit import/replacement confirmation
npm run dev -- --import /absolute/path/to/package.modarchive

# Production one-click URL, parsed again by the Rust runtime before confirmation
npm run dev -- --protocol 'deltamod-community://gb/Mod/42/https://gamebanana.com/dl/12'
```

The numeric URL above is a syntax example, not a recommended download.
Windows archive handoffs require a drive-rooted local path. Network shares,
alternate data streams, malformed file URLs, control characters and duplicate
launch arguments are rejected. The native archive validator and transactional
importer remain authoritative. The headless picker allowlist also accepts
`.modarchive`, matching the existing Tauri association.

Import review may be cancelled. Remote downloads use the existing native cancel
operation. An in-flight atomic local archive import cannot be interrupted from
the dialog. The dialog remains open until native acknowledgement. An explicit
`imported: false` becomes a skipped/cancelled result, never a success message.

Remaining handoff work: native macOS Launch Services open-file/open-URL callbacks,
foregrounding an existing native window, the trusted `.deltamod-open` CLI marker,
and installed Windows/macOS/Linux validation. Instance coordination is GPUIX-only,
not cross-shell mutual exclusion with Tauri. Do not run both against the same
writable managed root. A concurrent first-ever capability-file creation can fail
closed rather than start another backend. Windows inherited directory ACLs still
need native validation. These limitations prevent production cutover.

## Existing translations

Settings exposes the repository's existing languages: English, Italian, German,
Spanish, French, Japanese, Polish and Brazilian Portuguese. Native preview
preferences persist those locale IDs without changing the schema or Tauri data.

`npm run sync:locales` reads only the fixed `web/langs/*/language.json` sources at
build time. JSON comments and trailing commas are parsed without executing source
code. A translation is reused only for an exact English string with the same
source key, matching placeholders and no HTML. Ambiguous duplicate English strings
are skipped. Existing reviewed GPUIX English/Italian messages remain authoritative.
Unmatched text remains English. Provider names, paths and other user data are not
translated. This is source reuse, **not full translation coverage** for new GPUIX
screens. The generated module is ignored and recreated by dev/build/typecheck.

No generated translation bundle or upstream language file is edited or committed.
Full-catalogue generation still needs to run against a complete repository checkout.

## Native theme semantics

Still-image previews prefer the source `previewBackground` and otherwise use the
normal background. They retain the native raster-path containment and byte limits.
The preview exposes bounded theme names, descriptions, track names, cue-time
metadata and up to 32 credit entries. Existing resource metadata, including credit
URLs, is not rewritten. The UI does not turn arbitrary credit URLs into an opener.

A theme's six-digit hexadecimal or integer RGB color can be applied through the
existing native-acknowledged preferences command. Soul color and boot sync time
are preserved as metadata. They are not claims that soul effects or media cues
are executing. No arbitrary CSS is evaluated. Existing reduced-motion and opaque
surface preferences remain in force.

Native video/audio decoding, playback synchronization, cue execution and custom
first-party visual effects are **not implemented by this batch**. GPUIX media
parity needs a contained native player/custom-element integration, not HTML media
elements or a hidden WebView. Arbitrary browser CSS compatibility is outside the
native rendering contract.

## Packaging and updates

Packaging adds the missing native preview image resource layout, product/license
metadata and the `.modarchive` association behind the existing cutover switch.
The production switch rejects missing standalone locks. macOS cutover is refused
until the native Launch Services event hook exists. These guards are not proof of
platform packaging readiness.

React and native GPUIX remain pinned to 0.10.0. Root npm/Cargo locks and updater
trust material are unchanged. Standalone npm/Cargo locks are still required. The
GPUIX native runtime/sidecar must be staged and checked in actual installed
packages. Tool provenance and complete license trees remain release requirements.
No release or new update feed is published by this change.

The existing GPUIX-specific updater endpoint and public key are retained. A signed
installed update/restart/rollback rehearsal, platform/architecture validation and
valid signing authority are still cutover gates.

## Authentication and shared runtime

Existing GameBanana credentials stay in the same OS keyring. The current login
implementation obtains cookies from a dedicated Tauri WebView, validates them and
stores them natively. A provider-approved system-browser/native-client login flow
has not been established here. This is not proof that every WebView-free approach
is impossible, and no browser-cookie extraction workaround is added.

Nexus system-browser PKCE and Windows controller-mode operations already existed
on the branch and remain available. The headless backend still reuses the Tauri
shell library. Moving reusable application behavior into shared Rust crates remains
work after the functional migration is sufficiently established.

## Validation and cutover

Focused validation covers handoff bounds, deduplication, modal ownership, cancelled
reviews, native acknowledgement handling, source-catalogue parsing and authenticated
loopback forwarding. TypeScript syntax parsing is not a typecheck against installed
GPUIX, and loopback transport checks are not native desktop acceptance.

Still required: real GPUIX 0.10 typechecking, Rust compilation, native renderer smoke,
keyboard/screen-reader/controller checks, installed packages and signed updates on
supported targets. Only then compare production-equivalent builds on the same host,
retaining all startup/memory/size/render samples. No performance win is claimed.
Tauri must remain available as the rollback path until those gates are demonstrated.
