use deltamod_product_contracts::{
    LifecycleOperationKind, OperationPhase, OperationState, ProductErrorCode, ProviderAccountState,
    ProviderArtifactKind, ProviderAuthentication, ProviderCapability, ProviderItemKind,
    RecoveryAction,
};
use deltamod_provider_platform::{
    gamebanana, local, map_legacy_source, moddb, nexus, normalize_provider_error,
    provider_descriptors, CapabilityAvailability, DownloadProgressTracker, KnownProvider,
    ProviderCancellationToken, ProviderErrorInput, ProviderFailureKind, SearchRequest, SearchSort,
};
use serde_json::json;

const GAMEBANANA_SEARCH: &[u8] = include_bytes!("fixtures/gamebanana-search.json");
const GAMEBANANA_DETAIL: &[u8] = include_bytes!("fixtures/gamebanana-detail.json");
const GAMEBANANA_ACCOUNT: &[u8] = include_bytes!("fixtures/gamebanana-account.json");
const NEXUS_SEARCH: &[u8] = include_bytes!("fixtures/nexus-search.json");
const NEXUS_MOD: &[u8] = include_bytes!("fixtures/nexus-mod.json");
const NEXUS_FILES: &[u8] = include_bytes!("fixtures/nexus-files.json");
const NEXUS_DOWNLOAD_LINKS: &[u8] = include_bytes!("fixtures/nexus-download-links.json");
const NEXUS_STATUS: &[u8] = include_bytes!("fixtures/nexus-status.json");
const MODDB_CATALOG: &[u8] = include_bytes!("fixtures/moddb-catalog.json");
const LOCAL_ARCHIVE: &[u8] = include_bytes!("fixtures/local-archive.json");
const MALFORMED: &[u8] = include_bytes!("fixtures/malformed.json");

fn request(provider: KnownProvider, scope: &str) -> SearchRequest {
    SearchRequest::new(provider, scope, "", SearchSort::LatestAdded, 0, 50).unwrap()
}

fn serialized<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap()
}

fn assert_sha256_identity(value: &str) {
    assert_eq!(value.len(), 64);
    assert!(value.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

fn assert_public_source(source: &deltamod_product_contracts::ProviderRef) {
    let url = source
        .canonical_url()
        .expect("remote providers retain a source URL");
    assert!(url.starts_with("https://"));
    assert!(!url.contains('@'));
    assert!(!url.contains('?'));
    assert!(!url.contains('#'));
}

#[test]
fn request_error_and_download_debug_views_never_render_secrets() {
    const SEARCH_SECRET: &str = "SEARCH_CREDENTIAL_7f19f0";
    const SCOPE_SECRET: &str = "transient-scope-probe-7f19";
    let search = SearchRequest::new(
        KnownProvider::Nexus,
        SCOPE_SECRET,
        SEARCH_SECRET,
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    let search_debug = format!("{search:?}");
    assert!(!search_debug.contains(SEARCH_SECRET));
    assert!(!search_debug.contains(SCOPE_SECRET));
    let cache = search.cache_identity(KnownProvider::Nexus);
    assert_sha256_identity(cache.as_str());
    assert!(!cache.as_str().contains(SEARCH_SECRET));
    assert!(!cache.as_str().contains(SCOPE_SECRET));
    assert!(!format!("{cache:?}").contains(SEARCH_SECRET));
    assert!(!format!("{cache:?}").contains(SCOPE_SECRET));
    assert!(!serialized(&cache).contains(SEARCH_SECRET));
    assert!(!serialized(&cache).contains(SCOPE_SECRET));

    let page_request = SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        SEARCH_SECRET,
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    let stable_page = nexus::map_search(NEXUS_SEARCH, &page_request).unwrap();
    let stable_page_json = serde_json::to_value(&stable_page).unwrap();
    assert!(stable_page_json.get("scope").is_none());
    assert_sha256_identity(
        stable_page_json
            .get("scopeDigest")
            .and_then(serde_json::Value::as_str)
            .unwrap(),
    );
    assert!(!serialized(&stable_page).contains(SEARCH_SECRET));

    let stable_scope_request = SearchRequest::new(
        KnownProvider::ModDb,
        SCOPE_SECRET,
        "",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    let stable_scope_page = moddb::map_search(MODDB_CATALOG, &stable_scope_request).unwrap();
    assert!(!serialized(&stable_scope_page).contains(SCOPE_SECRET));
    assert!(!format!("{stable_scope_page:?}").contains(SCOPE_SECRET));

    // SearchRequest deliberately has no Serialize implementation. Its compile-fail contract is
    // exercised by the crate-level documentation test; only digested derivatives are stable.
    assert!(SearchRequest::new(
        KnownProvider::Nexus,
        "token-supersecret/private-game-key",
        "ordinary search",
        SearchSort::Relevance,
        0,
        25,
    )
    .is_err());
    assert!(SearchRequest::new(
        KnownProvider::Local,
        "local",
        "ordinary search",
        SearchSort::Relevance,
        0,
        25,
    )
    .is_err());
    let wrong_provider = SearchRequest::new(
        KnownProvider::ModDb,
        "deltarune",
        "",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    assert!(nexus::map_search(NEXUS_SEARCH, &wrong_provider).is_err());
    assert!(SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        " ".repeat(257),
        SearchSort::Relevance,
        0,
        25,
    )
    .is_err());
    assert!(SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        "https://alice:URL_PASSWORD_SECRET@example.com/mod",
        SearchSort::Relevance,
        0,
        25,
    )
    .is_err());
    assert!(SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        "https://example.com/mod?access_token=URL_QUERY_SECRET",
        SearchSort::Relevance,
        0,
        25,
    )
    .is_err());
    assert!(SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        "https://example.com/mod?topic=ordinary",
        SearchSort::Relevance,
        0,
        25,
    )
    .is_ok());

    let mut error_input = ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Http);
    error_input.raw_message = Some("RAW_ERROR_SECRET_d310d8");
    error_input.operation_id = Some("operation-safe-id");
    let input_debug = format!("{error_input:?}");
    assert!(!input_debug.contains("RAW_ERROR_SECRET_d310d8"));
    assert!(!input_debug.contains("operation-safe-id"));

    let details = nexus::map_details(NEXUS_MOD, NEXUS_FILES, "deltarune").unwrap();
    let links = br#"[{
        "URI":"https://cdn.nexus-cdn.com/PATH_SECRET_84dd2f.zip?token=QUERY_SECRET_05b1b0"
    }]"#;
    let download = nexus::resolve_download(
        &details.versions()[0],
        links,
        ProviderAccountState::SignedIn,
    )
    .unwrap();
    let redacted = download.url().redacted();
    let url_debug = format!("{:?}", download.url());
    for secret in ["PATH_SECRET_84dd2f", "QUERY_SECRET_05b1b0"] {
        assert!(!redacted.contains(secret));
        assert!(!url_debug.contains(secret));
    }
    assert_eq!(redacted, "https://cdn.nexus-cdn.com/<redacted>");
    assert!(!redacted.contains('?'));
    assert!(!redacted.contains('#'));
    assert!(!redacted.contains('@'));

    let rejected_links = br#"[{
        "URI":"https://url-user:URL_PASSWORD_SECRET@cdn.nexus-cdn.com/file.zip#FRAGMENT_SECRET"
    }]"#;
    let failure = nexus::resolve_download(
        &details.versions()[0],
        rejected_links,
        ProviderAccountState::SignedIn,
    )
    .unwrap_err();
    let failure_debug = format!("{failure:?}");
    assert!(!failure_debug.contains("URL_PASSWORD_SECRET"));
    assert!(!failure_debug.contains("FRAGMENT_SECRET"));
}

#[test]
fn external_images_are_omitted_from_stable_provider_data() {
    const QUERY_SECRET: &str = "QUERY_IMAGE_SECRET_91a7";
    const FRAGMENT_SECRET: &str = "FRAGMENT_IMAGE_SECRET_91a7";
    const USERINFO_SECRET: &str = "USERINFO_IMAGE_SECRET_91a7";
    const PATH_SECRET: &str = "PATH_SIGNED_SECRET_91a7";
    let payload = format!(
        r#"[
            {{"modId":901,"gameDomainName":"deltarune","name":"Query","pictureUrl":"https://images.nexusmods.com/image.png?token={QUERY_SECRET}"}},
            {{"modId":902,"gameDomainName":"deltarune","name":"Fragment","pictureUrl":"https://images.nexusmods.com/image.png#{FRAGMENT_SECRET}"}},
            {{"modId":903,"gameDomainName":"deltarune","name":"Userinfo","pictureUrl":"https://{USERINFO_SECRET}:password@images.nexusmods.com/image.png"}},
            {{"modId":904,"gameDomainName":"deltarune","name":"Path","pictureUrl":"https://images.nexusmods.com/{PATH_SECRET}/image.png"}}
        ]"#
    );
    let request = request(KnownProvider::Nexus, "deltarune");
    let page = nexus::map_search(payload.as_bytes(), &request).unwrap();
    assert_eq!(page.items().len(), 4);
    assert!(page.items().iter().all(|item| item.images().is_empty()));

    for provider in [
        KnownProvider::GameBanana,
        KnownProvider::Nexus,
        KnownProvider::ModDb,
    ] {
        assert!(!provider
            .descriptor()
            .capabilities
            .contains(&ProviderCapability::Images));
    }

    let stable_json = serialized(&page);
    let stable_debug = format!("{page:?}");
    let cache_json = serialized(page.cache_identity());
    for secret in [QUERY_SECRET, FRAGMENT_SECRET, USERINFO_SECRET, PATH_SECRET] {
        assert!(!stable_json.contains(secret));
        assert!(!stable_debug.contains(secret));
        assert!(!cache_json.contains(secret));
    }
}

