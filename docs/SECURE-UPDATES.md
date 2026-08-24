# Secure updates — 2.1

Deltamod Community treats automatic updates as a software-supply-chain boundary. An update must not become installable merely because it was downloaded over HTTPS or published on GitHub.

## Current state

Automatic installation is intentionally disabled. Stable releases remain manual downloads while the signed updater path is completed.

The Tauri updater runtime already fails closed unless all of these conditions are true:

- the application is a packaged build;
- updater artifacts were created for that build;
- every metadata endpoint is HTTPS;
- a public verification key is configured;
- the downloaded payload passes mandatory signature verification;
- the payload remains within the configured size bound;
- the verified payload version matches the version that was offered.

`scripts/verify-secure-updater-config.js` adds a repository-level policy gate around that runtime. If updater artifacts are enabled, CI requires the stable metadata endpoint to be exactly:

`https://github.com/cmdr-chara/deltamod/releases/latest/download/latest.json`

It also rejects insecure transport, credentials in update URLs, private signing material in application configuration, and a stable release workflow that does not source the signing key from GitHub Actions secrets.

## Security invariants

1. **Fail closed.** Missing metadata, keys, signatures, or unsupported packaging must disable updating rather than bypass verification.
2. **Private signing keys never enter the repository or application bundle.** Only the public verification key may be shipped with Deltamod.
3. **HTTPS is transport protection, not artifact trust.** The updater signature remains mandatory even when GitHub hosts the payload.
4. **Release metadata and artifacts are version-bound.** A verified payload for another version must not be installed.
5. **Downloads are bounded.** The runtime default is 512 MiB and must reject declared or cumulative download sizes above its limit.
6. **One update operation at a time.** Updater state remains application-owned so concurrent checks or installations cannot race each other.
7. **No silent expansion to unsupported package formats.** A platform is enabled only after its actual release package has a tested updater path.

## Rollout stages

### Stage 0 — policy gate

Implemented by this 2.1 security tranche:

- automatic updates remain disabled;
- CI validates the updater trust configuration;
- unsafe or partially configured enablement fails the policy check;
- the existing signed-update runtime remains the only permitted installation boundary.

Rollback: remove the policy-only branch changes. Stable releases continue to work exactly as manual downloads.

### Stage 1 — signing-key ceremony

Before enabling updater artifacts:

1. Generate the Tauri updater signing key outside the repository.
2. Store the private key only in the GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY`.
3. If the private key is encrypted, store its password as a separate Actions secret.
4. Put only the corresponding public key in `plugins.updater.pubkey`.
5. Back up the private key in an access-controlled offline location. Losing it prevents already-installed clients from trusting future update signatures.

The private key must never be placed in `tauri.conf.json`, a workflow literal, a release artifact, a test fixture, or a committed `.env` file.

### Stage 2 — signed Windows and macOS updates

After the official updater dependency and generated Cargo lockfile are reviewed:

- enable `bundle.createUpdaterArtifacts`;
- configure the canonical `latest.json` endpoint and updater public key;
- build the updater artifacts and `.sig` files through the stable release workflow;
- generate `latest.json` only from artifacts produced by that release;
- publish the manifest, signatures, artifacts, SHA-256 manifest, and GitHub attestations together;
- wire `fireUpdate`, `start-update`, `ignore-update`, updater status, and progress to the concrete Tauri updater adapter;
- verify update and failure paths on Windows x64, macOS Intel, and macOS Apple Silicon before making checks automatic.

Promotion gate: signature mismatch, missing artifact, wrong version, oversized payload, interrupted download, or metadata failure must all leave the currently installed application runnable.

### Stage 3 — Linux decision

The current stable Linux distribution is a Debian package. Do not claim Linux automatic updates until the installed package format has a tested and supportable update mechanism.

Options are:

- keep `.deb` releases on manual updates; or
- add a stable AppImage distribution and validate Tauri's signed updater path for that package separately.

The Windows/macOS updater must not silently treat the Linux `.deb` as equivalent to an updater-compatible bundle.

### Stage 4 — recovery and default-on rollout

Automatic installation should not become the default until recovery behavior is tested. Required work includes:

- retain a known-good release path;
- make interrupted update recovery deterministic;
- document manual recovery when the platform installer itself fails;
- add stable/beta channel separation without sharing mutable unsigned metadata;
- observe update success/failure telemetry without collecting secrets or user content.

## Verification

Repository policy:

```text
node scripts/verify-secure-updater-config.js --self-test
node scripts/verify-secure-updater-config.js
```

Updater runtime when host integration is introduced:

```text
cargo fmt --all --check --manifest-path src-tauri/Cargo.toml
cargo test --workspace --locked --manifest-path src-tauri/Cargo.toml
cargo clippy --workspace --all-targets --locked --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Release promotion must additionally exercise real signed artifacts on every platform enabled for automatic installation.

## Explicit non-goals of Stage 0

This change does **not** claim that automatic updating is already available, that unsigned stable releases have become signed, or that rollback is complete. It establishes the policy and trust boundary required to enable those features without weakening the existing release security model.
