use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PathBoundaryError {
    #[error("path is malformed")]
    Malformed,
    #[error("path escaped its transaction root")]
    EscapedRoot,
    #[error("path includes an unsafe filesystem entry")]
    UnsafeEntry,
    #[error("filesystem boundary could not be inspected")]
    Io,
}

/// A decoded, portable relative path. Deserialization always runs the same
/// validation as construction, so durable lifecycle records cannot smuggle an
/// unchecked path into a mutation plan.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ValidatedRelativePath(String);

impl ValidatedRelativePath {
    pub fn parse(value: &str) -> Result<Self, PathBoundaryError> {
        let mut decoded = value.to_owned();
        for _ in 0..5 {
            let next = percent_decode(&decoded)?;
            if next == decoded {
                break;
            }
            decoded = next;
        }
        if decoded.contains('%') {
            return Err(PathBoundaryError::Malformed);
        }
        if decoded.is_empty()
            || decoded.starts_with('/')
            || decoded.starts_with('\\')
            || decoded.contains('\\')
            || decoded.contains(':')
            || decoded.chars().any(|character| {
                character == '\0'
                    || character.is_control()
                    || matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
            })
        {
            return Err(PathBoundaryError::Malformed);
        }
        let mut count = 0;
        for component in decoded.split('/') {
            count += 1;
            if component.is_empty()
                || matches!(component, "." | "..")
                || component.ends_with(['.', ' '])
                || windows_reserved(component)
            {
                return Err(PathBoundaryError::Malformed);
            }
        }
        if count == 0 || decoded.len() > 4096 {
            return Err(PathBoundaryError::Malformed);
        }
        Ok(Self(decoded))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ValidatedRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for ValidatedRelativePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ValidatedRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

fn percent_decode(value: &str) -> Result<String, PathBoundaryError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(PathBoundaryError::Malformed);
            }
            output.push((hex(bytes[index + 1]) << 4) | hex(bytes[index + 2]));
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| PathBoundaryError::Malformed)
}

fn hex(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        b'A'..=b'F' => value - b'A' + 10,
        _ => 0,
    }
}

fn windows_reserved(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || matches!(
            stem.as_str(),
            "COM¹" | "COM²" | "COM³" | "LPT¹" | "LPT²" | "LPT³"
        )
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .and_then(|suffix| suffix.parse::<u8>().ok())
            .is_some_and(|number| (1..=9).contains(&number))
}

fn is_unsafe(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

/// Read-only preflight inspection. This deliberately does not expose a
/// `publish` method: a runtime must bind publication to opened directory/file
/// identities and no-follow platform APIs, rather than treating a stale path
/// comparison as authority.
#[derive(Clone, Debug)]
pub struct PreflightTransactionRoot {
    canonical: PathBuf,
}

impl PreflightTransactionRoot {
    pub fn open(root: &Path) -> Result<Self, PathBoundaryError> {
        let metadata = fs::symlink_metadata(root).map_err(|_| PathBoundaryError::Io)?;
        if !metadata.is_dir() || is_unsafe(&metadata) {
            return Err(PathBoundaryError::UnsafeEntry);
        }
        let canonical = fs::canonicalize(root).map_err(|_| PathBoundaryError::Io)?;
        Ok(Self { canonical })
    }

    #[must_use]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical
    }

    /// Inspect every currently existing component. This is preflight evidence,
    /// not a publication capability; the mutation runtime must repeat and bind
    /// these checks at the actual filesystem operation.
    pub fn inspect(&self, relative: &ValidatedRelativePath) -> Result<PathBuf, PathBoundaryError> {
        let mut candidate = self.canonical.clone();
        for component in relative.as_str().split('/') {
            candidate.push(component);
            match fs::symlink_metadata(&candidate) {
                Ok(metadata) => {
                    if is_unsafe(&metadata) {
                        return Err(PathBoundaryError::UnsafeEntry);
                    }
                    let resolved =
                        fs::canonicalize(&candidate).map_err(|_| PathBoundaryError::Io)?;
                    if !resolved.starts_with(&self.canonical) {
                        return Err(PathBoundaryError::EscapedRoot);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(PathBoundaryError::Io),
            }
        }
        if !candidate.starts_with(&self.canonical) {
            return Err(PathBoundaryError::EscapedRoot);
        }
        Ok(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_encoding_ads_and_device_names_are_rejected() {
        for value in [
            "../escape",
            "%2e%2e/escape",
            "%252e%252e/escape",
            "/absolute",
            "//server/share",
            "C:/drive",
            "safe\\escape",
            "file.dat:stream",
            "safe//file",
            "NUL.txt",
            "folder/COM1",
            "folder/COM¹.txt",
            "folder/LPT²",
            "folder/invalid?.dat",
            "folder/invalid*.dat",
            "folder/invalid|.dat",
            "folder/invalid<.dat",
            "folder/invalid>.dat",
            "folder/invalid\".dat",
            "folder/trailing.",
            "folder/trailing ",
        ] {
            assert_eq!(
                ValidatedRelativePath::parse(value),
                Err(PathBoundaryError::Malformed),
                "{value}"
            );
        }
        assert_eq!(
            ValidatedRelativePath::parse("mods/safe-file.dat")
                .unwrap()
                .as_str(),
            "mods/safe-file.dat"
        );
    }

    #[test]
    fn serde_cannot_bypass_validation() {
        assert!(serde_json::from_str::<ValidatedRelativePath>(r#""../escape""#).is_err());
        let safe: ValidatedRelativePath =
            serde_json::from_str(r#""mods/safe.dat""#).expect("safe path");
        assert_eq!(safe.as_str(), "mods/safe.dat");
    }

    #[test]
    fn preflight_path_stays_beneath_root() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("mods")).unwrap();
        let root = PreflightTransactionRoot::open(directory.path()).unwrap();
        let relative = ValidatedRelativePath::parse("mods/new.dat").unwrap();
        let target = root.inspect(&relative).unwrap();
        assert_eq!(target, root.canonical_path().join("mods").join("new.dat"));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected_during_preflight() {
        use std::os::unix::fs::symlink;
        let root_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root_dir.path().join("linked")).unwrap();
        let root = PreflightTransactionRoot::open(root_dir.path()).unwrap();
        let relative = ValidatedRelativePath::parse("linked/escape.dat").unwrap();
        assert_eq!(root.inspect(&relative), Err(PathBoundaryError::UnsafeEntry));
    }
}
