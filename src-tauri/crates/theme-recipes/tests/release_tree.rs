use deltamod_theme_recipes::RecipeId;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

const REQUESTED_ASSET_NAMES: [&str; 12] = [
    "card_castle.ogg",
    "noelle.ogg",
    "tv_world.ogg",
    "knight.ogg",
    "mus_ruins.ogg",
    "mus_snowy.ogg",
    "mus_waterfall.ogg",
    "mus_st_him.ogg",
    "mus_anothermedium.ogg",
    "mus_core.ogg",
    "mus_hereweare.ogg",
    "mus_endarea_parta.ogg",
];
const SOURCE_ASSET_IDS: [&str; 12] = [
    "card_castle",
    "noelle",
    "tv_world",
    "knight",
    "mus_ruins",
    "mus_snowy",
    "mus_waterfall",
    "mus_st_him",
    "mus_anothermedium",
    "mus_core",
    "mus_hereweare",
    "mus_endarea_parta",
];
const BUNDLED_THEME_IDS: [&str; 12] = [
    "card-castle",
    "noelle",
    "tv-world",
    "the-knight",
    "undertale-ruins",
    "undertale-snowdin",
    "undertale-waterfall",
    "undertale-void",
    "undertale-hotland",
    "undertale-core",
    "undertale-true-lab",
    "undertale-new-home",
];
const RELEASE_ROOTS: [&str; 8] = [
    "web/themes",
    "customThemes",
    "resources",
    "release",
    "dist",
    "src-tauri/resources",
    "updater-release",
    "release-artifacts",
];
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum FindingKind {
    ExtractedAsset,
    UnsafeLinkOrReparse,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Finding {
    kind: FindingKind,
    path: PathBuf,
}

impl Finding {
    fn extracted_asset(path: &Path) -> Self {
        Self {
            kind: FindingKind::ExtractedAsset,
            path: path.to_path_buf(),
        }
    }

    fn unsafe_link_or_reparse(path: &Path) -> Self {
        Self {
            kind: FindingKind::UnsafeLinkOrReparse,
            path: path.to_path_buf(),
        }
    }
}

#[test]
fn release_tree_contains_only_reviewed_bundled_theme_assets() {
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crate_root = manifest_root
        .ancestors()
        .find(|candidate| {
            path_file_name_is(candidate, "theme-recipes")
                && candidate
                    .parent()
                    .is_some_and(|parent| path_file_name_is(parent, "crates"))
                && candidate
                    .parent()
                    .and_then(Path::parent)
                    .is_some_and(|parent| path_file_name_is(parent, "src-tauri"))
        })
        .expect("locate the theme-recipes crate from the active manifest");
    let repository_root = crate_root
        .ancestors()
        .nth(3)
        .expect("theme-recipes is nested below the repository root");
    let mut findings = Vec::new();

    scan_crate_for_media(crate_root, &mut findings);
    findings.extend(scan_repository_release_roots(repository_root));
    sort_and_deduplicate(&mut findings);

    assert!(
        findings.is_empty(),
        "unreviewed game assets or unsafe filesystem entries must never enter crate or release trees: {findings:#?}"
    );
}

#[test]
fn root_custom_themes_source_id_asset_is_reported_without_asset_bytes() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let generated_asset = repository
        .path()
        .join("customThemes")
        .join("card_castle")
        .join("opaque.bin");
    create_empty_marker(&generated_asset);

    assert_eq!(
        scan_repository_release_roots(repository.path()),
        vec![Finding::extracted_asset(&generated_asset)]
    );
}

