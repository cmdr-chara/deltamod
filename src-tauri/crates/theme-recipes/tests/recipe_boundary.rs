mod support;

use deltamod_mods_themes_runtime::ThemeJson;
use deltamod_theme_recipes::{
    build_execution_plan, build_provenance, definition, definitions, parse_and_validate_recipe,
    provenance_json, provenance_sha256, validate_recipe, AudioFormat, OutputArtifact, OutputScope,
    ReadPolicy, RecipeError, RecipeId, RightsAttestation, SelectedGameRoot, SelectorCandidate,
    SelectorStatus, SourceSlot, TransformKind, PROVENANCE_SCHEMA_VERSION, RECIPE_SCHEMA_VERSION,
    THEME_MANIFEST_SCHEMA_VERSION,
};
use serde_json::{json, Value};
use std::{fs, path::Path};
use tempfile::TempDir;

struct SyntheticFixture {
    _directory: TempDir,
    root: SelectedGameRoot,
    recipe: deltamod_theme_recipes::ValidatedRecipe,
    selectors: deltamod_theme_recipes::ValidatedSelectors,
}

impl SyntheticFixture {
    fn new(id: RecipeId) -> Self {
        let directory = tempfile::tempdir().expect("synthetic fixture temp directory");
        let asset_dir = directory.path().join("fixture-assets");
        fs::create_dir(&asset_dir).expect("synthetic fixture asset directory");
        fs::write(asset_dir.join("background.png"), support::synthetic_png())
            .expect("synthetic PNG fixture");
        fs::write(asset_dir.join("music.ogg"), support::synthetic_ogg())
            .expect("synthetic OGG fixture");

        let recipe = validate_recipe(definition(id)).expect("canonical recipe");
        let root = SelectedGameRoot::open_user_selected(directory.path())
            .expect("canonical synthetic game root");
        let selectors = root
            .validate_selectors(
                &recipe,
                &[
                    SelectorCandidate::new(
                        SourceSlot::Background,
                        "synthetic-background",
                        "fixture-assets/background.png",
                    )
                    .expect("background candidate"),
                    SelectorCandidate::new(
                        SourceSlot::Music,
                        "synthetic-music",
                        "fixture-assets/music.ogg",
                    )
                    .expect("music candidate"),
                ],
                RightsAttestation::accepted(),
            )
            .expect("validated synthetic selectors");

        Self {
            _directory: directory,
            root,
            recipe,
            selectors,
        }
    }

    fn plan(&self) -> deltamod_theme_recipes::ExecutionPlan {
        build_execution_plan(
            &self.recipe,
            &self.root,
            Some(&self.selectors),
            RightsAttestation::accepted(),
        )
        .expect("synthetic execution plan")
    }
}

