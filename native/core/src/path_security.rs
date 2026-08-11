use std::env;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseSensitivity {
    Sensitive,
    Insensitive,
}

impl CaseSensitivity {
    pub const fn host() -> Self {
        if cfg!(windows) {
            Self::Insensitive
        } else {
            Self::Sensitive
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum UnsafePathError {
    #[error("Path must be a non-empty string.")]
    Empty,
    #[error("Path contains invalid URL encoding.")]
    InvalidUrlEncoding,
    #[error("Path contains a null byte.")]
    NullByte,
    #[error("Absolute and device paths are not allowed.")]
    AbsoluteOrDevice,
    #[error("Path traversal is not allowed.")]
    Traversal,
}

impl UnsafePathError {
    pub const fn code(self) -> &'static str {
        "UNSAFE_PATH"
    }
}

pub fn decode_path(value: &str) -> Result<String, UnsafePathError> {
    let mut decoded = value.to_owned();
    for _ in 0..3 {
        let bytes = decoded.as_bytes();
        let mut next = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                if index + 2 >= bytes.len() {
                    return Err(UnsafePathError::InvalidUrlEncoding);
                }
                let high = hex(bytes[index + 1]).ok_or(UnsafePathError::InvalidUrlEncoding)?;
                let low = hex(bytes[index + 2]).ok_or(UnsafePathError::InvalidUrlEncoding)?;
                next.push((high << 4) | low);
                index += 3;
            } else {
                next.push(bytes[index]);
                index += 1;
            }
        }
        let next = String::from_utf8(next).map_err(|_| UnsafePathError::InvalidUrlEncoding)?;
        if next == decoded {
            break;
        }
        decoded = next;
    }
    Ok(decoded)
}

pub fn validate_relative_path(candidate: &str) -> Result<PathBuf, UnsafePathError> {
    if candidate.is_empty() {
        return Err(UnsafePathError::Empty);
    }
    let decoded = decode_path(candidate)?;
    if decoded.contains('\0') {
        return Err(UnsafePathError::NullByte);
    }
    if is_cross_platform_absolute(&decoded) {
        return Err(UnsafePathError::AbsoluteOrDevice);
    }

    let parts: Vec<_> = decoded.split(['/', '\\']).collect();
    if parts.contains(&"..") {
        return Err(UnsafePathError::Traversal);
    }
    let mut normalized = PathBuf::new();
    for part in parts {
        if !part.is_empty() && part != "." {
            normalized.push(part);
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(".");
    }
    Ok(normalized)
}

pub fn is_within(root: &Path, target: &Path, allow_root: bool) -> bool {
    is_within_with_case(root, target, allow_root, CaseSensitivity::host())
}

pub fn is_within_with_case(
    root: &Path,
    target: &Path,
    allow_root: bool,
    case_sensitivity: CaseSensitivity,
) -> bool {
    let Ok(root) = absolute_lexical(root) else {
        return false;
    };
    let Ok(target) = absolute_lexical(target) else {
        return false;
    };
    let root_parts = comparison_parts(&root, case_sensitivity);
    let target_parts = comparison_parts(&target, case_sensitivity);
    if root_parts == target_parts {
        return allow_root;
    }
    target_parts.len() > root_parts.len() && target_parts.starts_with(&root_parts)
}

fn is_cross_platform_absolute(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

fn absolute_lexical(path: &Path) -> std::io::Result<PathBuf> {
    let source = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let mut result = PathBuf::new();
    for component in source.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}

fn comparison_parts(path: &Path, case_sensitivity: CaseSensitivity) -> Vec<String> {
    path.components()
        .map(|part| {
            let value = part.as_os_str().to_string_lossy().into_owned();
            match case_sensitivity {
                CaseSensitivity::Sensitive => value,
                CaseSensitivity::Insensitive => value.to_lowercase(),
            }
        })
        .collect()
}

const fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_three_times_and_rejects_malformed_encoding() {
        assert_eq!(decode_path("safe%25252fname").unwrap(), "safe/name");
        assert_eq!(decode_path("%2525252e%2525252e").unwrap(), "%2e%2e");
        for invalid in ["%", "%2", "%gg", "%E0%A4%A"] {
            assert_eq!(
                decode_path(invalid),
                Err(UnsafePathError::InvalidUrlEncoding)
            );
        }
    }

    #[test]
    fn validates_and_normalizes_relative_paths() {
        assert_eq!(
            validate_relative_path("mods//./data\\file").unwrap(),
            PathBuf::from("mods/data/file")
        );
        assert_eq!(validate_relative_path(".").unwrap(), PathBuf::from("."));
        for unsafe_path in [
            "",
            "../file",
            "a\\..\\file",
            "%252e%252e%252ffile",
            "/etc/passwd",
            "\\rooted",
            "C:relative",
            "C:\\absolute",
            "\\\\server\\share",
            "\\\\?\\C:\\file",
            "\\\\.\\device",
            "file\0name",
        ] {
            assert!(
                validate_relative_path(unsafe_path).is_err(),
                "{unsafe_path:?}"
            );
        }
    }

    #[test]
    fn checks_lexical_containment_and_root_handling() {
        let root = env::current_dir().unwrap().join("Root");
        assert!(!is_within_with_case(
            &root,
            &root,
            false,
            CaseSensitivity::Sensitive
        ));
        assert!(is_within_with_case(
            &root,
            &root,
            true,
            CaseSensitivity::Sensitive
        ));
        assert!(is_within_with_case(
            &root,
            &root.join("child"),
            false,
            CaseSensitivity::Sensitive
        ));
        assert!(!is_within_with_case(
            &root,
            &root.join("../outside"),
            false,
            CaseSensitivity::Sensitive
        ));
        assert!(!is_within_with_case(
            &root,
            &root.with_file_name("root").join("child"),
            false,
            CaseSensitivity::Sensitive
        ));
        assert!(is_within_with_case(
            &root,
            &root.with_file_name("root").join("child"),
            false,
            CaseSensitivity::Insensitive
        ));
    }
}
