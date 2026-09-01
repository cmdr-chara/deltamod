# Test Classification and Electron Retirement Policy

Deltamod retains the fast test suite during and after the Tauri migration. The
machine-readable inventory is
[`scripts/tauri-parity/fixtures/test-classification.json`](../scripts/tauri-parity/fixtures/test-classification.json).
It classifies all 54 product root Vitest files (the 52-test migration baseline,
Installed Mods v2, and the desktop benchmark gate) and all three end-to-end specs; the parity-classification
test is listed separately as a governance test so it can verify that inventory
without counting itself.

| Layer | Tests | Retirement rule |
| --- | ---: | --- |
| Domain | 4 | Permanent shared coverage |
| Lifecycle | 10 | Permanent shared coverage |
| Provider | 4 | Permanent shared coverage |
| Renderer | 13 | Permanent shared coverage |
| Security | 10 | Permanent shared coverage |
| Compatibility | 7 | Retain while the represented platform behavior exists |
| Electron shell | 4 | Remove only after equivalent Tauri capability evidence |
| Tauri shell | 5 | Permanent Tauri shell coverage |

The inventory contains 57 classified tests: 48 shared, four Electron-specific,
and five Tauri-specific. Every entry is explicitly marked `retained: true`.

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
