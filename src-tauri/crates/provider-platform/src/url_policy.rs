use std::fmt;
use url::Url;

const MAX_URL_BYTES: usize = 4096;

fn host_allowed(host: &str, roots: &[&str]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    roots.iter().any(|root| {
        let root = root.to_ascii_lowercase();
        host == root || host.ends_with(&format!(".{root}"))
    })
}

fn parse_https(raw: &str, roots: &[&str], allow_query: bool) -> Result<Url, ()> {
    if raw.is_empty()
        || raw.len() > MAX_URL_BYTES
        || raw.chars().any(char::is_control)
        || (!allow_query && raw.contains('?'))
    {
        return Err(());
    }
    let url = Url::parse(raw).map_err(|_| ())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(());
    }
    let host = url.host_str().ok_or(())?;
    if host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok()
        || !host_allowed(host, roots)
    {
        return Err(());
    }
    Ok(url)
}

/// Produces a credential-free stable public URL. Query and fragment data are intentionally
/// discarded because canonical identities and persisted source metadata must not retain signed
/// parameters or tracking credentials.
pub(crate) fn canonical_public_url(raw: &str, roots: &[&str]) -> Result<String, ()> {
    let mut url = parse_https(raw, roots, true)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

pub(crate) fn canonical_url(raw: &str, roots: &[&str]) -> Result<Url, ()> {
    let mut url = parse_https(raw, roots, true)?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// A short-lived provider download URL. It intentionally implements neither `Serialize` nor
/// `Display`; callers must explicitly opt in with [`EphemeralDownloadUrl::expose`]. Debug output
/// exposes only the validated host and an opaque marker.
#[derive(Clone, Eq, PartialEq)]
pub struct EphemeralDownloadUrl(Url);

impl EphemeralDownloadUrl {
    pub(crate) fn parse(raw: &str, roots: &[&str]) -> Result<Self, ()> {
        let url = parse_https(raw, roots, true)?;
        if url.fragment().is_some() {
            return Err(());
        }
        Ok(Self(url))
    }

    pub(crate) fn from_url(url: Url, roots: &[&str]) -> Result<Self, ()> {
        Self::parse(url.as_str(), roots)
    }

    /// Exposes the URL only to the immediate download adapter. Do not persist or log this value.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    #[must_use]
    pub fn redacted(&self) -> String {
        let host = self
            .0
            .host_str()
            .expect("ephemeral URLs always have a validated host");
        format!("https://{host}/<redacted>")
    }
}

impl fmt::Debug for EphemeralDownloadUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("EphemeralDownloadUrl")
            .field(&self.redacted())
            .finish()
    }
}
