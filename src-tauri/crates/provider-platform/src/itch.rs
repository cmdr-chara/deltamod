//! Bounded itch.io integration for known projects and the documented `wharf/latest` API.
//!
//! This module deliberately does not implement catalogue search or package downloads. The
//! public API does not document either capability for third-party consumers.

use crate::{
    model::{
        decode_json, provider_ref, ContentRating, ProviderAccount, ProviderItem, ProviderItemInput,
    },
    text_policy::normalize_version_label,
    KnownProvider, ProviderFailure, ProviderResult,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use deltamod_product_contracts::{
    ProviderArtifactKind, ProviderItemKind, ProviderRef, ProviderResourceId,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use url::Url;

const PROVIDER: KnownProvider = KnownProvider::Itch;
const MAX_PROJECT_URL_BYTES: usize = 512;
const MAX_SLUG_BYTES: usize = 80;
const MAX_TAG_BYTES: usize = 80;
const MAX_CHANNEL_BYTES: usize = 128;
const MAX_RSS_BYTES: usize = 1024 * 1024;
const MAX_RSS_NODES: usize = 20_000;
const MAX_RSS_DEPTH: usize = 32;
const MAX_RSS_ITEMS: usize = 100;
const VERSION_ID_PREFIX: &str = ".itch-b64.";

#[derive(Clone, Eq, PartialEq)]
struct ProjectIdentity {
    creator: String,
    project: String,
    canonical_url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItchLatestEnvelope {
    #[serde(default, deserialize_with = "deserialize_present_latest")]
    latest: Option<String>,
}

fn deserialize_present_latest<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

/// Transient request for itch.io's documented unauthenticated latest-version endpoint.
/// It intentionally implements neither `Serialize` nor `Display` and redacts request values.
#[derive(Clone, Eq, PartialEq)]
pub struct ItchLatestRequest {
    url: Url,
}

impl ItchLatestRequest {
    /// Exposes the complete request only to the immediate HTTP adapter.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.url.as_str()
    }
}

impl fmt::Debug for ItchLatestRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItchLatestRequest")
            .field("endpoint", &"https://api.itch.io/wharf/latest?<redacted>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItchFeedKind {
    New,
    Featured,
    Sales,
}

impl ItchFeedKind {
    const fn path(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Featured => "featured",
            Self::Sales => "sales",
        }
    }
}

/// Fixed official RSS request. It has no user-controlled query or credentials.
#[derive(Clone, Eq, PartialEq)]
pub struct ItchRssRequest {
    url: Url,
    kind: ItchFeedKind,
}

/// Official itch.io tag feed selected from a packaged game definition.
/// The tag is validated before it can become part of either URL.
#[derive(Clone, Eq, PartialEq)]
pub struct ItchTagRssRequest {
    url: Url,
    catalogue_url: Url,
    tag: String,
}

impl ItchTagRssRequest {
    #[must_use]
    pub fn expose(&self) -> &str {
        self.url.as_str()
    }

    #[must_use]
    pub fn catalogue_url(&self) -> &str {
        self.catalogue_url.as_str()
    }

    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }
}

impl fmt::Debug for ItchTagRssRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItchTagRssRequest")
            .field("endpoint", &"https://itch.io/games/tag-<game>.xml")
            .finish()
    }
}

impl ItchRssRequest {
    #[must_use]
    pub fn expose(&self) -> &str {
        self.url.as_str()
    }

    #[must_use]
    pub const fn kind(&self) -> ItchFeedKind {
        self.kind
    }
}

impl fmt::Debug for ItchRssRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItchRssRequest")
            .field("kind", &self.kind)
            .field("endpoint", &"https://itch.io/feed/<kind>.xml")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchFeedPage {
    kind: ItchFeedKind,
    items: Vec<ProviderItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchTagPage {
    tag: String,
    items: Vec<ProviderItem>,
}

impl ItchTagPage {
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    #[must_use]
    pub fn items(&self) -> &[ProviderItem] {
        &self.items
    }
}

impl ItchFeedPage {
    #[must_use]
    pub const fn kind(&self) -> ItchFeedKind {
        self.kind
    }

    #[must_use]
    pub fn items(&self) -> &[ProviderItem] {
        &self.items
    }
}

/// Stable, credential-free result suitable for update comparison and persistence.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchLatestVersion {
    source: ProviderRef,
    channel_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
}

