from pathlib import Path
import re

def write(p,s):
    p=Path(p); p.parent.mkdir(parents=True,exist_ok=True); p.write_text(s)
def edit(p,fn):
    p=Path(p); old=p.read_text(); new=fn(old)
    if old!=new: p.write_text(new)

edit('AGENTS.md',lambda s:s.replace('- Keep Electron/Tauri IPC and sidecar contracts compatible where both runtimes remain supported. A renderer fixture does not prove a packaged native application works.', '- Tauri is the only desktop runtime. Keep the renderer/native IPC and sidecar contracts compatible. A renderer fixture does not prove a packaged native application works. Do not reintroduce Electron runtime, packaging, or preload paths.'))
edit('CONTRIBUTING.md',lambda s:s.replace('You need Node.js 22. Rust and the platform prerequisites for [Tauri](https://v2.tauri.app/start/prerequisites/) are required for native and Tauri work.', 'You need Node.js 22, Rust, and the platform prerequisites for [Tauri](https://v2.tauri.app/start/prerequisites/). `npm run dev` starts the native Tauri application. Node.js is build/test tooling, not a shipped desktop runtime.')
    .replace('Release changes must also satisfy', 'Electron has been retired. Shared JavaScript domain/reference tests remain valuable and must not be removed merely because they run in Node.js. `npm run verify:tauri-only` checks the active contract, packaging targets, retired runtime paths, and sidecar staging. Historical desktop benchmark evidence is immutable.\n\nRelease changes must also satisfy'))
edit('README.md',lambda s:s.replace('Development requires Node.js 22. Rust and the platform prerequisites for Tauri are required for native builds.', 'Development requires Node.js 22, Rust, and the platform prerequisites for Tauri. `npm run dev` starts Tauri. Electron is no longer a runtime or packaging option. The retained Node.js modules serve shared tests and build tools only.')
    .replace('## Mod sources', '### Native launch limitations\n\nSteam installation discovery includes alternative Linux roots and Flatpak layouts. Ordinary Steam launches use the native platform opener. Transactional patch-and-run is deliberately refused for a Steam handoff because Deltamod cannot observe the external game process lifetime safely. No files are patched in that case. Use a supported managed installation with an owned game process for transactional patch-and-run.\n\nSource retirement is not signed-release evidence. See [the platform audit](./docs/TAURI-ONLY-AUDIT.md) for the changes and remaining release checks.\n\n## Mod sources'))

