# Deltamod agent instructions

## Product contracts

- Protect users' original game files, installed profiles, recovery state, and saved data. Import, patch, cancellation, repair, and uninstall changes must preserve transactional recovery rather than report partial writes as success.
- Treat downloaded mods, archives, provider responses, paths, and deep links as untrusted. Preserve containment, validation, process ownership, and explicit failure handling across renderer/native boundaries.
- Keep Electron/Tauri IPC and sidecar contracts compatible where both runtimes remain supported. A renderer fixture does not prove a packaged native application works.
- Preserve updater trust, target matching, signatures, provenance, and third-party license boundaries. External modding tools remain separate processes, not code silently linked into Rust crates.
- Localization changes must preserve placeholders, keys, provider identifiers, and fallback behavior. Respect reduced motion and existing desktop interaction conventions.

## Task-specific guidance

Use [CONTRIBUTING.md](CONTRIBUTING.md) for setup and the affected JavaScript, renderer, native-worker, or Tauri check lane. Use the committed toolchain and dependency locks. Consult [SECURITY.md](SECURITY.md) for security work and [docs/RELEASE-GATE.md](docs/RELEASE-GATE.md) only for packaging, updater, or release changes.

Keep generated `web/`, downloaded tools, native build output, personal game files, and transient test artifacts out of ordinary source edits. Change generators or acquisition manifests rather than patching staged output.

## Completion

Complete the requested change with affected runtime/IPC/recovery contracts checked, relevant regression coverage, and synchronized documentation. State which operating systems and packaged flows actually ran. Never weaken signing, provenance, or release gates to turn a build into a stable-release claim. Game-data erasure and release publication are not routine validation steps.