impl ItchLatestVersion {
    #[must_use]
    pub fn source(&self) -> &ProviderRef {
        &self.source
    }

    #[must_use]
    pub fn channel_name(&self) -> &str {
        &self.channel_name
    }

    #[must_use]
    pub fn latest(&self) -> Option<&str> {
        self.latest.as_deref()
    }
}

/// Creates a stable reference for one known public itch.io project page.
pub fn project_reference(project_url: &str) -> ProviderResult<ProviderRef> {
    let identity = project_identity_from_url(project_url)?;
    reference(&identity, None, None)
}

/// Builds the documented, unauthenticated latest-version request for a known target/channel.
pub fn latest_request(target: &str, channel_name: &str) -> ProviderResult<ItchLatestRequest> {
    let identity = project_identity_from_target(target)?;
    let channel_name = channel(channel_name)?;
    let mut url = Url::parse("https://api.itch.io/wharf/latest")
        .expect("the fixed itch.io latest endpoint is valid");
    url.query_pairs_mut()
        .append_pair(
            "target",
            &format!("{}/{}", identity.creator, identity.project),
        )
        .append_pair("channel_name", &channel_name);
    Ok(ItchLatestRequest { url })
}

/// Normalizes a bounded JSON response from the documented `wharf/latest` endpoint.
pub fn map_latest(
    payload: &[u8],
    target: &str,
    channel_name: &str,
) -> ProviderResult<ItchLatestVersion> {
    let identity = project_identity_from_target(target)?;
    let channel_name = channel(channel_name)?;
    let envelope: ItchLatestEnvelope = decode_json(PROVIDER, payload)?;
    let latest = envelope
        .latest
        .map(|value| {
            normalize_version_label(&value)
                .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))
        })
        .transpose()?;
    let source = reference(&identity, Some(&channel_name), latest.as_deref())?;
    Ok(ItchLatestVersion {
        source,
        channel_name,
        latest,
    })
}

#[must_use]
pub fn rss_request(kind: ItchFeedKind) -> ItchRssRequest {
    let url = Url::parse(&format!("https://itch.io/feed/{}.xml", kind.path()))
        .expect("fixed itch.io feed URLs are valid");
    ItchRssRequest { url, kind }
}

/// Builds the official RSS endpoint for a bounded game tag from trusted game metadata.
pub fn tag_rss_request(tag: &str) -> ProviderResult<ItchTagRssRequest> {
    let tag = game_tag(tag)?;
    let url = Url::parse(&format!("https://itch.io/games/tag-{tag}.xml"))
        .expect("validated itch.io tag feed URLs are valid");
    let catalogue_url = Url::parse(&format!("https://itch.io/games/tag-{tag}"))
        .expect("validated itch.io tag catalogue URLs are valid");
    Ok(ItchTagRssRequest {
        url,
        catalogue_url,
        tag: tag.to_owned(),
    })
}

/// Parses a bounded official RSS response without retaining arbitrary HTML or image URLs.
pub fn map_rss(payload: &[u8], kind: ItchFeedKind) -> ProviderResult<ItchFeedPage> {
    let items = map_rss_items(payload, kind == ItchFeedKind::Featured)?;
    Ok(ItchFeedPage { kind, items })
}

/// Parses an official game-tag RSS response without treating unrelated Featured games as mods.
pub fn map_tag_rss(payload: &[u8], tag: &str) -> ProviderResult<ItchTagPage> {
    let tag = game_tag(tag)?.to_owned();
    let items = map_rss_items(payload, false)?;
    Ok(ItchTagPage { tag, items })
}

