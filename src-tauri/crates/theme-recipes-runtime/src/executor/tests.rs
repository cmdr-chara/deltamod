use super::*;
use deltamod_mods_themes_runtime::ThemeJson;
use deltamod_theme_recipes::{
    build_execution_plan, definition, validate_recipe, ExecutionPlan, OutputArtifact,
    ProvenanceDocument, RecipeId, RightsAttestation, RightsMarker, SelectedGameRoot,
    SelectorCandidate, SourceSlot,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Barrier, Mutex},
    thread,
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    game_root: PathBuf,
    staging: PathBuf,
    live_marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("synthetic runtime fixture");
        let game_root = temp.path().join("synthetic-game");
        let assets = game_root.join("fixture-assets");
        let staging = temp.path().join("private-operation-root");
        let live_tree = temp.path().join("customThemes");
        fs::create_dir_all(&assets).expect("synthetic source directory");
        fs::create_dir(&staging).expect("per-operation staging root");
        make_private(&staging);
        fs::create_dir(&live_tree).expect("unowned live tree sentinel");
        let live_marker = live_tree.join("must-not-change.txt");
        fs::write(&live_marker, b"unchanged").expect("live tree sentinel");
        fs::write(assets.join("background.png"), synthetic_png(1)).expect("synthetic PNG");
        fs::write(assets.join("music.ogg"), synthetic_ogg(1)).expect("synthetic OGG");
        Self {
            _temp: temp,
            game_root,
            staging,
            live_marker,
        }
    }

    fn roots(&self) -> HostSelectedStagingRoot {
        HostSelectedStagingRoot::validate(&self.staging).expect("trusted private staging root")
    }

    fn executor(&self) -> LocalThemeRecipeExecutor {
        LocalThemeRecipeExecutor::new(self.roots())
    }

    fn request(&self) -> StagingRequest {
        StagingRequest::new("theme-op-0001", "theme-idempotency-0001")
            .expect("stable operation identity")
    }

    fn plan(&self, id: RecipeId) -> ExecutionPlan {
        let recipe = validate_recipe(definition(id)).expect("accepted recipe");
        let root = SelectedGameRoot::open_user_selected(&self.game_root)
            .expect("synthetic selected game root");
        let selectors = root
            .validate_selectors(
                &recipe,
                &[
                    SelectorCandidate::new(
                        SourceSlot::Background,
                        "synthetic-background",
                        "fixture-assets/background.png",
                    )
                    .expect("background selector"),
                    SelectorCandidate::new(
                        SourceSlot::Music,
                        "synthetic-music",
                        "fixture-assets/music.ogg",
                    )
                    .expect("music selector"),
                ],
                RightsAttestation::accepted(),
            )
            .expect("accepted selectors");
        build_execution_plan(
            &recipe,
            &root,
            Some(&selectors),
            RightsAttestation::accepted(),
        )
        .expect("rights-attested plan")
    }

    fn source(&self, name: &str) -> PathBuf {
        self.game_root.join("fixture-assets").join(name)
    }

    fn assert_live_tree_unchanged(&self) {
        assert_eq!(
            fs::read(&self.live_marker).expect("live sentinel remains readable"),
            b"unchanged"
        );
        assert_eq!(
            entry_names(self.live_marker.parent().unwrap()),
            ["must-not-change.txt"]
        );
    }
}

