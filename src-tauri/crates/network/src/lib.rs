#![forbid(unsafe_code)]
#![warn(clippy::all)]

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    GameBanana,
    Nexus,
    ModDb,
}

pub const GAMEBANANA_API_HOST: &str = "gamebanana.com";
pub const NEXUS_API_HOST: &str = "api.nexusmods.com";
pub const MODDB_API_HOST: &str = "www.moddb.com";

#[must_use]
pub fn is_exact_api_host(provider: Provider, host: &str) -> bool {
    let normalized = host.to_ascii_lowercase();
    match provider {
        Provider::GameBanana => normalized == GAMEBANANA_API_HOST,
        Provider::Nexus => normalized == NEXUS_API_HOST,
        Provider::ModDb => normalized == MODDB_API_HOST,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeUrl {
    pub raw: String,
    pub host: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UrlError {
    Invalid,
    NotHttps,
    Credentials,
    HostNotAllowed,
    LocalAddress,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for UrlError {}

fn host_allowed(provider: Provider, host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    let sub = |root: &str| h == root || h.ends_with(&format!(".{root}"));
    match provider {
        Provider::GameBanana => sub("gamebanana.com"),
        Provider::ModDb => sub("moddb.com"),
        Provider::Nexus => sub("nexusmods.com") || sub("nexus-cdn.com"),
    }
}

fn is_ip_or_local(host: &str) -> bool {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == "localhost"
        || h == "localhost.localdomain"
        || h == "[::1]"
        || h.parse::<std::net::Ipv4Addr>().is_ok()
        || h.parse::<std::net::Ipv6Addr>().is_ok()
}

pub fn validate_https_url(provider: Provider, value: &str) -> Result<SafeUrl, UrlError> {
    let (scheme, remainder) = value.split_once("://").ok_or(UrlError::Invalid)?;
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(UrlError::NotHttps);
    }
    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return Err(if authority.contains('@') {
            UrlError::Credentials
        } else {
            UrlError::Invalid
        });
    }
    let host = authority
        .split(':')
        .next()
        .filter(|h| !h.is_empty())
        .ok_or(UrlError::Invalid)?
        .to_ascii_lowercase();
    if is_ip_or_local(&host) {
        return Err(UrlError::LocalAddress);
    }
    if !host_allowed(provider, &host) || host.ends_with('.') {
        return Err(UrlError::HostNotAllowed);
    }
    Ok(SafeUrl {
        raw: value.to_owned(),
        host,
    })
}

pub fn validate_redirect(
    provider: Provider,
    current: &SafeUrl,
    location: &str,
) -> Result<SafeUrl, UrlError> {
    if location.contains("://") {
        return validate_https_url(provider, location);
    }
    let base = current
        .raw
        .split_once("://")
        .map_or(current.raw.as_str(), |(_, x)| x);
    let authority_end = base.find(['/', '?', '#']).unwrap_or(base.len());
    let authority = &base[..authority_end];
    let path = if location.starts_with('/') {
        location.to_owned()
    } else {
        let parent = current
            .raw
            .rsplit_once('/')
            .map_or(current.raw.as_str(), |(p, _)| p);
        format!("{parent}/{location}")
    };
    validate_https_url(provider, &format!("https://{authority}{path}"))
}

pub const MAX_BROWSE_PAGE: u32 = 50;
pub const MAX_QUERY_LENGTH: usize = 256;
pub const MAX_REDIRECTS: u8 = 5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseRequest {
    pub provider: Provider,
    pub domain: Option<String>,
    pub query: Option<String>,
    pub offset: u32,
    pub count: u32,
}
impl BrowseRequest {
    pub fn new(
        provider: Provider,
        domain: Option<String>,
        query: Option<String>,
        offset: u32,
        count: u32,
    ) -> Result<Self, &'static str> {
        if count == 0 || count > MAX_BROWSE_PAGE {
            return Err("count out of bounds");
        }
        if offset > 1_000_000 {
            return Err("offset out of bounds");
        }
        if domain
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 80)
            || query.as_ref().is_some_and(|s| s.len() > MAX_QUERY_LENGTH)
        {
            return Err("field out of bounds");
        }
        Ok(Self {
            provider,
            domain,
            query,
            offset,
            count,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRequest {
    pub operation_id: String,
    pub source: SafeUrl,
    pub maximum_bytes: u64,
    pub maximum_redirects: u8,
}
impl DownloadRequest {
    pub fn new(
        operation_id: String,
        source: SafeUrl,
        maximum_bytes: u64,
    ) -> Result<Self, &'static str> {
        if !valid_operation_id(&operation_id) || maximum_bytes == 0 {
            return Err("invalid download request");
        }
        Ok(Self {
            operation_id,
            source,
            maximum_bytes,
            maximum_redirects: MAX_REDIRECTS,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentEnvelope<T> {
    pub operation_id: String,
    pub value: T,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorEnvelope {
    pub operation_id: Option<String>,
    pub code: String,
    pub message: String,
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
    pub quota: BTreeMap<String, QuotaWindow>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressEnvelope {
    pub operation_id: String,
    pub completed: u64,
    pub total: Option<u64>,
    pub current_item: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommentTarget {
    pub id: u64,
    pub model: String,
}
pub const MAX_COMMENT_LENGTH: usize = 10_000;
pub fn normalize_comment_target(id: &str, model: &str) -> Result<CommentTarget, &'static str> {
    let id = id.parse::<u64>().map_err(|_| "invalid item id")?;
    if id == 0
        || model.is_empty()
        || model.len() > 32
        || !model
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        || !model.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err("invalid target");
    }
    Ok(CommentTarget {
        id,
        model: model.to_owned(),
    })
}
pub fn normalize_comment(value: &str) -> Result<String, &'static str> {
    let text = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned();
    if text.is_empty() {
        return Err("empty comment");
    }
    if text.chars().count() > MAX_COMMENT_LENGTH
        || text.chars().any(|c| {
            (c as u32 <= 8)
                || (11..=12).contains(&(c as u32))
                || (14..=31).contains(&(c as u32))
                || c == '\u{7f}'
        })
    {
        return Err("invalid comment");
    }
    Ok(text)
}
pub fn escape_comment_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('\n', "<br>")
}

pub const SSO_ENDPOINT: &str = "wss://sso.nexusmods.com";
pub const SSO_PAGE: &str = "https://www.nexusmods.com/sso";
pub fn parse_sso_app_id(value: &str) -> Option<String> {
    let s = value.trim();
    if (2..=80).contains(&s.len())
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(s.to_owned())
    } else {
        None
    }
}
pub fn parse_sso_message(value: &str) -> Result<String, &'static str> {
    let mut s = value.trim();
    if s.is_empty() || s.len() > 4096 {
        return Err("invalid API credential");
    }
    if s.starts_with('{') {
        if s.contains("\"success\":false") || s.contains("\"success\": false") {
            return Err("Nexus Mods rejected the sign-in request");
        }
        let marker = "\"api_key\"";
        let start = s.find(marker).ok_or("invalid API credential")? + marker.len();
        s = s[start..].trim_start_matches([' ', ':', '"']);
        s = s.split('"').next().ok_or("invalid API credential")?;
    }
    if s.len() < 20
        || s.len() > 200
        || !s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"+/=_-".contains(&c))
    {
        return Err("invalid API credential");
    }
    Ok(s.to_owned())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SsoState {
    Idle,
    Connecting,
    AwaitingAuthorization,
    Completed,
    Cancelled,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SsoEvent {
    Start,
    SocketOpened,
    CredentialReceived,
    Cancel,
    Timeout,
    ConnectionFailed,
}
pub fn sso_transition(state: SsoState, event: SsoEvent) -> Result<SsoState, &'static str> {
    match (state, event) {
        (SsoState::Idle, SsoEvent::Start) => Ok(SsoState::Connecting),
        (SsoState::Connecting, SsoEvent::SocketOpened) => Ok(SsoState::AwaitingAuthorization),
        (SsoState::AwaitingAuthorization, SsoEvent::CredentialReceived) => Ok(SsoState::Completed),
        (SsoState::Connecting | SsoState::AwaitingAuthorization, SsoEvent::Cancel) => {
            Ok(SsoState::Cancelled)
        }
        (
            SsoState::Connecting | SsoState::AwaitingAuthorization,
            SsoEvent::Timeout | SsoEvent::ConnectionFailed,
        ) => Ok(SsoState::Failed),
        _ => Err("invalid SSO transition"),
    }
}

pub trait HttpsTransport {
    type Error;
}
pub trait WebSocketTransport {
    type Error;
}
pub trait SecretStore {
    type Error;
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, Self::Error>;
    fn put(&self, name: &str, value: &[u8]) -> Result<(), Self::Error>;
    fn delete(&self, name: &str) -> Result<(), Self::Error>;
}
pub fn filter_secret_environment<'a, I>(pairs: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    pairs
        .into_iter()
        .filter(|(k, _)| {
            matches!(
                *k,
                "DELTAMOD_NEXUS_SSO_APP_ID" | "DELTAMOD_NETWORK_TIMEOUT_MS"
            )
        })
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaWindow {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset_unix: Option<i64>,
}

pub fn parse_quota_headers<'a, I>(headers: I) -> BTreeMap<String, QuotaWindow>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for (name, value) in headers {
        let lower = name.to_ascii_lowercase();
        let (period, field) = if let Some(period) = lower.strip_prefix("x-rl-") {
            if let Some((period, field)) = period.split_once('-') {
                (period.to_owned(), field.to_owned())
            } else {
                continue;
            }
        } else {
            continue;
        };
        if let Ok(number) = value.trim().parse::<u64>() {
            values.entry(period).or_default().insert(field, number);
        }
    }
    values
        .into_iter()
        .map(|(period, fields)| {
            (
                period,
                QuotaWindow {
                    limit: fields.get("limit").copied(),
                    remaining: fields.get("remaining").copied(),
                    reset_unix: fields.get("reset").copied().map(|v| v as i64),
                },
            )
        })
        .collect()
}
pub fn parse_retry_after(value: &str, now_unix: i64) -> Option<Duration> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    parse_http_date(value).map(|at| Duration::from_secs(at.saturating_sub(now_unix) as u64))
}
pub fn pause_for_retry(retry_after: Option<Duration>, quota_exhausted: bool) -> Duration {
    retry_after.unwrap_or_else(|| {
        if quota_exhausted {
            Duration::from_secs(60 * 60)
        } else {
            Duration::from_secs(60)
        }
    })
}
fn parse_http_date(value: &str) -> Option<i64> {
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.len() != 6 {
        return None;
    }
    let day = parts[1].parse::<u32>().ok()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[2])? as u32
        + 1;
    let year = parts[3].parse::<i64>().ok()?;
    let time_parts: Vec<_> = parts[4].split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let (hour, minute, second) = (
        time_parts[0].parse::<i64>().ok()?,
        time_parts[1].parse::<i64>().ok()?,
        time_parts[2].parse::<i64>().ok()?,
    );
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from(m) + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn valid_operation_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SizeAccounting {
    pub received: u64,
    pub maximum: u64,
}
impl SizeAccounting {
    pub fn new(maximum: u64) -> Result<Self, &'static str> {
        if maximum == 0 {
            Err("maximum must be nonzero")
        } else {
            Ok(Self {
                received: 0,
                maximum,
            })
        }
    }
    pub fn accept(&mut self, bytes: usize) -> Result<u64, &'static str> {
        self.received = self
            .received
            .checked_add(bytes as u64)
            .ok_or("size overflow")?;
        if self.received > self.maximum {
            return Err("download too large");
        }
        Ok(self.received)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urls_are_strict() {
        for (url, ok) in [
            ("https://gamebanana.com/a", true),
            ("https://images.gamebanana.com/a", true),
            ("https://gamebanana.com.evil/x", false),
            ("https://127.0.0.1/x", false),
            ("https://localhost/x", false),
            ("https://u:p@gamebanana.com/x", false),
            ("http://gamebanana.com/x", false),
        ] {
            assert_eq!(validate_https_url(Provider::GameBanana, url).is_ok(), ok);
        }
        assert!(validate_https_url(Provider::Nexus, "https://cdn.nexus-cdn.com/a").is_ok());
        assert!(validate_https_url(Provider::Nexus, "https://nexusmods.com.evil/a").is_err());
        assert!(is_exact_api_host(Provider::Nexus, "api.nexusmods.com"));
        assert!(!is_exact_api_host(Provider::Nexus, "x.api.nexusmods.com"));
    }
    #[test]
    fn redirects_remain_contained() {
        let start =
            validate_https_url(Provider::GameBanana, "https://gamebanana.com/a/file").unwrap();
        assert!(validate_redirect(Provider::GameBanana, &start, "/next").is_ok());
        assert!(validate_redirect(Provider::GameBanana, &start, "https://evil.test/x").is_err());
    }
    #[test]
    fn bounded_requests_and_ids() {
        assert!(BrowseRequest::new(Provider::Nexus, None, None, 0, 50).is_ok());
        assert!(BrowseRequest::new(Provider::Nexus, None, None, 0, 51).is_err());
        assert!(valid_operation_id("abc-01"));
        assert!(!valid_operation_id("../x"));
    }
    #[test]
    fn comments_are_normalized_and_escaped() {
        assert_eq!(normalize_comment(" a\r\nb ").unwrap(), "a\nb");
        assert_eq!(
            escape_comment_html("<x> &\n\""),
            "&lt;x&gt; &amp;<br>&quot;"
        );
        assert!(normalize_comment_target("698242", "Mod").is_ok());
        assert!(normalize_comment_target("../1", "Mod").is_err());
    }
    #[test]
    fn sso_parsing_and_state_machine() {
        assert_eq!(
            parse_sso_app_id("deltamod-community").as_deref(),
            Some("deltamod-community")
        );
        assert!(parse_sso_app_id("bad slug/x").is_none());
        assert!(parse_sso_message("A-secure-looking-api-key-123456").is_ok());
        assert!(parse_sso_message(
            r#"{"success":true,"data":{"api_key":"A-secure-looking-api-key-123456"}}"#
        )
        .is_ok());
        assert!(parse_sso_message(&"x".repeat(4097)).is_err());
        assert_eq!(
            sso_transition(SsoState::Idle, SsoEvent::Start).unwrap(),
            SsoState::Connecting
        );
        assert_eq!(
            sso_transition(SsoState::AwaitingAuthorization, SsoEvent::Cancel).unwrap(),
            SsoState::Cancelled
        );
    }
    #[test]
    fn retry_quota_and_size() {
        assert_eq!(parse_retry_after("5", 0), Some(Duration::from_secs(5)));
        assert_eq!(
            parse_retry_after("Thu, 01 Jan 1970 00:01:00 GMT", 0),
            Some(Duration::from_secs(60))
        );
        assert_eq!(pause_for_retry(None, false), Duration::from_secs(60));
        let quota = parse_quota_headers([
            ("x-rl-daily-limit", "20000"),
            ("x-rl-daily-remaining", "0"),
            ("x-rl-daily-reset", "2030"),
        ]);
        assert_eq!(quota["daily"].remaining, Some(0));
        let mut s = SizeAccounting::new(3).unwrap();
        assert_eq!(s.accept(2), Ok(2));
        assert!(s.accept(2).is_err());
    }
    #[test]
    fn secrets_are_exactly_named() {
        let got = filter_secret_environment([
            ("DELTAMOD_NEXUS_SSO_APP_ID", "x"),
            ("NEXUS_API_KEY", "secret"),
            ("DELTAMOD_NETWORK_TIMEOUT_MS", "5000"),
        ]);
        assert_eq!(got.len(), 2);
        assert!(!got.contains_key("NEXUS_API_KEY"));
    }
}