fn map_rss_items(payload: &[u8], featured: bool) -> ProviderResult<Vec<ProviderItem>> {
    if payload.is_empty() || payload.len() > MAX_RSS_BYTES {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let xml =
        std::str::from_utf8(payload).map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    let lowercase = xml.to_ascii_lowercase();
    if lowercase.contains("<!doctype") || lowercase.contains("<!entity") {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let document =
        roxmltree::Document::parse(xml).map_err(|_| ProviderFailure::invalid_payload(PROVIDER))?;
    let mut node_count = 0_usize;
    for node in document.descendants() {
        node_count += 1;
        if node_count > MAX_RSS_NODES || node.ancestors().count() > MAX_RSS_DEPTH {
            return Err(ProviderFailure::invalid_payload(PROVIDER));
        }
    }

    let root = document.root_element();
    if root.tag_name().name() != "rss" {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let channels = root
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "channel")
        .collect::<Vec<_>>();
    let [channel_node] = channels.as_slice() else {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    };
    let item_nodes = channel_node
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "item")
        .collect::<Vec<_>>();
    if item_nodes.len() > MAX_RSS_ITEMS {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    let mut items = Vec::with_capacity(item_nodes.len());
    for item in item_nodes {
        let Some((title, identity)) = one_child_text(item, "title")
            .and_then(|title| {
                one_child_text(item, "link")
                    .and_then(project_identity_from_url)
                    .map(|identity| (title, identity))
            })
            .ok()
        else {
            continue;
        };
        let Ok(source) = reference(&identity, None, None) else {
            continue;
        };
        let Ok(provider_item) = ProviderItem::from_input(
            PROVIDER,
            ProviderItemInput {
                source,
                title: title.to_owned(),
                summary: None,
                author: Some(identity.creator),
                updated_at: None,
                content_rating: ContentRating::Unknown,
                downloads: None,
                endorsements: None,
                featured,
            },
        ) else {
            continue;
        };
        items.push(provider_item);
    }
    Ok(items)
}

#[must_use]
pub fn account() -> ProviderAccount {
    ProviderAccount::not_required(PROVIDER)
}

#[must_use]
pub fn unsupported_public_search() -> ProviderFailure {
    ProviderFailure::unsupported(PROVIDER)
}

fn reference(
    identity: &ProjectIdentity,
    channel_name: Option<&str>,
    latest: Option<&str>,
) -> ProviderResult<ProviderRef> {
    let version_id = latest.map(|value| {
        if ProviderResourceId::parse(value).is_ok() {
            value.to_owned()
        } else {
            format!(
                "{VERSION_ID_PREFIX}{}",
                URL_SAFE_NO_PAD.encode(value.as_bytes())
            )
        }
    });
    provider_ref(
        PROVIDER,
        ProviderItemKind::Game,
        &identity.project,
        Some(&identity.creator),
        channel_name,
        if channel_name.is_some() {
            ProviderArtifactKind::Build
        } else {
            ProviderArtifactKind::Unknown
        },
        version_id.as_deref(),
        Some(identity.canonical_url.clone()),
    )
}

fn project_identity_from_target(target: &str) -> ProviderResult<ProjectIdentity> {
    if target.len() > MAX_SLUG_BYTES * 2 + 1 || target.chars().any(char::is_control) {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let (creator, project) = target
        .split_once('/')
        .filter(|(_, project)| !project.contains('/'))
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    let creator = slug(creator).ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    let project = slug(project).ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    Ok(ProjectIdentity {
        creator: creator.to_owned(),
        project: project.to_owned(),
        canonical_url: format!("https://{creator}.itch.io/{project}"),
    })
}

fn project_identity_from_url(raw: &str) -> ProviderResult<ProjectIdentity> {
    if raw.is_empty()
        || raw.len() > MAX_PROJECT_URL_BYTES
        || raw.chars().any(char::is_control)
        || raw.contains(['\\', '%'])
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let url = Url::parse(raw).map_err(|_| ProviderFailure::invalid_request(PROVIDER))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let host = url
        .host_str()
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    let mut host_parts = host.split('.');
    let creator = host_parts
        .next()
        .and_then(slug)
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    if host_parts.next() != Some("itch")
        || host_parts.next() != Some("io")
        || host_parts.next().is_some()
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    let project = url
        .path()
        .strip_prefix('/')
        .and_then(|path| path.strip_suffix('/').or(Some(path)))
        .filter(|path| !path.contains('/'))
        .and_then(slug)
        .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))?;
    Ok(ProjectIdentity {
        creator: creator.to_owned(),
        project: project.to_owned(),
        canonical_url: format!("https://{creator}.itch.io/{project}"),
    })
}

fn slug(value: &str) -> Option<&str> {
    (!value.is_empty()
        && value.len() <= MAX_SLUG_BYTES
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    .then_some(value)
}

fn game_tag(value: &str) -> ProviderResult<&str> {
    (!value.is_empty()
        && value.len() <= MAX_TAG_BYTES
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'))
    .then_some(value)
    .ok_or_else(|| ProviderFailure::invalid_request(PROVIDER))
}

fn channel(value: &str) -> ProviderResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_CHANNEL_BYTES
        || value.chars().any(char::is_control)
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return Err(ProviderFailure::invalid_request(PROVIDER));
    }
    Ok(value.to_owned())
}

fn one_child_text<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    name: &str,
) -> ProviderResult<&'a str> {
    let mut values = parent
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == name)
        .filter_map(|node| node.text().map(str::trim))
        .filter(|value| !value.is_empty());
    let value = values
        .next()
        .ok_or_else(|| ProviderFailure::invalid_payload(PROVIDER))?;
    if values.next().is_some() {
        return Err(ProviderFailure::invalid_payload(PROVIDER));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST: &[u8] = include_bytes!("../tests/fixtures/itch-latest.json");
    const RSS: &[u8] = include_bytes!("../tests/fixtures/itch-featured.xml");

    #[test]
    fn maps_known_project_and_latest_channel_without_claiming_a_download() {
        let result = map_latest(LATEST, "creator/example-mod", "windows-stable").unwrap();
        assert_eq!(result.latest(), Some("1.2.3"));
        assert_eq!(result.channel_name(), "windows-stable");
        assert_eq!(result.source().provider_id().as_str(), "itch");
        assert_eq!(result.source().resource_id().as_str(), "example-mod");
        assert_eq!(result.source().scope().unwrap().as_str(), "creator");
        assert_eq!(result.source().version_id().unwrap().as_str(), "1.2.3");
        assert_eq!(
            result.source().canonical_url(),
            Some("https://creator.itch.io/example-mod")
        );
    }

    #[test]
    fn latest_request_is_explicitly_exposed_and_debug_redacted() {
        let request = latest_request("creator/example-mod", "windows-stable").unwrap();
        assert_eq!(
            request.expose(),
            "https://api.itch.io/wharf/latest?target=creator%2Fexample-mod&channel_name=windows-stable"
        );
        let debug = format!("{request:?}");
        assert!(!debug.contains("creator"));
        assert!(!debug.contains("windows-stable"));
    }

    #[test]
    fn rejects_noncanonical_or_credential_bearing_project_routes() {
        for url in [
            "http://creator.itch.io/example-mod",
            "https://creator.itch.io/example-mod?token=secret",
            "https://token@creator.itch.io/example-mod",
            "https://nested.creator.itch.io/example-mod",
            "https://creator.itch.io/example-mod/extra",
            "https://creator.itch.io/%65xample-mod",
        ] {
            assert!(project_reference(url).is_err(), "accepted {url}");
        }
    }

    #[test]
    fn omitted_latest_version_remains_a_valid_no_update_result() {
        let result = map_latest(b"{}", "creator/example-mod", "linux").unwrap();
        assert_eq!(result.latest(), None);
        assert_eq!(result.source().artifact_id().unwrap().as_str(), "linux");
        assert!(result.source().version_id().is_none());
    }

    #[test]
    fn latest_rejects_error_and_unknown_envelopes() {
        for payload in [
            br#"{"error":"not found"}"#.as_slice(),
            br#"{"message":"not found"}"#.as_slice(),
            br#"{"errors":["not found"]}"#.as_slice(),
            br#"{"unknown":true}"#.as_slice(),
            br#"{"latest":"1.2.3","message":"unexpected"}"#.as_slice(),
            br#"{"latest":null}"#.as_slice(),
        ] {
            assert!(
                map_latest(payload, "creator/example-mod", "linux").is_err(),
                "accepted {}",
                String::from_utf8_lossy(payload)
            );
        }
    }

    #[test]
    fn version_ids_preserve_normalized_identity_round_trip() {
        let result = map_latest(
            br#"{"latest":"Release   Candidate build+1"}"#,
            "creator/example-mod",
            "linux",
        )
        .unwrap();
        let normalized = result.latest().unwrap();
        assert_eq!(normalized, "Release Candidate build+1");

        let version_id = result.source().version_id().unwrap().as_str();
        let encoded = version_id.strip_prefix(VERSION_ID_PREFIX).unwrap();
        assert_eq!(
            String::from_utf8(URL_SAFE_NO_PAD.decode(encoded).unwrap()).unwrap(),
            normalized
        );
        assert!(ProviderResourceId::parse(version_id).is_ok());
    }

    #[test]
    fn maps_bounded_official_rss_to_credential_free_items() {
        let request = rss_request(ItchFeedKind::Featured);
        assert_eq!(request.expose(), "https://itch.io/feed/featured.xml");
        let page = map_rss(RSS, request.kind()).unwrap();
        assert_eq!(page.items().len(), 2);
        assert_eq!(page.items()[0].title(), "Example Mod");
        assert_eq!(page.items()[0].author(), Some("creator"));
        assert!(page.items()[0].featured());
    }

    #[test]
    fn maps_a_game_tag_feed_without_featured_semantics() {
        let request = tag_rss_request("deltarune").unwrap();
        assert_eq!(request.expose(), "https://itch.io/games/tag-deltarune.xml");
        assert_eq!(
            request.catalogue_url(),
            "https://itch.io/games/tag-deltarune"
        );
        let page = map_tag_rss(RSS, request.tag()).unwrap();
        assert_eq!(page.tag(), "deltarune");
        assert_eq!(page.items().len(), 2);
        assert!(!page.items()[0].featured());
    }

    #[test]
    fn skips_one_non_contract_item_without_rejecting_the_game_tag_feed() {
        let payload = br#"<rss><channel>
            <item><title>Path/shaped title</title><link>https://bad.itch.io/rejected</link></item>
            <item><title>Valid Project</title><link>https://creator.itch.io/valid-project</link></item>
        </channel></rss>"#;
        let page = map_tag_rss(payload, "undertale").unwrap();
        assert_eq!(page.items().len(), 1);
        assert_eq!(page.items()[0].title(), "Valid Project");
    }

    #[test]
    fn rejects_unbounded_or_noncanonical_game_tags() {
        for tag in [
            "",
            "DELTARUNE",
            "../deltarune",
            "deltarune.xml",
            "two words",
        ] {
            assert!(tag_rss_request(tag).is_err(), "accepted {tag}");
        }
    }

    #[test]
    fn rss_rejects_dtds_and_excess_items() {
        assert!(map_rss(
            b"<!DOCTYPE rss [<!ENTITY x SYSTEM 'file:///secret'>]><rss/>",
            ItchFeedKind::New
        )
        .is_err());
        let oversized = format!(
            "<rss><channel>{}</channel></rss>",
            "<item><title>x</title><link>https://a.itch.io/b</link></item>"
                .repeat(MAX_RSS_ITEMS + 1)
        );
        assert!(map_rss(oversized.as_bytes(), ItchFeedKind::New).is_err());
    }
}
