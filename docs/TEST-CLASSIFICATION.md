# Test Classification and Electron Retirement Policy

Deltamod retains the fast test suite during and after the Tauri migration. The
machine-readable inventory is
[`scripts/tauri-parity/fixtures/test-classification.json`](../scripts/tauri-parity/fixtures/test-classification.json).
It classifies all 61 product root Vitest files and all four end-to-end specs. The
remaining root Vitest file, `tauri-parity-classification.test.js`, is listed
separately as a governance test so it can exhaustively compare that inventory
with the files on disk without counting itself.

| Layer | Tests | Retirement rule |
| --- | ---: | --- |
| Domain | 4 | Permanent shared coverage |
| Lifecycle | 10 | Permanent shared coverage |
| Provider | 5 | Permanent shared coverage |
| Renderer | 14 | Permanent shared coverage |
| Security | 10 | Permanent shared coverage |
| Compatibility | 11 | Retain while the represented platform behavior exists |
| Electron shell | 4 | Remove only after equivalent Tauri capability evidence |
| Tauri shell | 7 | Permanent Tauri shell coverage |

The inventory contains 65 classified tests: 54 shared, four Electron-specific,
and seven Tauri-specific. Every entry is explicitly marked `retained: true`.

## Rules

- Electron retirement never deletes domain, lifecycle, provider, renderer,
  security, or generally applicable compatibility tests.
- A shell-Electron test may be removed only after an equivalent Tauri capability
  test passes, the packaged smoke covers the behavior where appropriate, and the
  replacement is recorded in review evidence.
- Electron runtime and packaging removal happens in a separate cleanup release
  after one successful stable Tauri release.
- Capability coverage is migrated; filenames are not treated as the contract.
- New root or end-to-end test files must be classified in the JSON inventory.
- The governance test fails on omissions, duplicates, unknown layers/runtimes,
  or any entry whose retention flag is false.

## Current Tauri evidence boundary

The [IPC gap inventory](../scripts/tauri-parity/IPC-GAP-INVENTORY.md) and parity
report currently account for all renderer-visible commands and preload events;
unsupported capabilities remain explicit rather than silently succeeding. The
adapter rejects unknown commands through its exact positive allowlist. A validated
[smoke contract](../scripts/tauri-parity/SMOKE-TEST-PLAN.md) defines the required
dev and packaged runs. Windows has bounded release-binary capability evidence;
installed package, protocol, updater, and native Linux/macOS evidence remain
release gates until their platform jobs publish passing artifacts.