#[test]
fn direct_custom_theme_closed_recipe_output_directories_are_rejected() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let mut expected = Vec::new();

    for recipe_id in RecipeId::ALL {
        let marker = repository
            .path()
            .join("customThemes")
            .join(recipe_id.as_str())
            .join("opaque.bin");
        create_empty_marker(&marker);
        expected.push(Finding::extracted_asset(&marker));
    }
    for relative in [
        "customThemes/card-castle/theme.json",
        "customThemes/noelle/background.png",
        "customThemes/tv-world/music.ogg",
        "customThemes/the-knight/music.wav",
        "customThemes/undertale-new-home/provenance.json",
    ] {
        let marker = fixture_path(repository.path(), relative);
        create_empty_marker(&marker);
        expected.push(Finding::extracted_asset(&marker));
    }
    sort_and_deduplicate(&mut expected);

    assert_eq!(scan_repository_release_roots(repository.path()), expected);
}

#[test]
fn production_sources_do_not_embed_user_machine_installation_roots() {
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_root = manifest_root.parent().expect("workspace crates directory");
    let source_roots = [
        manifest_root.join("src"),
        crates_root.join("theme-recipes-runtime").join("src"),
    ];
    let forbidden_roots = [
        r"E:\SteamLibrary\steamapps\common\Undertale",
        "E:/SteamLibrary/steamapps/common/Undertale",
        r"G:\DELTARUNE",
        "G:/DELTARUNE",
    ];
    let mut findings = Vec::new();
    for root in source_roots {
        scan_source_text_no_follow(&root, &forbidden_roots, &mut findings);
    }
    sort_paths_and_deduplicate(&mut findings);
    assert!(
        findings.is_empty(),
        "production Rust sources must not embed user-machine installation roots; tests and policy documentation are intentionally outside the scan: {findings:#?}"
    );

    let fixture = tempfile::tempdir().expect("synthetic source tree");
    let source = fixture_path(fixture.path(), "src/paths.rs");
    create_text_placeholder(
        &source,
        r#"const ROOTS: [&str; 4] = ["e:/STEAMLIBRARY/steamapps/common/UNDERTALE", r"E:\SteamLibrary\steamapps\common\Undertale", "g:/deltarune", r"G:\DELTARUNE"];"#,
    );
    create_text_placeholder(
        &fixture_path(fixture.path(), "docs/policy.md"),
        r"Never store E:\SteamLibrary\steamapps\common\Undertale or G:\DELTARUNE.",
    );
    let mut fixture_findings = Vec::new();
    scan_source_text_no_follow(
        &fixture_path(fixture.path(), "src"),
        &forbidden_roots,
        &mut fixture_findings,
    );
    assert_eq!(fixture_findings, vec![source]);
}

#[test]
fn source_id_cross_product_is_independent_and_complete() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let artifact_roots = [
        "web/themes",
        "customThemes",
        "resources",
        "release",
        "dist",
        "src-tauri/resources",
        "updater-release",
        "release-artifacts",
        "src-tauri/target/release/bundle",
        "src-tauri/target/x86_64-pc-windows-msvc/release/bundle",
    ];
    let literal_source_ids = [
        "card_castle",
        "noelle",
        "tv_world",
        "knight",
        "mus_ruins",
        "mus_snowy",
        "mus_waterfall",
        "mus_st_him",
        "mus_anothermedium",
        "mus_core",
        "mus_hereweare",
        "mus_endarea_parta",
    ];
    let expected_count = artifact_roots.len() * literal_source_ids.len() * 3;

    let mut expected = Vec::new();
    for artifact_root in artifact_roots {
        for source_id in literal_source_ids {
            for relative in [
                format!("{artifact_root}/component-form/{source_id}/opaque.bin"),
                format!("{artifact_root}/filename-form/{source_id}"),
                format!("{artifact_root}/stem-form/{source_id}.bin"),
            ] {
                let marker = fixture_path(repository.path(), &relative);
                create_empty_marker(&marker);
                expected.push(Finding::extracted_asset(&marker));
            }
        }
    }
    assert_eq!(expected.len(), expected_count);
    sort_and_deduplicate(&mut expected);

    assert_eq!(scan_repository_release_roots(repository.path()), expected);
}

