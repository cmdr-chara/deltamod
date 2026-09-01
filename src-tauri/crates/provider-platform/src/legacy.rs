//! Mapping for source metadata already persisted/emitted by the Node and native compatibility
//! providers. The result always contains a provider reference; an un-attributed archive becomes
//! a stable `local` source keyed by its separately computed SHA-256.

use crate::{
    local, moddb,
    model::{
        decode_json, normalize_sha256, positive_numeric_alias, provider_ref, version_contract_id,
        ProviderSourceRecord,
    },
    text_policy::{credential_shaped_scope, normalize_version_label},
    url_policy::{canonical_public_url, canonical_url},
    KnownProvider, ProviderFailure, ProviderResult,
};
use deltamod_product_contracts::{
    ProviderArtifactKind, ProviderItemKind, ProviderResourceId, ProviderScope,
};
use serde_json::Value;
use std::collections::BTreeSet;

const GAMEBANANA_ROOTS: &[&str] = &["gamebanana.com"];
const NEXUS_PUBLIC_ROOTS: &[&str] = &["nexusmods.com"];

pub fn map_legacy_source(
    payload: &[u8],
    computed_archive_sha256: Option<&str>,
) -> ProviderResult<ProviderSourceRecord> {
    let computed_archive_sha256 = match computed_archive_sha256 {
        Some(value) => Some(
            normalize_sha256(value)
                .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?,
        ),
        None => None,
    };
    let value: Value = decode_json(KnownProvider::Local, payload)?;
    let archive_sha256 = computed_archive_sha256.or(legacy_archive_hash(&value)?);
    let version = legacy_version(&value)?;
    let candidates = identity_candidates(&value)?;
    let Some(selected) = candidates
        .iter()
        .max_by_key(|candidate| candidate.priority())
    else {
        return local_fallback(archive_sha256.as_deref(), version.as_deref());
    };
    match selected.kind {
        LegacyCandidateKind::Generic => {
            map_generic_source(selected.source, archive_sha256, version)
        }
        LegacyCandidateKind::GameBanana => {
            map_gamebanana_legacy(selected.source, archive_sha256, version)
        }
    }
}

fn map_generic_source(
    source: &Value,
    archive_sha256: Option<String>,
    version: Option<String>,
) -> ProviderResult<ProviderSourceRecord> {
    let provider_name = source
        .get("provider")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
    let provider = KnownProvider::from_id(provider_name)
        .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
    match provider {
        KnownProvider::Local => local_fallback(archive_sha256.as_deref(), version.as_deref()),
        KnownProvider::GameBanana => map_gamebanana_legacy(source, archive_sha256, version),
        KnownProvider::Nexus => map_nexus_legacy(source, archive_sha256, version),
        KnownProvider::ModDb => map_moddb_legacy(source, archive_sha256, version),
        KnownProvider::GameJolt | KnownProvider::Itch => {
            Err(ProviderFailure::invalid_payload(provider))
        }
    }
}

fn map_gamebanana_legacy(
    source: &Value,
    archive_sha256: Option<String>,
    version: Option<String>,
) -> ProviderResult<ProviderSourceRecord> {
    let provider = KnownProvider::GameBanana;
    let explicit_id = numeric_identity_alias(source, &["id", "gamebanana_id"], provider)?;
    let explicit_scope = scope_identity_alias(
        source,
        &["scope", "model", "gamebanana_model"],
        provider,
        normalize_gamebanana_model,
    )?;
    let from_url = optional_string_value(source, "url", provider)?
        .map(|url| {
            gamebanana_identity_from_url(url)
                .ok_or_else(|| ProviderFailure::invalid_payload(provider))
        })
        .transpose()?;
    let id = merge_marker_value(
        provider,
        explicit_id,
        from_url.as_ref().map(|(_, id)| id.clone()),
    )?
    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
    let scope = merge_marker_value(provider, explicit_scope, from_url.map(|(scope, _)| scope))?
        .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
    let plural = if scope == "mod" { "mods" } else { "wips" };
    let canonical_url = canonical_public_url(
        &format!("https://gamebanana.com/{plural}/{id}"),
        GAMEBANANA_ROOTS,
    )
    .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    let artifact_id =
        numeric_identity_alias(source, &["fileId", "file_id", "source_file_id"], provider)?;
    let version_id = artifact_id
        .as_deref()
        .map(|fallback| version_contract_id(version.as_deref(), fallback));
    let reference = provider_ref(
        provider,
        ProviderItemKind::Mod,
        &id,
        Some(&scope),
        artifact_id.as_deref(),
        if artifact_id.is_some() {
            ProviderArtifactKind::File
        } else {
            ProviderArtifactKind::Unknown
        },
        version_id.as_deref(),
        Some(canonical_url),
    )?;
    ProviderSourceRecord::new(provider, reference, archive_sha256, version)
}

