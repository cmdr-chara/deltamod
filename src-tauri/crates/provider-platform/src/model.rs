use crate::{
    digest::sha256_framed_hex,
    text_policy::{
        credential_shaped_identifier, credential_shaped_scope, credential_shaped_text,
        normalize_version_label, safe_stable_basename, stable_text_allowed,
    },
    url_policy::EphemeralDownloadUrl,
    KnownProvider, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{
    ProviderAccountState, ProviderArtifactKind, ProviderItemKind, ProviderRef, ProviderResourceId,
    ProviderScope,
};
use serde::{
    de::{DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize, Serialize,
};
use serde_json::Value;
use std::{collections::BTreeSet, fmt};
use url::Url;

const MAX_FIXTURE_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_NODES: usize = 20_000;
const MAX_JSON_SEQUENCE_ITEMS: usize = 4_096;
const MAX_JSON_OBJECT_MEMBERS: usize = 1_024;
const MAX_JSON_KEY_BYTES: usize = 128;
const MAX_SCOPE_BYTES: usize = 128;
const MAX_QUERY_BYTES: usize = 256;
pub(crate) const MAX_RESULT_ITEMS: usize = 200;
pub(crate) const MAX_VERSION_ENTRIES: usize = 128;
const SEARCH_CACHE_DOMAIN: &[u8] = b"deltamod.provider-platform/search-cache/v3";
const PAGE_SCOPE_DOMAIN: &[u8] = b"deltamod.provider-platform/page-scope/v1";
const ITEM_CACHE_DOMAIN: &[u8] = b"deltamod.provider-platform/item-cache/v2";
const VERSION_CACHE_DOMAIN: &[u8] = b"deltamod.provider-platform/version-cache/v2";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    Relevance,
    LatestAdded,
    LatestUpdated,
    Trending,
}

/// Transient operational search input. It must never become a stable serialized artifact.
///
/// ```compile_fail
/// use deltamod_provider_platform::{KnownProvider, SearchRequest, SearchSort};
///
/// let request = SearchRequest::new(
///     KnownProvider::Nexus,
///     "deltarune",
///     "transient query",
///     SearchSort::Relevance,
///     0,
///     25,
/// ).unwrap();
/// let _serialized = serde_json::to_string(&request).unwrap();
/// ```
#[derive(Clone, Eq, PartialEq)]
pub struct SearchRequest {
    provider: KnownProvider,
    scope: String,
    query: String,
    sort: SearchSort,
    offset: u32,
    limit: u32,
}

impl SearchRequest {
    pub fn new(
        provider: KnownProvider,
        scope: impl AsRef<str>,
        query: impl AsRef<str>,
        sort: SearchSort,
        offset: u32,
        limit: u32,
    ) -> ProviderResult<Self> {
        let raw_scope = scope.as_ref();
        let raw_query = query.as_ref();
        if raw_scope.len() > MAX_SCOPE_BYTES
            || raw_query.len() > MAX_QUERY_BYTES
            || raw_scope.chars().any(char::is_control)
            || raw_query.chars().any(char::is_control)
            || credential_bearing_url(raw_scope)
            || credential_bearing_url(raw_query)
            || offset > 1_000_000
            || !(1..=50).contains(&limit)
        {
            return Err(ProviderFailure::invalid_request(provider));
        }
        let scope = canonical_search_scope(provider, raw_scope)?;
        let query = normalize_whitespace(raw_query);
        Ok(Self {
            provider,
            scope,
            query,
            sort,
            offset,
            limit,
        })
    }

    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub const fn sort(&self) -> SearchSort {
        self.sort
    }

    #[must_use]
    pub const fn offset(&self) -> u32 {
        self.offset
    }

    #[must_use]
    pub const fn limit(&self) -> u32 {
        self.limit
    }

    pub(crate) fn validate_for(&self, provider: KnownProvider) -> ProviderResult<()> {
        let valid_scope = canonical_search_scope(provider, &self.scope)
            .is_ok_and(|canonical| canonical == self.scope);
        if self.provider != provider || !valid_scope {
            Err(ProviderFailure::invalid_request(provider))
        } else {
            Ok(())
        }
    }

    #[must_use]
    pub fn cache_identity(&self, provider: KnownProvider) -> CacheIdentity {
        let normalized_query = self.query.to_lowercase();
        let offset = self.offset.to_be_bytes();
        let limit = self.limit.to_be_bytes();
        CacheIdentity(sha256_framed_hex(
            SEARCH_CACHE_DOMAIN,
            &[
                self.provider.as_str().as_bytes(),
                provider.as_str().as_bytes(),
                self.scope.as_bytes(),
                normalized_query.as_bytes(),
                search_sort_name(self.sort).as_bytes(),
                &offset,
                &limit,
            ],
        ))
    }

    fn scope_digest(&self, provider: KnownProvider) -> CacheIdentity {
        CacheIdentity(sha256_framed_hex(
            PAGE_SCOPE_DOMAIN,
            &[
                self.provider.as_str().as_bytes(),
                provider.as_str().as_bytes(),
                self.scope.as_bytes(),
            ],
        ))
    }
}

impl fmt::Debug for SearchRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchRequest")
            .field("provider", &self.provider)
            .field("scope", &"<redacted>")
            .field(
                "query",
                &if self.query.is_empty() {
                    "<empty>"
                } else {
                    "<redacted>"
                },
            )
            .field("sort", &self.sort)
            .field("offset", &self.offset)
            .field("limit", &self.limit)
            .finish()
    }
}