#[test]
fn requested_source_names_remain_detected_at_any_nested_position() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let cases = [
        "web/themes/neutral/deep/CARD_CASTLE.OGG",
        "customThemes/neutral/deep/noelle.ogg",
        "resources/neutral/deep/Tv_WoRlD.OgG",
        "release/neutral/deep/KNIGHT.OGG",
        "dist/neutral/deep/MUS_RUINS.OGG",
        "src-tauri/resources/neutral/deep/mus_snowy.ogg",
        "updater-release/neutral/deep/Mus_Waterfall.Ogg",
        "release-artifacts/neutral/deep/mus_st_him.ogg",
        "web/themes/neutral/deep/mus_anothermedium.ogg",
        "release/neutral/deep/MUS_CORE.OGG",
        "dist/neutral/deep/mus_hereweare.ogg",
        "updater-release/neutral/deep/MUS_ENDAREA_PARTA.OGG",
    ];

    let mut expected = Vec::new();
    for relative in cases {
        let marker = fixture_path(repository.path(), relative);
        create_empty_marker(&marker);
        expected.push(Finding::extracted_asset(&marker));
    }
    sort_and_deduplicate(&mut expected);

    assert_eq!(scan_repository_release_roots(repository.path()), expected);
}

#[test]
fn every_explicit_non_media_context_permits_exact_source_id_stems() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let permitted_contexts = [
        ("definitions", "json"),
        ("configuration", "toml"),
        ("recipes", "yaml"),
        ("selectors", "json"),
        ("source-selectors", "txt"),
        ("placeholders", "json"),
        ("original", "json"),
        ("originals", "txt"),
        ("docs", "md"),
        ("tests", "bin"),
        ("metadata", "json"),
    ];
    let literal_source_ids = [
        "card_castle",
        "noelle",
        "tv_world",
        "knight",
        "mus_ruins",
        "mus_snowy",
        "mus_waterfall",
        "mus_st_him",
        "mus_anothermedium",
        "mus_core",
        "mus_hereweare",
        "mus_endarea_parta",
    ];
    let expected_fixture_count = permitted_contexts.len() * literal_source_ids.len();

    let mut fixture_count = 0;
    for (context, extension) in permitted_contexts {
        for source_id in literal_source_ids {
            let relative = format!("release/{context}/{source_id}.{extension}");
            create_empty_marker(&fixture_path(repository.path(), &relative));
            fixture_count += 1;
        }
    }
    assert_eq!(fixture_count, expected_fixture_count);
    let mention_only = fixture_path(repository.path(), "release/docs/source-identifiers.md");
    create_text_placeholder(
        &mention_only,
        "card_castle noelle tv_world knight card_castle.ogg tv_world.ogg",
    );

    assert!(scan_repository_release_roots(repository.path()).is_empty());
}

#[test]
fn context_confusion_and_media_cannot_bypass_rights_scanning() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let forbidden = [
        "release/CARD_CASTLE/opaque.bin",
        "dist/tv_world.bin",
        "updater-release/KNIGHT/opaque.bin",
        "release/assets/docs/noelle.md",
        "release/documentation/noelle.md",
        "release/definitions/noelle.bin",
        "release/docs/noelle.bin",
        "release/placeholders/noelle.bin",
        "release/tests/noelle.exe",
        "release/metadata/noelle.bin",
        "release/definitions/noelle/opaque.json",
        "release/tests/noelle/opaque.bin",
        "release/stage/tests/noelle.bin",
        "release/original/noelle.png",
        "release/placeholders/noelle.ogg",
        "release/docs/tv_world.wav",
        "release/source-selectors/noelle.ogg",
    ];
    let allowed_near_matches = [
        "release/card-castle.bin",
        "dist/tv-world.bin",
        "updater-release/the-knight.bin",
        "release/card_castle_preview.bin",
        "release/tv_world-definition.bin",
        "release/knightly.bin",
        "release/definitions/noelle.recipe.json",
    ];

    let mut expected = Vec::new();
    for relative in forbidden {
        let marker = fixture_path(repository.path(), relative);
        create_empty_marker(&marker);
        expected.push(Finding::extracted_asset(&marker));
    }
    for relative in allowed_near_matches {
        create_empty_marker(&fixture_path(repository.path(), relative));
    }
    let mention_only = fixture_path(repository.path(), "release/docs/rights-policy.md");
    create_text_placeholder(
        &mention_only,
        "card_castle noelle tv_world knight are source identifiers, not embedded assets",
    );
    assert_eq!(expected.len(), 17);
    sort_and_deduplicate(&mut expected);

    assert_eq!(scan_repository_release_roots(repository.path()), expected);
}

