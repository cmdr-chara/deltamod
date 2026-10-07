# Migration Boundary

The Tauri shell links only EUPL-1.2 Deltamod domain/runtime crates and the EUPL
native core. The authoritative Tauri lifecycle, profile-install, and patch
publication paths use that Rust code in-process so no Electron/Node publisher sits
inside the filesystem trust boundary. File hashing, archive security, staged copy,
patch-plan validation, and patch transactions are also shipped as five
target-specific compatibility workers and must pass their installed bounded-protocol
smoke on every release target.

G3MTool and UndertaleModTool are never Cargo dependencies and never linked into the shell. Their complete, checksum-verified release trees are Tauri resources, including upstream license files and the corresponding source archives in the GitHub release. On Apple Silicon, UndertaleModTool is intentionally absent because the pinned upstream release has no arm64 CLI; the UI must report CSX unavailable for that target. Patch staging invokes G3MTool through the tools runtime (`run_bounded_with_cancel_probe`) with inputs copied into a staging temp directory; the tool never writes to game files directly. On macOS, the managed game copy is re-signed ad hoc with `/usr/bin/codesign` after each publish and restore.

The current Electron package remains the reference implementation and rollback artifact until `docs/RELEASE-GATE.md` passes. Do not remove Electron scripts, assets, or release jobs as part of the first stable Tauri release.

## Patch and recovery compatibility

macOS patch startup, restoration, and publication resolve the existing lifecycle
installation key before adopting a baseline. Legacy keys are matched using the
recorded game-directory path and inode, including the legacy device number in
journals from an earlier boot. Known manifest-only baselines and interrupted
leases also participate in this lookup. Existing keys, manifests, leases, and
recovery generations are retained; the transition does not rewrite durable
contracts. Conflicting keys for the same game block patching and preserve both
sets of recovery state.

Legacy macOS root identities are bound to the currently opened directory pins
using the canonical path and inode. Current stable volume identities still require
an exact match, and directory replacement remains an error. Newly recorded games
use the stable macOS installation key. Windows and Linux identity policies remain
unchanged.

Windows xdelta/G3M merge grouping uses the same case-insensitive target identity
as plan validation and output verification, retaining the first target spelling
and selected patch order. Case-sensitive platforms retain separate targets.

On macOS, compatibility checks consult the packet's bounded, metadata-only patch
plan and use the same base-file resolver as G3M staging for reference-backed
targets. Other required files continue to use installed game bytes. Reference
hash-cache entries are separate from native entries and include the reference
root path; missing, linked, and changed files are checked before cache reuse.

Tauri-only means the native shell and privileged backend capabilities are Rust and
the Electron/Node runtime is absent. The existing web renderer remains shared
HTML/CSS/JavaScript so its fast renderer tests survive the migration.

## Stable packaging boundary

The shell configuration opts into bundling and declares the five target-matched
sidecars and verified resources expected by the staging and package verifiers.
Each target bundle must pass those verifiers before publication. Stable Windows
and macOS artifacts additionally require a valid platform signature from the
expected publisher; users must never be directed to bypass operating-system
security checks.

Signed updater artifacts are enabled only for Windows NSIS and macOS app
bundles. Their release metadata is generated from the matching `.sig` files and
published with the existing checksums and GitHub attestations. Linux `.deb`
remains manual-only and must report an unsupported updater gate until an
AppImage distribution and recovery path are separately verified.

The target staging script writes `src-tauri/binaries/` for the configured
external binaries. Keep the native trees for the Electron fallback. The
resource script copies only verified tool trees plus the root
`NOTICE.md` and `THIRD_PARTY_NOTICES.md` files. It does not replace upstream
license files or stage source archives into the application bundle.