fn map_nexus_legacy(
    source: &Value,
    archive_sha256: Option<String>,
    version: Option<String>,
) -> ProviderResult<ProviderSourceRecord> {
    let provider = KnownProvider::Nexus;
    let explicit_id = numeric_identity_alias(source, &["id", "modId", "mod_id"], provider)?;
    let explicit_scope = scope_identity_alias(
        source,
        &["scope", "domain"],
        provider,
        normalized_nexus_scope,
    )?;
    let from_url = optional_string_value(source, "url", provider)?
        .map(|url| {
            nexus_identity_from_url(url).ok_or_else(|| ProviderFailure::invalid_payload(provider))
        })
        .transpose()?;
    let id = merge_marker_value(
        provider,
        explicit_id,
        from_url.as_ref().map(|(_, id)| id.clone()),
    )?
    .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
    let domain = merge_marker_value(provider, explicit_scope, from_url.map(|(scope, _)| scope))?
        .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
    let canonical_url = canonical_public_url(
        &format!("https://www.nexusmods.com/{domain}/mods/{id}"),
        NEXUS_PUBLIC_ROOTS,
    )
    .map_err(|_| ProviderFailure::invalid_payload(provider))?;
    let artifact_id = numeric_identity_alias(source, &["fileId", "file_id"], provider)?;
    let version_id = artifact_id
        .as_deref()
        .map(|fallback| version_contract_id(version.as_deref(), fallback));
    let reference = provider_ref(
        provider,
        ProviderItemKind::Mod,
        &id,
        Some(&domain),
        artifact_id.as_deref(),
        if artifact_id.is_some() {
            ProviderArtifactKind::File
        } else {
            ProviderArtifactKind::Unknown
        },
        version_id.as_deref(),
        Some(canonical_url),
    )?;
    ProviderSourceRecord::new(provider, reference, archive_sha256, version)
}

fn map_moddb_legacy(
    source: &Value,
    archive_sha256: Option<String>,
    version: Option<String>,
) -> ProviderResult<ProviderSourceRecord> {
    let provider = KnownProvider::ModDb;
    let route = validated_moddb_legacy_route(source)?;
    let artifact_id = resource_identity_alias(source, &["fileId", "file_id"], provider)?;
    let version_id = artifact_id
        .as_deref()
        .map(|fallback| version_contract_id(version.as_deref(), fallback));
    let reference = provider_ref(
        provider,
        ProviderItemKind::Mod,
        &route.resource_id,
        Some(&route.scope),
        artifact_id.as_deref(),
        if artifact_id.is_some() {
            ProviderArtifactKind::File
        } else {
            ProviderArtifactKind::Unknown
        },
        version_id.as_deref(),
        Some(route.canonical_url),
    )?;
    ProviderSourceRecord::new(provider, reference, archive_sha256, version)
}

#[derive(Clone, Eq, PartialEq)]
struct LegacyIdentityMarker {
    provider: KnownProvider,
    resource_id: Option<String>,
    scope: Option<String>,
    artifact_id: Option<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LegacyCandidateKind {
    Generic,
    GameBanana,
}

struct LegacyIdentityCandidate<'a> {
    source: &'a Value,
    marker: LegacyIdentityMarker,
    kind: LegacyCandidateKind,
}

impl LegacyIdentityCandidate<'_> {
    fn priority(&self) -> u8 {
        let kind = match self.kind {
            LegacyCandidateKind::Generic => 10,
            LegacyCandidateKind::GameBanana => 0,
        };
        kind + u8::from(self.marker.artifact_id.is_some()) * 100
            + u8::from(self.source.get("url").is_some())
    }
}

const MAX_LEGACY_SCAN_DEPTH: usize = 16;
const MAX_LEGACY_SCAN_NODES: usize = 4_096;

fn identity_candidates(value: &Value) -> ProviderResult<Vec<LegacyIdentityCandidate<'_>>> {
    let mut candidates = Vec::new();
    let mut nodes = 0_usize;
    scan_identity_candidates(value, 0, &mut nodes, false, &mut candidates)?;
    validate_identity_candidates(&candidates)?;
    Ok(candidates)
}