#[test]
fn updater_and_artifact_roots_are_scanned_case_insensitively() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let updater_asset = repository
        .path()
        .join("updater-release")
        .join("nested")
        .join("CARD_CASTLE.OGG");
    let artifact_asset = repository
        .path()
        .join("release-artifacts")
        .join("CuStOmThEmEs")
        .join("NoElLe")
        .join("nested")
        .join("BaCkGrOuNd.PnG");
    create_empty_marker(&updater_asset);
    create_empty_marker(&artifact_asset);

    assert_eq!(
        scan_repository_release_roots(repository.path()),
        sorted_findings([
            Finding::extracted_asset(&updater_asset),
            Finding::extracted_asset(&artifact_asset),
        ])
    );
}

#[test]
fn tauri_bundle_roots_reject_closed_recipe_outputs_without_scanning_compiler_artifacts() {
    let repository = tempfile::tempdir().expect("synthetic repository root");
    let host_bundle_asset = repository
        .path()
        .join("src-tauri")
        .join("target")
        .join("release")
        .join("bundle")
        .join("stage")
        .join("customThemes")
        .join("undertale-new-home")
        .join("theme.json");
    let target_bundle_asset = repository
        .path()
        .join("src-tauri")
        .join("target")
        .join("x86_64-pc-windows-msvc")
        .join("release")
        .join("bundle")
        .join("stage")
        .join("customThemes")
        .join("CaRd-CaStLe")
        .join("music.wav");
    let compiler_artifact_marker = repository
        .path()
        .join("src-tauri")
        .join("target")
        .join("debug")
        .join("deps")
        .join("noelle.ogg");
    create_empty_marker(&host_bundle_asset);
    create_empty_marker(&target_bundle_asset);
    create_empty_marker(&compiler_artifact_marker);

    let findings = scan_repository_release_roots(repository.path());
    assert_eq!(
        findings,
        sorted_findings([
            Finding::extracted_asset(&host_bundle_asset),
            Finding::extracted_asset(&target_bundle_asset),
        ])
    );
    assert!(!findings
        .iter()
        .any(|finding| finding.path == compiler_artifact_marker));
}

#[cfg(unix)]
#[test]
fn linked_release_root_is_reported_and_not_traversed() {
    use std::os::unix::fs::symlink;

    let repository = tempfile::tempdir().expect("synthetic repository root");
    let outside = tempfile::tempdir().expect("outside directory");
    let outside_asset = outside.path().join("card_castle.ogg");
    create_empty_marker(&outside_asset);
    let linked_root = repository.path().join("customThemes");
    symlink(outside.path(), &linked_root).expect("create linked release root");

    assert_eq!(
        scan_repository_release_roots(repository.path()),
        vec![Finding::unsafe_link_or_reparse(&linked_root)]
    );
}

