use crate::{ProgressEnvelope, RuntimeError};
use futures_util::StreamExt;
use reqwest::{header, Method, Url};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::{io::AsyncWriteExt, sync::watch, time::sleep};

const MAX_URL_BYTES: usize = 4096;

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

fn valid_operation_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl crate::Client {
    pub async fn download_allowlisted<F>(
        &self,
        operation_id: String,
        source: &str,
        hosts: HostAllowlist,
        policy: DownloadPolicy,
        cancel: &watch::Receiver<bool>,
        mut progress: F,
    ) -> Result<DownloadedFile, RuntimeError>
    where
        F: FnMut(ProgressEnvelope) + Send,
    {
        if !valid_operation_id(&operation_id) || policy.maximum_bytes == 0 {
            return Err(RuntimeError::InvalidInput(
                "invalid download operation or byte limit".into(),
            ));
        }
        if *cancel.borrow() {
            return Err(RuntimeError::Cancelled);
        }

        let permit = self
            .pace
            .acquire()
            .await
            .map_err(|_| RuntimeError::Cancelled)?;
        let mut url = validate_url(source, hosts)?;
        let mut redirects = 0_u8;
        let response = loop {
            if *cancel.borrow() {
                return Err(RuntimeError::Cancelled);
            }
            let response = self.http.request(Method::GET, url.clone()).send().await?;
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
        drop(permit);

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

        let total = response.content_length();
        if total.is_some_and(|bytes| bytes > policy.maximum_bytes) {
            return Err(RuntimeError::TooLarge {
                limit: policy.maximum_bytes,
            });
        }
        let temporary = NamedTempFile::new()?;
        let path = temporary.path().to_path_buf();
        let mut output = tokio::fs::File::from_std(temporary.reopen()?);
        let mut stream = response.bytes_stream();
        let mut completed = 0_u64;
        while let Some(chunk) = stream.next().await {
            if *cancel.borrow() {
                drop(output);
                let _ = std::fs::remove_file(&path);
                return Err(RuntimeError::Cancelled);
            }
            let chunk = chunk?;
            completed =
                completed
                    .checked_add(chunk.len() as u64)
                    .ok_or(RuntimeError::TooLarge {
                        limit: policy.maximum_bytes,
                    })?;
            if completed > policy.maximum_bytes {
                drop(output);
                let _ = std::fs::remove_file(&path);
                return Err(RuntimeError::TooLarge {
                    limit: policy.maximum_bytes,
                });
            }
            output.write_all(&chunk).await?;
            progress(ProgressEnvelope {
                operation_id: operation_id.clone(),
                completed,
                total,
                current_item: Some(url.to_string()),
            });
            if !self.min_interval.is_zero() {
                sleep(self.min_interval).await;
            }
        }
        output.flush().await?;
        drop(output);
        temporary.keep().map_err(|error| error.error)?;
        Ok(DownloadedFile {
            path,
            bytes: completed,
            total,
            final_url: url.to_string(),
        })
    }
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
}