fn validate_identity_candidates(candidates: &[LegacyIdentityCandidate<'_>]) -> ProviderResult<()> {
    for candidate in candidates {
        let complete = match candidate.marker.provider {
            KnownProvider::GameBanana => {
                candidate.marker.resource_id.is_some() && candidate.marker.scope.is_some()
            }
            KnownProvider::Nexus | KnownProvider::ModDb => {
                candidate.marker.resource_id.is_some()
                    && candidate.marker.scope.is_some()
                    && candidate
                        .source
                        .get("url")
                        .and_then(Value::as_str)
                        .is_some()
            }
            KnownProvider::Local => {
                candidate.marker.resource_id.is_none()
                    && candidate.marker.scope.is_none()
                    && candidate.marker.artifact_id.is_none()
            }
            KnownProvider::GameJolt | KnownProvider::Itch => false,
        };
        if !complete {
            return Err(ProviderFailure::invalid_payload(candidate.marker.provider));
        }
    }
    let providers = candidates
        .iter()
        .map(|candidate| candidate.marker.provider)
        .collect::<BTreeSet<_>>();
    let provider = candidates
        .first()
        .map_or(KnownProvider::Local, |candidate| candidate.marker.provider);
    if providers.len() > 1 {
        return Err(ProviderFailure::invalid_payload(provider));
    }
    let resource_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.marker.resource_id.as_deref())
        .collect::<BTreeSet<_>>();
    let scopes = candidates
        .iter()
        .filter_map(|candidate| candidate.marker.scope.as_deref())
        .collect::<BTreeSet<_>>();
    let artifact_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.marker.artifact_id.as_deref())
        .collect::<BTreeSet<_>>();
    if resource_ids.len() > 1 || scopes.len() > 1 || artifact_ids.len() > 1 {
        Err(ProviderFailure::invalid_payload(provider))
    } else {
        Ok(())
    }
}

fn scan_identity_candidates<'a>(
    value: &'a Value,
    depth: usize,
    nodes: &mut usize,
    gamebanana_context: bool,
    candidates: &mut Vec<LegacyIdentityCandidate<'a>>,
) -> ProviderResult<()> {
    *nodes = nodes.saturating_add(1);
    if depth > MAX_LEGACY_SCAN_DEPTH || *nodes > MAX_LEGACY_SCAN_NODES {
        return Err(ProviderFailure::invalid_payload(KnownProvider::Local));
    }
    let Value::Object(object) = value else {
        if let Value::Array(values) = value {
            for child in values {
                scan_identity_candidates(child, depth + 1, nodes, false, candidates)?;
            }
        }
        return Ok(());
    };

    for key in object.keys() {
        if identity_key_case_variant(key) {
            return Err(ProviderFailure::invalid_payload(KnownProvider::Local));
        }
    }

    let suppress_immediate_gamebanana =
        gamebanana_context && object.get("supports").and_then(Value::as_bool) == Some(false);
    let mut context = suppress_immediate_gamebanana.then_some(KnownProvider::GameBanana);
    if object.contains_key("provider") {
        let declared_provider = object
            .get("provider")
            .and_then(Value::as_str)
            .and_then(KnownProvider::from_id)
            .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
        if suppress_immediate_gamebanana && declared_provider == KnownProvider::GameBanana {
            context = Some(KnownProvider::GameBanana);
        } else {
            let marker = generic_identity_marker(value)?;
            context = Some(marker.provider);
            candidates.push(LegacyIdentityCandidate {
                source: value,
                marker,
                kind: LegacyCandidateKind::Generic,
            });
        }
    }

    if gamebanana_context && !suppress_immediate_gamebanana {
        if !declares_gamebanana_identity(object) {
            return Err(ProviderFailure::invalid_payload(KnownProvider::GameBanana));
        }
        let marker = gamebanana_identity_marker(value)?;
        context = Some(KnownProvider::GameBanana);
        candidates.push(LegacyIdentityCandidate {
            source: value,
            marker,
            kind: LegacyCandidateKind::GameBanana,
        });
    } else if context.is_none()
        && (object.contains_key("gamebanana_id") || object.contains_key("gamebanana_model"))
    {
        let marker = gamebanana_identity_marker(value)?;
        context = Some(KnownProvider::GameBanana);
        candidates.push(LegacyIdentityCandidate {
            source: value,
            marker,
            kind: LegacyCandidateKind::GameBanana,
        });
    }

    for key in object.keys().filter(|key| is_identity_key(key)) {
        if !context.is_some_and(|provider| provider_allows_identity_key(provider, key)) {
            return Err(ProviderFailure::invalid_payload(
                context.unwrap_or(KnownProvider::Local),
            ));
        }
    }

    for (key, child) in object {
        if key == "gamebanana" {
            if !child.is_object() {
                return Err(ProviderFailure::invalid_payload(KnownProvider::GameBanana));
            }
            scan_identity_candidates(child, depth + 1, nodes, true, candidates)?;
        } else {
            if key == "source" && (!child.is_object() || child.get("provider").is_none()) {
                return Err(ProviderFailure::invalid_payload(KnownProvider::Local));
            }
            if child.is_object() || child.is_array() {
                scan_identity_candidates(child, depth + 1, nodes, false, candidates)?;
            }
        }
    }
    Ok(())
}