#[cfg(windows)]
#[test]
fn linked_release_root_is_reported_and_not_traversed() {
    use std::os::windows::fs::symlink_dir;

    let repository = tempfile::tempdir().expect("synthetic repository root");
    let outside = tempfile::tempdir().expect("outside directory");
    let outside_asset = outside.path().join("card_castle.ogg");
    create_empty_marker(&outside_asset);
    let linked_root = repository.path().join("customThemes");

    if let Err(error) = symlink_dir(outside.path(), &linked_root) {
        assert_eq!(
            error.raw_os_error(),
            Some(1314),
            "unexpected Windows symlink creation error"
        );
        // Unprivileged Windows test hosts commonly deny symlink creation. The shared
        // classifier still proves that a reparse-marked root is rejected in that case.
        assert!(unsafe_metadata_flags(false, FILE_ATTRIBUTE_REPARSE_POINT));
        return;
    }

    assert_eq!(
        scan_repository_release_roots(repository.path()),
        vec![Finding::unsafe_link_or_reparse(&linked_root)]
    );
}

fn scan_repository_release_roots(repository_root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    if !root_is_safe_directory(repository_root, &mut findings) {
        return findings;
    }

    for relative in RELEASE_ROOTS {
        scan_relative_release_root(repository_root, Path::new(relative), &mut findings);
    }
    scan_tauri_bundle_roots(repository_root, &mut findings);
    sort_and_deduplicate(&mut findings);
    findings
}

fn scan_tauri_bundle_roots(repository_root: &Path, findings: &mut Vec<Finding>) {
    let target_relative = Path::new("src-tauri/target");
    let Some(target_root) = resolve_safe_directory(repository_root, target_relative, findings)
    else {
        return;
    };

    scan_relative_release_root(
        repository_root,
        Path::new("src-tauri/target/release/bundle"),
        findings,
    );

    for entry in read_directory(&target_root) {
        let entry = entry.expect("read Tauri target directory entry");
        let path = entry.path();
        let metadata = metadata_or_panic(&path);
        if metadata_is_unsafe(&metadata) {
            findings.push(Finding::unsafe_link_or_reparse(&path));
            continue;
        }
        if !metadata.file_type().is_dir()
            || entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("release")
        {
            continue;
        }

        let mut bundle_relative = target_relative.to_path_buf();
        bundle_relative.push(entry.file_name());
        bundle_relative.push("release");
        bundle_relative.push("bundle");
        scan_relative_release_root(repository_root, &bundle_relative, findings);
    }
}

fn scan_relative_release_root(
    repository_root: &Path,
    relative: &Path,
    findings: &mut Vec<Finding>,
) {
    let Some(root) = resolve_safe_directory(repository_root, relative, findings) else {
        return;
    };
    walk_files_no_follow(&root, findings, &mut |path| {
        let relative_path = path
            .strip_prefix(&root)
            .expect("walked release path remains beneath its checked root");
        let repository_relative_path = relative.join(relative_path);
        !is_approved_bundled_theme_asset(&repository_relative_path)
            && (is_closed_recipe_output(&repository_relative_path)
                || is_requested_asset(relative_path)
                || (is_source_id_asset(relative_path)
                    && !is_permitted_non_extracted_context(relative_path)))
    });
}

fn is_approved_bundled_theme_asset(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() != 4
        || !components[0].eq_ignore_ascii_case("web")
        || !components[1].eq_ignore_ascii_case("themes")
    {
        return false;
    }

    BUNDLED_THEME_IDS.iter().any(|id| {
        let expected = match components[2].to_ascii_lowercase().as_str() {
            "data" => format!("{id}.theme.json"),
            "img" => format!("{id}.png"),
            "mus" => format!("{id}.ogg"),
            _ => return false,
        };
        components[3].eq_ignore_ascii_case(&expected)
    })
}

fn resolve_safe_directory(
    repository_root: &Path,
    relative: &Path,
    findings: &mut Vec<Finding>,
) -> Option<PathBuf> {
    let mut current = repository_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            panic!("release scan roots must be normal relative paths: {relative:?}");
        };
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => panic!("inspect release root component {current:?}: {error}"),
        };
        if metadata_is_unsafe(&metadata) {
            findings.push(Finding::unsafe_link_or_reparse(&current));
            return None;
        }
        if !metadata.file_type().is_dir() {
            return None;
        }
    }
    Some(current)
}