#[test]
fn search_scope_grammars_are_provider_specific_and_bounded() {
    let gamebanana = SearchRequest::new(
        KnownProvider::GameBanana,
        "0006755",
        "",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    assert_eq!(gamebanana.scope(), "6755");
    assert!(SearchRequest::new(
        KnownProvider::GameBanana,
        "game-6755",
        "",
        SearchSort::Relevance,
        0,
        25,
    )
    .is_err());

    for provider in [KnownProvider::Nexus, KnownProvider::ModDb] {
        let request =
            SearchRequest::new(provider, " DeltaRune ", "", SearchSort::Relevance, 0, 25).unwrap();
        assert_eq!(request.scope(), "deltarune");
        for invalid in [
            "deltarune/private-key",
            "deltarune_private",
            "deltarune.private",
        ] {
            assert!(
                SearchRequest::new(provider, invalid, "", SearchSort::Relevance, 0, 25,).is_err()
            );
        }
        assert!(
            SearchRequest::new(provider, "x".repeat(81), "", SearchSort::Relevance, 0, 25,)
                .is_err()
        );
    }

    assert!(nexus::map_details(NEXUS_MOD, NEXUS_FILES, &"x".repeat(81)).is_err());
    assert!(moddb::map_details(MODDB_CATALOG, &"x".repeat(81)).is_err());
}

#[test]
fn descriptors_and_runtime_reports_are_truthful() {
    let descriptors = provider_descriptors();
    let ids = descriptors
        .iter()
        .map(|descriptor| descriptor.provider_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        ["gamebanana", "gamejolt", "itch", "moddb", "nexus", "local"]
    );
    assert_eq!(
        KnownProvider::from_id("gamejolt"),
        Some(KnownProvider::GameJolt)
    );
    assert_eq!(KnownProvider::from_id("itch"), Some(KnownProvider::Itch));

    let gamebanana = KnownProvider::GameBanana.descriptor();
    assert_eq!(gamebanana.authentication, ProviderAuthentication::Optional);
    assert!(gamebanana
        .capabilities
        .contains(&ProviderCapability::Search));
    assert!(gamebanana
        .capabilities
        .contains(&ProviderCapability::DirectDownload));
    assert!(!gamebanana
        .capabilities
        .contains(&ProviderCapability::RateLimit));

    let moddb = KnownProvider::ModDb.descriptor();
    assert_eq!(moddb.authentication, ProviderAuthentication::None);
    assert!(moddb
        .capabilities
        .contains(&ProviderCapability::ExternalDownload));
    assert!(!moddb.capabilities.contains(&ProviderCapability::Versions));
    assert!(!moddb.capabilities.contains(&ProviderCapability::Search));
    assert!(!moddb.capabilities.contains(&ProviderCapability::Details));
    assert!(!moddb
        .capabilities
        .contains(&ProviderCapability::DirectDownload));

    let nexus = KnownProvider::Nexus.descriptor();
    assert_eq!(nexus.authentication, ProviderAuthentication::Optional);
    assert!(nexus.capabilities.contains(&ProviderCapability::RateLimit));
    assert!(!nexus
        .capabilities
        .contains(&ProviderCapability::UpdateOrdering));
    let signed_out = KnownProvider::Nexus.capability_report(ProviderAccountState::SignedOut);
    assert_eq!(
        signed_out.direct_download,
        CapabilityAvailability::AuthenticationRequired
    );
    assert_eq!(
        signed_out.versions,
        CapabilityAvailability::AuthenticationRequired
    );
    assert_eq!(
        signed_out.external_download,
        CapabilityAvailability::Available
    );
    let signed_in = KnownProvider::Nexus.capability_report(ProviderAccountState::SignedIn);
    assert_eq!(signed_in.direct_download, CapabilityAvailability::Available);
    assert_eq!(signed_in.versions, CapabilityAvailability::Available);

    let local = KnownProvider::Local.descriptor();
    assert_eq!(local.authentication, ProviderAuthentication::None);
    assert!(local
        .capabilities
        .contains(&ProviderCapability::ProviderInstall));
    assert!(!local.capabilities.contains(&ProviderCapability::Search));
    let local_report = KnownProvider::Local.capability_report(ProviderAccountState::NotRequired);
    assert_eq!(local_report.local_import, CapabilityAvailability::Available);
    assert_eq!(local_report.search, CapabilityAvailability::Unavailable);
    assert_eq!(
        local_report.direct_download,
        CapabilityAvailability::Unavailable
    );
}

#[test]
fn gamebanana_maps_legacy_payloads_and_deduplicates_identity() {
    let page = gamebanana::map_search(
        GAMEBANANA_SEARCH,
        &request(KnownProvider::GameBanana, "6755"),
    )
    .unwrap();
    assert_eq!(page.provider(), KnownProvider::GameBanana);
    assert_eq!(page.items().len(), 1);
    assert_eq!(page.duplicate_count(), 1);
    assert!(page.has_more());
    assert_eq!(page.total_count(), Some(3));
    let item = &page.items()[0];
    assert_eq!(item.title(), "Fixture Mod");
    assert_eq!(item.summary(), Some("A fixture & description."));
    assert_eq!(item.source().provider_id().as_str(), "gamebanana");
    assert_eq!(item.source().resource_id().as_str(), "42");
    assert_eq!(item.source().scope().unwrap().as_str(), "mod");
    assert_public_source(item.source());
    assert_eq!(
        item.source().canonical_url(),
        Some("https://gamebanana.com/mods/42")
    );
    let page_json = serialized(&page);
    assert!(!page_json.contains("password"));
    assert!(!page_json.contains("do-not-persist"));

    let details = gamebanana::map_details(GAMEBANANA_DETAIL).unwrap();
    assert_eq!(details.versions().len(), 2);
    assert_eq!(details.duplicate_version_count(), 1);
    let current = &details.versions()[0];
    assert_eq!(current.label(), "2.0.0");
    assert_eq!(current.source().artifact_id().unwrap().as_str(), "1001");
    assert_eq!(current.source().version_id().unwrap().as_str(), "2.0.0");
    assert_eq!(current.sha256(), Some("a".repeat(64).as_str()));
    assert!(current.directly_downloadable());
    assert_eq!(
        current.source().canonical_identity(),
        details.item().source().canonical_identity()
    );
    let old = &details.versions()[1];
    assert_eq!(old.label(), "1.0 beta");
    assert_eq!(old.source().version_id().unwrap().as_str(), "1002");
    assert_eq!(old.sha256(), Some("b".repeat(64).as_str()));
    assert!(!old.directly_downloadable());

    let resolution = gamebanana::resolve_download(GAMEBANANA_DETAIL, current.source()).unwrap();
    assert!(resolution.url().expose().contains("/mmdl/1001"));
    assert!(resolution.url().expose().contains("gamebanana-secret"));
    let debug = format!("{resolution:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("gamebanana-secret"));
    let details_json = serialized(&details);
    assert!(!details_json.contains("gamebanana-secret"));
    assert!(!details_json.contains("_sDownloadUrl"));
}

#[test]
fn nexus_always_preserves_scoped_source_identity() {
    let page =
        nexus::map_search(NEXUS_SEARCH, &request(KnownProvider::Nexus, "DeltaRune")).unwrap();
    assert_eq!(page.items().len(), 2);
    assert_eq!(page.duplicate_count(), 1);
    assert!(page.has_more());
    assert_eq!(page.total_count(), Some(100));
    assert_eq!(page.items()[0].source().resource_id().as_str(), "23");
    assert!(page.items()[0].featured());
    for item in page.items() {
        assert_eq!(item.source().provider_id().as_str(), "nexus");
        assert_eq!(item.source().scope().unwrap().as_str(), "deltarune");
        assert_public_source(item.source());
    }
    let page_json = serialized(&page);
    assert!(!page_json.contains("image-secret"));

    let details = nexus::map_details(NEXUS_MOD, NEXUS_FILES, "deltarune").unwrap();
    assert_eq!(details.versions().len(), 2);
    assert_eq!(details.duplicate_version_count(), 1);
    assert_eq!(details.item().source().provider_id().as_str(), "nexus");
    assert_public_source(details.item().source());
    for version in details.versions() {
        assert_eq!(version.source().provider_id().as_str(), "nexus");
        assert_eq!(version.source().scope().unwrap().as_str(), "deltarune");
        assert_public_source(version.source());
        assert_eq!(
            version.source().canonical_identity(),
            details.item().source().canonical_identity()
        );
    }
    let current = &details.versions()[0];
    assert_eq!(current.source().artifact_id().unwrap().as_str(), "99");
    assert_eq!(current.source().version_id().unwrap().as_str(), "2.0.0");
    assert_eq!(current.sha256(), Some("c".repeat(64).as_str()));
    assert!(current.directly_downloadable());
    let old = &details.versions()[1];
    assert_eq!(old.label(), "1.0 beta");
    assert_eq!(old.source().version_id().unwrap().as_str(), "98");
    assert!(!old.directly_downloadable());

    let auth_error = nexus::resolve_download(
        current,
        NEXUS_DOWNLOAD_LINKS,
        ProviderAccountState::SignedOut,
    )
    .unwrap_err();
    assert_eq!(
        auth_error.contract().code,
        ProductErrorCode::AuthenticationRequired
    );
    let resolution = nexus::resolve_download(
        current,
        NEXUS_DOWNLOAD_LINKS,
        ProviderAccountState::SignedIn,
    )
    .unwrap();
    assert!(resolution.url().expose().contains("nexus-secret"));
    assert_eq!(resolution.source().scope().unwrap().as_str(), "deltarune");
    let debug = format!("{resolution:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("nexus-secret"));
    let details_json = serialized(&details);
    assert!(!details_json.contains("nexus-secret"));
    assert!(!details_json.contains("URI"));
}

#[test]
fn moddb_reports_external_only_and_uses_canonical_source_paths() {
    let page =
        moddb::map_search(MODDB_CATALOG, &request(KnownProvider::ModDb, "deltarune")).unwrap();
    assert_eq!(page.items().len(), 1);
    assert_eq!(page.duplicate_count(), 1);
    assert!(!page.has_more());
    let source = page.items()[0].source();
    assert_eq!(source.provider_id().as_str(), "moddb");
    assert_eq!(source.resource_id().as_str(), "gendered-kris");
    assert_eq!(
        source.scope().unwrap().as_str(),
        "games.deltarune.downloads"
    );
    assert_eq!(
        source.canonical_url(),
        Some("https://www.moddb.com/games/deltarune/downloads/gendered-kris")
    );
    assert_public_source(source);
    assert!(!serialized(&page).contains("remove-me"));

    assert!(moddb::map_details(MODDB_CATALOG, "deltarune").is_err());
    let exact_item = br#"{
        "title":"Gendered Kris",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/gendered-kris"
    }"#;
    let details = moddb::map_details(exact_item, "deltarune").unwrap();
    assert!(details.versions().is_empty());
    let report = KnownProvider::ModDb.capability_report(ProviderAccountState::NotRequired);
    assert_eq!(report.search, CapabilityAvailability::Unavailable);
    assert_eq!(report.details, CapabilityAvailability::Unavailable);
    assert_eq!(report.direct_download, CapabilityAvailability::Unavailable);
}

#[test]
fn moddb_category_is_part_of_current_and_legacy_identity() {
    let payload = br#"{
        "items":[
            {
                "title":"Shared download",
                "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/shared"
            },
            {
                "title":"Shared mod",
                "sourceUrl":"https://www.moddb.com/games/deltarune/mods/shared"
            }
        ]
    }"#;
    let page = moddb::map_search(payload, &request(KnownProvider::ModDb, "deltarune")).unwrap();
    assert_eq!(page.items().len(), 2);
    assert_eq!(page.duplicate_count(), 0);
    assert_eq!(
        page.items()[0].source().scope().unwrap().as_str(),
        "games.deltarune.downloads"
    );
    assert_eq!(
        page.items()[1].source().scope().unwrap().as_str(),
        "games.deltarune.mods"
    );
    assert_ne!(
        page.items()[0].source().canonical_identity(),
        page.items()[1].source().canonical_identity()
    );

    let download_legacy = br#"{
        "source":{
            "provider":"moddb",
            "url":"https://www.moddb.com/games/deltarune/downloads/shared"
        }
    }"#;
    let mod_legacy = br#"{
        "source":{
            "provider":"moddb",
            "url":"https://www.moddb.com/games/deltarune/mods/shared"
        }
    }"#;
    let download_source = map_legacy_source(download_legacy, Some(&"a".repeat(64))).unwrap();
    let mod_source = map_legacy_source(mod_legacy, Some(&"b".repeat(64))).unwrap();
    assert_ne!(
        download_source.source().canonical_identity(),
        mod_source.source().canonical_identity()
    );
}

#[test]
fn local_archives_use_computed_hash_as_stable_identity() {
    let uppercase_hash = "D".repeat(64);
    let expected_hash = "d".repeat(64);
    let details = local::map_archive(LOCAL_ARCHIVE, &uppercase_hash).unwrap();
    let source = details.item().source();
    assert_eq!(source.provider_id().as_str(), "local");
    assert_eq!(source.item_kind(), ProviderItemKind::LocalArchive);
    assert_eq!(source.resource_id().as_str(), expected_hash);
    assert_eq!(source.artifact_id().unwrap().as_str(), expected_hash);
    assert_eq!(source.artifact_kind(), ProviderArtifactKind::Archive);
    assert!(source.scope().is_none());
    assert!(source.canonical_url().is_none());
    assert_eq!(source.version_id().unwrap().as_str(), expected_hash);
    assert_eq!(details.versions()[0].label(), "1.2 beta");
    assert_eq!(details.versions()[0].sha256(), Some(expected_hash.as_str()));
    assert!(!details.versions()[0].directly_downloadable());

    let renamed = br#"{"metadata":{"name":"Renamed local fixture","version":"9 beta"}}"#;
    let same_archive = local::map_archive(renamed, &uppercase_hash).unwrap();
    assert_eq!(
        same_archive.item().source().canonical_identity(),
        source.canonical_identity()
    );
    let other_archive = local::map_archive(renamed, &"e".repeat(64)).unwrap();
    assert_ne!(
        other_archive.item().source().canonical_identity(),
        source.canonical_identity()
    );
    let details_json = serialized(&details);
    assert!(details_json.contains(&expected_hash));
    assert!(!details_json.contains(&"f".repeat(64)));
}