fn declares_gamebanana_identity(object: &serde_json::Map<String, Value>) -> bool {
    [
        "id",
        "gamebanana_id",
        "model",
        "gamebanana_model",
        "scope",
        "url",
        "fileId",
        "file_id",
        "source_file_id",
    ]
    .iter()
    .any(|key| object.contains_key(*key))
}

fn identity_key_case_variant(key: &str) -> bool {
    const KEYS: &[&str] = &[
        "provider",
        "id",
        "modId",
        "mod_id",
        "fileId",
        "file_id",
        "source_file_id",
        "scope",
        "domain",
        "url",
        "model",
        "gamebanana_id",
        "gamebanana_model",
        "source",
        "gamebanana",
    ];
    KEYS.iter()
        .any(|canonical| key.eq_ignore_ascii_case(canonical) && key != *canonical)
}

fn is_identity_key(key: &str) -> bool {
    matches!(
        key,
        "provider"
            | "id"
            | "modId"
            | "mod_id"
            | "fileId"
            | "file_id"
            | "source_file_id"
            | "scope"
            | "domain"
            | "url"
            | "model"
            | "gamebanana_id"
            | "gamebanana_model"
    )
}

fn provider_allows_identity_key(provider: KnownProvider, key: &str) -> bool {
    match provider {
        KnownProvider::GameBanana => matches!(
            key,
            "provider"
                | "id"
                | "fileId"
                | "file_id"
                | "source_file_id"
                | "scope"
                | "url"
                | "model"
                | "gamebanana_id"
                | "gamebanana_model"
        ),
        KnownProvider::Nexus => matches!(
            key,
            "provider"
                | "id"
                | "modId"
                | "mod_id"
                | "fileId"
                | "file_id"
                | "scope"
                | "domain"
                | "url"
        ),
        KnownProvider::ModDb => {
            matches!(
                key,
                "provider" | "id" | "fileId" | "file_id" | "scope" | "url"
            )
        }
        KnownProvider::Local => key == "provider",
        KnownProvider::GameJolt | KnownProvider::Itch => false,
    }
}

fn generic_identity_marker(source: &Value) -> ProviderResult<LegacyIdentityMarker> {
    let provider_name = source
        .get("provider")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
    let provider = KnownProvider::from_id(provider_name)
        .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
    let raw_url = optional_string_value(source, "url", provider)?;
    let (resource_id, scope, artifact_id) = match provider {
        KnownProvider::GameBanana => {
            let explicit_id = numeric_identity_alias(source, &["id", "gamebanana_id"], provider)?;
            let explicit_scope = scope_identity_alias(
                source,
                &["scope", "model", "gamebanana_model"],
                provider,
                normalize_gamebanana_model,
            )?;
            let from_url = raw_url
                .map(|url| {
                    gamebanana_identity_from_url(url)
                        .ok_or_else(|| ProviderFailure::invalid_payload(provider))
                })
                .transpose()?;
            (
                merge_marker_value(
                    provider,
                    explicit_id,
                    from_url.as_ref().map(|(_, id)| id.clone()),
                )?,
                merge_marker_value(provider, explicit_scope, from_url.map(|(scope, _)| scope))?,
                numeric_identity_alias(source, &["fileId", "file_id", "source_file_id"], provider)?,
            )
        }
        KnownProvider::Nexus => {
            let explicit_id = numeric_identity_alias(source, &["id", "modId", "mod_id"], provider)?;
            let explicit_scope = scope_identity_alias(
                source,
                &["scope", "domain"],
                provider,
                normalized_nexus_scope,
            )?;
            let from_url = raw_url
                .map(|url| {
                    nexus_identity_from_url(url)
                        .ok_or_else(|| ProviderFailure::invalid_payload(provider))
                })
                .transpose()?;
            (
                merge_marker_value(
                    provider,
                    explicit_id,
                    from_url.as_ref().map(|(_, id)| id.clone()),
                )?,
                merge_marker_value(provider, explicit_scope, from_url.map(|(scope, _)| scope))?,
                numeric_identity_alias(source, &["fileId", "file_id"], provider)?,
            )
        }
        KnownProvider::ModDb => {
            let artifact_id = resource_identity_alias(source, &["fileId", "file_id"], provider)?;
            if raw_url.is_some() {
                let route = validated_moddb_legacy_route(source)?;
                (Some(route.resource_id), Some(route.scope), artifact_id)
            } else {
                (
                    resource_identity_alias(source, &["id"], provider)?,
                    scope_identity_alias(source, &["scope"], provider, normalized_provider_scope)?,
                    artifact_id,
                )
            }
        }
        KnownProvider::Local => (None, None, None),
        KnownProvider::GameJolt | KnownProvider::Itch => {
            return Err(ProviderFailure::invalid_payload(provider));
        }
    };
    Ok(LegacyIdentityMarker {
        provider,
        resource_id,
        scope,
        artifact_id,
    })
}

