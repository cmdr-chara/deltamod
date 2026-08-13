#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, sync::Arc};
use thiserror::Error;
use zeroize::Zeroize;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_SECRET_BYTES: usize = 64 * 1024;
const SERVICE: &str = "deltamod";
const METADATA_USER: &str = "credentials-schema";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CredentialKind {
    GameBananaCookies,
    NexusOAuthTokens,
    NexusLegacySsoKey,
}

impl CredentialKind {
    fn user(self) -> &'static str {
        match self {
            Self::GameBananaCookies => "gamebanana-cookies",
            Self::NexusOAuthTokens => "nexus-oauth-tokens",
            Self::NexusLegacySsoKey => "nexus-sso-key",
        }
    }

    pub fn keyring_account(self) -> &'static str {
        self.user()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialMetadata {
    pub schema_version: u32,
    pub present: BTreeMap<String, bool>,
}

#[derive(Clone, Eq, PartialEq, Zeroize)]
#[zeroize(drop)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        if value.is_empty() {
            return Err(Error::InvalidInput);
        }
        if value.len() > MAX_SECRET_BYTES {
            return Err(Error::SecretTooLarge);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid credential input")]
    InvalidInput,
    #[error("credential is too large")]
    SecretTooLarge,
    #[error("credential is not available")]
    NotFound,
    #[error("secure credential store is unavailable")]
    Unavailable,
    #[error("secure credential store operation failed")]
    Backend,
    #[error("credential metadata is invalid")]
    Metadata,
    #[error("migration failed")]
    Migration,
}

pub trait Backend: Send + Sync {
    fn get(&self, user: &str) -> Result<Option<String>, Error>;
    fn set(&self, user: &str, value: &str) -> Result<(), Error>;
    fn delete(&self, user: &str) -> Result<(), Error>;
}

pub struct KeyringBackend;

impl Default for KeyringBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyringBackend {
    pub fn new() -> Self {
        Self
    }
    fn entry(user: &str) -> Result<keyring::Entry, Error> {
        keyring::Entry::new(SERVICE, user).map_err(|_| Error::Unavailable)
    }
}

impl Backend for KeyringBackend {
    fn get(&self, user: &str) -> Result<Option<String>, Error> {
        match Self::entry(user)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(Error::Unavailable),
        }
    }
    fn set(&self, user: &str, value: &str) -> Result<(), Error> {
        Self::entry(user)?
            .set_password(value)
            .map_err(|_| Error::Unavailable)
    }
    fn delete(&self, user: &str) -> Result<(), Error> {
        match Self::entry(user)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(Error::Unavailable),
        }
    }
}

pub struct CredentialStore<B: Backend> {
    backend: Arc<B>,
}

impl<B: Backend> CredentialStore<B> {
    pub fn new(backend: Arc<B>) -> Result<Self, Error> {
        let store = Self { backend };
        store.ensure_metadata()?;
        Ok(store)
    }
    pub fn clear(&self, kind: CredentialKind) -> Result<(), Error> {
        self.backend.delete(kind.user())?;
        self.update_presence(kind, false)
    }
    pub fn status(&self) -> Result<CredentialMetadata, Error> {
        let raw = self.backend.get(METADATA_USER)?.ok_or(Error::Metadata)?;
        serde_json::from_str(&raw).map_err(|_| Error::Metadata)
    }
    pub fn store(&self, kind: CredentialKind, secret: Secret) -> Result<(), Error> {
        self.backend.set(kind.user(), secret.expose())?;
        if let Err(error) = self.update_presence(kind, true) {
            let _ = self.backend.delete(kind.user());
            return Err(error);
        }
        Ok(())
    }
    pub fn load(&self, kind: CredentialKind) -> Result<Option<Secret>, Error> {
        self.backend.get(kind.user())?.map(Secret::new).transpose()
    }
    fn ensure_metadata(&self) -> Result<(), Error> {
        if self.backend.get(METADATA_USER)?.is_none() {
            let metadata = CredentialMetadata {
                schema_version: SCHEMA_VERSION,
                present: BTreeMap::new(),
            };
            let encoded = serde_json::to_string(&metadata).map_err(|_| Error::Metadata)?;
            self.backend.set(METADATA_USER, &encoded)?;
        }
        Ok(())
    }
    fn update_presence(&self, kind: CredentialKind, present: bool) -> Result<(), Error> {
        let mut metadata = self.status()?;
        metadata.present.insert(kind.user().to_owned(), present);
        let encoded = serde_json::to_string(&metadata).map_err(|_| Error::Metadata)?;
        self.backend
            .set(METADATA_USER, &encoded)
            .map_err(|_| Error::Unavailable)
    }
}

pub trait ElectronBlobDecryptor {
    fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>, Error>;
}
pub trait ElectronBlobMigrator {
    fn migrate(&self, decrypted: &[u8]) -> Result<Vec<(CredentialKind, Secret)>, Error>;
}

pub fn migrate_electron_blob<B: Backend, D: ElectronBlobDecryptor, M: ElectronBlobMigrator>(
    store: &CredentialStore<B>,
    blob: &[u8],
    decryptor: &D,
    migrator: &M,
) -> Result<usize, Error> {
    if blob.len() > MAX_SECRET_BYTES {
        return Err(Error::SecretTooLarge);
    }
    let decrypted = decryptor.decrypt(blob).map_err(|_| Error::Migration)?;
    if decrypted.len() > MAX_SECRET_BYTES {
        return Err(Error::SecretTooLarge);
    }
    let credentials = migrator.migrate(&decrypted).map_err(|_| Error::Migration)?;
    let mut count = 0;
    for (kind, secret) in credentials {
        store.store(kind, secret)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Mock(Mutex<BTreeMap<String, String>>);
    impl Backend for Mock {
        fn get(&self, k: &str) -> Result<Option<String>, Error> {
            Ok(self.0.lock().map_err(|_| Error::Backend)?.get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> Result<(), Error> {
            self.0
                .lock()
                .map_err(|_| Error::Backend)?
                .insert(k.into(), v.into());
            Ok(())
        }
        fn delete(&self, k: &str) -> Result<(), Error> {
            self.0.lock().map_err(|_| Error::Backend)?.remove(k);
            Ok(())
        }
    }
    #[test]
    fn round_trip_and_clear() {
        let store = CredentialStore::new(Arc::new(Mock::default())).unwrap();
        store
            .store(
                CredentialKind::GameBananaCookies,
                Secret::new("a=b").unwrap(),
            )
            .unwrap();
        assert_eq!(
            store
                .load(CredentialKind::GameBananaCookies)
                .unwrap()
                .unwrap()
                .expose(),
            "a=b"
        );
        assert!(store.status().unwrap().present["gamebanana-cookies"]);
        store.clear(CredentialKind::GameBananaCookies).unwrap();
        assert!(store
            .load(CredentialKind::GameBananaCookies)
            .unwrap()
            .is_none());
    }
    #[test]
    fn bounds_and_redaction() {
        assert!(matches!(
            Secret::new("x".repeat(MAX_SECRET_BYTES + 1)),
            Err(Error::SecretTooLarge)
        ));
        assert_eq!(
            format!("{:?}", Secret::new("secret").unwrap()),
            "Secret([redacted])"
        );
    }
}