#[test]
fn legacy_mapping_preserves_provider_version_source_and_hash() {
    let archive_hash = "a".repeat(64);
    let nexus_payload = serde_json::to_vec(&json!({
        "source": {
            "provider": "nexus",
            "id": 42,
            "fileId": 99,
            "url": "https://www.nexusmods.com/deltarune/mods/42?token=discard-me"
        },
        "archiveSha256": archive_hash,
        "version": "2.0.0"
    }))
    .unwrap();
    let nexus_source = map_legacy_source(&nexus_payload, None).unwrap();
    assert_eq!(nexus_source.source().provider_id().as_str(), "nexus");
    assert_eq!(nexus_source.source().scope().unwrap().as_str(), "deltarune");
    assert_eq!(nexus_source.source().artifact_id().unwrap().as_str(), "99");
    assert_eq!(
        nexus_source.source().version_id().unwrap().as_str(),
        "2.0.0"
    );
    assert_eq!(nexus_source.archive_sha256(), Some("a".repeat(64).as_str()));
    assert_eq!(nexus_source.version(), Some("2.0.0"));
    assert_public_source(nexus_source.source());
    assert!(!serialized(&nexus_source).contains("discard-me"));

    let gamebanana_payload = br#"{
        "gamebanana":{"supports":true,"id":42,"model":"Mod","fileId":1001},
        "version":"2.0.0"
    }"#;
    let gamebanana_source = map_legacy_source(gamebanana_payload, Some(&"b".repeat(64))).unwrap();
    assert_eq!(
        gamebanana_source.source().provider_id().as_str(),
        "gamebanana"
    );
    assert_eq!(gamebanana_source.source().scope().unwrap().as_str(), "mod");
    assert_eq!(
        gamebanana_source.source().artifact_id().unwrap().as_str(),
        "1001"
    );
    assert_public_source(gamebanana_source.source());

    let moddb_payload = br#"{
        "source":{
            "provider":"moddb",
            "url":"https://www.moddb.com/games/deltarune/downloads/gendered-kris"
        }
    }"#;
    let moddb_source = map_legacy_source(moddb_payload, Some(&"c".repeat(64))).unwrap();
    assert_eq!(moddb_source.source().provider_id().as_str(), "moddb");
    assert_eq!(
        moddb_source.source().scope().unwrap().as_str(),
        "games.deltarune.downloads"
    );
    assert_public_source(moddb_source.source());

    let local_source = map_legacy_source(b"{}", Some(&"d".repeat(64))).unwrap();
    assert_eq!(local_source.source().provider_id().as_str(), "local");
    assert_eq!(local_source.source().resource_id().as_str(), "d".repeat(64));
    assert!(local_source.source().canonical_url().is_none());
}

#[test]
fn legacy_mapping_rejects_ambiguous_provider_markers() {
    let nexus_and_gamebanana = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "fileId":99,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        },
        "gamebanana":{"supports":true,"id":42,"model":"Mod"}
    }"#;
    let failure = map_legacy_source(nexus_and_gamebanana, Some(&"a".repeat(64))).unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);

    let mismatched_gamebanana = br#"{
        "source":{
            "provider":"gamebanana",
            "id":42,
            "url":"https://gamebanana.com/mods/42"
        },
        "gamebanana":{"supports":true,"id":43,"model":"Mod"}
    }"#;
    assert!(map_legacy_source(mismatched_gamebanana, Some(&"a".repeat(64))).is_err());

    let consistent_gamebanana = br#"{
        "source":{
            "provider":"gamebanana",
            "id":42,
            "fileId":1001,
            "url":"https://gamebanana.com/mods/42"
        },
        "gamebanana":{"supports":true,"id":42,"model":"Mod"}
    }"#;
    let mapped = map_legacy_source(consistent_gamebanana, Some(&"a".repeat(64))).unwrap();
    assert_eq!(mapped.source().provider_id().as_str(), "gamebanana");
    assert_eq!(mapped.source().resource_id().as_str(), "42");

    let conflicting_nexus_ids = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "modId":43,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    assert!(map_legacy_source(conflicting_nexus_ids, Some(&"a".repeat(64))).is_err());

    let conflicting_artifact_ids = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "fileId":99,
            "file_id":100,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    assert!(map_legacy_source(conflicting_artifact_ids, Some(&"a".repeat(64))).is_err());

    let consistent_nexus_aliases = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "modId":42,
            "scope":"deltarune",
            "domain":"DeltaRune",
            "fileId":99,
            "file_id":99,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    let mapped = map_legacy_source(consistent_nexus_aliases, Some(&"a".repeat(64))).unwrap();
    assert_eq!(mapped.source().resource_id().as_str(), "42");
    assert_eq!(mapped.source().artifact_id().unwrap().as_str(), "99");
}

#[test]
fn moddb_legacy_identity_must_match_its_canonical_route() {
    let matching = br#"{
        "source":{
            "provider":"moddb",
            "id":"shared",
            "scope":"games.deltarune.downloads",
            "url":"https://www.moddb.com/games/deltarune/downloads/shared"
        }
    }"#;
    let mapped = map_legacy_source(matching, Some(&"a".repeat(64))).unwrap();
    assert_eq!(mapped.source().resource_id().as_str(), "shared");
    assert_eq!(
        mapped.source().scope().unwrap().as_str(),
        "games.deltarune.downloads"
    );

    let mismatched_scope = br#"{
        "source":{
            "provider":"moddb",
            "id":"shared",
            "scope":"games.deltarune.mods",
            "url":"https://www.moddb.com/games/deltarune/downloads/shared"
        }
    }"#;
    assert!(map_legacy_source(mismatched_scope, Some(&"a".repeat(64))).is_err());

    let mismatched_id = br#"{
        "source":{
            "provider":"moddb",
            "id":"second-resource",
            "scope":"games.deltarune.downloads",
            "url":"https://www.moddb.com/games/deltarune/downloads/shared"
        }
    }"#;
    assert!(map_legacy_source(mismatched_id, Some(&"a".repeat(64))).is_err());

    let matching_separate_markers = br#"{
        "source":{
            "provider":"moddb",
            "id":"shared",
            "scope":"games.deltarune.downloads",
            "url":"https://www.moddb.com/games/deltarune/downloads/shared"
        },
        "provider":"moddb",
        "id":"shared",
        "scope":"games.deltarune.downloads",
        "url":"https://www.moddb.com/games/deltarune/downloads/shared"
    }"#;
    let mapped = map_legacy_source(matching_separate_markers, Some(&"a".repeat(64))).unwrap();
    assert_eq!(mapped.source().resource_id().as_str(), "shared");
}

#[test]
fn legacy_json_rejects_every_duplicate_member_before_identity_dispatch() {
    let duplicate_provider_same = br#"{
        "source":{
            "provider":"nexus",
            "provider":"nexus",
            "id":42,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    let duplicate_provider_conflict = br#"{
        "source":{
            "provider":"nexus",
            "provider":"gamebanana",
            "id":42,
            "url":"https://gamebanana.com/mods/42"
        }
    }"#;
    let duplicate_id_same = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "id":42,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    let duplicate_id_conflict = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "id":43,
            "url":"https://www.nexusmods.com/deltarune/mods/43"
        }
    }"#;
    let duplicate_non_identity = br#"{
        "metadata":{"name":"first","name":"second"},
        "archiveSha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }"#;
    let escaped_duplicate_id = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "\u0069d":42,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    for payload in [
        duplicate_provider_same.as_slice(),
        duplicate_provider_conflict.as_slice(),
        duplicate_id_same.as_slice(),
        duplicate_id_conflict.as_slice(),
        duplicate_non_identity.as_slice(),
        escaped_duplicate_id.as_slice(),
    ] {
        assert!(map_legacy_source(payload, Some(&"a".repeat(64))).is_err());
    }
}

#[test]
fn oversized_raw_fixture_input_is_rejected_before_json_mapping() {
    let oversized = vec![b' '; 4 * 1024 * 1024 + 1];
    assert!(nexus::map_search(&oversized, &request(KnownProvider::Nexus, "deltarune")).is_err());
    assert!(map_legacy_source(&oversized, Some(&"a".repeat(64))).is_err());
}

#[test]
fn nexus_legacy_source_can_never_degrade_to_none_or_a_signed_cdn() {
    let missing_identity = br#"{"source":{"provider":"nexus","id":42,"fileId":99}}"#;
    let error = map_legacy_source(missing_identity, Some(&"a".repeat(64))).unwrap_err();
    assert_eq!(error.contract().code, ProductErrorCode::InvalidRequest);

    let signed_cdn = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "scope":"deltarune",
            "url":"https://cdn.nexus-cdn.com/file.zip?token=must-not-persist"
        }
    }"#;
    let error = map_legacy_source(signed_cdn, Some(&"a".repeat(64))).unwrap_err();
    let rendered = format!("{error:?}");
    assert!(!rendered.contains("must-not-persist"));
    assert!(!rendered.contains("nexus-cdn.com"));

    let non_identity_url = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "scope":"deltarune",
            "url":"https://www.nexusmods.com/users/my-profile"
        }
    }"#;
    assert!(map_legacy_source(non_identity_url, Some(&"a".repeat(64))).is_err());
}

#[test]
fn account_states_are_normalized_without_transport_data() {
    let gamebanana_account = gamebanana::map_account(GAMEBANANA_ACCOUNT).unwrap();
    assert_eq!(gamebanana_account.state(), ProviderAccountState::SignedIn);
    assert_eq!(gamebanana_account.display_name(), Some("Fixture Author"));
    assert_eq!(gamebanana_account.account_id(), Some("7"));
    assert!(!serialized(&gamebanana_account).contains("not-account-data"));
    assert_eq!(
        gamebanana::map_account(br#"{"_idMemberRow":0}"#)
            .unwrap()
            .state(),
        ProviderAccountState::SignedOut
    );

    let nexus_account = nexus::map_account(NEXUS_STATUS).unwrap();
    assert_eq!(nexus_account.state(), ProviderAccountState::SignedIn);
    assert_eq!(nexus_account.display_name(), Some("Nexus Fixture User"));
    assert_eq!(nexus_account.account_id(), Some("9001"));
    assert_eq!(nexus_account.premium(), Some(true));
    assert_eq!(nexus_account.supporter(), Some(false));
    for (payload, expected) in [
        (
            br#"{"ssoPending":true}"#.as_slice(),
            ProviderAccountState::Authorizing,
        ),
        (
            br#"{"configured":true,"connected":false}"#.as_slice(),
            ProviderAccountState::Expired,
        ),
        (
            br#"{"ssoAvailable":false}"#.as_slice(),
            ProviderAccountState::Unavailable,
        ),
        (br#"{}"#.as_slice(), ProviderAccountState::SignedOut),
    ] {
        assert_eq!(nexus::map_account(payload).unwrap().state(), expected);
    }
    assert_eq!(moddb::account().state(), ProviderAccountState::NotRequired);
    assert_eq!(local::account().state(), ProviderAccountState::NotRequired);
}

#[test]
fn runtime_failures_are_normalized_and_never_echo_raw_errors() {
    let raw = "request to https://api.example.invalid/file?token=hunter2 failed: Authorization: Bearer secret";
    let mut input = ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Http);
    input.status = Some(503);
    input.operation_id = Some("download-42");
    input.phase = Some(OperationPhase::Downloading);
    input.raw_message = Some(raw);
    let failure = normalize_provider_error(input);
    assert_eq!(failure.contract().code, ProductErrorCode::DownloadFailed);
    assert!(failure.contract().retryable);
    assert_eq!(failure.contract().recovery_action, RecoveryAction::Retry);
    assert_eq!(failure.contract().safe_details["provider"], "nexus");
    assert_eq!(failure.contract().safe_details["http_status"], "503");
    let rendered = format!("{failure:?} {}", failure);
    assert!(!rendered.contains("hunter2"));
    assert!(!rendered.contains("Authorization"));
    assert!(!rendered.contains("example.invalid"));
    let contract_json = serialized(failure.contract());
    assert!(!contract_json.contains("hunter2"));
    assert!(!contract_json.contains("Bearer"));

    let cases = [
        (
            ProviderFailureKind::Http,
            Some(401),
            ProductErrorCode::AuthenticationRequired,
        ),
        (
            ProviderFailureKind::Http,
            Some(403),
            ProductErrorCode::AuthenticationRequired,
        ),
        (
            ProviderFailureKind::Http,
            Some(429),
            ProductErrorCode::RateLimited,
        ),
        (
            ProviderFailureKind::Offline,
            None,
            ProductErrorCode::ProviderUnavailable,
        ),
        (
            ProviderFailureKind::Cancelled,
            None,
            ProductErrorCode::Cancelled,
        ),
    ];
    for (kind, status, expected) in cases {
        let mut input = ProviderErrorInput::new(KnownProvider::GameBanana, kind);
        input.status = status;
        input.raw_message = Some(raw);
        let failure = normalize_provider_error(input);
        assert_eq!(failure.contract().code, expected);
        assert!(!format!("{failure:?}").contains("hunter2"));
    }

    let mut rate_limit =
        ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::RateLimited);
    rate_limit.retry_after_ms = Some(30_000);
    let failure = normalize_provider_error(rate_limit);
    assert_eq!(failure.contract().safe_details["retry_after_ms"], "30000");

    let mut timeout = ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Http);
    timeout.status = Some(408);
    let timeout_failure = normalize_provider_error(timeout);
    assert_eq!(
        timeout_failure.contract().code,
        ProductErrorCode::ProviderUnavailable
    );
    assert_eq!(
        timeout_failure.contract().message_key,
        "provider.http_failure"
    );
    assert!(timeout_failure.contract().retryable);
    assert_eq!(
        timeout_failure.contract().recovery_action,
        RecoveryAction::Retry
    );

    let mut download_timeout =
        ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Http);
    download_timeout.status = Some(408);
    download_timeout.phase = Some(OperationPhase::Downloading);
    let download_failure = normalize_provider_error(download_timeout);
    assert_eq!(
        download_failure.contract().code,
        ProductErrorCode::DownloadFailed
    );
    assert_eq!(
        download_failure.contract().message_key,
        "provider.http_failure"
    );
    assert!(download_failure.contract().retryable);
}