p=Path('ROADMAP.md'); s=p.read_text()
marker='## Release F — Electron retirement'
start=s.index(marker); end=s.index('## Storage and retention',start)
s=s[:start]+'''## Release F — Electron retirement

The source cleanup is implemented at the maintainer's explicit request. Tauri is
now the sole desktop runtime. Electron entry points, preload/tracer/download-modal
code, dependencies, packaging, release/bootstrap workflows, and runtime-specific
validation scripts are removed. Shared renderer/domain/reference tests and all
historical desktop benchmark evidence remain.

The six previously unsupported commands were deliberately retired from the public
bridge and their already-disabled UI actions: `rebootDev`, `createInstallLink`,
`undertaleModTool:openInstallation`, `gamebanana_downloadAllInCollection`,
`npsCallback`, and the destructive `initialize` reset. This is not a claim that
shortcut creation, collection restore, or UMT workspace opening was implemented.

Release E's signed installed-package, upgrade, rollback, updater, and platform
smoke evidence still applies. Removing Electron does not waive those gates.
Re-measure desktop startup, memory, and package size after packaging this tree.
Do not rewrite or combine the historical Electron baseline with cleanup results.

'''+s[end:]
s=s.replace('- One stable Tauri release succeeds before a separate cleanup release removes\n  Electron runtime and packaging.', '- Electron source retirement was explicitly requested before the remaining signed\n  release evidence was complete. Keep those release gates open until actually verified.')
s=s.replace('## Codebase audit — next work', '## Codebase audit — 2026-09-27 historical intake')
s=s.replace('## Upstream convergence —', '''## Maintenance update — 2026-09-28

Implemented source changes include the Tauri-only public command contract and
current parity fixtures, bounded updater-manifest inputs, bounded tool downloads,
target-scoped sidecar staging with rollback, validated recovery-journal ownership,
serialized installation/credential updates, strict Steam URIs, bounded VDF parsing,
alternate Linux Steam roots, cross-platform Steam handoff, and direct Linux binary
launch instead of passing ELF files to a shell.

Remaining work is tracked in `docs/TAURI-ONLY-AUDIT.md`. In particular, externally
launched Steam games do not have an owned process lifetime, macOS arm64 `.csx`
support remains unavailable, signed/installed-package evidence is not implied by
unit tests, and adversarial filesystem swaps need stronger OS-relative handles.

itch.io accounts, collections, and Mod Shop integration are explicitly out of
scope. The pre-existing native game acquisition tool is not a new itch.io feature.
Upstream behavior may inform regression cases, but no upstream code is copied,
cherry-picked, transplanted, or closely reproduced.

## Upstream convergence —''')
s=s.replace('- Decide whether to adopt upstream\'s itch.io account/collection expansion. If adopted,\n  build it on the Community credentials/provider boundaries rather than importing\n  plaintext account JSON or browser/local-callback token handling.', '- itch.io account/collection expansion is intentionally excluded by maintainer decision.')
s=s.replace('- **Requires product/provenance decision:** itch.io authenticated accounts and\n  collections, multi-provider collection UX, and the Chapter 3 theme.', '- **Intentionally excluded:** itch.io authenticated accounts and collections.\n- **Requires product/provenance decision:** multi-provider collection UX and the Chapter 3 theme.')
p.write_text(s)
write('docs/TAURI-ONLY-AUDIT.md','''# Tauri-only maintenance audit

## Scope and provenance

This change removes the Electron desktop application, not the JavaScript renderer
or shared Node.js reference/build/test modules. No upstream implementation was
copied, cherry-picked, transplanted, or closely reproduced. itch.io account,
collection, and Mod Shop functionality is not implemented.

Historical records under `benchmarks/desktop` and `scripts/desktop-benchmark` are
retained unchanged. This audit does not relabel historical performance data or
claim a new packaged desktop benchmark.

## Implemented behavior

- The live Tauri adapter is the public contract. All exposed commands have a native
  implementation. Six previously unsupported actions are removed, not mocked as
  successful. Cache cleanup and danger-confirmed recovery cleanup remain separate.
- Updater manifest generation validates exact version/architecture identity,
  bounds traversal and signature reads, and rejects links, special files, invalid
  UTF-8, duplicate targets, and missing signed platform artifacts.
- Tool acquisition bounds the actual stream before buffering unbounded content.
  Unix butler binaries receive executable permissions after verified extraction.
- Sidecar staging respects `CARGO_TARGET_DIR`, preserves other target triples,
  checks all inputs before publication, and rolls back failed publication. Failed
  rollback preserves recovery files and refuses another writer rather than erasing
  the only backup. It is not an atomic multi-file filesystem transaction.
- Profile recovery validates operation-owned paths and every existing ancestor,
  bounds actual journal reads, and preserves ambiguous copies or cleanup failures.
  Filesystem-mutating installation operations are serialized within the shared
  runtime, separately from metadata updates and cancellation/progress handling.
- Installation IDs are validated during deserialization, not only construction.
- Credential updates are serialized. Metadata is schema-checked before mutation.
  Failed metadata writes restore an existing credential instead of deleting it.
  An explicit logout never restores its deleted token after metadata failure.
- Steam discovery uses bounded VDF parsing, actual read limits, existing absolute
  libraries, and canonical root deduplication. Linux includes XDG, alternate home
  roots, and Flatpak. macOS uses its Application Support Steam directory.
- Steam URIs reject path/query suffixes, zero, overflow, and nonnumeric IDs. Unix
  opener failures are errors. Windows Explorer's handoff status is treated
  separately. Steam handoff does not terminate the manager.
- Linux native binaries launch directly. Explicit `.sh` launchers use `sh`.
  Launch exclusion remains active until exit finalization completes. Invalid
  Unicode environment entries cannot panic environment collection.
- Linux and macOS bundle targets are explicit. Windows controller binaries are
  scoped to Windows resources rather than inherited by Unix packages.

## Platform status and limitations

| Area | Current behavior | Evidence still required |
| --- | --- | --- |
| Windows x64 | Native executable and Steam opener paths retained | Installed-package, signature/updater, controller and real-game smoke |
| Linux x64 | Native binary/script distinction, alternate Steam roots, `.deb` target and Unix permissions corrected | WebKitGTK UI, package install/uninstall, desktop protocol and real-game smoke |
| macOS x64 | Native app/DMG targets, Steam opener status and sidecar handling corrected | Installed DMG, Gatekeeper/notarization, protocol, updater and real-game smoke |
| macOS arm64 | Separate target staging and native app/DMG configuration retained | Same platform smoke plus tool availability; `.csx` remains unavailable |
| Steam patch-and-run | Refused before mutation when the game lifetime is externally owned | A reliable owned Steam process lifecycle before transactional support can be enabled |

Unit tests and mocked process adapters are not installed-package evidence. The
pull request and CI runs report the exact hosts and checks that actually ran.
No stable release, signature, notarization, or interactive game smoke is claimed
by this source change.

## Remaining security and reliability boundaries

Path validation is not a claim of immunity to hostile local filesystem swaps
between validation and use. Descriptor-relative operations and cross-process
coordination remain a separate hardening lane. Runtime locks serialize clones of
one application runtime, not arbitrary external writers.

OS keyring and metadata writes are not one OS-level transaction. A process crash
between them still needs reconciliation. Recovery ambiguity intentionally fails
closed and retains evidence rather than guessing which copy to delete.

The Tauri updater download wrapper still needs enforceable cancellation/size
limits during the plugin's download future rather than only checking its final
buffer. This remains open and must not be represented as fixed by the manifest
or build-tool download hardening.

Collection restore, desktop shortcuts, UMT workspace opening, and a destructive
full reset are not public commands. Reintroducing any of them requires a native,
validated implementation and product-level regression evidence.
''')
# New independent modules carry notices without rewriting existing provenance history.
for file in ['src-tauri/crates/installations/src/steam.rs','src-tauri/crates/credentials/src/hardening_tests.rs','src-tauri/crates/updater-launch-runtime/src/hardening_tests.rs','src-tauri/crates/profile-install/src/recovery_guard.rs','src-tauri/crates/profile-install/src/recovery_tests.rs']:
    p=Path(file); s=p.read_text()
    if 'SPDX-License-Identifier:' not in s:
        p.write_text('// SPDX-FileCopyrightText: 2026 cmdr-chara\n// SPDX-License-Identifier: EUPL-1.2\n'+s)
print('Current documentation distinguishes source retirement, platform fixes, and still-open release evidence.')