fn canonical_search_scope(provider: KnownProvider, value: &str) -> ProviderResult<String> {
    let value = value.trim();
    let canonical = match provider {
        KnownProvider::GameBanana => value
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(|id| id.to_string()),
        KnownProvider::Nexus | KnownProvider::ModDb => {
            if value.len() > 80 || credential_shaped_scope(value) {
                return Err(ProviderFailure::invalid_request(provider));
            }
            let canonical = value.to_ascii_lowercase();
            (canonical
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && canonical
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && ProviderScope::parse(&canonical).is_ok())
            .then_some(canonical)
        }
        KnownProvider::GameJolt | KnownProvider::Itch | KnownProvider::Local => None,
    };
    canonical.ok_or_else(|| ProviderFailure::invalid_request(provider))
}

fn credential_bearing_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return true;
    }
    if url.query_pairs().any(|(key, _)| sensitive_url_key(&key)) {
        return true;
    }
    url.fragment().is_some_and(|fragment| {
        let fragment = fragment.to_ascii_lowercase();
        [
            "token=",
            "password=",
            "passwd=",
            "secret=",
            "signature=",
            "authorization=",
            "credential=",
        ]
        .iter()
        .any(|marker| fragment.contains(marker))
    })
}

fn sensitive_url_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '.'], "_");
    matches!(
        normalized.as_str(),
        "token"
            | "access_token"
            | "auth_token"
            | "api_key"
            | "apikey"
            | "password"
            | "passwd"
            | "secret"
            | "signature"
            | "authorization"
            | "credential"
    ) || normalized.ends_with("_token")
        || normalized.ends_with("_secret")
}

