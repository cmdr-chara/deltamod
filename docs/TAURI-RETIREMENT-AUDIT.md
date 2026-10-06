# Tauri retirement and hardening audit — 2026-09-28

Base: `DeltaMaster@d451315768e9043cdf5b2b635398a2350f3f1745`.
Implementation commit: `c0e76c9e02101bffb2c2656388e9c0322e64829c`.
This is an implemented source change, not a signed release or a completed security audit.

> 2026-10-06 follow-up: the current public bridge is 126/126 implemented with
> zero unsupported commands, and native CI passes on Windows x64, Linux x64,
> macOS x64, and macOS arm64. Historical validation counts below remain the
> 2026-09-28 snapshot.

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
| `npm run verify:tauri:contract` and parity harness | Passed. 129 invoke channels, 19 events, 123 implemented classifications and six explicit rejections. Static evidence only |
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
- Collection restore and installation shortcuts are implemented through bounded
  native flows. The obsolete UndertaleModTool installation-opening bridge and other
  legacy-only public callbacks were retired; current parity is 126/126 implemented
  with zero unsupported public commands.
- Full native CI now passes on Windows x64, Linux x64, macOS x64, and macOS arm64.
  Signed installed-app, updater, protocol, and real-client acceptance remain required;
  macOS arm64 CSX remains deliberately unavailable.
- Complete signed updater, platform publisher/notarization, data-preservation and
  rollback gates before stable release. Source retirement does not waive them.
- Expand coverage-guided native fuzzing and adversarial recovery testing. This audit
  does not claim that no worthwhile defects remain.

No itch.io integration was implemented. Previously reviewed upstream issue/commit
behavior was used only as problem context. No upstream implementation was copied,
cherry-picked or transplanted. New fixes stay within the Community Rust/Tauri model.


## macOS download and launch investigation

Investigated the report of mod downloads failing on macOS and one downloaded mod
not appearing in-game against `be6128dd2d5b3914ea5fe60d4537739c05d95943`.
The report does not establish the tester's installed app version, CPU architecture,
game edition or exact mod archive. These are independently reproduced code defects,
not a claim that that particular installation or mod was reproduced.

### Implemented corrections

- Mod/game transfers no longer inherit the short API request timeout or sleep after
  every network chunk. They have a 30-minute whole-transfer budget and 30-second
  idle/header deadlines, bounded full-transfer concurrency, immediate queued/idle
  cancellation, stream byte limits, strict size/encoding checks and complete-file
  flushing. Progress is throttled separately and does not expose signed query strings.
- Game-directory copies preserve Unix executable bits, discard privilege/write bits
  that should not be imported, detect permission changes during inventory/copy and
  sync copied files before publication. One buffer is reused across the file set.
  Two regression tests fail on the unchanged parent implementation and pass here.
- Finder sidecars no longer make a singly wrapped archive appear to lack its manifest.
  All entries still undergo the original traversal/link/expansion validation.
- Required-file hashes and patch publication use the same macOS/Linux data-path
  mapping. Existing spelling and already-qualified paths are preserved. A wrong
  game-version hash is still incompatible, not bypassed.
- An owned launch reservation spans staging, game launch and restoration. A concurrent
  launch cannot start during publication or restoration. Steam handoff is explicitly
  distinct from a reaped child, so opener exit no longer immediately restores original
  files. Failed handoff restores originals, and successful handoff retains durable
  recovery data. This does not add cross-restart Steam process detection: close the
  game before restarting the manager/recovering its files.
- macOS launches request a new instance of the exact bundle via `/usr/bin/open -n -W`.
  The flag contract is tested with injected adapters, not a real LaunchServices session.
- The importer requires an affirmative native acknowledgement before showing success.
  Cancellation/existing-copy decisions remain retryable, malformed acknowledgements
  show errors, and buttonless requests handle progress safely.
- Native patch errors reach the existing user-visible launch alert instead of returning
  a success acknowledgement after navigating away. The channel no longer attempts
  an unowned legacy rollback after the runtime has handled recovery.
