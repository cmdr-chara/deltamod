use crate::{ProgressEnvelope, RuntimeError};
use futures_util::StreamExt;
use reqwest::{header, Method, Url};
use std::{
    future::Future,
    path::PathBuf,
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio::{io::AsyncWriteExt, sync::watch};

const MAX_URL_BYTES: usize = 4096;
// API responses have short deadlines. A game/mod transfer may legitimately take
// longer, but still has a whole-operation deadline and an idle-read deadline.
pub(super) const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const TRANSFER_STALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
pub struct DownloadPolicy {
    pub maximum_bytes: u64,
    pub maximum_redirects: u8,
}

impl DownloadPolicy {
    pub const fn mods() -> Self {
        Self {
            maximum_bytes: 2 * 1024 * 1024 * 1024,
            maximum_redirects: 5,
        }
    }

    pub const fn games() -> Self {
        Self {
            maximum_bytes: 8 * 1024 * 1024 * 1024,
            maximum_redirects: 5,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HostAllowlist(&'static [&'static str]);

impl HostAllowlist {
    pub const GAMEBANANA: Self = Self(&["gamebanana.com"]);
    pub const NEXUS: Self = Self(&["nexusmods.com", "nexus-cdn.com"]);
    pub const GAME_DOWNLOADS: Self = Self(&[
        "itch.io",
        "hwcdn.net",
        "gamejolt.com",
        "gamejolt.net",
        "gjcdn.net",
    ]);

    pub const fn new(roots: &'static [&'static str]) -> Self {
        Self(roots)
    }

    fn permits(self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.0.iter().any(|root| {
            let root = root.to_ascii_lowercase();
            host == root || host.ends_with(&format!(".{root}"))
        })
    }
}

#[derive(Debug)]
pub struct DownloadedFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub total: Option<u64>,
    pub final_url: String,
}

impl Drop for DownloadedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn validate_url(raw: &str, hosts: HostAllowlist) -> Result<Url, RuntimeError> {
    if raw.is_empty()
        || raw.len() > MAX_URL_BYTES
        || raw.chars().any(char::is_control)
        || raw.contains('#')
    {
        return Err(RuntimeError::Url("invalid download URL".into()));
    }
    let url = Url::parse(raw).map_err(|error| RuntimeError::Url(error.to_string()))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(RuntimeError::Url(
            "HTTPS URL without credentials or an explicit port required".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| RuntimeError::Url("missing host".into()))?;
    let normalized = host.trim_end_matches('.');
    if normalized.eq_ignore_ascii_case("localhost")
        || normalized.eq_ignore_ascii_case("localhost.localdomain")
        || normalized.parse::<std::net::IpAddr>().is_ok()
        || !hosts.permits(host)
    {
        return Err(RuntimeError::Url(format!("host not allowed: {host}")));
    }
    Ok(url)
}

pub fn validate_download_url(raw: &str, hosts: HostAllowlist) -> Result<(), RuntimeError> {
    validate_url(raw, hosts).map(|_| ())
}

pub(super) fn valid_operation_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// A closed channel is a non-cancellable caller, not a cancellation request.
/// Keep the boolean contract while waking immediately during queued/idle I/O.
pub(super) async fn cancellable<T>(
    cancel: &watch::Receiver<bool>,
    work: impl Future<Output = Result<T, RuntimeError>>,
) -> Result<T, RuntimeError> {
    let mut changed = cancel.clone();
    let cancelled = async {
        loop {
            if *changed.borrow_and_update() {
                return;
            }
            if changed.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Err(RuntimeError::Cancelled),
        result = work => result,
    }
}

fn transfer_request(client: &reqwest::Client, url: Url) -> reqwest::RequestBuilder {
    client
        .request(Method::GET, url)
        .header(header::ACCEPT_ENCODING, "identity")
        .timeout(TRANSFER_TIMEOUT)
}

impl crate::Client {
    pub async fn download_allowlisted<F>(
        &self,
        operation_id: String,
        source: &str,
        hosts: HostAllowlist,
        policy: DownloadPolicy,
        cancel: &watch::Receiver<bool>,
        progress: F,
    ) -> Result<DownloadedFile, RuntimeError>
    where
        F: FnMut(ProgressEnvelope) + Send,
    {
        if !valid_operation_id(&operation_id) || policy.maximum_bytes == 0 {
            return Err(RuntimeError::InvalidInput(
                "invalid download operation or byte limit".into(),
            ));
        }
        let mut url = validate_url(source, hosts)?;
        cancellable(cancel, async {
            tokio::time::timeout(TRANSFER_TIMEOUT, async {
                // Keep the permit through the body and disk writes. Releasing it
                // at the headers allows unlimited simultaneous large downloads.
                let _permit = self
                    .transfers
                    .acquire()
                    .await
                    .map_err(|_| RuntimeError::Cancelled)?;
                let mut redirects = 0_u8;
                let response = loop {
                    let response = tokio::time::timeout(
                        TRANSFER_STALL_TIMEOUT,
                        transfer_request(&self.http, url.clone()).send(),
                    )
                    .await
                    .map_err(|_| RuntimeError::DownloadTimeout)??;
                    if !response.status().is_redirection() {
                        break response;
                    }
                    if redirects >= policy.maximum_redirects {
                        return Err(RuntimeError::Url("redirect limit exceeded".into()));
                    }
                    let location = response
                        .headers()
                        .get(header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or_else(|| RuntimeError::Url("redirect without Location".into()))?;
                    let next = url
                        .join(location)
                        .map_err(|error| RuntimeError::Url(error.to_string()))?;
                    url = validate_url(next.as_str(), hosts)?;
                    redirects += 1;
                };
                receive_download(
                    response,
                    operation_id,
                    url,
                    policy.maximum_bytes,
                    self.min_interval,
                    progress,
                )
                .await
            })
            .await
            .map_err(|_| RuntimeError::DownloadTimeout)?
        })
        .await
    }
}

pub(super) async fn receive_download<F: FnMut(ProgressEnvelope)>(
    response: reqwest::Response,
    operation_id: String,
    url: Url,
    maximum_bytes: u64,
    progress_interval: Duration,
    mut progress: F,
) -> Result<DownloadedFile, RuntimeError> {
    if !response.status().is_success() {
        let status = response.status();
        return Err(RuntimeError::Http {
            status: status.as_u16(),
            message: status
                .canonical_reason()
                .unwrap_or("download failed")
                .to_owned(),
            envelope: Box::new(crate::ErrorEnvelope {
                operation_id: Some(operation_id),
                code: format!("HTTP_{}", status.as_u16()),
                message: status
                    .canonical_reason()
                    .unwrap_or("download failed")
                    .to_owned(),
                status: Some(status.as_u16()),
                retry_after_ms: None,
                quota: Default::default(),
            }),
        });
    }
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|value| value != "identity")
    {
        return Err(RuntimeError::IncompleteDownload);
    }
    let mut lengths = response.headers().get_all(header::CONTENT_LENGTH).iter();
    let declared = lengths
        .next()
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or(RuntimeError::IncompleteDownload)
        })
        .transpose()?;
    if lengths.next().is_some() {
        return Err(RuntimeError::IncompleteDownload);
    }
    let total = declared.or_else(|| response.content_length());
    if total.is_some_and(|bytes| bytes > maximum_bytes) {
        return Err(RuntimeError::TooLarge {
            limit: maximum_bytes,
        });
    }
    let temporary = NamedTempFile::new()?;
    let path = temporary.path().to_path_buf();
    let mut output = tokio::fs::File::from_std(temporary.reopen()?);
    let mut stream = response.bytes_stream();
    let mut completed = 0_u64;
    let mut last_progress = None::<Instant>;
    loop {
        let chunk = tokio::time::timeout(TRANSFER_STALL_TIMEOUT, stream.next())
            .await
            .map_err(|_| RuntimeError::DownloadTimeout)?;
        let Some(chunk) = chunk else {
            break;
        };
        let chunk = chunk?;
        completed = completed
            .checked_add(chunk.len() as u64)
            .filter(|bytes| *bytes <= maximum_bytes)
            .ok_or(RuntimeError::TooLarge {
                limit: maximum_bytes,
            })?;
        output.write_all(&chunk).await?;
        if last_progress.is_none_or(|last| last.elapsed() >= progress_interval) {
            progress(ProgressEnvelope {
                operation_id: operation_id.clone(),
                completed,
                total,
                // Signed download query parameters must never become progress/log text.
                current_item: url.host_str().map(str::to_owned),
            });
            last_progress = Some(Instant::now());
        }
        // Rate-limit UI notifications, never network chunks. A 100 ms sleep per
        // 16 KiB TLS record previously capped transfers near 160 KiB/s.
    }
    if completed == 0 || total.is_some_and(|expected| expected != completed) {
        return Err(RuntimeError::IncompleteDownload);
    }
    output.flush().await?;
    output.sync_all().await?;
    drop(output);
    progress(ProgressEnvelope {
        operation_id,
        completed,
        total,
        current_item: url.host_str().map(str::to_owned),
    });
    temporary.keep().map_err(|error| error.error)?;
    Ok(DownloadedFile {
        path,
        bytes: completed,
        total,
        final_url: url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_hosts_match_legacy_allowlist() {
        assert!(validate_url("https://gamebanana.com/dl/1", HostAllowlist::GAMEBANANA).is_ok());
        assert!(validate_url(
            "https://files.gamebanana.com/dl/1",
            HostAllowlist::GAMEBANANA
        )
        .is_ok());
        assert!(validate_url(
            "https://gamebanana.com.evil.test/dl/1",
            HostAllowlist::GAMEBANANA
        )
        .is_err());
    }

    #[test]
    fn game_hosts_and_url_shape_are_strict() {
        for host in [
            "itch.io",
            "cdn.hwcdn.net",
            "gamejolt.com",
            "download.gamejolt.net",
            "cdn.gjcdn.net",
        ] {
            assert!(validate_url(
                &format!("https://{host}/archive.zip"),
                HostAllowlist::GAME_DOWNLOADS
            )
            .is_ok());
        }
        for url in [
            "http://itch.io/archive.zip",
            "https://user:pass@itch.io/archive.zip",
            "https://itch.io:444/archive.zip",
            "https://127.0.0.1/archive.zip",
            "https://itch.io/archive.zip#fragment",
        ] {
            assert!(validate_url(url, HostAllowlist::GAME_DOWNLOADS).is_err());
        }
    }

    #[test]
    fn nexus_download_hosts_reject_suffix_lookalikes() {
        assert!(validate_url(
            "https://cf-files.nexusmods.com/archive.zip",
            HostAllowlist::NEXUS
        )
        .is_ok());
        assert!(validate_url(
            "https://cdn.nexus-cdn.com/archive.zip",
            HostAllowlist::NEXUS
        )
        .is_ok());
        assert!(validate_url(
            "https://nexusmods.com.evil.test/archive.zip",
            HostAllowlist::NEXUS
        )
        .is_err());
    }

    #[test]
    fn policies_preserve_legacy_limits() {
        assert_eq!(DownloadPolicy::mods().maximum_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(
            DownloadPolicy::games().maximum_bytes,
            8 * 1024 * 1024 * 1024
        );
        assert_eq!(DownloadPolicy::games().maximum_redirects, 5);
    }

    #[tokio::test]
    async fn cancellation_is_checked_before_network_io() {
        let client = crate::Client::new(
            std::time::Duration::from_secs(1),
            1,
            std::time::Duration::ZERO,
        )
        .unwrap();
        let (sender, receiver) = watch::channel(true);
        let result = client
            .download_allowlisted(
                "cancelled-1".into(),
                "https://gamebanana.com/file.zip",
                HostAllowlist::GAMEBANANA,
                DownloadPolicy::mods(),
                &receiver,
                |_| {},
            )
            .await;
        drop(sender);
        assert!(matches!(result, Err(RuntimeError::Cancelled)));
    }

    #[test]
    fn runtime_operation_ids_are_bounded() {
        assert!(valid_operation_id("download_01-test"));
        assert!(!valid_operation_id("../download"));
        assert!(!valid_operation_id(&"a".repeat(129)));
    }

    #[test]
    fn downloaded_file_owns_temporary_path_cleanup() {
        let temporary = tempfile::NamedTempFile::new().unwrap();
        let path = temporary.path().to_path_buf();
        temporary.keep().unwrap();
        let downloaded = DownloadedFile {
            path: path.clone(),
            bytes: 0,
            total: None,
            final_url: "https://gamebanana.com/file.zip".into(),
        };
        assert!(path.is_file());
        drop(downloaded);
        assert!(!path.exists());
    }
    fn response(chunks: usize, declared: Option<u64>) -> reqwest::Response {
        let stream = futures_util::stream::iter(
            (0..chunks).map(|_| Ok::<_, std::io::Error>(vec![42_u8; 1024])),
        );
        let mut response = hyper::Response::builder();
        if let Some(size) = declared {
            response = response.header(header::CONTENT_LENGTH, size);
        }
        response
            .body(reqwest::Body::wrap_stream(stream))
            .unwrap()
            .into()
    }
    fn test_url() -> Url {
        Url::parse("https://files.gamebanana.com/mod.zip?secret=must-not-be-logged").unwrap()
    }
    #[tokio::test]
    async fn transfer_notifications_are_throttled_without_throttling_chunks() {
        let mut events = Vec::new();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            receive_download(
                response(128, Some(128 * 1024)),
                "mod-1".into(),
                test_url(),
                1024 * 1024,
                Duration::from_secs(60),
                |event| events.push(event),
            ),
        )
        .await
        .expect("progress interval must not sleep between chunks")
        .unwrap();
        assert_eq!(result.bytes, 128 * 1024);
        assert_eq!(
            std::fs::read(&result.path).unwrap(),
            vec![42_u8; 128 * 1024]
        );
        assert_eq!(events.len(), 2);
        assert_eq!(events.last().unwrap().completed, result.bytes);
        assert!(events
            .iter()
            .all(|event| event.current_item.as_deref() == Some("files.gamebanana.com")));
        let path = result.path.clone();
        drop(result);
        assert!(!path.exists());
    }
    #[tokio::test]
    async fn transfers_reject_empty_truncated_and_over_limit_bodies() {
        for (chunks, declared, limit) in
            [(0, Some(0), 4096), (1, Some(2048), 4096), (2, None, 1024)]
        {
            assert!(receive_download(
                response(chunks, declared),
                "mod-1".into(),
                test_url(),
                limit,
                Duration::ZERO,
                |_| {}
            )
            .await
            .is_err());
        }
    }
    #[tokio::test]
    async fn cancellation_wakes_queued_transfers_without_sending_a_request() {
        let client = crate::Client::new(Duration::from_millis(20), 1, Duration::ZERO).unwrap();
        let held = client.transfers.acquire().await.unwrap();
        let (sender, receiver) = watch::channel(false);
        let cancel = async {
            tokio::task::yield_now().await;
            sender.send(true).unwrap();
        };
        let work = client.download_allowlisted(
            "queued".into(),
            "https://gamebanana.com/dl/1",
            HostAllowlist::GAMEBANANA,
            DownloadPolicy::mods(),
            &receiver,
            |_| panic!("queued transfer progressed"),
        );
        let (result, _) =
            tokio::time::timeout(Duration::from_secs(1), async { tokio::join!(work, cancel) })
                .await
                .unwrap();
        assert!(matches!(result, Err(RuntimeError::Cancelled)));
        drop(held);
        assert_eq!(client.transfers.available_permits(), 1);
    }
    #[tokio::test]
    async fn cancellation_interrupts_pending_io_and_closed_channels_are_not_cancellation() {
        let (sender, receiver) = watch::channel(false);
        let pending = cancellable(
            &receiver,
            std::future::pending::<Result<(), RuntimeError>>(),
        );
        let cancel = async {
            tokio::task::yield_now().await;
            sender.send(true).unwrap();
        };
        let (result, _) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(pending, cancel)
        })
        .await
        .unwrap();
        assert!(matches!(result, Err(RuntimeError::Cancelled)));
        let (sender, receiver) = watch::channel(false);
        drop(sender);
        assert_eq!(cancellable(&receiver, async { Ok(7) }).await.unwrap(), 7);
    }
    #[test]
    fn transfer_request_has_its_own_bounded_deadline() {
        let client =
            crate::Client::new(Duration::from_secs(20), 2, Duration::from_millis(100)).unwrap();
        let request = transfer_request(&client.http, test_url()).build().unwrap();
        assert_eq!(request.timeout().copied(), Some(TRANSFER_TIMEOUT));
        assert!(TRANSFER_TIMEOUT > Duration::from_secs(20));
    }
    #[test]
    fn oversized_retry_after_is_rejected_without_overflow() {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::RETRY_AFTER, u64::MAX.to_string().parse().unwrap());
        assert_eq!(crate::retry_after(&headers), None);
        headers.insert(header::RETRY_AFTER, "10".parse().unwrap());
        assert_eq!(crate::retry_after(&headers), Some(10_000));
    }
}