fn root_is_safe_directory(root: &Path, findings: &mut Vec<Finding>) -> bool {
    let metadata = metadata_or_panic(root);
    if metadata_is_unsafe(&metadata) {
        findings.push(Finding::unsafe_link_or_reparse(root));
        return false;
    }
    metadata.file_type().is_dir()
}

fn scan_crate_for_media(root: &Path, findings: &mut Vec<Finding>) {
    if !root_is_safe_directory(root, findings) {
        return;
    }
    walk_files_no_follow(root, findings, &mut has_media_extension);
}

fn walk_files_no_follow(
    root: &Path,
    findings: &mut Vec<Finding>,
    inspect: &mut impl FnMut(&Path) -> bool,
) {
    let metadata = metadata_or_panic(root);
    if metadata_is_unsafe(&metadata) {
        findings.push(Finding::unsafe_link_or_reparse(root));
        return;
    }
    if !metadata.file_type().is_dir() {
        return;
    }

    for entry in read_directory(root) {
        let entry = entry.expect("read release directory entry");
        let path = entry.path();
        let metadata = metadata_or_panic(&path);
        if metadata_is_unsafe(&metadata) {
            findings.push(Finding::unsafe_link_or_reparse(&path));
        } else if metadata.file_type().is_dir() {
            walk_files_no_follow(&path, findings, inspect);
        } else if metadata.file_type().is_file() && inspect(&path) {
            findings.push(Finding::extracted_asset(&path));
        }
    }
}

fn metadata_is_unsafe(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    let windows_attributes = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
    };
    #[cfg(not(windows))]
    let windows_attributes = 0;

    unsafe_metadata_flags(metadata.file_type().is_symlink(), windows_attributes)
}

fn unsafe_metadata_flags(is_symlink: bool, windows_attributes: u32) -> bool {
    is_symlink || windows_attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn is_requested_asset(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    REQUESTED_ASSET_NAMES
        .iter()
        .any(|expected| file_name.eq_ignore_ascii_case(expected))
}

fn is_closed_recipe_output(path: &Path) -> bool {
    let mut follows_custom_themes = false;
    for component in path.components() {
        let Component::Normal(component) = component else {
            follows_custom_themes = false;
            continue;
        };
        let Some(component) = component.to_str() else {
            follows_custom_themes = false;
            continue;
        };
        if follows_custom_themes
            && RecipeId::ALL
                .iter()
                .any(|recipe_id| component.eq_ignore_ascii_case(recipe_id.as_str()))
        {
            return true;
        }
        follows_custom_themes = component.eq_ignore_ascii_case("customThemes");
    }
    false
}

fn has_media_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            ["png", "ogg", "wav"]
                .iter()
                .any(|expected| extension.eq_ignore_ascii_case(expected))
        })
}

fn is_source_id_asset(path: &Path) -> bool {
    source_id_in_parent(path) || source_id_in_file_name(path)
}

fn source_id_in_parent(path: &Path) -> bool {
    path.parent().is_some_and(|parent| {
        parent.components().any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(is_source_asset_id)
        })
    })
}

fn source_id_in_file_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(is_source_asset_id)
        || path
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(is_source_asset_id)
}

fn is_source_asset_id(value: &str) -> bool {
    SOURCE_ASSET_IDS
        .iter()
        .any(|source_id| value.eq_ignore_ascii_case(source_id))
}

