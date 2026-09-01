use deltamod_product_contracts::DEFAULT_CACHE_LIMIT_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const CACHE_SCHEMA_VERSION: u32 = 1;
const MAX_CATALOG_ENTRY_BYTES: u64 = 4 * 1024 * 1024;
const FRESH_TTL_MS: u64 = 10 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogCacheEntry {
    schema_version: u32,
    request_key: String,
    stored_at_ms: u64,
    last_accessed_at_ms: u64,
    result: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheFreshness {
    Fresh,
    Stale,
}

#[derive(Debug)]
pub struct CachedCatalog {
    pub freshness: CacheFreshness,
    pub stored_at_ms: u64,
    pub result: Value,
}

#[derive(Debug)]
pub struct ProviderCatalogCache {
    root: PathBuf,
    max_bytes: u64,
}

impl ProviderCatalogCache {
    pub fn open(data_root: &Path) -> Result<Self, ()> {
        let root = data_root.join("provider-cache").join("catalog-v1");
        if fs::symlink_metadata(&root)
            .map(|metadata| is_link_or_reparse(&metadata))
            .unwrap_or(false)
        {
            return Err(());
        }
        fs::create_dir_all(&root).map_err(|_| ())?;
        let root = fs::canonicalize(root).map_err(|_| ())?;
        Ok(Self {
            root,
            max_bytes: DEFAULT_CACHE_LIMIT_BYTES,
        })
    }

    pub fn request_key(parts: &[&str]) -> String {
        let mut digest = Sha256::new();
        digest.update(b"deltamod/provider-catalog-cache/v1\0");
        for part in parts {
            digest.update((part.len() as u64).to_be_bytes());
            digest.update(part.as_bytes());
        }
        digest
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            })
    }

    pub fn get(&mut self, request_key: &str) -> Option<CachedCatalog> {
        let path = self.entry_path(request_key)?;
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() == 0
            || metadata.len() > MAX_CATALOG_ENTRY_BYTES
        {
            return None;
        }
        let mut entry: CatalogCacheEntry = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
        if entry.schema_version != CACHE_SCHEMA_VERSION
            || entry.request_key != request_key
            || !entry.result.is_object()
        {
            return None;
        }
        let now = now_ms();
        let freshness = if now.saturating_sub(entry.stored_at_ms) <= FRESH_TTL_MS {
            CacheFreshness::Fresh
        } else {
            CacheFreshness::Stale
        };
        entry.last_accessed_at_ms = now;
        let _ = self.write_entry(&path, &entry);
        Some(CachedCatalog {
            freshness,
            stored_at_ms: entry.stored_at_ms,
            result: entry.result,
        })
    }

    pub fn put(&mut self, request_key: &str, result: &Value) -> Result<(), ()> {
        if !result.is_object() {
            return Err(());
        }
        let now = now_ms();
        let entry = CatalogCacheEntry {
            schema_version: CACHE_SCHEMA_VERSION,
            request_key: request_key.to_owned(),
            stored_at_ms: now,
            last_accessed_at_ms: now,
            result: result.clone(),
        };
        let path = self.entry_path(request_key).ok_or(())?;
        self.write_entry(&path, &entry)?;
        self.prune();
        Ok(())
    }

    pub fn usage_bytes(&self) -> u64 {
        self.entries().into_iter().map(|(_, size, _)| size).sum()
    }

    pub fn clear_redownloadable(&self) -> Result<u64, ()> {
        if !self.root_is_trusted() {
            return Err(());
        }
        let mut removed = 0u64;
        for (_, size, path) in self.entries() {
            fs::remove_file(path).map_err(|_| ())?;
            removed = removed.saturating_add(size);
        }
        Ok(removed)
    }

    fn entry_path(&self, request_key: &str) -> Option<PathBuf> {
        if request_key.len() != 64 || !request_key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(self.root.join(format!("{request_key}.json")))
    }

    fn write_entry(&self, path: &Path, entry: &CatalogCacheEntry) -> Result<(), ()> {
        let bytes = serde_json::to_vec(entry).map_err(|_| ())?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_CATALOG_ENTRY_BYTES {
            return Err(());
        }
        let temp = self
            .root
            .join(format!(".catalog-{}-{}.tmp", std::process::id(), now_ms()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| ())?;
        let result = (|| {
            file.write_all(&bytes).map_err(|_| ())?;
            file.sync_all().map_err(|_| ())?;
            drop(file);
            if path.exists() {
                fs::remove_file(path).map_err(|_| ())?;
            }
            fs::rename(&temp, path).map_err(|_| ())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }

    fn prune(&self) {
        let mut entries = self.entries();
        let mut total = entries.iter().map(|entry| entry.1).sum::<u64>();
        entries.sort_by_key(|entry| entry.0);
        for (_, size, path) in entries {
            if total <= self.max_bytes {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }

    fn entries(&self) -> Vec<(u64, u64, PathBuf)> {
        if !self.root_is_trusted() {
            return Vec::new();
        }
        fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).ok()?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || path.extension().and_then(|value| value.to_str()) != Some("json")
                {
                    return None;
                }
                let accessed = serde_json::from_slice::<CatalogCacheEntry>(&fs::read(&path).ok()?)
                    .map(|entry| entry.last_accessed_at_ms)
                    .unwrap_or(0);
                Some((accessed, metadata.len(), path))
            })
            .collect()
    }

    fn root_is_trusted(&self) -> bool {
        fs::symlink_metadata(&self.root)
            .ok()
            .is_some_and(|metadata| metadata.is_dir() && !is_link_or_reparse(&metadata))
            && fs::canonicalize(&self.root)
                .ok()
                .is_some_and(|canonical| canonical == self.root)
    }
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{CacheFreshness, ProviderCatalogCache};
    use serde_json::json;

    #[test]
    fn cache_round_trip_uses_a_digest_key_and_never_exposes_query_text() {
        let root = std::env::temp_dir().join(format!(
            "deltamod-provider-cache-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut cache = ProviderCatalogCache::open(&root).unwrap();
        let key = ProviderCatalogCache::request_key(&[
            "nexus",
            "deltarune",
            "secret-looking search",
            "trending",
            "0",
            "50",
        ]);
        assert_eq!(key.len(), 64);
        assert!(!key.contains("secret"));
        cache
            .put(&key, &json!({"provider":"nexus","items":[]}))
            .unwrap();
        let restored = cache.get(&key).unwrap();
        assert_eq!(restored.freshness, CacheFreshness::Fresh);
        assert_eq!(restored.result["provider"], "nexus");
        assert!(cache.usage_bytes() > 0);
        assert!(cache.clear_redownloadable().unwrap() > 0);
        assert_eq!(cache.usage_bytes(), 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_keys_cannot_escape_the_cache_root() {
        let root = std::env::temp_dir().join(format!(
            "deltamod-provider-cache-key-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut cache = ProviderCatalogCache::open(&root).unwrap();
        assert!(cache.get("../../outside").is_none());
        assert!(cache.put("../../outside", &json!({})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
