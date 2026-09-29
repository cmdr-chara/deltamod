use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const CACHE_SCHEMA_VERSION: u32 = 1;
const MAX_CATALOG_ENTRY_BYTES: u64 = 4 * 1024 * 1024;
// Catalogue JSON is redownloadable metadata, not an archive or recovery copy.
const MAX_CATALOG_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATALOG_ENTRIES: usize = 512;
const ACCESS_WRITE_INTERVAL_MS: u64 = 60 * 1_000;
const FRESH_TTL_MS: u64 = 10 * 60 * 1_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogCacheEntry<T = Value> {
    schema_version: u32,
    request_key: String,
    stored_at_ms: u64,
    last_accessed_at_ms: u64,
    result: T,
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
        let cache = Self {
            root,
            max_bytes: MAX_CATALOG_CACHE_BYTES,
        };
        // Apply the smaller budget to existing caches too. This scans metadata
        // only and never touches downloaded archives or recovery generations.
        cache.prune();
        Ok(cache)
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
            || is_link_or_reparse(&metadata)
            || metadata.len() == 0
            || metadata.len() > MAX_CATALOG_ENTRY_BYTES
        {
            return None;
        }
        // Bound the actual read as well as the stat: a file can grow between them.
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        fs::File::open(&path)
            .ok()?
            .take(MAX_CATALOG_ENTRY_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() as u64 > MAX_CATALOG_ENTRY_BYTES {
            return None;
        }
        let mut entry: CatalogCacheEntry = serde_json::from_slice(&bytes).ok()?;
        drop(bytes);
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
        // Approximate LRU is sufficient for redownloadable catalogue pages.
        // Coalesce access updates so repeated hits do not serialize and fsync
        // the complete response. Access never extends the provider freshness TTL.
        if now.saturating_sub(entry.last_accessed_at_ms) >= ACCESS_WRITE_INTERVAL_MS {
            entry.last_accessed_at_ms = now;
            let _ = self.write_entry(&path, &entry);
        }
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
            result,
        };
        let path = self.entry_path(request_key).ok_or(())?;
        self.write_entry(&path, &entry)?;
        self.prune();
        Ok(())
    }

    pub fn usage_bytes(&self) -> u64 {
        self.entries()
            .into_iter()
            .fold(0u64, |total, (_, size, _)| total.saturating_add(size))
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
        if !valid_request_key(request_key) || !self.root_is_trusted() {
            return None;
        }
        Some(self.root.join(format!("{request_key}.json")))
    }

    fn write_entry<T: Serialize>(
        &self,
        path: &Path,
        entry: &CatalogCacheEntry<T>,
    ) -> Result<(), ()> {
        if !self.root_is_trusted() {
            return Err(());
        }
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
        let mut total = entries
            .iter()
            .fold(0u64, |total, entry| total.saturating_add(entry.1));
        let mut count = entries.len();
        // Sorting is unnecessary for the common, under-budget case.
        if total <= self.max_bytes && count <= MAX_CATALOG_ENTRIES {
            return;
        }
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
        for (_, size, path) in entries {
            if total <= self.max_bytes && count <= MAX_CATALOG_ENTRIES {
                break;
            }
            if fs::remove_file(path).is_ok() {
                total = total.saturating_sub(size);
                count -= 1;
            }
        }
    }

    fn entries(&self) -> Vec<(SystemTime, u64, PathBuf)> {
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
                    || is_link_or_reparse(&metadata)
                    || path.extension().and_then(|value| value.to_str()) != Some("json")
                    || !path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .is_some_and(valid_request_key)
                {
                    return None;
                }
                // Writes and coalesced access updates refresh mtime. Inspecting
                // usage or choosing victims must not read/parse every JSON body.
                let accessed = metadata.modified().unwrap_or(UNIX_EPOCH);
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

fn valid_request_key(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

    fn temporary_cache(label: &str) -> (std::path::PathBuf, ProviderCatalogCache) {
        let root = std::env::temp_dir().join(format!(
            "deltamod-provider-cache-{label}-{}-{}",
            std::process::id(),
            super::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let cache = ProviderCatalogCache::open(&root).unwrap();
        (root, cache)
    }

    #[test]
    fn catalog_budget_is_separate_from_download_and_recovery_budgets() {
        let (root, cache) = temporary_cache("budget");
        assert_eq!(cache.max_bytes, 64 * 1024 * 1024);
        assert!(cache.max_bytes < deltamod_product_contracts::DEFAULT_CACHE_LIMIT_BYTES);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_hits_do_not_rewrite_or_extend_freshness() {
        let (root, mut cache) = temporary_cache("hits");
        let key = ProviderCatalogCache::request_key(&["hits"]);
        let path = cache.entry_path(&key).unwrap();
        let now = super::now_ms();
        let stored_at = now - super::FRESH_TTL_MS - 1;
        let entry = super::CatalogCacheEntry {
            schema_version: super::CACHE_SCHEMA_VERSION,
            request_key: key.clone(),
            stored_at_ms: stored_at,
            last_accessed_at_ms: now,
            result: json!({"items":[]}),
        };
        cache.write_entry(&path, &entry).unwrap();
        let before = std::fs::read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        for _ in 0..3 {
            let result = cache.get(&key).unwrap();
            assert_eq!(result.freshness, CacheFreshness::Stale);
            assert_eq!(result.stored_at_ms, stored_at);
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn old_access_is_refreshed_without_refreshing_the_provider_ttl() {
        let (root, mut cache) = temporary_cache("touch");
        let key = ProviderCatalogCache::request_key(&["touch"]);
        let path = cache.entry_path(&key).unwrap();
        let entry = super::CatalogCacheEntry {
            schema_version: super::CACHE_SCHEMA_VERSION,
            request_key: key.clone(),
            stored_at_ms: 1,
            last_accessed_at_ms: 1,
            result: json!({"items":[]}),
        };
        cache.write_entry(&path, &entry).unwrap();
        assert_eq!(cache.get(&key).unwrap().freshness, CacheFreshness::Stale);
        let after: super::CatalogCacheEntry =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(after.last_accessed_at_ms > 1);
        assert_eq!(after.stored_at_ms, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn metadata_scan_counts_corrupt_entries_without_parsing_them() {
        let (root, mut cache) = temporary_cache("metadata");
        let key = ProviderCatalogCache::request_key(&["corrupt"]);
        let path = cache.entry_path(&key).unwrap();
        std::fs::write(&path, b"not json").unwrap();
        let unrelated = cache.root.join("keep.json");
        std::fs::write(&unrelated, b"not a catalogue entry").unwrap();
        let recovery = root.join("recovery");
        std::fs::write(&recovery, b"keep recovery").unwrap();
        assert_eq!(cache.usage_bytes(), 8);
        assert!(cache.get(&key).is_none());
        assert_eq!(cache.clear_redownloadable().unwrap(), 8);
        assert!(unrelated.exists());
        assert!(recovery.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prune_uses_modification_time_and_only_removes_old_catalogue_entries() {
        let (root, mut cache) = temporary_cache("prune");
        let old = cache
            .entry_path(&ProviderCatalogCache::request_key(&["old"]))
            .unwrap();
        let recent = cache
            .entry_path(&ProviderCatalogCache::request_key(&["recent"]))
            .unwrap();
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(&recent, b"new").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(super::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000)),
            )
            .unwrap();
        cache.max_bytes = 3;
        cache.prune();
        assert!(!old.exists());
        assert!(recent.exists());
        assert_eq!(cache.usage_bytes(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reopening_bounds_the_number_of_small_catalogue_entries() {
        let (root, cache) = temporary_cache("count");
        for index in 0..=super::MAX_CATALOG_ENTRIES {
            let key = ProviderCatalogCache::request_key(&[&index.to_string()]);
            std::fs::write(cache.entry_path(&key).unwrap(), b"{}").unwrap();
        }
        let reopened = ProviderCatalogCache::open(&root).unwrap();
        assert_eq!(reopened.entries().len(), super::MAX_CATALOG_ENTRIES);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_entry_is_not_loaded() {
        let (root, mut cache) = temporary_cache("oversized");
        let key = ProviderCatalogCache::request_key(&["large"]);
        let file = std::fs::File::create(cache.entry_path(&key).unwrap()).unwrap();
        file.set_len(super::MAX_CATALOG_ENTRY_BYTES + 1).unwrap();
        drop(file);
        assert!(cache.get(&key).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