fn is_permitted_non_extracted_context(path: &Path) -> bool {
    if has_media_extension(path) || source_id_in_parent(path) || !source_id_in_file_name(path) {
        return false;
    }
    let Some(context) = path
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
    else {
        return false;
    };
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };

    if ["definitions", "configuration", "recipes"]
        .iter()
        .any(|allowed| context.eq_ignore_ascii_case(allowed))
    {
        return extension_is_one_of(extension, &["json", "toml", "yaml", "yml"]);
    }
    if ["selectors", "source-selectors"]
        .iter()
        .any(|allowed| context.eq_ignore_ascii_case(allowed))
    {
        return extension_is_one_of(extension, &["json", "toml", "yaml", "yml", "txt"]);
    }
    if ["placeholders", "original", "originals"]
        .iter()
        .any(|allowed| context.eq_ignore_ascii_case(allowed))
    {
        return extension_is_one_of(extension, &["json", "toml", "yaml", "yml", "txt"]);
    }
    if context.eq_ignore_ascii_case("docs") {
        return extension_is_one_of(extension, &["md", "txt", "rst"]);
    }
    if context.eq_ignore_ascii_case("tests") {
        return extension_is_one_of(extension, &["bin", "json", "toml", "txt"]);
    }
    if context.eq_ignore_ascii_case("metadata") {
        return extension_is_one_of(extension, &["json", "toml", "yaml", "yml", "txt"]);
    }
    false
}

fn extension_is_one_of(extension: &str, allowed: &[&str]) -> bool {
    allowed
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

fn path_file_name_is(path: &Path, expected: &str) -> bool {
    path.file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

fn metadata_or_panic(path: &Path) -> fs::Metadata {
    fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("inspect release entry {path:?}: {error}"))
}

fn read_directory(path: &Path) -> fs::ReadDir {
    fs::read_dir(path).unwrap_or_else(|error| panic!("scan release tree {path:?}: {error}"))
}

fn scan_source_text_no_follow(root: &Path, forbidden_roots: &[&str], findings: &mut Vec<PathBuf>) {
    let metadata = metadata_or_panic(root);
    if metadata_is_unsafe(&metadata) {
        findings.push(root.to_path_buf());
        return;
    }
    if metadata.file_type().is_dir() {
        for entry in read_directory(root) {
            let entry = entry.expect("read production source directory entry");
            scan_source_text_no_follow(&entry.path(), forbidden_roots, findings);
        }
        return;
    }
    if !metadata.file_type().is_file()
        || !root
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
    {
        return;
    }

    let source = fs::read_to_string(root)
        .unwrap_or_else(|error| panic!("read production Rust source {root:?}: {error}"));
    let source = normalize_path_text(&source);
    if forbidden_roots
        .iter()
        .map(|value| normalize_path_text(value))
        .any(|forbidden| source.contains(&forbidden))
    {
        findings.push(root.to_path_buf());
    }
}

fn normalize_path_text(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut previous_was_separator = false;
    for character in value.chars() {
        if matches!(character, '/' | '\\') {
            if !previous_was_separator {
                normalized.push('/');
            }
            previous_was_separator = true;
        } else {
            normalized.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        }
    }
    normalized
}

fn create_empty_marker(path: &Path) {
    fs::create_dir_all(path.parent().expect("marker parent")).expect("create marker directory");
    fs::File::create(path).expect("create empty generated-asset marker");
    assert_eq!(fs::metadata(path).expect("marker metadata").len(), 0);
}

fn create_text_placeholder(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("placeholder parent"))
        .expect("create placeholder directory");
    fs::write(path, text).expect("create textual placeholder fixture");
}

fn fixture_path(root: &Path, slash_separated_relative: &str) -> PathBuf {
    slash_separated_relative
        .split('/')
        .fold(root.to_path_buf(), |mut path, component| {
            path.push(component);
            path
        })
}

fn sorted_findings<const N: usize>(findings: [Finding; N]) -> Vec<Finding> {
    let mut findings = findings.into_iter().collect::<Vec<_>>();
    sort_and_deduplicate(&mut findings);
    findings
}

fn sort_and_deduplicate(findings: &mut Vec<Finding>) {
    findings.sort();
    findings.dedup();
}

fn sort_paths_and_deduplicate(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}
