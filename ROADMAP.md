# Deltamod Community roadmap

> **Product thesis:** Deltamod Community is a transactional, recoverable, and
> explainable mod manager. Every modification is planned before execution,
> attributable afterward, verifiable at any time, and recoverable when it fails.

The intended user journey is:

**Discover → Understand → Review → Install safely → Verify → Update → Recover → Reproduce/share**

This roadmap uses release-sized outcomes. Shared contracts are serialized through
one integration owner; bounded implementation work may proceed in parallel after
those contracts pass their compatibility and safety tests.

## Non-negotiable invariants

- At most one filesystem-mutating lifecycle operation may run per game installation.
- Every mutation has a stable operation ID and idempotency key; duplicate delivery
  returns the existing operation rather than executing twice.
- No planned output may escape its declared transaction root. Paths are decoded,
  normalized, checked for links/reparse points, and revalidated before publication.
- Identical file content may be co-owned. Differing content is a blocking conflict.
- External modifications are never overwritten silently.
- Active, unfinished, pinned, and sole recovery copies are protected from cleanup.
- Electron-specific code may be retired only after equivalent Tauri capability
  evidence exists. Fast shared domain, renderer, security, and compatibility tests remain.

## Contract freeze

The shell-independent `deltamod-product-contracts` crate owns versioned wire and
persistence contracts for installed mods, file claims, lifecycle journals,
conflicts, verification, game health, operation progress, provider capabilities,
provider references, retention defaults, and structured errors.

Persistent lifecycle records must pass schema-version inspection and migration
before deserialization. Unknown newer versions fail closed before any mutation.

## Release A — Safe lifecycle

- Mandatory preflight showing affected files, owners, conflicts, external changes,
  backup requirements, provider/version identity, hashes, and patch tools.
- Transactional install and uninstall with staging, backups, verification, atomic
  manifest commit, deterministic recovery, and **Restore Last Working State**.
- Installed Mods v2 and persistent Operations UI, initially fixture-driven and then
  activated as backend capabilities land.
- Bundled DELTARUNE and UNDERTALE themes may ship once their fixed manifests,
  media, provenance, and visual/audio checks pass.
- Polish the hidden Chara encounter with stronger keyboard/focus behavior,
  reduced-motion safety, and renderer tests retained across the Tauri migration.

## Release B — Maintainability

- Update while retaining the previous usable version until verification succeeds.
- Verify and repair using the exact cached archive or exact provider version.
- Game health, recovery UI, structured diagnostics, patch timeouts, cancellation,
  process-tree termination, output validation, and deterministic cleanup.

## Release C — Discovery

- Normalize GameBanana, ModDB, Nexus Mods, and local archives behind capability-based
  provider contracts.
- Unified Mod Shop search, filtering, sorting, canonical identities, alternate
  sources, bounded caching, offline states, retry, cancellation, and normalized errors.
- Game Jolt and itch.io remain outside the Mod Shop. They are used only by the
  original configured game-download flows with known build/file identifiers.

## Release D — Reproducibility

- A profile is a desired set and order of lifecycle manifests, not a second installer.
- Profile activation is one preflighted transaction with verification and rollback.
- Versioned profile lockfiles record exact provider versions, archive hashes,
  file-plan fingerprints, game identity, configuration, and load order.
- Patch output is published exclusively through lifecycle transactions.

## Release E — Tauri-only

- Required IPC parity, packaged Windows/macOS/Linux smoke, protocol, updater,
  sidecar, lifecycle, patching, persistence, and WebKitGTK evidence all pass.
- One stable Tauri release succeeds before a separate cleanup release removes
  Electron runtime and packaging.
- Tests are retained or removed by behavioral capability, never by filename alone.

“Tauri/Rust rewrite” means the native shell, IPC, filesystem, network, updater,
lifecycle, and patch execution paths move to Rust with no Electron/Node runtime.
The existing HTML/CSS/JavaScript renderer remains the product UI and keeps its fast
shared tests; replacing that renderer with a Rust UI framework is not part of Release E.

## Storage and retention

- Keep the latest three completed recovery generations per installation.
- Default re-downloadable cache limit: 5 GiB, cleaned least-recently-used first.
- Default completed-recovery limit: 10 GiB, excluding protected generations.
- Preflight requires staging + backup estimates + 512 MiB free-space reserve.
- Settings reports cache, backup, journal, and total usage.
- **Clear cache** and **Delete recovery data** are separate actions; the latter is
  danger-confirmed and cannot delete active journal dependencies.
- Keep the newest 100 operations or 30 days; unresolved/recoverable operations stay pinned.

## Bundled game themes

The theme selector ships twelve additional built-in themes. Users do not run an
extractor or import them: the packaged manifests, backgrounds, and Ogg tracks are
loaded through the same built-in theme runtime as the original themes.

The bundled set contains four DELTARUNE themes and eight UNDERTALE locations:

- Ruins — `mus_ruins.ogg` with purple tiles, Ruins door, pillar, and vines.
- Snowdin — `mus_snowy.ogg` with Snowdin sign and winter-tree sprites.
- Waterfall — `mus_waterfall.ogg` with waterfall tiles, water pillar, echo flower,
  and glowshroom sprites.
