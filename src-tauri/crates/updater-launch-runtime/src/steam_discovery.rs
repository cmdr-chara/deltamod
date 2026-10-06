// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
//! Bounded Steam configuration parsing, independent of a GUI or the host OS.
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

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

/// The same numeric domain used by the native Steam URI launcher.
/// Do not accept empty strings, signed numbers, URI fragments or u32 overflow.
pub fn steam_app_id(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|id| *id != 0)
}

/// Steam's install directory is one child of steamapps/common, not a path.
/// Apply the Windows restrictions on every host so metadata stays portable.
pub fn valid_steam_install_dir(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 255
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return false;
    }
    let device = value
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let numbered_device = device.len() == 4
        && (device.starts_with("COM") || device.starts_with("LPT"))
        && matches!(device.as_bytes()[3], b'1'..=b'9');
    !(matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered_device)
}

/// Resolve only a unique AppState's own appid/installdir fields. A malformed
/// manifest, duplicate identity, or another game's manifest is not a fallback.
pub fn steam_manifest_install_dir(input: &str, expected_app_id: u32) -> Option<String> {
    if expected_app_id == 0 {
        return None;
    }
    let root = object(&mut tokenize(input)?.into_iter(), 0, false)?;
    let mut states = root
        .into_iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case("AppState"));
    let (_, Value::Object(fields)) = states.next()? else {
        return None;
    };
    if states.next().is_some() {
        return None;
    }
    let mut app_id = None;
    let mut install_dir = None;
    for (key, value) in fields {
        if key.eq_ignore_ascii_case("appid") {
            let Value::Text(value) = value else {
                return None;
            };
            if app_id.replace(steam_app_id(&value)?).is_some() {
                return None;
            }
        } else if key.eq_ignore_ascii_case("installdir") {
            let Value::Text(value) = value else {
                return None;
            };
            if !valid_steam_install_dir(&value) || install_dir.replace(value).is_some() {
                return None;
            }
        }
    }
    if app_id != Some(expected_app_id) {
        return None;
    }
    install_dir
}