#[test]
fn progress_is_monotonic_cancellable_and_url_safe() {
    let cancellation = ProviderCancellationToken::default();
    let mut tracker = DownloadProgressTracker::new(
        KnownProvider::Nexus,
        "download-42",
        "installation-main",
        LifecycleOperationKind::Install,
        Some(100),
        1_000,
        cancellation.clone(),
    )
    .unwrap();
    let initial = tracker.snapshot().unwrap();
    assert_eq!(initial.state, OperationState::Running);
    assert_eq!(initial.phase, OperationPhase::Downloading);
    assert!(initial.cancellable);

    let redacted = tracker
        .advance(
            10,
            Some(100),
            Some("https://cdn.nexus-cdn.com/file?token=secret"),
            1_100,
        )
        .unwrap();
    assert!(redacted.current_item.is_none());
    let advanced = tracker
        .advance(20, Some(100), Some("nexus-fixture-v2.zip"), 1_200)
        .unwrap();
    assert_eq!(advanced.completed, 20);
    assert_eq!(
        advanced.current_item.as_deref(),
        Some("nexus-fixture-v2.zip")
    );
    assert!(tracker.advance(19, Some(100), None, 1_300).is_err());

    let cancelling = tracker.request_cancel(1_300).unwrap();
    assert_eq!(cancelling.state, OperationState::Cancelling);
    assert!(!cancelling.cancellable);
    assert!(cancellation.is_cancelled());
    let cancelled_error = tracker.checkpoint().unwrap_err();
    assert_eq!(cancelled_error.contract().code, ProductErrorCode::Cancelled);
    let cancelled = tracker.finish_cancelled(1_400).unwrap();
    assert_eq!(cancelled.state, OperationState::Cancelled);
    assert_eq!(cancelled.phase, OperationPhase::Complete);
    assert!(cancelled.state.terminal());
    assert!(!cancelled.cancellable);

    let mut successful = DownloadProgressTracker::new(
        KnownProvider::GameBanana,
        "download-43",
        "installation-main",
        LifecycleOperationKind::Update,
        Some(8),
        2_000,
        ProviderCancellationToken::default(),
    )
    .unwrap();
    successful.advance(4, None, None, 2_100).unwrap();
    let complete = successful.finish_success(2_200).unwrap();
    assert_eq!(complete.state, OperationState::Succeeded);
    assert_eq!(complete.phase, OperationPhase::Complete);
    assert_eq!(complete.completed, 8);
    assert!(!complete.cancellable);
}

#[test]
fn cache_keys_are_normalized_scope_sensitive_and_version_sensitive() {
    let first = SearchRequest::new(
        KnownProvider::Nexus,
        " DeltaRune ",
        "  Fixture   Mod ",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    let equivalent = SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        "fixture mod",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    assert_eq!(
        first.cache_identity(KnownProvider::Nexus),
        equivalent.cache_identity(KnownProvider::Nexus)
    );
    let other_scope = SearchRequest::new(
        KnownProvider::Nexus,
        "undertale",
        "fixture mod",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    assert_ne!(
        first.cache_identity(KnownProvider::Nexus),
        other_scope.cache_identity(KnownProvider::Nexus)
    );
    let cache = first.cache_identity(KnownProvider::Nexus);
    assert_sha256_identity(cache.as_str());
    assert!(!cache.as_str().contains("fixture mod"));
    assert!(!cache.as_str().contains("deltarune"));
    assert!(!cache.as_str().contains("https://"));
    assert!(!cache.as_str().contains("token="));

    let digest_vector = SearchRequest::new(
        KnownProvider::Nexus,
        "deltarune",
        "abc",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap()
    .cache_identity(KnownProvider::Nexus);
    assert_sha256_identity(digest_vector.as_str());
    assert!(!digest_vector.as_str().contains("abc"));

    let page = nexus::map_search(NEXUS_SEARCH, &first).unwrap();
    assert_sha256_identity(page.scope_digest().as_str());
    assert_ne!(page.scope_digest(), &cache);

    let details = nexus::map_details(NEXUS_MOD, NEXUS_FILES, "deltarune").unwrap();
    assert_sha256_identity(details.item().cache_identity().as_str());
    assert!(!details
        .item()
        .cache_identity()
        .as_str()
        .contains("deltarune"));
    for version in details.versions() {
        assert_sha256_identity(version.cache_identity().as_str());
    }
    assert_ne!(
        details.versions()[0].cache_identity(),
        details.versions()[1].cache_identity()
    );
    assert_eq!(
        details.versions()[0].source().canonical_identity(),
        details.versions()[1].source().canonical_identity()
    );
}

#[test]
fn malformed_and_credentialed_payloads_return_safe_contract_errors() {
    let failures = [
        gamebanana::map_search(MALFORMED, &request(KnownProvider::GameBanana, "6755")).unwrap_err(),
        nexus::map_search(MALFORMED, &request(KnownProvider::Nexus, "deltarune")).unwrap_err(),
        moddb::map_search(MALFORMED, &request(KnownProvider::ModDb, "deltarune")).unwrap_err(),
        local::map_archive(MALFORMED, &"a".repeat(64)).unwrap_err(),
    ];
    for failure in failures {
        assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
        let rendered = format!("{failure:?}");
        assert!(!rendered.contains("serde"));
        assert!(!rendered.contains("line"));
        assert!(!rendered.contains("column"));
    }

    let credentialed_moddb = br#"{
        "title":"Credentialed",
        "sourceUrl":"https://user:secret@www.moddb.com/games/deltarune/downloads/credentialed"
    }"#;
    let failure = moddb::map_details(credentialed_moddb, "deltarune").unwrap_err();
    let rendered = format!("{failure:?} {}", failure);
    assert!(!rendered.contains("user"));
    assert!(!rendered.contains("secret"));
    assert!(!rendered.contains("moddb.com"));

    assert!(local::map_archive(LOCAL_ARCHIVE, "not-a-sha256").is_err());
    assert!(nexus::map_details(b"{}", NEXUS_FILES, "deltarune").is_err());
    let unknown = br#"{"source":{"provider":"gamejolt","id":"42"}}"#;
    assert!(map_legacy_source(unknown, Some(&"a".repeat(64))).is_err());
}

#[test]
fn moddb_accepts_only_exact_canonical_route_shapes() {
    let canonical = br#"{
        "title":"Canonical",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/canonical"
    }"#;
    let details = moddb::map_details(canonical, "deltarune").unwrap();
    assert_eq!(
        details.item().source().canonical_url(),
        Some("https://www.moddb.com/games/deltarune/downloads/canonical")
    );
    let standalone_mod = br#"{
        "title":"Standalone",
        "sourceUrl":"https://www.moddb.com/mods/standalone"
    }"#;
    let details = moddb::map_details(standalone_mod, "deltarune").unwrap();
    assert_eq!(details.item().source().scope().unwrap().as_str(), "mods");
    assert_eq!(
        details.item().source().canonical_url(),
        Some("https://www.moddb.com/mods/standalone")
    );

    let nested_download = br#"{
        "title":"Nested",
        "sourceUrl":"https://www.moddb.com/mods/standalone/downloads/release-one"
    }"#;
    let details = moddb::map_details(nested_download, "deltarune").unwrap();
    assert_eq!(
        details.item().source().scope().unwrap().as_str(),
        "mods.standalone.downloads"
    );

    for forbidden in [
        "https://www.moddb.com/games/deltarune/downloads/shared?token=QUERY_SECRET",
        "https://www.moddb.com/games/deltarune/downloads/shared#FRAGMENT_SECRET",
        "https://user:URL_PASSWORD_SECRET@www.moddb.com/games/deltarune/downloads/shared",
        "https://www.moddb.com:443/games/deltarune/downloads/shared",
        "https://moddb.com/games/deltarune/downloads/shared",
        "https://www.moddb.com/redirect/SIGNED_PATH_TOKEN_4f91",
        "https://www.moddb.com/games/deltarune/downloads/shared/EXTRA_PATH_SECRET",
        "https://www.moddb.com/games/deltarune/downloads/shared/",
        "https://www.moddb.com/games/deltarune/mods/shared/EXTRA_PATH_SECRET",
        "https://www.moddb.com/games/deltarune/./downloads/shared",
        "https://www.moddb.com/games/deltarune/mods/../downloads/shared",
        "https://www.moddb.com/games/deltarune/%2e/downloads/shared",
        "https://www.moddb.com/games/deltarune/%2E%2E/downloads/shared",
        "https://www.moddb.com/games/deltarune/downloads%2fshared",
        "https://www.moddb.com/games/deltarune/downloads%5Cshared",
        "https://www.moddb.com/games/DeltaRune/downloads/shared",
    ] {
        let payload =
            serde_json::to_vec(&json!({"title":"Rejected", "sourceUrl":forbidden})).unwrap();
        let failure = moddb::map_details(&payload, "deltarune").unwrap_err();
        let rendered = format!("{failure:?} {failure}");
        assert!(!rendered.contains("SIGNED_PATH_TOKEN_4f91"));
        assert!(!rendered.contains("EXTRA_PATH_SECRET"));
        assert!(!rendered.contains("QUERY_SECRET"));
        assert!(!rendered.contains("FRAGMENT_SECRET"));
        assert!(!rendered.contains("URL_PASSWORD_SECRET"));
        assert!(!rendered.contains("redirect"));
    }
}

