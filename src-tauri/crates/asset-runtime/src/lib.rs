#![forbid(unsafe_code)]

use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

pub const MAX_URI_BYTES: usize = 4096;
pub const MAX_PATH_BYTES: usize = 512;
pub const MAX_QUEUE_ITEMS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Malformed,
    InvalidScheme,
    InvalidHost,
    InvalidPath,
    NotFound,
    NotAllowed,
    Io,
    TooLarge,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed request",
            Self::InvalidScheme => "invalid scheme",
            Self::InvalidHost => "invalid host",
            Self::InvalidPath => "invalid path",
            Self::NotFound => "asset not found",
            Self::NotAllowed => "asset not allowed",
            Self::Io => "asset unavailable",
            Self::TooLarge => "request too large",
        })
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetKind {
    App,
    Theme,
    Packet,
}
#[derive(Clone, Debug)]
pub struct Roots {
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
#[derive(Clone, Debug, PartialEq)]
pub struct AssetPlan {
    pub path: PathBuf,
    pub content_type: &'static str,
    pub length: u64,
}

fn percent_decode(value: &str, limit: usize) -> Result<String, Error> {
    if value.len() > limit {
        return Err(Error::TooLarge);
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit()
            {
                return Err(Error::Malformed);
            }
            out.push((hex(bytes[i + 1]) << 4) | hex(bytes[i + 2]));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::Malformed)
}
fn hex(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
fn relative(path: &str) -> Result<(), Error> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains('\0')
        || Path::new(path).is_absolute()
    {
        return Err(Error::InvalidPath);
    }
    if Path::new(path)
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::InvalidPath);
    }
    Ok(())
}
pub fn parse_asset_request(raw: &str) -> Result<AssetRequest, Error> {
    if raw.len() > MAX_URI_BYTES || raw.contains('#') {
        return Err(Error::Malformed);
    }
    let (scheme, rest) = raw.split_once("://").ok_or(Error::Malformed)?;
    let kind = match scheme {
        "app" => AssetKind::App,
        "theme" => AssetKind::Theme,
        "packet" => AssetKind::Packet,
        _ => return Err(Error::InvalidScheme),
    };
    let (host, encoded) = rest.split_once('/').ok_or(Error::Malformed)?;
    if host.is_empty()
        || !host.is_ascii()
        || host
            .chars()
            .any(|c| c == '?' || c == '@' || c == ':' || c == '\\')
    {
        return Err(Error::InvalidHost);
    }
    let path = percent_decode(encoded, MAX_PATH_BYTES)?;
    relative(&path)?;
    Ok(AssetRequest {
        kind,
        host: host.to_owned(),
        path,
    })
}

pub fn validate_deep_link(raw: &str) -> Result<(), Error> {
    if raw.len() > MAX_URI_BYTES || raw.contains('#') {
        return Err(Error::Malformed);
    }
    let rest = raw
        .strip_prefix("deltamod-community://")
        .ok_or(Error::InvalidScheme)?;
    let (host, tail) = rest.split_once('/').ok_or(Error::Malformed)?;
    if host != "gb" || tail.contains('@') || tail.contains('\\') {
        return Err(Error::InvalidHost);
    }
    let (route, query) = tail.split_once('?').ok_or(Error::Malformed)?;
    if !matches!(route, "launch" | "import") || query.is_empty() {
        return Err(Error::InvalidPath);
    }
    let mut values = HashMap::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(Error::Malformed)?;
        if !matches!(key, "item" | "file" | "source")
            || values
                .insert(key, percent_decode(value, MAX_URI_BYTES)?)
                .is_some()
        {
            return Err(Error::NotAllowed);
        }
    }
    let valid_id = |key: &str| {
        values
            .get(key)
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|value| value > 0 && value <= 2_000_000_000)
    };
    if route == "launch" {
        return if values.len() == 1 && valid_id("item") {
            Ok(())
        } else {
            Err(Error::NotAllowed)
        };
    }
    let source = values.get("source").ok_or(Error::NotAllowed)?;
    let valid_source = source.strip_prefix("https://").is_some_and(|value| {
        let authority = value.split(['/', '?']).next().unwrap_or("");
        let host = authority.split(':').next().unwrap_or("");
        !host.is_empty()
            && !authority.contains('@')
            && (!authority.contains(':') || authority.ends_with(":443"))
            && (host.eq_ignore_ascii_case("gamebanana.com")
                || host.to_ascii_lowercase().ends_with(".gamebanana.com"))
    });
    if values.len() == 3 && valid_id("item") && valid_id("file") && valid_source {
        Ok(())
    } else {
        Err(Error::NotAllowed)
    }
}