fn validated_moddb_legacy_route(source: &Value) -> ProviderResult<moddb::ModDbRouteIdentity> {
    let provider = KnownProvider::ModDb;
    let raw_url = source
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
    let route = moddb::route_identity(raw_url, None)?;

    if let Some(raw_id) = source.get("id") {
        let explicit_id = normalized_resource_id(raw_id)
            .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
        if explicit_id != route.resource_id {
            return Err(ProviderFailure::invalid_payload(provider));
        }
    }

    if let Some(raw_scope) = source.get("scope") {
        let explicit_scope = raw_scope
            .as_str()
            .and_then(normalized_provider_scope)
            .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
        if explicit_scope != route.scope {
            return Err(ProviderFailure::invalid_payload(provider));
        }
    }

    Ok(route)
}

fn gamebanana_identity_marker(source: &Value) -> ProviderResult<LegacyIdentityMarker> {
    let provider = KnownProvider::GameBanana;
    let explicit_id = numeric_identity_alias(source, &["id", "gamebanana_id"], provider)?;
    let explicit_scope = scope_identity_alias(
        source,
        &["scope", "model", "gamebanana_model"],
        provider,
        normalize_gamebanana_model,
    )?;
    let from_url = optional_string_value(source, "url", provider)?
        .map(|url| {
            gamebanana_identity_from_url(url)
                .ok_or_else(|| ProviderFailure::invalid_payload(provider))
        })
        .transpose()?;
    Ok(LegacyIdentityMarker {
        provider,
        resource_id: merge_marker_value(
            provider,
            explicit_id,
            from_url.as_ref().map(|(_, id)| id.clone()),
        )?,
        scope: merge_marker_value(provider, explicit_scope, from_url.map(|(scope, _)| scope))?,
        artifact_id: numeric_identity_alias(
            source,
            &["fileId", "file_id", "source_file_id"],
            provider,
        )?,
    })
}

fn numeric_identity_alias(
    source: &Value,
    keys: &[&str],
    provider: KnownProvider,
) -> ProviderResult<Option<String>> {
    positive_numeric_alias(source, keys, provider)
}

fn resource_identity_alias(
    source: &Value,
    keys: &[&str],
    provider: KnownProvider,
) -> ProviderResult<Option<String>> {
    let mut merged = None;
    for key in keys {
        if let Some(raw) = source.get(*key) {
            let value = normalized_resource_id(raw)
                .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
            merged = merge_marker_value(provider, merged, Some(value))?;
        }
    }
    Ok(merged)
}

fn scope_identity_alias(
    source: &Value,
    keys: &[&str],
    provider: KnownProvider,
    normalize: fn(&str) -> Option<String>,
) -> ProviderResult<Option<String>> {
    let mut merged = None;
    for key in keys {
        if let Some(raw) = source.get(*key) {
            let value = raw
                .as_str()
                .and_then(normalize)
                .ok_or_else(|| ProviderFailure::invalid_payload(provider))?;
            merged = merge_marker_value(provider, merged, Some(value))?;
        }
    }
    Ok(merged)
}