#[test]
fn stages_exact_hash_verified_outputs_and_returns_unwired_handoff() {
    let fixture = Fixture::new();
    let plan = fixture.plan(RecipeId::CardCastle);
    let handoff = fixture
        .executor()
        .execute(&fixture.request(), &plan, &CancellationToken::new())
        .expect("verified staging handoff");

    assert_eq!(handoff.schema_version(), 1);
    assert_eq!(handoff.operation_id(), "theme-op-0001");
    assert_eq!(handoff.idempotency_key(), "theme-idempotency-0001");
    assert_eq!(handoff.recipe_id(), RecipeId::CardCastle);
    assert_eq!(
        handoff.rights(),
        RightsMarker::UserAuthorizedInstallationLocalOnly
    );
    assert_eq!(handoff.publication(), PublicationState::Unwired);
    assert_eq!(
        entry_names(&fixture.staging),
        [
            "background.png",
            "music.ogg",
            "provenance.json",
            PUBLICATION_HANDOFF_FILE,
            STAGING_INTENT_FILE,
            "theme.json",
        ]
    );

    for output in handoff.outputs() {
        let bytes =
            fs::read(fixture.staging.join(output.staging_file())).expect("read staged output");
        assert_eq!(bytes.len() as u64, output.length());
        assert_eq!(
            deltamod_theme_recipes::Sha256Digest::from_bytes(&bytes),
            *output.sha256()
        );
        assert_eq!(
            output.destination(),
            format!("customThemes/card-castle/{}", output.artifact().file_name())
        );
    }

    let theme: ThemeJson = serde_json::from_slice(
        &fs::read(fixture.staging.join("theme.json")).expect("staged theme JSON"),
    )
    .expect("real theme contract");
    assert_eq!(theme.id, "card-castle");
    let _: ProvenanceDocument = serde_json::from_slice(
        &fs::read(fixture.staging.join("provenance.json")).expect("staged provenance"),
    )
    .expect("strict provenance contract");

    let persisted: PublicationHandoff = serde_json::from_slice(
        &fs::read(fixture.staging.join(PUBLICATION_HANDOFF_FILE)).expect("persisted handoff"),
    )
    .expect("closed persisted handoff");
    assert_eq!(persisted, handoff);
    let public_surface = format!(
        "{handoff:?} {}",
        serde_json::to_string(&handoff).expect("serialize handoff")
    );
    for forbidden in [
        fixture.game_root.to_string_lossy().as_ref(),
        fixture.staging.to_string_lossy().as_ref(),
        "fixture-assets",
    ] {
        assert!(!public_surface.contains(forbidden));
    }
    fixture.assert_live_tree_unchanged();
}

#[test]
fn completed_repeat_returns_same_verified_handoff_without_rewriting() {
    let fixture = Fixture::new();
    let plan = fixture.plan(RecipeId::Noelle);
    let writes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&writes);
    let executor = LocalThemeRecipeExecutor::with_test_control(fixture.roots(), move |point| {
        if let Checkpoint::BeforeOutput(artifact) = point {
            observed.lock().unwrap().push(artifact);
        }
    });
    let request = fixture.request();
    let first = executor
        .execute(&request, &plan, &CancellationToken::new())
        .expect("first staging");
    let first_write_count = writes.lock().unwrap().len();
    let theme_modified = fs::metadata(fixture.staging.join("theme.json"))
        .expect("theme metadata")
        .modified()
        .expect("theme modified time");

    let second = executor
        .execute(&request, &plan, &CancellationToken::new())
        .expect("idempotent completed repeat");
    assert_eq!(second, first);
    assert_eq!(writes.lock().unwrap().len(), first_write_count);
    assert_eq!(
        fs::metadata(fixture.staging.join("theme.json"))
            .expect("repeat theme metadata")
            .modified()
            .expect("repeat modified time"),
        theme_modified
    );

    let conflict = StagingRequest::new("theme-op-0002", "theme-idempotency-0002").unwrap();
    assert_eq!(
        executor
            .execute(&conflict, &plan, &CancellationToken::new())
            .expect_err("different operation cannot adopt completed root")
            .code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn cancellation_is_checkpointed_and_same_request_resumes_create_only() {
    let fixture = Fixture::new();
    let plan = fixture.plan(RecipeId::TvWorld);
    let request = fixture.request();
    let cancellation = CancellationToken::new();
    let cancel_from_callback = cancellation.clone();
    let executor = LocalThemeRecipeExecutor::with_test_control(fixture.roots(), move |point| {
        if point == Checkpoint::BeforeOutput(OutputArtifact::BackgroundPng) {
            cancel_from_callback.cancel();
        }
    });
    assert_eq!(
        executor
            .execute(&request, &plan, &cancellation)
            .expect_err("checkpoint cancellation")
            .code,
        ErrorCode::Cancelled
    );
    assert_eq!(
        entry_names(&fixture.staging),
        [STAGING_INTENT_FILE, "theme.json"]
    );
    let theme_before = fs::read(fixture.staging.join("theme.json")).expect("partial theme");

    let writes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&writes);
    let resumed = LocalThemeRecipeExecutor::with_test_control(fixture.roots(), move |point| {
        if let Checkpoint::BeforeOutput(artifact) = point {
            observed.lock().unwrap().push(artifact);
        }
    });
    resumed
        .execute(&request, &plan, &CancellationToken::new())
        .expect("resume same immutable intent");
    assert_eq!(
        fs::read(fixture.staging.join("theme.json")).expect("resumed theme"),
        theme_before
    );
    assert_eq!(
        *writes.lock().unwrap(),
        [
            OutputArtifact::BackgroundPng,
            OutputArtifact::MusicOgg,
            OutputArtifact::ProvenanceJson,
        ]
    );
}

