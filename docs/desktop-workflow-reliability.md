# Desktop workflow reliability

This work builds on accepted `DeltaMaster` commit
`6a980ec831925d34ea75c8dc82b7297a698ad079` (PR #59, version 2.0.18).
The original navigation rail, pixel typography, theme backgrounds, glass panels,
soul/gear artwork and radial language picker remain in place.

## Behavior

- Home ignores results from a previous visit. Returning while a configuration
  save is pending waits for that save before reading the mod list.
- Launch waits for variant and enabled-state writes. A failed variant change
  restores the saved choice and shows the existing local save feedback. An
  enabled mod with no persisted variant must be saved before it can launch.
- Tauri accepts the renderer's existing `setModVariant([variant, folder])`
  contract. Imported TOML packages expose their declared variants and persist
  the selected XML in `__variant`, which the patch planner already consumes.
  Modern manifests retain their `id`/`label` fields and also expose the
  renderer's `filename`/`name` aliases and saved selection.
- A failed GameBanana catalogue page leaves previously loaded rows in place.
  Its inline Retry requests the same page; automatic pagination waits until
  recovery. Malformed pages cannot leave partially rendered rows behind.
- Failed file-list requests and mod imports release the Download button for
  another attempt. The existing multiple-file chooser also has a Cancel action.
- Local game-copy cancellation IDs reach the copy workflow, while registered
  game-download IDs continue to cancel their download tokens.
- If reimporting a game fails while saving metadata, the previous profile store
  is restored before removing the replacement copy. If restoration also fails,
  the replacement is retained so the saved path does not point at deleted data.
  Metadata is read after copying so settings changed during a long copy survive.

## Regression checks

The new browser suites run real accepted shell markup, styles and view scripts
with mocked native/provider boundaries. They exercise delayed saves, navigation,
rollback, catalogue retry and download recovery. They do not prove native IPC,
live provider availability or Windows desktop execution.

```sh
npm run typecheck
npm run build:boot
npx playwright test tests/e2e/frontend-refinement.spec.js tests/e2e/frontend-workflow-reliability.spec.js tests/e2e/shop-recovery.spec.js tests/e2e/tauri-themes.spec.js tests/e2e/tauri-localization.spec.js
cargo build --workspace --locked --manifest-path native/Cargo.toml
npm test
cargo test --workspace --all-targets --locked --manifest-path src-tauri/Cargo.toml
```

Rust regressions use real temporary files and imported ZIP packages. They cover
profile-store and registry write failures, variant persistence after reopening,
path rejection and use of the selected XML by the patch planner.

The original frontend workflow now runs these browser checks on both Ubuntu and
Windows, with separate evidence artifacts. The Community CI matrix retains its
native Windows/Linux build and test jobs. Editing these workflows does not mean
their remote jobs have run.

## Local platform evidence

On 2026-09-07, local verification on Linux included:

- Type checking, boot build and renderer/native contract parity: passed.
- JavaScript: 478 tests passed using bundled Node 24.19.0 and two Vitest workers.
- Chromium renderer suites: 34 tests passed, including narrow/wide layouts and
  the original theme and localization contracts. Native/provider IPC was mocked.
- Native Rust workspace: build, formatting and Clippy with warnings denied passed;
  all 37 tests passed.
- Tauri Rust workspace: all 511 tests passed. The final affected-package rerun
  passed all 144 tests after the last cancellation correction. Formatting and
  Clippy with warnings denied passed across the workspace and all targets.
- Native sidecars: real archive validation, atomic copy, hashing, patch-plan
  validation and rollback checks passed against disposable files.
- Native Linux Tauri 2.0.18 debug build: started with disposable application data
  and its original interface was inspected. A bounded liveness probe passed.
  The final development executable was subsequently rebuilt and opened
  successfully from this worktree.

The host's Node 26.7.0 test runs encountered subprocess-fixture timing failures;
the complete suite passed with bundled Node 24.19.0. One earlier Chromium run
crashed while loading an unchanged Credits fixture; the final complete browser
run passed. These are recorded as test-environment observations, not application
bug diagnoses.

The optimized Linux release build was interrupted during final optimization,
before producing an executable. Release capability checks remain unverified.

No Windows executable was built or run locally. Windows CI is configured but has
not run for this uncommitted change. The browser fixtures and portable Rust
filesystem/path tests do not substitute for native Windows desktop execution.