fn merge_marker_value(
    provider: KnownProvider,
    explicit: Option<String>,
    from_url: Option<String>,
) -> ProviderResult<Option<String>> {
    if explicit
        .as_deref()
        .zip(from_url.as_deref())
        .is_some_and(|(left, right)| left != right)
    {
        Err(ProviderFailure::invalid_payload(provider))
    } else {
        Ok(explicit.or(from_url))
    }
}

fn local_fallback(
    archive_sha256: Option<&str>,
    version: Option<&str>,
) -> ProviderResult<ProviderSourceRecord> {
    let hash = archive_sha256
        .and_then(normalize_sha256)
        .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
    local::source_record(&hash, version)
}

fn normalize_gamebanana_model(value: &str) -> Option<String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("mod") || value.eq_ignore_ascii_case("mods") {
        Some("mod".into())
    } else if value.eq_ignore_ascii_case("wip") || value.eq_ignore_ascii_case("wips") {
        Some("wip".into())
    } else {
        None
    }
}

fn gamebanana_identity_from_url(raw: &str) -> Option<(String, String)> {
    let url = canonical_url(raw, GAMEBANANA_ROOTS).ok()?;
    if !matches!(
        url.host_str(),
        Some("gamebanana.com" | "www.gamebanana.com")
    ) || url.path().ends_with('/')
    {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    if segments.len() != 2 || segments.iter().any(|segment| segment.is_empty()) {
        return None;
    }
    let scope = normalize_gamebanana_model(segments[0])?;
    let id = segments[1].parse::<u64>().ok().filter(|id| *id > 0)?;
    Some((scope, id.to_string()))
}

fn nexus_identity_from_url(raw: &str) -> Option<(String, String)> {
    let url = canonical_url(raw, NEXUS_PUBLIC_ROOTS).ok()?;
    if !matches!(url.host_str(), Some("nexusmods.com" | "www.nexusmods.com"))
        || url.path().ends_with('/')
    {
        return None;
    }
    let segments = url.path_segments()?.collect::<Vec<_>>();
    if segments.len() != 3
        || segments.iter().any(|segment| segment.is_empty())
        || !segments[1].eq_ignore_ascii_case("mods")
    {
        return None;
    }
    let domain = segments[0].to_ascii_lowercase();
    let id = segments[2].parse::<u64>().ok().filter(|id| *id > 0)?;
    valid_scope_slug(&domain).then_some((domain, id.to_string()))
}

fn valid_scope_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && !credential_shaped_scope(value)
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && ProviderScope::parse(value).is_ok()
}

fn normalized_nexus_scope(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() > 80 {
        return None;
    }
    let value = value.to_ascii_lowercase();
    valid_scope_slug(&value).then_some(value)
}

fn normalized_provider_scope(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 {
        return None;
    }
    let value = value.to_ascii_lowercase();
    ProviderScope::parse(&value).ok().map(|_| value)
}

fn normalized_resource_id(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => {
            let value = value.trim();
            ProviderResourceId::parse(value)
                .ok()
                .map(|_| value.to_owned())
        }
        Value::Number(value) => {
            let value = value.to_string();
            ProviderResourceId::parse(&value).ok().map(|_| value)
        }
        _ => None,
    }
}

fn optional_string_value<'a>(
    value: &'a Value,
    key: &str,
    provider: KnownProvider,
) -> ProviderResult<Option<&'a str>> {
    match value.get(key) {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(ProviderFailure::invalid_payload(provider)),
    }
}

fn legacy_archive_hash(value: &Value) -> ProviderResult<Option<String>> {
    let mut hash = None;
    for key in ["archiveSha256", "archive_sha256", "sha256"] {
        if let Some(raw) = value.get(key) {
            let candidate = raw
                .as_str()
                .and_then(normalize_sha256)
                .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
            hash = merge_marker_value(KnownProvider::Local, hash, Some(candidate))?;
        }
    }
    Ok(hash)
}

fn legacy_version(value: &Value) -> ProviderResult<Option<String>> {
    let mut version = None;
    for raw in [value.get("version"), value.pointer("/metadata/version")]
        .into_iter()
        .flatten()
    {
        let candidate = raw
            .as_str()
            .and_then(normalize_version_label)
            .ok_or_else(|| ProviderFailure::invalid_payload(KnownProvider::Local))?;
        version = merge_marker_value(KnownProvider::Local, version, Some(candidate))?;
    }
    Ok(version)
}
