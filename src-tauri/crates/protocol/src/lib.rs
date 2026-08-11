#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, VecDeque},
    fmt, fs,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

pub const MAX_URI_BYTES: usize = 4096;
pub const MAX_PATH_BYTES: usize = 512;
pub const MAX_QUEUE_ITEMS: usize = 256;
pub const MAX_ID: u32 = 2_000_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DomainError {
    Malformed,
    InvalidScheme,
    InvalidHost,
    InvalidPath,
    InvalidParam,
    InvalidId,
    InvalidUrl,
    TooLarge,
    NotFound,
    NotAllowed,
    Io,
}
impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed request",
            Self::InvalidScheme => "invalid scheme",
            Self::InvalidHost => "invalid host",
            Self::InvalidPath => "invalid path",
            Self::InvalidParam => "invalid parameter",
            Self::InvalidId => "invalid id",
            Self::InvalidUrl => "invalid URL",
            Self::TooLarge => "request too large",
            Self::NotFound => "asset not found",
            Self::NotAllowed => "asset not allowed",
            Self::Io => "asset unavailable",
        })
    }
}
impl std::error::Error for DomainError {}

fn decode_component(s: &str, limit: usize) -> Result<String, DomainError> {
    if s.len() > limit {
        return Err(DomainError::TooLarge);
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() || !b[i + 1].is_ascii_hexdigit() || !b[i + 2].is_ascii_hexdigit() {
                return Err(DomainError::Malformed);
            }
            out.push((hex(b[i + 1]) << 4) | hex(b[i + 2]));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| DomainError::Malformed)
}
fn hex(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
fn id(s: &str) -> Result<u32, DomainError> {
    let n = s.parse::<u32>().map_err(|_| DomainError::InvalidId)?;
    if n == 0 || n > MAX_ID {
        Err(DomainError::InvalidId)
    } else {
        Ok(n)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommunityAction {
    Import {
        item_id: u32,
        file_id: u32,
        source: String,
    },
    Launch {
        item_id: u32,
    },
}

/// Contract: deltamod-community://gb/import?item=<u32>&file=<u32>&source=<https URL>,
/// or deltamod-community://gb/launch?item=<u32>. Query keys are exact and unique.
pub fn parse_deep_link(raw: &str) -> Result<CommunityAction, DomainError> {
    if raw.len() > MAX_URI_BYTES || raw.contains('#') {
        return Err(DomainError::Malformed);
    }
    let rest = raw
        .strip_prefix("deltamod-community://")
        .ok_or(DomainError::InvalidScheme)?;
    let (authority, tail) = rest.split_once('/').ok_or(DomainError::Malformed)?;
    if authority != "gb" || authority.contains('@') || authority.contains(':') {
        return Err(DomainError::InvalidHost);
    }
    let (route, query) = tail.split_once('?').ok_or(DomainError::InvalidParam)?;
    if route != "import" && route != "launch" || query.is_empty() {
        return Err(DomainError::InvalidParam);
    }
    let mut params = HashMap::new();
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').ok_or(DomainError::Malformed)?;
        if k.is_empty()
            || params
                .insert(k, decode_component(v, MAX_URI_BYTES))
                .is_some()
        {
            return Err(DomainError::InvalidParam);
        }
    }
    let val = |key: &str| {
        params
            .get(key)
            .ok_or(DomainError::InvalidParam)
            .and_then(|v| {
                v.as_ref()
                    .map(String::as_str)
                    .map(str::to_owned)
                    .map_err(|_| DomainError::Malformed)
            })
    };
    if route == "launch" {
        if params.len() != 1 || !params.contains_key("item") {
            return Err(DomainError::InvalidParam);
        }
        return Ok(CommunityAction::Launch {
            item_id: id(&val("item")?)?,
        });
    }
    if params.len() != 3
        || !params.contains_key("item")
        || !params.contains_key("file")
        || !params.contains_key("source")
    {
        return Err(DomainError::InvalidParam);
    }
    let source = val("source")?;
    validate_remote_url(&source)?;
    Ok(CommunityAction::Import {
        item_id: id(&val("item")?)?,
        file_id: id(&val("file")?)?,
        source,
    })
}

fn validate_remote_url(raw: &str) -> Result<(), DomainError> {
    let (scheme, rest) = raw.split_once("://").ok_or(DomainError::InvalidUrl)?;
    if scheme != "https" || rest.is_empty() || rest.contains('#') || rest.contains('@') {
        return Err(DomainError::InvalidUrl);
    }
    let authority = rest
        .split(['/', '?'])
        .next()
        .ok_or(DomainError::InvalidUrl)?;
    let host = authority.split(':').next().ok_or(DomainError::InvalidUrl)?;
    if authority.contains(':') && !authority.ends_with(":443") {
        return Err(DomainError::InvalidUrl);
    }
    if host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok()
    {
        return Err(DomainError::InvalidUrl);
    }
    let h = host.to_ascii_lowercase();
    if h != "gamebanana.com" && !h.ends_with(".gamebanana.com") {
        return Err(DomainError::InvalidUrl);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingOpen {
    DeepLink(String),
    File(PathBuf),
}
#[derive(Clone, Debug)]
pub struct PendingQueue {
    inner: Arc<Mutex<QueueInner>>,
}
#[derive(Debug)]
struct QueueInner {
    ready: bool,
    items: VecDeque<PendingOpen>,
}
impl PendingQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(QueueInner {
                ready: false,
                items: VecDeque::new(),
            })),
        }
    }
    pub fn push(&self, item: PendingOpen) -> Result<(), DomainError> {
        let mut q = self.inner.lock().map_err(|_| DomainError::Io)?;
        if q.items.len() >= MAX_QUEUE_ITEMS {
            return Err(DomainError::TooLarge);
        }
        q.items.push_back(item);
        Ok(())
    }
    pub fn mark_renderer_ready(&self) -> Result<Vec<PendingOpen>, DomainError> {
        let mut q = self.inner.lock().map_err(|_| DomainError::Io)?;
        q.ready = true;
        Ok(q.items.drain(..).collect())
    }
    pub fn take_ready(&self) -> Result<Option<PendingOpen>, DomainError> {
        let mut q = self.inner.lock().map_err(|_| DomainError::Io)?;
        if q.ready {
            Ok(q.items.pop_front())
        } else {
            Ok(None)
        }
    }
    pub fn len(&self) -> Result<usize, DomainError> {
        Ok(self.inner.lock().map_err(|_| DomainError::Io)?.items.len())
    }
    pub fn is_empty(&self) -> Result<bool, DomainError> {
        Ok(self.len()? == 0)
    }
}
impl Default for PendingQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ByteRange {
    Full,
    Bounded { start: u64, end: u64 },
    Suffix { length: u64 },
    Open { start: u64 },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RangePlan {
    Full { length: u64 },
    Partial { start: u64, end: u64, length: u64 },
    Unsatisfiable { total: u64 },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponsePlan {
    pub status: u16,
    pub content_length: u64,
    pub content_range: Option<String>,
}
pub fn parse_range(header: &str) -> Result<ByteRange, DomainError> {
    let x = header
        .strip_prefix("bytes=")
        .ok_or(DomainError::Malformed)?;
    if x.contains(',') {
        return Err(DomainError::NotAllowed);
    }
    let (a, b) = x.split_once('-').ok_or(DomainError::Malformed)?;
    if a.is_empty() {
        let n = b.parse::<u64>().map_err(|_| DomainError::Malformed)?;
        if n == 0 {
            Err(DomainError::InvalidParam)
        } else {
            Ok(ByteRange::Suffix { length: n })
        }
    } else {
        let start = a.parse::<u64>().map_err(|_| DomainError::Malformed)?;
        if b.is_empty() {
            Ok(ByteRange::Open { start })
        } else {
            let end = b.parse::<u64>().map_err(|_| DomainError::Malformed)?;
            if end < start {
                Err(DomainError::InvalidParam)
            } else {
                Ok(ByteRange::Bounded { start, end })
            }
        }
    }
}
pub fn plan_range(range: Option<&str>, total: u64) -> Result<RangePlan, DomainError> {
    if total == 0 {
        return if range.is_some() {
            Ok(RangePlan::Unsatisfiable { total })
        } else {
            Ok(RangePlan::Full { length: 0 })
        };
    }
    let Some(h) = range else {
        return Ok(RangePlan::Full { length: total });
    };
    let r = parse_range(h)?;
    let p = match r {
        ByteRange::Bounded { start, end } if start < total => Some((start, end.min(total - 1))),
        ByteRange::Open { start } if start < total => Some((start, total - 1)),
        ByteRange::Suffix { length } => Some((total.saturating_sub(length), total - 1)),
        _ => None,
    };
    Ok(p.map_or(RangePlan::Unsatisfiable { total }, |(s, e)| {
        RangePlan::Partial {
            start: s,
            end: e,
            length: e - s + 1,
        }
    }))
}
pub fn plan_response(range: Option<&str>, total: u64) -> Result<ResponsePlan, DomainError> {
    match plan_range(range, total)? {
        RangePlan::Full { length } => Ok(ResponsePlan {
            status: 200,
            content_length: length,
            content_range: None,
        }),
        RangePlan::Partial { start, end, length } => Ok(ResponsePlan {
            status: 206,
            content_length: length,
            content_range: Some(format!("bytes {start}-{end}/{total}")),
        }),
        RangePlan::Unsatisfiable { total } => Ok(ResponsePlan {
            status: 416,
            content_length: 0,
            content_range: Some(format!("bytes */{total}")),
        }),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetKind {
    App,
    Theme,
    Packet,
}
#[derive(Clone, Debug)]
pub struct AssetRoots {
    pub app: PathBuf,
    pub builtin_theme: PathBuf,
    pub user_theme: Option<PathBuf>,
    pub packets: HashMap<String, PathBuf>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetRequest {
    pub kind: AssetKind,
    pub host: String,
    pub path: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetPlan {
    pub path: PathBuf,
    pub content_type: &'static str,
}
pub fn parse_asset_request(raw: &str) -> Result<AssetRequest, DomainError> {
    if raw.len() > MAX_URI_BYTES || raw.contains('#') {
        return Err(DomainError::Malformed);
    };
    let (scheme, rest) = raw.split_once("://").ok_or(DomainError::Malformed)?;
    let kind = match scheme {
        "app" => AssetKind::App,
        "theme" => AssetKind::Theme,
        "packet" => AssetKind::Packet,
        _ => return Err(DomainError::InvalidScheme),
    };
    let (host, p) = rest.split_once('/').ok_or(DomainError::Malformed)?;
    if host.is_empty() || host.chars().any(|c| c == '?' || c == '@' || c == ':') {
        return Err(DomainError::InvalidHost);
    };
    let path = decode_component(p, MAX_PATH_BYTES)?;
    validate_relative(&path)?;
    Ok(AssetRequest {
        kind,
        host: host.to_owned(),
        path,
    })
}
fn validate_relative(s: &str) -> Result<(), DomainError> {
    if s.is_empty() || s.contains('\\') || s.contains('\0') {
        return Err(DomainError::InvalidPath);
    };
    let p = Path::new(s);
    if p.is_absolute() {
        return Err(DomainError::InvalidPath);
    };
    for c in p.components() {
        if !matches!(c, Component::Normal(_)) {
            return Err(DomainError::InvalidPath);
        }
    }
    Ok(())
}
// Kept separate from path construction: canonicalization resolves symlinks/reparse points before authorization.
pub fn plan_asset(raw: &str, roots: &AssetRoots) -> Result<AssetPlan, DomainError> {
    let r = parse_asset_request(raw)?;
    let ext = Path::new(&r.path)
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_ascii_lowercase())
        .ok_or(DomainError::NotAllowed)?;
    let allowed = match r.kind {
        AssetKind::App => matches!(
            ext.as_str(),
            "css"
                | "js"
                | "json"
                | "html"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "ico"
                | "ttf"
                | "woff"
                | "woff2"
                | "mp3"
                | "wav"
                | "ogg"
                | "mp4"
        ),
        AssetKind::Theme => matches!(
            ext.as_str(),
            "json"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "mp3"
                | "wav"
                | "ogg"
                | "mp4"
        ),
        AssetKind::Packet => {
            r.path.starts_with("image/")
                && matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
                )
        }
    };
    if !allowed {
        return Err(DomainError::NotAllowed);
    };
    let roots_to_try: Vec<&Path> = match r.kind {
        AssetKind::App => vec![roots.app.as_path()],
        AssetKind::Theme => {
            let mut v = vec![roots.builtin_theme.as_path()];
            if let Some(x) = roots.user_theme.as_deref() {
                v.push(x)
            }
            v
        }
        AssetKind::Packet => vec![roots
            .packets
            .get(&r.host)
            .ok_or(DomainError::NotFound)?
            .as_path()],
    };
    for root in roots_to_try {
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        let candidate = root.join(&r.path);
        let Ok(real) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !real.starts_with(&root) {
            return Err(DomainError::NotAllowed);
        };
        if !real.is_file() {
            continue;
        };
        return Ok(AssetPlan {
            path: real,
            content_type: content_type(&ext),
        });
    }
    Err(DomainError::NotFound)
}
fn content_type(e: &str) -> &'static str {
    match e {
        "css" => "text/css",
        "js" => "text/javascript",
        "json" => "application/json",
        "html" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    #[test]
    fn links_are_strict() {
        assert_eq!(
            parse_deep_link("deltamod-community://gb/launch?item=12"),
            Ok(CommunityAction::Launch { item_id: 12 })
        );
        assert!(parse_deep_link("deltamod-community://gb/launch?item=12&item=13").is_err());
        assert!(parse_deep_link("deltamod-community://gb/launch?item=%ZZ").is_err());
        assert!(parse_deep_link("deltamod-community://gb/launch?item=12#x").is_err());
        assert!(parse_deep_link("deltamod-community://gb/launch?item=0").is_err());
    }
    #[test]
    fn url_validation_rejects_tricks() {
        for h in [
            "http://gamebanana.com/x",
            "https://localhost/x",
            "https://127.0.0.1/x",
            "https://gamebanana.com.evil/x",
            "https://user@gamebanana.com/x",
        ] {
            assert!(validate_remote_url(h).is_err())
        }
        assert!(validate_remote_url("https://sub.gamebanana.com/x").is_ok());
    }
    #[test]
    fn queue_is_fifo_and_not_lossy() {
        let q = PendingQueue::new();
        q.push(PendingOpen::DeepLink("a".into())).unwrap();
        q.push(PendingOpen::DeepLink("b".into())).unwrap();
        assert!(q.take_ready().unwrap().is_none());
        assert_eq!(q.mark_renderer_ready().unwrap().len(), 2);
        assert_eq!(q.take_ready().unwrap(), None);
    }
    #[test]
    fn ranges_cover_empty_overflow_and_partial() {
        assert_eq!(plan_range(None, 0).unwrap(), RangePlan::Full { length: 0 });
        assert_eq!(
            plan_range(Some("bytes=0-"), 0).unwrap(),
            RangePlan::Unsatisfiable { total: 0 }
        );
        assert_eq!(
            plan_range(Some("bytes=3-99"), 10).unwrap(),
            RangePlan::Partial {
                start: 3,
                end: 9,
                length: 7
            }
        );
        assert_eq!(
            plan_response(Some("bytes=3-99"), 10).unwrap(),
            ResponsePlan {
                status: 206,
                content_length: 7,
                content_range: Some("bytes 3-9/10".into())
            }
        );
        assert_eq!(plan_response(Some("bytes=99-"), 10).unwrap().status, 416);
        assert!(parse_range("bytes=1-2,4-5").is_err());
        assert_eq!(
            plan_range(Some("bytes=-4"), 3).unwrap(),
            RangePlan::Partial {
                start: 0,
                end: 2,
                length: 3
            }
        );
    }
    #[test]
    fn assets_prefer_builtin_and_block_escape() {
        let d = tempdir().unwrap();
        let app = d.path().join("app");
        let builtin = d.path().join("builtin");
        let user = d.path().join("user");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&builtin).unwrap();
        fs::create_dir_all(&user).unwrap();
        fs::write(builtin.join("x.png"), b"b").unwrap();
        fs::write(user.join("x.png"), b"u").unwrap();
        let roots = AssetRoots {
            app,
            builtin_theme: builtin,
            user_theme: Some(user),
            packets: HashMap::new(),
        };
        let p = plan_asset("theme://x/x.png", &roots).unwrap();
        assert!(p.path.ends_with("builtin\\x.png") || p.path.ends_with("builtin/x.png"));
        assert!(plan_asset("theme://x/%2e%2e/x.png", &roots).is_err());
        assert!(plan_asset("theme://x/x.exe", &roots).is_err());
    }
}
