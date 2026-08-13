# Project documentation

These documents describe release, packaging, and platform-boundary contracts:

- [Tauri release gate](./RELEASE-GATE.md)
- [Lockfile policy](./LOCKFILE-POLICY.md)
- [Protocol registration](./PROTOCOL-REGISTRATION.md)
- [Tauri migration boundary](./TAURI-MIGRATION-BOUNDARY.md)

User-facing contribution, support, security, licensing, and release-note files
remain at the repository root so GitHub and package tooling can discover them.

The startup banner lives with the other artwork at `art/ascii-banner.txt`.
Runtime feature flags are defined in `package.json`; there is no second feature
flag manifest to keep in sync.
