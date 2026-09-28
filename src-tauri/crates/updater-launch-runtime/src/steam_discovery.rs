// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
//! Bounded Steam configuration parsing, independent of a GUI or the host OS.
use std::path::{Path, PathBuf};

pub const MAX_VDF_BYTES: usize = 1024 * 1024;
const MAX_TOKENS: usize = 16_384;
const MAX_DEPTH: usize = 16;
const MAX_LIBRARIES: usize = 128;

#[derive(Debug)]
enum Token {
    Text(String),
    Open,
    Close,
}
#[derive(Debug)]
enum Value {
    Text(String),
    Object(Vec<(String, Value)>),
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    if input.len() > MAX_VDF_BYTES {
        return None;
    }
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();
    while let Some(c) = chars.next() {
        let token = match c {
            c if c.is_ascii_whitespace() => continue,
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            '{' => Token::Open,
            '}' => Token::Close,
            '"' => {
                let mut text = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => {
                            let next = chars.next()?;
                            if next.is_control() {
                                return None;
                            }
                            if next != '\\' && next != '"' {
                                text.push('\\');
                            }
                            text.push(next);
                        }
                        c if c.is_control() => return None,
                        c => text.push(c),
                    }
                }
                Token::Text(text)
            }
            _ => return None,
        };
        if tokens.len() == MAX_TOKENS {
            return None;
        }
        tokens.push(token);
    }
    Some(tokens)
}

fn object(
    tokens: &mut impl Iterator<Item = Token>,
    depth: usize,
    nested: bool,
) -> Option<Vec<(String, Value)>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut pairs = Vec::new();
    loop {
        let key = match tokens.next() {
            None if !nested => return Some(pairs),
            Some(Token::Close) if nested => return Some(pairs),
            Some(Token::Text(key)) => key,
            _ => return None,
        };
        let value = match tokens.next()? {
            Token::Text(value) => Value::Text(value),
            Token::Open => Value::Object(object(tokens, depth + 1, true)?),
            Token::Close => return None,
        };
        pairs.push((key, value));
    }
}

fn absolute_library(value: &str) -> bool {
    let bytes = value.as_bytes();
    let absolute = value.starts_with('/')
        || value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'));
    absolute
        && !value.chars().any(char::is_control)
        && !value
            .split(['/', '\\'])
            .any(|part| part == ".." || part == ".")
}

/// Read only numeric entries under LibraryFolders, never arbitrary `path` fields.
/// Malformed, ambiguous or oversized documents fail closed as no discovered roots.
pub fn steam_library_roots(input: &str) -> Vec<PathBuf> {
    let parse = || -> Option<Vec<PathBuf>> {
        let root = object(&mut tokenize(input)?.into_iter(), 0, false)?;
        let mut folders = root
            .into_iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("libraryfolders"));
        let (_, Value::Object(entries)) = folders.next()? else {
            return None;
        };
        if folders.next().is_some() {
            return None;
        }
        let mut roots = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        for (key, value) in entries {
            if key.is_empty() || !key.bytes().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if !ids.insert(key) || ids.len() > MAX_LIBRARIES {
                return None;
            }
            let path = match value {
                Value::Text(path) => path,
                Value::Object(fields) => {
                    let mut paths = fields
                        .into_iter()
                        .filter(|(key, _)| key.eq_ignore_ascii_case("path"));
                    let Some((_, Value::Text(path))) = paths.next() else {
                        continue;
                    };
                    if paths.next().is_some() {
                        return None;
                    }
                    path
                }
            };
            if absolute_library(&path) {
                let path = PathBuf::from(path);
                if !roots.contains(&path) {
                    roots.push(path);
                }
            }
        }
        Some(roots)
    };
    parse().unwrap_or_default()
}

pub fn linux_steam_roots(home: &Path, xdg_data_home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(xdg) = xdg_data_home.filter(|path| path.is_absolute()) {
        roots.push(xdg.join("Steam"));
    }
    if home.is_absolute() {
        for relative in [
            ".local/share/Steam",
            ".steam/steam",
            ".steam/root",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ] {
            let path = home.join(relative);
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_and_legacy_libraries_are_supported() {
        let input = r#"// library locations
            "libraryfolders" { "0" { "path" "C:\\Steam" "apps" { "391540" "10" } }
            "1" "/mnt/games" "2" { "path" "/mnt/games" } }"#;
        assert_eq!(
            steam_library_roots(input),
            vec![PathBuf::from(r"C:\Steam"), PathBuf::from("/mnt/games")]
        );
    }
    #[test]
    fn unrelated_fields_relative_paths_and_traversal_are_not_libraries() {
        let input = r#""path" "/outside" "LibraryFolders" {
            "0" { "label" "path" "apps" { "path" "/nested" } }
            "1" "relative" "2" "/root/../outside" "3" "C:relative" "4" { "path" "/valid" } }"#;
        assert_eq!(steam_library_roots(input), vec![PathBuf::from("/valid")]);
    }
    #[test]
    fn malformed_ambiguous_and_oversized_documents_fail_closed() {
        for input in [
            r#""libraryfolders" { "0" "/valid""#,
            r#""libraryfolders" { "0" "/valid" } }"#,
            r#""libraryfolders" { "0" "/valid" "0" "/other" }"#,
            r#""libraryfolders" { "0" { "path" "/one" "PATH" "/two" } }"#,
            r#""libraryfolders" { "0" "/valid" } "libraryfolders" {}"#,
        ] {
            assert!(steam_library_roots(input).is_empty(), "{input}");
        }
        assert!(steam_library_roots(&" ".repeat(MAX_VDF_BYTES + 1)).is_empty());
        assert!(
            steam_library_roots(&format!("{}{}", "\"x\" {".repeat(32), "}".repeat(32))).is_empty()
        );
        let entries = (0..=MAX_LIBRARIES)
            .map(|id| format!("\"{id}\" \"/library/{id}\" "))
            .collect::<String>();
        assert!(steam_library_roots(&format!("\"libraryfolders\" {{ {entries} }}")).is_empty());
    }
    #[test]
    fn linux_roots_cover_native_flatpak_and_absolute_xdg() {
        let home = Path::new("/home/test");
        let roots = linux_steam_roots(home, Some(Path::new("/data")));
        assert_eq!(roots.len(), 5);
        assert!(roots.contains(&home.join(".steam/root")));
        assert!(roots.contains(&home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")));
        assert_eq!(
            linux_steam_roots(home, Some(Path::new("relative"))).len(),
            4
        );
    }
    #[test]
    fn generated_inputs_never_escape_parser_bounds() {
        let mut seed = 0x5a17_u64;
        for _ in 0..10_000 {
            let mut text = String::new();
            for _ in 0..96 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push(b"\"{}\\/abc012 \n"[(seed >> 32) as usize % 13] as char);
            }
            for path in steam_library_roots(&text) {
                assert!(absolute_library(path.to_str().unwrap()));
            }
        }
    }
}
