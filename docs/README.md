# Project documentation

These documents describe release, packaging, and platform-boundary contracts:

- [Tauri release gate](./RELEASE-GATE.md)
- [Lockfile policy](./LOCKFILE-POLICY.md)
- [Protocol registration](./PROTOCOL-REGISTRATION.md)
- [Provider capability evidence](./PROVIDER-CAPABILITY-EVIDENCE.md)
- [Tauri migration boundary](./TAURI-MIGRATION-BOUNDARY.md)
- [Bundled UNDERTALE theme sources](./UNDERTALE-THEME-RECIPES.md)
- [Desktop runtime benchmark](../benchmarks/desktop/README.md)
- [Desktop workflow reliability](./desktop-workflow-reliability.md)

GitHub community files live in `.github/`; licensing and provenance records live
under `docs/legal/` and `docs/provenance/` so the repository root stays focused.

The startup banner and README artwork live under `docs/assets/`.
Runtime feature flags are defined in `package.json`; there is no second feature
flag manifest to keep in sync.
