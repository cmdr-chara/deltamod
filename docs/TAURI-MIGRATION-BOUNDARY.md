# Migration Boundary

The Tauri shell links only the EUPL-1.2 Deltamod domain and runtime crates in `src-tauri`. Native file hashing, archive security, staged copy, patch-plan validation, and patch transactions are shipped as five target-specific sidecars and are invoked through the existing backend adapter. This preserves process isolation and prevents a native worker failure from becoming an unsafe in-process library call.

G3MTool and UndertaleModTool are never Cargo dependencies and never linked into the shell. Their complete, checksum-verified release trees are Tauri resources, including upstream license files and the corresponding source archives in the GitHub release. On Apple Silicon, UndertaleModTool is intentionally absent because the pinned upstream release has no arm64 CLI; the UI must report CSX unavailable for that target.

The current Electron package remains the reference implementation and rollback artifact until `docs/RELEASE-GATE.md` passes. Do not remove Electron scripts, assets, or release jobs as part of the first stable Tauri release.

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
