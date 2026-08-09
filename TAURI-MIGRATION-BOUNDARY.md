# Migration Boundary

The Tauri shell links only the EUPL-1.2 Deltamod domain and runtime crates in `src-tauri`. Native file hashing, archive security, staged copy, patch-plan validation, and patch transactions are shipped as five target-specific sidecars and are invoked through the existing backend adapter. This preserves process isolation and prevents a native worker failure from becoming an unsafe in-process library call.

G3MTool and UndertaleModTool are never Cargo dependencies and never linked into the shell. Their complete, checksum-verified release trees are Tauri resources, including upstream license files and the corresponding source archives in the GitHub release. On Apple Silicon, UndertaleModTool is intentionally absent because the pinned upstream release has no arm64 CLI; the UI must report CSX unavailable for that target.

The current Electron package remains the reference implementation until `RELEASE-GATE.md` passes. Do not remove Electron scripts, assets, or release jobs as part of the first Tauri beta.

## Configuration merge prerequisite

This packaging pipeline intentionally does not modify `src-tauri/**`. Before a
Tauri bundle can pass the gate, the shell configuration must opt into bundling
and declare the five target-matched sidecars/resources expected by the staging
and package verifiers. The current configuration has bundling disabled, so
`build:tauri:no-bundle` is the only safe local dry run until that shell change
is reviewed.

The target staging script writes `src-tauri/binaries/` because the pending
Tauri configuration is expected to use external binaries. If the final shell
invokes workers from the existing `native/*/bin/<platform>-<arch>/` trees
instead, remove the corresponding Tauri sidecar entries and do not copy those
files into `src-tauri/binaries/`; keep the native trees for the Electron
fallback. The resource script copies only verified tool trees plus the root
`NOTICE.md` and `THIRD_PARTY_NOTICES.md` files. It does not replace upstream
license files or stage source archives into the application bundle.
