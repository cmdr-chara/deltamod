# Tauri runtime contracts

- Keep native filesystem publication, patch transactions, and recovery authoritative in the runtime. Do not move trust decisions into renderer code or return success before persistence completes.
- Own child processes and asynchronous work through cancellation and shutdown. Preserve bounded output, containment failures, and target-specific executable resolution.
- Maintain the renderer/IPC channel contract and native sidecar JSON protocols together with their consumers. An unsupported platform/tool path must fail explicitly, not fall back silently.
- Do not link separately licensed external modding tools into runtime crates or change updater trust material as an incidental repair.

For this workspace, use the locked formatting, strict Clippy, and test commands in [CONTRIBUTING.md](../CONTRIBUTING.md). IPC changes also need `npm run verify:tauri:contract` from the repository root and affected renderer tests. Cross-workspace changes need the `native/Cargo.toml` lane too. Packaging and release evidence is governed by [RELEASE-GATE.md](../docs/RELEASE-GATE.md); Linux unit tests do not establish Windows/macOS process behavior.