#[test]
fn twelve_definitions_are_closed_local_only_placeholders() {
    let expected = [
        (
            RecipeId::CardCastle,
            "card-castle",
            "Card Castle",
            "card_castle.ogg",
            "DELTARUNE",
        ),
        (
            RecipeId::Noelle,
            "noelle",
            "Noelle",
            "noelle.ogg",
            "DELTARUNE",
        ),
        (
            RecipeId::TvWorld,
            "tv-world",
            "TV World",
            "tv_world.ogg",
            "DELTARUNE",
        ),
        (
            RecipeId::TheKnight,
            "the-knight",
            "The Knight",
            "knight.ogg",
            "DELTARUNE",
        ),
        (
            RecipeId::UndertaleRuins,
            "undertale-ruins",
            "Ruins",
            "mus_ruins.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleSnowdin,
            "undertale-snowdin",
            "Snowdin",
            "mus_snowy.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleWaterfall,
            "undertale-waterfall",
            "Waterfall",
            "mus_waterfall.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleVoid,
            "undertale-void",
            "Void",
            "mus_st_him.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleHotland,
            "undertale-hotland",
            "Hotland",
            "mus_anothermedium.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleCore,
            "undertale-core",
            "CORE",
            "mus_core.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleTrueLab,
            "undertale-true-lab",
            "True Lab",
            "mus_hereweare.ogg",
            "UNDERTALE",
        ),
        (
            RecipeId::UndertaleNewHome,
            "undertale-new-home",
            "New Home",
            "mus_endarea_parta.ogg",
            "UNDERTALE",
        ),
    ];
    let recipes = definitions();
    assert_eq!(recipes.len(), expected.len());
    assert_eq!(
        RecipeId::ALL.map(|id| id.as_str()),
        expected.map(|(_, slug, _, _, _)| slug),
        "recipe order and serialized IDs are stable"
    );

    for (recipe, (id, slug, name, requested_music, source_work)) in
        recipes.into_iter().zip(expected)
    {
        assert_eq!(recipe.schema_version, RECIPE_SCHEMA_VERSION);
        assert_eq!(recipe.id, id);
        assert_eq!(recipe.id.as_str(), slug);
        assert_eq!(recipe.name, name);
        assert_eq!(
            recipe.selectors.background.status,
            SelectorStatus::Unresolved
        );
        assert_eq!(recipe.selectors.background.requested_name, None);
        assert_eq!(recipe.selectors.music.status, SelectorStatus::Unresolved);
        assert_eq!(
            recipe.selectors.music.requested_name.as_deref(),
            Some(requested_music)
        );
        assert_eq!(recipe.output.scope, OutputScope::CustomThemes);
        assert_eq!(recipe.output.audio_format, AudioFormat::Ogg);
        assert_eq!(recipe.attribution.source_work, source_work);
        assert!(recipe.attribution.notice.contains("local-only"));
        assert_eq!(recipe.transforms.background.len(), 1);
        assert_eq!(recipe.transforms.music.len(), 1);
        assert_eq!(
            recipe.transforms.background[0].kind,
            TransformKind::CopyVerified
        );

        let json = serde_json::to_vec(&recipe).expect("serialize recipe");
        let serialized: Value = serde_json::from_slice(&json).expect("inspect recipe JSON");
        assert_eq!(serialized["id"], slug);
        let reparsed = parse_and_validate_recipe(&json).expect("strict recipe round trip");
        assert_eq!(reparsed.id(), id);
        let json = String::from_utf8(json).expect("recipe JSON is UTF-8");
        for absolute_drive_marker in [":\\", ":/"] {
            assert!(
                !json.contains(absolute_drive_marker),
                "canonical recipes must not embed an installation path"
            );
        }
    }
}

#[test]
fn v2_contracts_replace_unpersisted_v1_and_recipe_versions_fail_closed() {
    assert_eq!(RECIPE_SCHEMA_VERSION, 2);
    assert_eq!(THEME_MANIFEST_SCHEMA_VERSION, 2);
    assert_eq!(PROVENANCE_SCHEMA_VERSION, 2);

    for unsupported in [1, RECIPE_SCHEMA_VERSION + 1] {
        let mut document =
            serde_json::to_value(definition(RecipeId::CardCastle)).expect("recipe value");
        document["schemaVersion"] = json!(unsupported);
        let bytes = serde_json::to_vec(&document).expect("versioned recipe JSON");
        assert!(
            matches!(
                parse_and_validate_recipe(&bytes),
                Err(RecipeError::UnsupportedSchema(actual)) if actual == unsupported
            ),
            "unexpectedly accepted recipe schema version {unsupported}"
        );
    }
}

#[test]
fn built_in_extraction_is_disabled_until_selectors_resolve() {
    let directory = tempfile::tempdir().expect("temporary game root");
    let root = SelectedGameRoot::open_user_selected(directory.path()).expect("selected root");
    let recipe = validate_recipe(definition(RecipeId::CardCastle)).expect("canonical recipe");
    let result = build_execution_plan(&recipe, &root, None, RightsAttestation::accepted());
    assert!(matches!(result, Err(RecipeError::UnresolvedSelectors)));
}