- Barrier — `mus_barrier.ogg` with the canonical Barrier location render. The old
  `undertale-void` internal ID remains stable so existing preferences keep working.
- Hotland — `mus_anothermedium.ogg` with lava, edge, and sign sprites.
- CORE — `mus_core.ogg` with door, glow-wall, light-strip, and wall-strip sprites.
- True Lab — `mus_hereweare.ogg` with fog, determination machine, camera, and door sprites.
- New Home — `mus_endarea_parta.ogg` with the verified Asgore parlor background,
  parlor stairs, and key/note placement; Part B is basement-only and is not merged.

Last Corridor remains deferred until its room/resource pairing has equally specific
evidence. The reproducible maintainer generator accepts explicit installation and
cache roots, records source/output SHA-256 hashes in
`docs/BUNDLED-THEME-PROVENANCE.json`, and never stores machine-specific source paths.
Only the twelve reviewed IDs are permitted in the bundled-theme allowlist; local
custom themes remain separate.

## Rewrite performance gate

The immutable Electron baseline and comparison protocol live under
`benchmarks/desktop`. On the clean `9e6f8af` baseline (which includes `a882423`), seven
post-warm-up launches measured a 1,513.96 ms median ready time and 728.60 MiB median
peak working set. The current unsigned packaged Windows Tauri candidate completed the same
protocol at 1,664.31 ms median ready time and 422.67 MiB median peak working set.
These numbers satisfy the performance comparator but do not replace signing,
updater, or non-Windows release evidence. The freshly rebuilt Windows NSIS package
now also passes installed capability, protocol-handler, and five-worker
import/hash/patch-rollback smoke.

The baseline also records 408 `native/target` entries in the current Electron ASAR.
Any packaging exclusion is a separate measured change, not silently folded into the
runtime comparison.

## Continuous delivery policy

- **P0:** corruption, security, and irreversible failures interrupt other work.
- **P1:** crashes and broken install/download behavior block adjacent expansion.
- **P2:** UX and performance regressions are batched aggressively.
- **P3:** cosmetic/developer cleanup stays inside touched boundaries.
- Feature work receives an adjacent cleanup budget, never an unrelated repo-wide rewrite.

## Parallel lanes and integration cadence

- Reliability exclusively owns lifecycle persistence and filesystem semantics.
- Product UI may render frozen contract fixtures early, but live actions stay
  disabled until their backend operation is accepted and wired.
- Provider work consumes the frozen capability/error/progress contracts; it does
  not create provider-specific install paths.
- Tauri work migrates capability evidence while permanently retaining shared
  domain, renderer, security, and compatibility tests.
- Shared contracts, shell routing, workspace manifests, lockfiles, generated
  outputs, localization catalogues, and cross-lane fixtures have one integration owner.
- Every accepted coherent slice is integrated promptly; incomplete pairings use
  explicit flags/routes, and the integrated tree reruns cross-lane gates.

## Continuous product and UI lane

The implementation-ready hierarchy, state, responsive, and accessibility rules
for this lane are frozen in `.codex-run/UI-DIRECTION.md`. The selected direction
keeps Deltamod's pixel identity while making health, operations, conflicts, and
recovery easier to scan than decorative surfaces.

1. Standardize loading/skeleton, empty, offline, error, notification, progress,
   confirmation, focus, contrast, keyboard, form, and reduced-motion behavior.
2. Make Installed Mods the control center and normalize Mod Shop/provider cards,
   account state, download/install progress, retry, and cancellation.
3. Add persistent Operations and Recovery screens, conflict resolution, game
   health, repair/update UX, diagnostics, profiles, and a task-focused first run.
4. Finish responsive resizing, interaction polish, microcopy, localization, and
   an accessibility sweep without delaying correctness fixes.
5. Keep optional characterful details, including the Chara encounter, polished,
   accessible, and isolated from lifecycle or release-critical behavior.

The first-run success condition is: **Game ready — you can safely install mods.**
Diagnostics must be sanitizable and copyable without opening DevTools.

## Product gates

| Outcome | Required evidence |
| --- | --- |
| Failed update destroys the previous install | Zero cases in fault-injection coverage |
| Journal recovery is deterministic | Every modeled phase covered |
| Unknown overwrite without confirmation | Zero mutation paths |
| Duplicate UI delivery repeats a mutation | Zero; same operation is returned |
| Raw provider HTTP failures reach users | Zero |
| Installed mods have source/version/hash | All records where a provider supplies them |
| Long operations expose structured state | Every long-running operation |
| Diagnostics require DevTools | No |
| Shared tests survive Electron retirement | Yes |
| Unreviewed game-theme assets enter Git/releases | Zero; only the fixed bundled allowlist is accepted |

## Current execution status

