# Profile/install runtime integration

This crate is intentionally Tauri-independent. Add the following dependencies to the Tauri adapter's `Cargo.toml` (use repository-relative paths in the real workspace):

```toml
deltamod-profile-install-runtime = { path = "../profile-install-runtime" }
```

The runtime itself uses these staged domain crates:

```toml
deltamod-installations-domain = { path = ".../src-tauri/crates/installations" }
deltamod-native-core = { path = ".../native/core" }
deltamod-storage-domain = { path = ".../src-tauri/crates/storage" }
```

Construct `Runtime::open(app_data_dir)` during startup. Pass renderer-selected paths to `import_official_profile` or `create_installation`; this crate never opens dialogs. Forward `events()` (or replace it with a host event sink) to the renderer. Route `cancelOfficialProfileImport` and operation cancellation to `cancel(id)`.

The legacy channels remain value-compatible: `legacy_installations()` returns an array and `legacy_system_index()` returns a number. Installation APIs return the installation-domain camelCase envelopes. Managed copies are created through the hardened staged copier; linked installations never delete their external source. Journals are under `.runtime-journals`; startup removes abandoned staging and journal files before serving requests.

Use `Runtime::with_backend` to inject a test or platform copy implementation. Do not bypass the runtime state file or mutate managed directories directly.
