//! Bounded Game Jolt source normalization and explicit handoffs.
//!
//! Game Jolt does not expose a documented third-party catalogue or package-download API for
//! this adapter. The only supported paths are a validated, already-known project URL, a browser
//! catalogue handoff, and delegation of a user-selected archive to the local provider.

use crate::{
    local,
    model::{decode_json, provider_ref, required_stable_text, ProviderAccount, ProviderDetails},
    url_policy::canonical_url,
    KnownProvider, ProviderCapabilityReport, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{
    ProviderAccountState, ProviderArtifactKind, ProviderItemKind, ProviderRef,
};
use serde::Deserialize;
use std::fmt;
use url::Url;

const PROVIDER: KnownProvider = KnownProvider::GameJolt;
const GAMEJOLT_ROOTS: &[&str] = &["gamejolt.com"];
const GAMEJOLT_HOST: &str = "gamejolt.com";
const CATALOGUE_SEARCH_URL: &str = "https://gamejolt.com/search/games";
const MAX_PROJECT_URL_BYTES: usize = 512;
const MAX_PROJECT_SLUG_BYTES: usize = 100;
const MAX_CATALOGUE_QUERY_BYTES: usize = 256;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnownProjectFixture {
    project_url: String,
    item_kind: ProviderItemKind,
}

struct ProjectIdentity {
    resource_id: String,
    canonical_url: Url,
}

/// The only browser destinations emitted by this adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserHandoffKind {
    KnownProject,
    CatalogueSearch,
}

/// An external-browser target. It intentionally implements neither `Serialize` nor `Display`;
/// callers must explicitly opt in with [`BrowserHandoff::expose_url`]. Debug output contains no
/// path or query data.
///
/// ```compile_fail
/// use deltamod_provider_platform::gamejolt;
///
/// let handoff = gamejolt::catalogue_handoff("fixture game").unwrap();
/// let _serialized = serde_json::to_string(&handoff).unwrap();
/// ```
#[derive(Clone, Eq, PartialEq)]
pub struct BrowserHandoff {
    kind: BrowserHandoffKind,
    target: Url,
}

impl BrowserHandoff {
    fn new(kind: BrowserHandoffKind, target: Url) -> Self {
        Self { kind, target }
    }

    #[must_use]
    pub const fn kind(&self) -> BrowserHandoffKind {
        self.kind
    }

    /// Exposes the URL only to the immediate external-browser adapter. Do not persist or log it.
    #[must_use]
    pub fn expose_url(&self) -> &str {
        self.target.as_str()
    }

    #[must_use]
    pub fn redacted_url(&self) -> String {
        format!("https://{GAMEJOLT_HOST}/<redacted>")
    }
}

impl fmt::Debug for BrowserHandoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BrowserHandoff")
            .field("kind", &self.kind)
            .field("target", &self.redacted_url())
            .finish()
    }
}

/// Normalizes an explicitly configured project fixture. This is local configuration input, not
/// a Game Jolt API response.
pub fn map_known_project(payload: &[u8]) -> ProviderResult<ProviderRef> {
    let fixture: KnownProjectFixture = decode_json(PROVIDER, payload)?;
    normalized_reference(&fixture.project_url, fixture.item_kind)
}

/// Builds a stable reference for an already-known public Game Jolt project page.
pub fn reference(project_url: &str, item_kind: ProviderItemKind) -> ProviderResult<ProviderRef> {
    normalized_reference(project_url, item_kind)
        .map_err(|_| ProviderFailure::invalid_request(PROVIDER))
}

/// Produces an external-browser handoff for a previously normalized project reference.
pub fn project_handoff(source: &ProviderRef) -> ProviderResult<BrowserHandoff> {
    if source.validate().is_err()
        || source.provider_id().as_str() != PROVIDER.as_str()
        || !matches!(
            source.item_kind(),
            ProviderItemKind::Game | ProviderItemKind::Mod
        )
        || source.scope().is_some()
        || source.artifact_id().is_some()
        || source.artifact_kind() != ProviderArtifactKind::Unknown
        || source.version_id().is_some()
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }

    let identity = source
        .canonical_url()
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))
        .and_then(|url| {
            project_identity(url).map_err(|_| ProviderFailure::invalid_request(PROVIDER))
        })?;
    if identity.resource_id != source.resource_id().as_str() {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }

    Ok(BrowserHandoff::new(
        BrowserHandoffKind::KnownProject,
        identity.canonical_url,
    ))
}

/// Produces a browser-only catalogue search target. No provider response is fetched or parsed,
/// and the normalized capability report continues to mark native search as unavailable.
pub fn catalogue_handoff(query: &str) -> ProviderResult<BrowserHandoff> {
    let query = required_stable_text(query, MAX_CATALOGUE_QUERY_BYTES)
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    let mut target =
        Url::parse(CATALOGUE_SEARCH_URL).expect("the static Game Jolt catalogue URL is valid");
    target.query_pairs_mut().append_pair("q", &query);
    Ok(BrowserHandoff::new(
        BrowserHandoffKind::CatalogueSearch,
        target,
    ))
}

/// Delegates a user-selected archive to the existing local provider normalizer. Runtime callers
/// must continue with the ordinary local-archive lifecycle preflight; this does not resolve a
/// Game Jolt package or download URL.
pub fn local_archive_handoff(
    payload: &[u8],
    archive_sha256: &str,
) -> ProviderResult<ProviderDetails> {
    local::map_archive(payload, archive_sha256)
}

#[must_use]
pub fn account() -> ProviderAccount {
    ProviderAccount::not_required(PROVIDER)
}

#[must_use]
pub const fn capability_report() -> ProviderCapabilityReport {
    PROVIDER.capability_report(ProviderAccountState::NotRequired)
}

fn normalized_reference(
    project_url: &str,
    item_kind: ProviderItemKind,
) -> ProviderResult<ProviderRef> {
    if !matches!(item_kind, ProviderItemKind::Game | ProviderItemKind::Mod) {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let identity =
        project_identity(project_url).map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    provider_ref(
        PROVIDER,
        item_kind,
        &identity.resource_id,
        None,
        None,
        ProviderArtifactKind::Unknown,
        None,
        Some(identity.canonical_url.into()),
    )
}

fn project_identity(raw: &str) -> Result<ProjectIdentity, ()> {
    if raw.is_empty()
        || raw.len() > MAX_PROJECT_URL_BYTES
        || raw.chars().any(char::is_control)
        || raw.contains(['?', '#', '%', '\\'])
    {
        return Err(());
    }

    let canonical_url = canonical_url(raw, GAMEJOLT_ROOTS)?;
    if canonical_url.host_str() != Some(GAMEJOLT_HOST) || canonical_url.as_str() != raw {
        return Err(());
    }
    let segments = canonical_url.path_segments().ok_or(())?.collect::<Vec<_>>();
    let [root, slug, resource_id] = segments.as_slice() else {
        return Err(());
    };
    if *root != "games"
        || slug.is_empty()
        || slug.len() > MAX_PROJECT_SLUG_BYTES
        || !slug
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(());
    }
    let numeric_id = resource_id
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or(())?;
    if numeric_id.to_string() != *resource_id {
        return Err(());
    }

    Ok(ProjectIdentity {
        resource_id: resource_id.to_string(),
        canonical_url,
    })
}