#[test]
fn strict_schema_rejects_commands_scripts_and_arbitrary_selectors() {
    let canonical = definition(RecipeId::CardCastle);

    for (pointer, value) in [
        ("/command", json!("extract.exe --all")),
        ("/script", json!("do-whatever.ps1")),
        ("/destination", json!("web/themes")),
        ("/selectors/background/selector", json!("sprites/* | shell")),
        ("/transforms/background/0/args", json!(["--exec"])),
    ] {
        let mut document = serde_json::to_value(&canonical).expect("recipe value");
        insert_pointer(&mut document, pointer, value);
        let bytes = serde_json::to_vec(&document).expect("mutated recipe JSON");
        assert!(
            matches!(
                parse_and_validate_recipe(&bytes),
                Err(RecipeError::InvalidRecipeJson(_))
            ),
            "unexpectedly accepted {pointer}"
        );
    }

    let mut release_scope = serde_json::to_value(&canonical).expect("recipe value");
    release_scope["output"]["scope"] = json!("bundled-themes");
    assert!(matches!(
        parse_and_validate_recipe(&serde_json::to_vec(&release_scope).expect("release-scope JSON")),
        Err(RecipeError::InvalidRecipeJson(_))
    ));

    let mut renamed = canonical;
    renamed.name = "Not the closed definition".to_owned();
    assert!(matches!(
        validate_recipe(renamed),
        Err(RecipeError::NonCanonicalDefinition)
    ));
}

#[test]
fn synthetic_fixture_produces_only_no_follow_closed_plan() {
    let fixture = SyntheticFixture::new(RecipeId::CardCastle);
    let plan = fixture.plan();

    assert!(plan
        .reads()
        .iter()
        .all(|read| read.policy == ReadPolicy::NoFollow));
    assert!(plan.reads().iter().all(|read| {
        read.expected_length() > 0
            && read
                .try_clone_source_handle()
                .and_then(|file| file.metadata())
                .is_ok_and(|metadata| metadata.is_file())
            && read.source_ancestor_count() > 0
    }));
    assert_eq!(
        plan.outputs()
            .iter()
            .map(|output| output.relative_path.as_str())
            .collect::<Vec<_>>(),
        [
            "customThemes/card-castle/theme.json",
            "customThemes/card-castle/background.png",
            "customThemes/card-castle/music.ogg",
            "customThemes/card-castle/provenance.json",
        ]
    );
    assert_eq!(
        plan.outputs()
            .iter()
            .map(|output| output.artifact)
            .collect::<Vec<_>>(),
        [
            OutputArtifact::ThemeJson,
            OutputArtifact::BackgroundPng,
            OutputArtifact::MusicOgg,
            OutputArtifact::ProvenanceJson,
        ]
    );

    let manifest: Value =
        serde_json::from_slice(&plan.theme_json().expect("theme JSON")).expect("parse theme JSON");
    assert_eq!(manifest["icon"], "background.png");
    assert_eq!(manifest["music"], "music.ogg");
    assert_eq!(manifest["localOnly"], true);
    assert_eq!(manifest["builtIn"], false);
}

#[test]
fn selector_and_plan_debug_surfaces_redact_every_source_path_and_identity() {
    let fixture = SyntheticFixture::new(RecipeId::Noelle);
    let plan = fixture.plan();
    let root_text = fixture.root.canonical_path().to_string_lossy();
    let debug = format!(
        "{:?} {:?} {:?} {:?}",
        fixture.selectors,
        plan,
        plan.reads(),
        plan.outputs()
    );
    for forbidden in [
        root_text.as_ref(),
        "fixture-assets",
        "background.png",
        "music.ogg",
    ] {
        assert!(!debug.contains(forbidden), "debug leaked {forbidden}");
    }
    assert!(debug.contains("<redacted>"));

    let provenance =
        provenance_json(&build_provenance(&plan).expect("path-free deterministic provenance"))
            .expect("serialize provenance");
    let provenance = String::from_utf8(provenance).expect("provenance UTF-8");
    assert!(!provenance.contains("fixture-assets"));
    assert!(!provenance.contains(root_text.as_ref()));
}

#[test]
fn selector_candidate_debug_and_diagnostic_json_redact_source_relative_path() {
    let candidate = SelectorCandidate::new(
        SourceSlot::Background,
        "synthetic-background",
        "private-installation/assets/background.png",
    )
    .expect("validated candidate");
    let debug = format!("{candidate:?}");
    let serialized = serde_json::to_string(&candidate).expect("diagnostic candidate JSON");

    for forbidden in ["private-installation", "assets/background.png"] {
        assert!(!debug.contains(forbidden));
        assert!(!serialized.contains(forbidden));
    }
    assert!(debug.contains("<redacted>"));
    assert!(serialized.contains("<redacted>"));
}

