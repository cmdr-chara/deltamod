#![forbid(unsafe_code)]
#![warn(clippy::all)]

pub mod import_download;

use futures_util::StreamExt;
use reqwest::{header, Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tempfile::NamedTempFile;
use thiserror::Error;
use tokio::{io::AsyncWriteExt, sync::Semaphore, time::sleep};

pub const MAX_REDIRECTS: u8 = 5;
pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_API_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    Nexus,
    ModDb,
    GameBanana,
}
impl Provider {
    fn host_ok(self, h: &str) -> bool {
        let h = h.to_ascii_lowercase();
        match self {
            Self::Nexus => {
                h == "api.nexusmods.com"
                    || h.ends_with(".nexusmods.com")
                    || h.ends_with(".nexus-cdn.com")
            }
            Self::ModDb => h == "www.moddb.com" || h.ends_with(".moddb.com"),
            Self::GameBanana => h == "gamebanana.com" || h.ends_with(".gamebanana.com"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgressEnvelope {
    pub operation_id: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CurrentEnvelope<T> {
    pub operation_id: String,
    pub value: T,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset_unix: Option<i64>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub operation_id: Option<String>,
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
    pub quota: BTreeMap<String, QuotaWindow>,
}
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid or unsafe URL: {0}")]
    Url(String),
    #[error("HTTP {status}: {message}")]
    Http {
        status: u16,
        message: String,
        envelope: Box<ErrorEnvelope>,
    },
    #[error("response exceeded {limit} bytes")]
    TooLarge { limit: u64 },
    #[error("operation cancelled")]
    Cancelled,
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("request: {0}")]
    Request(#[from] reqwest::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("XML: {0}")]
    Xml(String),
    #[error("authentication is unavailable: {0}")]
    Auth(String),
    #[error("unsupported contract: {0}")]
    Unsupported(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[derive(Clone, Copy)]
enum Authentication<'a> {
    GameBananaCookie(&'a str),
    NexusBearer(&'a str),
}

impl Authentication<'_> {
    fn allowed_for(self, url: &Url) -> bool {
        match self {
            Self::GameBananaCookie(_) => url.host_str() == Some("gamebanana.com"),
            Self::NexusBearer(_) => url.host_str() == Some("api.nexusmods.com"),
        }
    }
}

pub trait SecretStore: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, Self::Error>;
    fn put(&self, name: &str, value: &[u8]) -> Result<(), Self::Error>;
    fn delete(&self, name: &str) -> Result<(), Self::Error>;
}
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    max_redirects: u8,
    pace: Arc<Semaphore>,
    min_interval: Duration,
}
impl Client {
    pub fn new(
        timeout: Duration,
        max_concurrent: u32,
        min_interval: Duration,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(timeout)
                .user_agent("DeltamodNetworkRuntime/0.1")
                .build()?,
            max_redirects: MAX_REDIRECTS,
            pace: Arc::new(Semaphore::new(max_concurrent.max(1) as usize)),
            min_interval,
        })
    }
    pub fn with_redirect_limit(mut self, n: u8) -> Self {
        self.max_redirects = n;
        self
    }
}

fn safe_url(provider: Provider, raw: &str) -> Result<Url, RuntimeError> {
    let u = Url::parse(raw).map_err(|e| RuntimeError::Url(e.to_string()))?;
    if u.scheme() != "https" || u.username() != "" || u.password().is_some() {
        return Err(RuntimeError::Url(
            "HTTPS URL without credentials required".into(),
        ));
    }
    let h = u
        .host_str()
        .ok_or_else(|| RuntimeError::Url("missing host".into()))?;
    if h == "localhost" || h.parse::<std::net::IpAddr>().is_ok() || !provider.host_ok(h) {
        return Err(RuntimeError::Url(format!("host not allowed: {h}")));
    }
    Ok(u)
}
fn retry_after(h: &header::HeaderMap) -> Option<u64> {
    h.get(header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| s * 1000)
}
fn quota(h: &header::HeaderMap) -> BTreeMap<String, QuotaWindow> {
    let mut m = BTreeMap::new();
    for (k, v) in h {
        let n = k.as_str().to_ascii_lowercase();
        if let Some(x) = n.strip_prefix("x-rl-") {
            if let Some((p, f)) = x.split_once('-') {
                if let Ok(v) = v.to_str().unwrap_or("").parse() {
                    let q = m.entry(p.to_owned()).or_insert(QuotaWindow {
                        limit: None,
                        remaining: None,
                        reset_unix: None,
                    });
                    match f {
                        "limit" => q.limit = Some(v),
                        "remaining" => q.remaining = Some(v),
                        "reset" => q.reset_unix = Some(v as i64),
                        _ => {}
                    }
                }
            }
        }
    }
    m
}
impl Client {
    async fn send(
        &self,
        provider: Provider,
        method: Method,
        start: &str,
        auth: Option<Authentication<'_>>,
        body: Option<serde_json::Value>,
    ) -> Result<reqwest::Response, RuntimeError> {
        self.send_with_error_policy(provider, method, start, auth, body, false)
            .await
    }

    async fn send_with_error_policy(
        &self,
        provider: Provider,
        method: Method,
        start: &str,
        auth: Option<Authentication<'_>>,
        body: Option<serde_json::Value>,
        allow_http_error: bool,
    ) -> Result<reqwest::Response, RuntimeError> {
        let permit = self
            .pace
            .acquire()
            .await
            .map_err(|_| RuntimeError::Cancelled)?;
        let mut url = safe_url(provider, start)?;
        for hop in 0..=self.max_redirects {
            let mut r = self.http.request(method.clone(), url.clone());
            if provider == Provider::Nexus && url.host_str() == Some("api.nexusmods.com") {
                r = r
                    .header("application-name", "Deltamod Community")
                    .header("application-version", "2.0.3");
            }
            if let Some(auth) = auth.filter(|value| value.allowed_for(&url)) {
                r = match auth {
                    Authentication::GameBananaCookie(cookie) => r.header(header::COOKIE, cookie),
                    Authentication::NexusBearer(token) => r.bearer_auth(token),
                };
            }
            if let Some(b) = body.clone() {
                r = r.json(&b);
            }
            let response = r.send().await?;
            if response.status().is_redirection() {
                let loc = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| RuntimeError::Url("redirect without Location".into()))?;
                if hop == self.max_redirects {
                    return Err(RuntimeError::Url("redirect limit exceeded".into()));
                }
                url = safe_url(
                    provider,
                    url.join(loc)
                        .map_err(|e| RuntimeError::Url(e.to_string()))?
                        .as_str(),
                )?;
                continue;
            }
            drop(permit);
            if !response.status().is_success() && !allow_http_error {
                let env = ErrorEnvelope {
                    operation_id: None,
                    code: format!("HTTP_{}", response.status().as_u16()),
                    message: response
                        .status()
                        .canonical_reason()
                        .unwrap_or("request failed")
                        .into(),
                    status: Some(response.status().as_u16()),
                    retry_after_ms: retry_after(response.headers()),
                    quota: quota(response.headers()),
                };
                return Err(RuntimeError::Http {
                    status: response.status().as_u16(),
                    message: env.message.clone(),
                    envelope: Box::new(env),
                });
            }
            return Ok(response);
        }
        Err(RuntimeError::Url("redirect loop".into()))
    }
    pub async fn json<T: for<'de> Deserialize<'de>>(
        &self,
        provider: Provider,
        url: &str,
        auth: Option<&str>,
    ) -> Result<T, RuntimeError> {
        let auth = auth.map(|value| match provider {
            Provider::Nexus => Authentication::NexusBearer(value),
            Provider::GameBanana => Authentication::GameBananaCookie(value),
            Provider::ModDb => Authentication::GameBananaCookie(value),
        });
        self.request_json(provider, Method::GET, url, auth, None)
            .await
    }

    pub async fn nexus_graphql(
        &self,
        body: serde_json::Value,
        access_token: Option<&str>,
    ) -> Result<serde_json::Value, RuntimeError> {
        self.request_json(
            Provider::Nexus,
            Method::POST,
            "https://api.nexusmods.com/v2/graphql",
            access_token.map(Authentication::NexusBearer),
            Some(body),
        )
        .await
    }

    async fn request_json<T: for<'de> Deserialize<'de>>(
        &self,
        provider: Provider,
        method: Method,
        url: &str,
        auth: Option<Authentication<'_>>,
        body: Option<serde_json::Value>,
    ) -> Result<T, RuntimeError> {
        let response = self.send(provider, method, url, auth, body).await?;
        let bytes = read_limited(response, MAX_API_RESPONSE_BYTES).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn download<F>(
        &self,
        provider: Provider,
        operation_id: String,
        url: &str,
        auth: Option<&str>,
        max_bytes: u64,
        cancel: &tokio::sync::watch::Receiver<bool>,
        mut progress: F,
    ) -> Result<PathBuf, RuntimeError>
    where
        F: FnMut(ProgressEnvelope) + Send,
    {
        let auth = auth.map(|value| match provider {
            Provider::Nexus => Authentication::NexusBearer(value),
            Provider::GameBanana => Authentication::GameBananaCookie(value),
            Provider::ModDb => Authentication::GameBananaCookie(value),
        });
        let response = self.send(provider, Method::GET, url, auth, None).await?;
        let total = response.content_length();
        if total.is_some_and(|n| n > max_bytes) {
            return Err(RuntimeError::TooLarge { limit: max_bytes });
        }
        let tmp = NamedTempFile::new()?;
        let path = tmp.path().to_path_buf();
        let mut file = tokio::fs::File::from_std(tmp.reopen()?);
        let mut done: u64 = 0;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if *cancel.borrow() {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(RuntimeError::Cancelled);
            }
            let chunk = chunk?;
            done = done.saturating_add(chunk.len() as u64);
            if done > max_bytes {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(RuntimeError::TooLarge { limit: max_bytes });
            }
            file.write_all(&chunk).await?;
            progress(ProgressEnvelope {
                operation_id: operation_id.clone(),
                completed: done,
                total,
                current_item: None,
            });
            if !self.min_interval.is_zero() {
                sleep(self.min_interval).await
            }
        }
        file.flush().await?;
        drop(file);
        tmp.keep().map_err(|e| e.error)?;
        Ok(path)
    }
}

async fn read_limited(response: reqwest::Response, limit: u64) -> Result<Vec<u8>, RuntimeError> {
    if response.content_length().is_some_and(|size| size > limit) {
        return Err(RuntimeError::TooLarge { limit });
    }
    let mut output = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if output.len() as u64 + chunk.len() as u64 > limit {
            return Err(RuntimeError::TooLarge { limit });
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NexusStatus {
    pub authenticated: bool,
    pub quota: BTreeMap<String, QuotaWindow>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NexusUser {
    pub name: Option<String>,
    pub user_id: Option<u64>,
    pub is_premium: Option<bool>,
    pub is_supporter: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NexusFile {
    pub file_id: u64,
    pub name: Option<String>,
    pub file_name: Option<String>,
    pub category_name: Option<String>,
    pub is_primary: Option<bool>,
    pub size_kb: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NexusFilesResponse {
    files: Vec<NexusFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NexusDownloadLink {
    #[serde(rename = "URI")]
    uri: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NexusResolvedDownload {
    pub download_url: String,
    pub file_id: u64,
    pub file_name: String,
    pub maximum_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModEntry {
    pub title: String,
    pub link: String,
    pub published: Option<String>,
    pub summary: Option<String>,
    pub image_url: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameBananaComment {
    pub id: Option<u64>,
    pub author: Option<String>,
    pub text: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub status: u16,
    pub data: serde_json::Value,
}
pub struct Nexus<'a> {
    pub client: &'a Client,
    pub access_token: Option<String>,
}
impl<'a> Nexus<'a> {
    fn credential(&self) -> Result<&str, RuntimeError> {
        self.access_token
            .as_deref()
            .filter(|token| valid_nexus_token(token))
            .ok_or_else(|| RuntimeError::Auth("no valid Nexus OAuth access token".into()))
    }

    pub async fn validate(&self) -> Result<NexusUser, RuntimeError> {
        self.client
            .request_json(
                Provider::Nexus,
                Method::GET,
                "https://api.nexusmods.com/v1/users/validate.json",
                Some(Authentication::NexusBearer(self.credential()?)),
                None,
            )
            .await
    }

    pub async fn status(&self) -> Result<CurrentEnvelope<NexusStatus>, RuntimeError> {
        let key = self.credential()?;
        let r = self
            .client
            .send(
                Provider::Nexus,
                Method::GET,
                "https://api.nexusmods.com/v1/users/me.json",
                Some(Authentication::NexusBearer(key)),
                None,
            )
            .await?;
        Ok(CurrentEnvelope {
            operation_id: "nexus-status".into(),
            value: NexusStatus {
                authenticated: r.status() == StatusCode::OK,
                quota: quota(r.headers()),
            },
        })
    }
    pub async fn browse<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
    ) -> Result<CurrentEnvelope<T>, RuntimeError> {
        let key = self.credential()?;
        Ok(CurrentEnvelope {
            operation_id: "nexus-browse".into(),
            value: self.client.json(Provider::Nexus, url, Some(key)).await?,
        })
    }

    pub async fn resolve_primary_download(
        &self,
        domain: &str,
        mod_id: u64,
    ) -> Result<NexusResolvedDownload, RuntimeError> {
        if !valid_slug(domain) || mod_id == 0 {
            return Err(RuntimeError::InvalidInput(
                "invalid Nexus game or mod id".into(),
            ));
        }
        let key = self.credential()?;
        let files_url =
            format!("https://api.nexusmods.com/v1/games/{domain}/mods/{mod_id}/files.json");
        let result: NexusFilesResponse = self
            .client
            .request_json(
                Provider::Nexus,
                Method::GET,
                &files_url,
                Some(Authentication::NexusBearer(key)),
                None,
            )
            .await?;
        let selected = result
            .files
            .into_iter()
            .filter(|file| {
                file.file_id > 0
                    && !matches!(
                        file.category_name
                            .as_deref()
                            .map(str::to_ascii_uppercase)
                            .as_deref(),
                        Some("DELETED" | "ARCHIVED" | "OLD_VERSION")
                    )
            })
            .min_by_key(|file| {
                if file.is_primary == Some(true) {
                    0
                } else if file
                    .category_name
                    .as_deref()
                    .is_some_and(|category| category.eq_ignore_ascii_case("MAIN"))
                {
                    1
                } else {
                    2
                }
            })
            .ok_or_else(|| RuntimeError::Unsupported("no downloadable Nexus file".into()))?;
        let links_url = format!(
            "https://api.nexusmods.com/v1/games/{domain}/mods/{mod_id}/files/{}/download_link.json",
            selected.file_id
        );
        let links: Vec<NexusDownloadLink> = self
            .client
            .request_json(
                Provider::Nexus,
                Method::GET,
                &links_url,
                Some(Authentication::NexusBearer(key)),
                None,
            )
            .await?;
        let link = links
            .into_iter()
            .find(|link| safe_url(Provider::Nexus, &link.uri).is_ok())
            .ok_or_else(|| RuntimeError::Unsupported("manual Nexus download required".into()))?;
        let advertised = selected.size_kb.unwrap_or(0).saturating_mul(1024);
        let maximum_bytes = if advertised == 0 {
            DEFAULT_MAX_BYTES
        } else {
            advertised
                .saturating_add(1024 * 1024)
                .clamp(16 * 1024 * 1024, DEFAULT_MAX_BYTES)
        };
        Ok(NexusResolvedDownload {
            download_url: link.uri,
            file_id: selected.file_id,
            file_name: safe_remote_filename(
                selected.file_name.or(selected.name).as_deref(),
                &format!("nexus-{mod_id}"),
            ),
            maximum_bytes,
        })
    }
}

fn valid_nexus_token(value: &str) -> bool {
    (20..=8192).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_slug(value: &str) -> bool {
    (1..=80).contains(&value.len())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn safe_remote_filename(value: Option<&str>, fallback: &str) -> String {
    let Some(value) = value else {
        return fallback.to_owned();
    };
    if value.is_empty()
        || value.len() > 255
        || value.chars().any(char::is_control)
        || value.contains(['/', '\\'])
        || matches!(value, "." | "..")
    {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn strip_markup(value: &str) -> String {
    let mut text = String::new();
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => {
                inside_tag = false;
                text.push(' ');
            }
            _ if !inside_tag => text.push(character),
            _ => {}
        }
    }
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    quick_xml::escape::unescape(&compact)
        .map(|value| value.into_owned())
        .unwrap_or(compact)
}

fn decode_xml_text(value: &[u8]) -> String {
    let value = String::from_utf8_lossy(value);
    quick_xml::escape::unescape(&value)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| value.into_owned())
}

pub struct ModDb<'a> {
    pub client: &'a Client,
}
fn parse_moddb_feed(text: &str) -> Result<Vec<ModEntry>, RuntimeError> {
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut title = None;
    let mut link = None;
    let mut pubd = None;
    let mut summary = None;
    let mut image_url = None;
    let mut tag = String::new();
    let mut inside_item = false;
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => {
                tag = String::from_utf8_lossy(e.name().as_ref()).into();
                if e.name().as_ref() == b"item" {
                    inside_item = true;
                    title = None;
                    link = None;
                    pubd = None;
                    summary = None;
                    image_url = None;
                } else if inside_item
                    && matches!(e.name().as_ref(), b"enclosure" | b"media:content")
                {
                    image_url = e
                        .attributes()
                        .flatten()
                        .find(|attribute| attribute.key.as_ref() == b"url")
                        .map(|attribute| {
                            String::from_utf8_lossy(attribute.value.as_ref()).into_owned()
                        });
                }
            }
            Ok(quick_xml::events::Event::Empty(e))
                if inside_item && matches!(e.name().as_ref(), b"enclosure" | b"media:content") =>
            {
                image_url = e
                    .attributes()
                    .flatten()
                    .find(|attribute| attribute.key.as_ref() == b"url")
                    .map(|attribute| {
                        String::from_utf8_lossy(attribute.value.as_ref()).into_owned()
                    });
            }
            Ok(quick_xml::events::Event::Text(e)) if inside_item => {
                let v = decode_xml_text(e.as_ref());
                match tag.as_str() {
                    "title" => title = Some(v),
                    "link" => link = Some(v),
                    "pubDate" => pubd = Some(v),
                    "description" => summary = Some(v),
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::CData(e)) if inside_item && tag == "description" => {
                summary = Some(strip_markup(&String::from_utf8_lossy(e.as_ref())));
            }
            Ok(quick_xml::events::Event::End(e)) if e.name().as_ref() == b"item" => {
                if let (Some(t), Some(l)) = (title.take(), link.take()) {
                    out.push(ModEntry {
                        title: t,
                        link: l,
                        published: pubd.take(),
                        summary: summary.take(),
                        image_url: image_url.take(),
                    })
                }
                inside_item = false;
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(RuntimeError::Xml(e.to_string())),
            _ => {}
        }
    }
    Ok(out)
}

impl<'a> ModDb<'a> {
    pub async fn browse(&self, url: &str) -> Result<CurrentEnvelope<Vec<ModEntry>>, RuntimeError> {
        let text = self
            .client
            .send(Provider::ModDb, Method::GET, url, None, None)
            .await?
            .text()
            .await?;
        Ok(CurrentEnvelope {
            operation_id: "moddb-browse".into(),
            value: parse_moddb_feed(&text)?,
        })
    }
}
pub struct GameBanana<'a> {
    pub client: &'a Client,
    pub token: Option<String>,
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_insecure_local_and_cross_provider_urls() {
        assert!(safe_url(Provider::Nexus, "http://api.nexusmods.com/v1").is_err());
        assert!(safe_url(Provider::Nexus, "https://localhost/v1").is_err());
        assert!(safe_url(Provider::Nexus, "https://api.gamebanana.com/v1").is_err());
        assert!(safe_url(Provider::Nexus, "https://user:pass@api.nexusmods.com/v1").is_err());
    }

    #[test]
    fn redirect_resolution_is_revalidated() {
        let current = safe_url(Provider::Nexus, "https://api.nexusmods.com/v1/me").unwrap();
        let same_host = current.join("../mods").unwrap();
        assert!(safe_url(Provider::Nexus, same_host.as_str()).is_ok());
        assert!(safe_url(Provider::Nexus, "https://example.invalid/file").is_err());
    }

    #[test]
    fn credentials_are_not_forwarded_to_provider_subdomains() {
        let nexus_api = safe_url(Provider::Nexus, "https://api.nexusmods.com/v1/me").unwrap();
        let nexus_cdn = safe_url(Provider::Nexus, "https://files.nexus-cdn.com/archive").unwrap();
        let gamebanana = safe_url(Provider::GameBanana, "https://gamebanana.com/apiv12").unwrap();
        let gamebanana_image =
            safe_url(Provider::GameBanana, "https://images.gamebanana.com/image").unwrap();
        assert!(Authentication::NexusBearer("secret").allowed_for(&nexus_api));
        assert!(!Authentication::NexusBearer("secret").allowed_for(&nexus_cdn));
        assert!(Authentication::GameBananaCookie("secret").allowed_for(&gamebanana));
        assert!(!Authentication::GameBananaCookie("secret").allowed_for(&gamebanana_image));
    }

    #[test]
    fn provider_identifiers_are_bounded() {
        assert!(valid_nexus_token("A-secure-looking-oauth-token-123456"));
        assert!(!valid_nexus_token("short"));
        assert!(valid_slug("deltarune"));
        assert!(!valid_slug("../deltarune"));
        assert!(validate_model_and_id("Mod", 42).is_ok());
        assert!(validate_model_and_id("../Mod", 42).is_err());
        assert_eq!(
            safe_remote_filename(Some("archive.zip"), "fallback"),
            "archive.zip"
        );
        assert_eq!(
            safe_remote_filename(Some("../archive.zip"), "fallback"),
            "fallback"
        );
    }

    #[test]
    fn quota_headers_are_preserved_in_legacy_shape() {
        let mut h = header::HeaderMap::new();
        h.insert("x-rl-hour-limit", "100".parse().unwrap());
        h.insert("x-rl-hour-remaining", "97".parse().unwrap());
        h.insert("x-rl-hour-reset", "1234".parse().unwrap());
        let q = quota(&h);
        assert_eq!(q["hour"].limit, Some(100));
        assert_eq!(q["hour"].remaining, Some(97));
        assert_eq!(q["hour"].reset_unix, Some(1234));
    }

    #[test]
    fn retry_after_is_bounded_to_numeric_seconds() {
        let mut h = header::HeaderMap::new();
        h.insert(header::RETRY_AFTER, "4".parse().unwrap());
        assert_eq!(retry_after(&h), Some(4000));
        h.insert(
            header::RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&h), None);
    }

    #[test]
    fn moddb_feed_reads_self_closing_media_thumbnails() {
        let feed = r#"<rss xmlns:media="http://search.yahoo.com/mrss/"><channel><item>
            <title>Example mod</title><link>https://www.moddb.com/mods/example</link>
            <media:content url="https://media.moddb.com/images/example.png" />
        </item></channel></rss>"#;

        let entries = parse_moddb_feed(feed).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].image_url.as_deref(),
            Some("https://media.moddb.com/images/example.png")
        );
    }
}
impl<'a> GameBanana<'a> {
    fn credential(&self) -> Result<Authentication<'_>, RuntimeError> {
        self.token
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(Authentication::GameBananaCookie)
            .ok_or_else(|| RuntimeError::Auth("no GameBanana keyring credential".into()))
    }

    pub async fn validate<T: for<'de> Deserialize<'de>>(&self) -> Result<T, RuntimeError> {
        self.client
            .request_json(
                Provider::GameBanana,
                Method::GET,
                "https://gamebanana.com/apiv12/Member/UiConfig?_sUrl=/",
                Some(self.credential()?),
                None,
            )
            .await
    }

    pub async fn comments<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
    ) -> Result<CurrentEnvelope<T>, RuntimeError> {
        Ok(CurrentEnvelope {
            operation_id: "gamebanana-comments".into(),
            value: self
                .client
                .json(Provider::GameBanana, url, self.token.as_deref())
                .await?,
        })
    }
    pub async fn like(&self, url: &str) -> Result<(), RuntimeError> {
        self.like_response(url).await.map(|_| ())
    }

    pub async fn like_response(&self, url: &str) -> Result<ProviderResponse, RuntimeError> {
        let response = self
            .client
            .send_with_error_policy(
                Provider::GameBanana,
                Method::POST,
                url,
                Some(self.credential()?),
                Some(serde_json::json!({})),
                true,
            )
            .await?;
        let status = response.status().as_u16();
        let bytes = read_limited(response, MAX_API_RESPONSE_BYTES).await?;
        let data = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        };
        Ok(ProviderResponse { status, data })
    }

    pub async fn like_target(
        &self,
        model: &str,
        id: u64,
    ) -> Result<ProviderResponse, RuntimeError> {
        validate_model_and_id(model, id)?;
        self.like_response(&format!("https://gamebanana.com/apiv12/{model}/{id}/Like"))
            .await
    }

    pub async fn leave_comment(
        &self,
        id: u64,
        model: &str,
        escaped_html: &str,
    ) -> Result<bool, RuntimeError> {
        validate_model_and_id(model, id)?;
        let url = format!("https://gamebanana.com/apiv12/{model}/{id}/Post/Add");
        self.client
            .send(
                Provider::GameBanana,
                Method::POST,
                &url,
                Some(self.credential()?),
                Some(serde_json::json!({
                    "_aImageFiles": [],
                    "_aImages": [],
                    "_aMentionedMemberRowIds": [],
                    "_sText": format!("<p>{escaped_html}</p>")
                })),
            )
            .await?;
        Ok(true)
    }

    pub async fn create_collection<T: for<'de> Deserialize<'de>>(
        &self,
        name: &str,
    ) -> Result<T, RuntimeError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(RuntimeError::InvalidInput("invalid collection name".into()));
        }
        self.client
            .request_json(
                Provider::GameBanana,
                Method::POST,
                "https://gamebanana.com/apiv12/Collection/Add",
                Some(self.credential()?),
                Some(serde_json::json!({
                    "_bIsPrivate": true,
                    "_sName": name,
                    "_sPassword": "deltamod"
                })),
            )
            .await
    }

    pub async fn add_to_collection<T: for<'de> Deserialize<'de>>(
        &self,
        collection_id: u64,
        item_id: u64,
        model: &str,
    ) -> Result<T, RuntimeError> {
        validate_model_and_id(model, item_id)?;
        if collection_id == 0 {
            return Err(RuntimeError::InvalidInput("invalid collection id".into()));
        }
        let url = format!("https://gamebanana.com/apiv12/{model}/{item_id}/AddToCollection");
        self.client
            .request_json(
                Provider::GameBanana,
                Method::POST,
                &url,
                Some(self.credential()?),
                Some(serde_json::json!({"_idCollectionRow": collection_id})),
            )
            .await
    }

    pub async fn delete_collection<T: for<'de> Deserialize<'de>>(
        &self,
        collection_id: u64,
    ) -> Result<T, RuntimeError> {
        if collection_id == 0 {
            return Err(RuntimeError::InvalidInput("invalid collection id".into()));
        }
        self.client
            .request_json(
                Provider::GameBanana,
                Method::DELETE,
                &format!("https://gamebanana.com/apiv12/Collection/{collection_id}"),
                Some(self.credential()?),
                Some(serde_json::json!({
                    "_idReasonRow": 1,
                    "_sNotes": "<p>Deleted by Deltamod on request of user</p>"
                })),
            )
            .await
    }

    pub async fn delete_collection_status(&self, collection_id: u64) -> Result<bool, RuntimeError> {
        if collection_id == 0 {
            return Err(RuntimeError::InvalidInput("invalid collection id".into()));
        }
        self.client
            .send(
                Provider::GameBanana,
                Method::DELETE,
                &format!("https://gamebanana.com/apiv12/Collection/{collection_id}"),
                Some(self.credential()?),
                Some(serde_json::json!({
                    "_idReasonRow": 1,
                    "_sNotes": "<p>Deleted by Deltamod on request of user</p>"
                })),
            )
            .await?;
        Ok(true)
    }

    pub async fn list_collections<T: for<'de> Deserialize<'de>>(&self) -> Result<T, RuntimeError> {
        self.client
            .request_json(
                Provider::GameBanana,
                Method::GET,
                "https://gamebanana.com/apiv12/Tool/20575/AccessorCollections",
                Some(self.credential()?),
                None,
            )
            .await
    }
    pub async fn user<T: for<'de> Deserialize<'de>>(&self, url: &str) -> Result<T, RuntimeError> {
        self.client
            .json(Provider::GameBanana, url, self.token.as_deref())
            .await
    }
    pub async fn collections<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
    ) -> Result<T, RuntimeError> {
        self.client
            .json(Provider::GameBanana, url, self.token.as_deref())
            .await
    }
}

fn validate_model_and_id(model: &str, id: u64) -> Result<(), RuntimeError> {
    if id == 0
        || model.is_empty()
        || model.len() > 32
        || !model
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !model.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(RuntimeError::InvalidInput(
            "invalid GameBanana target".into(),
        ));
    }
    Ok(())
}
