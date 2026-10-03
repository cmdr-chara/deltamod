//! Manifest-less ("raw") mod packages, as published for Deltahub/G3M on
//! GameBanana: a bare `.xdelta` plus optional music and a README, with the
//! target chapter only described in prose. The importer finds the patch files,
//! guesses each chapter, lets the caller confirm, then writes `meta.toml` and
//! `modding.xml` so the packet looks like any other Deltamod mod.

use crate::ImportError;
use std::fs;
use std::path::{Path, PathBuf};

/// Chapters the DELTARUNE Mac mapping (`chapterN_windows` -> `chapterN_mac`) supports.
pub const MAX_CHAPTER: u8 = 5;
/// `0` targets the root `data.win` (the chapter-select launcher).
pub const ROOT_DATA: u8 = 0;

const PATCH_EXTENSIONS: [&str; 3] = ["xdelta", "vcdiff", "g3mpatch"];
const MUSIC_EXTENSION: &str = "ogg";
const MAX_README_BYTES: u64 = 64 * 1024;
const MAX_RAW_FILES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawPatch {
    /// Path relative to the package root, `/`-separated.
    pub path: String,
    /// Chapter named by the patch's own file name, e.g. `ch3.xdelta`.
    pub chapter_from_name: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawPackage {
    pub patches: Vec<RawPatch>,
    /// `.ogg` files, installed into the game's `mus/` folder under their own names.
    pub music: Vec<String>,
    /// Chapter the README names most often, if exactly one stands out.
    pub chapter_from_readme: Option<u8>,
}

impl RawPackage {
    /// The best guess for one patch: its file name, then the README.
    #[must_use]
    pub fn suggested_chapter(&self, patch: &RawPatch) -> Option<u8> {
        patch.chapter_from_name.or(self.chapter_from_readme)
    }

    /// True when every patch names its own chapter, so no question is needed.
    #[must_use]
    pub fn fully_resolved(&self) -> bool {
        self.patches
            .iter()
            .all(|patch| patch.chapter_from_name.is_some())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawPlan {
    pub package_id: String,
    pub name: String,
    /// One target per entry of `RawPackage::patches`: `ROOT_DATA` or `1..=MAX_CHAPTER`.
    pub chapters: Vec<u8>,
}

/// Returns the raw package at `root`, or `None` when it holds no patch files.
pub fn inspect(root: &Path) -> Result<Option<RawPackage>, ImportError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();

    let mut patches = Vec::new();
    let mut music = Vec::new();
    let mut readme_text = String::new();
    for relative in &files {
        let lower = relative.to_ascii_lowercase();
        let file_name = lower.rsplit('/').next().unwrap_or(&lower);
        let extension = file_name.rsplit_once('.').map(|(_, ext)| ext);
        if extension.is_some_and(|ext| PATCH_EXTENSIONS.contains(&ext)) {
            patches.push(RawPatch {
                path: relative.clone(),
                chapter_from_name: unique_chapter(&chapter_mentions(file_name)),
            });
        } else if extension == Some(MUSIC_EXTENSION) {
            music.push(relative.clone());
        } else if extension == Some("txt") || file_name.starts_with("readme") {
            let path = root.join(relative);
            if fs::metadata(&path)?.len() <= MAX_README_BYTES {
                if let Ok(text) = fs::read_to_string(path) {
                    readme_text.push_str(&text.to_ascii_lowercase());
                    readme_text.push('\n');
                }
            }
        }
    }
    if patches.is_empty() {
        return Ok(None);
    }
    Ok(Some(RawPackage {
        patches,
        music,
        chapter_from_readme: unique_chapter(&chapter_mentions(&readme_text)),
    }))
}

/// Moves the package's files to safe top-level names and writes the manifests.
pub fn write_manifest(
    root: &Path,
    package: &RawPackage,
    plan: &RawPlan,
    max_bytes: u64,
) -> Result<(), ImportError> {
    if plan.chapters.len() != package.patches.len()
        || plan.chapters.iter().any(|chapter| *chapter > MAX_CHAPTER)
    {
        return Err(ImportError::Manifest("raw package plan is invalid"));
    }
    let mut operations = String::new();
    let mut used = Vec::new();
    for (patch, chapter) in package.patches.iter().zip(&plan.chapters) {
        let source = relocate(root, &patch.path, &mut used)?;
        let target = if *chapter == ROOT_DATA {
            "./data.win".to_owned()
        } else {
            format!("./chapter{chapter}_windows/data.win")
        };
        let kind = if source.ends_with(".g3mpatch") {
            "g3mpatch"
        } else {
            "xdelta"
        };
        operations.push_str(&format!(
            "<patch type=\"{kind}\" patch=\"./{}\" to=\"{target}\" />\n",
            xml_escape(&source)
        ));
    }
    for track in &package.music {
        let original = track.rsplit('/').next().unwrap_or(track);
        let source = relocate(root, track, &mut used)?;
        operations.push_str(&format!(
            "<patch type=\"override\" patch=\"./{}\" to=\"./mus/{}\" />\n",
            xml_escape(&source),
            xml_escape(original)
        ));
    }

    let mut metadata = toml::map::Map::new();
    metadata.insert("name".into(), toml::Value::String(plan.name.clone()));
    metadata.insert(
        "packageID".into(),
        toml::Value::String(plan.package_id.clone()),
    );
    metadata.insert("game".into(), toml::Value::String("toby.deltarune".into()));
    metadata.insert("demoMod".into(), toml::Value::Boolean(false));
    metadata.insert(
        "description".into(),
        toml::Value::String("Imported from a Deltahub package.".into()),
    );
    let mut document = toml::map::Map::new();
    document.insert("metadata".into(), toml::Value::Table(metadata));
    let manifest = toml::to_string(&toml::Value::Table(document))
        .map_err(|_| ImportError::Manifest("meta.toml could not be serialized"))?;
    if manifest.len() as u64 > max_bytes || operations.len() as u64 > max_bytes {
        return Err(ImportError::Manifest("raw package manifest is too large"));
    }
    fs::write(root.join("meta.toml"), manifest)?;
    fs::write(root.join("modding.xml"), operations)?;
    Ok(())
}

/// Counts how often each chapter is named, e.g. "chapter3_windows", "Chapter 3", "ch3".
#[must_use]
pub fn chapter_mentions(text: &str) -> [usize; MAX_CHAPTER as usize + 1] {
    let mut counts = [0; MAX_CHAPTER as usize + 1];
    let text = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    for (start, _) in text.match_indices("ch") {
        if start > 0 && bytes[start - 1].is_ascii_alphabetic() {
            continue;
        }
        let mut index = start + 2;
        if text[index..].starts_with("apter") {
            index += 5;
        }
        while index < bytes.len() && matches!(bytes[index], b' ' | b'_' | b'-' | b'.') {
            index += 1;
        }
        let Some(digit) = bytes.get(index).filter(|byte| byte.is_ascii_digit()) else {
            continue;
        };
        if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
            continue;
        }
        let chapter = usize::from(digit - b'0');
        if (1..=usize::from(MAX_CHAPTER)).contains(&chapter) {
            counts[chapter] += 1;
        }
    }
    counts
}

fn unique_chapter(counts: &[usize]) -> Option<u8> {
    let best = *counts.iter().max()?;
    if best == 0 || counts.iter().filter(|count| **count == best).count() != 1 {
        return None;
    }
    counts
        .iter()
        .position(|count| *count == best)
        .and_then(|index| u8::try_from(index).ok())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), ImportError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "__MACOSX" || name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            if out.len() >= MAX_RAW_FILES {
                return Err(ImportError::Manifest("raw package has too many files"));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| ImportError::Manifest("raw package path escaped its root"))?;
            out.push(
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    Ok(())
}

/// Moves `relative` to a root-level name made only of `[A-Za-z0-9._-]`.
fn relocate(root: &Path, relative: &str, used: &mut Vec<String>) -> Result<String, ImportError> {
    let file_name = relative.rsplit('/').next().unwrap_or(relative);
    let mut safe: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.starts_with('.') {
        safe.insert(0, '_');
    }
    let mut candidate = safe.clone();
    let mut counter = 1;
    while used.contains(&candidate.to_ascii_lowercase())
        || (candidate != relative && root.join(&candidate).exists())
    {
        counter += 1;
        candidate = format!("{counter}_{safe}");
    }
    used.push(candidate.to_ascii_lowercase());
    if candidate != relative {
        let from: PathBuf = root.join(relative);
        fs::rename(from, root.join(&candidate))?;
    }
    Ok(candidate)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_mentions_cover_common_readme_wording() {
        let readme = "put the data.win in the chapter3_windows folder. \
                      only patches chapter 3 version v0.0.105";
        assert_eq!(unique_chapter(&chapter_mentions(readme)), Some(3));
        assert_eq!(unique_chapter(&chapter_mentions("mod_ch2.xdelta")), Some(2));
        assert_eq!(unique_chapter(&chapter_mentions("Ch.4 patch")), Some(4));
        // Ties, out-of-range chapters and words that merely contain "ch" are not guesses.
        assert_eq!(unique_chapter(&chapter_mentions("ch1 and ch2")), None);
        assert_eq!(unique_chapter(&chapter_mentions("chapter 12")), None);
        assert_eq!(unique_chapter(&chapter_mentions("touch3 match5")), None);
    }

    #[test]
    fn deltahub_package_becomes_a_deltamod_packet() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        fs::create_dir_all(root.join("custom song (optional)")).unwrap();
        fs::create_dir_all(root.join("save file if you need one")).unwrap();
        fs::write(root.join("kaizo_knight.xdelta"), b"patch").unwrap();
        fs::write(root.join("custom song (optional)/kaizoknight.ogg"), b"ogg").unwrap();
        fs::write(root.join("save file if you need one/filech3_0"), b"save").unwrap();
        fs::write(
            root.join("README.txt"),
            "put the data.win in the chapter3_windows folder",
        )
        .unwrap();

        let package = inspect(root).unwrap().unwrap();
        assert_eq!(package.patches.len(), 1);
        assert_eq!(package.patches[0].chapter_from_name, None);
        assert!(!package.fully_resolved());
        assert_eq!(package.suggested_chapter(&package.patches[0]), Some(3));
        assert_eq!(package.music, ["custom song (optional)/kaizoknight.ogg"]);

        let plan = RawPlan {
            package_id: "gb.662826".into(),
            name: "Kaizo Roaring Knight".into(),
            chapters: vec![3],
        };
        write_manifest(root, &package, &plan, 1024 * 1024).unwrap();
        let modding = fs::read_to_string(root.join("modding.xml")).unwrap();
        assert!(modding.contains(
            r#"<patch type="xdelta" patch="./kaizo_knight.xdelta" to="./chapter3_windows/data.win" />"#
        ));
        assert!(modding.contains(
            r#"<patch type="override" patch="./kaizoknight.ogg" to="./mus/kaizoknight.ogg" />"#
        ));
        assert!(root.join("kaizoknight.ogg").is_file());
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(root.join("meta.toml")).unwrap()).unwrap();
        assert_eq!(
            manifest["metadata"]["packageID"].as_str(),
            Some("gb.662826")
        );
    }

    #[test]
    fn packages_without_patches_are_not_raw_mods() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("song.ogg"), b"ogg").unwrap();
        assert_eq!(inspect(root.path()).unwrap(), None);
    }

    #[test]
    fn invalid_plans_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.xdelta"), b"patch").unwrap();
        let package = inspect(root.path()).unwrap().unwrap();
        for chapters in [vec![], vec![MAX_CHAPTER + 1]] {
            let plan = RawPlan {
                package_id: "gb.1".into(),
                name: "x".into(),
                chapters,
            };
            assert!(write_manifest(root.path(), &package, &plan, 1024).is_err());
        }
    }
}
