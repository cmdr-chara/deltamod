# Tauri retirement and hardening audit — 2026-09-28

Base: `DeltaMaster@d451315768e9043cdf5b2b635398a2350f3f1745`.
Implementation commit: `c0e76c9e02101bffb2c2656388e9c0322e64829c`.
This is an implemented source change, not a signed release or a completed security audit.

## Implemented

- Removed Electron runtime modules, preload/tracer windows, destructive legacy launch
  scripts, Electron release workflows, builder configuration and dependencies.
  Preserved shared renderer/domain/provider/security tooling and historical benchmarks.
- Made the Tauri bridge authoritative. Retired the unavailable reset, developer-reboot
  and patch-continuation UI paths. Kept real native capability checks rather than
  reporting unsupported commands as success.
- Added a regression guard against Electron dependency/runtime reintroduction.
- Bound updater signature reads and artifact traversal, tightened target/version
  matching, rejected links/special files, and made malformed static IPC evidence fail.
- Removed the credential store's destructive replacement compensation. Presence now
  comes from the secure store instead of a separate mutable metadata flag. Unknown
  metadata versions fail before mutation. OAuth token debug output is redacted.
- Added bounded VDF parsing, XDG/legacy/Flatpak Linux Steam roots, canonical library
  deduplication, Steam handoff on all supported hosts, strict app IDs, checked Unix
  opener status, bounded catalog reads and non-Unicode environment handling.
- Held direct-launch exclusion through lifecycle finalization. Steam remains an
  explicit client handoff, not a claim that the opener process owns the game lifetime.
- Kept staging and Tauri CLI targets identical, handled Windows invocation without a
  shell, selected platform bundle formats, and scoped the Windows controller utility
  to the Windows resource override.
- Preserved all Windows resource overrides when disabling signing for the CI-only
  diagnostic package, with an executable configuration regression test.
- Replaced two smoke-fixture PID-alive assumptions with explicit initialization
  evidence. This tests the runner, not an actual installed app.

## Validation performed

| Check | Actual result |
| --- | --- |
| Integrated `npm test` | 449 Vitest tests passed in 61 files, plus secure-updater checks and 36 Node audit tests |
| Generated-input coverage | 20,000 deterministic JavaScript cases and 10,000 deterministic VDF cases. Not coverage-guided fuzzing |
| `npm run typecheck` | Passed |
| `npm run verify:tauri:contract` and parity harness | Passed. 129 invoke channels, 18 events, 123 implemented classifications and six explicit rejections. Static evidence only |
| Rust credentials and updater/launch runtime | 30 tests and strict all-target Clippy passed on Linux x64 with locked dependencies |
| Native workers | All five debug worker binaries built on Linux x64 |
| Formatting | Full Tauri workspace `cargo fmt --all --check` passed |
| Tauri CLI wrapper | Real installed CLI `build --help` succeeded through the wrapper |
| Full Linux GUI build | Not run: GTK 3 and WebKitGTK 4.1 development packages are absent locally |
| Windows, macOS x64/arm64 installed apps | Not run locally. Native CI, signing and installed-package evidence remain required |

The follow-up Windows diagnostic configuration regression passed separately.

The dependency-pruned local installation was used for JavaScript checks. The lockfile
contains 186 package records instead of 454: 268 removed, no new records, no retained
package version changes. This is not a current vulnerability-database audit result.

## Measured tooling performance

Raw samples: [`tauri-retirement-20260928.json`](../benchmarks/tooling/tauri-retirement-20260928.json).
Reproduce from a full checkout:

```console
node --expose-gc benchmarks/tooling/updater-manifest.cjs d451315768e9043cdf5b2b635398a2350f3f1745
```

Synthetic three-target manifest median: 0.092 ms baseline and 0.286 ms candidate.
The additional validation costs about 0.194 ms in this run. Rejecting a synthetic
32 MiB signature took 691.3 ms / 109.6 MiB peak process RSS before, versus
3.8 ms / 45.8 MiB after. Oversized-input results are single child-process samples,
not statistically controlled app benchmarks. The script records source blob hashes.

No startup, installer-size or desktop-memory improvement is claimed. The immutable
Electron/Tauri desktop comparison remains unchanged.

## Remaining actionable work

- The follow-up [recovery/updater hardening](RECOVERY-UPDATER-HARDENING.md) implements
  journal containment, backup preservation, kernel leases and in-flight updater
  cancellation/budgets. Ambiguous v1 recovery still requires intervention; real
  installed-updater and platform acceptance remain open.
- Finish collection restore, installation shortcuts and UndertaleModTool installation
  opening. Those three controls remain capability-gated. Retired channel names are
  explicitly rejected at the native boundary for stale-client safety.
- Run full native shell tests and installed-package checks on Windows, Linux and both
  macOS architectures. macOS arm64 CSX remains deliberately unavailable. Steam fake-
  platform tests do not establish real client/protocol behavior on those machines.
- Complete signed updater, platform publisher/notarization, data-preservation and
  rollback gates before stable release. Source retirement does not waive them.
- Expand coverage-guided native fuzzing and adversarial recovery testing. This audit
  does not claim that no worthwhile defects remain.

No itch.io integration was implemented. Previously reviewed upstream issue/commit
behavior was used only as problem context. No upstream implementation was copied,
cherry-picked or transplanted. New fixes stay within the Community Rust/Tauri model.