#[test]
fn nexus_reconciles_every_numeric_identity_alias() {
    let conflicting_mod = br#"{
        "modId":42,
        "mod_id":"042",
        "id":43,
        "domain_name":"deltarune",
        "name":"Conflicting"
    }"#;
    assert!(nexus::map_details(conflicting_mod, br#"{"files":[]}"#, "deltarune").is_err());

    let consistent_mod = br#"{
        "modId":"042",
        "mod_id":42,
        "id":"42",
        "domain_name":"deltarune",
        "name":"Consistent"
    }"#;
    let details = nexus::map_details(consistent_mod, br#"{"files":[]}"#, "deltarune").unwrap();
    assert_eq!(details.item().source().resource_id().as_str(), "42");

    let conflicting_file = br#"{
        "files":[{
            "file_id":99,
            "fileId":"099",
            "id":100,
            "version":"1.0.0",
            "file_name":"conflict.zip"
        }]
    }"#;
    assert!(nexus::map_details(consistent_mod, conflicting_file, "deltarune").is_err());

    let consistent_file = br#"{
        "files":[{
            "file_id":99,
            "fileId":"099",
            "id":"99",
            "version":"1.0.0",
            "file_name":"consistent.zip"
        }]
    }"#;
    let details = nexus::map_details(consistent_mod, consistent_file, "deltarune").unwrap();
    assert_eq!(
        details.versions()[0]
            .source()
            .artifact_id()
            .unwrap()
            .as_str(),
        "99"
    );
}

#[test]
fn moddb_stable_identity_aliases_are_reconciled() {
    let conflicting = br#"{
        "title":"Conflicting aliases",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/first-release",
        "link":"https://www.moddb.com/games/deltarune/downloads/second-release"
    }"#;
    assert!(moddb::map_details(conflicting, "deltarune").is_err());

    let malformed_secondary = br#"{
        "title":"Malformed secondary alias",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/safe-release",
        "link":42
    }"#;
    assert!(moddb::map_details(malformed_secondary, "deltarune").is_err());

    let equivalent = br#"{
        "title":"Equivalent aliases",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/same-release",
        "link":"https://www.moddb.com/games/deltarune/downloads/same-release"
    }"#;
    let details = moddb::map_details(equivalent, "deltarune").unwrap();
    assert_eq!(
        details.item().source().resource_id().as_str(),
        "same-release"
    );
}

#[test]
fn gamebanana_model_stable_identity_aliases_are_reconciled() {
    let conflicting = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "model":"Wip",
        "_sName":"Conflicting aliases",
        "_aFiles":[]
    }"#;
    assert!(gamebanana::map_details(conflicting).is_err());

    let malformed_secondary = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "model":42,
        "_sName":"Malformed secondary alias",
        "_aFiles":[]
    }"#;
    assert!(gamebanana::map_details(malformed_secondary).is_err());

    let equivalent = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "model":"mOD",
        "_sName":"Equivalent aliases",
        "_aFiles":[]
    }"#;
    let details = gamebanana::map_details(equivalent).unwrap();
    assert_eq!(details.item().source().scope().unwrap().as_str(), "mod");
}

#[test]
fn gamebanana_version_stable_identity_aliases_are_reconciled() {
    let conflicting = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "_sName":"Version aliases",
        "_aFiles":[{
            "_idRow":1001,
            "_sVersion":"1.0.0",
            "version":"2.0.0",
            "_sFile":"conflicting.zip"
        }]
    }"#;
    assert!(gamebanana::map_details(conflicting).is_err());

    let malformed_secondary = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "_sName":"Version aliases",
        "_aFiles":[{
            "_idRow":1001,
            "_sVersion":"1.0.0",
            "version":42,
            "_sFile":"malformed.zip"
        }]
    }"#;
    assert!(gamebanana::map_details(malformed_secondary).is_err());

    let equivalent = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "_sName":"Version aliases",
        "_aFiles":[{
            "_idRow":1001,
            "_sVersion":" 1.0.0 ",
            "version":"1.0.0",
            "_sFile":"equivalent.zip"
        }]
    }"#;
    let details = gamebanana::map_details(equivalent).unwrap();
    assert_eq!(details.versions()[0].label(), "1.0.0");
    assert_eq!(
        details.versions()[0]
            .source()
            .version_id()
            .unwrap()
            .as_str(),
        "1.0.0"
    );
}

#[test]
fn nexus_version_stable_identity_aliases_are_reconciled() {
    let mod_payload = br#"{
        "mod_id":42,
        "domain_name":"deltarune",
        "name":"Version aliases"
    }"#;
    let conflicting = br#"{
        "files":[{
            "file_id":99,
            "version":"1.0.0",
            "mod_version":"2.0.0",
            "file_name":"conflicting.zip"
        }]
    }"#;
    assert!(nexus::map_details(mod_payload, conflicting, "deltarune").is_err());

    let malformed_secondary = br#"{
        "files":[{
            "file_id":99,
            "version":"1.0.0",
            "mod_version":42,
            "file_name":"malformed.zip"
        }]
    }"#;
    assert!(nexus::map_details(mod_payload, malformed_secondary, "deltarune").is_err());

    let equivalent = br#"{
        "files":[{
            "file_id":99,
            "version":" 1.0.0 ",
            "mod_version":"1.0.0",
            "file_name":"equivalent.zip"
        }]
    }"#;
    let details = nexus::map_details(mod_payload, equivalent, "deltarune").unwrap();
    assert_eq!(details.versions()[0].label(), "1.0.0");
    assert_eq!(
        details.versions()[0]
            .source()
            .version_id()
            .unwrap()
            .as_str(),
        "1.0.0"
    );
}

#[test]
fn legacy_identity_discovery_is_recursive_bounded_and_fail_closed() {
    let hidden_valid = br#"{
        "metadata":{
            "items":[{
                "provider":"nexus",
                "id":"042",
                "modId":42,
                "scope":"deltarune",
                "url":"https://www.nexusmods.com/deltarune/mods/42"
            }]
        }
    }"#;
    let mapped = map_legacy_source(hidden_valid, Some(&"a".repeat(64))).unwrap();
    assert_eq!(mapped.source().provider_id().as_str(), "nexus");
    assert_eq!(mapped.source().resource_id().as_str(), "42");

    let hidden_conflict = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "scope":"deltarune",
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        },
        "metadata":{"items":[{"provider":"gamebanana","id":42,"model":"Mod"}]}
    }"#;
    assert!(map_legacy_source(hidden_conflict, Some(&"a".repeat(64))).is_err());

    let hidden_second_id = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "scope":"deltarune",
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        },
        "metadata":{"identity":{"id":43}}
    }"#;
    assert!(map_legacy_source(hidden_second_id, Some(&"a".repeat(64))).is_err());
    assert!(map_legacy_source(
        br#"{"metadata":{"identity":{"id":42}}}"#,
        Some(&"a".repeat(64))
    )
    .is_err());
    assert!(map_legacy_source(
        br#"{"metadata":{"source":{"name":"unsupported source shape"}}}"#,
        Some(&"a".repeat(64))
    )
    .is_err());

    let extra_nexus_segment = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "scope":"deltarune",
            "url":"https://www.nexusmods.com/deltarune/mods/42/EXTRA_ROUTE_SECRET"
        }
    }"#;
    let failure = map_legacy_source(extra_nexus_segment, Some(&"a".repeat(64))).unwrap_err();
    assert!(!format!("{failure:?}").contains("EXTRA_ROUTE_SECRET"));

    let case_duplicate_provider = br#"{
        "source":{
            "provider":"nexus",
            "Provider":"nexus",
            "id":42,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    let case_duplicate_id = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "ID":42,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    let escaped_case_duplicate = br#"{
        "source":{
            "provider":"nexus",
            "\u0050rovider":"nexus",
            "id":42,
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        }
    }"#;
    for payload in [
        case_duplicate_provider.as_slice(),
        case_duplicate_id.as_slice(),
        escaped_case_duplicate.as_slice(),
    ] {
        assert!(map_legacy_source(payload, Some(&"a".repeat(64))).is_err());
    }
    assert!(map_legacy_source(
        br#"{"Source":{"provider":"nexus","id":42,"scope":"deltarune","url":"https://www.nexusmods.com/deltarune/mods/42"}}"#,
        Some(&"a".repeat(64))
    )
    .is_err());
    assert!(map_legacy_source(
        br#"{"source":{"provider":"nexus","id":42,"scope":"deltarune","url":"https://api.nexusmods.com/deltarune/mods/42"}}"#,
        Some(&"a".repeat(64))
    )
    .is_err());

    let mut nested = "{}".to_owned();
    for _ in 0..40 {
        nested = format!(r#"{{"metadata":{nested}}}"#);
    }
    assert!(map_legacy_source(nested.as_bytes(), Some(&"a".repeat(64))).is_err());

    let many_nodes = format!(r#"{{"metadata":[{}]}}"#, vec!["null"; 100_000].join(","));
    assert!(map_legacy_source(many_nodes.as_bytes(), Some(&"a".repeat(64))).is_err());
}

#[test]
fn stable_free_text_rejects_versions_paths_credentials_and_urls() {
    let safe = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "fileId":99,
            "scope":"deltarune",
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        },
        "version":"1.2 beta"
    }"#;
    assert_eq!(
        map_legacy_source(safe, Some(&"a".repeat(64)))
            .unwrap()
            .version(),
        Some("1.2 beta")
    );

    for secret_version in [
        "https://example.invalid/VERSION_URL_SECRET",
        "C:\\private\\VERSION_PATH_SECRET",
        "../VERSION_PATH_SECRET",
        "access_token = VERSION_CREDENTIAL_SECRET",
    ] {
        let payload = serde_json::to_vec(&json!({
            "source": {
                "provider": "nexus",
                "id": 42,
                "fileId": 99,
                "scope": "deltarune",
                "url": "https://www.nexusmods.com/deltarune/mods/42"
            },
            "version": secret_version
        }))
        .unwrap();
        let failure = map_legacy_source(&payload, Some(&"a".repeat(64))).unwrap_err();
        let rendered = format!("{failure:?} {failure}");
        assert!(!rendered.contains("VERSION_URL_SECRET"));
        assert!(!rendered.contains("VERSION_PATH_SECRET"));
        assert!(!rendered.contains("VERSION_CREDENTIAL_SECRET"));
    }

    for payload in [
        br#"{"metadata":{"name":"C:\\private\\LOCAL_NAME_SECRET","version":"1.0"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","description":"access-token : LOCAL_DESCRIPTION_SECRET","version":"1.0"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","description":"Imported from C:\\private\\LOCAL_EMBEDDED_PATH_SECRET","version":"1.0"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","version":"https://example.invalid/LOCAL_VERSION_SECRET"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","version":"1.0"},"fileName":"../LOCAL_FILENAME_SECRET.zip"}"#.as_slice(),
    ] {
        let failure = local::map_archive(payload, &"a".repeat(64)).unwrap_err();
        let rendered = format!("{failure:?} {failure}");
        assert!(!rendered.contains("LOCAL_"));
    }

    let unsafe_title = br#"{
        "items":[{"modId":42,"gameDomainName":"deltarune","name":"access_token = ITEM_TITLE_SECRET"}]
    }"#;
    let failure =
        nexus::map_search(unsafe_title, &request(KnownProvider::Nexus, "deltarune")).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ITEM_TITLE_SECRET"));

    let unsafe_optional_text = br#"{
        "items":[{
            "modId":42,
            "gameDomainName":"deltarune",
            "name":"Safe title",
            "summary":"Imported from C:\\private\\ITEM_SUMMARY_SECRET",
            "updatedAt":"access-token: ITEM_STATUS_SECRET"
        }]
    }"#;
    let page = nexus::map_search(
        unsafe_optional_text,
        &request(KnownProvider::Nexus, "deltarune"),
    )
    .unwrap();
    assert!(page.items()[0].summary().is_none());
    assert!(page.items()[0].updated_at().is_none());
    let stable = format!("{page:?} {}", serialized(&page));
    assert!(!stable.contains("ITEM_SUMMARY_SECRET"));
    assert!(!stable.contains("ITEM_STATUS_SECRET"));
}

