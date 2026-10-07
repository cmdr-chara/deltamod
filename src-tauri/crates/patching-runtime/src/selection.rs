// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

use super::{
    parse_patch_type, read_mod_name, require_directory, validate_selection, Error, Runtime,
    MAX_METADATA_BYTES,
};
use deltamod_native_core::{
    patch_plan::{PatchCandidate, MAX_PATCHES},
    path_security::validate_relative_path,
};
use deltamod_tools_runtime::{read_relative_regular_file, SecurePathError};
use roxmltree::{Document, ParsingOptions};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

pub(super) const MAX_STORE_ENTRIES: usize = 10_000;
const MAX_VARIANT_BYTES: u64 = 4_096;
const MAX_MANIFEST_NODES: u32 = 50_000;

#[derive(Deserialize)]
struct Identity {
    #[serde(rename = "uniqueId")]
    unique_id: String,
}

/// Resolve every requested identity exactly once, preserving caller load order.
/// An unreadable unrelated packet must not prevent a healthy packet from loading.
pub(super) fn resolve(
    runtime: &Runtime,
    selected: &[String],
) -> Result<Vec<(String, PathBuf)>, Error> {
    validate_selection(selected)?;
    if selected.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = available_roots(runtime, selected)?;
    let wanted_count = selected.iter().collect::<HashSet<_>>().len();
    if found.len() != wanted_count || found.values().any(Option::is_none) {
        return Err(Error::SelectionUnavailable);
    }
    let mut ordered = Vec::with_capacity(wanted_count);
    for id in selected {
        if let Some(Some(root)) = found.remove(id) {
            ordered.push((id.clone(), root));
        }
    }
    Ok(ordered)
}

/// Bounded catalogue lookup for hash-only callers, who may include identities
/// without stored packets. Ambiguous identities cannot supply patch metadata.
pub(super) fn available_roots(
    runtime: &Runtime,
    selected: &[String],
) -> Result<HashMap<String, Option<PathBuf>>, Error> {
    require_directory(&runtime.mod_root, Error::ModStoreUnavailable)?;
    let wanted = selected.iter().map(String::as_str).collect::<HashSet<_>>();
    let mut found = HashMap::new();
    for (index, entry) in fs::read_dir(&runtime.mod_root)?.enumerate() {
        if index >= MAX_STORE_ENTRIES {
            return Err(Error::ModStoreUnavailable);
        }
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let root = entry.path();
        let Some(identity) =
            read_relative_regular_file(&root, Path::new("__deltaID.json"), MAX_METADATA_BYTES)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Identity>(&bytes).ok())
        else {
            continue;
        };
        if wanted.contains(identity.unique_id.as_str()) {
            found
                .entry(identity.unique_id)
                .and_modify(|root| *root = None)
                .or_insert(Some(root));
        }
    }
    Ok(found)
}

