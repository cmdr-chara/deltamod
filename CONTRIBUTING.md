# Contributing to Deltamod Community

Thank you for helping improve Deltamod Community. Contributions should be focused, reproducible, and safe for users' game installations.

By participating, you agree to follow the [Code of Conduct](./CODE_OF_CONDUCT.md).

## Before you start

- Search [existing issues](https://github.com/cmdr-chara/deltamod/issues) before opening a new report.
- Use the bug-report template and include your operating system, game, mod, and exact reproduction steps.
- Report vulnerabilities privately as described in [SECURITY.md](./SECURITY.md). Never publish credentials, private game files, authentication data, or a working exploit in an issue.
- Keep changes scoped. Avoid unrelated refactors, generated output, dependency upgrades, or formatting churn.

## Development setup

You need Node.js 22. Rust and the platform prerequisites for [Tauri](https://v2.tauri.app/start/prerequisites/) are required for native and Tauri work.

```console
git clone https://github.com/cmdr-chara/deltamod.git
cd deltamod
npm ci
npm run dev
```

Do not commit `node_modules/`, `dist/`, test results, downloaded tools, native build output, or files from `.codex-run/`.

## Verification

Run the checks relevant to your change. For ordinary JavaScript or UI changes:

```console
npm test
npm run typecheck
npm run security:audit
```

For renderer and desktop integration changes:

```console
npm run test:e2e
```

For native workers:

```console
cargo fmt --all --check --manifest-path native/Cargo.toml
cargo clippy --workspace --all-targets --locked --manifest-path native/Cargo.toml -- -D warnings
cargo test --workspace --locked --manifest-path native/Cargo.toml
```

For the Tauri shell:

```console
cargo fmt --all --check --manifest-path src-tauri/Cargo.toml
cargo clippy --workspace --all-targets --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --workspace --locked --manifest-path src-tauri/Cargo.toml
```

Release changes must also satisfy [RELEASE-GATE.md](./docs/RELEASE-GATE.md).

## Pull requests

1. Create a focused branch from `DeltaMaster`.
2. Add or update regression coverage for behavior changes.
3. Run the applicable verification commands locally.
4. Explain what changed, why, how it was tested, and any remaining limitations.
5. Keep commits reviewable and do not include secrets, personal data, copyrighted game files, or unrelated generated artifacts.

Maintainers may ask for changes when a contribution alters public behavior, security boundaries, packaging, licensing, or supported platforms.