#[test]
fn generated_theme_json_matches_mods_themes_runtime_shape_and_source_work() {
    let fixture = SyntheticFixture::new(RecipeId::CardCastle);
    let bytes = fixture.plan().theme_json().expect("generated theme JSON");
    let theme: ThemeJson =
        serde_json::from_slice(&bytes).expect("real mods-themes-runtime ThemeJson contract");

    assert_eq!(theme.id, "card-castle");
    assert_eq!(theme.name, "Card Castle");
    assert_eq!(
        theme.description.as_deref(),
        Some("Generated locally from a user-selected DELTARUNE installation.")
    );
    assert!(!theme.built_in);
    assert_eq!(theme.icon.as_deref(), Some("background.png"));
    assert_eq!(theme.music.as_deref(), Some("music.ogg"));
    assert_eq!(theme.color, None);
    assert_eq!(theme.soul_color, None);

    let raw: Value = serde_json::from_slice(&bytes).expect("inspect generated theme JSON");
    assert!(raw.get("background").is_none());

    assert_eq!(raw["schemaVersion"], THEME_MANIFEST_SCHEMA_VERSION);

    let undertale_fixture = SyntheticFixture::new(RecipeId::UndertaleNewHome);
    let undertale_plan = undertale_fixture.plan();
    let undertale_bytes = undertale_plan
        .theme_json()
        .expect("generated UNDERTALE theme JSON");
    let undertale_theme: ThemeJson = serde_json::from_slice(&undertale_bytes)
        .expect("real mods-themes-runtime ThemeJson contract");
    assert_eq!(undertale_theme.id, "undertale-new-home");
    assert_eq!(undertale_theme.name, "New Home");
    assert_eq!(
        undertale_theme.description.as_deref(),
        Some("Generated locally from a user-selected UNDERTALE installation.")
    );
    assert_eq!(
        undertale_plan
            .outputs()
            .iter()
            .map(|output| output.relative_path.as_str())
            .collect::<Vec<_>>(),
        [
            "customThemes/undertale-new-home/theme.json",
            "customThemes/undertale-new-home/background.png",
            "customThemes/undertale-new-home/music.ogg",
            "customThemes/undertale-new-home/provenance.json",
        ]
    );

    let undertale_provenance =
        build_provenance(&undertale_plan).expect("UNDERTALE provenance document");
    assert_eq!(
        undertale_provenance.schema_version,
        PROVENANCE_SCHEMA_VERSION
    );
    assert_eq!(undertale_provenance.recipe_id, RecipeId::UndertaleNewHome);
    let undertale_provenance: Value = serde_json::from_slice(
        &provenance_json(&undertale_provenance).expect("UNDERTALE provenance JSON"),
    )
    .expect("inspect UNDERTALE provenance JSON");
    assert_eq!(undertale_provenance["recipeId"], "undertale-new-home");
}

#[test]
fn rights_attestation_is_a_three_part_gate() {
    let fixture = SyntheticFixture::new(RecipeId::Noelle);
    let rejected = [
        RightsAttestation {
            user_selected_installation: false,
            authorized_to_extract: true,
            local_only: true,
        },
        RightsAttestation {
            user_selected_installation: true,
            authorized_to_extract: false,
            local_only: true,
        },
        RightsAttestation {
            user_selected_installation: true,
            authorized_to_extract: true,
            local_only: false,
        },
    ];

    for attestation in rejected {
        let unreadable_candidates = [
            SelectorCandidate::new(SourceSlot::Background, "background", "missing.png")
                .expect("background candidate"),
            SelectorCandidate::new(SourceSlot::Music, "music", "missing.ogg")
                .expect("music candidate"),
        ];
        assert!(matches!(
            fixture
                .root
                .validate_selectors(&fixture.recipe, &unreadable_candidates, attestation),
            Err(RecipeError::RightsAttestationRequired)
        ));
        assert!(matches!(
            build_execution_plan(
                &fixture.recipe,
                &fixture.root,
                Some(&fixture.selectors),
                attestation,
            ),
            Err(RecipeError::RightsAttestationRequired)
        ));
    }
}