fn extension(path: &str) -> Result<&str, Error> {
    Path::new(path)
        .extension()
        .and_then(|x| x.to_str())
        .ok_or(Error::NotAllowed)
}
fn mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
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
        _ => return None,
    })
}
fn allowed(kind: AssetKind, path: &str, ext: &str) -> bool {
    match kind {
        AssetKind::App => mime(ext).is_some(),
        AssetKind::Theme => matches!(
            ext,
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
            (path == "icon.png" || path.starts_with("image/"))
                && matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
        }
    }
}

fn reparse_or_link(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.is_file() && meta.nlink() > 1 {
            return true;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn secure_file(
    io_root: &Path,
    auth_root: &Path,
    relative_path: &str,
) -> Result<(PathBuf, u64), Error> {
    let candidate = io_root.join(relative_path);
    let mut current = io_root.to_path_buf();
    for component in Path::new(relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(Error::InvalidPath);
        };
        current.push(name);
        let meta = fs::symlink_metadata(&current).map_err(|_| Error::NotFound)?;
        if meta.file_type().is_symlink() || reparse_or_link(&meta) {
            return Err(Error::NotAllowed);
        }
    }
    let real = fs::canonicalize(&candidate).map_err(|_| Error::NotFound)?;
    if !real.starts_with(auth_root) {
        return Err(Error::NotAllowed);
    }
    let meta = fs::metadata(&real).map_err(|_| Error::NotFound)?;
    if !meta.is_file() || reparse_or_link(&meta) {
        return Err(Error::NotAllowed);
    }
    Ok((real, meta.len()))
}

#[derive(Clone, Debug)]
pub struct AssetRuntime {
    roots: Roots,
    canonical: Roots,
}
impl AssetRuntime {
    pub fn new(roots: Roots) -> Result<Self, Error> {
        let canon = Roots {
            app: fs::canonicalize(&roots.app).map_err(|_| Error::Io)?,
            builtin_theme: fs::canonicalize(&roots.builtin_theme).map_err(|_| Error::Io)?,
            user_theme: roots
                .user_theme
                .as_ref()
                .map(|p| fs::canonicalize(p).map_err(|_| Error::Io))
                .transpose()?,
            packets: roots
                .packets
                .iter()
                .map(|(k, p)| Ok((k.clone(), fs::canonicalize(p).map_err(|_| Error::Io)?)))
                .collect::<Result<_, Error>>()?,
        };
        Ok(Self {
            roots,
            canonical: canon,
        })
    }
    pub fn resolve(&self, raw: &str) -> Result<AssetPlan, Error> {
        let request = parse_asset_request(raw)?;
        let ext = extension(&request.path)?.to_ascii_lowercase();
        if !allowed(request.kind, &request.path, &ext) {
            return Err(Error::NotAllowed);
        }
        let roots: Vec<(&Path, &Path)> = match request.kind {
            AssetKind::App => vec![(&self.roots.app, &self.canonical.app)],
            AssetKind::Theme => {
                let mut v = vec![(
                    &self.roots.builtin_theme as &Path,
                    self.canonical.builtin_theme.as_path(),
                )];
                if let (Some(io_root), Some(auth_root)) =
                    (&self.roots.user_theme, self.canonical.user_theme.as_deref())
                {
                    v.push((io_root, auth_root));
                }
                v
            }
            AssetKind::Packet => vec![(
                self.roots
                    .packets
                    .get(&request.host)
                    .ok_or(Error::NotFound)?
                    .as_path(),
                self.canonical
                    .packets
                    .get(&request.host)
                    .ok_or(Error::NotFound)?
                    .as_path(),
            )],
        };
        for (io_root, auth_root) in roots {
            match secure_file(io_root, auth_root, &request.path) {
                Ok((path, length)) => {
                    return Ok(AssetPlan {
                        path,
                        content_type: mime(&ext).ok_or(Error::NotAllowed)?,
                        length,
                    })
                }
                Err(Error::NotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(Error::NotFound)
    }
    pub fn open(&self, plan: &AssetPlan) -> Result<File, Error> {
        File::open(&plan.path).map_err(|_| Error::Io)
    }
    pub fn roots(&self) -> &Roots {
        &self.roots
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Range {
    Full,
    Partial { start: u64, end: u64 },
    Unsatisfiable,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Headers {
    pub status: u16,
    pub content_length: u64,
    pub content_range: Option<String>,
    pub content_type: &'static str,
    pub accept_ranges: &'static str,
}
pub fn plan_range(header: Option<&str>, total: u64) -> Result<Range, Error> {
    let Some(header) = header else {
        return Ok(Range::Full);
    };
    if !header.starts_with("bytes=") || header.contains(',') {
        return Err(Error::NotAllowed);
    }
    let (a, b) = header[6..].split_once('-').ok_or(Error::Malformed)?;
    let range = if a.is_empty() {
        let n = b.parse::<u64>().map_err(|_| Error::Malformed)?;
        if n == 0 || total == 0 {
            return Ok(Range::Unsatisfiable);
        }
        (total.saturating_sub(n), total - 1)
    } else {
        let start = a.parse::<u64>().map_err(|_| Error::Malformed)?;
        if start >= total {
            return Ok(Range::Unsatisfiable);
        }
        let end = if b.is_empty() {
            total - 1
        } else {
            b.parse::<u64>()
                .map_err(|_| Error::Malformed)?
                .min(total - 1)
        };
        if end < start {
            return Ok(Range::Unsatisfiable);
        }
        (start, end)
    };
    Ok(Range::Partial {
        start: range.0,
        end: range.1,
    })
}
pub fn headers(plan: &AssetPlan, range: Option<&str>) -> Result<Headers, Error> {
    match plan_range(range, plan.length)? {
        Range::Full => Ok(Headers {
            status: 200,
            content_length: plan.length,
            content_range: None,
            content_type: plan.content_type,
            accept_ranges: "bytes",
        }),
        Range::Partial { start, end } => Ok(Headers {
            status: 206,
            content_length: end - start + 1,
            content_range: Some(format!("bytes {start}-{end}/{}", plan.length)),
            content_type: plan.content_type,
            accept_ranges: "bytes",
        }),
        Range::Unsatisfiable => Ok(Headers {
            status: 416,
            content_length: 0,
            content_range: Some(format!("bytes */{}", plan.length)),
            content_type: plan.content_type,
            accept_ranges: "bytes",
        }),
    }
}
pub struct Body {
    file: File,
    remaining: u64,
}
impl Body {
    pub fn new(mut file: File, range: Range, total: u64) -> Result<Self, Error> {
        let (start, remaining) = match range {
            Range::Full => (0, total),
            Range::Partial { start, end } => (start, end - start + 1),
            Range::Unsatisfiable => (0, 0),
        };
        file.seek(SeekFrom::Start(start)).map_err(|_| Error::Io)?;
        Ok(Self { file, remaining })
    }
}
impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let max = buf.len().min(self.remaining as usize);
        let n = self.file.read(&mut buf[..max])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Pending {
    DeepLink(String),
    File(PathBuf),
}
#[derive(Clone, Debug)]
pub struct DeepLinkState {
    inner: Arc<Mutex<VecDeque<Pending>>>,
    ready: Arc<Mutex<bool>>,
}
impl Default for DeepLinkState {
    fn default() -> Self {
        Self::new()
    }
}
impl DeepLinkState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
            ready: Arc::new(Mutex::new(false)),
        }
    }
    pub fn enqueue(&self, item: Pending) -> Result<(), Error> {
        let mut q = self.inner.lock().map_err(|_| Error::Io)?;
        if q.len() >= MAX_QUEUE_ITEMS {
            return Err(Error::TooLarge);
        }
        q.push_back(item);
        Ok(())
    }
    pub fn renderer_ready(&self) -> Result<(), Error> {
        *self.ready.lock().map_err(|_| Error::Io)? = true;
        Ok(())
    }
    pub fn next(&self) -> Result<Option<Pending>, Error> {
        if !*self.ready.lock().map_err(|_| Error::Io)? {
            return Ok(None);
        }
        Ok(self.inner.lock().map_err(|_| Error::Io)?.pop_front())
    }
    pub fn len(&self) -> Result<usize, Error> {
        Ok(self.inner.lock().map_err(|_| Error::Io)?.len())
    }
    pub fn is_empty(&self) -> Result<bool, Error> {
        Ok(self.len()? == 0)
    }
}
pub trait DeepLinkEvents {
    fn initial_urls(&self) -> Vec<String>;
    fn second_instance(&self, urls: &[String]);
    fn open_urls(&self, urls: &[String]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, fs};
    use tempfile::tempdir;
    fn runtime() -> (AssetRuntime, tempfile::TempDir) {
        let d = tempdir().unwrap();
        let app = d.path().join("app");
        let built = d.path().join("built");
        let user = d.path().join("user");
        let packet = d.path().join("packet");
        for p in [&app, &built, &user, &packet] {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(built.join("same.png"), b"builtin").unwrap();
        fs::write(user.join("same.png"), b"user").unwrap();
        fs::write(packet.join("image/a.png"), b"x").unwrap_or_else(|_| {
            fs::create_dir_all(packet.join("image")).unwrap();
            fs::write(packet.join("image/a.png"), b"x").unwrap();
        });
        fs::write(packet.join("icon.png"), b"x").unwrap();
        let mut packets = HashMap::new();
        packets.insert("p".into(), packet);
        (
            AssetRuntime::new(Roots {
                app,
                builtin_theme: built,
                user_theme: Some(user),
                packets,
            })
            .unwrap(),
            d,
        )
    }
    #[test]
    fn traversal_and_allowlists() {
        let (r, _d) = runtime();
        assert!(r.resolve("theme://x/%2e%2e/same.png").is_err());
        assert!(r.resolve("theme://x/same.exe").is_err());
        assert!(r.resolve("packet://p/audio/a.mp3").is_err());
    }
    #[test]
    fn deep_links_are_strict() {
        assert!(validate_deep_link("deltamod-community://gb/launch?item=12").is_ok());
        assert!(validate_deep_link("deltamod-community://gb/launch?item=12&item=13").is_err());
        assert!(validate_deep_link("deltamod-community://gb/import?item=12&file=13&source=https%3A%2F%2Fgamebanana.com%2Fx").is_ok());
        assert!(validate_deep_link(
            "deltamod-community://gb/import?item=12&file=13&source=https%3A%2F%2Fevil.example%2Fx"
        )
        .is_err());
    }
    #[test]
    fn builtin_wins_and_packet_is_images() {
        let (r, _d) = runtime();
        let p = r.resolve("theme://x/same.png").unwrap();
        assert!(p.path.to_string_lossy().contains("built"));
        assert!(r.resolve("packet://p/image/a.png").is_ok());
        assert!(r.resolve("packet://p/icon.png").is_ok());
    }
    #[test]
    fn ranges_empty_large_and_headers() {
        let (r, _d) = runtime();
        let mut p = r.resolve("packet://p/image/a.png").unwrap();
        p.length = 0;
        assert_eq!(plan_range(None, 0).unwrap(), Range::Full);
        assert_eq!(
            plan_range(Some("bytes=0-"), 0).unwrap(),
            Range::Unsatisfiable
        );
        assert_eq!(
            plan_range(Some("bytes=3-99"), 10).unwrap(),
            Range::Partial { start: 3, end: 9 }
        );
        assert_eq!(headers(&p, None).unwrap().status, 200);
    }
    #[test]
    fn queue_is_fifo_until_ready() {
        let q = DeepLinkState::new();
        q.enqueue(Pending::DeepLink("a".into())).unwrap();
        q.enqueue(Pending::DeepLink("b".into())).unwrap();
        assert!(q.next().unwrap().is_none());
        q.renderer_ready().unwrap();
        assert_eq!(q.next().unwrap(), Some(Pending::DeepLink("a".into())));
        assert_eq!(q.next().unwrap(), Some(Pending::DeepLink("b".into())));
    }
    #[cfg(unix)]
    #[test]
    fn hardlinks_are_rejected() {
        use std::os::unix::fs::symlink;
        let (r, d) = runtime();
        let outside = d.path().join("outside.png");
        fs::write(&outside, b"x").unwrap();
        let link = d.path().join("app").join("link.png");
        fs::hard_link(outside, link).unwrap();
        assert_eq!(r.resolve("app://x/link.png"), Err(Error::NotAllowed));
        let sym = d.path().join("app").join("sym.png");
        symlink(d.path().join("packet/image/a.png"), sym).unwrap();
        assert_eq!(r.resolve("app://x/sym.png"), Err(Error::NotAllowed));
    }
}
