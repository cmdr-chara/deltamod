use deltamod_lifecycle_runtime::ProfileLockfile;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOCKFILE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct ProfileRegistry {
    root: PathBuf,
}

impl ProfileRegistry {
    pub fn open(data_root: &Path) -> Result<Self, ()> {
        let root = data_root.join("lifecycle-profiles-v1");
        if fs::symlink_metadata(&root)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(());
        }
        fs::create_dir_all(&root).map_err(|_| ())?;
        Ok(Self {
            root: fs::canonicalize(root).map_err(|_| ())?,
        })
    }

    pub fn save(&self, profile: &ProfileLockfile) -> Result<(), ()> {
        self.ensure_root()?;
        let canonical = profile.to_canonical_json().map_err(|_| ())?;
        if canonical.is_empty() || canonical.len() as u64 > MAX_LOCKFILE_BYTES {
            return Err(());
        }
        let prefix = digest_id(&profile.profile_id);
        let generation = self.next_generation(&prefix)?;
        let content_digest = digest_id(&canonical);
        let destination = self.root.join(format!(
            "{prefix}-{generation:020}-{content_digest}.lock.json"
        ));
        self.write_immutable(&destination, canonical.as_bytes())
    }

    pub fn import(&self, input: &[u8]) -> Result<ProfileLockfile, ()> {
        if input.is_empty() || input.len() as u64 > MAX_LOCKFILE_BYTES {
            return Err(());
        }
        let input = std::str::from_utf8(input).map_err(|_| ())?;
        let profile = ProfileLockfile::from_canonical_json(input).map_err(|_| ())?;
        self.save(&profile)?;
        Ok(profile)
    }

    pub fn get(&self, profile_id: &str) -> Option<ProfileLockfile> {
        self.ensure_root().ok()?;
        let prefix = digest_id(profile_id);
        self.read_profiles()
            .into_iter()
            .filter(|(_, stored_prefix, profile)| {
                stored_prefix == &prefix && profile.profile_id == profile_id
            })
            .max_by_key(|(generation, _, _)| *generation)
            .map(|(_, _, profile)| profile)
    }

    pub fn list(&self) -> Vec<ProfileLockfile> {
        if self.ensure_root().is_err() {
            return Vec::new();
        }
        let mut newest = BTreeMap::<String, (u64, ProfileLockfile)>::new();
        for (generation, _, profile) in self.read_profiles() {
            let current = newest
                .entry(profile.profile_id.clone())
                .or_insert_with(|| (generation, profile.clone()));
            if generation > current.0 {
                *current = (generation, profile);
            }
        }
        newest.into_values().map(|(_, profile)| profile).collect()
    }

    fn ensure_root(&self) -> Result<(), ()> {
        let metadata = fs::symlink_metadata(&self.root).map_err(|_| ())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(());
        }
        let canonical = fs::canonicalize(&self.root).map_err(|_| ())?;
        (canonical == self.root).then_some(()).ok_or(())
    }

    fn next_generation(&self, prefix: &str) -> Result<u64, ()> {
        self.read_profiles()
            .into_iter()
            .filter(|(_, stored_prefix, _)| stored_prefix == prefix)
            .map(|(generation, _, _)| generation)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(())
    }

    fn read_profiles(&self) -> Vec<(u64, String, ProfileLockfile)> {
        fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = fs::symlink_metadata(entry.path()).ok()?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() == 0
                    || metadata.len() > MAX_LOCKFILE_BYTES
                {
                    return None;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let (prefix, generation, expected_digest) = parse_generation_filename(&name)?;
                let bytes = fs::read(entry.path()).ok()?;
                let input = std::str::from_utf8(&bytes).ok()?;
                if digest_id(input) != expected_digest {
                    return None;
                }
                let profile = ProfileLockfile::from_canonical_json(input).ok()?;
                (digest_id(&profile.profile_id) == prefix).then_some((generation, prefix, profile))
            })
            .collect()
    }

    fn write_immutable(&self, destination: &Path, bytes: &[u8]) -> Result<(), ()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ())?
            .as_nanos();
        let temp = self.root.join(format!(
            ".profile-{}-{nonce}-{}.tmp",
            std::process::id(),
            digest_id(&format!(
                "{}:{}",
                destination.display(),
                digest_id(std::str::from_utf8(bytes).map_err(|_| ())?)
            ))
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| ())?;
        let result = (|| {
            file.write_all(bytes).map_err(|_| ())?;
            file.sync_all().map_err(|_| ())?;
            drop(file);
            fs::rename(&temp, destination).map_err(|_| ())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

fn parse_generation_filename(name: &str) -> Option<(String, u64, String)> {
    let stem = name.strip_suffix(".lock.json")?;
    let mut parts = stem.split('-');
    let prefix = parts.next()?;
    let generation = parts.next()?.parse().ok()?;
    let content_digest = parts.next()?;
    if parts.next().is_some()
        || prefix.len() != 64
        || content_digest.len() != 64
        || !prefix.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !content_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some((prefix.to_owned(), generation, content_digest.to_owned()))
}

fn digest_id(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

#[cfg(test)]
mod tests {
    use super::ProfileRegistry;
    use deltamod_lifecycle_runtime::{
        LockedProfileMod, ProfileDefinition, ProfileLockfile, ProfileModDefinition,
    };
    use deltamod_product_contracts::{
        ProviderArtifactKind, ProviderId, ProviderItemKind, ProviderRef, ProviderResourceId,
    };

    fn lockfile(profile_id: &str) -> ProfileLockfile {
        let provider = ProviderRef::new(
            ProviderId::parse("local").unwrap(),
            ProviderItemKind::LocalArchive,
            ProviderResourceId::parse("artifact").unwrap(),
            None,
            Some(ProviderResourceId::parse("artifact").unwrap()),
            ProviderArtifactKind::Archive,
            Some(ProviderResourceId::parse("v1").unwrap()),
            None,
        )
        .unwrap();
        let definition = ProfileDefinition::new(
            profile_id,
            "game",
            "installation",
            vec![ProfileModDefinition {
                order: 0,
                instance_id: "instance".into(),
                mod_id: "mod".into(),
                display_name: "Mod".into(),
                provider: provider.clone(),
                configuration_fingerprint: None,
            }],
        )
        .unwrap();
        ProfileLockfile::new(
            &definition,
            vec![LockedProfileMod {
                order: 0,
                instance_id: "instance".into(),
                mod_id: "mod".into(),
                display_name: "Mod".into(),
                version: Some("1".into()),
                provider,
                archive_sha256: "a".repeat(64),
                file_plan_fingerprint: "b".repeat(64),
                configuration_fingerprint: None,
            }],
        )
        .unwrap()
    }

    #[test]
    fn canonical_lockfiles_round_trip_without_exposing_profile_ids_in_paths() {
        let root =
            std::env::temp_dir().join(format!("deltamod-profile-registry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let registry = ProfileRegistry::open(&root).unwrap();
        let profile = lockfile("profile-a");
        registry.save(&profile).unwrap();
        let mut updated = profile.clone();
        updated.mods[0].version = Some("2".into());
        registry.save(&updated).unwrap();
        assert_eq!(registry.get("profile-a"), Some(updated.clone()));
        assert_eq!(registry.list(), vec![updated]);
        let names = std::fs::read_dir(root.join("lifecycle-profiles-v1"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().all(|name| !name.contains("profile-a")));
        assert_eq!(names.len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_noncanonical_and_forward_version_documents() {
        let root =
            std::env::temp_dir().join(format!("deltamod-profile-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let registry = ProfileRegistry::open(&root).unwrap();
        let canonical = lockfile("profile-b").to_canonical_json().unwrap();
        assert!(registry
            .import(format!("{canonical}\n").as_bytes())
            .is_err());
        let future = canonical.replace("\"schemaVersion\":1", "\"schemaVersion\":2");
        assert!(registry.import(future.as_bytes()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