#[test]
fn nexus_stable_text_rejects_standalone_credential_assignments() {
    let credential_title = br#"{
        "items":[{
            "modId":42,
            "gameDomainName":"deltarune",
            "name":"token=ROUND7_NEXUS_TITLE"
        }]
    }"#;
    let failure = nexus::map_search(
        credential_title,
        &request(KnownProvider::Nexus, "deltarune"),
    )
    .unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ROUND7_NEXUS_TITLE"));

    let optional_text = br#"{
        "items":[{
            "modId":42,
            "gameDomainName":"deltarune",
            "name":"Safe Nexus title",
            "summary":"token = ROUND7_NEXUS_SUMMARY",
            "author":"'API_KEY' = 'ROUND7_NEXUS_AUTHOR'",
            "updatedAt":"Bearer ROUND7_NEXUS_UPDATED"
        }]
    }"#;
    let page =
        nexus::map_search(optional_text, &request(KnownProvider::Nexus, "deltarune")).unwrap();
    let item = &page.items()[0];
    assert!(item.summary().is_none());
    assert!(item.author().is_none());
    assert!(item.updated_at().is_none());
    assert!(!serialized(&page).contains("ROUND7_NEXUS"));

    let files = br#"{
        "files":[{
            "file_id":99,
            "version":"1.0.0",
            "file_name":"Bearer ROUND7_NEXUS_FILE.zip"
        }]
    }"#;
    let failure = nexus::map_details(NEXUS_MOD, files, "deltarune").unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ROUND7_NEXUS_FILE"));

    let account = nexus::map_account(
        br#"{
            "connected":true,
            "valid":true,
            "name":"secret=ROUND7_NEXUS_ACCOUNT",
            "userId":9001
        }"#,
    )
    .unwrap();
    assert!(account.display_name().is_none());
    assert!(!serialized(&account).contains("ROUND7_NEXUS_ACCOUNT"));
}

#[test]
fn gamebanana_stable_text_rejects_standalone_credential_assignments() {
    let credential_title = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "_sName":"SeCrEt = ROUND7_GAMEBANANA_TITLE",
        "_aFiles":[]
    }"#;
    let failure = gamebanana::map_details(credential_title).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ROUND7_GAMEBANANA_TITLE"));

    let optional_text = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "_sName":"Safe GameBanana title",
        "_sDescription":"'token' : 'ROUND7_GAMEBANANA_SUMMARY'",
        "_aSubmitter":{"_sName":"Bearer ROUND7_GAMEBANANA_AUTHOR"},
        "_aFiles":[]
    }"#;
    let details = gamebanana::map_details(optional_text).unwrap();
    assert!(details.item().summary().is_none());
    assert!(details.item().author().is_none());
    assert!(!serialized(&details).contains("ROUND7_GAMEBANANA"));

    let credential_label = br#"{
        "_idRow":42,
        "_sModelName":"Mod",
        "_sName":"Safe GameBanana title",
        "_aFiles":[{
            "_idRow":1001,
            "_sName":"Bearer ROUND7_GAMEBANANA_VERSION"
        }]
    }"#;
    let failure = gamebanana::map_details(credential_label).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ROUND7_GAMEBANANA_VERSION"));

    let account = gamebanana::map_account(
        br#"{
            "_idMemberRow":7,
            "_sName":"token=ROUND7_GAMEBANANA_ACCOUNT"
        }"#,
    )
    .unwrap();
    assert!(account.display_name().is_none());
    assert!(!serialized(&account).contains("ROUND7_GAMEBANANA_ACCOUNT"));
}

#[test]
fn moddb_stable_text_rejects_standalone_credential_assignments() {
    let credential_title = br#"{
        "title":"token=ROUND7_MODDB_TITLE",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/credential-title"
    }"#;
    let failure = moddb::map_details(credential_title, "deltarune").unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ROUND7_MODDB_TITLE"));

    let optional_text = br#"{
        "title":"Safe ModDB title",
        "summary":"prefix; \"ToKeN\" = \"ROUND7_MODDB_SUMMARY\"",
        "author":"secret: ROUND7_MODDB_AUTHOR",
        "published":"Bearer ROUND7_MODDB_UPDATED",
        "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/safe-text"
    }"#;
    let details = moddb::map_details(optional_text, "deltarune").unwrap();
    assert!(details.item().summary().is_none());
    assert!(details.item().author().is_none());
    assert!(details.item().updated_at().is_none());
    assert!(!serialized(&details).contains("ROUND7_MODDB"));

    let page_payload = br#"{
        "attribution":"apikey=ROUND7_MODDB_ATTRIBUTION",
        "items":[{
            "title":"Safe ModDB title",
            "sourceUrl":"https://www.moddb.com/games/deltarune/downloads/safe-attribution"
        }]
    }"#;
    let page =
        moddb::map_search(page_payload, &request(KnownProvider::ModDb, "deltarune")).unwrap();
    assert!(page.attribution().is_none());
    assert!(!serialized(&page).contains("ROUND7_MODDB_ATTRIBUTION"));
}

#[test]
fn local_and_legacy_text_reject_standalone_credential_assignments() {
    for payload in [
        br#"{"metadata":{"name":"token=ROUND7_LOCAL_TITLE","version":"1.0"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","description":"secret = ROUND7_LOCAL_SUMMARY","version":"1.0"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","author":"Bearer ROUND7_LOCAL_AUTHOR","version":"1.0"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","version":"Bearer ROUND7_LOCAL_VERSION"}}"#.as_slice(),
        br#"{"metadata":{"name":"Safe","version":"1.0"},"fileName":"Bearer ROUND7_LOCAL_FILE.zip"}"#.as_slice(),
    ] {
        let failure = local::map_archive(payload, &"a".repeat(64)).unwrap_err();
        assert!(!format!("{failure:?} {failure}").contains("ROUND7_LOCAL"));
    }

    let source_record = br#"{
        "source":{
            "provider":"nexus",
            "id":42,
            "scope":"deltarune",
            "url":"https://www.nexusmods.com/deltarune/mods/42"
        },
        "version":"Bearer ROUND7_SOURCE_RECORD"
    }"#;
    let failure = map_legacy_source(source_record, Some(&"a".repeat(64))).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains("ROUND7_SOURCE_RECORD"));
}

#[test]
fn benign_credential_substrings_and_phrases_remain_serializable() {
    let payload = br#"{
        "items":[{
            "modId":42,
            "gameDomainName":"deltarune",
            "name":"Passwordless mode",
            "summary":"Signature move; tokenizer=enabled; token economy=healthy; secret level=hidden",
            "author":"Secretariat",
            "updatedAt":"Tokenization"
        }]
    }"#;
    let page = nexus::map_search(payload, &request(KnownProvider::Nexus, "deltarune")).unwrap();
    let item = &page.items()[0];
    assert_eq!(item.title(), "Passwordless mode");
    assert_eq!(
        item.summary(),
        Some("Signature move; tokenizer=enabled; token economy=healthy; secret level=hidden")
    );
    assert_eq!(item.author(), Some("Secretariat"));
    assert_eq!(item.updated_at(), Some("Tokenization"));
    let stable = format!("{page:?} {} {:?}", serialized(&page), item.source());
    for benign in [
        "Passwordless mode",
        "Signature move",
        "Secretariat",
        "Tokenization",
    ] {
        assert!(stable.contains(benign), "lost benign text {benign:?}");
    }
}

#[test]
fn symbol_unicode_and_split_key_credentials_never_serialize_across_adapters() {
    const NEXUS_SECRET: &str = "ROUND9_NEXUS_TITLE_91a7";
    let nexus_title = serde_json::to_vec(&json!({
        "items": [{
            "modId": 42,
            "gameDomainName": "deltarune",
            "name": format!(r#""t-o-k-e-n" = "!密钥{NEXUS_SECRET}!""#)
        }]
    }))
    .unwrap();
    let nexus_result = nexus::map_search(&nexus_title, &request(KnownProvider::Nexus, "deltarune"));
    if let Ok(page) = &nexus_result {
        for surface in [
            serialized(page),
            format!("{page:?}"),
            serialized(&page.items()[0]),
            format!("{:?}", page.items()[0]),
        ] {
            assert!(
                !surface.contains(NEXUS_SECRET),
                "split credential title reached a stable item/page surface"
            );
        }
    }
    let failure = nexus_result.unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(NEXUS_SECRET));

    const GAMEBANANA_SUMMARY_SECRET: &str = "ROUND9_GAMEBANANA_SUMMARY_3f2b";
    const GAMEBANANA_AUTHOR_SECRET: &str = "ROUND9_GAMEBANANA_AUTHOR_4c8d";
    let gamebanana_summary = serde_json::to_vec(&json!({
        "_idRow": 42,
        "_sModelName": "Mod",
        "_sName": "Safe GameBanana title",
        "_sDescription": format!(r#"s-e-c-r-e-t="!密钥{GAMEBANANA_SUMMARY_SECRET}!""#),
        "_aSubmitter": {
            "_sName": format!("t•o—k…e·n = '[密钥{GAMEBANANA_AUTHOR_SECRET}!]'"),
        },
        "_aFiles": []
    }))
    .unwrap();
    let details = gamebanana::map_details(&gamebanana_summary).unwrap();
    assert!(details.item().summary().is_none());
    assert!(details.item().author().is_none());
    for surface in [
        serialized(details.item()),
        format!("{:?}", details.item()),
        serialized(&details),
        format!("{details:?}"),
    ] {
        assert!(!surface.contains(GAMEBANANA_SUMMARY_SECRET));
        assert!(!surface.contains(GAMEBANANA_AUTHOR_SECRET));
    }

    const MODDB_SECRET: &str = "ROUND9_MODDB_AUTHOR_7d5e";
    let moddb_author = serde_json::to_vec(&json!({
        "title": "Safe ModDB title",
        "author": format!(r#"'s::e::c::r::e::t' = "!密钥{MODDB_SECRET}!""#),
        "sourceUrl": "https://www.moddb.com/mods/safe-unicode-author"
    }))
    .unwrap();
    let details = moddb::map_details(&moddb_author, "deltarune").unwrap();
    assert!(details.item().author().is_none());
    assert!(!serialized(&details).contains(MODDB_SECRET));
    assert!(!format!("{details:?}").contains(MODDB_SECRET));

    const LOCAL_SECRET: &str = "ROUND9_LOCAL_TITLE_2a6c";
    let local_title = serde_json::to_vec(&json!({
        "metadata": {
            "name": format!(r#"'b-e-a-r-e-r' "!密钥{LOCAL_SECRET}!""#),
            "version": "1.0"
        }
    }))
    .unwrap();
    let failure = local::map_archive(&local_title, &"a".repeat(64)).unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(LOCAL_SECRET));

    const ACCOUNT_SECRET: &str = "ROUND9_NEXUS_ACCOUNT_8b1f";
    let nexus_account = nexus::map_account(
        &serde_json::to_vec(&json!({
            "connected": true,
            "valid": true,
            "name": format!(r#""s-e-c-r-e-t" = [!密钥{ACCOUNT_SECRET}!]"#),
            "userId": 9001
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(nexus_account.display_name().is_none());
    assert!(!serialized(&nexus_account).contains(ACCOUNT_SECRET));
    assert!(!format!("{nexus_account:?}").contains(ACCOUNT_SECRET));
}

#[test]
fn credential_shaped_provider_identities_never_reach_public_surfaces() {
    const RESOURCE_MARKER: &str = "b-e-a-r-e-r-round9-source-secret";
    let details_payload = format!(
        r#"{{
            "title":"Safe identity title",
            "sourceUrl":"https://www.moddb.com/mods/{RESOURCE_MARKER}"
        }}"#
    );
    let details_result = moddb::map_details(details_payload.as_bytes(), "deltarune");
    if let Ok(details) = &details_result {
        let source = details.item().source();
        for surface in [
            source.canonical_identity(),
            source.canonical_url().unwrap_or_default().to_owned(),
            serialized(source),
            format!("{source:?}"),
            serialized(details.item()),
            format!("{:?}", details.item()),
            serialized(details),
            format!("{details:?}"),
        ] {
            assert!(
                !surface.contains(RESOURCE_MARKER),
                "credential identity reached a ProviderRef/item/details surface"
            );
        }
    }
    let failure = details_result.unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(RESOURCE_MARKER));

    let page_payload = format!(
        r#"{{
            "items":[{{
                "title":"Safe page title",
                "sourceUrl":"https://www.moddb.com/mods/{RESOURCE_MARKER}"
            }}]
        }}"#
    );
    let page_result = moddb::map_search(
        page_payload.as_bytes(),
        &request(KnownProvider::ModDb, "deltarune"),
    );
    if let Ok(page) = &page_result {
        for surface in [serialized(page), format!("{page:?}")] {
            assert!(
                !surface.contains(RESOURCE_MARKER),
                "credential identity reached a ProviderPage surface"
            );
        }
    }
    let failure = page_result.unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(RESOURCE_MARKER));

    let source_record_payload = format!(
        r#"{{
            "source":{{
                "provider":"moddb",
                "url":"https://www.moddb.com/mods/{RESOURCE_MARKER}"
            }}
        }}"#
    );
    let source_record_result = map_legacy_source(source_record_payload.as_bytes(), None);
    if let Ok(record) = &source_record_result {
        for surface in [
            serialized(record.source()),
            format!("{:?}", record.source()),
            serialized(record),
            format!("{record:?}"),
        ] {
            assert!(
                !surface.contains(RESOURCE_MARKER),
                "credential identity reached a source-record surface"
            );
        }
    }
    let failure = source_record_result.unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(RESOURCE_MARKER));

    const SCOPE_MARKER: &str = "b-e-a-r-e-r-round9-game-secret";
    let scoped_payload = format!(
        r#"{{
            "title":"Safe scoped title",
            "sourceUrl":"https://www.moddb.com/games/{SCOPE_MARKER}/downloads/safe-download"
        }}"#
    );
    let failure = moddb::map_details(scoped_payload.as_bytes(), "deltarune").unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(SCOPE_MARKER));

    const ARTIFACT_MARKER: &str = "b-e-a-r-e-r-round9-artifact-secret";
    let artifact_payload = format!(
        r#"{{
            "source":{{
                "provider":"moddb",
                "url":"https://www.moddb.com/mods/safe-artifact",
                "fileId":"{ARTIFACT_MARKER}"
            }}
        }}"#
    );
    let artifact_result = map_legacy_source(artifact_payload.as_bytes(), None);
    if let Ok(record) = &artifact_result {
        for surface in [
            record.source().canonical_identity(),
            serialized(record.source()),
            format!("{:?}", record.source()),
            serialized(record),
            format!("{record:?}"),
        ] {
            assert!(
                !surface.contains(ARTIFACT_MARKER),
                "credential artifact identity reached a source-record surface"
            );
        }
    }
    let failure = artifact_result.unwrap_err();
    assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
    assert_eq!(failure.contract().message_key, "provider.invalid_payload");
    assert!(!format!("{failure:?} {failure}").contains(ARTIFACT_MARKER));
}