- The updater cancellation fixture waits for observed request/header state before
  cancelling, rather than depending on a 50 ms cross-thread scheduling assumption.
  Production cancellation, signature and deadline checks are not relaxed.

### Local validation

Linux x64: 35 native-core tests plus copy-worker compilation, 22 network tests,
24 patch-runtime tests and 40 updater/launch-runtime tests passed. These include the
synthetic macOS-bundle Steam patch/retain/recover scenario. Fourteen focused renderer
tests passed, as did typecheck, affected-crate strict Clippy and static IPC checks.
The archive library compiles locally. Its GUI-coupled integration tests and full native
shell tests must run in CI with the real platform SDK/development libraries.

The original two new Unix copy tests reproduce lost executable bits and unobserved
permission mutation. Tests use synthetic game data only, with no commercial game files
or downloaded mod implementation committed.

Copy timing samples and the exact temporary reproducer are in
[`macos-import-copy-20260928.json`](../benchmarks/tooling/macos-import-copy-20260928.json).
The debug, warm-cache overlay-filesystem run improved the 200-small-file median from
10.204 to 9.223 ms, while four 8 MiB files increased from 4.644 to 5.878 ms with the
new per-file durability sync. These are not packaged app or macOS benchmarks, nor a
claim that all copying became faster.

### Still not established

External xdelta/G3M/CSX patch execution remains blocked in the Tauri staging pipeline
until a verified confinement implementation exists. Downloading a package does not
make its patch mechanism or Windows-specific game hashes compatible with macOS.
macOS ARM CSX remains unavailable. The error now explicitly says that the mod was not
applied and the game was not launched rather than silently returning to the menu.

Real Steam/LaunchServices lifetime behavior, installed app signing/quarantine and
specific mod compatibility still need exact-version native acceptance. A successful
unit test or CI build alone is not evidence that the screenshot's exact case is fixed.


## Selection and capability follow-up

Base: `e3380db83b39bba81d96855b27a3e2c294c4958b`.

- Resolve every selected packet identity exactly once, reject missing/ambiguous IDs,
  preserve caller load order and deduplicate repeated selection. An empty selection
  still permits vanilla launch. A selected empty manifest is now an error, not a
  successful no-op. Unrelated incomplete packets cannot derail a healthy selection
  or its post-patch acknowledgement.
- Share one bounded XML parser between staging and capability reporting. Identity,
  variant and metadata reads use the existing no-follow file boundary. XML bytes,
  node count, patch count and packet enumeration are bounded. Source/target paths
  are validated before hashing any patch body, and relative hashes use no-follow
  ancestors plus individual and aggregate byte budgets.
- Normalize archive path separators consistently so Windows-authored patch paths
  can reference real files on Linux/macOS. Game-version checks are unchanged.
- Report unsupported external patch mechanisms in the installed packet catalogue,
  independently of optional hash checks. A download or matching hash does not make
  xdelta/G3M/CSX execution available. This does not enable unconfined external tools.
- Bound the shared Node native-validator process with a deadline and cancellation,
  terminate it on invalid output, wait for pipe closure before settling, reject invalid
  UTF-8 and remove its dead ASAR resource lookup. Its Windows regression previously
  let a non-worker executable consume the entire test deadline.

Validation on Linux x64: six behavioral regressions fail against the unchanged parent
planner and pass with the fix. The complete affected test runs passed: 33 patch-runtime
unit tests, 13 tool-runtime unit tests, 12 process integration tests and 45 focused
JavaScript tests. Strict all-target Clippy for both changed Rust crates, typecheck,
static IPC validation and formatting passed. The new catalogue integration test is
included in full-shell CI, not claimed as a locally executed GUI test.

The parent's Linux, Intel macOS and Apple Silicon full-shell jobs passed. Its Windows CI failure
was the unbounded validator fixture above. Exact new-head cross-platform results
belong to the PR checks. These changes do not establish installed-game acceptance,
resolve the screenshot's unknown mod/version, or waive the external-tool confinement
and stable-release gates. No new desktop performance claim or dependency upgrade.