#[test]
fn selector_paths_reject_traversal_drives_globs_and_streams() {
    for path in [
        "../escape.png",
        "%2e%2e/escape.png",
        "/absolute.png",
        "C:/hardcoded.png",
        "E:/SteamLibrary/steamapps/common/Undertale/asset.png",
        "G:/DELTARUNE/asset.png",
        r"\\server\share\asset.png",
        r"folder\asset.png",
        "folder/*.png",
        "folder/asset?.png",
        "folder/asset.png:stream",
    ] {
        assert!(
            SelectorCandidate::new(SourceSlot::Background, "source", path).is_err(),
            "unexpectedly accepted {path}"
        );
    }
    assert!(SelectedGameRoot::open_user_selected(Path::new("relative/game-root")).is_err());
}

#[test]
fn selector_validation_rejects_wrong_media_and_duplicate_bindings() {
    let directory = tempfile::tempdir().expect("temporary fixture");
    fs::write(directory.path().join("wrong.png"), support::synthetic_ogg())
        .expect("wrong media fixture");
    fs::write(directory.path().join("music.ogg"), support::synthetic_ogg()).expect("music fixture");
    let root = SelectedGameRoot::open_user_selected(directory.path()).expect("selected root");
    let recipe = validate_recipe(definition(RecipeId::TvWorld)).expect("canonical recipe");

    let wrong_media = [
        SelectorCandidate::new(SourceSlot::Background, "background", "wrong.png")
            .expect("background candidate"),
        SelectorCandidate::new(SourceSlot::Music, "music", "music.ogg").expect("music candidate"),
    ];
    assert!(matches!(
        root.validate_selectors(&recipe, &wrong_media, RightsAttestation::accepted()),
        Err(RecipeError::SourceMediaMismatch)
    ));

    let duplicate = [
        SelectorCandidate::new(SourceSlot::Background, "same", "wrong.png")
            .expect("first candidate"),
        SelectorCandidate::new(SourceSlot::Music, "same", "music.ogg").expect("second candidate"),
    ];
    assert!(matches!(
        root.validate_selectors(&recipe, &duplicate, RightsAttestation::accepted()),
        Err(RecipeError::DuplicateSelector)
    ));
}

#[test]
fn selector_validation_rejects_magic_prefix_only_media() {
    let directory = tempfile::tempdir().expect("temporary fixture");
    fs::write(
        directory.path().join("background.png"),
        b"\x89PNG\r\n\x1a\nnot-a-structured-png",
    )
    .expect("prefix-only PNG fixture");
    fs::write(
        directory.path().join("music.ogg"),
        b"OggSnot-a-structured-ogg",
    )
    .expect("prefix-only Ogg fixture");
    let root = SelectedGameRoot::open_user_selected(directory.path()).expect("selected root");
    let recipe = validate_recipe(definition(RecipeId::TvWorld)).expect("canonical recipe");
    let candidates = [
        SelectorCandidate::new(SourceSlot::Background, "background", "background.png")
            .expect("background candidate"),
        SelectorCandidate::new(SourceSlot::Music, "music", "music.ogg").expect("music candidate"),
    ];

    assert!(matches!(
        root.validate_selectors(&recipe, &candidates, RightsAttestation::accepted()),
        Err(RecipeError::SourceMediaMismatch)
    ));
}

#[test]
fn validated_selectors_cannot_be_rebound_to_another_root() {
    let fixture = SyntheticFixture::new(RecipeId::TheKnight);
    let other = tempfile::tempdir().expect("other root");
    let other_root = SelectedGameRoot::open_user_selected(other.path()).expect("other root");
    assert!(matches!(
        build_execution_plan(
            &fixture.recipe,
            &other_root,
            Some(&fixture.selectors),
            RightsAttestation::accepted(),
        ),
        Err(RecipeError::SelectorContextMismatch)
    ));
}

