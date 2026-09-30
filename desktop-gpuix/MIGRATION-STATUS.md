# GPUIX migration status

Updated 2026-09-30. **Preview, not production-ready. Tauri remains the production
and rollback shell.** PR #122 stays a draft. This update builds on the inspected
`f308372b2e4ef530c5a9fd7f5fa4fa0fa62064b4` branch, rather than repeating its work.

## Already present and preserved

The GPUIX UI and managed host already reuse lifecycle/import/patch/recovery,
launch, storage and OS-keyring operations. Source-profile attachment remains
read-only, preview preferences have a separate root, and managed mutations remain
explicit. Existing Nexus browser/PKCE sign-in is retained.

Bounded, authenticated single-instance delivery and the review inbox preserve
protocol/archive validation, duplicate suppression, operation acknowledgements
and modal/busy guards. The CLI accepts reviewed preview links, community protocol
links and absolute `.modarchive` paths. A handoff never silently bypasses review.

All eight existing locale selections and exact source-catalogue reuse remain.
Contained still-image previews, metadata, credits, accent/soul colors and
synchronization metadata remain available. Production Tauri dependencies, signing
configuration, release assets and updater feed are not retired or replaced.

## Implemented in this continuation

- **Native media preview:** bounded FFmpeg-to-GPUIX BGRA video frames, owned FFplay
  audio, explicit Play/Stop/mute/volume, cue seeking, cue/soul-color transition,
  reduced-motion behavior, stale-frame cancellation and bounded process reaping.
  No WebView, arbitrary shell, remote media URL or automatic codec download.
- **Foreground delivery:** authenticated secondary launches, including empty
  launches, request native window activation. Requests arriving before mount are
  coalesced. Refused activation never retries an accepted import. Initial
  no-handoff launches retain the `--no-focus` setting.
- **Shared writer exclusion:** a standard-library-only `native/desktop-runtime`
  lease is acquired by both the Tauri and GPUIX AppState paths before runtime
  initialization/recovery. The OS releases it on handle close, with no PID-file
  stealing or unlink-based reclamation. The previous state implementation is
  moved byte-for-byte to `state_impl.rs`. This only protects cooperating builds.
- **Backend correctness:** fixed headless credential clearing to use
  `self.state.credentials`. Rust 1.89 is declared for the standard-library lock
  API. The standalone host inherits the vendored GLib fix explicitly at its own
  workspace root.
- **Packaging inputs:** exact native-addon staging, target/header/digest checks,
  dependency licenses, optional reviewed codec tree, stale-lock/binary rejection,
  and explicit Windows x64/macOS arm64/Linux x64 package formats. Full theme media
  is staged. Unsupported targets and premature production takeover fail closed.
- **Updater gating:** unsigned/unverified previews and unsupported formats cannot
  contact the update feed or run an installer. GPUIX's existing independent feed
  and verification key are preserved. Package-format mismatches remain errors.
- **Localization/accessibility:** 22 new native-control/help/error messages have
  entries in all eight languages. Source-catalogue fallback remains intact.
  Disabled buttons no longer advertise an actionable click handler or misuse
  selected-state semantics. Page headings have roles and dialogs restore valid
  opener focus after the last modal closes.
- **Dependency resolution workflow:** added a guarded standalone npm/Cargo lock
  resolver and lock-required build/staging/package gates. This is not a claim
  that the missing locks have already been generated.

## Remaining production blockers

1. **Media parity and distribution:** reviewed codec builds/licenses/dependencies,
   actual GPU/audio-device validation on every target, long-running A/V sync,
   global background loops/sound effects and full theme effects parity. The native
   preview has separate decoder clocks. See [media/packaging readiness](PACKAGING-READINESS.md).
2. **GameBanana native sign-in:** the current provider consumes validated browser
   cookies, while the documented app-auth endpoint uses a different contract.
   Registration/token-flow/adapter approval is missing. See the
   [concrete blocker and provider evidence](GAMEBANANA-AUTH-BLOCKER.md).
3. **OS integration:** macOS Launch Services open-file/open-URL callbacks, trusted
   `.deltamod-open` handling, and installed protocol/file-association/foreground
   smoke evidence. Production association takeover stays disabled. Older Tauri
   releases do not acquire the new writer lease and must still be closed before
   the same managed data root is used by GPUIX.
4. **Standalone locks:** `desktop-gpuix/package-lock.json` and
   `desktop-gpuix/native/Cargo.lock` remain absent. They require a real resolver
   run and review, not copied production locks or invented checksums.
5. **Platform release/updater trust:** clean-host install/uninstall, matching
   sidecars, code signing, notarization where applicable, authenticated update,
   restart and rollback rehearsal. No runnable signed release is established by
   header checks or a package configuration. Linux DEB remains non-self-updating.
6. **Full backend extraction:** the host still links `deltamod-tauri-shell`.
   The shared lease is an extracted boundary, not the complete runtime. State
   updater/lifecycle implementations and mixed import/Nexus channel wrappers
   still couple business code to Tauri. No fake Tauri shim or duplicated business
   implementation was introduced to hide that dependency.
7. **Complete localization/accessibility:** untranslated legacy/runtime strings,
   native screen-reader trees, keyboard traversal/focus behavior and long/localized
   layout need full platform audits. Additive catalogue coverage is not that audit.

## Focused validation in this run

Twenty-six focused Node tests passed across native frame/media ownership,
foreground/focus helpers, runtime layout, updater gating and native translations.
The real FFmpeg smoke generated and decoded a small clip through the application's
BGRA frame pipeline. The first media test pass exposed a mock signal-default
mistake, which was corrected before rerunning only that suite.

Six changed TSX files passed syntax transpilation. Changed JavaScript passed syntax
checks, JSON/TOML parsed, and original Rust blob comparisons confirmed that the
credential correction and shell MSRV edit introduce no unrelated rewrites.
These are **not** dependency-aware TypeScript, Cargo, GPUIX native-render or
installed-package passes. No CI run was repeatedly tested, watched or polled.

`npm run lock:resolve` stopped at `spawnSync cargo ENOENT` before creating locks.
The environment had no Rust/Cargo toolchain or direct dependency-network access.
Rust lease tests are included but were not executed. Native compilation, full
existing suites and signing/installer tests were not run here. No performance
improvement or Tauri-retirement readiness is claimed.