#[test]
fn source_change_and_untrusted_partial_state_fail_closed_without_live_mutation() {
    let fixture = Fixture::new();
    let plan = fixture.plan(RecipeId::TheKnight);
    fs::write(fixture.source("background.png"), synthetic_png(9)).expect("change source");
    assert_eq!(
        fixture
            .executor()
            .execute(&fixture.request(), &plan, &CancellationToken::new())
            .expect_err("planned source hash changed")
            .code,
        ErrorCode::SourceChanged
    );
    fixture.assert_live_tree_unchanged();

    let other = Fixture::new();
    fs::write(other.staging.join("foreign.txt"), b"foreign").expect("foreign staging entry");
    assert_eq!(
        other
            .executor()
            .execute(
                &other.request(),
                &other.plan(RecipeId::CardCastle),
                &CancellationToken::new(),
            )
            .expect_err("unbound partial state")
            .code,
        ErrorCode::IncompleteStaging
    );
    assert_eq!(
        fs::read(other.staging.join("foreign.txt")).expect("foreign entry preserved"),
        b"foreign"
    );
    other.assert_live_tree_unchanged();
}

#[test]
fn same_operation_root_uses_one_process_shared_lock() {
    let fixture = Fixture::new();
    let worker_roots = fixture.roots();
    let contender = fixture.executor();
    let plan = fixture.plan(RecipeId::CardCastle);
    let contender_plan = plan.clone();
    let request = fixture.request();
    let contender_request = request.clone();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let entered_worker = Arc::clone(&entered);
    let release_worker = Arc::clone(&release);
    let handle = thread::spawn(move || {
        let executor = LocalThemeRecipeExecutor::with_test_control(worker_roots, move |point| {
            if point == Checkpoint::LockHeld {
                entered_worker.wait();
                release_worker.wait();
            }
        });
        executor.execute(&request, &plan, &CancellationToken::new())
    });
    entered.wait();
    let conflict = contender
        .execute(
            &contender_request,
            &contender_plan,
            &CancellationToken::new(),
        )
        .expect_err("shared root lock must reject contender");
    release.wait();
    assert_eq!(conflict.code, ErrorCode::Busy);
    handle
        .join()
        .expect("staging worker")
        .expect("first operation");
}

#[cfg(unix)]
#[test]
fn operation_root_must_be_private_and_no_follow() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temp = tempfile::tempdir().unwrap();
    let broad = temp.path().join("broad");
    fs::create_dir(&broad).unwrap();
    fs::set_permissions(&broad, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        HostSelectedStagingRoot::validate(&broad)
            .expect_err("group-readable root rejected")
            .code,
        ErrorCode::RootNotPrivate
    );

    let target = temp.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
    let link = temp.path().join("linked");
    symlink(&target, &link).unwrap();
    assert_eq!(
        HostSelectedStagingRoot::validate(&link)
            .expect_err("linked root rejected")
            .code,
        ErrorCode::InvalidHostRoot
    );
}

#[test]
fn hardlinked_source_is_rejected_by_staging_boundary() {
    let fixture = Fixture::new();
    let plan = fixture.plan(RecipeId::Noelle);
    fs::hard_link(
        fixture.source("background.png"),
        fixture.game_root.join("background-hardlink.png"),
    )
    .expect("source hardlink");
    assert_eq!(
        fixture
            .executor()
            .execute(&fixture.request(), &plan, &CancellationToken::new())
            .expect_err("hardlinked source rejected")
            .code,
        ErrorCode::SourceHardlinked
    );
}

#[test]
fn live_theme_tree_names_cannot_be_selected_as_operation_roots() {
    let temp = tempfile::tempdir().unwrap();
    for name in ["customThemes", "THEMES"] {
        let root = temp.path().join(name);
        fs::create_dir(&root).unwrap();
        make_private(&root);
        assert_eq!(
            HostSelectedStagingRoot::validate(&root)
                .expect_err("live theme tree name rejected")
                .code,
            ErrorCode::InvalidHostRoot
        );
    }
}

#[cfg(windows)]
#[test]
fn canonical_live_theme_tree_alias_cannot_be_selected_as_operation_root() {
    let temp = tempfile::tempdir().unwrap();
    let live_tree = temp.path().join("customThemes");
    let operation_root = live_tree.join("private-operation-root");
    fs::create_dir_all(&operation_root).unwrap();
    make_private(&operation_root);

    let alias = temp
        .path()
        .join("customThemes.")
        .join("private-operation-root");
    assert_eq!(
        fs::canonicalize(&alias).expect("Windows normalization alias resolves"),
        fs::canonicalize(&operation_root).expect("live-tree operation root resolves")
    );
    assert_eq!(
        HostSelectedStagingRoot::validate(&alias)
            .expect_err("canonical live-tree alias rejected")
            .code,
        ErrorCode::InvalidHostRoot
    );
}

