# Test Classification and Tauri Coverage Policy

Deltamod keeps shared product coverage and Tauri-specific shell coverage as the
authoritative test surface. Electron-specific tests are no longer release gates.

The machine-readable inventory is
[`scripts/tauri-parity/fixtures/test-classification.json`](../scripts/tauri-parity/fixtures/test-classification.json).
It classifies 60 product root Vitest files and seven end-to-end specs. The
remaining root Vitest file, `tauri-parity-classification.test.js`, is listed
separately as a governance test so it can exhaustively compare that inventory
with the files on disk without counting itself.

| Layer | Tests | Policy |
| --- | ---: | --- |
| Domain | 4 | Permanent shared coverage |
| Lifecycle | 10 | Permanent shared coverage |
| Provider | 5 | Permanent shared coverage |
| Renderer | 20 | Permanent shared coverage |
| Security | 10 | Permanent shared coverage |
| Compatibility | 11 | Retain while the represented platform behavior exists |
| Tauri shell | 7 | Permanent Tauri shell coverage |

The inventory contains 67 classified tests: 60 shared and seven Tauri-specific.
Every entry is explicitly marked `retained: true`.

## Rules

- Domain, lifecycle, provider, renderer, security, and generally applicable
  compatibility tests remain independent of the desktop shell.
- Electron-only tests are not required for Tauri release acceptance.
- Packaged Tauri smoke, platform-native protocol/updater checks, and Rust workspace
  tests are the shell authority.
- Capability coverage is the contract; filenames are not.
- New root or end-to-end test files must be classified in the JSON inventory.
- The governance test fails on omissions, duplicates, unknown layers/runtimes,
  or any entry whose retention flag is false.

## Current Tauri evidence boundary

The [IPC gap inventory](../scripts/tauri-parity/IPC-GAP-INVENTORY.md) and parity
report account for renderer-visible commands and events; unsupported capabilities
remain explicit rather than silently succeeding. The adapter rejects unknown
commands through its exact positive allowlist. A validated
[smoke contract](../scripts/tauri-parity/SMOKE-TEST-PLAN.md) defines the required
dev and packaged Tauri runs. Windows has bounded release-binary capability evidence;
installed package, protocol, updater, and native Linux/macOS evidence remain release
gates until their platform jobs publish passing artifacts.
