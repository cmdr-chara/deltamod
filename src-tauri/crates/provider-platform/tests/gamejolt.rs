use deltamod_product_contracts::{
    ProductErrorCode, ProviderAccountState, ProviderArtifactKind, ProviderAuthentication,
    ProviderCapability, ProviderItemKind,
};
use deltamod_provider_platform::{
    gamejolt::{self, BrowserHandoffKind},
    CapabilityAvailability, KnownProvider,
};
use url::Url;

const KNOWN_PROJECT: &[u8] = include_bytes!("fixtures/gamejolt-known-project.json");
const LOCAL_ARCHIVE: &[u8] = include_bytes!("fixtures/local-archive.json");
const ARCHIVE_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn fixture_normalizes_only_a_known_canonical_project() {
    let source = gamejolt::map_known_project(KNOWN_PROJECT).unwrap();
    assert_eq!(source.provider_id().as_str(), "gamejolt");
    assert_eq!(source.item_kind(), ProviderItemKind::Game);
    assert_eq!(source.resource_id().as_str(), "930477");
    assert!(source.scope().is_none());
    assert!(source.artifact_id().is_none());
    assert_eq!(source.artifact_kind(), ProviderArtifactKind::Unknown);
    assert!(source.version_id().is_none());
    assert_eq!(
        source.canonical_url(),
        Some("https://gamejolt.com/games/frickbears3/930477")
    );

    let serialized = serde_json::to_string(&source).unwrap();
    assert!(serialized.contains("https://gamejolt.com/games/frickbears3/930477"));
    assert!(!serialized.contains('?'));
    assert!(!serialized.contains('#'));
}

#[test]
fn known_project_validation_rejects_noncanonical_and_credentialed_urls() {
    for invalid in [
        "http://gamejolt.com/games/fixture/1",
        "https://www.gamejolt.com/games/fixture/1",
        "https://gamejolt.com.evil.example/games/fixture/1",
        "https://user:password@gamejolt.com/games/fixture/1",
        "https://gamejolt.com:444/games/fixture/1",
        "https://gamejolt.com/games/fixture/1?token=PROJECT_SECRET",
        "https://gamejolt.com/games/fixture/1#PROJECT_SECRET",
        "https://gamejolt.com/games/fixture/1/",
        "https://gamejolt.com/games/fixture/01",
        "https://gamejolt.com/games/fixture/0",
        "https://gamejolt.com/games/fixture",
        "https://gamejolt.com/p/fixture/1",
        "https://gamejolt.com/games/fixture%2fescape/1",
    ] {
        let failure = gamejolt::reference(invalid, ProviderItemKind::Game).unwrap_err();
        let rendered = format!("{failure:?} {failure}");
        assert!(!rendered.contains("PROJECT_SECRET"));
        assert!(!rendered.contains(invalid));
    }

    assert!(gamejolt::reference(
        "https://gamejolt.com/games/fixture/1",
        ProviderItemKind::LocalArchive,
    )
    .is_err());
}