#[cfg(windows)]
#[test]
fn alternate_data_streams_are_rejected_on_every_selected_root() {
    let fixture = Fixture::new();
    write_named_stream(&fixture.staging, "audit", b"staging root ADS");
    assert_eq!(
        HostSelectedStagingRoot::validate(&fixture.staging)
            .expect_err("staging root ADS rejected")
            .code,
        ErrorCode::NamedStreams
    );

    let source_fixture = Fixture::new();
    let plan = source_fixture.plan(RecipeId::Noelle);
    write_named_stream(&source_fixture.game_root, "audit", b"source root ADS");
    assert_eq!(
        source_fixture
            .executor()
            .execute(&source_fixture.request(), &plan, &CancellationToken::new(),)
            .expect_err("selected source root ADS rejected")
            .code,
        ErrorCode::NamedStreams
    );
}

#[test]
fn checked_in_runtime_tree_contains_no_extracted_assets_or_custom_theme_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("inspect runtime tree") {
            let entry = entry.expect("runtime entry");
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("runtime metadata");
            if metadata.is_dir() {
                assert_ne!(
                    entry.file_name().to_string_lossy().to_ascii_lowercase(),
                    "customthemes"
                );
                if entry.file_name() != "target" {
                    pending.push(path);
                }
            } else if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
                assert!(
                    !matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "png" | "ogg" | "wav"
                    ),
                    "checked-in media residue: {path:?}"
                );
            }
        }
    }
}

fn make_private(path: &Path) {
    #[cfg(windows)]
    fence_windows::harden_private_directory(path).expect("harden test operation root");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .expect("private test operation root mode");
    }
}

fn entry_names(directory: &Path) -> Vec<String> {
    let mut names = fs::read_dir(directory)
        .expect("list directory")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[cfg(windows)]
fn write_named_stream(path: &Path, stream: &str, bytes: &[u8]) {
    use std::io::Write as _;
    let stream_path = format!("{}:{stream}", path.display());
    let mut file = fs::File::create(stream_path).expect("create named stream fixture");
    file.write_all(bytes).expect("write named stream fixture");
    file.sync_all().expect("sync named stream fixture");
}

fn synthetic_png(seed: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&1_u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    append_png_chunk(&mut bytes, b"IHDR", &ihdr);
    let scanline = [0_u8, seed, seed.wrapping_add(1), seed.wrapping_add(2), 0xff];
    let mut compressed = vec![0x78, 0x01, 0x01];
    let length = u16::try_from(scanline.len()).expect("small PNG scanline");
    compressed.extend_from_slice(&length.to_le_bytes());
    compressed.extend_from_slice(&(!length).to_le_bytes());
    compressed.extend_from_slice(&scanline);
    compressed.extend_from_slice(&adler32(&scanline).to_be_bytes());
    append_png_chunk(&mut bytes, b"IDAT", &compressed);
    append_png_chunk(&mut bytes, b"IEND", &[]);
    bytes
}

fn synthetic_ogg(seed: u8) -> Vec<u8> {
    let payload = [seed; 16];
    let mut page = vec![0_u8; 28];
    page[..4].copy_from_slice(b"OggS");
    page[5] = 0x06;
    page[14..18].copy_from_slice(&0x4531_0001_u32.to_le_bytes());
    page[26] = 1;
    page[27] = u8::try_from(payload.len()).expect("small Ogg packet");
    page.extend_from_slice(&payload);
    let checksum = ogg_checksum(&page);
    page[22..26].copy_from_slice(&checksum.to_le_bytes());
    page
}

fn append_png_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], payload: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(payload.len())
            .expect("synthetic PNG chunk length")
            .to_be_bytes(),
    );
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(payload);
    output.extend_from_slice(&png_crc(chunk_type, payload).to_be_bytes());
}

fn png_crc(chunk_type: &[u8; 4], payload: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in chunk_type.iter().chain(payload) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                (crc >> 1) ^ 0xedb8_8320
            };
        }
    }
    !crc
}

fn ogg_checksum(bytes: &[u8]) -> u32 {
    let mut checksum = 0_u32;
    for byte in bytes {
        checksum ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            checksum = if checksum & 0x8000_0000 == 0 {
                checksum << 1
            } else {
                (checksum << 1) ^ 0x04c1_1db7
            };
        }
    }
    checksum
}

fn adler32(bytes: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    let mut first = 1_u32;
    let mut second = 0_u32;
    for byte in bytes {
        first = (first + u32::from(*byte)) % MODULUS;
        second = (second + first) % MODULUS;
    }
    (second << 16) | first
}