#[test]
fn provenance_is_deterministic_hashed_and_path_free() {
    let fixture = SyntheticFixture::new(RecipeId::CardCastle);
    let plan = fixture.plan();
    let first = build_provenance(&plan).expect("provenance");
    let second = build_provenance(&plan).expect("repeat provenance");
    assert_eq!(first, second);

    let first_json = provenance_json(&first).expect("provenance JSON");
    let second_json = provenance_json(&second).expect("repeat provenance JSON");
    assert_eq!(first_json, second_json);
    assert_eq!(
        provenance_sha256(&first).expect("provenance hash"),
        provenance_sha256(&second).expect("repeat provenance hash")
    );

    assert_eq!(first.sources.len(), 2);
    assert_eq!(first.transforms.len(), 2);
    assert_eq!(first.outputs.len(), 3);
    assert_eq!(
        first.sources[0].sha256.as_str(),
        "ff6674e545c921b2cab9b12a478e9104a83cf2b03724e11a9ce94743f9474864"
    );
    assert_eq!(
        first.sources[1].sha256.as_str(),
        "f829939a6d51561d5af1a729e91991cf8160ebfdc82a8d67fe009ad74b5a0a20"
    );
    assert_eq!(
        provenance_sha256(&first)
            .expect("golden provenance hash")
            .as_str(),
        "5bae934a4df93fff796b420fcf59e7cc6611060948120a8ace35735743633e60"
    );

    let text = String::from_utf8(first_json).expect("provenance is UTF-8");
    let canonical_root = plan.canonical_game_root().to_string_lossy();
    assert!(!text.contains(canonical_root.as_ref()));
    assert!(!text.contains("fixture-assets"));
    assert!(!text.contains("relativePath"));
    assert!(text.contains("user-authorized-installation-local-only"));
    assert!(text.contains("Source rights remain with their respective owners"));

    let mut value: Value = serde_json::from_str(&text).expect("strict provenance JSON");
    value["absoluteSourcePath"] = json!("C:/forbidden");
    assert!(serde_json::from_value::<deltamod_theme_recipes::ProvenanceDocument>(value).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    use std::os::unix::fs::symlink;

    let fixture_root = tempfile::tempdir().expect("fixture root");
    let outside = tempfile::tempdir().expect("outside root");
    fs::write(
        outside.path().join("background.png"),
        support::synthetic_png(),
    )
    .expect("outside fixture");
    symlink(outside.path(), fixture_root.path().join("linked")).expect("fixture symlink");
    fs::write(
        fixture_root.path().join("music.ogg"),
        support::synthetic_ogg(),
    )
    .expect("music fixture");
    assert_link_selector_rejected(fixture_root.path());
}

#[cfg(windows)]
#[test]
fn reparse_escape_is_rejected() {
    use std::os::windows::fs::symlink_dir;
    use std::process::Command;

    let fixture_root = tempfile::tempdir().expect("fixture root");
    let outside = tempfile::tempdir().expect("outside root");
    fs::write(
        outside.path().join("background.png"),
        support::synthetic_png(),
    )
    .expect("outside fixture");
    let link = fixture_root.path().join("linked");
    if let Err(error) = symlink_dir(outside.path(), &link) {
        assert_eq!(
            error.raw_os_error(),
            Some(1314),
            "unexpected Windows symlink error"
        );
        let status = Command::new("pwsh")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:DELTAMOD_E1_TEST_LINK -Target $env:DELTAMOD_E1_TEST_TARGET | Out-Null",
            ])
            .env("DELTAMOD_E1_TEST_LINK", &link)
            .env("DELTAMOD_E1_TEST_TARGET", outside.path())
            .status()
            .expect("PowerShell 7 is required by repository policy");
        assert!(status.success(), "failed to create test junction");
    }
    fs::write(
        fixture_root.path().join("music.ogg"),
        support::synthetic_ogg(),
    )
    .expect("music fixture");
    assert_link_selector_rejected(fixture_root.path());
}

fn assert_link_selector_rejected(root_path: &Path) {
    let root = SelectedGameRoot::open_user_selected(root_path).expect("selected root");
    let recipe = validate_recipe(definition(RecipeId::Noelle)).expect("canonical recipe");
    let candidates = [
        SelectorCandidate::new(
            SourceSlot::Background,
            "linked-background",
            "linked/background.png",
        )
        .expect("linked candidate"),
        SelectorCandidate::new(SourceSlot::Music, "music", "music.ogg").expect("music candidate"),
    ];
    assert!(matches!(
        root.validate_selectors(&recipe, &candidates, RightsAttestation::accepted()),
        Err(RecipeError::PathBoundary(_)) | Err(RecipeError::UnsafeSource)
    ));
}

fn insert_pointer(document: &mut Value, pointer: &str, value: Value) {
    let (parent, key) = pointer.rsplit_once('/').expect("JSON pointer with parent");
    document.pointer_mut(parent).expect("JSON pointer parent")[key] = value;
}