fn search_sort_name(sort: SearchSort) -> &'static str {
    match sort {
        SearchSort::Relevance => "relevance",
        SearchSort::LatestAdded => "latest_added",
        SearchSort::LatestUpdated => "latest_updated",
        SearchSort::Trending => "trending",
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CacheIdentity(String);

impl CacheIdentity {
    #[must_use]
    pub fn for_item(source: &ProviderRef) -> Self {
        Self(sha256_framed_hex(
            ITEM_CACHE_DOMAIN,
            &[source.canonical_identity().as_bytes()],
        ))
    }

    #[must_use]
    pub fn for_version(source: &ProviderRef) -> Self {
        let canonical_identity = source.canonical_identity();
        Self(sha256_framed_hex(
            VERSION_CACHE_DOMAIN,
            &[
                canonical_identity.as_bytes(),
                source.artifact_kind().as_str().as_bytes(),
                source
                    .artifact_id()
                    .map_or(b"".as_slice(), |id| id.as_str().as_bytes()),
                source
                    .version_id()
                    .map_or(b"".as_slice(), |id| id.as_str().as_bytes()),
            ],
        ))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentRating {
    General,
    Adult,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccount {
    provider: KnownProvider,
    state: ProviderAccountState,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    premium: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    supporter: Option<bool>,
}

impl ProviderAccount {
    pub(crate) fn new(
        provider: KnownProvider,
        state: ProviderAccountState,
        display_name: Option<String>,
        account_id: Option<String>,
        premium: Option<bool>,
        supporter: Option<bool>,
    ) -> ProviderResult<Self> {
        let display_name = optional_stable_text(display_name.as_deref(), 128);
        let account_id = account_id
            .as_deref()
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && !value.chars().any(char::is_control)
                    && stable_text_allowed(value)
            })
            .map(str::to_owned);
        if state == ProviderAccountState::SignedIn && display_name.is_none() && account_id.is_none()
        {
            return Err(ProviderFailure::invalid_payload(provider));
        }
        Ok(Self {
            provider,
            state,
            display_name,
            account_id,
            premium,
            supporter,
        })
    }

    #[must_use]
    pub fn not_required(provider: KnownProvider) -> Self {
        Self {
            provider,
            state: ProviderAccountState::NotRequired,
            display_name: None,
            account_id: None,
            premium: None,
            supporter: None,
        }
    }

    #[must_use]
    pub const fn provider(&self) -> KnownProvider {
        self.provider
    }

    #[must_use]
    pub const fn state(&self) -> ProviderAccountState {
        self.state
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    #[must_use]
    pub fn account_id(&self) -> Option<&str> {
        self.account_id.as_deref()
    }

    #[must_use]
    pub const fn premium(&self) -> Option<bool> {
        self.premium
    }

    #[must_use]
    pub const fn supporter(&self) -> Option<bool> {
        self.supporter
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderItem {
    source: ProviderRef,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    images: Vec<String>,
    content_rating: ContentRating,
    #[serde(skip_serializing_if = "Option::is_none")]
    downloads: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    endorsements: Option<u64>,
    featured: bool,
}

pub(crate) struct ProviderItemInput {
    pub source: ProviderRef,
    pub title: String,
    pub summary: Option<String>,
    pub author: Option<String>,
    pub updated_at: Option<String>,
    pub content_rating: ContentRating,
    pub downloads: Option<u64>,
    pub endorsements: Option<u64>,
    pub featured: bool,
}

impl ProviderItem {
    pub(crate) fn from_input(
        provider: KnownProvider,
        input: ProviderItemInput,
    ) -> ProviderResult<Self> {
        validate_source(provider, &input.source)?;
        let title = required_stable_text(&input.title, 512)
            .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
        Ok(Self {
            source: input.source,
            title,
            summary: optional_stable_text(input.summary.as_deref(), 4096),
            author: optional_stable_text(input.author.as_deref(), 256),
            updated_at: optional_stable_text(input.updated_at.as_deref(), 128),
            // External image URLs are intentionally omitted until a trusted proxy/cache owns
            // their transport. Stable provider data must never retain signed URL material.
            images: Vec::new(),
            content_rating: input.content_rating,
            downloads: input.downloads,
            endorsements: input.endorsements,
            featured: input.featured,
        })
    }

    #[must_use]
    pub fn source(&self) -> &ProviderRef {
        &self.source
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    #[must_use]
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    #[must_use]
    pub fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }

    #[must_use]
    pub fn images(&self) -> &[String] {
        &self.images
    }

    #[must_use]
    pub const fn content_rating(&self) -> ContentRating {
        self.content_rating
    }

    #[must_use]
    pub const fn downloads(&self) -> Option<u64> {
        self.downloads
    }

    #[must_use]
    pub const fn endorsements(&self) -> Option<u64> {
        self.endorsements
    }

    #[must_use]
    pub const fn featured(&self) -> bool {
        self.featured
    }

    #[must_use]
    pub fn cache_identity(&self) -> CacheIdentity {
        CacheIdentity::for_item(&self.source)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderVersion {
    source: ProviderRef,
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    primary: bool,
    directly_downloadable: bool,
}

pub(crate) struct ProviderVersionInput {
    pub source: ProviderRef,
    pub label: String,
    pub file_name: Option<String>,
    pub size_bytes: Option<u64>,
    pub published_at: Option<String>,
    pub sha256: Option<String>,
    pub primary: bool,
    pub directly_downloadable: bool,
}

impl ProviderVersion {
    pub(crate) fn from_input(
        provider: KnownProvider,
        input: ProviderVersionInput,
    ) -> ProviderResult<Self> {
        validate_source(provider, &input.source)?;
        if input.source.artifact_id().is_none() {
            return Err(ProviderFailure::invalid_payload(provider));
        }
        let label = normalize_version_label(&input.label)
            .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
        let file_name = match input.file_name.as_deref() {
            Some(value) => Some(
                safe_file_name(value, 255)
                    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?,
            ),
            None => None,
        };
        let sha256 = match input.sha256.as_deref() {
            Some(value) => Some(
                normalize_sha256(value)
                    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?,
            ),
            None => None,
        };
        Ok(Self {
            source: input.source,
            label,
            file_name,
            size_bytes: input.size_bytes,
            published_at: optional_stable_text(input.published_at.as_deref(), 128),
            sha256,
            primary: input.primary,
            directly_downloadable: input.directly_downloadable,
        })
    }

    #[must_use]
    pub fn source(&self) -> &ProviderRef {
        &self.source
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    #[must_use]
    pub const fn size_bytes(&self) -> Option<u64> {
        self.size_bytes
    }

    #[must_use]
    pub fn published_at(&self) -> Option<&str> {
        self.published_at.as_deref()
    }

    #[must_use]
    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }

    #[must_use]
    pub const fn primary(&self) -> bool {
        self.primary
    }

    #[must_use]
    pub const fn directly_downloadable(&self) -> bool {
        self.directly_downloadable
    }

    #[must_use]
    pub fn cache_identity(&self) -> CacheIdentity {
        CacheIdentity::for_version(&self.source)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPage {
    provider: KnownProvider,
    scope_digest: CacheIdentity,
    items: Vec<ProviderItem>,
    has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_count: Option<u64>,
    duplicate_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    attribution: Option<String>,
    cache_identity: CacheIdentity,
}

impl ProviderPage {
    pub(crate) fn new(
        provider: KnownProvider,
        request: &SearchRequest,
        items: Vec<ProviderItem>,
        has_more: bool,
        total_count: Option<u64>,
        attribution: Option<String>,
    ) -> ProviderResult<Self> {
        request.validate_for(provider)?;
        if items.len() > MAX_RESULT_ITEMS {
            return Err(ProviderFailure::invalid_payload(provider));
        }
        let mut identities = BTreeSet::new();
        let mut normalized = Vec::with_capacity(items.len());
        let mut duplicate_count = 0;
        for item in items {
            validate_source(provider, item.source())?;
            if identities.insert(item.source().canonical_identity()) {
                normalized.push(item);
            } else {
                duplicate_count += 1;
            }
        }
        Ok(Self {
            provider,
            scope_digest: request.scope_digest(provider),
            items: normalized,
            has_more,
            total_count,
            duplicate_count,
            attribution: optional_stable_text(attribution.as_deref(), 512),
            cache_identity: request.cache_identity(provider),
        })
    }

    #[must_use]
    pub const fn provider(&self) -> KnownProvider {
        self.provider
    }

    #[must_use]
    pub fn scope_digest(&self) -> &CacheIdentity {
        &self.scope_digest
    }

    #[must_use]
    pub fn items(&self) -> &[ProviderItem] {
        &self.items
    }

    #[must_use]
    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    #[must_use]
    pub const fn total_count(&self) -> Option<u64> {
        self.total_count
    }

    #[must_use]
    pub const fn duplicate_count(&self) -> usize {
        self.duplicate_count
    }

    #[must_use]
    pub fn attribution(&self) -> Option<&str> {
        self.attribution.as_deref()
    }

    #[must_use]
    pub fn cache_identity(&self) -> &CacheIdentity {
        &self.cache_identity
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetails {
    item: ProviderItem,
    versions: Vec<ProviderVersion>,
    duplicate_version_count: usize,
}

impl ProviderDetails {
    pub(crate) fn new(
        provider: KnownProvider,
        item: ProviderItem,
        versions: Vec<ProviderVersion>,
    ) -> ProviderResult<Self> {
        validate_source(provider, item.source())?;
        if versions.len() > MAX_VERSION_ENTRIES {
            return Err(ProviderFailure::invalid_payload(provider));
        }
        let identity = item.source().canonical_identity();
        let mut version_keys = BTreeSet::new();
        let mut normalized = Vec::with_capacity(versions.len());
        let mut duplicate_version_count = 0;
        for version in versions {
            if version.source().canonical_identity() != identity {
                return Err(ProviderFailure::invalid_payload(provider));
            }
            let key = version.cache_identity();
            if version_keys.insert(key) {
                normalized.push(version);
            } else {
                duplicate_version_count += 1;
            }
        }
        Ok(Self {
            item,
            versions: normalized,
            duplicate_version_count,
        })
    }

    #[must_use]
    pub fn item(&self) -> &ProviderItem {
        &self.item
    }

    #[must_use]
    pub fn versions(&self) -> &[ProviderVersion] {
        &self.versions
    }

    #[must_use]
    pub const fn duplicate_version_count(&self) -> usize {
        self.duplicate_version_count
    }
}

/// A direct download resolution. This type is intentionally not serializable, and its custom
/// debug representation delegates to the query-redacting URL wrapper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadResolution {
    source: ProviderRef,
    url: EphemeralDownloadUrl,
    file_name: String,
    size_bytes: Option<u64>,
    sha256: Option<String>,
}

impl DownloadResolution {
    pub(crate) fn new(
        provider: KnownProvider,
        source: ProviderRef,
        url: EphemeralDownloadUrl,
        file_name: impl Into<String>,
        size_bytes: Option<u64>,
        sha256: Option<String>,
    ) -> ProviderResult<Self> {
        validate_source(provider, &source)?;
        let file_name = safe_file_name(&file_name.into(), 255)
            .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
        let sha256 = match sha256.as_deref() {
            Some(value) => Some(
                normalize_sha256(value)
                    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?,
            ),
            None => None,
        };
        Ok(Self {
            source,
            url,
            file_name,
            size_bytes,
            sha256,
        })
    }

    #[must_use]
    pub fn source(&self) -> &ProviderRef {
        &self.source
    }

    #[must_use]
    pub fn url(&self) -> &EphemeralDownloadUrl {
        &self.url
    }

    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    #[must_use]
    pub const fn size_bytes(&self) -> Option<u64> {
        self.size_bytes
    }

    #[must_use]
    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSourceRecord {
    source: ProviderRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

impl ProviderSourceRecord {
    pub(crate) fn new(
        provider: KnownProvider,
        source: ProviderRef,
        archive_sha256: Option<String>,
        version: Option<String>,
    ) -> ProviderResult<Self> {
        validate_source(provider, &source)?;
        let archive_sha256 = match archive_sha256.as_deref() {
            Some(value) => Some(
                normalize_sha256(value)
                    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?,
            ),
            None => None,
        };
        let version = match version.as_deref() {
            Some(value) => Some(
                normalize_version_label(value)
                    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?,
            ),
            None => None,
        };
        Ok(Self {
            source,
            archive_sha256,
            version,
        })
    }

    #[must_use]
    pub fn source(&self) -> &ProviderRef {
        &self.source
    }

    #[must_use]
    pub fn archive_sha256(&self) -> Option<&str> {
        self.archive_sha256.as_deref()
    }

    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

pub(crate) fn decode_json<T: DeserializeOwned>(
    provider: KnownProvider,
    bytes: &[u8],
) -> ProviderResult<T> {
    decode_json_with_array_limits(provider, bytes, &[])
}

#[derive(Clone, Copy)]
pub(crate) struct JsonArrayLimit {
    path: &'static [&'static str],
    maximum: usize,
}

impl JsonArrayLimit {
    pub(crate) const fn new(path: &'static [&'static str], maximum: usize) -> Self {
        Self { path, maximum }
    }
}

pub(crate) fn decode_json_with_array_limits<T: DeserializeOwned>(
    provider: KnownProvider,
    bytes: &[u8],
    array_limits: &[JsonArrayLimit],
) -> ProviderResult<T> {
    if bytes.is_empty() || bytes.len() > MAX_FIXTURE_BYTES {
        return Err(ProviderFailure::invalid_payload(provider));
    }
    let mut duplicate_checker = serde_json::Deserializer::from_slice(bytes);
    let mut budget = JsonBudget::default();
    RejectDuplicateMembers {
        budget: &mut budget,
        depth: 0,
        path: Vec::new(),
        array_limits,
    }
    .deserialize(&mut duplicate_checker)
    .and_then(|_| duplicate_checker.end())
    .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    serde_json::from_slice(bytes).map_err(|_| ProviderFailure::invalid_payload(provider))
}

#[derive(Default)]
struct JsonBudget {
    nodes: usize,
}

struct RejectDuplicateMembers<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
    path: Vec<String>,
    array_limits: &'a [JsonArrayLimit],
}

impl<'de> DeserializeSeed<'de> for RejectDuplicateMembers<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.depth > MAX_JSON_DEPTH {
            return Err(serde::de::Error::custom("JSON nesting limit exceeded"));
        }
        self.budget.nodes = self.budget.nodes.saturating_add(1);
        if self.budget.nodes > MAX_JSON_NODES {
            return Err(serde::de::Error::custom("JSON node limit exceeded"));
        }
        deserializer.deserialize_any(RejectDuplicateMembersVisitor {
            budget: self.budget,
            depth: self.depth,
            path: self.path,
            array_limits: self.array_limits,
        })
    }
}

struct RejectDuplicateMembersVisitor<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
    path: Vec<String>,
    array_limits: &'a [JsonArrayLimit],
}

impl<'de> Visitor<'de> for RejectDuplicateMembersVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object member names")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        RejectDuplicateMembers {
            budget: self.budget,
            depth: self.depth + 1,
            path: self.path,
            array_limits: self.array_limits,
        }
        .deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let maximum = self
            .array_limits
            .iter()
            .filter(|limit| path_matches(&self.path, limit.path))
            .map(|limit| limit.maximum)
            .min()
            .unwrap_or(MAX_JSON_SEQUENCE_ITEMS)
            .min(MAX_JSON_SEQUENCE_ITEMS);
        for _ in 0..maximum {
            if sequence
                .next_element_seed(RejectDuplicateMembers {
                    budget: self.budget,
                    depth: self.depth + 1,
                    path: self.path.clone(),
                    array_limits: self.array_limits,
                })?
                .is_none()
            {
                return Ok(());
            }
        }
        if sequence.next_element_seed(RejectExcessElement)?.is_some() {
            unreachable!("the rejecting seed cannot produce an element")
        }
        Ok(())
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut names = BTreeSet::new();
        let mut count = 0_usize;
        while let Some(name) = object.next_key::<String>()? {
            count += 1;
            if count > MAX_JSON_OBJECT_MEMBERS || name.len() > MAX_JSON_KEY_BYTES {
                return Err(serde::de::Error::custom("JSON object limit exceeded"));
            }
            if !names.insert(name.to_lowercase()) {
                return Err(serde::de::Error::custom("duplicate JSON object member"));
            }
            let mut child_path = self.path.clone();
            child_path.push(name);
            object.next_value_seed(RejectDuplicateMembers {
                budget: self.budget,
                depth: self.depth + 1,
                path: child_path,
                array_limits: self.array_limits,
            })?;
        }
        Ok(())
    }
}

fn path_matches(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

struct RejectExcessElement;

impl<'de> DeserializeSeed<'de> for RejectExcessElement {
    type Value = ();

    fn deserialize<D>(self, _deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Err(serde::de::Error::custom("JSON sequence limit exceeded"))
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn provider_ref(
    provider: KnownProvider,
    item_kind: ProviderItemKind,
    resource_id: &str,
    scope: Option<&str>,
    artifact_id: Option<&str>,
    artifact_kind: ProviderArtifactKind,
    version_id: Option<&str>,
    canonical_url: Option<String>,
) -> ProviderResult<ProviderRef> {
    if [Some(resource_id), scope, artifact_id, version_id]
        .into_iter()
        .flatten()
        .any(credential_shaped_identifier)
        || canonical_url.as_deref().is_some_and(credential_shaped_text)
    {
        return Err(ProviderFailure::invalid_payload(provider));
    }

    let resource_id = ProviderResourceId::parse(resource_id)
        .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    let scope = scope
        .map(ProviderScope::parse)
        .transpose()
        .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    let artifact_id = artifact_id
        .map(ProviderResourceId::parse)
        .transpose()
        .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    let version_id = version_id
        .map(ProviderResourceId::parse)
        .transpose()
        .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    ProviderRef::new(
        provider.provider_id(),
        item_kind,
        resource_id,
        scope,
        artifact_id,
        artifact_kind,
        version_id,
        canonical_url,
    )
    .map_err(|_| ProviderFailure::invalid_payload(provider))
}

pub(crate) fn validate_source(provider: KnownProvider, source: &ProviderRef) -> ProviderResult<()> {
    if source.provider_id().as_str() != provider.as_str() || source.validate().is_err() {
        return Err(ProviderFailure::invalid_payload(provider));
    }
    match provider {
        KnownProvider::Nexus => {
            if source.item_kind() != ProviderItemKind::Mod
                || source.scope().is_none()
                || source.canonical_url().is_none()
            {
                return Err(ProviderFailure::invalid_payload(provider));
            }
        }
        KnownProvider::Local => {
            let hash = source.resource_id().as_str();
            if source.item_kind() != ProviderItemKind::LocalArchive
                || normalize_sha256(hash).as_deref() != Some(hash)
                || source.artifact_id().map(ProviderResourceId::as_str) != Some(hash)
                || source.artifact_kind() != ProviderArtifactKind::Archive
                || source.canonical_url().is_some()
            {
                return Err(ProviderFailure::invalid_payload(provider));
            }
        }
        KnownProvider::GameBanana | KnownProvider::ModDb => {
            if source.item_kind() != ProviderItemKind::Mod || source.canonical_url().is_none() {
                return Err(ProviderFailure::invalid_payload(provider));
            }
        }
        KnownProvider::GameJolt | KnownProvider::Itch => {
            if !matches!(
                source.item_kind(),
                ProviderItemKind::Game | ProviderItemKind::Mod
            ) || source.canonical_url().is_none()
            {
                return Err(ProviderFailure::invalid_payload(provider));
            }
        }
    }
    Ok(())
}

pub(crate) fn positive_numeric_id(value: Option<&Value>) -> Option<String> {
    let number = match value {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.trim().parse::<u64>().ok(),
        _ => None,
    }?;
    (number > 0).then(|| number.to_string())
}

pub(crate) fn positive_numeric_alias(
    value: &Value,
    keys: &[&str],
    provider: KnownProvider,
) -> ProviderResult<Option<String>> {
    let mut identity = None;
    for key in keys {
        if let Some(raw) = value.get(*key) {
            let candidate = positive_numeric_id(Some(raw))
                .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
            if identity
                .as_deref()
                .is_some_and(|existing| existing != candidate)
            {
                return Err(ProviderFailure::invalid_payload(provider));
            }
            identity = Some(candidate);
        }
    }
    Ok(identity)
}

pub(crate) fn normalized_version_alias(
    value: &Value,
    keys: &[&str],
    provider: KnownProvider,
) -> ProviderResult<Option<String>> {
    let mut version = None;
    for key in keys {
        if let Some(raw) = value.get(*key) {
            let candidate = raw
                .as_str()
                .and_then(normalize_version_label)
                .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
            if version
                .as_deref()
                .is_some_and(|existing| existing != candidate)
            {
                return Err(ProviderFailure::invalid_payload(provider));
            }
            version = Some(candidate);
        }
    }
    Ok(version)
}

pub(crate) fn string_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn normalize_sha256(value: &str) -> Option<String> {
    let value = value.trim();
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

pub(crate) fn safe_file_name(value: &str, maximum: usize) -> Option<String> {
    safe_stable_basename(value, maximum)
}

pub(crate) fn optional_stable_text(value: Option<&str>, maximum: usize) -> Option<String> {
    value.and_then(|value| required_stable_text(value, maximum))
}

pub(crate) fn required_stable_text(value: &str, maximum: usize) -> Option<String> {
    required_plain_text(value, maximum).filter(|value| stable_text_allowed(value))
}

pub(crate) fn required_plain_text(value: &str, maximum: usize) -> Option<String> {
    if value.len() > maximum || value.chars().any(char::is_control) {
        return None;
    }
    let stripped = strip_markup(value);
    let normalized = normalize_whitespace(&stripped);
    (!normalized.is_empty() && normalized.len() <= maximum).then_some(normalized)
}

pub(crate) fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_markup(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' if inside_tag => {
                inside_tag = false;
                output.push(' ');
            }
            _ if !inside_tag => output.push(character),
            _ => {}
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

pub(crate) fn version_contract_id(version: Option<&str>, fallback: &str) -> String {
    version
        .and_then(normalize_version_label)
        .as_deref()
        .filter(|value| ProviderResourceId::parse(value).is_ok())
        .unwrap_or(fallback)
        .to_owned()
}