#[test]
fn separator_obfuscated_short_credentials_never_reach_moddb_provider_refs() {
    for resource_id in [
        "t-o-k-e-n-round9credential",
        "s-e-c-r-e-t-round9credential",
        "s-i-g-n-a-t-u-r-e-round9credential",
        "t_o-k_e-n-round9credential",
        "se-c-r_e-t-round9credential",
        "signa-t_u-r-e-round9credential",
    ] {
        let payload = format!(
            r#"{{
                "title":"Rejected credential identity",
                "sourceUrl":"https://www.moddb.com/mods/{resource_id}"
            }}"#
        );
        let result = moddb::map_details(payload.as_bytes(), "deltarune");
        if let Ok(details) = &result {
            let source = details.item().source();
            for surface in [
                source.canonical_identity(),
                source.canonical_url().unwrap_or_default().to_owned(),
                serialized(source),
                format!("{source:?}"),
                serialized(details),
                format!("{details:?}"),
            ] {
                assert!(
                    !surface.contains(resource_id),
                    "separator-obfuscated credential reached a ProviderRef surface"
                );
            }
        }

        let failure = result.unwrap_err();
        assert_eq!(failure.contract().code, ProductErrorCode::InvalidRequest);
        assert_eq!(failure.contract().message_key, "provider.invalid_payload");
        for surface in [
            serialized(failure.contract()),
            format!("{:?}", failure.contract()),
            format!("{failure:?}"),
            failure.to_string(),
        ] {
            assert!(!surface.contains(resource_id));
        }
    }
}

#[test]
fn separator_obfuscated_short_credentials_never_reach_error_operation_id_serialization() {
    for operation_id in [
        "t-o-k-e-n-round9credential",
        "s-e-c-r-e-t-round9credential",
        "s-i-g-n-a-t-u-r-e-round9credential",
        "TO-K_E.N-round9credential",
        "SE-C_R.E-T-round9credential",
        "SIGNA-T_U.R-E-round9credential",
    ] {
        let mut input =
            ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Download);
        input.operation_id = Some(operation_id);
        assert!(!format!("{input:?}").contains(operation_id));

        let failure = normalize_provider_error(input);
        assert!(failure.contract().operation_id.is_none());
        for surface in [
            serialized(failure.contract()),
            format!("{:?}", failure.contract()),
            format!("{failure:?}"),
            failure.to_string(),
        ] {
            assert!(!surface.contains(operation_id));
        }

        let tracker = DownloadProgressTracker::new(
            KnownProvider::Nexus,
            operation_id,
            "installation-main",
            LifecycleOperationKind::Install,
            None,
            1,
            ProviderCancellationToken::default(),
        );
        if let Ok(tracker) = &tracker {
            let snapshot = tracker.snapshot().unwrap();
            for surface in [
                format!("{tracker:?}"),
                serialized(&snapshot),
                format!("{snapshot:?}"),
            ] {
                assert!(!surface.contains(operation_id));
            }
        }
        assert!(tracker.is_err());
    }

    for operation_id in [
        "token-economy",
        "secret-level",
        "signature-move",
        "tokenization",
        "secretariat",
    ] {
        let mut input =
            ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Download);
        input.operation_id = Some(operation_id);
        let failure = normalize_provider_error(input);
        assert_eq!(
            failure.contract().operation_id.as_deref(),
            Some(operation_id)
        );
        assert!(serialized(failure.contract()).contains(operation_id));

        let tracker = DownloadProgressTracker::new(
            KnownProvider::Nexus,
            operation_id,
            "installation-main",
            LifecycleOperationKind::Install,
            None,
            1,
            ProviderCancellationToken::default(),
        )
        .unwrap();
        let snapshot = tracker.snapshot().unwrap();
        assert_eq!(snapshot.operation_id, operation_id);
        assert!(serialized(&snapshot).contains(operation_id));
    }
}

#[test]
fn benign_identity_words_remain_valid_moddb_routes() {
    for slug in [
        "token-economy",
        "secret-level",
        "bearerless-mod",
        "passwordless-mode",
        "signature-move",
        "tokenization",
        "secretariat",
    ] {
        let payload = format!(
            r#"{{
                "title":"Safe benign identity",
                "sourceUrl":"https://www.moddb.com/mods/{slug}"
            }}"#
        );
        let details = moddb::map_details(payload.as_bytes(), "deltarune").unwrap();
        assert_eq!(details.item().source().resource_id().as_str(), slug);
        assert!(serialized(&details).contains(slug));
    }
}

#[test]
fn progress_surfaces_accept_only_safe_ids_and_basenames() {
    let mut tracker = DownloadProgressTracker::new(
        KnownProvider::Nexus,
        "download-structured",
        "installation-main",
        LifecycleOperationKind::Install,
        Some(10),
        1,
        ProviderCancellationToken::default(),
    )
    .unwrap();
    let redacted = tracker
        .advance(1, Some(10), Some("access_token = PROGRESS_SECRET_91a7"), 2)
        .unwrap();
    assert!(redacted.current_item.is_none());
    let stable = format!("{redacted:?} {}", serialized(&redacted));
    assert!(!stable.contains("PROGRESS_SECRET_91a7"));

    for rejected in [
        "../PROGRESS_PATH_SECRET.zip",
        "C:\\private\\PROGRESS_PATH_SECRET.zip",
        "status: PROGRESS_MESSAGE_SECRET",
        "authorization bearer PROGRESS_AUTH_SECRET",
    ] {
        let snapshot = tracker.advance(2, Some(10), Some(rejected), 3).unwrap();
        assert!(snapshot.current_item.is_none());
    }
    let safe = tracker
        .advance(3, Some(10), Some("fixture-release_1.2.zip"), 4)
        .unwrap();
    assert_eq!(
        safe.current_item.as_deref(),
        Some("fixture-release_1.2.zip")
    );

    assert!(DownloadProgressTracker::new(
        KnownProvider::Nexus,
        "access-token-PROGRESS_OPERATION_SECRET",
        "installation-main",
        LifecycleOperationKind::Install,
        None,
        1,
        ProviderCancellationToken::default(),
    )
    .is_err());
    assert!(DownloadProgressTracker::new(
        KnownProvider::Nexus,
        "x".repeat(10_000),
        "installation-main",
        LifecycleOperationKind::Install,
        None,
        1,
        ProviderCancellationToken::default(),
    )
    .is_err());

    let mut error_input =
        ProviderErrorInput::new(KnownProvider::Nexus, ProviderFailureKind::Download);
    error_input.operation_id = Some("access-token-PROGRESS_ERROR_SECRET");
    let failure = normalize_provider_error(error_input);
    assert!(failure.contract().operation_id.is_none());
    assert!(!serialized(failure.contract()).contains("PROGRESS_ERROR_SECRET"));
}

#[test]
fn provider_version_collections_have_hard_boundaries_before_mapping() {
    fn nexus_files(count: usize) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "files": vec![json!({
                "file_id": 99,
                "version": "1.0.0",
                "file_name": "fixture.zip"
            }); count]
        }))
        .unwrap()
    }

    fn gamebanana_files(count: usize) -> Vec<u8> {
        let mut detail: serde_json::Value = serde_json::from_slice(GAMEBANANA_DETAIL).unwrap();
        let file = detail["_aFiles"][0].clone();
        detail["_aFiles"] = serde_json::Value::Array(vec![file; count]);
        serde_json::to_vec(&detail).unwrap()
    }

    assert!(nexus::map_details(NEXUS_MOD, &nexus_files(128), "deltarune").is_ok());
    assert!(nexus::map_details(NEXUS_MOD, &nexus_files(129), "deltarune").is_err());
    let root_nexus_files = |count| {
        serde_json::to_vec(&vec![
            json!({
                "file_id": 99,
                "version": "1.0.0",
                "file_name": "fixture.zip"
            });
            count
        ])
        .unwrap()
    };
    assert!(nexus::map_details(NEXUS_MOD, &root_nexus_files(128), "deltarune").is_ok());
    assert!(nexus::map_details(NEXUS_MOD, &root_nexus_files(129), "deltarune").is_err());
    assert!(gamebanana::map_details(&gamebanana_files(128)).is_ok());
    assert!(gamebanana::map_details(&gamebanana_files(129)).is_err());

    let hundred_thousand = format!(r#"{{"files":[{}]}}"#, vec!["null"; 100_000].join(","));
    assert!(nexus::map_details(NEXUS_MOD, hundred_thousand.as_bytes(), "deltarune").is_err());
    let hundred_thousand_gamebanana = format!(
        r#"{{"_idRow":42,"_sModelName":"Mod","_sName":"Huge","_aFiles":[{}]}}"#,
        vec!["null"; 100_000].join(",")
    );
    assert!(gamebanana::map_details(hundred_thousand_gamebanana.as_bytes()).is_err());

    let detail_with_integrations = |count| {
        let mut detail: serde_json::Value = serde_json::from_slice(GAMEBANANA_DETAIL).unwrap();
        detail["_aFiles"] = json!([{
            "_idRow": 1001,
            "_sVersion": "1.0.0",
            "_sFile": "fixture.zip",
            "_sDownloadUrl": "https://gamebanana.com/dl/1001",
            "_aModManagerIntegrations": vec![json!({"_idToolRow": 20_575}); count]
        }]);
        serde_json::to_vec(&detail).unwrap()
    };
    assert!(gamebanana::map_details(&detail_with_integrations(64)).is_ok());
    assert!(gamebanana::map_details(&detail_with_integrations(65)).is_err());

    let details = nexus::map_details(NEXUS_MOD, NEXUS_FILES, "deltarune").unwrap();
    let links = |count| {
        serde_json::to_vec(&json!({
            "links": vec![json!({"URI":"https://cdn.nexus-cdn.com/fixture.zip"}); count]
        }))
        .unwrap()
    };
    assert!(nexus::resolve_download(
        &details.versions()[0],
        &links(32),
        ProviderAccountState::SignedIn
    )
    .is_ok());
    assert!(nexus::resolve_download(
        &details.versions()[0],
        &links(33),
        ProviderAccountState::SignedIn
    )
    .is_err());
}

