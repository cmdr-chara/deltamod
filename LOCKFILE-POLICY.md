# Lockfile Policy

Tauri release builds are reproducible only when both lockfiles are present and honored.

- `package-lock.json` is consumed by `npm ci`; no `npm install` is permitted in CI or release jobs.
- `native/Cargo.lock` locks the five worker crates and their shared `deltamod-native-core`; every native build uses `cargo ... --locked`.
- `src-tauri/Cargo.lock` locks the Tauri shell and domain crates; every shell build uses the lockfile generated and reviewed with the Tauri dependency change.
- A release job must fail if any lockfile is missing, dirty after dependency resolution, or changed without a dependency-review entry in the release commit.
- Do not vendor GPL tools into either Cargo workspace. They are checksum-verified resource trees and remain separate processes.