#[test]
fn project_handoff_requires_the_exact_normalized_source_and_redacts_debug() {
    let source = gamejolt::map_known_project(KNOWN_PROJECT).unwrap();
    let handoff = gamejolt::project_handoff(&source).unwrap();
    assert_eq!(handoff.kind(), BrowserHandoffKind::KnownProject);
    assert_eq!(handoff.expose_url(), source.canonical_url().unwrap());
    assert_eq!(handoff.redacted_url(), "https://gamejolt.com/<redacted>");

    let debug = format!("{handoff:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("frickbears3"));
    assert!(!debug.contains("930477"));
}

#[test]
fn catalogue_search_is_an_encoded_nonpersistent_browser_handoff() {
    const QUERY: &str = "Delta Rune & Yellow";
    let handoff = gamejolt::catalogue_handoff(QUERY).unwrap();
    assert_eq!(handoff.kind(), BrowserHandoffKind::CatalogueSearch);
    let target = Url::parse(handoff.expose_url()).unwrap();
    assert_eq!(target.scheme(), "https");
    assert_eq!(target.host_str(), Some("gamejolt.com"));
    assert_eq!(target.path(), "/search/games");
    assert_eq!(
        target.query_pairs().collect::<Vec<_>>(),
        [("q".into(), QUERY.into())]
    );

    let rendered = format!("{handoff:?} {}", handoff.redacted_url());
    assert!(rendered.contains("<redacted>"));
    assert!(!rendered.contains("Delta"));
    assert!(!rendered.contains("Yellow"));
    assert!(!rendered.contains("q="));

    for invalid in [
        "",
        "token=CATALOGUE_SECRET",
        "https://gamejolt.com/games/fixture/1",
        "line\nbreak",
    ] {
        let failure = gamejolt::catalogue_handoff(invalid).unwrap_err();
        let rendered = format!("{failure:?} {failure}");
        assert!(!rendered.contains("CATALOGUE_SECRET"));
        if !invalid.is_empty() {
            assert!(!rendered.contains(invalid));
        }
    }
    assert!(gamejolt::catalogue_handoff(&"x".repeat(257)).is_err());
}

#[test]
fn local_archive_handoff_stays_owned_by_the_local_provider() {
    let details = gamejolt::local_archive_handoff(LOCAL_ARCHIVE, ARCHIVE_SHA256).unwrap();
    assert_eq!(details.item().source().provider_id().as_str(), "local");
    assert_eq!(
        details.item().source().item_kind(),
        ProviderItemKind::LocalArchive
    );
    assert_eq!(details.versions().len(), 1);
    assert!(!details.versions()[0].directly_downloadable());
    assert_eq!(details.versions()[0].sha256(), Some(ARCHIVE_SHA256));
}

#[test]
fn capabilities_do_not_claim_native_search_download_or_credentials() {
    let descriptor = KnownProvider::GameJolt.descriptor();
    assert_eq!(descriptor.authentication, ProviderAuthentication::None);
    assert!(descriptor
        .capabilities
        .contains(&ProviderCapability::ExternalDownload));
    assert!(descriptor
        .capabilities
        .contains(&ProviderCapability::GameAcquisition));
    assert!(!descriptor
        .capabilities
        .contains(&ProviderCapability::Search));
    assert!(!descriptor
        .capabilities
        .contains(&ProviderCapability::DirectDownload));
    assert!(!descriptor
        .capabilities
        .contains(&ProviderCapability::Authentication));

    let report = gamejolt::capability_report();
    assert_eq!(report.search, CapabilityAvailability::Unavailable);
    assert_eq!(report.details, CapabilityAvailability::Unavailable);
    assert_eq!(report.versions, CapabilityAvailability::Unavailable);
    assert_eq!(report.direct_download, CapabilityAvailability::Unavailable);
    assert_eq!(report.external_download, CapabilityAvailability::Available);
    assert_eq!(report.local_import, CapabilityAvailability::Unavailable);
    assert_eq!(
        gamejolt::account().state(),
        ProviderAccountState::NotRequired
    );
}

#[test]
fn malformed_or_api_shaped_fixtures_return_safe_normalized_errors() {
    let malformed = gamejolt::map_known_project(b"{").unwrap_err();
    assert_eq!(malformed.contract().code, ProductErrorCode::InvalidRequest);
    let rendered = format!("{malformed:?} {malformed}");
    assert!(!rendered.contains("serde"));
    assert!(!rendered.contains("line"));
    assert!(!rendered.contains("column"));

    let api_shaped = br#"{
        "projectUrl":"https://gamejolt.com/games/fixture/1",
        "itemKind":"game",
        "gameApiKey":"GAME_API_SECRET"
    }"#;
    let failure = gamejolt::map_known_project(api_shaped).unwrap_err();
    let rendered = format!("{failure:?} {failure}");
    assert!(!rendered.contains("GAME_API_SECRET"));
    assert!(!rendered.contains("gameApiKey"));
}
