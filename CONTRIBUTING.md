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

For provenance-sensitive changes from a full Git clone:

```console
node scripts/verify-community-provenance.js
```

The dedicated Community Provenance workflow reruns this check with full Git history and requires every registered original-work path to retain its SPDX notices and match its recorded file-introduction commit.

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

## Contribution licensing

Deltamod Community's covered software is distributed under **EUPL-1.2**. By submitting a contribution for inclusion in the repository, you represent that you own the contribution or otherwise have sufficient rights to provide it for distribution under **EUPL-1.2 on the same terms as the repository's covered work**.

You retain copyright in your contribution. Submission does **not** assign your copyright to Deltamod Community or its maintainers.

Do not submit code, assets, generated material, or other content copied from sources with incompatible, unclear, or unsatisfied licensing terms. Preserve required copyright, licence, attribution, patent, trademark, and SPDX notices, and identify separately licensed material in the pull request.

If you add a new file that should be recorded as Community-original work, add an in-file SPDX copyright/licence notice and update [`ORIGINAL_WORK.md`](./ORIGINAL_WORK.md) plus [`provenance/community-original-work.json`](./provenance/community-original-work.json). Do not register inherited or mixed-history files as whole-file Community originals.

Read [LICENSING.md](./LICENSING.md) for the repository's licensing and compliance guide, [COPYRIGHT.md](./COPYRIGHT.md) for ownership boundaries, and [PROVENANCE.md](./PROVENANCE.md) before importing code from upstream or other projects.

## Pull requests

1. Create a focused branch from `DeltaMaster`.
2. Add or update regression coverage for behavior changes.
3. Run the applicable verification commands locally.
4. Explain what changed, why, how it was tested, and any remaining limitations.
5. Keep commits reviewable and do not include secrets, personal data, copyrighted game files, or unrelated generated artifacts.
6. Confirm that you have the right to submit the contribution and that any separately licensed material is clearly identified with its required notices.
7. Preserve registered SPDX notices and update the original-work evidence registry only when the new record is supported by Git history.

Maintainers may ask for changes when a contribution alters public behavior, security boundaries, packaging, licensing, provenance, or supported platforms.
