#![forbid(unsafe_code)]
//! Native, Tauri-independent resolver and bounded downloader for `downloadGame`.

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::StreamExt;
use reqwest::{header, redirect::Policy, Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tempfile::Builder as TempBuilder;
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt};
use url::Url;
use uuid::Uuid;

mod butlerd;
pub use butlerd::{ButlerConfig, ButlerProgress, ButlerdAdapter};

pub const MAX_METADATA_BYTES: u64 = 1024 * 1024;
pub const MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_ARCHIVE_FILES: u64 = 100_000;
const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Itch,
    GameJolt,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Win32,
    Linux,
    Darwin,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Win32
        } else if cfg!(target_os = "macos") {
            Self::Darwin
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Original,
    Expanded,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Integrity {
    pub sha256: String,
    #[serde(default)]
    pub ed25519_signature: Option<String>,
    #[serde(default)]
    pub ed25519_public_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum ProviderMetadata {
    Itch {
        homepage: String,
        file_id: String,
        game_id: String,
    },
    GameJolt {
        build_id: String,
        game_id: String,
    },
}

impl ProviderMetadata {
    pub const fn provider(&self) -> Provider {
        match self {
            Self::Itch { .. } => Provider::Itch,
            Self::GameJolt { .. } => Provider::GameJolt,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArtifact {
    pub metadata: ProviderMetadata,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    #[serde(default = "default_edition")]
    pub edition: Edition,
    #[serde(default)]
    pub integrity: Option<Integrity>,
}

const fn default_edition() -> Edition {
    Edition::Original
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGame {
    pub id: String,
    pub name: String,
    pub executable: String,
    pub artifacts: Vec<CatalogArtifact>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    games: BTreeMap<String, CatalogGame>,
}

impl Catalog {
    pub fn new(games: impl IntoIterator<Item = CatalogGame>) -> Result<Self, RuntimeError> {
        let mut indexed = BTreeMap::new();
        for game in games {
            validate_game(&game)?;
            if indexed.insert(game.id.clone(), game).is_some() {
                return Err(RuntimeError::InvalidCatalog("duplicate game id"));
            }
        }
        Ok(Self { games: indexed })
    }

    /// Converts the packaged Electron game JSON without accepting any URL from IPC.
    pub fn from_legacy_values(values: &[serde_json::Value]) -> Result<Self, RuntimeError> {
        let mut games = Vec::new();
        for value in values {
            let id = bounded_text(value.get("id"), 80, "game id")?;
            let name = bounded_text(value.get("name"), 120, "game name")?;
            let executable = bounded_text(value.get("exeName"), 160, "executable")?;
            let feature = value
                .get("availableFeatures")
                .and_then(serde_json::Value::as_array)
                .and_then(|features| {
                    features.iter().find(|feature| {
                        feature.get("feat").and_then(serde_json::Value::as_str)
                            == Some("autodownload")
                    })
                });
            let Some(data) = feature.and_then(|feature| feature.get("data")) else {
                continue;
            };
            let plugin = bounded_text(data.get("pluginName"), 16, "provider")?;
            let metadata = match plugin.as_str() {
                "Itch" => ProviderMetadata::Itch {
                    homepage: bounded_text(data.get("homepage"), 300, "homepage")?,
                    file_id: numeric_id(data.get("fileID"), "file id")?,
                    game_id: numeric_id(data.get("gameId"), "Itch game id")?,
                },
                "GameJolt" => ProviderMetadata::GameJolt {
                    build_id: numeric_id(data.get("buildId"), "build id")?,
                    game_id: numeric_id(data.get("gameId"), "game id")?,
                },
                _ => return Err(RuntimeError::InvalidCatalog("unsupported provider")),
            };
            games.push(CatalogGame {
                id,
                name,
                executable,
                artifacts: vec![CatalogArtifact {
                    metadata,
                    platforms: Vec::new(),
                    edition: Edition::Original,
                    integrity: None,
                }],
            });
        }
        Self::new(games)
    }

    fn select(
        &self,
        request: &GameRequest,
    ) -> Result<(&CatalogGame, &CatalogArtifact), RuntimeError> {
        validate_game_id(&request.game_id)?;
        let game = self
            .games
            .get(&request.game_id)
            .ok_or(RuntimeError::GameUnavailable)?;
        let artifact = game
            .artifacts
            .iter()
            .find(|artifact| {
                artifact.edition == request.edition
                    && (artifact.platforms.is_empty()
                        || artifact.platforms.contains(&request.platform))
            })
            .ok_or(RuntimeError::ArtifactUnavailable)?;
        Ok((game, artifact))
    }

    pub fn selected(
        &self,
        request: &GameRequest,
    ) -> Result<(CatalogGame, CatalogArtifact), RuntimeError> {
        self.select(request)
            .map(|(game, artifact)| (game.clone(), artifact.clone()))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameRequest {
    pub game_id: String,
    #[serde(default = "Platform::current")]
    pub platform: Platform,
    #[serde(default = "default_edition")]
    pub edition: Edition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedArtifact {
    pub download_url: Url,
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub integrity: Option<Integrity>,
}

#[async_trait]
pub trait ProviderResolver: Send + Sync {
    async fn resolve(
        &self,
        metadata: &ProviderMetadata,
        cancelled: &CancellationToken,
    ) -> Result<ResolvedArtifact, RuntimeError>;
}

#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn checkpoint(&self) -> Result<(), RuntimeError> {
        if self.is_cancelled() {
            Err(RuntimeError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub operation_id: Uuid,
    pub phase: DownloadPhase,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadPhase {
    Resolving,
    Download,
    Verifying,
    Ready,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveLimits {
    pub max_archive_bytes: u64,
    pub max_expanded_bytes: u64,
    pub max_files: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTransactionPlan {
    pub game_pid: String,
    pub game_name: String,
    pub game_platform: Platform,
    pub edition: Edition,
    pub loaded_deltarune: bool,
    pub is_steam: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportTransactionPlan {
    pub operation_id: Uuid,
    pub archive_path: PathBuf,
    pub provider: Provider,
    pub executable: String,
    pub sha256: String,
    pub archive_limits: ArchiveLimits,
    pub unwrap_single_root: bool,
    pub delete_archive_after_import: bool,
    pub profile: ProfileTransactionPlan,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid packaged game catalog: {0}")]
    InvalidCatalog(&'static str),
    #[error("game is not available for native download")]
    GameUnavailable,
    #[error("game has no download for the selected platform and edition")]
    ArtifactUnavailable,
    #[error("provider configuration is required: {0}")]
    ProviderConfigurationRequired(&'static str),
    #[error("ITCH_AUTH_REQUIRED: Sign in to itch.io in the official native authentication flow before downloading this game")]
    ItchAuthRequired,
    #[error("BUTLER_PROVENANCE_REQUIRED: A checksum-verified packaged butler sidecar is required")]
    ButlerUnavailable,
    #[error("butlerd protocol failed: {0}")]
    ButlerProtocol(&'static str),
    #[error("provider returned an invalid response")]
    InvalidProviderResponse,
    #[error("provider returned HTTP {0}")]
    Http(u16),
    #[error("network request failed")]
    Network,
    #[error("download URL is not approved")]
    UnapprovedUrl,
    #[error("provider response exceeded its size limit")]
    MetadataTooLarge,
    #[error("game archive exceeded its size limit")]
    ArchiveTooLarge,
    #[error("game download was cancelled")]
    Cancelled,
    #[error("game archive checksum did not match trusted metadata")]
    ChecksumMismatch,
    #[error("game archive signature is invalid")]
    SignatureInvalid,
    #[error("download storage is unavailable")]
    Storage,
}

#[derive(Clone)]
pub struct Runtime {
    catalog: Catalog,
    resolver: Arc<dyn ProviderResolver>,
    client: Client,
    download_root: PathBuf,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Runtime")
            .field("download_root", &self.download_root)
            .finish_non_exhaustive()
    }
}

impl Runtime {
    pub fn catalog_selection(
        &self,
        request: &GameRequest,
    ) -> Result<(CatalogGame, CatalogArtifact), RuntimeError> {
        self.catalog.selected(request)
    }

    pub async fn new(
        catalog: Catalog,
        resolver: Arc<dyn ProviderResolver>,
        download_root: impl Into<PathBuf>,
    ) -> Result<Self, RuntimeError> {
        let download_root = download_root.into();
        fs::create_dir_all(&download_root)
            .await
            .map_err(|_| RuntimeError::Storage)?;
        let metadata = fs::symlink_metadata(&download_root)
            .await
            .map_err(|_| RuntimeError::Storage)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(RuntimeError::Storage);
        }
        let download_root = fs::canonicalize(download_root)
            .await
            .map_err(|_| RuntimeError::Storage)?;
        let client = secure_client()?;
        Ok(Self {
            catalog,
            resolver,
            client,
            download_root,
        })
    }

    pub async fn download_game<F>(
        &self,
        request: GameRequest,
        cancelled: CancellationToken,
        mut progress: F,
    ) -> Result<ImportTransactionPlan, RuntimeError>
    where
        F: FnMut(ProgressEvent) + Send,
    {
        let operation_id = Uuid::new_v4();
        let (game, selected) = self.catalog.select(&request)?;
        let game = game.clone();
        let selected = selected.clone();
        progress_event(
            &mut progress,
            operation_id,
            DownloadPhase::Resolving,
            0,
            None,
            None,
        );
        cancelled.checkpoint()?;
        let mut resolved = self
            .resolver
            .resolve(&selected.metadata, &cancelled)
            .await?;
        cancelled.checkpoint()?;
        validate_provider_url(selected.metadata.provider(), &resolved.download_url)?;
        if let Some(expected) = selected.integrity {
            resolved.integrity = Some(expected);
        }
        if resolved.size.is_some_and(|size| size > MAX_ARCHIVE_BYTES) {
            return Err(RuntimeError::ArchiveTooLarge);
        }

        let temporary = TempBuilder::new()
            .prefix("game-download-")
            .suffix(".part")
            .tempfile_in(&self.download_root)
            .map_err(|_| RuntimeError::Storage)?;
        let (_file, part_path) = temporary.keep().map_err(|_| RuntimeError::Storage)?;
        let final_path = self
            .download_root
            .join(format!("{operation_id}.game-archive"));
        let result = self
            .download_resolved(
                operation_id,
                selected.metadata.provider(),
                resolved,
                &part_path,
                &cancelled,
                &mut progress,
            )
            .await;
        let (sha256, file_name) = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = fs::remove_file(&part_path).await;
                return Err(error);
            }
        };
        if cancelled.checkpoint().is_err() {
            let _ = fs::remove_file(&part_path).await;
            return Err(RuntimeError::Cancelled);
        }
        if fs::rename(&part_path, &final_path).await.is_err() {
            let _ = fs::remove_file(&part_path).await;
            return Err(RuntimeError::Storage);
        }
        progress_event(
            &mut progress,
            operation_id,
            DownloadPhase::Ready,
            1,
            Some(1),
            file_name,
        );
        Ok(ImportTransactionPlan {
            operation_id,
            archive_path: final_path,
            provider: selected.metadata.provider(),
            executable: game.executable,
            sha256,
            archive_limits: ArchiveLimits {
                max_archive_bytes: MAX_ARCHIVE_BYTES,
                max_expanded_bytes: MAX_EXPANDED_BYTES,
                max_files: MAX_ARCHIVE_FILES,
            },
            unwrap_single_root: true,
            delete_archive_after_import: true,
            profile: ProfileTransactionPlan {
                game_pid: game.id,
                game_name: game.name,
                game_platform: request.platform,
                edition: request.edition,
                loaded_deltarune: true,
                is_steam: false,
            },
        })
    }

    async fn download_resolved<F>(
        &self,
        operation_id: Uuid,
        provider: Provider,
        resolved: ResolvedArtifact,
        destination: &Path,
        cancelled: &CancellationToken,
        progress: &mut F,
    ) -> Result<(String, Option<String>), RuntimeError>
    where
        F: FnMut(ProgressEvent) + Send,
    {
        let response = request_with_redirects(
            &self.client,
            resolved.download_url,
            provider,
            MAX_REDIRECTS,
            cancelled,
        )
        .await?;
        let declared = response.content_length().or(resolved.size);
        if declared.is_some_and(|size| size > MAX_ARCHIVE_BYTES) {
            return Err(RuntimeError::ArchiveTooLarge);
        }
        let mut output = fs::File::create(destination)
            .await
            .map_err(|_| RuntimeError::Storage)?;
        let mut stream = response.bytes_stream();
        let mut hasher = Sha256::new();
        let mut completed = 0_u64;
        while let Some(chunk) = stream.next().await {
            cancelled.checkpoint()?;
            let chunk = chunk.map_err(|_| RuntimeError::Network)?;
            completed = completed
                .checked_add(chunk.len() as u64)
                .ok_or(RuntimeError::ArchiveTooLarge)?;
            if completed > MAX_ARCHIVE_BYTES {
                return Err(RuntimeError::ArchiveTooLarge);
            }
            hasher.update(&chunk);
            output
                .write_all(&chunk)
                .await
                .map_err(|_| RuntimeError::Storage)?;
            progress_event(
                progress,
                operation_id,
                DownloadPhase::Download,
                completed,
                declared,
                resolved.file_name.clone(),
            );
        }
        output.flush().await.map_err(|_| RuntimeError::Storage)?;
        output.sync_all().await.map_err(|_| RuntimeError::Storage)?;
        let digest: [u8; 32] = hasher.finalize().into();
        let actual = hex::encode(digest);
        progress_event(
            progress,
            operation_id,
            DownloadPhase::Verifying,
            completed,
            Some(completed),
            resolved.file_name.clone(),
        );
        if let Some(integrity) = resolved.integrity {
            verify_integrity(&integrity, &actual, &digest)?;
        }
        Ok((actual, resolved.file_name))
    }
}

#[derive(Clone, Debug)]
pub struct BuiltInResolver {
    client: Client,
}

impl BuiltInResolver {
    pub fn new() -> Result<Self, RuntimeError> {
        Ok(Self {
            client: secure_client()?,
        })
    }
}

#[async_trait]
impl ProviderResolver for BuiltInResolver {
    async fn resolve(
        &self,
        metadata: &ProviderMetadata,
        cancelled: &CancellationToken,
    ) -> Result<ResolvedArtifact, RuntimeError> {
        cancelled.checkpoint()?;
        match metadata {
            ProviderMetadata::GameJolt { build_id, .. } => {
                let url = Url::parse(&format!(
                    "https://gamejolt.com/site-api/web/discover/games/builds/get-download-url/{build_id}"
                ))
                .map_err(|_| RuntimeError::InvalidCatalog("invalid build id"))?;
                let response = self
                    .client
                    .post(url)
                    .header(header::COOKIE, "gjtz=7200;")
                    .json(&serde_json::json!({ "forceDownload": true }))
                    .send()
                    .await
                    .map_err(|_| RuntimeError::Network)?;
                parse_gamejolt_response(response, cancelled).await
            }
            ProviderMetadata::Itch { .. } => Err(RuntimeError::ProviderConfigurationRequired(
                "Itch has no documented anonymous file-download resolver; configure an authenticated documented API adapter",
            )),
        }
    }
}

async fn parse_gamejolt_response(
    response: Response,
    cancelled: &CancellationToken,
) -> Result<ResolvedArtifact, RuntimeError> {
    let status = response.status();
    if !status.is_success() {
        return Err(RuntimeError::Http(status.as_u16()));
    }
    let body = bounded_body(response, MAX_METADATA_BYTES, cancelled).await?;
    parse_gamejolt_json(&body)
}

pub fn parse_gamejolt_json(body: &[u8]) -> Result<ResolvedArtifact, RuntimeError> {
    #[derive(Deserialize)]
    struct Envelope {
        payload: Payload,
    }
    #[derive(Deserialize)]
    struct Payload {
        url: String,
    }
    let envelope: Envelope =
        serde_json::from_slice(body).map_err(|_| RuntimeError::InvalidProviderResponse)?;
    let download_url =
        Url::parse(&envelope.payload.url).map_err(|_| RuntimeError::InvalidProviderResponse)?;
    validate_provider_url(Provider::GameJolt, &download_url)?;
    Ok(ResolvedArtifact {
        download_url,
        file_name: None,
        size: None,
        integrity: None,
    })
}

async fn bounded_body(
    response: Response,
    maximum: u64,
    cancelled: &CancellationToken,
) -> Result<Vec<u8>, RuntimeError> {
    if response.content_length().is_some_and(|size| size > maximum) {
        return Err(RuntimeError::MetadataTooLarge);
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        cancelled.checkpoint()?;
        let chunk = chunk.map_err(|_| RuntimeError::Network)?;
        if body.len().saturating_add(chunk.len()) > maximum as usize {
            return Err(RuntimeError::MetadataTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn request_with_redirects(
    client: &Client,
    mut url: Url,
    provider: Provider,
    maximum_redirects: usize,
    cancelled: &CancellationToken,
) -> Result<Response, RuntimeError> {
    for redirect in 0..=maximum_redirects {
        cancelled.checkpoint()?;
        validate_provider_url(provider, &url)?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| RuntimeError::Network)?;
        if response.status().is_redirection() {
            if redirect == maximum_redirects {
                return Err(RuntimeError::UnapprovedUrl);
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(RuntimeError::UnapprovedUrl)?;
            url = url
                .join(location)
                .map_err(|_| RuntimeError::UnapprovedUrl)?;
            validate_provider_url(provider, &url)?;
            continue;
        }
        if response.status() != StatusCode::OK {
            return Err(RuntimeError::Http(response.status().as_u16()));
        }
        return Ok(response);
    }
    Err(RuntimeError::UnapprovedUrl)
}

pub fn validate_provider_url(provider: Provider, url: &Url) -> Result<(), RuntimeError> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
    {
        return Err(RuntimeError::UnapprovedUrl);
    }
    let host = url.host_str().ok_or(RuntimeError::UnapprovedUrl)?;
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Err(RuntimeError::UnapprovedUrl);
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let approved = match provider {
        Provider::Itch => ["itch.io", "hwcdn.net"].as_slice(),
        Provider::GameJolt => ["gamejolt.com", "gamejolt.net", "gjcdn.net"].as_slice(),
    };
    if approved
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        Ok(())
    } else {
        Err(RuntimeError::UnapprovedUrl)
    }
}

fn verify_integrity(
    integrity: &Integrity,
    actual_hex: &str,
    digest: &[u8; 32],
) -> Result<(), RuntimeError> {
    if integrity.sha256.len() != 64
        || !integrity
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !integrity.sha256.eq_ignore_ascii_case(actual_hex)
    {
        return Err(RuntimeError::ChecksumMismatch);
    }
    match (
        integrity.ed25519_signature.as_deref(),
        integrity.ed25519_public_key.as_deref(),
    ) {
        (None, None) => Ok(()),
        (Some(signature), Some(public_key)) => {
            let signature = BASE64
                .decode(signature)
                .ok()
                .and_then(|bytes| Signature::from_slice(&bytes).ok())
                .ok_or(RuntimeError::SignatureInvalid)?;
            let public_key: [u8; 32] = BASE64
                .decode(public_key)
                .ok()
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or(RuntimeError::SignatureInvalid)?;
            let key = VerifyingKey::from_bytes(&public_key)
                .map_err(|_| RuntimeError::SignatureInvalid)?;
            key.verify(digest, &signature)
                .map_err(|_| RuntimeError::SignatureInvalid)
        }
        _ => Err(RuntimeError::SignatureInvalid),
    }
}

fn secure_client() -> Result<Client, RuntimeError> {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent("Deltamod-Community/native-game-download")
        .build()
        .map_err(|_| RuntimeError::Network)
}

fn validate_game(game: &CatalogGame) -> Result<(), RuntimeError> {
    validate_game_id(&game.id)?;
    if game.name.trim().is_empty()
        || game.name.len() > 120
        || game.name.chars().any(char::is_control)
    {
        return Err(RuntimeError::InvalidCatalog("invalid game name"));
    }
    if game.executable.is_empty()
        || game.executable.len() > 160
        || game.executable.contains(['/', '\\'])
        || game.executable == "."
        || game.executable == ".."
        || game.executable.chars().any(char::is_control)
    {
        return Err(RuntimeError::InvalidCatalog("invalid executable"));
    }
    if game.artifacts.is_empty() || game.artifacts.len() > 16 {
        return Err(RuntimeError::InvalidCatalog("invalid artifact count"));
    }
    for artifact in &game.artifacts {
        match &artifact.metadata {
            ProviderMetadata::Itch {
                homepage,
                file_id,
                game_id,
            } => {
                let url = Url::parse(homepage)
                    .map_err(|_| RuntimeError::InvalidCatalog("invalid Itch homepage"))?;
                validate_provider_url(Provider::Itch, &url)
                    .map_err(|_| RuntimeError::InvalidCatalog("invalid Itch homepage"))?;
                numeric_string(file_id, "invalid Itch file id")?;
                numeric_string(game_id, "invalid Itch game id")?;
            }
            ProviderMetadata::GameJolt { build_id, game_id } => {
                numeric_string(build_id, "invalid GameJolt build id")?;
                numeric_string(game_id, "invalid GameJolt game id")?;
            }
        }
        if let Some(integrity) = &artifact.integrity {
            if integrity.sha256.len() != 64
                || !integrity
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(RuntimeError::InvalidCatalog("invalid SHA-256"));
            }
            if integrity.ed25519_signature.is_some() != integrity.ed25519_public_key.is_some() {
                return Err(RuntimeError::InvalidCatalog(
                    "incomplete signature metadata",
                ));
            }
        }
    }
    Ok(())
}

fn validate_game_id(value: &str) -> Result<(), RuntimeError> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        Err(RuntimeError::InvalidCatalog("invalid game id"))
    } else {
        Ok(())
    }
}

fn numeric_string<'a>(value: &'a str, message: &'static str) -> Result<&'a str, RuntimeError> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        Err(RuntimeError::InvalidCatalog(message))
    } else {
        Ok(value)
    }
}

fn bounded_text(
    value: Option<&serde_json::Value>,
    maximum: usize,
    field: &'static str,
) -> Result<String, RuntimeError> {
    value
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= maximum)
        .map(str::to_owned)
        .ok_or(RuntimeError::InvalidCatalog(field))
}

fn numeric_id(
    value: Option<&serde_json::Value>,
    field: &'static str,
) -> Result<String, RuntimeError> {
    let value = bounded_text(value, 20, field)?;
    numeric_string(&value, field)?;
    Ok(value)
}

fn progress_event<F>(
    progress: &mut F,
    operation_id: Uuid,
    phase: DownloadPhase,
    completed: u64,
    total: Option<u64>,
    current_item: Option<String>,
) where
    F: FnMut(ProgressEvent),
{
    progress(ProgressEvent {
        operation_id,
        phase,
        completed,
        total,
        current_item,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn legacy_catalog() -> Catalog {
        Catalog::from_legacy_values(&[
            serde_json::from_str(include_str!("../tests/fixtures/itch-game.json")).unwrap(),
            serde_json::from_str(include_str!("../tests/fixtures/gamejolt-game.json")).unwrap(),
        ])
        .unwrap()
    }

    #[test]
    fn legacy_contract_loads_only_app_owned_provider_metadata() {
        let catalog = legacy_catalog();
        let request = GameRequest {
            game_id: "fans.utyellow".into(),
            platform: Platform::Win32,
            edition: Edition::Original,
        };
        let (_, artifact) = catalog.select(&request).unwrap();
        assert_eq!(
            artifact.metadata,
            ProviderMetadata::GameJolt {
                build_id: "1857285".into(),
                game_id: "136925".into()
            }
        );
    }

    #[test]
    fn parses_exact_gamejolt_payload_and_rejects_host_lookalikes() {
        let resolved = parse_gamejolt_json(include_bytes!(
            "../tests/fixtures/gamejolt-download-response.json"
        ))
        .unwrap();
        assert_eq!(resolved.download_url.host_str(), Some("cdn.gamejolt.net"));
        for url in [
            "https://gamejolt.com.evil.example/a.zip",
            "http://gamejolt.com/a.zip",
            "https://127.0.0.1/a.zip",
            "https://gamejolt.com:444/a.zip",
        ] {
            assert!(validate_provider_url(Provider::GameJolt, &Url::parse(url).unwrap()).is_err());
        }
    }

    #[test]
    fn platform_and_edition_are_selected_natively() {
        let mut game = legacy_catalog().games["fans.utyellow"].clone();
        game.artifacts[0].platforms = vec![Platform::Win32];
        assert!(Catalog::new([game])
            .unwrap()
            .select(&GameRequest {
                game_id: "fans.utyellow".into(),
                platform: Platform::Linux,
                edition: Edition::Original,
            })
            .is_err());
    }

    #[test]
    fn verifies_checksum_and_signature_when_available() {
        let digest: [u8; 32] = Sha256::digest(b"archive").into();
        let signing = SigningKey::from_bytes(&[7_u8; 32]);
        let integrity = Integrity {
            sha256: hex::encode(digest),
            ed25519_signature: Some(BASE64.encode(signing.sign(&digest).to_bytes())),
            ed25519_public_key: Some(BASE64.encode(signing.verifying_key().to_bytes())),
        };
        assert!(verify_integrity(&integrity, &hex::encode(digest), &digest).is_ok());
        let wrong: [u8; 32] = Sha256::digest(b"wrong").into();
        assert!(verify_integrity(&integrity, &hex::encode(wrong), &wrong).is_err());
    }

    #[tokio::test]
    async fn cancellation_is_observable_before_provider_io() {
        let resolver = BuiltInResolver::new().unwrap();
        let token = CancellationToken::default();
        token.cancel();
        let result = resolver
            .resolve(
                &ProviderMetadata::GameJolt {
                    build_id: "1857285".into(),
                    game_id: "136925".into(),
                },
                &token,
            )
            .await;
        assert!(matches!(result, Err(RuntimeError::Cancelled)));
    }

    #[tokio::test]
    async fn itch_is_a_truthful_configuration_blocker_not_a_scraper() {
        let resolver = BuiltInResolver::new().unwrap();
        let catalog = legacy_catalog();
        let (_, artifact) = catalog
            .select(&GameRequest {
                game_id: "toby.deltarune.demolts".into(),
                platform: Platform::Win32,
                edition: Edition::Original,
            })
            .unwrap();
        assert!(matches!(
            resolver
                .resolve(&artifact.metadata, &CancellationToken::default())
                .await,
            Err(RuntimeError::ProviderConfigurationRequired(_))
        ));
    }
}