| Slice | Status |
| --- | --- |
| Latest baseline and accessibility integration | Functional baseline complete at `9e6f8af` (contains local `a882423`); the sole newer `DeltaMaster` change from `7bb591c` is integrated in README. The next non-downgrade release identity is `2.0.18`, following published `2.0.17`. |
| Contract freeze and adversarial fixtures | Accepted: 52 tests green; two Sol audits passed |
| Release A lifecycle/UI foundations | A1/A2 are accepted and integrated. On Windows, Installed Mods v2 adopts legacy package libraries into authoritative lifecycle manifests without rewriting package bytes, retains a content-addressed exact snapshot, verifies exact owned hashes, repairs missing files from that snapshot, updates imported packet mods from a user-selected validated archive, and performs transactional uninstall only when no external change is present. Chara focus, delayed-start cancellation, native-position restoration, completed-line announcements, visible continuation, and reduced-motion safeguards remain integrated. |
| Provider reconnaissance/foundation | R2 evidence, C1 normalization, and the C4 catalogue transport slice are integrated. The Mod Shop exposes GameBanana, Nexus Mods, and ModDB through validated provider-specific routes with structured safe failures, canonical item identities, a bounded content-addressed Rust metadata cache, fresh-cache reuse, and stale/offline fallback. Game Jolt and itch.io are retained only for configured automatic game downloads, matching the original Deltamod behavior. |
| Bundled game themes | Four DELTARUNE and eight namespaced UNDERTALE themes, including New Home, are packaged as built-ins. The selector exposes 25 themes total while Chara remains hidden by default. Generation provenance is recorded without developer-specific installation paths. |
| Tauri/test foundation | D1 through D2g are integrated. The bridge has 129 public commands (123 implemented, 6 explicitly unsupported) and 18 renderer events with zero producer gaps. Native separate-window alerts now use the bounded Tauri dialog adapter; the Chara encounter has native Rust window motion with exact restoration, and two dead legacy IPC channels were retired. The packaged shell registers the Community deep-link scheme and single-instance forwarding before the strict Rust handoff parser. Its explicit renderer handshake waits for all required event listeners instead of intercepting a captured Tauri internal. The freshly rebuilt unsigned Windows NSIS package passes exact-version in-app capability smoke and now proves that its bounded UNDERTALE fixture is actually resolvable by the game runtime, rather than merely listed. The previous installed candidate also passes install, protocol registration, second-instance forwarding into the first process after renderer readiness, all five packaged Rust worker protocols, atomic fixture import, exact hashing, patch backup/restore, process-tree cleanup, uninstall, and user-data preservation. Evidence is retained in `benchmarks/packaged-smoke/tauri-windows-installed-nsis-protocol.json` and `benchmarks/packaged-smoke/tauri-windows-installed-nsis-sidecars.json`. The release matrix now runs both Rust workspaces' all-target tests natively on Windows x64, Linux x64, and macOS x64/arm64 before packaging, then exercises the same installed worker smoke on every platform. Settings reports both patch and library recovery usage, keeps provider-cache cleanup separate, and danger-confirms identity-bound deletion of removable recovery generations. Startup reconciles interrupted deletion tombstones and enforces the 10 GiB recovery plus 100-item/30-day operation-history policies without deleting protected state. Signed updater/platform-signing evidence and actual non-Windows CI execution remain outstanding. |
| Pre/post rewrite benchmark | Clean Electron baseline captured and recorded (7 measured launches; 1,513.96 ms median ready; 728.60 MiB median peak working set). The current unsigned Windows NSIS candidate completed the identical one-warm-up plus seven-launch protocol with fresh Deltamod/WebView2 profiles, the same bounded fixture, and renderer-authenticated main-route readiness: 1,664.31 ms median, 422.67 MiB median peak working set, and a 294.67 MiB installer. The comparator accepts the pair: +9.93% readiness, -41.99% memory, and -66.57% packaged size versus Electron. This is performance evidence only; signing and the remaining Release E gates still apply. |
| Releases B–E | A2 update/verify/repair/game-health/restore, executable A3 transactional profile switching with exact lockfile source resolution, and A4 internal patch staging are accepted and integrated. A3 uses one outer journal/lease and atomically couples the active-profile pointer to the committed manifest. Tauri `patchAndRun` adopts the exact verified baseline, publishes staged patch output through the journaled Rust lifecycle filesystem boundary, and restores the previous generation after game exit or before the next patch session. Windows uses `fence-windows`; Linux and macOS use pinned device/inode identities plus rustix `openat`/`renameat`/`unlinkat` no-follow operations. The boundary rejects hardlinks/link escapes and rediscovers interrupted workspaces after restart; the legacy compatibility publisher is no longer reachable from `patchAndRun`. Recovery retention now measures and identity-binds exact workspaces, records durable deletion tombstones, quarantines before purge, reconciles interrupted deletion at startup, and compacts the append log without losing sequence authority. The complete locked Tauri workspace and strict all-target Clippy pass on Windows; native Linux/macOS adversarial execution remains a Release E package gate. The tools runtime bounds aggregate output, propagates live cancellation into external G3M/CSX execution, and terminates/reaps full process trees on cancellation, timeout, overflow, and completion. The release workflow now fails closed on missing updater/publisher credentials, verifies Authenticode on every distributed Windows executable, and requires Developer ID, Gatekeeper, and notarization evidence on both macOS architectures. Native signed execution and one stable Tauri publication remain external Release E gates. |
