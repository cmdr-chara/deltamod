# Tauri Integration

The adapter is intentionally independent of Tauri. Construct one `CredentialStore< KeyringBackend >` during app setup and keep it in managed state. Never pass secrets through logs, events, URLs, command-line arguments, or renderer persistence.

Credentials are an internal native dependency, not a renderer credential API. Do not add generic store/load channels. Wire only the existing purpose-specific channels:

- `logoutGamebanana` clears `CredentialKind::GameBananaCookies` and any native GameBanana UI-config cache.
- `modSources:clearNexusKey` cancels active OAuth, clears `CredentialKind::NexusOAuthTokens` plus the retired legacy SSO entry, and returns `true` only after keyring deletion succeeds.
- `validateGamebananaToken`, comments, likes, and collection operations load the GameBanana cookie only for the immediate native HTTPS request.
- Nexus status/download operations load a current Nexus access token only for the immediate native HTTPS request.
- Successful restricted GameBanana login and validated Nexus OAuth PKCE are the only writers.

Map adapter errors to bounded stable strings such as `CREDENTIALS_UNAVAILABLE`, `CREDENTIALS_NOT_FOUND`, and `CREDENTIALS_INVALID`; do not include the underlying keyring error or secret. On Linux, require a running Secret Service provider (for example GNOME Keyring or KeePassXC). If it is unavailable, report `CREDENTIALS_UNAVAILABLE` and do not use a file, environment variable, or plaintext fallback.

Electron migration must be an explicit one-time native flow: decrypt the legacy blob through an `ElectronBlobDecryptor`, parse it through an `ElectronBlobMigrator`, write each result with `migrate_electron_blob`, then delete the legacy blob only after all writes succeed. The adapter has no built-in Electron crypto assumptions.