#[test]
fn provider_result_collections_have_streaming_boundaries() {
    let gamebanana_record = json!({
        "_idRow": 42,
        "_sModelName": "Mod",
        "_sName": "Fixture"
    });
    let gamebanana_payload = |count| {
        serde_json::to_vec(&json!({
            "_aMetadata": {"_bIsComplete": true},
            "_aRecords": vec![gamebanana_record.clone(); count]
        }))
        .unwrap()
    };
    assert!(gamebanana::map_search(
        &gamebanana_payload(200),
        &request(KnownProvider::GameBanana, "6755")
    )
    .is_ok());
    assert!(gamebanana::map_search(
        &gamebanana_payload(201),
        &request(KnownProvider::GameBanana, "6755")
    )
    .is_err());
    let gamebanana_root = serde_json::to_vec(&vec![gamebanana_record; 201]).unwrap();
    assert!(gamebanana::map_search(
        &gamebanana_root,
        &request(KnownProvider::GameBanana, "6755")
    )
    .is_err());

    let nexus_record = json!({
        "modId": 42,
        "gameDomainName": "deltarune",
        "name": "Fixture"
    });
    let nexus_payload =
        |count| serde_json::to_vec(&json!({"items": vec![nexus_record.clone(); count]})).unwrap();
    assert!(nexus::map_search(
        &nexus_payload(200),
        &request(KnownProvider::Nexus, "deltarune")
    )
    .is_ok());
    assert!(nexus::map_search(
        &nexus_payload(201),
        &request(KnownProvider::Nexus, "deltarune")
    )
    .is_err());
    let nexus_nodes = serde_json::to_vec(&json!({
        "data": {"mods": {"nodes": vec![nexus_record.clone(); 201]}}
    }))
    .unwrap();
    assert!(nexus::map_search(&nexus_nodes, &request(KnownProvider::Nexus, "deltarune")).is_err());
    let nexus_root = serde_json::to_vec(&vec![nexus_record; 201]).unwrap();
    assert!(nexus::map_search(&nexus_root, &request(KnownProvider::Nexus, "deltarune")).is_err());

    let moddb_record = json!({
        "title": "Fixture",
        "sourceUrl": "https://www.moddb.com/games/deltarune/downloads/fixture"
    });
    let moddb_payload =
        |count| serde_json::to_vec(&json!({"items": vec![moddb_record.clone(); count]})).unwrap();
    assert!(moddb::map_search(
        &moddb_payload(200),
        &request(KnownProvider::ModDb, "deltarune")
    )
    .is_ok());
    assert!(moddb::map_search(
        &moddb_payload(201),
        &request(KnownProvider::ModDb, "deltarune")
    )
    .is_err());
    let moddb_root = serde_json::to_vec(&vec![moddb_record; 201]).unwrap();
    assert!(moddb::map_search(&moddb_root, &request(KnownProvider::ModDb, "deltarune")).is_err());

    let sparse_huge = format!(r#"{{"items":[{}]}}"#, vec!["null"; 100_000].join(","));
    assert!(nexus::map_search(
        sparse_huge.as_bytes(),
        &request(KnownProvider::Nexus, "deltarune")
    )
    .is_err());
}

#[test]
fn gamebanana_supports_false_suppresses_only_its_immediate_identity() {
    let archive_hash = "a".repeat(64);
    let immediate_only = br#"{
        "gamebanana":{
            "supports":false,
            "provider":"gamebanana",
            "id":9001,
            "model":"Mod"
        }
    }"#;
    let mapped = map_legacy_source(immediate_only, Some(&archive_hash)).unwrap();
    assert_eq!(mapped.source().provider_id().as_str(), "local");

    let nested_nexus = br#"{
        "gamebanana":{
            "supports":false,
            "id":9001,
            "model":"Mod",
            "metadata":{"source":{
                "provider":"nexus",
                "id":42,
                "scope":"deltarune",
                "url":"https://www.nexusmods.com/deltarune/mods/42"
            }}
        }
    }"#;
    let mapped = map_legacy_source(nested_nexus, Some(&archive_hash)).unwrap();
    assert_eq!(mapped.source().provider_id().as_str(), "nexus");

    let nested_gamebanana = br#"{
        "gamebanana":{
            "supports":false,
            "metadata":{"gamebanana":{
                "supports":true,
                "id":42,
                "model":"Mod"
            }}
        }
    }"#;
    let mapped = map_legacy_source(nested_gamebanana, Some(&archive_hash)).unwrap();
    assert_eq!(mapped.source().provider_id().as_str(), "gamebanana");

    let nested_conflict = br#"{
        "gamebanana":{
            "supports":false,
            "metadata":[
                {"provider":"nexus","id":42,"scope":"deltarune","url":"https://www.nexusmods.com/deltarune/mods/42"},
                {"provider":"moddb","id":"shared","scope":"games.deltarune.downloads","url":"https://www.moddb.com/games/deltarune/downloads/shared"}
            ]
        }
    }"#;
    assert!(map_legacy_source(nested_conflict, Some(&archive_hash)).is_err());

    let same_provider_consistent = br#"{
        "gamebanana":{
            "supports":false,
            "metadata":[
                {"provider":"nexus","id":42,"scope":"deltarune","url":"https://www.nexusmods.com/deltarune/mods/42"},
                {"provider":"nexus","modId":"042","domain":"DeltaRune","url":"https://www.nexusmods.com/deltarune/mods/42"}
            ]
        }
    }"#;
    let mapped = map_legacy_source(same_provider_consistent, Some(&archive_hash)).unwrap();
    assert_eq!(mapped.source().provider_id().as_str(), "nexus");

    let same_provider_conflict = br#"{
        "gamebanana":{
            "supports":false,
            "metadata":[
                {"provider":"nexus","id":42,"scope":"deltarune","url":"https://www.nexusmods.com/deltarune/mods/42"},
                {"provider":"nexus","id":43,"scope":"deltarune","url":"https://www.nexusmods.com/deltarune/mods/43"}
            ]
        }
    }"#;
    assert!(map_legacy_source(same_provider_conflict, Some(&archive_hash)).is_err());

    let mut deep = r#"{"provider":"nexus","id":42,"scope":"deltarune","url":"https://www.nexusmods.com/deltarune/mods/42"}"#.to_owned();
    for _ in 0..20 {
        deep = format!(r#"{{"metadata":{deep}}}"#);
    }
    let deep = format!(r#"{{"gamebanana":{{"supports":false,"metadata":{deep}}}}}"#);
    assert!(map_legacy_source(deep.as_bytes(), Some(&archive_hash)).is_err());

    let nodes = format!(
        r#"{{"gamebanana":{{"supports":false,"metadata":[{}]}}}}"#,
        vec!["null"; 4_096].join(",")
    );
    assert!(map_legacy_source(nodes.as_bytes(), Some(&archive_hash)).is_err());
}

#[test]
fn nexus_stable_identity_comes_only_from_record_metadata() {
    const TRANSIENT_SCOPE: &str = "temporary-domain-probe-91a7";
    let request = SearchRequest::new(
        KnownProvider::Nexus,
        TRANSIENT_SCOPE,
        "",
        SearchSort::Relevance,
        0,
        25,
    )
    .unwrap();
    let payload = br#"{
        "items":[{
            "modId":42,
            "gameDomainName":"deltarune",
            "game":{"domainName":"DeltaRune"},
            "sourceUrl":"https://www.nexusmods.com/deltarune/mods/42",
            "name":"Safe"
        }]
    }"#;
    let page = nexus::map_search(payload, &request).unwrap();
    assert_eq!(
        page.items()[0].source().scope().unwrap().as_str(),
        "deltarune"
    );
    assert_eq!(
        page.items()[0].source().canonical_url(),
        Some("https://www.nexusmods.com/deltarune/mods/42")
    );
    for stable in [
        format!("{request:?}"),
        format!("{:?}", request.cache_identity(KnownProvider::Nexus)),
        serialized(&request.cache_identity(KnownProvider::Nexus)),
        format!("{page:?}"),
        serialized(&page),
        serialized(page.cache_identity()),
        serialized(page.items()[0].source()),
    ] {
        assert!(!stable.contains(TRANSIENT_SCOPE));
    }
    let details = nexus::map_details(NEXUS_MOD, NEXUS_FILES, TRANSIENT_SCOPE).unwrap();
    assert_eq!(
        details.item().source().scope().unwrap().as_str(),
        "deltarune"
    );
    assert!(!serialized(&details).contains(TRANSIENT_SCOPE));
    assert!(!format!("{details:?}").contains(TRANSIENT_SCOPE));

    for credential_scope in [
        "token-supersecret",
        "access-token-scope-secret-91a7",
        "private-game-key-scope-secret-91a7",
    ] {
        let failure = SearchRequest::new(
            KnownProvider::Nexus,
            credential_scope,
            "",
            SearchSort::Relevance,
            0,
            25,
        )
        .unwrap_err();
        assert!(!format!("{failure:?} {failure}").contains(credential_scope));
    }

    let missing_metadata = br#"{"items":[{"modId":42,"name":"Missing"}]}"#;
    assert!(nexus::map_search(missing_metadata, &request).is_err());
    let conflicting_metadata = br#"{
        "items":[{
            "modId":42,
            "gameDomainName":"deltarune",
            "game":{"domainName":"undertale"},
            "name":"Conflict"
        }]
    }"#;
    assert!(nexus::map_search(conflicting_metadata, &request).is_err());

    const RECORD_SECRET: &str = "access-token-record-secret-91a7";
    let secret_record = format!(
        r#"{{"items":[{{"modId":42,"game":{{"domainName":"{RECORD_SECRET}"}},"name":"Secret"}}]}}"#
    );
    let failure = nexus::map_search(secret_record.as_bytes(), &request).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains(RECORD_SECRET));

    const SOURCE_URL_SECRET: &str = "source-url-secret-91a7";
    let signed_source_url = format!(
        r#"{{"items":[{{"modId":42,"gameDomainName":"deltarune","sourceUrl":"https://www.nexusmods.com/deltarune/mods/42?token={SOURCE_URL_SECRET}","name":"Secret"}}]}}"#
    );
    let failure = nexus::map_search(signed_source_url.as_bytes(), &request).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains(SOURCE_URL_SECRET));

    let legacy_secret = format!(
        r#"{{"source":{{"provider":"nexus","id":42,"scope":"{RECORD_SECRET}","url":"https://www.nexusmods.com/{RECORD_SECRET}/mods/42"}}}}"#
    );
    let failure = map_legacy_source(legacy_secret.as_bytes(), Some(&archive_hash())).unwrap_err();
    assert!(!format!("{failure:?} {failure}").contains(RECORD_SECRET));
}

fn archive_hash() -> String {
    "a".repeat(64)
}