fn linked(metadata: &fs::Metadata) -> bool {
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

fn read_steam_manifest(path: &Path) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || linked(&metadata) || metadata.len() > MAX_VDF_BYTES as u64 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let file = fs::File::open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || linked(&opened) || opened.len() > MAX_VDF_BYTES as u64 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut input = String::new();
    file.take(MAX_VDF_BYTES as u64 + 1)
        .read_to_string(&mut input)?;
    if input.len() > MAX_VDF_BYTES {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(input)
}

/// Read-only discovery. Publication still belongs to the profile/lifecycle
/// runtime. Missing libraries are skipped; available aliases are canonicalized.
/// A present appmanifest is authoritative, including when it is invalid.
/// Only a missing manifest permits the packaged catalogue's legacy folder name.
pub fn steam_installation_candidates(
    common_folders: impl IntoIterator<Item = PathBuf>,
    app_id: u32,
    catalogue_folder: &str,
) -> Vec<PathBuf> {
    if app_id == 0 || !valid_steam_install_dir(catalogue_folder) {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut visited = std::collections::BTreeSet::new();
    for common in common_folders.into_iter().take(MAX_LIBRARIES) {
        let Ok(common) = fs::canonicalize(common) else {
            continue;
        };
        if !common.is_dir() || !visited.insert(common.clone()) {
            continue;
        }
        let Some(steamapps) = common.parent() else {
            continue;
        };
        let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));
        let folder = match read_steam_manifest(&manifest) {
            Ok(input) => match steam_manifest_install_dir(&input, app_id) {
                Some(folder) => folder,
                None => continue,
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => catalogue_folder.to_owned(),
            Err(_) => continue,
        };
        let candidate = common.join(folder);
        let Ok(metadata) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if !metadata.is_dir() || linked(&metadata) {
            continue;
        }
        let Ok(candidate) = fs::canonicalize(candidate) else {
            continue;
        };
        if candidate.starts_with(&common) && !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
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
        let home = std::env::temp_dir().join("deltamod-steam-home");
        let xdg = std::env::temp_dir().join("deltamod-steam-data");
        let home = home.as_path();
        let roots = linux_steam_roots(home, Some(&xdg));
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

    #[test]
    fn app_ids_match_the_launchers_numeric_domain() {
        assert_eq!(steam_app_id("391540"), Some(391540));
        assert_eq!(steam_app_id("000391540"), Some(391540));
        assert_eq!(steam_app_id("4294967295"), Some(u32::MAX));
        for value in [
            "",
            "0",
            "+1",
            "-1",
            " 1",
            "1 ",
            "4294967296",
            "1/args",
            "１２３",
        ] {
            assert_eq!(steam_app_id(value), None, "{value}");
        }
    }

    #[test]
    fn manifest_directory_is_bound_to_its_own_app_identity() {
        let input = r#""AppState" {
            "appid" "391540" "installdir" "Renamed Game"
            "UserConfig" { "installdir" "Ignored" }
        }"#;
        assert_eq!(
            steam_manifest_install_dir(input, 391540).as_deref(),
            Some("Renamed Game")
        );
        assert_eq!(steam_manifest_install_dir(input, 391541), None);
        assert_eq!(steam_manifest_install_dir(input, 0), None);
        assert_eq!(
            steam_manifest_install_dir(
                r#""appstate" { "AppId" "391540" "InstallDir" "Game" }"#,
                391540
            )
            .as_deref(),
            Some("Game")
        );
    }

    #[test]
    fn manifests_reject_ambiguous_incomplete_and_oversized_identity() {
        for input in [
            r#""AppState" { "appid" "391540" }"#,
            r#""AppState" { "installdir" "Game" }"#,
            r#""AppState" { "appid" "391540" "APPID" "391540" "installdir" "Game" }"#,
            r#""AppState" { "appid" "391540" "installdir" "Game" "INSTALLDIR" "Other" }"#,
            r#""AppState" { "appid" "391540" "installdir" "Game" } "AppState" {}"#,
            r#""AppState" { "appid" {} "installdir" "Game" }"#,
            r#""AppState" { "appid" "391540" "installdir" {} }"#,
            r#""AppState" { "appid" "0" "installdir" "Game" }"#,
            r#""AppState" { "appid" "391540" "installdir" "Game" "#,
            r#""AppState" { "appid" "391540" "installdir" "Game" } }"#,
        ] {
            assert!(
                steam_manifest_install_dir(input, 391540).is_none(),
                "{input}"
            );
        }
        let oversized = format!(
            "{}\"AppState\" {{ \"appid\" \"391540\" \"installdir\" \"Game\" }}",
            " ".repeat(MAX_VDF_BYTES)
        );
        assert!(steam_manifest_install_dir(&oversized, 391540).is_none());
    }

    #[test]
    fn install_names_cannot_be_paths_devices_or_windows_aliases() {
        for value in [
            "",
            ".",
            "..",
            "../Game",
            "Game/child",
            r"Game\child",
            "/Game",
            r"C:\Game",
            "C:Game",
            "Game:stream",
            "Game.",
            "Game ",
            "Game\n",
            "Game\0",
            "a?b",
            "a*b",
            "CON",
            "con.txt",
            "NUL",
            "AUX",
            "PRN",
            "COM1",
            "LPT9.txt",
        ] {
            assert!(!valid_steam_install_dir(value), "{value:?}");
        }
        assert!(!valid_steam_install_dir(&"x".repeat(256)));
        for value in [
            "UNDERTALE",
            "Renamed Game",
            "Deltarune 日本語",
            "Game.v2",
            "COM10",
        ] {
            assert!(valid_steam_install_dir(value), "{value:?}");
        }
        for value in ["../Game", "Game:stream", "CON"] {
            let input =
                format!("\"AppState\" {{ \"appid\" \"391540\" \"installdir\" \"{value}\" }}");
            assert!(steam_manifest_install_dir(&input, 391540).is_none());
        }
    }

    struct DiscoveryFixture(PathBuf);
    impl DiscoveryFixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "deltamod-steam-discovery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn library(&self, name: &str) -> PathBuf {
            let common = self.0.join(name).join("steamapps/common");
            fs::create_dir_all(&common).unwrap();
            common
        }
        fn install(common: &Path, name: &str) -> PathBuf {
            let game = common.join(name);
            fs::create_dir(&game).unwrap();
            fs::canonicalize(game).unwrap()
        }
        fn manifest(common: &Path, contents: &str) {
            fs::write(
                common.parent().unwrap().join("appmanifest_391540.acf"),
                contents,
            )
            .unwrap();
        }
    }
    impl Drop for DiscoveryFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn discovery_uses_manifest_names_and_skips_missing_libraries_and_aliases() {
        let fixture = DiscoveryFixture::new();
        let common = fixture.library("Second drive");
        let game = DiscoveryFixture::install(&common, "Renamed Game");
        DiscoveryFixture::manifest(
            &common,
            r#""AppState" { "appid" "391540" "installdir" "Renamed Game" }"#,
        );
        assert_eq!(
            steam_installation_candidates(
                [fixture.0.join("offline"), common.clone(), common.join(".")],
                391540,
                "UNDERTALE"
            ),
            vec![game]
        );
    }

    #[test]
    fn only_an_absent_manifest_allows_catalogue_folder_fallback() {
        let fixture = DiscoveryFixture::new();
        let common = fixture.library("Steam");
        let game = DiscoveryFixture::install(&common, "UNDERTALE");
        assert_eq!(
            steam_installation_candidates([common.clone()], 391540, "UNDERTALE"),
            vec![game]
        );
        for input in [
            "not vdf",
            r#""AppState" { "appid" "7" "installdir" "UNDERTALE" }"#,
            r#""AppState" { "appid" "391540" "installdir" "Missing" }"#,
        ] {
            DiscoveryFixture::manifest(&common, input);
            assert!(
                steam_installation_candidates([common.clone()], 391540, "UNDERTALE").is_empty()
            );
        }
        DiscoveryFixture::manifest(&common, &" ".repeat(MAX_VDF_BYTES + 1));
        assert!(steam_installation_candidates([common], 391540, "UNDERTALE").is_empty());
    }

    #[test]
    fn unavailable_or_invalid_earlier_libraries_do_not_hide_later_candidates() {
        let fixture = DiscoveryFixture::new();
        let first = fixture.library("first");
        DiscoveryFixture::install(&first, "UNDERTALE");
        DiscoveryFixture::manifest(&first, "invalid");
        let second = fixture.library("second");
        let expected = DiscoveryFixture::install(&second, "UNDERTALE");
        assert_eq!(
            steam_installation_candidates([first, second], 391540, "UNDERTALE"),
            vec![expected]
        );
    }

    #[test]
    fn discovery_rejects_invalid_identity_before_reading_roots() {
        let fixture = DiscoveryFixture::new();
        let common = fixture.library("Steam");
        DiscoveryFixture::install(&common, "UNDERTALE");
        assert!(steam_installation_candidates([common.clone()], 0, "UNDERTALE").is_empty());
        assert!(steam_installation_candidates([common], 391540, "../UNDERTALE").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn linked_manifests_and_game_directories_are_not_automatic_import_sources() {
        use std::os::unix::fs::symlink;
        let fixture = DiscoveryFixture::new();
        let common = fixture.library("Steam");
        DiscoveryFixture::install(&common, "UNDERTALE");
        let outside = fixture.0.join("outside.acf");
        fs::write(
            &outside,
            r#""AppState" { "appid" "391540" "installdir" "UNDERTALE" }"#,
        )
        .unwrap();
        let manifest = common.parent().unwrap().join("appmanifest_391540.acf");
        symlink(outside, &manifest).unwrap();
        assert!(steam_installation_candidates([common.clone()], 391540, "UNDERTALE").is_empty());
        fs::remove_file(manifest).unwrap();
        let outside = fixture.0.join("external-game");
        fs::create_dir(&outside).unwrap();
        symlink(outside, common.join("Linked")).unwrap();
        assert!(steam_installation_candidates([common], 391540, "Linked").is_empty());
    }
}
