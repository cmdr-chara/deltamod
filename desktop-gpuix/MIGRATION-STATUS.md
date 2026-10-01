# GPUIX migration status

Prepared from `46ba384cf105c47210ba84ed7ee0e25a6a25797f` on 2026-09-30.
**Preview only. Tauri remains the production application and rollback shell.**
The continuation below was prepared locally. Its publication and native acceptance
must not be inferred from this document.

## Retained implementation

Source-profile attachment remains read-only. Managed lifecycle, archive import,
patching, recovery, launch, credentials and Nexus PKCE continue to use the existing
Rust implementations. Authenticated single-instance forwarding, reviewed inboxes,
explicit native acknowledgements, reduced motion, contained media resources and
the existing eight-language catalogues remain in place.

Production file associations, updater authority, release assets and signing
configuration are not replaced. Existing stable updater and cutover guards remain
closed. Older Tauri builds do not participate in the new cooperative writer lease
and must not run against the same writable root.

## Prepared continuation

### Independent Rust build boundary

`native/app-runtime` defines `deltamod-app-runtime`. It compiles the same canonical
source used by the shell, without running Tauri's build script. The GPUIX host's
existing extern-crate alias now names this independent package. It is an alias,
not a dependency on the actual `deltamod-tauri-shell` package.

Tauri window lifecycle, updater, AppHandle and plugin-backed import/Nexus wrappers
are included only when the real shell build sets its private compilation cfg.
The state initializer accepts the existing game lifecycle adapter without requiring
Tauri in the independent build. Recovery and transaction logic are not copied or
reimplemented. The shared writer lease remains acquired before initialization.
A missing library-level platform-name helper used by shared channels is supplied.

`npm run check:backend` inspects Cargo's actual resolved normal/build graph and
rejects Tauri/Wry/WebView dependencies, including through aliases. Its fixture
tests passed, but the real graph and Rust compilation could not run without Cargo.
Source location under src-tauri is retained deliberately. This is an independent
build boundary, not a physical relocation of every backend source file.

### Native macOS handoffs and trusted CLI wake requests

A small windowless AppKit launcher handles cold/warm open-file, open-URL and reopen
events and owns the renderer process. A private readiness handshake carries the
managed-root identity, including custom roots. Bounded forward-only helpers send
requests through the authenticated inbox. They cannot elect a new primary if the
original exits, and unknown receipts are never automatically retried.

The launcher is built and target-checked during macOS runtime staging. Its digest
is bound to the runtime manifest. The app bundle's main binary is the launcher,
with the renderer and backend retained separately. Only the GPUIX preview protocol
is declared. Production archive/community protocol takeover remains disabled.

Trusted `.deltamod-open` markers now request foregrounding. The fixed magic bytes,
CLI temporary directory, local path, regular file, ownership and unchanged inode
are checked. The marker is removed only after successful primary initialization
or an authenticated secondary receipt. Failed or uncertain delivery retains it.
Windows inherited ACL/reparse behavior still needs native verification.

The Foundation policy checks compiled and ran on Linux. The AppKit launcher
passed syntax parsing only. It has not been typechecked against AppKit or run on
macOS. Installed callbacks, Finder/Dock behavior and process closure are gates,
not completed platform acceptance. See [macOS details](macos/README.md).

### Media ownership and repeat playback

One coordinator owns theme playback across player instances. Replacing a theme
waits for the previous decoder processes to close, not merely for a termination
signal. Unreaped processes block replacement. Disposed or superseded requests
cannot start stale playback.

Explicit repeat playback uses fixed FFmpeg/FFplay loop options and retains the
one-hour lifetime bound. Zero volume does not spawn a silent audio process.
Playback callback failures stop owned processes. The repeat control uses the
existing eight-language message catalogue. Native launcher failure messages also
cover those eight languages, selected from the OS locale with English fallback.

A real generated six-frame clip decoded into fourteen BGRA frames with repeat
and left zero child processes after stop. This verifies decoder repeat and owned
cleanup only. It does not verify GPUI painting, audio output, clock synchronization,
full-screen theme loops, sound effects or all first-party visual effects.

### Standalone dependency resolution

`lock:npm` and `lock:cargo` resolve independently. `lock:resolve` attempts both and
reports combined failures, rather than allowing a missing Cargo executable to
prevent the npm attempt. All modes protect the existing root/Tauri/native locks.
No lock or checksum is fabricated, and packaging still requires both real locks.

The actual npm attempt failed with `EAI_AGAIN` resolving registry.npmjs.org. Cargo
failed with `ENOENT`. Both standalone locks remain absent. GPUIX stays pinned to
0.10.0 and the other existing dependency versions are unchanged.

## Validation actually performed for this continuation

- 39 focused Node tests passed, with no failures or skips. Coverage includes real
  authenticated loopback forwarding and temporary-file marker safety, plus media
  replacement, handshake, resolver and dependency-graph fixtures.
- 29 Swift/Foundation policy assertions passed on Linux. AppKit launcher syntax
  parsing passed. Neither result is macOS application typechecking or execution.
- Real FFmpeg repeat/stop smoke: 14 frames, 921600 bytes per frame, no remaining
  children. No audible output or GPU renderer was tested.
- 23 materialized JavaScript files passed syntax checks, two TSX files passed
  syntax transpilation and five declaration files passed syntax parsing. Two
  TOML manifests and one JSON manifest parsed. These counts include unchanged
  local prerequisites, not only changed files.
- Actual standalone resolution and real backend graph check failed for the
  concrete environment reasons above. CI was not watched or polled.

## Remaining production gates

1. Resolve/review the standalone npm/Cargo locks, compile the independent Rust
   host, check the real dependency closure, and typecheck against GPUIX packages.
   Existing host test-fixture constructor calls also need reconciliation with the
   four-argument constructor before a native test pass can be claimed.
2. Build and run native GPUIX on each target. Verify new AppKit callbacks and all
   installed cold/warm handoffs, foreground behavior, archive associations, Windows
   ACLs and no orphan processes. macOS Intel is not an established package target.
3. Review/distribute codec binaries and their dependencies/licenses. Validate real
   GPU/audio output, long-running A/V synchronization, global theme loops, sound
   effects and remaining first-party effects. Arbitrary browser CSS is not native
   parity and no hidden WebView is introduced.
4. Establish provider-approved WebView-free GameBanana authorization and the
   cookie/token backend adapter. The existing [concrete blocker](GAMEBANANA-AUTH-BLOCKER.md)
   remains unresolved. No confidential app password or browser-cookie extraction
   workaround is added.
5. Install/uninstall packages on clean hosts, verify all target-matched tools and
   complete license trees, sign/notarize with real publisher authority, and rehearse
   authenticated update, signature rejection, restart and rollback. Linux DEB is
   not a self-updating format. See [packaging requirements](PACKAGING-READINESS.md).
6. Complete legacy/runtime translation coverage, screen-reader, keyboard/controller
   focus and long/localized layout audits. Additive messages are not that audit.

Do not retire Tauri, enable stable GPUIX updates or claim performance improvement
until those production gates have real evidence.