fn relative_name(value: &str) -> Result<String, Error> {
    let path = validate_relative_path(value).map_err(|_| Error::InvalidTarget)?;
    if path == Path::new(".") {
        return Err(Error::InvalidTarget);
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

/// One bounded parse drives both capability reporting and execution. Neither path
/// reads patch bodies or launches an external tool during manifest parsing.
pub(super) fn append_packet(
    runtime: &Runtime,
    id: &str,
    root: &Path,
    candidates: &mut Vec<PatchCandidate>,
) -> Result<(), Error> {
    let name = read_mod_name(root).unwrap_or_else(|| "Selected mod".into());
    let variant = match read_relative_regular_file(root, Path::new("__variant"), MAX_VARIANT_BYTES)
    {
        Ok(bytes) => {
            let text =
                std::str::from_utf8(&bytes).map_err(|_| Error::InvalidManifest(name.clone()))?;
            // A trailing newline is compatible with legacy markers. Leading
            // whitespace is not silently converted to a different variant.
            if text.trim_start() != text {
                return Err(Error::InvalidManifest(name));
            }
            relative_name(text.trim_end())?
        }
        Err(SecurePathError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            "modding.xml".into()
        }
        Err(_) => return Err(Error::InvalidManifest(name)),
    };
    let bytes = read_relative_regular_file(root, Path::new(&variant), MAX_METADATA_BYTES).map_err(
        |error| match error {
            SecurePathError::Io(error) if error.kind() == io::ErrorKind::NotFound => {
                Error::MissingManifest(name.clone())
            }
            _ => Error::InvalidManifest(name.clone()),
        },
    )?;
    let xml = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidManifest(name.clone()))?;
    let options = || ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_MANIFEST_NODES,
        ..ParsingOptions::default()
    };
    let wrapped;
    let document = match Document::parse_with_options(xml, options()) {
        Ok(document) => document,
        Err(_) => {
            wrapped = format!("<deltamod>{xml}</deltamod>");
            Document::parse_with_options(&wrapped, options())
                .map_err(|_| Error::InvalidManifest(name.clone()))?
        }
    };
    let start = candidates.len();
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("patch"))
    {
        if candidates.len() >= MAX_PATCHES {
            return Err(Error::InvalidManifest(name));
        }
        let kind = node.attribute("type").unwrap_or("").to_ascii_lowercase();
        let patch_type = parse_patch_type(&kind).ok_or_else(|| Error::UnsupportedPatch {
            mod_name: name.clone(),
            patch_type: kind,
        })?;
        let patch = relative_name(node.attribute("patch").unwrap_or(""))?;
        let to = relative_name(node.attribute("to").unwrap_or(""))?;
        let mapped_target = relative_name(&runtime.definition.map_patch_target(&to)?)?;
        candidates.push(PatchCandidate {
            patch_type,
            patch,
            to,
            mapped_target,
            mod_name: name.clone(),
            mod_id: id.to_owned(),
            mod_root: root.to_owned(),
        });
    }
    // Empty selection is a deliberate vanilla launch. A selected packet with no
    // instructions is not a successfully applied mod.
    if candidates.len() == start {
        return Err(Error::InvalidManifest(name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PatchPlatform, PlatformDefinition, StagingErrorCode};

    fn fixture() -> (tempfile::TempDir, Runtime) {
        let temp = tempfile::tempdir().unwrap();
        let runtime = Runtime {
            game_root: temp.path().join("game"),
            mod_root: temp.path().join("mods"),
            tools_root: temp.path().join("absent-tools"),
            hash_cache_path: temp.path().join("hashes.json"),
            reference_root: None,
            platform: PatchPlatform::Linux,
            platform_name: "linux".into(),
            arch: "x64".into(),
            definition: PlatformDefinition {
                data_files: vec!["data.win".into()],
                patch_layout: "windows-root".into(),
                content_root: None,
            },
        };
        fs::create_dir(&runtime.game_root).unwrap();
        fs::create_dir(&runtime.mod_root).unwrap();
        (temp, runtime)
    }
    fn packet(runtime: &Runtime, folder: &str, id: &str, to: &str) -> PathBuf {
        let root = runtime.mod_root.join(folder);
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("__deltaID.json"),
            format!(r#"{{"uniqueId":"{id}"}}"#),
        )
        .unwrap();
        fs::write(root.join("meta.toml"), "[metadata]\nname='Test'").unwrap();
        fs::write(root.join("patch.bin"), b"patched").unwrap();
        fs::write(
            root.join("modding.xml"),
            format!(r#"<patch type="override" patch="patch.bin" to="{to}"/>"#),
        )
        .unwrap();
        root
    }

    #[test]
    fn missing_selected_id_is_not_a_vanilla_success() {
        let (_temp, runtime) = fixture();
        packet(&runtime, "one", "one", "one.bin");
        assert!(runtime.build_plan(&["missing".into()]).is_err());
        assert!(runtime
            .build_plan(&["one".into(), "missing".into()])
            .is_err());
        assert!(runtime
            .check_selected_legacy_mods(&["missing".into()])
            .is_err());
        assert!(runtime
            .stage_patch_outputs(&["missing".into()], "missing", |_| {}, || false)
            .is_err());
        assert!(runtime.build_plan(&[]).is_ok());
    }
    #[test]
    fn duplicated_store_identity_is_ambiguous_even_with_disjoint_targets() {
        let (_temp, runtime) = fixture();
        packet(&runtime, "one", "same", "one.bin");
        packet(&runtime, "two", "same", "two.bin");
        assert!(runtime.build_plan(&["same".into()]).is_err());
    }
    #[test]
    fn selected_order_is_preserved_and_duplicate_delivery_does_not_duplicate_patches() {
        let (_temp, runtime) = fixture();
        packet(&runtime, "one", "one", "one.bin");
        packet(&runtime, "two", "two", "two.bin");
        for selected in [["one", "two"], ["two", "one"]] {
            let plan = runtime
                .build_plan(&[selected[0].into(), selected[1].into(), selected[0].into()])
                .unwrap();
            assert_eq!(
                plan.patches
                    .iter()
                    .map(|patch| patch.candidate.mod_id.as_str())
                    .collect::<Vec<_>>(),
                selected
            );
        }
    }
    #[test]
    fn unrelated_incomplete_packet_does_not_break_selected_mod() {
        let (_temp, runtime) = fixture();
        packet(&runtime, "one", "one", "one.bin");
        fs::create_dir(runtime.mod_root.join("incomplete")).unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_ok());
        fs::write(
            runtime.mod_root.join("incomplete/__deltaID.json"),
            "not json",
        )
        .unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_ok());
        assert!(runtime.mark_selected_patched(&["one".into()]).is_ok());
    }
    #[test]
    fn selected_empty_manifest_is_not_a_successful_patch() {
        let (_temp, runtime) = fixture();
        let root = packet(&runtime, "one", "one", "one.bin");
        fs::write(root.join("modding.xml"), "<mod/>").unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_err());
    }
    #[test]
    fn windows_archive_separators_resolve_to_real_posix_sources() {
        let (_temp, runtime) = fixture();
        let root = packet(&runtime, "one", "one", "one.bin");
        fs::create_dir(root.join("files")).unwrap();
        fs::rename(root.join("patch.bin"), root.join("files/patch.bin")).unwrap();
        fs::write(
            root.join("modding.xml"),
            r#"<patch type="override" patch="files\patch.bin" to="one.bin"/>"#,
        )
        .unwrap();
        let plan = runtime.build_plan(&["one".into()]).unwrap();
        assert_eq!(plan.patches[0].source, root.join("files/patch.bin"));
    }
    #[test]
    fn malformed_oversized_and_duplicate_metadata_fail_closed() {
        let (_temp, runtime) = fixture();
        let root = packet(&runtime, "one", "one", "one.bin");
        fs::write(
            root.join("__deltaID.json"),
            r#"{"uniqueId":"wrong","uniqueId":"one"}"#,
        )
        .unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_err());
        fs::write(root.join("__deltaID.json"), r#"{"uniqueId":"one"}"#).unwrap();
        for xml in [
            " ".repeat(MAX_METADATA_BYTES as usize + 1),
            "<mod/>".into(),
            "<!DOCTYPE root [<!ENTITY file SYSTEM 'file:///not-read'>]><root>&file;</root>".into(),
            format!(
                "<root>{}</root>",
                "<node/>".repeat(MAX_MANIFEST_NODES as usize)
            ),
            format!(
                "<root>{}</root>",
                r#"<patch type="copy" patch="patch.bin" to="one.bin"/>"#.repeat(MAX_PATCHES + 1)
            ),
        ] {
            fs::write(root.join("modding.xml"), xml).unwrap();
            assert!(runtime.build_plan(&["one".into()]).is_err());
        }
    }
    #[test]
    fn capability_and_staging_share_one_parser_without_touching_tools() {
        let (_temp, runtime) = fixture();
        let root = packet(&runtime, "one", "one", "one.bin");
        assert!(runtime.packet_staging_readiness("one").is_ok());
        // This fixture packages no tools: G3MTool mods report the missing tool,
        // UndertaleModCli scripts stay unsupported. Neither reads the patch.
        for (kind, code) in [
            ("xdelta", StagingErrorCode::ToolUnavailable),
            ("g3mpatch", StagingErrorCode::ToolUnavailable),
            ("csx", StagingErrorCode::SandboxUnavailable),
        ] {
            fs::write(
                root.join("modding.xml"),
                format!(r#"<patch type="{kind}" patch="absent.csx" to="one.bin"/>"#),
            )
            .unwrap();
            assert_eq!(
                runtime.packet_staging_readiness("one").unwrap_err().code(),
                code
            );
            assert_eq!(
                runtime
                    .stage_patch_outputs(&["one".into()], "check", |_| {}, || false)
                    .unwrap_err()
                    .code(),
                code
            );
        }
        assert!(runtime.packet_staging_readiness("../one").is_err());
        assert!(runtime.packet_staging_readiness("one/subfolder").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_xml_variant_source_and_ancestors_are_rejected() {
        use std::os::unix::fs::symlink;
        let (temp, runtime) = fixture();
        let root = packet(&runtime, "one", "one", "one.bin");
        let xml = fs::read(root.join("modding.xml")).unwrap();
        fs::write(temp.path().join("foreign.xml"), &xml).unwrap();
        fs::remove_file(root.join("modding.xml")).unwrap();
        symlink(temp.path().join("foreign.xml"), root.join("modding.xml")).unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_err());
        fs::remove_file(root.join("modding.xml")).unwrap();
        fs::write(root.join("modding.xml"), &xml).unwrap();
        fs::write(temp.path().join("variant"), "modding.xml").unwrap();
        symlink(temp.path().join("variant"), root.join("__variant")).unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_err());
        fs::remove_file(root.join("__variant")).unwrap();
        symlink(temp.path(), root.join("linked")).unwrap();
        fs::write(root.join("__variant"), "linked/foreign.xml").unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_err());
        fs::remove_file(root.join("__variant")).unwrap();
        fs::remove_file(root.join("patch.bin")).unwrap();
        symlink(temp.path().join("foreign.xml"), root.join("patch.bin")).unwrap();
        assert!(runtime.build_plan(&["one".into()]).is_err());
    }
}
