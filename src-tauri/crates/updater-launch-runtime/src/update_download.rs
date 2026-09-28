// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
//! Bounded update transport and publisher verification. `Update::download` in
//! the pinned plugin cannot stop when a progress callback rejects a chunk, so
//! transport is owned here. No bytes reach its installer without verification.
use crate::{UpdateControl, UpdateError};
use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};
use reqwest::{redirect::Policy, Client, Response, Url};
use std::{future::Future, time::Duration};

const MAX_SIGNATURE_TEXT: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

pub struct VerifiedDownload(Vec<u8>);
impl VerifiedDownload {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

fn signature_error() -> UpdateError {
    // Do not put attacker-controlled signature/key/URL content in logs or UI.
    UpdateError::SignatureVerification("publisher signature is invalid".into())
}

struct PublisherVerifier {
    key: PublicKey,
    signature: Signature,
}
impl PublisherVerifier {
    fn parse(public_key: &str, signature: &str) -> Result<Self, UpdateError> {
        fn decode(value: &str) -> Result<String, UpdateError> {
            if value.is_empty() || value.len() > MAX_SIGNATURE_TEXT {
                return Err(signature_error());
            }
            String::from_utf8(STANDARD.decode(value).map_err(|_| signature_error())?)
                .map_err(|_| signature_error())
        }
        Ok(Self {
            key: PublicKey::decode(&decode(public_key)?).map_err(|_| signature_error())?,
            signature: Signature::decode(&decode(signature)?).map_err(|_| signature_error())?,
        })
    }
    fn verify(&self, bytes: Vec<u8>) -> Result<VerifiedDownload, UpdateError> {
        // Maintain the pinned plugin's legacy-signature compatibility, including
        // key ID and authenticated-comment checks performed by minisign-verify.
        self.key
            .verify(&bytes, &self.signature, true)
            .map_err(|_| signature_error())?;
        Ok(VerifiedDownload(bytes))
    }
}

fn allowed_host(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
                    | "github-releases.githubusercontent.com"
            )
        )
}
fn validate_release_url(url: &Url) -> Result<(), UpdateError> {
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    if !allowed_host(url)
        || url.host_str() != Some("github.com")
        || url.query().is_some()
        || parts.len() != 6
        || parts[..4] != ["cmdr-chara", "deltamod", "releases", "download"]
        || parts[4..].iter().any(|part| {
            part.is_empty() || part.contains('\\') || part.replace("%20", " ").contains('%')
        })
    {
        return Err(UpdateError::InvalidMetadata("artifact URL"));
    }
    Ok(())
}

fn production_client() -> Result<Client, UpdateError> {
    Client::builder()
        .https_only(true)
        .user_agent("Deltamod-Community-Updater")
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(STALL_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        // Feature unification must not silently enable decompression around the byte budget.
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() > 5 || !allowed_host(attempt.url()) {
                attempt.error("updater redirect rejected")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|_| UpdateError::Adapter("update client unavailable".into()))
}

async fn cancellable<T>(
    control: &UpdateControl,
    work: impl Future<Output = Result<T, UpdateError>>,
) -> Result<T, UpdateError> {
    // A cancellation handle is usable before response headers and while the peer
    // stalls between chunks. Dropping work also drops its connection and buffer.
    let cancelled = async {
        loop {
            if control.is_cancelled() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    };
    tokio::select! {
        biased;
        _ = cancelled => Err(UpdateError::Cancelled),
        result = work => result,
    }
}

async fn receive_response(
    mut response: Response,
    verifier: &PublisherVerifier,
    limit: u64,
    control: &UpdateControl,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
) -> Result<VerifiedDownload, UpdateError> {
    if !response.status().is_success() {
        return Err(UpdateError::Adapter(format!(
            "update HTTP status {}",
            response.status().as_u16()
        )));
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|value| value != "identity")
    {
        return Err(UpdateError::InvalidMetadata("content encoding"));
    }
    let total = response.content_length();
    if total.is_some_and(|bytes| bytes > limit) {
        return Err(UpdateError::ArtifactTooLarge { limit });
    }
    let mut bytes = Vec::new();
    loop {
        if control.is_cancelled() {
            return Err(UpdateError::Cancelled);
        }
        let chunk = response
            .chunk()
            .await
            .map_err(|_| UpdateError::Adapter("update transfer failed".into()))?;
        let Some(chunk) = chunk else {
            break;
        };
        let length = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or(UpdateError::ArtifactTooLarge { limit })?;
        if length as u64 > limit {
            return Err(UpdateError::ArtifactTooLarge { limit });
        }
        progress(chunk.len() as u64, total)?;
        // Reserve only the bytes actually received, never the untrusted header.
        // A fallible capped growth strategy avoids both per-chunk reallocations
        // and Vec's usual doubling beyond the configured budget.
        if length > bytes.capacity() {
            let capacity = length
                .max(bytes.capacity().saturating_mul(2).max(64 * 1024))
                .min(usize::try_from(limit).unwrap_or(usize::MAX));
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| UpdateError::Adapter("insufficient memory for update".into()))?;
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() || total.is_some_and(|total| total != bytes.len() as u64) {
        return Err(UpdateError::InvalidMetadata("artifact length"));
    }
    if control.is_cancelled() {
        return Err(UpdateError::Cancelled);
    }
    let artifact = verifier.verify(bytes)?;
    if control.is_cancelled() {
        return Err(UpdateError::Cancelled);
    }
    Ok(artifact)
}

pub async fn download_verified(
    url: &Url,
    public_key: &str,
    signature: &str,
    limit: u64,
    control: &UpdateControl,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<(), UpdateError>,
) -> Result<VerifiedDownload, UpdateError> {
    if limit == 0 {
        return Err(UpdateError::InvalidLimit);
    }
    validate_release_url(url)?;
    let verifier = PublisherVerifier::parse(public_key, signature)?;
    let client = production_client()?;
    cancellable(control, async {
        let response = client
            .get(url.clone())
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| UpdateError::Adapter("update request failed".into()))?;
        receive_response(response, &verifier, limit, control, progress).await
    })
    .await
}

#[cfg(test)]
mod tests;
