# Shared desktop runtime

The first extracted service is the kernel-held `RuntimeLease`. It uses only the
Rust standard library (Rust 1.89 or later), with no Tauri, WebView or GPU dependency.
`src-tauri/src/state.rs` source-includes the exact same module for both the Tauri
binary and the GPUIX headless backend. This avoids changing the stable shell's
dependency graph merely to introduce a shared service.

Run `cargo test --manifest-path native/desktop-runtime/Cargo.toml` to exercise
release/reacquisition, file/link rejection and independent-process exclusion.
The lease file is persistent. Never delete it to recover a supposed stale process.
On Unix this remains a cooperative advisory lock. On Windows deletion sharing is
also denied while the handle is open. A malicious same-user process or an older
Tauri release that ignores the lease is not made safe by this change.

This is **not yet the complete HeadlessBackend extraction**. The GPUIX host still
links `deltamod-tauri-shell`. Its `AppState` owns `ShellUpdater`, `TauriGameLifecycle`
and application handles. `channels/import_download.rs` mixes pure import functions
with Tauri event emitters, and `channels/nexus_oauth.rs` mixes PKCE logic with the
Tauri opener. Those adapters must be separated before the GPUIX Cargo dependency
graph can truthfully be called Tauri-free. Do not replace that work with duplicated
business logic, fake Tauri shims or a success-only feature flag.
