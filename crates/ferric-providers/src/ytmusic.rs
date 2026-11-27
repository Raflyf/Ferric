//! Pure-Rust YouTube Music InnerTube client for Zero-Login Guest Mode (Home feed, live Search,
//! and direct audio stream caching) and 1-Click Google Account Sign-In (`SAPISIDHASH` session).

use crate::yt_cleaner::clean_youtube_metadata;
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{AudioQuality, Playlist, Track, TrackSource, VideoQuality};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const YTM_ORIGIN: &str = "https://music.youtube.com";
const YT_ORIGIN: &str = "https://www.youtube.com";
const WEB_REMIX_VERSION: &str = "1.20260707.12.00";
const WEB_REMIX_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";
const VISIONOS_VERSION: &str = "1.02";
const VISIONOS_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const STREAM_CHUNK_BYTES: u64 = 512 * 1024;
const STREAM_CONCURRENCY: usize = 3;
const MAX_SINGLE_STREAM_BYTES: u64 = 64 * 1024 * 1024;
const MAX_STREAM_CACHE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARTWORK_CACHE_BYTES: u64 = 128 * 1024 * 1024;

use std::sync::{Mutex, OnceLock};

static CACHE_PRUNE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static GUEST_VISITOR_CACHE: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();
static VISIONOS_AUTH_REJECTED: AtomicBool = AtomicBool::new(false);
static HOME_REFRESH_EPOCH: AtomicUsize = AtomicUsize::new(0);
static HOME_MORE_EPOCH: AtomicUsize = AtomicUsize::new(0);
type ExploreShelfCache = Option<(Instant, Vec<YtmHomeShelf>)>;
static CACHED_EXPLORE_SHELVES: OnceLock<Mutex<ExploreShelfCache>> = OnceLock::new();

fn extract_strict_https_host(raw: &str) -> Option<String> {
    if raw
        .bytes()
        .any(|b| b.is_ascii_control() || b == b' ' || b == b'\\')
    {
        return None;
    }
    let rest = raw.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains(':')
        || authority.contains('[')
        || authority.contains(']')
    {
        return None;
    }
    let host = authority.to_ascii_lowercase();
    if host.starts_with('.') || host.ends_with('.') || !host.contains('.') {
        return None;
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return None;
        }
    }
    Some(host)
}

/// Strictly allowlist HTTPS album/artist cover hosts (prevents SSRF via untrusted thumbnail URLs).
pub fn is_allowed_artwork_url(raw: &str) -> bool {
    let Some(host) = extract_strict_https_host(raw) else {
        return false;
    };
    host == "googleusercontent.com"
        || host.ends_with(".googleusercontent.com")
        || host == "ggpht.com"
        || host.ends_with(".ggpht.com")
        || host == "i.ytimg.com"
        || host == "i9.ytimg.com"
        || host == "scdn.co"
        || host.ends_with(".scdn.co")
        || host == "spotifycdn.com"
        || host.ends_with(".spotifycdn.com")
}

/// Strictly allowlist HTTPS media CDN hosts (`*.googlevideo.com`) for audio & video streams.
pub fn is_allowed_stream_url(raw: &str) -> bool {
    let Some(host) = extract_strict_https_host(raw) else {
        return false;
    };
    host == "googlevideo.com" || host.ends_with(".googlevideo.com")
}

/// Enforce a serialized LRU disk-space cap on a flat cache directory.
/// Removes stale `.part` leftovers (>30 min old) without touching active `.part` files or
/// `protect_path`, then evicts oldest-modified finalized files until `total <= max_bytes`.
fn prune_cache_dir_lru(cache_dir: &Path, max_bytes: u64, protect_path: Option<&Path>) {
    let lock = CACHE_PRUNE_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return;
    };
    let now = SystemTime::now();
    let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            let is_part = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("part"));
            if is_part {
                if now
                    .duration_since(modified)
                    .is_ok_and(|age| age > Duration::from_secs(30 * 60))
                {
                    let _ = fs::remove_file(&path);
                }
                continue;
            }
            let len = meta.len();
            total = total.saturating_add(len);
            if protect_path.is_some_and(|p| p == path.as_path()) {
                continue;
            }
            files.push((path, len, modified));
        }
    }
    if total <= max_bytes {
        return;
    }
    files.sort_by_key(|(_, _, m)| *m);
    for (path, len, _) in files {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

fn touch_cache_mtime(path: &Path) {
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

fn is_valid_audio_container_header(path: &Path, ext: &str) -> bool {
    use std::io::Read;
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut hdr = [0u8; 16];
    let Ok(n) = file.read(&mut hdr) else {
        return false;
    };
    if n < 12 {
        return false;
    }
    if ext.eq_ignore_ascii_case("webm") {
        // EBML / Matroska / WebM header signature.
        hdr[0..4] == [0x1A, 0x45, 0xDF, 0xA3]
    } else if ext.eq_ignore_ascii_case("m4a") {
        // ISO BMFF / MP4 top-level box type (`ftyp`, `moov`, `mdat`, `free`, `wide`).
        matches!(&hdr[4..8], b"ftyp" | b"moov" | b"mdat" | b"free" | b"wide")
    } else {
        true
    }
}

#[derive(Clone, Default)]
pub struct YtmSession {
    pub visitor_data: String,
    pub cookie: String,
    pub sapisid: String,
}

impl std::fmt::Debug for YtmSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |s: &str| if s.is_empty() { "" } else { "[REDACTED]" };
        f.debug_struct("YtmSession")
            .field("visitor_data", &redact(&self.visitor_data))
            .field("cookie", &redact(&self.cookie))
            .field("sapisid", &redact(&self.sapisid))
            .finish()
    }
}

impl YtmSession {
    pub fn guest(visitor_data: String) -> Self {
        Self {
            visitor_data,
            cookie: String::new(),
            sapisid: String::new(),
        }
    }

    pub fn is_signed_in(&self) -> bool {
        !self.cookie.trim().is_empty() && !self.sapisid.trim().is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct YtmHomeFeed {
    pub visitor_data: String,
    pub playlist: Playlist,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct YtmAccountLibrary {
    pub display_name: String,
    pub visitor_data: String,
    pub liked_tracks: Vec<Track>,
    pub playlists: Vec<(Playlist, Vec<Track>)>,
    pub home_tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct CachedAudioStream {
    pub path: PathBuf,
    pub codec: String,
    pub bitrate_kbps: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct VideoStreamOption {
    pub quality: VideoQuality,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: u32,
    pub url: String,
    pub mime: String,
}

#[derive(Debug, Clone)]
pub struct YtmMoodChip {
    pub title: String,
    pub params: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtmCardKind {
    Song,
    Album,
    Playlist,
    Artist,
}

#[derive(Debug, Clone)]
pub struct YtmShelfCard {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: YtmCardKind,
    pub browse_id: Option<String>,
    pub video_id: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct YtmHomeShelf {
    pub title: String,
    pub cards: Vec<YtmShelfCard>,
}

#[derive(Debug, Clone, Default)]
pub struct YtmRichHome {
    pub visitor_data: String,
    pub chips: Vec<YtmMoodChip>,
    pub quick_picks: Vec<Track>,
    pub shelves: Vec<YtmHomeShelf>,
    pub all_tracks: Vec<Track>,
    pub continuation_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct YtmBrowseDetail {
    pub browse_id: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub thumbnail_url: Option<String>,
    pub kind: YtmCardKind,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, Default)]
pub struct YtmRichSearch {
    pub tracks: Vec<Track>,
    pub cards: Vec<YtmShelfCard>,
}

/// Normalize Google / YouTube thumbnail URLs into crisp square JPEG endpoints
/// so the native JPEG decoder (`makepad_zune_jpeg`) decodes every cover reliably without WebP.
pub fn normalize_ytm_thumbnail_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let full = if let Some(rest) = trimmed.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        trimmed.to_string()
    };

    let host = extract_strict_https_host(&full).unwrap_or_default();
    if host == "lh3.googleusercontent.com"
        || host == "yt3.googleusercontent.com"
        || host == "yt3.ggpht.com"
        || host.ends_with(".googleusercontent.com")
        || host.ends_with(".ggpht.com")
    {
        let no_query = full.split('?').next().unwrap_or(&full);
        if let Some(slash_idx) = no_query.rfind('/')
            && let Some(eq_rel) = no_query[slash_idx..].find('=')
        {
            let base = &no_query[..slash_idx + eq_rel];
            return format!("{base}=w300-h300-l90-rj");
        }
        return format!("{no_query}=w300-h300-l90-rj");
    }

    if host == "i.ytimg.com" || host == "i9.ytimg.com" {
        let no_query = full.split('?').next().unwrap_or(&full);
        // Clamp 1280x720 maxresdefault/sddefault/hqdefault to compact 320x180 mqdefault.jpg
        // so artwork decodes 12x faster and consumes minimal RAM.
        for heavy in ["maxresdefault.jpg", "sddefault.jpg", "hqdefault.jpg"] {
            if let Some(prefix) = no_query.strip_suffix(heavy) {
                return format!("{prefix}mqdefault.jpg");
            }
        }
        return no_query.to_string();
    }

    full
}

fn extract_best_thumbnail(val: &Value, fallback_video_id: Option<&str>) -> Option<String> {
    let arr = val
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .or_else(|| val.pointer("/thumbnail/croppedSquareThumbnailRenderer/thumbnail/thumbnails"))
        .or_else(|| val.pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails"))
        .or_else(|| val.pointer("/thumbnail/thumbnails"))
        .or_else(|| val.get("thumbnails"))
        .and_then(Value::as_array);

    if let Some(thumbs) = arr {
        let best_url = thumbs
            .iter()
            .filter_map(|t| {
                let url = t.get("url").and_then(Value::as_str)?;
                let w = t.get("width").and_then(Value::as_u64).unwrap_or(0);
                Some((w, url))
            })
            .max_by_key(|(w, _)| *w)
            .map(|(_, u)| normalize_ytm_thumbnail_url(u))
            .filter(|s| !s.is_empty());
        if best_url.is_some() {
            return best_url;
        }
    }

    fallback_video_id
        .filter(|vid| is_valid_video_id(vid))
        .map(|vid| format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"))
}

fn is_supported_image_bytes(bytes: &[u8]) -> bool {
    (bytes.len() > 256 && bytes[0] == 0xFF && bytes[1] == 0xD8)
        || (bytes.len() > 256
            && bytes[0] == 0x89
            && bytes[1] == 0x50
            && bytes[2] == 0x4E
            && bytes[3] == 0x47)
}

/// Return the local disk-cache path for an artwork URL if already downloaded and valid.
pub fn cached_artwork_file(url: &str, cache_dir: &Path) -> Option<std::path::PathBuf> {
    let normalized = normalize_ytm_thumbnail_url(url);
    if normalized.is_empty() {
        return None;
    }
    let key = sha1_hex(normalized.as_bytes());
    let dest = cache_dir.join(format!("{key}.img"));
    dest.is_file().then_some(dest)
}

/// Download and disk-cache album/playlist/artist artwork bytes (JPEG or PNG) with 0 ms cache hits,
/// strict HTTPS host allowlist, zero redirects, and a 128 MiB LRU disk cap.
pub fn download_artwork_bytes(url: &str, cache_dir: &Path) -> ProviderResult<Vec<u8>> {
    let normalized = normalize_ytm_thumbnail_url(url);
    if normalized.is_empty() {
        return Err(ProviderError::Metadata("Empty artwork URL".to_string()));
    }
    if !is_allowed_artwork_url(&normalized) {
        return Err(ProviderError::Metadata(
            "Refused untrusted artwork URL host".to_string(),
        ));
    }
    fs::create_dir_all(cache_dir)?;
    let key = sha1_hex(normalized.as_bytes());
    let dest = cache_dir.join(format!("{key}.img"));
    if let Ok(existing) = fs::read(&dest)
        && is_supported_image_bytes(&existing)
    {
        touch_cache_mtime(&dest);
        return Ok(existing);
    }

    let agent = crate::shared_http_agent();
    let resp = agent
        .get(&normalized)
        .header("User-Agent", WEB_REMIX_UA)
        .header("Accept", "image/jpeg, image/png, */*;q=0.5")
        .call()
        .map_err(|e| ProviderError::Http(format!("Artwork fetch failed: {e}")))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http(format!("Artwork HTTP {status}")));
    }
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(4 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Artwork read failed: {e}")))?;

    if !is_supported_image_bytes(&bytes) {
        return Err(ProviderError::Metadata(
            "Unsupported artwork image format".to_string(),
        ));
    }

    let nonce = ferric_core::secrets::csprng_hex(6);
    let part = cache_dir.join(format!("{key}.{}.{nonce}.part", std::process::id()));
    if fs::write(&part, &bytes).is_ok() {
        if fs::rename(&part, &dest).is_err() {
            let _ = fs::remove_file(&part);
        } else {
            prune_cache_dir_lru(cache_dir, MAX_ARTWORK_CACHE_BYTES, Some(&dest));
        }
    }
    Ok(bytes)
}

fn shared_http_agent(ipv4_only: bool) -> &'static ureq::Agent {
    if ipv4_only {
        crate::shared_http_agent_ipv4()
    } else {
        crate::shared_http_agent()
    }
}

fn shared_stream_agent() -> &'static ureq::Agent {
    crate::shared_http_agent()
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1_700_000_000)
}

/// RFC 3174 pure-Rust SHA-1 hex digest used strictly for YouTube Music `SAPISIDHASH` headers.
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 80];
    for chunk in msg.as_chunks::<64>().0 {
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999_u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1_u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC_u32),
                _ => (b ^ c ^ d, 0xCA62_C1D6_u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    format!("{h0:08x}{h1:08x}{h2:08x}{h3:08x}{h4:08x}")
}

/// Build Google's `Authorization: SAPISIDHASH <ts>_<sha1>` header for authenticated InnerTube calls.
pub fn build_sapisidhash_header(sapisid: &str, origin: &str, unix_ts: u64) -> String {
    let input = format!("{unix_ts} {sapisid} {origin}");
    let digest = sha1_hex(input.as_bytes());
    format!("SAPISIDHASH {unix_ts}_{digest}")
}

/// Extract `SAPISID` or `__Secure-3PAPISID` from a standard HTTP `Cookie` header string.
pub fn extract_sapisid_from_cookie(cookie_header: &str) -> Option<String> {
    let mut fallback = None;
    for part in cookie_header.split(';') {
        if let Some((k, v)) = part.trim().split_once('=') {
            let key = k.trim();
            let val = v.trim();
            if val.is_empty() {
                continue;
            }
            if key == "__Secure-3PAPISID" {
                return Some(val.to_string());
            }
            if key == "SAPISID" {
                fallback = Some(val.to_string());
            }
        }
    }
    fallback
}

fn post_ytm_json_with_opts(
    endpoint: &str,
    body: &Value,
    session: &YtmSession,
    attempts: usize,
    timeout_secs: u64,
) -> ProviderResult<Value> {
    let url = format!("{YTM_ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false");
    let payload = serde_json::to_string(body)
        .map_err(|e| ProviderError::Metadata(format!("JSON encode failed: {e}")))?;

    let mut last_err = None;
    let total_attempts = attempts.max(1);
    for attempt in 0..total_attempts {
        // Attempt 0 uses the system default stack; retries force IPv4 + fresh Connection: close
        // to immediately escape Windows ISP IPv6 blackholes and stalled chunked keep-alive sockets.
        let ipv4_only = attempt >= 1;
        let _ = timeout_secs;
        let agent = shared_http_agent(ipv4_only);
        let mut req = agent
            .post(&url)
            .header("Content-Type", "application/json")
            .header("User-Agent", WEB_REMIX_UA)
            .header("Origin", YTM_ORIGIN)
            .header("Referer", "https://music.youtube.com/")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("X-YouTube-Client-Name", "67")
            .header("X-YouTube-Client-Version", WEB_REMIX_VERSION);
        if attempt >= 1 {
            req = req.header("Connection", "close");
        }

        if !session.visitor_data.is_empty() {
            req = req.header("X-Goog-Visitor-Id", &session.visitor_data);
        }
        if session.is_signed_in() {
            let auth =
                build_sapisidhash_header(&session.sapisid, YTM_ORIGIN, unix_timestamp_secs());
            req = req
                .header("Cookie", &session.cookie)
                .header("Authorization", &auth)
                .header("X-Origin", YTM_ORIGIN)
                .header("X-Goog-AuthUser", "0");
        }

        let res: ProviderResult<Value> = (|| {
            let resp = req.send(&payload).map_err(|e| {
                ProviderError::Http(format!("YouTube Music {endpoint} failed: {e}"))
            })?;
            let status = resp.status().as_u16();
            let text = resp
                .into_body()
                .into_with_config()
                .limit(12 * 1024 * 1024)
                .read_to_string()
                .map_err(|e| ProviderError::Http(format!("YouTube Music read failed: {e}")))?;
            if !(200..300).contains(&status) {
                return Err(ProviderError::Http(format!(
                    "YouTube Music {endpoint} returned HTTP {status}"
                )));
            }
            serde_json::from_str(&text).map_err(|e| {
                ProviderError::Metadata(format!("YouTube Music JSON parse failed: {e}"))
            })
        })();

        match res {
            Ok(val) => return Ok(val),
            Err(err) => {
                last_err = Some(err);
                if attempt + 1 < total_attempts {
                    std::thread::sleep(Duration::from_millis(280 + (attempt as u64) * 320));
                }
            }
        }
    }

    Err(last_err
        .unwrap_or_else(|| ProviderError::Http(format!("YouTube Music {endpoint} timed out"))))
}

fn post_ytm_json(endpoint: &str, body: &Value, session: &YtmSession) -> ProviderResult<Value> {
    post_ytm_json_with_opts(endpoint, body, session, 3, 18)
}

fn post_ytm_json_fast(endpoint: &str, body: &Value, session: &YtmSession) -> ProviderResult<Value> {
    post_ytm_json_with_opts(endpoint, body, session, 2, 14)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicLocale {
    GlobalEn,
    Indonesian,
    Japanese,
    Korean,
}

/// Detect the musical language/locale of a track from its title, artist, and album
/// so Radio recommendations stay language- and vibe-coherent instead of injecting random regional charts.
pub fn detect_track_locale(title: &str, artist: &str, album: &str) -> MusicLocale {
    let mut has_jp = false;
    for ch in title.chars().chain(artist.chars()).chain(album.chars()) {
        let cp = ch as u32;
        if (0xAC00..=0xD7AF).contains(&cp) || (0x1100..=0x11FF).contains(&cp) {
            return MusicLocale::Korean;
        }
        if (0x3040..=0x30FF).contains(&cp)
            || (0x4E00..=0x9FFF).contains(&cp)
            || (0x3400..=0x4DBF).contains(&cp)
        {
            has_jp = true;
        }
    }
    if has_jp {
        return MusicLocale::Japanese;
    }

    // Allocation-free ASCII token scan over title + album using stack lowercasing (`mem-avoid-format`).
    // Deliberately avoids classifying English songs by Indonesian-born international artists as ID.
    let mut id_hits = 0usize;
    let mut en_hits = 0usize;
    let mut buf = [0u8; 16];
    for source in [title, album] {
        for raw_tok in source.split(|c: char| !c.is_ascii_alphabetic()) {
            let len = raw_tok.len();
            if !(2..=16).contains(&len) {
                continue;
            }
            for (dst, &b) in buf[..len].iter_mut().zip(raw_tok.as_bytes()) {
                *dst = b.to_ascii_lowercase();
            }
            // SAFETY: `raw_tok` was split on `!is_ascii_alphabetic()`, so every byte is valid ASCII UTF-8.
            let tok = std::str::from_utf8(&buf[..len]).unwrap_or("");
            match tok {
                "koplo" | "dangdut" | "galau" | "melayu" | "indonesia" | "lagu" => {
                    return MusicLocale::Indonesian;
                }
                "aku" | "kamu" | "kau" | "cinta" | "rindu" | "hati" | "bukan" | "yang"
                | "untuk" | "untukmu" | "dengan" | "dalam" | "malam" | "hujan" | "senja"
                | "kisah" | "patah" | "selalu" | "berdua" | "pergi" | "pergilah" | "tidak"
                | "jangan" | "bila" | "saat" | "sampai" | "telah" | "lagi" | "saja" | "cuma"
                | "terlalu" | "dirimu" | "diriku" | "tentang" | "seperti" | "bersama"
                | "bahagia" | "terindah" | "sial" | "tak" | "di" | "ke" | "dari" | "dan"
                | "atau" | "jalan" | "nanti" | "kita" | "hari" | "ini" | "itu" | "semua"
                | "kembali" | "menunggu" | "mengapa" | "kenapa" | "sendiri" | "sepi" | "luka"
                | "rasa" | "janji" | "mimpi" | "jiwa" | "tuhan" | "indah" | "manusia"
                | "komang" | "monokrom" | "sepatu" | "akad" | "ruang" | "tunggu" | "menuju"
                | "teduh" | "payung" | "dunia" | "batas" | "perempuan" | "pelukan" | "sempurna"
                | "pulang" | "hilang" | "waktu" | "langit" | "bintang" | "cerita" | "selamat"
                | "tinggal" | "kasih" => {
                    id_hits += 1;
                }
                "the" | "you" | "your" | "love" | "with" | "from" | "that" | "this" | "have"
                | "don" | "can" | "stay" | "night" | "heart" | "world" | "feat" | "remix" => {
                    en_hits += 1;
                }
                _ => {}
            }
        }
    }
    if id_hits >= 1 && id_hits > en_hits {
        return MusicLocale::Indonesian;
    }
    MusicLocale::GlobalEn
}

fn web_remix_context_for_locale(session: &YtmSession, locale: MusicLocale) -> Value {
    let (hl, gl) = match locale {
        MusicLocale::GlobalEn => ("en", "US"),
        MusicLocale::Indonesian => ("en", "ID"),
        MusicLocale::Japanese => ("ja", "JP"),
        MusicLocale::Korean => ("ko", "KR"),
    };
    let mut client = json!({
        "clientName": "WEB_REMIX",
        "clientVersion": WEB_REMIX_VERSION,
        "hl": hl,
        "gl": gl
    });
    if !session.visitor_data.is_empty() {
        client["visitorData"] = json!(session.visitor_data);
    }
    json!({ "client": client })
}

fn web_remix_context(session: &YtmSession) -> Value {
    web_remix_context_for_locale(session, MusicLocale::GlobalEn)
}

fn extract_visitor_data(root: &Value, fallback: &str) -> String {
    root.pointer("/responseContext/visitorData")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.to_string())
}

/// Fetch YouTube Music Home / Quick Picks feed immediately without any login (or personalized if signed in).
pub fn fetch_home_feed(session: &YtmSession) -> ProviderResult<YtmHomeFeed> {
    let body = json!({
        "browseId": "FEmusic_home",
        "context": web_remix_context(session),
    });
    let root = post_ytm_json("browse", &body, session)?;
    let visitor_data = extract_visitor_data(&root, &session.visitor_data);

    let mut tracks = Vec::new();
    let mut seen = HashSet::new();
    collect_music_items(&root, "YouTube Music", 36, &mut seen, &mut tracks);

    if tracks.len() < 12 {
        let mut next_sess = session.clone();
        next_sess.visitor_data = visitor_data.clone();
        let explore_body = json!({
            "browseId": "FEmusic_explore",
            "context": web_remix_context(&next_sess),
        });
        if let Ok(explore_root) = post_ytm_json("browse", &explore_body, &next_sess) {
            collect_music_items(
                &explore_root,
                "YouTube Music Explore",
                40,
                &mut seen,
                &mut tracks,
            );
        }
    }

    let pl_name = if session.is_signed_in() {
        "YouTube Music For You".to_string()
    } else {
        "Discover & Quick Picks".to_string()
    };
    let pl_thumb = tracks.first().and_then(|t| t.thumbnail_url.clone());
    let playlist = Playlist {
        id: "ytm:home:discover".to_string(),
        name: pl_name,
        source: TrackSource::YouTubeMusic,
        etag: None,
        track_count: tracks.len(),
        description: "Instant YouTube Music Home & Quick Picks".to_string(),
        thumbnail_url: pl_thumb,
    };
    Ok(YtmHomeFeed {
        visitor_data,
        playlist,
        tracks,
    })
}

fn extract_continuation_token(val: &Value) -> Option<String> {
    match val {
        Value::Object(map) => {
            if let Some(tok) = map
                .get("nextContinuationData")
                .and_then(|d| d.get("continuation"))
                .and_then(Value::as_str)
                && !tok.trim().is_empty()
            {
                return Some(tok.to_string());
            }
            for child in map.values() {
                if let Some(found) = extract_continuation_token(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for child in arr {
                if let Some(found) = extract_continuation_token(child) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn tracks_to_song_cards(tracks: &[Track]) -> Vec<YtmShelfCard> {
    tracks
        .iter()
        .filter_map(|t| {
            let vid = t.youtube_video_id.clone()?;
            let thumb = t
                .thumbnail_url
                .clone()
                .or_else(|| Some(format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg")));
            Some(YtmShelfCard {
                id: format!("yt:video:{vid}"),
                title: t.title.clone(),
                subtitle: if t.album.is_empty() || t.album == "YouTube Music Radio" {
                    t.artist.clone()
                } else {
                    format!("{} · {}", t.artist, t.album)
                },
                kind: YtmCardKind::Song,
                browse_id: None,
                video_id: Some(vid),
                thumbnail_url: thumb,
            })
        })
        .collect()
}

/// Listening-history & affinity profile passed into `fetch_rich_home` so Home recommendations
/// reflect what the user actually plays most plus globally popular songs co-listened by fans of the same taste.
#[derive(Debug, Clone, Default)]
pub struct YtmHomeTasteProfile {
    /// User's most-played, liked, and recently listened tracks (highest affinity first).
    pub top_tracks: Vec<Track>,
    /// Broader stratified sample across the user's synced & local library for deep rediscovery.
    pub library_tracks: Vec<Track>,
    /// User's most-played and library primary artists (highest affinity first).
    pub top_artists: Vec<String>,
    /// Recent user search queries (secondary taste signal).
    pub recent_searches: Vec<String>,
    /// Track IDs & YouTube video IDs recently shown on Home (hard-excluded on Refresh).
    pub excluded_track_ids: HashSet<String>,
    /// Album/Playlist/Artist card IDs recently shown on Home (deprioritized on Refresh).
    pub excluded_card_ids: HashSet<String>,
    /// Primary artists recently featured as Home shelves or Recommended Stations.
    pub excluded_artists: HashSet<String>,
    /// Previous `FEmusic_home` continuation token (walked on Refresh for fresh personalized shelves).
    pub continuation_token: Option<String>,
    /// Optional rotating mood chip `params` sampled on Refresh for genuinely varied YTM shelves.
    pub refresh_mood_params: Option<String>,
}

fn dominant_taste_locale(tracks: &[Track]) -> MusicLocale {
    if tracks.is_empty() {
        return MusicLocale::GlobalEn;
    }
    let mut en_w = 0usize;
    let mut id_w = 0usize;
    let mut jp_w = 0usize;
    let mut kr_w = 0usize;
    for (idx, t) in tracks.iter().take(24).enumerate() {
        let w = 24usize.saturating_sub(idx).max(1);
        match detect_track_locale(&t.title, &t.artist, &t.album) {
            MusicLocale::GlobalEn => en_w += w,
            MusicLocale::Indonesian => id_w += w,
            MusicLocale::Japanese => jp_w += w,
            MusicLocale::Korean => kr_w += w,
        }
    }
    [
        (MusicLocale::GlobalEn, en_w),
        (MusicLocale::Indonesian, id_w),
        (MusicLocale::Japanese, jp_w),
        (MusicLocale::Korean, kr_w),
    ]
    .into_iter()
    .max_by_key(|(_, w)| *w)
    .map_or(MusicLocale::GlobalEn, |(l, _)| l)
}

fn taste_includes_locale(taste: &YtmHomeTasteProfile, target: MusicLocale) -> bool {
    let mut hits = 0usize;
    for t in taste.top_tracks.iter().chain(taste.library_tracks.iter()) {
        if detect_track_locale(&t.title, &t.artist, &t.album) == target {
            hits += 1;
            if hits >= 2 {
                return true;
            }
        }
    }
    false
}

fn is_track_excluded(track: &Track, excluded: &HashSet<String>) -> bool {
    if excluded.is_empty() {
        return false;
    }
    if excluded.contains(&track.id) {
        return true;
    }
    if let Some(vid) = track.youtube_video_id.as_deref()
        && excluded.contains(vid)
    {
        return true;
    }
    false
}

fn primary_artist_norm(raw: &str) -> String {
    raw.split(['&', ',', '•', '·'])
        .next()
        .unwrap_or(raw)
        .split(" feat.")
        .next()
        .unwrap_or(raw)
        .split(" ft.")
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase()
}

fn is_regional_geoip_chart_shelf(
    title: &str,
    cards: &[YtmShelfCard],
    taste_locale: MusicLocale,
    allow_indonesian: bool,
) -> bool {
    if allow_indonesian {
        return false;
    }
    let tl = title.to_ascii_lowercase();
    if taste_locale == MusicLocale::GlobalEn {
        if tl.contains("indonesia") || tl.contains("indo ") || tl.contains("dangdut") {
            return true;
        }
        let id_cards = cards
            .iter()
            .filter(|c| {
                let ct = format!("{} {}", c.title, c.subtitle).to_ascii_lowercase();
                ct.contains("indonesia")
                    || ct.contains("indo")
                    || detect_track_locale(&c.title, &c.subtitle, "") == MusicLocale::Indonesian
            })
            .count();
        if !cards.is_empty() && id_cards * 2 >= cards.len() {
            return true;
        }
    }
    false
}

/// Rank collaborative co-listening candidates across multiple seed radio pools so tracks
/// loved by millions of listeners who share the user's exact listening taste rise to the top,
/// while enforcing artist diversity and multi-locale coherence.
pub fn rank_personalized_home_candidates(
    radio_pools: &[Vec<Track>],
    artist_popular: &[Track],
    taste: &YtmHomeTasteProfile,
    taste_locale: MusicLocale,
    max_per_artist: usize,
) -> Vec<Track> {
    rank_personalized_home_candidates_ex(
        radio_pools,
        artist_popular,
        taste,
        taste_locale,
        max_per_artist,
        false,
    )
}

fn rank_personalized_home_candidates_ex(
    radio_pools: &[Vec<Track>],
    artist_popular: &[Track],
    taste: &YtmHomeTasteProfile,
    taste_locale: MusicLocale,
    max_per_artist: usize,
    fresh_seed: bool,
) -> Vec<Track> {
    let allow_indonesian = taste_locale == MusicLocale::Indonesian
        || taste_includes_locale(taste, MusicLocale::Indonesian);
    let top_artist_norms: HashSet<String> = taste
        .top_artists
        .iter()
        .map(|a| primary_artist_norm(a))
        .filter(|a| !a.is_empty())
        .collect();
    let recent_ids: HashSet<&str> = taste.top_tracks.iter().map(|t| t.id.as_str()).collect();
    let heavy_top_ids: HashSet<&str> = taste
        .top_tracks
        .iter()
        .take(12)
        .map(|t| t.id.as_str())
        .collect();

    struct ScoredTrack {
        track: Track,
        score: i32,
        pool_hits: usize,
        excluded: bool,
    }
    let mut by_key: HashMap<String, ScoredTrack> = HashMap::new();

    for pool in radio_pools {
        let mut seen_in_pool = HashSet::new();
        for (rank, t) in pool.iter().enumerate() {
            if is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms) {
                continue;
            }
            if !allow_indonesian
                && taste_locale == MusicLocale::GlobalEn
                && detect_track_locale(&t.title, &t.artist, &t.album) == MusicLocale::Indonesian
            {
                continue;
            }
            let key = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
            if !seen_in_pool.insert(key.clone()) {
                continue;
            }
            let pos_score = 32_i32.saturating_sub((rank as i32) * 2).max(4);
            let artist_n = primary_artist_norm(&t.artist);
            let affinity_bonus = if top_artist_norms.contains(&artist_n) {
                12
            } else {
                0
            };
            let freshness_bonus = if heavy_top_ids.contains(t.id.as_str()) {
                if fresh_seed { -24 } else { 0 }
            } else if recent_ids.contains(t.id.as_str()) {
                if fresh_seed { -8 } else { 4 }
            } else if fresh_seed {
                22
            } else {
                12
            };
            let excluded = is_track_excluded(t, &taste.excluded_track_ids);
            let entry = by_key.entry(key).or_insert_with(|| ScoredTrack {
                track: t.clone(),
                score: if fresh_seed {
                    fastrand::i32(-18..=22)
                } else {
                    0
                },
                pool_hits: 0,
                excluded,
            });
            entry.pool_hits += 1;
            // On initial load reward consensus strongly; on manual Refresh cap consensus so viral hub
            // tracks don't drown out newly explored corners of the user's library.
            let consensus_bonus = if entry.pool_hits >= 2 {
                if fresh_seed { 14 } else { 44 }
            } else {
                0
            };
            entry.score += pos_score + affinity_bonus + freshness_bonus + consensus_bonus;
        }
    }

    for (rank, t) in artist_popular.iter().enumerate() {
        if is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms) {
            continue;
        }
        if !allow_indonesian
            && taste_locale == MusicLocale::GlobalEn
            && detect_track_locale(&t.title, &t.artist, &t.album) == MusicLocale::Indonesian
        {
            continue;
        }
        let key = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
        let pos_score = 22_i32.saturating_sub((rank as i32) * 2).max(4);
        let excluded = is_track_excluded(t, &taste.excluded_track_ids);
        let entry = by_key.entry(key).or_insert_with(|| ScoredTrack {
            track: t.clone(),
            score: if fresh_seed {
                fastrand::i32(-14..=18)
            } else {
                0
            },
            pool_hits: 0,
            excluded,
        });
        entry.score += pos_score;
    }

    let mut ranked: Vec<ScoredTrack> = by_key.into_values().collect();
    ranked.sort_by(|a, b| {
        a.excluded
            .cmp(&b.excluded)
            .then_with(|| b.score.cmp(&a.score))
            .then_with(|| b.pool_hits.cmp(&a.pool_hits))
    });

    let mut out = Vec::new();
    let mut artist_counts: HashMap<String, usize> = HashMap::new();
    for item in ranked {
        let an = primary_artist_norm(&item.track.artist);
        let cap = max_per_artist.max(1);
        let cnt = artist_counts.entry(an).or_insert(0);
        if *cnt >= cap {
            continue;
        }
        *cnt += 1;
        out.push(item.track);
    }
    out
}

/// Fast single-POST YouTube Music search used by Home discovery shelves (avoids the 2x HTTP cost
/// of full `fetch_rich_search` while still collecting playable tracks + Album/Playlist/Artist cards).
fn fetch_home_discovery_search(
    session: &YtmSession,
    query: &str,
    limit: usize,
) -> Option<YtmRichSearch> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let body = json!({
        "query": q,
        "context": web_remix_context(session),
    });
    let root = post_ytm_json_fast("search", &body, session).ok()?;
    let mut tracks = Vec::new();
    let mut seen_vids = HashSet::new();
    let mut cards = Vec::new();
    let mut seen_cards = HashSet::new();
    collect_search_cards(&root, 20, &mut seen_cards, &mut cards);
    collect_music_items(
        &root,
        "YouTube Music",
        limit.max(8),
        &mut seen_vids,
        &mut tracks,
    );
    if tracks.is_empty() && cards.is_empty() {
        None
    } else {
        Some(YtmRichSearch { tracks, cards })
    }
}

/// Fetch rich YouTube Music Home feed:
/// - Uses a fast 5–6 request parallel fan-out (caching `FEmusic_explore` across refreshes and walking
///   `FEmusic_home` continuations / rotating Mood Chips on manual Refresh).
/// - Samples stratified multi-cluster / multi-locale seeds, excludes recently displayed Quick Picks
///   and shelf cards on Refresh, preserves signed-in `raw_home_picks`, and interleaves real YTM
///   shelves directly into the 8 visible Home shelf slots.
pub fn fetch_rich_home(
    session: &YtmSession,
    mood_params: Option<&str>,
    fresh_seed: bool,
    taste: &YtmHomeTasteProfile,
) -> ProviderResult<YtmRichHome> {
    let effective_session = if !session.is_signed_in() && fresh_seed {
        YtmSession::guest(String::new())
    } else {
        session.clone()
    };

    let is_default_home = mood_params.map(str::trim).is_none_or(str::is_empty);
    let has_taste = !taste.top_tracks.is_empty()
        || !taste.library_tracks.is_empty()
        || !taste.top_artists.is_empty();
    let taste_locale = dominant_taste_locale(&taste.top_tracks);
    let allow_indonesian = taste_locale == MusicLocale::Indonesian
        || taste_includes_locale(taste, MusicLocale::Indonesian);
    let refresh_epoch = if fresh_seed {
        HOME_REFRESH_EPOCH.fetch_add(1, Ordering::Relaxed) + 1
    } else {
        HOME_REFRESH_EPOCH.load(Ordering::Relaxed)
    };

    // On manual Refresh of default Home, alternate between walking the previous `FEmusic_home`
    // continuation token and sampling a rotating Mood Chip so YTM returns genuinely new shelves!
    let mut home_used_continuation_or_mood = false;
    let body = if is_default_home && fresh_seed {
        if refresh_epoch.is_multiple_of(2)
            && let Some(mp) = taste
                .refresh_mood_params
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            home_used_continuation_or_mood = true;
            json!({
                "browseId": "FEmusic_home",
                "params": mp,
                "context": web_remix_context_for_locale(&effective_session, taste_locale),
            })
        } else if let Some(tok) = taste
            .continuation_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            home_used_continuation_or_mood = true;
            json!({
                "continuation": tok,
                "context": web_remix_context_for_locale(&effective_session, taste_locale),
            })
        } else {
            json!({
                "browseId": "FEmusic_home",
                "context": web_remix_context_for_locale(&effective_session, taste_locale),
            })
        }
    } else {
        let mut b = json!({
            "browseId": "FEmusic_home",
            "context": web_remix_context_for_locale(&effective_session, taste_locale),
        });
        if let Some(params) = mood_params.map(str::trim).filter(|p| !p.is_empty()) {
            b["params"] = json!(params);
        }
        b
    };

    // Build a deep, deduplicated artist list spanning top_artists, library_tracks, and recent_searches.
    let mut all_taste_artists: Vec<(String, f64)> = Vec::new();
    let mut seen_artist_norms: HashSet<String> = HashSet::new();
    for (idx, a) in taste.top_artists.iter().enumerate() {
        let trimmed = a.trim();
        let norm = primary_artist_norm(trimmed);
        if !trimmed.is_empty()
            && !trimmed.eq_ignore_ascii_case("unknown artist")
            && !norm.is_empty()
            && seen_artist_norms.insert(norm)
        {
            let w = (42.0 - (idx as f64) * 0.7).max(8.0);
            all_taste_artists.push((trimmed.to_string(), w));
        }
    }
    for t in taste.top_tracks.iter().chain(taste.library_tracks.iter()) {
        let a = primary_artist_display(&t.artist);
        let norm = primary_artist_norm(&a);
        if !a.is_empty()
            && !a.eq_ignore_ascii_case("unknown artist")
            && !norm.is_empty()
            && seen_artist_norms.insert(norm)
        {
            let w = if t.liked { 18.0 } else { 9.0 };
            all_taste_artists.push((a, w));
        }
    }

    // Weighted random sampling without replacement on Refresh (excluding recently featured artists)
    let chosen_artists: Vec<String> = if fresh_seed && all_taste_artists.len() > 2 {
        let mut scored: Vec<(bool, f64, String)> = all_taste_artists
            .iter()
            .map(|(name, w)| {
                let norm = primary_artist_norm(name);
                let excluded = taste.excluded_artists.contains(&norm);
                let u = fastrand::f64().clamp(1e-6, 1.0 - 1e-6);
                let key = u.powf(1.0 / w.max(1.0));
                (excluded, key, name.clone())
            })
            .collect();
        scored.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        });
        scored.into_iter().take(4).map(|(_, _, n)| n).collect()
    } else {
        all_taste_artists
            .iter()
            .take(4)
            .map(|(n, _)| n.clone())
            .collect()
    };

    // Stratified seed selection (3 genuinely distant seed tracks across Recent, Deep Library, and
    // Secondary Locale) so RDAMVM radios return ~60–75 fresh, non-echo-chamber tracks every Refresh!
    let mut seed_tracks: Vec<Track> = Vec::new();
    if is_default_home && has_taste {
        let mut used_artists: HashSet<String> = HashSet::new();
        let top_yt: Vec<&Track> = taste
            .top_tracks
            .iter()
            .filter(|t| t.youtube_video_id.as_deref().is_some_and(is_valid_video_id))
            .collect();
        let lib_yt: Vec<&Track> = taste
            .library_tracks
            .iter()
            .filter(|t| t.youtube_video_id.as_deref().is_some_and(is_valid_video_id))
            .collect();

        let pick_weighted_seed = |pool: &[&Track],
                                  used: &mut HashSet<String>,
                                  prefer_secondary_locale: bool|
         -> Option<Track> {
            let mut cands: Vec<(bool, bool, f64, &Track)> = Vec::new();
            for (i, &t) in pool.iter().enumerate() {
                let an = primary_artist_norm(&t.artist);
                if !an.is_empty() && used.contains(&an) {
                    continue;
                }
                let loc = detect_track_locale(&t.title, &t.artist, &t.album);
                if prefer_secondary_locale && loc == taste_locale {
                    continue;
                }
                let artist_excl = fresh_seed && taste.excluded_artists.contains(&an);
                let track_excl = fresh_seed && is_track_excluded(t, &taste.excluded_track_ids);
                let base_w = if fresh_seed {
                    let decay = (28.0 - (i as f64) * 0.35).max(6.0);
                    if t.liked { decay + 10.0 } else { decay }
                } else {
                    (100.0 - (i as f64) * 2.0).max(5.0)
                };
                let key = if fresh_seed {
                    fastrand::f64().clamp(1e-6, 1.0 - 1e-6).powf(1.0 / base_w)
                } else {
                    base_w
                };
                cands.push((artist_excl, track_excl, key, t));
            }
            cands.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then_with(|| a.1.cmp(&b.1))
                    .then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
            });
            let chosen = cands.into_iter().next().map(|(_, _, _, t)| t.clone())?;
            let an = primary_artist_norm(&chosen.artist);
            if !an.is_empty() {
                used.insert(an);
            }
            Some(chosen)
        };

        // Seed 0: active taste / recent session
        if let Some(s0) = pick_weighted_seed(&top_yt, &mut used_artists, false) {
            seed_tracks.push(s0);
        }
        // Seed 1: deeper library track (falls back to top_yt if library_tracks has no YT IDs)
        let deep_pool = if !lib_yt.is_empty() { &lib_yt } else { &top_yt };
        if let Some(s1) = pick_weighted_seed(deep_pool, &mut used_artists, false)
            .or_else(|| pick_weighted_seed(&top_yt, &mut used_artists, false))
        {
            seed_tracks.push(s1);
        }
        // Seed 2: secondary active locale (e.g. Indonesian / Japanese) for true multi-locale variety,
        // falling back to another distant library/top artist
        let mut combined_yt: Vec<&Track> = Vec::with_capacity(lib_yt.len() + top_yt.len());
        combined_yt.extend_from_slice(&lib_yt);
        combined_yt.extend_from_slice(&top_yt);
        if let Some(s2) = pick_weighted_seed(&combined_yt, &mut used_artists, true)
            .or_else(|| pick_weighted_seed(&combined_yt, &mut used_artists, false))
        {
            seed_tracks.push(s2);
        }
    }

    let artist_1 = chosen_artists.first().cloned();
    let artist_2 = chosen_artists.get(1).cloned();

    const ALBUM_SUFFIXES: [&str; 4] = ["albums", "album", "discography", "new album"];
    const MIX_SUFFIXES: [&str; 4] = [
        "playlist mix",
        "essentials",
        "similar artists",
        "best songs",
    ];
    let album_suf = ALBUM_SUFFIXES[refresh_epoch % ALBUM_SUFFIXES.len()];
    let mix_suf = MIX_SUFFIXES[(refresh_epoch + 1) % MIX_SUFFIXES.len()];

    // Check 30-minute session cache for `FEmusic_explore` to avoid redundant HTTP roundtrips on Refresh
    let cached_explore: Option<Vec<YtmHomeShelf>> = {
        let cell = CACHED_EXPLORE_SHELVES.get_or_init(|| Mutex::new(None));
        let guard = cell.lock().unwrap_or_else(|p| p.into_inner());
        guard.as_ref().and_then(|(ts, shelves)| {
            (ts.elapsed() < Duration::from_secs(1800) && !shelves.is_empty())
                .then(|| shelves.clone())
        })
    };
    let need_explore_fetch = is_default_home && cached_explore.is_none();

    // Lean parallel fan-out (5–6 HTTP POSTs instead of 11):
    // 1x Home/Continuation/Mood + optional cached Explore + 3x diverse RDAMVM radios + 2x fast searches
    let (home_res, explore_res, charts_res, radio_pools, artist_search_1, artist_search_2) =
        if is_default_home && has_taste {
            let s0 = seed_tracks.first().cloned();
            let s1 = seed_tracks.get(1).cloned();
            let s2 = seed_tracks.get(2).cloned();
            let a1_q = artist_1.as_ref().map(|a| format!("{a} {album_suf}"));
            let a2_q = if fresh_seed && !taste.recent_searches.is_empty() && refresh_epoch % 3 == 2
            {
                let q = &taste.recent_searches[refresh_epoch % taste.recent_searches.len()];
                Some(q.clone())
            } else {
                artist_2.as_ref().map(|a| format!("{a} {mix_suf}"))
            };
            std::thread::scope(|s| {
                let h_home = s.spawn(|| post_ytm_json_fast("browse", &body, &effective_session));
                let h_explore = s.spawn(|| {
                    if !need_explore_fetch {
                        return None;
                    }
                    let sup_body = json!({
                        "browseId": "FEmusic_explore",
                        "context": web_remix_context_for_locale(&effective_session, taste_locale),
                    });
                    post_ytm_json_fast("browse", &sup_body, &effective_session).ok()
                });
                let h_r0 = s.spawn(|| {
                    s0.as_ref().and_then(|t| {
                        let vid = t.youtube_video_id.as_deref()?;
                        fetch_radio_up_next_inner(
                            &effective_session,
                            vid,
                            &t.title,
                            &t.artist,
                            &t.album,
                            24,
                            fresh_seed,
                            false,
                        )
                        .ok()
                    })
                });
                let h_r1 = s.spawn(|| {
                    s1.as_ref().and_then(|t| {
                        let vid = t.youtube_video_id.as_deref()?;
                        fetch_radio_up_next_inner(
                            &effective_session,
                            vid,
                            &t.title,
                            &t.artist,
                            &t.album,
                            24,
                            fresh_seed,
                            false,
                        )
                        .ok()
                    })
                });
                let h_r2 = s.spawn(|| {
                    s2.as_ref().and_then(|t| {
                        let vid = t.youtube_video_id.as_deref()?;
                        fetch_radio_up_next_inner(
                            &effective_session,
                            vid,
                            &t.title,
                            &t.artist,
                            &t.album,
                            22,
                            fresh_seed,
                            false,
                        )
                        .ok()
                    })
                });
                let h_a1 = s.spawn(|| {
                    a1_q.as_deref()
                        .and_then(|q| fetch_home_discovery_search(&effective_session, q, 16))
                });
                let h_a2 = s.spawn(|| {
                    a2_q.as_deref()
                        .and_then(|q| fetch_home_discovery_search(&effective_session, q, 16))
                });
                let mut pools = Vec::new();
                for res in [
                    h_r0.join().ok().flatten(),
                    h_r1.join().ok().flatten(),
                    h_r2.join().ok().flatten(),
                ]
                .into_iter()
                .flatten()
                {
                    if !res.is_empty() {
                        pools.push(res);
                    }
                }
                (
                    h_home.join().unwrap_or_else(|_| {
                        Err(ProviderError::Http(
                            "YouTube Music Home thread failed".to_string(),
                        ))
                    }),
                    h_explore.join().ok().flatten(),
                    None,
                    pools,
                    h_a1.join().ok().flatten(),
                    h_a2.join().ok().flatten(),
                )
            })
        } else if is_default_home {
            std::thread::scope(|s| {
                let h_home = s.spawn(|| post_ytm_json("browse", &body, &effective_session));
                let h_explore = s.spawn(|| {
                    if !need_explore_fetch {
                        return None;
                    }
                    let sup_body = json!({
                        "browseId": "FEmusic_explore",
                        "context": web_remix_context_for_locale(&effective_session, taste_locale),
                    });
                    post_ytm_json_fast("browse", &sup_body, &effective_session).ok()
                });
                let h_charts = s.spawn(|| {
                    let sup_body = json!({
                        "browseId": "FEmusic_charts",
                        "context": web_remix_context_for_locale(&effective_session, taste_locale),
                    });
                    post_ytm_json_fast("browse", &sup_body, &effective_session).ok()
                });
                (
                    h_home.join().unwrap_or_else(|_| {
                        Err(ProviderError::Http(
                            "YouTube Music Home thread failed".to_string(),
                        ))
                    }),
                    h_explore.join().ok().flatten(),
                    h_charts.join().ok().flatten(),
                    Vec::new(),
                    None,
                    None,
                )
            })
        } else {
            (
                post_ytm_json("browse", &body, &effective_session),
                None,
                None,
                Vec::new(),
                None,
                None,
            )
        };

    let home_root = match home_res {
        Ok(root) => Some(root),
        Err(err) => {
            if explore_res.is_none()
                && charts_res.is_none()
                && cached_explore.is_none()
                && radio_pools.is_empty()
                && artist_search_1.is_none()
            {
                return Err(err);
            }
            None
        }
    };

    let mut visitor_data = effective_session.visitor_data.clone();
    let mut chips = Vec::new();
    let mut raw_home_picks = Vec::new();
    let mut seen_tracks = HashSet::new();
    let mut raw_home_shelves = Vec::new();
    let mut all_tracks: Vec<Track> = Vec::new();
    let mut continuation_token = None;

    if let Some(root) = home_root {
        visitor_data = extract_visitor_data(&root, &visitor_data);
        collect_mood_chips(&root, &mut chips);
        collect_home_shelves(
            &root,
            &mut raw_home_picks,
            &mut seen_tracks,
            &mut raw_home_shelves,
        );
        continuation_token = extract_continuation_token(&root);
    }
    // If continuation/mood returned no new continuation token, reset so next refresh starts fresh
    if continuation_token.is_none() && home_used_continuation_or_mood {
        continuation_token = None;
    }

    let mut next_sess = effective_session;
    next_sess.visitor_data = visitor_data.clone();

    let mut explore_shelves: Vec<YtmHomeShelf> = cached_explore.unwrap_or_default();
    let mut fetched_fresh_explore = false;
    for sup_root in [explore_res, charts_res].into_iter().flatten() {
        if visitor_data.is_empty() {
            visitor_data = extract_visitor_data(&sup_root, &visitor_data);
        }
        if chips.is_empty() {
            collect_mood_chips(&sup_root, &mut chips);
        }
        let mut sup_picks = Vec::new();
        let mut sup_shelves = Vec::new();
        collect_home_shelves(
            &sup_root,
            &mut sup_picks,
            &mut seen_tracks,
            &mut sup_shelves,
        );
        for t in sup_picks {
            if raw_home_picks.len() < 48 && !raw_home_picks.iter().any(|q| q.id == t.id) {
                raw_home_picks.push(t);
            }
        }
        for shelf in sup_shelves {
            if !explore_shelves
                .iter()
                .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
            {
                explore_shelves.push(shelf);
                fetched_fresh_explore = true;
            }
        }
    }
    if fetched_fresh_explore && !explore_shelves.is_empty() {
        let cell = CACHED_EXPLORE_SHELVES.get_or_init(|| Mutex::new(None));
        if let Ok(mut guard) = cell.lock() {
            *guard = Some((Instant::now(), explore_shelves.clone()));
        }
    }

    if !has_taste || !is_default_home {
        for _ in 0..2 {
            let Some(tok) = continuation_token.clone() else {
                break;
            };
            let cont_body = json!({
                "continuation": tok,
                "context": web_remix_context_for_locale(&next_sess, taste_locale),
            });
            let Ok(cont_root) = post_ytm_json_fast("browse", &cont_body, &next_sess) else {
                break;
            };
            let mut c_picks = Vec::new();
            let mut c_shelves = Vec::new();
            collect_home_shelves(&cont_root, &mut c_picks, &mut seen_tracks, &mut c_shelves);
            for t in c_picks {
                if raw_home_picks.len() < 40 && !raw_home_picks.iter().any(|q| q.id == t.id) {
                    raw_home_picks.push(t);
                }
            }
            for shelf in c_shelves {
                if !raw_home_shelves
                    .iter()
                    .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
                {
                    raw_home_shelves.push(shelf);
                }
            }
            continuation_token = extract_continuation_token(&cont_root);
        }

        for shelf in explore_shelves.iter().cloned() {
            if !raw_home_shelves
                .iter()
                .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
            {
                raw_home_shelves.push(shelf);
            }
        }
    }

    let mut quick_picks: Vec<Track> = Vec::new();
    let mut shelves: Vec<YtmHomeShelf> = Vec::new();

    if is_default_home
        && has_taste
        && (!radio_pools.is_empty()
            || artist_search_1.is_some()
            || artist_search_2.is_some()
            || !raw_home_picks.is_empty())
    {
        let mut artist_popular_tracks: Vec<Track> = Vec::new();
        let mut album_cards: Vec<YtmShelfCard> = Vec::new();
        let mut playlist_cards: Vec<YtmShelfCard> = Vec::new();
        let mut artist_cards: Vec<YtmShelfCard> = Vec::new();
        let mut seen_card_ids: HashSet<String> = HashSet::new();

        for search_opt in [artist_search_1.as_ref(), artist_search_2.as_ref()]
            .into_iter()
            .flatten()
        {
            for t in search_opt.tracks.iter().take(14) {
                if !artist_popular_tracks.iter().any(|x| x.id == t.id) {
                    artist_popular_tracks.push(t.clone());
                }
            }
            for c in &search_opt.cards {
                if !seen_card_ids.insert(c.id.clone()) {
                    continue;
                }
                match c.kind {
                    YtmCardKind::Album => album_cards.push(c.clone()),
                    YtmCardKind::Playlist => playlist_cards.push(c.clone()),
                    YtmCardKind::Artist => artist_cards.push(c.clone()),
                    YtmCardKind::Song => {}
                }
            }
        }

        // Gather ALL Album/Playlist/Artist cards from raw_home_shelves + Explore BEFORE capping so
        // shuffling and excluded-card filtering draw from the entire catalog instead of only 15 items.
        for shelf in raw_home_shelves.iter().chain(explore_shelves.iter()) {
            if is_regional_geoip_chart_shelf(
                &shelf.title,
                &shelf.cards,
                taste_locale,
                allow_indonesian,
            ) {
                continue;
            }
            for c in &shelf.cards {
                if !seen_card_ids.insert(c.id.clone()) {
                    continue;
                }
                match c.kind {
                    YtmCardKind::Album => album_cards.push(c.clone()),
                    YtmCardKind::Playlist => playlist_cards.push(c.clone()),
                    YtmCardKind::Artist => artist_cards.push(c.clone()),
                    YtmCardKind::Song => {}
                }
            }
        }

        let prioritize_fresh_cards = |cards: &mut Vec<YtmShelfCard>| {
            if fresh_seed && cards.len() > 2 {
                fastrand::shuffle(cards);
            }
            if !taste.excluded_card_ids.is_empty() {
                cards.sort_by_key(|c| taste.excluded_card_ids.contains(&c.id));
            }
        };
        prioritize_fresh_cards(&mut album_cards);
        prioritize_fresh_cards(&mut playlist_cards);
        prioritize_fresh_cards(&mut artist_cards);

        let ranked_discovery = rank_personalized_home_candidates_ex(
            &radio_pools,
            &artist_popular_tracks,
            taste,
            taste_locale,
            if fresh_seed { 1 } else { 2 },
            fresh_seed,
        );

        // Clean & shuffle signed-in YTM Home / continuation / mood picks so they actually enter Quick Picks!
        let mut ytm_home_pool: Vec<Track> = raw_home_picks
            .iter()
            .filter(|t| !is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms))
            .filter(|t| {
                allow_indonesian
                    || taste_locale != MusicLocale::GlobalEn
                    || detect_track_locale(&t.title, &t.artist, &t.album) != MusicLocale::Indonesian
            })
            .cloned()
            .collect();
        if fresh_seed && ytm_home_pool.len() > 2 {
            fastrand::shuffle(&mut ytm_home_pool);
        }
        ytm_home_pool.sort_by_key(|t| is_track_excluded(t, &taste.excluded_track_ids));

        // Familiar / library rediscovery pool drawn from BOTH top_tracks and deeper library_tracks
        let mut fav_pool: Vec<Track> = Vec::new();
        let mut fav_seen: HashSet<String> = HashSet::new();
        for t in taste.library_tracks.iter().chain(taste.top_tracks.iter()) {
            if t.youtube_video_id.is_some()
                && !is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms)
                && fav_seen.insert(t.id.clone())
            {
                fav_pool.push(t.clone());
            }
        }
        if fresh_seed && fav_pool.len() > 2 {
            fastrand::shuffle(&mut fav_pool);
        }
        fav_pool.sort_by_key(|t| is_track_excluded(t, &taste.excluded_track_ids));

        // Quick Picks (32 songs = 4 pages):
        // Page 1 (first 8 slots) enforces:
        //   - 0 overlap with `excluded_track_ids` (unless pool exhausted)
        //   - max 1 track per primary artist across all 8 visible tiles
        //   - 6 fresh discovery tracks (interleaving diverse RDAMVM radios + YTM Home) + 2 library rediscovery
        let mut qp_seen: HashSet<String> = HashSet::new();
        let mut page1_artists: HashSet<String> = HashSet::new();
        let mut disc_fresh: std::collections::VecDeque<Track> = ranked_discovery
            .iter()
            .filter(|t| !is_track_excluded(t, &taste.excluded_track_ids))
            .cloned()
            .collect();
        let mut ytm_fresh: std::collections::VecDeque<Track> = ytm_home_pool
            .iter()
            .filter(|t| !is_track_excluded(t, &taste.excluded_track_ids))
            .cloned()
            .collect();
        let mut fav_fresh: std::collections::VecDeque<Track> = fav_pool
            .iter()
            .filter(|t| !is_track_excluded(t, &taste.excluded_track_ids))
            .cloned()
            .collect();

        let pop_diverse = |q: &mut std::collections::VecDeque<Track>,
                           seen_keys: &mut HashSet<String>,
                           seen_art: &mut HashSet<String>,
                           enforce_unique_artist: bool|
         -> Option<Track> {
            let mut deferred = Vec::new();
            let mut picked = None;
            while let Some(t) = q.pop_front() {
                let k = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
                if seen_keys.contains(&k) {
                    continue;
                }
                let an = primary_artist_norm(&t.artist);
                if enforce_unique_artist && !an.is_empty() && seen_art.contains(&an) {
                    deferred.push(t);
                    continue;
                }
                seen_keys.insert(k);
                if !an.is_empty() {
                    seen_art.insert(an);
                }
                picked = Some(t);
                break;
            }
            for t in deferred.into_iter().rev() {
                q.push_front(t);
            }
            picked
        };

        // Fill Page 1 (8 tiles): slots 0, 4 = familiar/rediscovery; slots 2, 6 = YTM Home picks; rest = radio discovery
        for slot in 0..8 {
            let chosen = match slot {
                0 | 4 => pop_diverse(&mut fav_fresh, &mut qp_seen, &mut page1_artists, true)
                    .or_else(|| {
                        pop_diverse(&mut disc_fresh, &mut qp_seen, &mut page1_artists, true)
                    })
                    .or_else(|| {
                        pop_diverse(&mut ytm_fresh, &mut qp_seen, &mut page1_artists, true)
                    }),
                2 | 6 => pop_diverse(&mut ytm_fresh, &mut qp_seen, &mut page1_artists, true)
                    .or_else(|| {
                        pop_diverse(&mut disc_fresh, &mut qp_seen, &mut page1_artists, true)
                    })
                    .or_else(|| {
                        pop_diverse(&mut fav_fresh, &mut qp_seen, &mut page1_artists, true)
                    }),
                _ => pop_diverse(&mut disc_fresh, &mut qp_seen, &mut page1_artists, true)
                    .or_else(|| pop_diverse(&mut ytm_fresh, &mut qp_seen, &mut page1_artists, true))
                    .or_else(|| {
                        pop_diverse(&mut fav_fresh, &mut qp_seen, &mut page1_artists, true)
                    }),
            };
            if let Some(t) = chosen {
                quick_picks.push(t);
            }
        }

        // Fill pages 2..4 (up to 32 tracks) with remaining fresh tracks first, then fallback pool
        let mut fallback_art = HashSet::new();
        while quick_picks.len() < 32 {
            let next = pop_diverse(&mut disc_fresh, &mut qp_seen, &mut fallback_art, false)
                .or_else(|| pop_diverse(&mut ytm_fresh, &mut qp_seen, &mut fallback_art, false))
                .or_else(|| pop_diverse(&mut fav_fresh, &mut qp_seen, &mut fallback_art, false));
            match next {
                Some(t) => quick_picks.push(t),
                None => break,
            }
        }
        if quick_picks.len() < 16 {
            for t in ranked_discovery
                .iter()
                .chain(ytm_home_pool.iter())
                .chain(fav_pool.iter())
            {
                if quick_picks.len() >= 32 {
                    break;
                }
                let k = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
                if qp_seen.insert(k) {
                    quick_picks.push(t.clone());
                }
            }
        }

        // Real personalized YTM Home / Continuation / Mood shelves (previously buried past slot 8!)
        let mut real_ytm_shelves: Vec<YtmHomeShelf> = raw_home_shelves
            .into_iter()
            .filter(|s| {
                s.cards.len() >= 3
                    && !is_regional_geoip_chart_shelf(
                        &s.title,
                        &s.cards,
                        taste_locale,
                        allow_indonesian,
                    )
            })
            .map(|mut s| {
                prioritize_fresh_cards(&mut s.cards);
                s
            })
            .collect();
        if fresh_seed && real_ytm_shelves.len() > 2 {
            fastrand::shuffle(&mut real_ytm_shelves);
        }

        let push_shelf_unique = |out: &mut Vec<YtmHomeShelf>, shelf: YtmHomeShelf| {
            if !shelf.cards.is_empty()
                && !out
                    .iter()
                    .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
            {
                out.push(shelf);
            }
        };

        // Shelf 0: Recommended Albums for You (unseen cards prioritized)
        if album_cards.len() >= 3 {
            push_shelf_unique(
                &mut shelves,
                YtmHomeShelf {
                    title: "Recommended albums for you".to_string(),
                    cards: album_cards.into_iter().take(15).collect(),
                },
            );
        }

        // Shelf 1: Popular with Listeners Like You (15 fresh unseen songs, zero overlap with first 8 Quick Picks)
        let first_qp_ids: HashSet<&str> =
            quick_picks.iter().take(8).map(|t| t.id.as_str()).collect();
        let top_ids: HashSet<&str> = taste
            .top_tracks
            .iter()
            .take(12)
            .map(|t| t.id.as_str())
            .collect();
        let mut popular_discovery: Vec<Track> = ranked_discovery
            .iter()
            .filter(|t| {
                !first_qp_ids.contains(t.id.as_str())
                    && !top_ids.contains(t.id.as_str())
                    && !is_track_excluded(t, &taste.excluded_track_ids)
            })
            .take(15)
            .cloned()
            .collect();
        if popular_discovery.len() < 6 {
            for t in ranked_discovery.iter().chain(ytm_home_pool.iter()) {
                if popular_discovery.len() >= 15 {
                    break;
                }
                if !first_qp_ids.contains(t.id.as_str())
                    && !popular_discovery.iter().any(|x| x.id == t.id)
                {
                    popular_discovery.push(t.clone());
                }
            }
        }
        let pop_cards = tracks_to_song_cards(&popular_discovery);
        if pop_cards.len() >= 4 {
            push_shelf_unique(
                &mut shelves,
                YtmHomeShelf {
                    title: "Popular with listeners like you".to_string(),
                    cards: pop_cards,
                },
            );
        }

        // Interleave Real YTM Home / Continuation / Mood Shelf #1 into the visible fold!
        let mut ytm_shelf_iter = real_ytm_shelves.into_iter();
        if let Some(real1) = ytm_shelf_iter.next() {
            push_shelf_unique(&mut shelves, real1);
        }

        // Because You Listen to <Weighted-Random Artist 1>
        if let Some(a1) = &artist_1 {
            let mut artist_mix: Vec<Track> = Vec::new();
            if let Some(first_pool) = radio_pools.first() {
                for t in first_pool {
                    if !first_qp_ids.contains(t.id.as_str())
                        && !artist_mix.iter().any(|x| x.id == t.id)
                    {
                        artist_mix.push(t.clone());
                    }
                }
            }
            if let Some(s1) = artist_search_1.as_ref() {
                for t in &s1.tracks {
                    if !first_qp_ids.contains(t.id.as_str())
                        && !artist_mix.iter().any(|x| x.id == t.id)
                    {
                        artist_mix.push(t.clone());
                    }
                }
            }
            if fresh_seed && artist_mix.len() > 3 {
                fastrand::shuffle(&mut artist_mix);
            }
            let a1_cards =
                tracks_to_song_cards(&artist_mix.into_iter().take(15).collect::<Vec<_>>());
            if a1_cards.len() >= 4 {
                push_shelf_unique(
                    &mut shelves,
                    YtmHomeShelf {
                        title: format!("Because you listen to {a1}"),
                        cards: a1_cards,
                    },
                );
            }
        }

        // Curated Playlists & Mixes (unseen shuffled playlists)
        if playlist_cards.len() >= 3 {
            push_shelf_unique(
                &mut shelves,
                YtmHomeShelf {
                    title: "Playlists & mixes for you".to_string(),
                    cards: playlist_cards.into_iter().take(15).collect(),
                },
            );
        }

        // Interleave Real YTM Home / Continuation / Mood Shelf #2 into visible slots!
        if let Some(real2) = ytm_shelf_iter.next() {
            push_shelf_unique(&mut shelves, real2);
        }

        // Artists You Might Like (circular Artist cards)
        if artist_cards.len() >= 3 {
            push_shelf_unique(
                &mut shelves,
                YtmHomeShelf {
                    title: "Artists you might like".to_string(),
                    cards: artist_cards.into_iter().take(15).collect(),
                },
            );
        }

        // More Like <Weighted-Random Artist 2>
        if let Some(a2) = &artist_2 {
            let mut mix2: Vec<Track> = Vec::new();
            if let Some(second_pool) = radio_pools.get(1) {
                for t in second_pool {
                    if !first_qp_ids.contains(t.id.as_str()) && !mix2.iter().any(|x| x.id == t.id) {
                        mix2.push(t.clone());
                    }
                }
            }
            if let Some(s2) = artist_search_2.as_ref() {
                for t in &s2.tracks {
                    if !first_qp_ids.contains(t.id.as_str()) && !mix2.iter().any(|x| x.id == t.id) {
                        mix2.push(t.clone());
                    }
                }
            }
            if fresh_seed && mix2.len() > 3 {
                fastrand::shuffle(&mut mix2);
            }
            let a2_cards = tracks_to_song_cards(&mix2.into_iter().take(15).collect::<Vec<_>>());
            if a2_cards.len() >= 4 {
                push_shelf_unique(
                    &mut shelves,
                    YtmHomeShelf {
                        title: format!("More like {a2}"),
                        cards: a2_cards,
                    },
                );
            }
        }

        // Rediscover Your Library (samples across ALL library_tracks + top_tracks, not just top 60!)
        let mut listen_pool: Vec<Track> = fav_pool;
        if fresh_seed && listen_pool.len() > 4 {
            fastrand::shuffle(&mut listen_pool);
        }
        listen_pool.sort_by_key(|t| {
            first_qp_ids.contains(t.id.as_str()) || is_track_excluded(t, &taste.excluded_track_ids)
        });
        let listen_again_cards = tracks_to_song_cards(&listen_pool[..listen_pool.len().min(15)]);
        if listen_again_cards.len() >= 3 {
            push_shelf_unique(
                &mut shelves,
                YtmHomeShelf {
                    title: if fresh_seed && refresh_epoch % 2 == 1 {
                        "Rediscover your library".to_string()
                    } else {
                        "Listen again".to_string()
                    },
                    cards: listen_again_cards,
                },
            );
        }

        // Append remaining YTM Home + Explore shelves for Load More rotation
        for shelf in ytm_shelf_iter.chain(explore_shelves) {
            if is_regional_geoip_chart_shelf(
                &shelf.title,
                &shelf.cards,
                taste_locale,
                allow_indonesian,
            ) {
                continue;
            }
            push_shelf_unique(&mut shelves, shelf);
        }

        // Aggregate all_tracks cleanly with O(1) HashSet deduplication (`coll-set-membership`)
        let mut all_seen: HashSet<String> = all_tracks.iter().map(|a| a.id.clone()).collect();
        for t in quick_picks
            .iter()
            .chain(ranked_discovery.iter())
            .chain(artist_popular_tracks.iter())
        {
            if all_seen.insert(t.id.clone()) {
                all_tracks.push(t.clone());
            }
        }
    } else {
        // Cold-start or Mood-chip view
        quick_picks = raw_home_picks;
        shelves = raw_home_shelves;
        if taste_locale == MusicLocale::GlobalEn && has_taste && !allow_indonesian {
            shelves.retain(|s| {
                !is_regional_geoip_chart_shelf(&s.title, &s.cards, taste_locale, allow_indonesian)
            });
        }
        all_tracks.extend(quick_picks.iter().cloned());
        if (fresh_seed || !session.is_signed_in()) && quick_picks.len() > 4 {
            fastrand::shuffle(&mut quick_picks);
            if !taste.excluded_track_ids.is_empty() {
                quick_picks.sort_by_key(|t| is_track_excluded(t, &taste.excluded_track_ids));
            }
        }
        if fresh_seed && shelves.len() > 2 {
            let rot = refresh_epoch % shelves.len();
            shelves.rotate_left(rot);
            for s in &mut shelves {
                if s.cards.len() > 2 {
                    fastrand::shuffle(&mut s.cards);
                }
            }
        }
    }

    if quick_picks.len() < 12 {
        for t in &all_tracks {
            if quick_picks.len() >= 24 {
                break;
            }
            if !quick_picks.iter().any(|q| q.id == t.id) {
                quick_picks.push(t.clone());
            }
        }
    }

    Ok(YtmRichHome {
        visitor_data,
        chips,
        quick_picks,
        shelves,
        all_tracks,
        continuation_token,
    })
}

/// Load additional endless recommendation shelves using a `continuation` token PLUS
/// personalized deep-catalog album/playlist/artist expansion so exploration never runs dry.
pub fn fetch_home_continuation(
    session: &YtmSession,
    continuation: Option<&str>,
    fallback_mood_params: Option<&str>,
    taste: &YtmHomeTasteProfile,
) -> ProviderResult<YtmRichHome> {
    let epoch = HOME_MORE_EPOCH.fetch_add(1, Ordering::Relaxed);
    let body = if let Some(tok) = continuation.map(str::trim).filter(|s| !s.is_empty()) {
        json!({
            "continuation": tok,
            "context": web_remix_context(session),
        })
    } else if let Some(params) = fallback_mood_params
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        json!({
            "browseId": "FEmusic_home",
            "params": params,
            "context": web_remix_context(session),
        })
    } else {
        json!({
            "browseId": "FEmusic_explore",
            "context": web_remix_context(session),
        })
    };

    // Pick 2 fresh taste artists (or curated genre queries on cold start) for endless personalized shelves
    let deep_queries: (String, String, String, String) = if !taste.top_artists.is_empty() {
        let n = taste.top_artists.len();
        let a1 = &taste.top_artists[(epoch * 2 + 2) % n];
        let a2 = &taste.top_artists[(epoch * 2 + 3) % n];
        let suf1 = ["albums", "playlist", "essentials", "deep cuts"][epoch % 4];
        let suf2 = ["similar artists", "mix", "top songs", "radio"][(epoch + 1) % 4];
        (
            format!("{a1} {suf1}"),
            format!("Explore {a1} & albums"),
            format!("{a2} {suf2}"),
            format!("Vibes like {a2}"),
        )
    } else {
        let presets = [
            (
                "indie pop albums",
                "Indie & alt albums",
                "chill r&b playlist",
                "Late-night R&B mixes",
            ),
            (
                "electronic synthwave albums",
                "Electronic & synth",
                "acoustic folk playlist",
                "Acoustic & stripped",
            ),
            (
                "j-pop anime hits",
                "J-Pop & anime",
                "pop indonesia indie",
                "Indonesian indie gems",
            ),
            (
                "hip hop classic albums",
                "Essential hip-hop",
                "dream pop shoegaze",
                "Dream pop & atmospheric",
            ),
        ];
        let p = presets[epoch % presets.len()];
        (
            p.0.to_string(),
            p.1.to_string(),
            p.2.to_string(),
            p.3.to_string(),
        )
    };

    let (cont_res, extra_1, extra_2) = std::thread::scope(|s| {
        let h_cont = s.spawn(|| post_ytm_json("browse", &body, session));
        let h_e1 = s.spawn(|| fetch_rich_search(session, &deep_queries.0, 14).ok());
        let h_e2 = s.spawn(|| fetch_rich_search(session, &deep_queries.2, 14).ok());
        (
            h_cont.join().unwrap_or_else(|_| {
                Err(ProviderError::Http(
                    "Continuation thread failed".to_string(),
                ))
            }),
            h_e1.join().ok().flatten(),
            h_e2.join().ok().flatten(),
        )
    });

    let mut visitor_data = session.visitor_data.clone();
    let mut quick_picks = Vec::new();
    let mut seen_tracks = HashSet::new();
    let mut shelves = Vec::new();
    let mut all_tracks = Vec::new();
    let mut next_tok = None;

    if let Ok(root) = cont_res {
        visitor_data = extract_visitor_data(&root, &session.visitor_data);
        collect_home_shelves(&root, &mut quick_picks, &mut seen_tracks, &mut shelves);
        all_tracks.extend(quick_picks.iter().cloned());
        collect_music_items(
            &root,
            "YouTube Music",
            48,
            &mut seen_tracks,
            &mut all_tracks,
        );
        next_tok = extract_continuation_token(&root);
    }

    for (res_opt, shelf_title) in [(extra_1, deep_queries.1), (extra_2, deep_queries.3)] {
        if let Some(res) = res_opt {
            let mut cards: Vec<YtmShelfCard> = res
                .cards
                .into_iter()
                .filter(|c| {
                    matches!(
                        c.kind,
                        YtmCardKind::Album | YtmCardKind::Playlist | YtmCardKind::Artist
                    )
                })
                .take(12)
                .collect();
            if cards.len() < 5 {
                let song_cards = tracks_to_song_cards(&res.tracks);
                for sc in song_cards {
                    if !cards.iter().any(|c| c.id == sc.id) {
                        cards.push(sc);
                    }
                    if cards.len() >= 12 {
                        break;
                    }
                }
            }
            if cards.len() >= 3 {
                shelves.insert(
                    0,
                    YtmHomeShelf {
                        title: shelf_title,
                        cards,
                    },
                );
            }
            for t in res.tracks.into_iter().take(10) {
                if !quick_picks.iter().any(|q| q.id == t.id) {
                    quick_picks.push(t.clone());
                }
                if !all_tracks.iter().any(|a| a.id == t.id) {
                    all_tracks.push(t);
                }
            }
        }
    }

    if shelves.is_empty() && quick_picks.is_empty() {
        return Err(ProviderError::Http(
            "No additional recommendations returned".to_string(),
        ));
    }

    Ok(YtmRichHome {
        visitor_data,
        chips: Vec::new(),
        quick_picks,
        shelves,
        all_tracks,
        continuation_token: next_tok,
    })
}

/// Fetch full tracklist and header metadata for any YouTube Music Album, Playlist, or Artist `browseId`.
pub fn fetch_browse_detail(
    session: &YtmSession,
    browse_id: &str,
) -> ProviderResult<YtmBrowseDetail> {
    let bid = browse_id.trim();
    if bid.is_empty() {
        return Err(ProviderError::Metadata("Empty browseId".to_string()));
    }
    let body = json!({
        "browseId": bid,
        "context": web_remix_context(session),
    });
    let root = post_ytm_json("browse", &body, session)?;

    let (title, subtitle, description, header_thumb) = extract_browse_header(&root);
    let resolved_title = if title.is_empty() {
        "YouTube Music".to_string()
    } else {
        title
    };

    let mut tracks = Vec::new();
    let mut seen = HashSet::new();
    collect_music_items(&root, &resolved_title, 80, &mut seen, &mut tracks);

    // YouTube Music Mix / Radio cards (e.g. `VLRDCLAK...` / `VLRD...`) often return tracks via `next`
    // (`playlistId` without `VL` prefix) instead of `browse`. Fall back automatically so every card works.
    if tracks.is_empty() {
        let stripped_pl = bid.strip_prefix("VL").unwrap_or(bid);
        let next_body = json!({
            "playlistId": stripped_pl,
            "enablePersistentPlaylistPanel": true,
            "isAudioOnly": true,
            "context": web_remix_context(session),
        });
        if let Ok(next_root) = post_ytm_json("next", &next_body, session) {
            collect_music_items(&next_root, &resolved_title, 50, &mut seen, &mut tracks);
        }
    }
    if tracks.is_empty()
        && resolved_title != "YouTube Music"
        && let Ok(hits) = search_music(session, &resolved_title, 20)
    {
        tracks = hits;
    }

    let kind = if bid.starts_with("MPREb_") || bid.starts_with("OLAK5uy_") {
        YtmCardKind::Album
    } else if bid.starts_with("UC") {
        YtmCardKind::Artist
    } else {
        YtmCardKind::Playlist
    };

    let thumbnail_url =
        header_thumb.or_else(|| tracks.first().and_then(|t| t.thumbnail_url.clone()));
    if let Some(ref cover) = thumbnail_url {
        for t in &mut tracks {
            if t.thumbnail_url.is_none() {
                t.thumbnail_url = Some(cover.clone());
            }
        }
    }

    Ok(YtmBrowseDetail {
        browse_id: bid.to_string(),
        title: resolved_title,
        subtitle,
        description,
        thumbnail_url,
        kind,
        tracks,
    })
}

static RADIO_FRESH_TICK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn normalize_core_title(title: &str) -> String {
    let mut base = title.trim();
    for sep in ["(", "[", "|", " - ", " \u{2013} ", " \u{2014} "] {
        if let Some((head, _)) = base.split_once(sep) {
            let trimmed = head.trim();
            if !trimmed.is_empty() {
                base = trimmed;
            }
        }
    }
    let low = base.to_ascii_lowercase();
    let mut out = String::with_capacity(low.len());
    let mut prev_space = false;
    for tok in low.split_whitespace() {
        if tok == "feat"
            || tok == "feat."
            || tok == "ft"
            || tok == "ft."
            || tok.starts_with("+1key")
            || tok.starts_with("+2key")
            || tok.starts_with("+3key")
            || tok.starts_with("+4key")
            || tok.starts_with("+5key")
            || tok.starts_with("-1key")
            || tok.starts_with("-2key")
            || tok.starts_with("-3key")
            || tok.starts_with("-4key")
        {
            break;
        }
        for ch in tok.chars() {
            if ch.is_ascii_alphanumeric() {
                out.push(ch);
                prev_space = false;
            } else if !prev_space && !out.is_empty() {
                out.push(' ');
                prev_space = true;
            }
        }
        if !prev_space && !out.is_empty() {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn is_same_song_variant(seed_core: &str, cand_core: &str) -> bool {
    if seed_core.is_empty() || cand_core.is_empty() {
        return false;
    }
    if seed_core == cand_core {
        return true;
    }
    if seed_core.len() >= 4
        && (cand_core
            .strip_prefix(seed_core)
            .is_some_and(|r| r.starts_with(' '))
            || seed_core
                .strip_prefix(cand_core)
                .is_some_and(|r| r.starts_with(' ')))
    {
        return true;
    }
    false
}

fn extract_artist_tokens(artist: &str) -> Vec<String> {
    let normalized = artist
        .to_ascii_lowercase()
        .replace(" feat. ", ",")
        .replace(" feat ", ",")
        .replace(" ft. ", ",")
        .replace(" ft ", ",")
        .replace(" with ", ",")
        .replace(" x ", ",");
    normalized
        .split(&[',', '&', '•', ';', '/'][..])
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "unknown artist" && *s != "various artists")
        .map(ToOwned::to_owned)
        .collect()
}

fn primary_artist_key(artist: &str) -> String {
    extract_artist_tokens(artist)
        .into_iter()
        .next()
        .unwrap_or_else(|| artist.trim().to_ascii_lowercase())
}

#[inline]
fn contains_ascii_icase(haystack: &str, needle_lower: &[u8]) -> bool {
    let n = needle_lower.len();
    if n == 0 || haystack.len() < n {
        return false;
    }
    haystack
        .as_bytes()
        .windows(n)
        .any(|w| w.eq_ignore_ascii_case(needle_lower))
}

fn is_low_quality_radio_candidate(
    title: &str,
    artist: &str,
    seed_title: &str,
    duration_ms: u64,
) -> bool {
    if duration_ms > 0 && !(75_000..=540_000).contains(&duration_ms) {
        return true;
    }
    let seed_is_remix = contains_ascii_icase(seed_title, b"remix");
    if !seed_is_remix
        && (contains_ascii_icase(title, b"remix") || contains_ascii_icase(artist, b"remix"))
    {
        return true;
    }
    static BAD_MATCHER: std::sync::OnceLock<aho_corasick::AhoCorasick> = std::sync::OnceLock::new();
    let matcher = BAD_MATCHER.get_or_init(|| {
        aho_corasick::AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build([
                "karaoke",
                "sped up",
                "speed up",
                "slowed",
                "reverb",
                "nightcore",
                "8d audio",
                "1 hour",
                "10 hours",
                "podcast",
                "episode",
                "reaction",
                "interview",
                "trailer",
                "teaser",
                "tutorial",
                "ringtone",
                "dj tiktok",
                "tiktok version",
                "full bass",
                "bass boosted",
                "jedag jedug",
                "parody",
                "+1key",
                "+2key",
                "+3key",
                "+4key",
                "+5key",
                "-1key",
                "-2key",
                "-3key",
                "-4key",
                "key no",
                "mini pop",
                "kidz bop",
                "metal version",
                "metal cover",
                "piano cover",
                "guitar cover",
                "acoustic cover",
                "violin cover",
                "drum cover",
                "vocal cover",
                "cover by",
                "(cover)",
                "[cover]",
                "instrumental",
                "backing track",
                "lullaby",
                "music box",
                "8-bit",
                "8 bit",
                "minecraft",
                "extended",
                "tribute",
                "chipmunk",
                "edit audio",
                "mashup",
                "acapella",
                "a cappella",
                "todays top hits",
                "today's top hits",
                "radio top hits",
                "chill hit songs",
                "top hits radio",
                "hits playlist",
                "nonstop",
            ])
            .expect("static radio filter patterns must compile")
    });
    matcher.is_match(title) || matcher.is_match(artist)
}

fn filter_coherent_radio_tracks(
    seed_locale: MusicLocale,
    seed_title: &str,
    seed_artist: &str,
    candidates: Vec<Track>,
    limit: usize,
) -> Vec<Track> {
    let seed_core = normalize_core_title(seed_title);
    let seed_tokens = extract_artist_tokens(seed_artist);
    let mut seen_artists: HashSet<String> = HashSet::new();
    let mut seen_titles: HashSet<String> = HashSet::new();
    let mut other_artists = Vec::with_capacity(limit);
    let mut seed_artist_pick: Option<Track> = None;

    for t in candidates {
        if is_low_quality_radio_candidate(&t.title, &t.artist, seed_title, t.duration_ms) {
            continue;
        }
        let cand_core = normalize_core_title(&t.title);
        if is_same_song_variant(&seed_core, &cand_core) {
            continue;
        }
        if !cand_core.is_empty() && seen_titles.contains(&cand_core) {
            continue;
        }
        let cand_locale = detect_track_locale(&t.title, &t.artist, &t.album);
        let cand_tokens = extract_artist_tokens(&t.artist);
        let same_artist = !seed_tokens.is_empty()
            && cand_tokens.iter().any(|ct| {
                seed_tokens
                    .iter()
                    .any(|st| ct == st || (st.len() >= 4 && (ct.contains(st) || st.contains(ct))))
            });
        if seed_locale == MusicLocale::GlobalEn
            && cand_locale == MusicLocale::Indonesian
            && !same_artist
        {
            continue;
        }

        if same_artist {
            // Keep at most 1 song by the seed artist, never spamming or dominating Up Next
            if seed_artist_pick.is_none() {
                if !cand_core.is_empty() {
                    seen_titles.insert(cand_core);
                }
                seed_artist_pick = Some(t);
            }
            continue;
        }

        let artist_key = primary_artist_key(&t.artist);
        if artist_key.is_empty() || !seen_artists.insert(artist_key) {
            continue;
        }
        if !cand_core.is_empty() {
            seen_titles.insert(cand_core);
        }
        other_artists.push(t);
        if other_artists.len() >= limit {
            break;
        }
    }

    let mut out = other_artists;
    if let Some(seed_track) = seed_artist_pick
        && out.len() < limit
    {
        // Place the single optional seed-artist track mid-queue (never #1) so playback feels diverse
        let insert_pos = 4usize.min(out.len());
        out.insert(insert_pos, seed_track);
    }
    out.truncate(limit);
    out
}

/// Fetch locale- and vibe-coherent, multi-artist Radio / Up Next recommendations for a playing
/// YouTube Music song via `youtubei/v1/next` with second-hop artist expansion and strict
/// single-track-per-artist diversity enforcement.
pub fn fetch_radio_up_next_for_track(
    session: &YtmSession,
    video_id: &str,
    seed_title: &str,
    seed_artist: &str,
    seed_album: &str,
    limit: usize,
    force_fresh: bool,
) -> ProviderResult<Vec<Track>> {
    fetch_radio_up_next_inner(
        session,
        video_id,
        seed_title,
        seed_artist,
        seed_album,
        limit,
        force_fresh,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn fetch_radio_up_next_inner(
    session: &YtmSession,
    video_id: &str,
    seed_title: &str,
    seed_artist: &str,
    seed_album: &str,
    limit: usize,
    force_fresh: bool,
    allow_extra_hops: bool,
) -> ProviderResult<Vec<Track>> {
    if !is_valid_video_id(video_id) {
        return Err(ProviderError::Metadata(
            "Invalid videoId for radio".to_string(),
        ));
    }
    let target_limit = limit.max(1);
    let seed_locale = detect_track_locale(seed_title, seed_artist, seed_album);
    let radio_session = if !session.is_signed_in() {
        YtmSession::guest(String::new())
    } else {
        session.clone()
    };
    let tick = if force_fresh {
        RADIO_FRESH_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    } else {
        0
    };
    // Omit broken/URL-encoded `params` (e.g. `wAEB8gECKAE%3D` returns 0 tracks from InnerTube);
    // freshness comes cleanly from stratified seed rotation and randomized 2nd-hop expansion.
    let body = json!({
        "videoId": video_id,
        "playlistId": format!("RDAMVM{video_id}"),
        "isAudioOnly": true,
        "enablePersistentPlaylistPanel": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "context": web_remix_context_for_locale(&radio_session, seed_locale),
    });
    let root = if allow_extra_hops {
        post_ytm_json("next", &body, &radio_session)?
    } else {
        post_ytm_json_fast("next", &body, &radio_session)?
    };
    let mut raw_tracks = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(video_id.to_string());
    collect_radio_panel_items(
        &root,
        target_limit.saturating_mul(3).max(36),
        &mut seen,
        &mut raw_tracks,
    );

    // Second-hop artist expansion: pick a different artist from the primary radio panel and fetch
    // their `RDAMVM` radio neighborhood too so queues always span 12-15 genuinely distinct artists.
    let seed_tokens = extract_artist_tokens(seed_artist);
    let hop_candidates: Vec<String> = raw_tracks
        .iter()
        .filter(|t| {
            !is_low_quality_radio_candidate(&t.title, &t.artist, seed_title, t.duration_ms)
                && !extract_artist_tokens(&t.artist)
                    .iter()
                    .any(|ct| seed_tokens.iter().any(|st| ct == st))
        })
        .filter_map(|t| t.youtube_video_id.clone())
        .collect();
    if allow_extra_hops && !hop_candidates.is_empty() {
        let hop_idx = if force_fresh {
            fastrand::usize(..hop_candidates.len())
        } else {
            tick % hop_candidates.len()
        };
        let hop_vid = &hop_candidates[hop_idx];
        let hop_body = json!({
            "videoId": hop_vid,
            "playlistId": format!("RDAMVM{hop_vid}"),
            "isAudioOnly": true,
            "enablePersistentPlaylistPanel": true,
            "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            "context": web_remix_context_for_locale(&radio_session, seed_locale),
        });
        if let Ok(hop_root) = post_ytm_json_fast("next", &hop_body, &radio_session) {
            collect_radio_panel_items(&hop_root, 28, &mut seen, &mut raw_tracks);
        }
    }

    let mut coherent = filter_coherent_radio_tracks(
        seed_locale,
        seed_title,
        seed_artist,
        raw_tracks,
        target_limit,
    );

    // Locale-matched multi-artist fallback (NEVER raw `seed_artist` search, which spams same-artist covers)
    if allow_extra_hops && coherent.len() < target_limit.min(10) {
        let fallback_queries: &[&str] = match seed_locale {
            MusicLocale::GlobalEn => &[
                "top pop hits radio",
                "global pop songs mix",
                "modern indie pop hits",
                "chill pop favorites",
                "r&b pop radio hits",
            ],
            MusicLocale::Indonesian => &[
                "lagu pop indonesia populer",
                "musik indie indonesia terbaik",
                "hits pop indonesia terbaru",
            ],
            MusicLocale::Japanese => &["j-pop hits radio", "japanese pop songs"],
            MusicLocale::Korean => &["k-pop hits radio", "korean pop songs"],
        };
        let query = fallback_queries[tick % fallback_queries.len()];
        let search_body = json!({
            "query": query,
            "params": "EgWKAQIIAWoMEAMQBBAJEAoQBRAV",
            "context": web_remix_context_for_locale(&radio_session, seed_locale),
        });
        if let Ok(search_root) = post_ytm_json_fast("search", &search_body, &radio_session) {
            let mut extra = Vec::new();
            collect_music_items(
                &search_root,
                "YouTube Music Radio",
                24,
                &mut seen,
                &mut extra,
            );
            let mut existing_artists: HashSet<String> = coherent
                .iter()
                .map(|c| primary_artist_key(&c.artist))
                .collect();
            let mut filtered_extra = filter_coherent_radio_tracks(
                seed_locale,
                seed_title,
                seed_artist,
                extra,
                target_limit,
            );
            if force_fresh && filtered_extra.len() > 2 {
                fastrand::shuffle(&mut filtered_extra);
            }
            for t in filtered_extra {
                if coherent.len() >= target_limit {
                    break;
                }
                let ak = primary_artist_key(&t.artist);
                if !ak.is_empty()
                    && existing_artists.insert(ak)
                    && !coherent.iter().any(|c| c.id == t.id)
                {
                    coherent.push(t);
                }
            }
        }
    }

    if force_fresh && coherent.len() > 3 {
        fastrand::shuffle(&mut coherent);
        // Ensure #1 Up Next is never the exact same artist as the currently playing song
        if let Some(first_diff) = coherent.iter().position(|t| {
            !extract_artist_tokens(&t.artist)
                .iter()
                .any(|ct| seed_tokens.iter().any(|st| ct == st))
        }) && first_diff > 0
        {
            coherent.swap(0, first_diff);
        }
    }
    Ok(coherent)
}

/// Fetch infinite Radio / Up Next recommendations for a playing YouTube Music song via `youtubei/v1/next`.
pub fn fetch_radio_up_next(
    session: &YtmSession,
    video_id: &str,
    limit: usize,
) -> ProviderResult<Vec<Track>> {
    fetch_radio_up_next_for_track(session, video_id, "", "", "", limit, false)
}

/// Personalized Home "Recommended Station" card (Spotify Radio parity).
///
/// Populated either directly by Engine A (`spotify_web::fetch_spotify_recommended_stations`,
/// carrying Spotify's pre-rendered 3-circle Radio artwork & live 50-song playlist ID) or by
/// Engine B (`build_algorithmic_recommended_stations`, generating multi-locale artist- and
/// song-seeded stations with 3-circle triptych covers and lazy multi-hop co-listening expansion).
#[derive(Debug, Clone)]
pub struct RecommendedStation {
    pub id: String,
    pub title: String,
    pub display_name: String,
    pub subtitle: String,
    pub palette_idx: u8,
    pub is_spotify_prebuilt_art: bool,
    pub center_art_url: Option<String>,
    pub left_art_url: Option<String>,
    pub right_art_url: Option<String>,
    pub spotify_playlist_id: Option<String>,
    pub seed_video_id: Option<String>,
    pub seed_artist: String,
    pub seed_title: String,
    pub is_spotify_origin: bool,
    pub cached_tracks: Vec<Track>,
}

impl RecommendedStation {
    /// Convert a live Spotify Pathfinder `home` Recommended Station card into the unified model.
    pub fn from_spotify_card(
        card: &crate::spotify_web::SpotifyStationCard,
        palette_idx: u8,
    ) -> Self {
        let has_art = card.thumbnail_url.is_some();
        let subtitle = if card.subtitle.trim().is_empty() {
            format!("Non-stop radio inspired by {}", card.display_name)
        } else {
            card.subtitle.clone()
        };
        Self {
            id: format!("sp:station:{}", card.playlist_id),
            title: card.title.clone(),
            display_name: card.display_name.clone(),
            subtitle,
            palette_idx: palette_idx % 8,
            is_spotify_prebuilt_art: has_art,
            center_art_url: card.thumbnail_url.clone(),
            left_art_url: None,
            right_art_url: None,
            spotify_playlist_id: Some(card.playlist_id.clone()),
            seed_video_id: None,
            seed_artist: card.display_name.clone(),
            seed_title: card.display_name.clone(),
            is_spotify_origin: true,
            cached_tracks: Vec::new(),
        }
    }
}

fn stable_palette_index(label: &str, prev_idx: Option<u8>) -> u8 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in label.to_ascii_lowercase().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let mut idx = (hash % 8) as u8;
    if prev_idx == Some(idx) {
        idx = (idx + 3) % 8;
    }
    idx
}

fn primary_artist_display(raw: &str) -> String {
    let first = raw
        .split(['&', ',', '•', '·'])
        .next()
        .unwrap_or(raw)
        .split(" feat.")
        .next()
        .unwrap_or(raw)
        .split(" ft.")
        .next()
        .unwrap_or(raw)
        .trim();
    if first.is_empty() {
        raw.trim().to_string()
    } else {
        first.to_string()
    }
}

fn clean_station_song_label(title: &str) -> String {
    let mut s = title.trim().to_string();
    for sep in [" (feat.", " (ft.", " (with ", " [Official", " (Official"] {
        if let Some(pos) = s.to_ascii_lowercase().find(&sep.to_ascii_lowercase())
            && pos >= 2
        {
            s.truncate(pos);
        }
    }
    s.trim().to_string()
}

/// Format companion artists using Spotify Radio's exact `"With A1, A2, A3 and more"` phrasing.
pub fn format_station_subtitle(companions: &[String], fallback_seed: &str) -> String {
    let mut uniq: Vec<&str> = Vec::new();
    for c in companions {
        let trimmed = c.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !uniq.iter().any(|u| u.eq_ignore_ascii_case(trimmed)) {
            uniq.push(trimmed);
            if uniq.len() >= 3 {
                break;
            }
        }
    }
    match uniq.len() {
        0 => format!("Non-stop radio based on {fallback_seed}"),
        1 => format!("With {} and more", uniq[0]),
        2 => format!("With {}, {} and more", uniq[0], uniq[1]),
        _ => format!("With {}, {}, {} and more", uniq[0], uniq[1], uniq[2]),
    }
}

/// Interleave a station tracklist so no two consecutive songs share the same primary artist,
/// while preserving high-affinity track ordering (Spotify Radio sequencing rule).
pub fn interleave_station_tracks(tracks: Vec<Track>, max_tracks: usize) -> Vec<Track> {
    let target = max_tracks.min(tracks.len());
    let mut pool: Vec<(String, Track)> = tracks
        .into_iter()
        .map(|t| (primary_artist_key(&t.artist), t))
        .collect();
    let mut out: Vec<Track> = Vec::with_capacity(target);
    let mut last_artist = String::new();

    while out.len() < target && !pool.is_empty() {
        let pick_idx = pool
            .iter()
            .position(|(ak, _)| last_artist.is_empty() || ak.is_empty() || *ak != last_artist)
            .unwrap_or(0);
        let (ak, chosen) = pool.remove(pick_idx);
        last_artist = ak;
        out.push(chosen);
    }
    out
}

/// Build up to `max_stations` diverse artist- and song-seeded Recommended Stations using a
/// Spotify-parity affinity model:
/// 1. Exponential recency/play-count decay across `taste.top_tracks` & `taste.top_artists`,
///    merged with Liked Songs and cross-library frequency (`library_tracks`) + cold-start `home_tracks`.
/// 2. Stratified multi-locale round-robin across active taste clusters (English, Indonesian,
///    Japanese, Korean) so mixed-language listeners mirror Spotify's Home shelf diversity.
/// 3. Alternating Artist-seeded (`"JVKE Radio"`) and Song-seeded (`"Kill Bill Radio"`) stations,
///    strictly 1 station per primary artist, paired with same-cluster companion artists and 3
///    distinct cover art URLs for the pastel 3-circle triptych.
pub fn build_algorithmic_recommended_stations(
    taste: &YtmHomeTasteProfile,
    library_tracks: &[Track],
    home_tracks: &[Track],
    max_stations: usize,
) -> Vec<RecommendedStation> {
    build_algorithmic_recommended_stations_rotated(
        taste,
        library_tracks,
        home_tracks,
        max_stations,
        0,
    )
}

/// Same as `build_algorithmic_recommended_stations` with an explicit `rotation_epoch` so pressing
/// Refresh deals genuinely new seed artists/songs from deeper in the user's multi-locale taste graph.
pub fn build_algorithmic_recommended_stations_rotated(
    taste: &YtmHomeTasteProfile,
    library_tracks: &[Track],
    home_tracks: &[Track],
    max_stations: usize,
    rotation_epoch: usize,
) -> Vec<RecommendedStation> {
    if max_stations == 0 {
        return Vec::new();
    }

    #[derive(Clone)]
    struct ArtistClusterNode {
        norm: String,
        display: String,
        locale: MusicLocale,
        score: i32,
        best_track: Track,
        best_track_score: i32,
        sample_tracks: Vec<Track>,
    }

    let mut artist_nodes: HashMap<String, ArtistClusterNode> = HashMap::new();

    let absorb_track = |nodes: &mut HashMap<String, ArtistClusterNode>,
                        t: &Track,
                        base_weight: i32| {
        if t.title.trim().is_empty()
            || t.artist.trim().is_empty()
            || t.artist.eq_ignore_ascii_case("Unknown Artist")
            || is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms)
        {
            return;
        }
        let norm = primary_artist_norm(&t.artist);
        if norm.is_empty() || norm == "various artists" {
            return;
        }
        let art_bonus = if t.thumbnail_url.is_some() { 14 } else { 0 };
        let vid_bonus = if t.youtube_video_id.is_some() { 6 } else { 0 };
        let liked_bonus = if t.liked { 26 } else { 0 };
        let track_score = base_weight + art_bonus + vid_bonus + liked_bonus;

        if let Some(node) = nodes.get_mut(&norm) {
            node.score += base_weight.max(4);
            if track_score > node.best_track_score {
                node.best_track_score = track_score;
                node.best_track = t.clone();
                node.locale = detect_track_locale(&t.title, &t.artist, &t.album);
            } else if node.best_track.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                node.best_track = t.clone();
            }
            if node.sample_tracks.len() < 5 && !node.sample_tracks.iter().any(|s| s.id == t.id) {
                node.sample_tracks.push(t.clone());
            }
        } else {
            let display = primary_artist_display(&t.artist);
            let locale = detect_track_locale(&t.title, &t.artist, &t.album);
            nodes.insert(
                norm.clone(),
                ArtistClusterNode {
                    norm,
                    display,
                    locale,
                    score: base_weight + liked_bonus,
                    best_track: t.clone(),
                    best_track_score: track_score,
                    sample_tracks: vec![t.clone()],
                },
            );
        }
    };

    // Signal 1: Recent / heavily played taste profile tracks (strongest decay curve)
    for (idx, t) in taste.top_tracks.iter().take(60).enumerate() {
        let w = 120_i32.saturating_sub((idx as i32) * 2).max(24);
        absorb_track(&mut artist_nodes, t, w);
    }
    // Signal 2: Full library (scan ALL tracks; never truncate alphabetically!)
    for t in library_tracks {
        let w = if t.liked { 38 } else { 14 };
        absorb_track(&mut artist_nodes, t, w);
    }
    // Signal 3: Explicit top artists affinity boost
    for (idx, a) in taste.top_artists.iter().take(32).enumerate() {
        let norm = primary_artist_norm(a);
        if let Some(node) = artist_nodes.get_mut(&norm) {
            node.score += 105_i32.saturating_sub((idx as i32) * 4).max(18);
        }
    }
    // Signal 4: Live Home quick picks (ensures Guest / brand-new users still get vibrant stations)
    let home_w: i32 = if artist_nodes.len() < max_stations {
        48
    } else {
        12
    };
    for (idx, t) in home_tracks.iter().take(40).enumerate() {
        let w = home_w.saturating_sub((idx as i32) / 2).max(6);
        absorb_track(&mut artist_nodes, t, w);
    }

    let is_noisy_cast_name = |name: &str| {
        name.chars().count() > 30
            || name.contains('(')
            || name.contains('（')
            || name.contains('[')
            || name.contains("CV:")
            || name.contains("cv.")
    };

    let mut all_nodes: Vec<ArtistClusterNode> = artist_nodes
        .into_values()
        .filter(|n| n.score >= 36 || !is_noisy_cast_name(&n.display))
        .collect();
    all_nodes.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.display.cmp(&b.display))
    });
    if all_nodes.is_empty() {
        return Vec::new();
    }

    // Group into locale buckets so multi-language libraries mirror Spotify's cross-cluster variety.
    // On Refresh (`rotation_epoch > 0`), apply Efraimidis-Spirakis weighted random sampling without
    // replacement (excluding recently shown station artists) instead of a blind linear shift that
    // jumps into obscure 1-track tail entries.
    let mut locale_buckets: Vec<(MusicLocale, i32, Vec<ArtistClusterNode>)> = Vec::new();
    for loc in [
        MusicLocale::GlobalEn,
        MusicLocale::Indonesian,
        MusicLocale::Japanese,
        MusicLocale::Korean,
    ] {
        let mut members: Vec<ArtistClusterNode> = all_nodes
            .iter()
            .filter(|n| n.locale == loc)
            .cloned()
            .collect();
        if !members.is_empty() {
            let total_w: i32 = members.iter().take(6).map(|m| m.score).sum();
            if rotation_epoch > 0 && members.len() > 1 {
                // Keep the quality tier (top 40 or score >= 28) ahead of 1-track stragglers
                let orig_leader = members[0].norm.clone();
                let mut keyed: Vec<(bool, bool, f64, ArtistClusterNode)> = members
                    .into_iter()
                    .enumerate()
                    .map(|(rank, node)| {
                        let excluded = taste.excluded_artists.contains(&node.norm);
                        let low_tier = rank >= 40 && node.score < 28;
                        let w = (node.score.max(1) as f64).powf(0.85);
                        let u = fastrand::f64().clamp(1e-6, 1.0 - 1e-6);
                        let key = u.powf(1.0 / w);
                        (excluded, low_tier, key, node)
                    })
                    .collect();
                keyed.sort_by(|a, b| {
                    a.0.cmp(&b.0)
                        .then_with(|| a.1.cmp(&b.1))
                        .then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
                });
                members = keyed.into_iter().map(|(_, _, _, n)| n).collect();
                // Guarantee the leading card genuinely rotates away from the epoch-0 leader
                if members.len() > 1 && members[0].norm == orig_leader {
                    members.rotate_left(1);
                }
            }
            locale_buckets.push((loc, total_w, members));
        }
    }
    locale_buckets.sort_by_key(|a| std::cmp::Reverse(a.1));

    // Weighted round-robin seed selection: dominant locale gets 2 slots per cycle, secondary locales 1 slot.
    let mut cursors = vec![0usize; locale_buckets.len()];
    let mut chosen_seeds: Vec<ArtistClusterNode> = Vec::with_capacity(max_stations);
    let mut chosen_norms: HashSet<String> = HashSet::new();

    while chosen_seeds.len() < max_stations {
        let mut progressed = false;
        for (bi, (_, _, members)) in locale_buckets.iter().enumerate() {
            let quota = if bi == 0 && locale_buckets.len() > 1 {
                2
            } else {
                1
            };
            for _ in 0..quota {
                if chosen_seeds.len() >= max_stations {
                    break;
                }
                while cursors[bi] < members.len() {
                    let cand = &members[cursors[bi]];
                    cursors[bi] += 1;
                    if chosen_norms.insert(cand.norm.clone()) {
                        chosen_seeds.push(cand.clone());
                        progressed = true;
                        break;
                    }
                }
            }
        }
        if !progressed {
            break;
        }
    }

    let mut stations = Vec::with_capacity(chosen_seeds.len());
    let mut prev_palette: Option<u8> = None;

    for (idx, seed) in chosen_seeds.into_iter().enumerate() {
        // Vary the featured seed track across refreshes when an artist has multiple library songs
        let featured_track = if rotation_epoch > 0 && seed.sample_tracks.len() > 1 {
            seed.sample_tracks[fastrand::usize(..seed.sample_tracks.len())].clone()
        } else {
            seed.best_track.clone()
        };
        let clean_song = clean_station_song_label(&featured_track.title);
        let use_song_seed = (idx + rotation_epoch).is_multiple_of(2)
            && !clean_song.is_empty()
            && clean_song.chars().count() <= 26
            && !clean_song.eq_ignore_ascii_case(&seed.display);

        let display_name = if use_song_seed {
            clean_song
        } else {
            seed.display.clone()
        };
        let title = format!("{display_name} Radio");

        // Gather companion artists from the same locale cluster first, then global affinity pool.
        let mut companion_nodes: Vec<&ArtistClusterNode> = all_nodes
            .iter()
            .filter(|n| n.norm != seed.norm && n.locale == seed.locale)
            .collect();
        if companion_nodes.len() < 3 {
            for n in &all_nodes {
                if n.norm != seed.norm && !companion_nodes.iter().any(|c| c.norm == n.norm) {
                    companion_nodes.push(n);
                    if companion_nodes.len() >= 8 {
                        break;
                    }
                }
            }
        }

        let comp_len = companion_nodes.len();
        let mut rotated_companions: Vec<&ArtistClusterNode> = Vec::with_capacity(3);
        if comp_len > 0 {
            let start = if rotation_epoch > 0 {
                fastrand::usize(..comp_len)
            } else {
                (idx * 2) % comp_len
            };
            for step in 0..comp_len.min(3) {
                rotated_companions.push(companion_nodes[(start + step) % comp_len]);
            }
        }

        let companion_names: Vec<String> = if use_song_seed {
            let mut names = vec![seed.display.clone()];
            for c in &rotated_companions {
                names.push(c.display.clone());
            }
            names
        } else {
            rotated_companions
                .iter()
                .map(|c| c.display.clone())
                .collect()
        };
        let subtitle = format_station_subtitle(&companion_names, &display_name);

        let center_art_url = featured_track
            .thumbnail_url
            .clone()
            .or_else(|| seed.best_track.thumbnail_url.clone());
        let left_art_url = rotated_companions
            .first()
            .and_then(|c| c.best_track.thumbnail_url.clone())
            .filter(|u| Some(u) != center_art_url.as_ref());
        let right_art_url = rotated_companions
            .get(1)
            .and_then(|c| c.best_track.thumbnail_url.clone())
            .filter(|u| Some(u) != center_art_url.as_ref() && Some(u) != left_art_url.as_ref())
            .or_else(|| {
                rotated_companions
                    .get(2)
                    .and_then(|c| c.best_track.thumbnail_url.clone())
                    .filter(|u| {
                        Some(u) != center_art_url.as_ref() && Some(u) != left_art_url.as_ref()
                    })
            });

        let palette_idx = stable_palette_index(&display_name, prev_palette);
        prev_palette = Some(palette_idx);

        let slug: String = display_name
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        stations.push(RecommendedStation {
            id: format!("ferric:station:{idx}:{slug}"),
            title,
            display_name,
            subtitle,
            palette_idx,
            is_spotify_prebuilt_art: false,
            center_art_url,
            left_art_url,
            right_art_url,
            spotify_playlist_id: None,
            seed_video_id: featured_track
                .youtube_video_id
                .clone()
                .or_else(|| seed.best_track.youtube_video_id.clone()),
            seed_artist: seed.display,
            seed_title: featured_track.title,
            is_spotify_origin: false,
            cached_tracks: Vec::new(),
        });
    }

    stations
}

/// Result of lazily expanding a `RecommendedStation` via multi-hop co-listening graph traversal.
#[derive(Debug, Clone)]
pub struct ExpandedStationTracks {
    pub tracks: Vec<Track>,
    pub subtitle: String,
    pub left_art: Option<String>,
    pub right_art: Option<String>,
}

/// Lazily expand an algorithmic Recommended Station into a full 35–50 track curated Spotify-Radio-style
/// playlist via multi-hop `RDAMVM` co-listening graph expansion, strict artist caps, locale coherence,
/// and zero-back-to-back artist interleaving.
pub fn expand_recommended_station_tracks(
    session: &YtmSession,
    station: &RecommendedStation,
    library_pool: &[Track],
) -> ProviderResult<ExpandedStationTracks> {
    let seed_artist_norm = primary_artist_norm(&station.seed_artist);
    let seed_title_core = normalize_core_title(&station.seed_title);
    let seed_locale = detect_track_locale(&station.seed_title, &station.seed_artist, "");

    // 1. Resolve anchor video ID (existing video_id, or fast YTM Songs lookup for seed song/artist).
    let mut seed_track: Option<Track> = library_pool
        .iter()
        .find(|t| {
            t.youtube_video_id.is_some()
                && primary_artist_norm(&t.artist) == seed_artist_norm
                && normalize_core_title(&t.title) == seed_title_core
        })
        .cloned();

    let seed_vid = if let Some(vid) = station
        .seed_video_id
        .as_deref()
        .filter(|v| is_valid_video_id(v))
    {
        vid.to_string()
    } else if let Some(vid) = seed_track
        .as_ref()
        .and_then(|t| t.youtube_video_id.clone())
        .filter(|v| is_valid_video_id(v))
    {
        vid
    } else {
        let query = if station
            .seed_title
            .eq_ignore_ascii_case(&station.seed_artist)
            || station.seed_title.trim().is_empty()
        {
            station.seed_artist.clone()
        } else {
            format!("{} {}", station.seed_artist, station.seed_title)
        };
        let search_body = json!({
            "query": query,
            "params": "EgWKAQIIAWoMEAMQBBAJEAoQBRAV",
            "context": web_remix_context_for_locale(session, seed_locale),
        });
        let root = post_ytm_json("search", &search_body, session)?;
        let mut hits = Vec::new();
        let mut seen = HashSet::new();
        collect_music_items(&root, &station.title, 8, &mut seen, &mut hits);
        let picked = hits
            .into_iter()
            .find(|t| {
                t.youtube_video_id.is_some()
                    && !is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms)
            })
            .ok_or_else(|| {
                ProviderError::Metadata(format!(
                    "Could not resolve seed song for {}",
                    station.title
                ))
            })?;
        let vid = picked.youtube_video_id.clone().unwrap_or_default();
        seed_track = Some(picked);
        vid
    };

    // 2. Primary RDAMVM collaborative co-listening neighborhood
    let mut raw_candidates: Vec<Track> = Vec::new();
    if let Ok(hop1) = fetch_radio_up_next_for_track(
        session,
        &seed_vid,
        &station.seed_title,
        &station.seed_artist,
        "",
        26,
        false,
    ) {
        raw_candidates.extend(hop1);
    }

    // 3. Second-hop expansion from 2 distinct companion artists (yields ~45-65 collaborative candidates)
    let hop2_seeds: Vec<Track> = {
        let mut picked = Vec::new();
        let mut seen_a = HashSet::new();
        seen_a.insert(seed_artist_norm.clone());
        for t in &raw_candidates {
            let ak = primary_artist_norm(&t.artist);
            if !ak.is_empty() && t.youtube_video_id.is_some() && seen_a.insert(ak) {
                picked.push(t.clone());
                if picked.len() >= 2 {
                    break;
                }
            }
        }
        picked
    };
    for h in &hop2_seeds {
        if let Some(hvid) = h.youtube_video_id.as_deref()
            && let Ok(hop2) = fetch_radio_up_next_inner(
                session, hvid, &h.title, &h.artist, &h.album, 18, false, false,
            )
        {
            raw_candidates.extend(hop2);
        }
    }

    // 4. Seed-artist anchor songs (Spotify Radio includes ~3-4 hits by the seed artist spaced through the 50 songs)
    let mut seed_artist_hits: Vec<Track> = Vec::new();
    if let Some(st) = seed_track {
        seed_artist_hits.push(st);
    }
    for t in library_pool {
        if seed_artist_hits.len() >= 4 {
            break;
        }
        if primary_artist_norm(&t.artist) == seed_artist_norm
            && !is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms)
            && !seed_artist_hits
                .iter()
                .any(|x| normalize_core_title(&x.title) == normalize_core_title(&t.title))
        {
            seed_artist_hits.push(t.clone());
        }
    }

    // 5. Merge + deduplicate + enforce artist caps (seed <= 4, companion <= 2) + locale coherence
    let mut curated: Vec<Track> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_titles: HashSet<String> = HashSet::new();
    let mut artist_counts: HashMap<String, usize> = HashMap::new();

    for t in seed_artist_hits.into_iter().chain(raw_candidates) {
        if curated.len() >= 45 {
            break;
        }
        if is_low_quality_radio_candidate(&t.title, &t.artist, &station.seed_title, t.duration_ms) {
            continue;
        }
        let cand_locale = detect_track_locale(&t.title, &t.artist, &t.album);
        if seed_locale == MusicLocale::GlobalEn && cand_locale == MusicLocale::Indonesian {
            continue;
        }
        let title_key = normalize_core_title(&t.title);
        if title_key.is_empty() || !seen_titles.insert(title_key) {
            continue;
        }
        let dedup_id = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
        if !seen_ids.insert(dedup_id) {
            continue;
        }
        let ak = primary_artist_norm(&t.artist);
        let cap = if ak == seed_artist_norm { 4 } else { 2 };
        let cnt = artist_counts.entry(ak).or_insert(0);
        if *cnt >= cap {
            continue;
        }
        *cnt += 1;
        curated.push(t);
    }

    if curated.is_empty() {
        return Err(ProviderError::Metadata(format!(
            "No playable tracks found for {}",
            station.title
        )));
    }

    // 6. Sequence with zero back-to-back same artist (seed song locked at index 0)
    let sequenced = interleave_station_tracks(curated, 45);

    // 7. Compute authentic companion subtitle & 2 side-circle artworks from top companion artists
    let mut companion_names: Vec<String> = Vec::new();
    let mut side_arts: Vec<String> = Vec::new();
    for t in &sequenced {
        let ak = primary_artist_norm(&t.artist);
        if ak.is_empty() || ak == seed_artist_norm {
            continue;
        }
        let disp = primary_artist_display(&t.artist);
        if !companion_names
            .iter()
            .any(|c| c.eq_ignore_ascii_case(&disp))
        {
            companion_names.push(disp);
            if let Some(url) = t.thumbnail_url.as_ref()
                && Some(url) != station.center_art_url.as_ref()
                && !side_arts.contains(url)
            {
                side_arts.push(url.clone());
            }
        }
        if companion_names.len() >= 3 && side_arts.len() >= 2 {
            break;
        }
    }
    let subtitle = format_station_subtitle(&companion_names, &station.display_name);
    let left_art = side_arts
        .first()
        .cloned()
        .or_else(|| station.left_art_url.clone());
    let right_art = side_arts
        .get(1)
        .cloned()
        .or_else(|| station.right_art_url.clone());

    Ok(ExpandedStationTracks {
        tracks: sequenced,
        subtitle,
        left_art,
        right_art,
    })
}

/// Search YouTube Music returning both playable studio songs (with full Artist, Album & Duration)
/// and clickable Album / Artist / Playlist cards by querying Songs + General endpoints in parallel.
pub fn fetch_rich_search(
    session: &YtmSession,
    query: &str,
    limit: usize,
) -> ProviderResult<YtmRichSearch> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(YtmRichSearch::default());
    }

    let songs_body = json!({
        "query": q,
        "params": "EgWKAQIIAWoMEAMQBBAJEAoQBRAV",
        "context": web_remix_context(session),
    });
    let general_body = json!({
        "query": q,
        "context": web_remix_context(session),
    });

    let (songs_res, general_res) = std::thread::scope(|s| {
        let h_songs = s.spawn(|| post_ytm_json("search", &songs_body, session));
        let h_gen = s.spawn(|| post_ytm_json("search", &general_body, session));
        (
            h_songs.join().unwrap_or_else(|_| {
                Err(ProviderError::Http(
                    "YouTube Music songs search worker panicked".to_string(),
                ))
            }),
            h_gen.join().unwrap_or_else(|_| {
                Err(ProviderError::Http(
                    "YouTube Music general search worker panicked".to_string(),
                ))
            }),
        )
    });

    let (songs_opt, gen_opt) = match (songs_res, general_res) {
        (Err(_), Err(err)) => return Err(err),
        (s, g) => (s.ok(), g.ok()),
    };

    let cap = limit.max(1);
    let mut tracks = Vec::new();
    let mut seen_vids = HashSet::new();
    let mut cards = Vec::new();
    let mut seen_cards = HashSet::new();

    // 1. Official Songs results first (always carry verified Artist, Album, and exact Duration)
    if let Some(ref songs_root) = songs_opt {
        collect_music_items(songs_root, "Single", cap, &mut seen_vids, &mut tracks);
    }

    // 2. General search provides Album/Artist/Playlist cards plus supplementary official videos
    if let Some(ref gen_root) = gen_opt {
        collect_search_cards(gen_root, 18, &mut seen_cards, &mut cards);
        if tracks.len() < cap {
            collect_music_items(gen_root, "YouTube Music", cap, &mut seen_vids, &mut tracks);
        }
    }

    Ok(YtmRichSearch { tracks, cards })
}

/// Search the entire YouTube Music catalog in real time with ZERO API key and ZERO login.
pub fn search_music(session: &YtmSession, query: &str, limit: usize) -> ProviderResult<Vec<Track>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let body = json!({
        "query": q,
        "params": "EgWKAQIIAWoMEAMQBBAJEAoQBRAV",
        "context": web_remix_context(session),
    });
    let root = post_ytm_json("search", &body, session)?;
    let mut tracks = Vec::new();
    let mut seen = HashSet::new();
    collect_music_items(&root, "YouTube Music", limit.max(1), &mut seen, &mut tracks);

    if tracks.is_empty() {
        let fallback_body = json!({
            "query": q,
            "context": web_remix_context(session),
        });
        if let Ok(fb_root) = post_ytm_json("search", &fallback_body, session) {
            collect_music_items(
                &fb_root,
                "YouTube Music",
                limit.max(1),
                &mut seen,
                &mut tracks,
            );
        }
    }
    Ok(tracks)
}

/// Find the best matching YouTube Music `videoId` for a Spotify/local metadata track so it can stream immediately.
pub fn resolve_best_video_id_for_track(
    session: &YtmSession,
    artist: &str,
    title: &str,
) -> ProviderResult<Option<String>> {
    let query = format!("{artist} {title}").trim().to_string();
    let hits = search_music(session, &query, 5)?;
    Ok(hits.into_iter().find_map(|t| t.youtube_video_id))
}

/// Fetch signed-in Google account name, Liked Songs, Playlists, and personalized Home feed.
pub fn fetch_account_library(session: &YtmSession) -> ProviderResult<YtmAccountLibrary> {
    if !session.is_signed_in() {
        return Err(ProviderError::Metadata(
            "Not signed in to Google / YouTube Music".to_string(),
        ));
    }

    let menu_body = json!({ "context": web_remix_context(session) });
    let menu_root = post_ytm_json("account/account_menu", &menu_body, session)?;
    let visitor_data = extract_visitor_data(&menu_root, &session.visitor_data);
    let display_name =
        find_account_name(&menu_root).unwrap_or_else(|| "Google Account".to_string());

    let mut auth_sess = session.clone();
    auth_sess.visitor_data = visitor_data.clone();

    // 1. Liked Songs (VLLM / FEmusic_liked_videos)
    let mut liked_tracks = Vec::new();
    let mut seen_liked = HashSet::new();
    for liked_browse_id in ["VLLM", "FEmusic_liked_videos"] {
        let b = json!({
            "browseId": liked_browse_id,
            "context": web_remix_context(&auth_sess),
        });
        if let Ok(r) = post_ytm_json("browse", &b, &auth_sess) {
            collect_music_items(&r, "Liked Music", 100, &mut seen_liked, &mut liked_tracks);
            if !liked_tracks.is_empty() {
                break;
            }
        }
    }
    for t in &mut liked_tracks {
        t.liked = true;
    }

    // 2. User Playlists (FEmusic_liked_playlists)
    let mut playlists = Vec::new();
    let pl_body = json!({
        "browseId": "FEmusic_liked_playlists",
        "context": web_remix_context(&auth_sess),
    });
    if let Ok(pl_root) = post_ytm_json("browse", &pl_body, &auth_sess) {
        let mut discovered = Vec::new();
        collect_playlist_refs(&pl_root, 8, &mut discovered);
        for (browse_id, name) in discovered {
            let item_body = json!({
                "browseId": browse_id,
                "context": web_remix_context(&auth_sess),
            });
            if let Ok(item_root) = post_ytm_json("browse", &item_body, &auth_sess) {
                let mut pl_tracks = Vec::new();
                let mut seen_pl = HashSet::new();
                collect_music_items(&item_root, &name, 100, &mut seen_pl, &mut pl_tracks);
                if !pl_tracks.is_empty() {
                    let pl_thumb = pl_tracks.first().and_then(|t| t.thumbnail_url.clone());
                    let pl = Playlist {
                        id: format!("ytm:pl:{browse_id}"),
                        name,
                        source: TrackSource::YouTubeMusic,
                        etag: None,
                        track_count: pl_tracks.len(),
                        description: "Synced from YouTube Music account".to_string(),
                        thumbnail_url: pl_thumb,
                    };
                    playlists.push((pl, pl_tracks));
                }
            }
        }
    }

    // 3. Personalized Home Recommendations
    let home_tracks = fetch_home_feed(&auth_sess)
        .map(|h| h.tracks)
        .unwrap_or_default();

    Ok(YtmAccountLibrary {
        display_name,
        visitor_data,
        liked_tracks,
        playlists,
        home_tracks,
    })
}

fn cached_guest_visitor_data(force_refresh: bool) -> String {
    let lock = GUEST_VISITOR_CACHE.get_or_init(|| Mutex::new(None));
    if !force_refresh {
        let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((vd, fetched_at)) = guard.as_ref()
            && fetched_at.elapsed() < Duration::from_secs(20 * 60)
            && !vd.is_empty()
        {
            return vd.clone();
        }
    }
    let guest = YtmSession::guest(String::new());
    let prime_body = json!({
        "browseId": "FEmusic_home",
        "context": web_remix_context(&guest),
    });
    let vd = post_ytm_json("browse", &prime_body, &guest)
        .map(|r| extract_visitor_data(&r, ""))
        .unwrap_or_default();
    if !vd.is_empty() {
        let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        *guard = Some((vd.clone(), Instant::now()));
    }
    vd
}

fn fetch_player_root_as_clean_guest(video_id: &str) -> ProviderResult<Value> {
    let vd = cached_guest_visitor_data(false);
    let guest = YtmSession::guest(vd);
    match fetch_player_root_once(video_id, &guest, false) {
        Ok(root) => Ok(root),
        Err(first_err) => {
            let fresh_vd = cached_guest_visitor_data(true);
            let fresh_guest = YtmSession::guest(fresh_vd);
            fetch_player_root_once(video_id, &fresh_guest, false).map_err(|_| first_err)
        }
    }
}

/// Resolve the player response. A stale/mismatched stored `visitorData` or account cookie makes
/// InnerTube answer HTTP 400 / `LOGIN_REQUIRED`; in that case retry once as a clean guest session
/// with freshly primed `visitorData` (stream URLs from guest VISIONOS are fully playable).
fn fetch_player_root(video_id: &str, session: &YtmSession) -> ProviderResult<Value> {
    if session.is_signed_in() && VISIONOS_AUTH_REJECTED.load(Ordering::Relaxed) {
        return fetch_player_root_as_clean_guest(video_id);
    }
    match fetch_player_root_once(video_id, session, false) {
        Ok(root) => Ok(root),
        Err(first_err) => {
            let retryable = match &first_err {
                ProviderError::Http(msg) => {
                    msg.contains("HTTP 400")
                        || msg.contains("HTTP 401")
                        || msg.contains("HTTP 403")
                        || msg.contains("player request failed")
                }
                ProviderError::Metadata(msg) => {
                    msg.contains("LOGIN_REQUIRED") || msg.contains("UNKNOWN")
                }
                _ => false,
            };
            if !retryable {
                return Err(first_err);
            }
            if session.is_signed_in() {
                VISIONOS_AUTH_REJECTED.store(true, Ordering::Relaxed);
            }
            fetch_player_root_as_clean_guest(video_id).map_err(|_| first_err)
        }
    }
}

fn fetch_player_root_once(
    video_id: &str,
    session: &YtmSession,
    force_fresh_visitor: bool,
) -> ProviderResult<Value> {
    if !is_valid_video_id(video_id) {
        return Err(ProviderError::Metadata(
            "Invalid YouTube video ID".to_string(),
        ));
    }

    // Ensure fresh visitorData (required by VISIONOS 1.02)
    let mut visitor_data = if force_fresh_visitor {
        String::new()
    } else {
        session.visitor_data.clone()
    };
    if visitor_data.is_empty() {
        let prime_body = json!({
            "browseId": "FEmusic_home",
            "context": web_remix_context(session),
        });
        if let Ok(r) = post_ytm_json("browse", &prime_body, session) {
            visitor_data = extract_visitor_data(&r, "");
        }
    }

    let mut client = json!({
        "clientName": "VISIONOS",
        "clientVersion": VISIONOS_VERSION,
        "hl": "en",
        "gl": "ID"
    });
    if !visitor_data.is_empty() {
        client["visitorData"] = json!(visitor_data);
    }
    let player_body = json!({
        "videoId": video_id,
        "context": { "client": client },
        "contentCheckOk": true,
        "racyCheckOk": true,
    });

    let agent = shared_http_agent(false);
    let mut req = agent
        .post(&format!("{YT_ORIGIN}/youtubei/v1/player?prettyPrint=false"))
        .header("Content-Type", "application/json")
        .header("User-Agent", VISIONOS_UA)
        .header("Origin", YT_ORIGIN)
        .header("X-YouTube-Client-Name", "101")
        .header("X-YouTube-Client-Version", VISIONOS_VERSION);
    if !visitor_data.is_empty() {
        req = req.header("X-Goog-Visitor-Id", &visitor_data);
    }
    if session.is_signed_in() {
        let auth = build_sapisidhash_header(&session.sapisid, YT_ORIGIN, unix_timestamp_secs());
        req = req
            .header("Cookie", &session.cookie)
            .header("Authorization", &auth)
            .header("X-Origin", YT_ORIGIN)
            .header("X-Goog-AuthUser", "0");
    }

    let body_str = serde_json::to_string(&player_body)
        .map_err(|e| ProviderError::Metadata(format!("Player request JSON failed: {e}")))?;
    let resp = req
        .send(&body_str)
        .map_err(|e| ProviderError::Http(format!("YouTube player request failed: {e}")))?;
    let status = resp.status().as_u16();
    let raw_json = resp
        .into_body()
        .into_with_config()
        .limit(8 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| ProviderError::Http(format!("YouTube player read failed: {e}")))?;
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http(format!(
            "YouTube player returned HTTP {status}"
        )));
    }
    let root: Value = serde_json::from_str(&raw_json)
        .map_err(|e| ProviderError::Metadata(format!("YouTube player JSON parse failed: {e}")))?;

    let play_status = root
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    if play_status != "OK" {
        let reason = root
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or("Track unavailable");
        return Err(ProviderError::Metadata(format!(
            "Stream unavailable ({play_status}): {reason}"
        )));
    }

    Ok(root)
}

/// Resolve direct YouTube audio stream via `VISIONOS 1.02` at the requested `AudioQuality` tier
/// and download in parallel Range chunks into `cache_dir`.
pub fn resolve_and_cache_audio_stream_with_quality(
    video_id: &str,
    session: &YtmSession,
    cache_dir: &Path,
    quality: AudioQuality,
) -> ProviderResult<CachedAudioStream> {
    if !is_valid_video_id(video_id) {
        return Err(ProviderError::Metadata(
            "Invalid YouTube video ID".to_string(),
        ));
    }
    fs::create_dir_all(cache_dir)?;

    let suffix = quality.cache_suffix();
    for (ext, codec, fallback_br) in [("webm", "OPUS", 150u32), ("m4a", "AAC", 128u32)] {
        let candidate = cache_dir.join(format!("{video_id}_{suffix}.{ext}"));
        if let Ok(meta) = fs::metadata(&candidate)
            && meta.len() > 16_384
        {
            if is_valid_audio_container_header(&candidate, ext) {
                touch_cache_mtime(&candidate);
                let br = match quality {
                    AudioQuality::Low => 64,
                    AudioQuality::Normal => 128,
                    AudioQuality::High | AudioQuality::Best => fallback_br,
                };
                return Ok(CachedAudioStream {
                    path: candidate,
                    codec: codec.to_string(),
                    bitrate_kbps: br,
                    duration_ms: 0,
                });
            }
            let _ = fs::remove_file(&candidate);
        }
    }

    // Legacy cache compatibility for Normal/High if already downloaded
    if matches!(quality, AudioQuality::Normal | AudioQuality::High) {
        for (ext, codec, br) in [("m4a", "AAC", 128), ("webm", "OPUS", 150)] {
            let legacy = cache_dir.join(format!("{video_id}.{ext}"));
            if let Ok(meta) = fs::metadata(&legacy)
                && meta.len() > 16_384
            {
                if is_valid_audio_container_header(&legacy, ext) {
                    touch_cache_mtime(&legacy);
                    return Ok(CachedAudioStream {
                        path: legacy,
                        codec: codec.to_string(),
                        bitrate_kbps: br,
                        duration_ms: 0,
                    });
                }
                let _ = fs::remove_file(&legacy);
            }
        }
    }

    let root = fetch_player_root(video_id, session)?;
    let selected = select_audio_format_for_quality(&root, quality).ok_or_else(|| {
        ProviderError::Metadata("No playable audio format found in stream response".to_string())
    })?;

    let dest_path = cache_dir.join(format!("{video_id}_{suffix}.{}", selected.ext));
    let nonce = ferric_core::secrets::csprng_hex(6);
    let part_path = cache_dir.join(format!(
        "{video_id}_{suffix}.{}.{nonce}.part",
        std::process::id()
    ));
    if let Err(err) = download_ranged_parallel(&selected.url, selected.content_length, &part_path) {
        let _ = fs::remove_file(&part_path);
        return Err(err);
    }
    if !is_valid_audio_container_header(&part_path, selected.ext) {
        let _ = fs::remove_file(&part_path);
        return Err(ProviderError::Metadata(
            "Downloaded audio stream failed container header validation".to_string(),
        ));
    }
    if let Err(err) = fs::rename(&part_path, &dest_path) {
        let _ = fs::remove_file(&part_path);
        // Another concurrent prefetch/play worker may have already finalized `dest_path`.
        if !fs::metadata(&dest_path).is_ok_and(|m| m.len() > 16_384)
            || !is_valid_audio_container_header(&dest_path, selected.ext)
        {
            return Err(err.into());
        }
    }
    prune_cache_dir_lru(cache_dir, MAX_STREAM_CACHE_BYTES, Some(&dest_path));

    Ok(CachedAudioStream {
        path: dest_path,
        codec: selected.codec.to_string(),
        bitrate_kbps: selected.bitrate_kbps,
        duration_ms: selected.duration_ms,
    })
}

/// Convenience wrapper using default High audio quality.
pub fn resolve_and_cache_audio_stream(
    video_id: &str,
    session: &YtmSession,
    cache_dir: &Path,
) -> ProviderResult<CachedAudioStream> {
    resolve_and_cache_audio_stream_with_quality(video_id, session, cache_dir, AudioQuality::High)
}

/// Resolved video stream set distinguishing true Official Music Videos (OMV / widescreen)
/// from static YouTube Music Art Track cover videos (`" - Topic"` / 1:1 album cover).
#[derive(Debug, Clone)]
pub struct ResolvedVideoStreams {
    pub video_id: String,
    pub is_official_mv: bool,
    pub streams: Vec<VideoStreamOption>,
}

#[derive(Debug, Clone)]
struct MusicVideoCandidate {
    video_id: String,
    title: String,
    subtitle: String,
    is_omv: bool,
    duration_ms: u64,
}

fn is_static_art_track(root: &Value, streams: &[VideoStreamOption]) -> bool {
    let author = root
        .pointer("/videoDetails/author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let desc = root
        .pointer("/videoDetails/shortDescription")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if author.ends_with(" - topic") || desc.contains("provided to youtube by") {
        return true;
    }
    !streams.is_empty()
        && streams
            .iter()
            .all(|s| s.width > 0 && s.height > 0 && (s.width as f32 / s.height as f32) < 1.18)
}

fn tokenize_alnum(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn score_music_video_candidate(
    cand: &MusicVideoCandidate,
    track_artist: &str,
    track_title: &str,
    track_duration_ms: u64,
) -> i32 {
    let target_core = normalize_core_title(track_title);
    let cand_core = normalize_core_title(&cand.title);
    let cand_full = format!("{} {}", cand.title, cand.subtitle).to_ascii_lowercase();
    let target_title_low = track_title.to_ascii_lowercase();

    let target_tokens: Vec<String> = tokenize_alnum(&target_core)
        .into_iter()
        .filter(|t| t.len() >= 2)
        .collect();
    let cand_tokens: HashSet<String> = tokenize_alnum(&cand_core).into_iter().collect();

    let title_matched = if !target_core.is_empty() && cand_core == target_core {
        100
    } else if !target_core.is_empty()
        && (cand_core.contains(&target_core) || target_core.contains(&cand_core))
    {
        75
    } else if !target_tokens.is_empty() {
        let hits = target_tokens
            .iter()
            .filter(|t| cand_tokens.contains(*t))
            .count();
        let ratio = hits as f32 / target_tokens.len() as f32;
        if ratio >= 0.75 {
            65
        } else if ratio >= 0.50 {
            35
        } else {
            return -200;
        }
    } else {
        0
    };

    let mut score = title_matched;
    if cand.is_omv {
        score += 85;
    }

    let artist_tokens = extract_artist_tokens(track_artist);
    let sub_low = cand.subtitle.to_ascii_lowercase();
    let primary_art = primary_artist_key(track_artist).to_ascii_lowercase();
    if !primary_art.is_empty()
        && (sub_low.contains(&primary_art) || cand_full.contains(&primary_art))
    {
        score += 55;
    } else if !artist_tokens.is_empty()
        && artist_tokens
            .iter()
            .any(|t| sub_low.contains(t) || cand_full.contains(t))
    {
        score += 35;
    } else if !artist_tokens.is_empty() {
        score -= 55;
    }

    for positive in ["official music video", "official video", "(official"] {
        if cand_full.contains(positive) {
            score += 22;
            break;
        }
    }

    for bad in [
        "slowed",
        "reverb",
        "sped up",
        "nightcore",
        "8d audio",
        "karaoke",
        "instrumental",
        "piano cover",
        "guitar cover",
        "tutorial",
        "reaction",
        "cover",
        "remix",
        "mashup",
        "clean version",
    ] {
        if cand_full.contains(bad) && !target_title_low.contains(bad) {
            score -= 115;
        }
    }
    for static_hint in ["lyrics", "lyric video", "official audio", " - topic"] {
        if cand_full.contains(static_hint) && !target_title_low.contains(static_hint) {
            score -= 65;
        }
    }

    if track_duration_ms > 0 && cand.duration_ms > 0 {
        let diff_ms = track_duration_ms.abs_diff(cand.duration_ms);
        if diff_ms <= 45_000 {
            score += 20;
        } else if diff_ms > 180_000 {
            score -= 45;
        }
    }

    score
}

fn collect_video_search_candidates(
    val: &Value,
    out: &mut Vec<MusicVideoCandidate>,
    seen: &mut HashSet<String>,
) {
    match val {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer")
                && let Some(cand) = parse_video_search_candidate(item)
                && seen.insert(cand.video_id.clone())
            {
                out.push(cand);
            }
            for v in map.values() {
                collect_video_search_candidates(v, out, seen);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_video_search_candidates(v, out, seen);
            }
        }
        _ => {}
    }
}

fn find_music_video_type(val: &Value) -> Option<&str> {
    match val {
        Value::Object(map) => {
            if let Some(s) = map.get("musicVideoType").and_then(Value::as_str) {
                return Some(s);
            }
            for v in map.values() {
                if let Some(found) = find_music_video_type(v) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_music_video_type),
        _ => None,
    }
}

fn find_first_video_id(val: &Value) -> Option<String> {
    match val {
        Value::Object(map) => {
            if let Some(id) = map.get("videoId").and_then(Value::as_str)
                && is_valid_video_id(id)
            {
                return Some(id.to_string());
            }
            for v in map.values() {
                if let Some(found) = find_first_video_id(v) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(find_first_video_id),
        _ => None,
    }
}

fn parse_video_search_candidate(item: &Value) -> Option<MusicVideoCandidate> {
    let video_id = find_first_video_id(item)?;
    let vtype = find_music_video_type(item).unwrap_or("");
    // Skip static Art Tracks (`ATV`) explicitly when looking for real music videos
    if vtype == "MUSIC_VIDEO_TYPE_ATV" {
        return None;
    }
    let cols = item.get("flexColumns").and_then(Value::as_array)?;
    let title = cols
        .first()
        .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
        .and_then(|v| extract_runs_text(Some(v)))
        .unwrap_or_default();
    if title.trim().is_empty() {
        return None;
    }
    let subtitle = cols
        .get(1)
        .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
        .and_then(|v| extract_runs_text(Some(v)))
        .unwrap_or_default();
    let duration_ms = subtitle
        .split('•')
        .rev()
        .find_map(|part| parse_clock_ms(part.trim()))
        .unwrap_or(0);
    Some(MusicVideoCandidate {
        video_id,
        title,
        subtitle,
        is_omv: vtype == "MUSIC_VIDEO_TYPE_OMV",
        duration_ms,
    })
}

fn search_official_music_video_candidates(
    session: &YtmSession,
    artist: &str,
    title: &str,
    track_duration_ms: u64,
    exclude_vid: Option<&str>,
) -> Vec<MusicVideoCandidate> {
    let clean_title = normalize_core_title(title);
    let clean_artist = primary_artist_key(artist);
    let query = if !clean_artist.is_empty() && !clean_title.is_empty() {
        format!("{clean_artist} {clean_title}")
    } else {
        format!("{artist} {title}").trim().to_string()
    };
    if query.is_empty() {
        return Vec::new();
    }
    // YouTube Music "Videos" search tab filter: surfaces `MUSIC_VIDEO_TYPE_OMV` official music videos
    let body = json!({
        "query": query,
        "params": "EgWKAQIQAWoKEAMQBBAJEAoQBQ%3D%3D",
        "context": web_remix_context(session),
    });
    let Ok(root) = post_ytm_json("search", &body, session) else {
        return Vec::new();
    };
    let mut raw = Vec::new();
    let mut seen = HashSet::new();
    collect_video_search_candidates(&root, &mut raw, &mut seen);

    let mut scored: Vec<(i32, MusicVideoCandidate)> = raw
        .into_iter()
        .filter(|c| exclude_vid != Some(c.video_id.as_str()))
        .map(|c| {
            let s = score_music_video_candidate(&c, artist, title, track_duration_ms);
            (s, c)
        })
        .filter(|(s, _)| *s >= 45)
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().take(3).map(|(_, c)| c).collect()
}

fn extract_video_streams_from_player(root: &Value) -> ProviderResult<Vec<VideoStreamOption>> {
    let Some(formats) = root
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
    else {
        return Err(ProviderError::Metadata(
            "No adaptive video streams available".to_string(),
        ));
    };

    let mut best_by_tier: Vec<(VideoQuality, u32, VideoStreamOption)> = Vec::new();
    for fmt in formats {
        let Some(url) = fmt.get("url").and_then(Value::as_str) else {
            continue;
        };
        if !is_allowed_stream_url(url) {
            continue;
        }
        let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("video/") {
            continue;
        }
        let height = fmt.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
        let width = fmt.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
        let bitrate = fmt.get("bitrate").and_then(Value::as_u64).unwrap_or(0) as u32;
        let tier = match height {
            320..=400 => VideoQuality::P360,
            401..=540 => VideoQuality::P480,
            541..=800 => VideoQuality::P720,
            801..=1200 => VideoQuality::P1080,
            _ => continue,
        };
        // Strongly prefer H.264 MP4 (`avc1`) for universal DXVA hardware decode in WebView2,
        // falling back to VP9 WebM if unavailable.
        let codec_rank = if mime.contains("avc1") {
            3_000_000_u32
        } else if mime.starts_with("video/mp4") && !mime.contains("av01") {
            2_000_000_u32
        } else if mime.contains("vp9") {
            1_000_000_u32
        } else {
            250_000_u32
        };
        let score = codec_rank.saturating_add(bitrate / 1_000);
        let opt = VideoStreamOption {
            quality: tier,
            label: tier.label().to_string(),
            width,
            height,
            bitrate_kbps: bitrate / 1_000,
            url: url.to_string(),
            mime: mime.to_string(),
        };
        if let Some(slot) = best_by_tier.iter_mut().find(|(q, _, _)| *q == tier) {
            if score > slot.1 {
                *slot = (tier, score, opt);
            }
        } else {
            best_by_tier.push((tier, score, opt));
        }
    }

    best_by_tier.sort_by_key(|(q, _, _)| q.target_height());
    let out: Vec<VideoStreamOption> = best_by_tier.into_iter().map(|(_, _, o)| o).collect();
    if out.is_empty() {
        return Err(ProviderError::Metadata(
            "No compatible video streams found".to_string(),
        ));
    }
    Ok(out)
}

/// Resolve the real Official Music Video (widescreen OMV) for a track instead of playing a static
/// album-cover YouTube Art Track (`" - Topic"`), falling back cleanly only when no MV exists.
pub fn resolve_music_video_streams(
    session: &YtmSession,
    artist: &str,
    title: &str,
    existing_vid: Option<&str>,
    track_duration_ms: u64,
) -> ProviderResult<ResolvedVideoStreams> {
    let valid_existing = existing_vid.map(str::trim).filter(|v| is_valid_video_id(v));

    let mut fallback_art: Option<ResolvedVideoStreams> = None;
    if let Some(vid) = valid_existing
        && let Ok(root) = fetch_player_root(vid, session)
        && let Ok(streams) = extract_video_streams_from_player(&root)
    {
        if !is_static_art_track(&root, &streams) {
            return Ok(ResolvedVideoStreams {
                video_id: vid.to_string(),
                is_official_mv: true,
                streams,
            });
        }
        fallback_art = Some(ResolvedVideoStreams {
            video_id: vid.to_string(),
            is_official_mv: false,
            streams,
        });
    }

    let candidates = search_official_music_video_candidates(
        session,
        artist,
        title,
        track_duration_ms,
        valid_existing,
    );
    for cand in candidates.into_iter().take(2) {
        if let Ok(root) = fetch_player_root(&cand.video_id, session)
            && let Ok(streams) = extract_video_streams_from_player(&root)
            && !is_static_art_track(&root, &streams)
        {
            return Ok(ResolvedVideoStreams {
                video_id: cand.video_id,
                is_official_mv: true,
                streams,
            });
        }
    }

    if let Some(fb) = fallback_art {
        return Ok(fb);
    }

    let vid = resolve_best_video_id_for_track(session, artist, title)?
        .ok_or_else(|| ProviderError::Metadata("No music video found for this song".to_string()))?;
    let root = fetch_player_root(&vid, session)?;
    let streams = extract_video_streams_from_player(&root)?;
    let is_official_mv = !is_static_art_track(&root, &streams);
    Ok(ResolvedVideoStreams {
        video_id: vid,
        is_official_mv,
        streams,
    })
}

/// Fetch direct hardware-decodable MP4/WebM video stream URLs for 360p, 480p, 720p, and 1080p
/// so Ferric plays official music videos natively inside the app with instant quality switching.
pub fn fetch_video_streams(
    video_id: &str,
    session: &YtmSession,
) -> ProviderResult<Vec<VideoStreamOption>> {
    let root = fetch_player_root(video_id, session)?;
    extract_video_streams_from_player(&root)
}

struct SelectedStreamFormat {
    url: String,
    ext: &'static str,
    codec: &'static str,
    bitrate_kbps: u32,
    content_length: u64,
    duration_ms: u64,
}

fn select_audio_format_for_quality(
    root: &Value,
    quality: AudioQuality,
) -> Option<SelectedStreamFormat> {
    let formats = root
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)?;

    let mut best: Option<(u32, SelectedStreamFormat)> = None;
    for fmt in formats {
        let Some(url) = fmt.get("url").and_then(Value::as_str) else {
            continue;
        };
        if !is_allowed_stream_url(url) {
            continue;
        }
        let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("audio/") {
            continue;
        }
        let itag = fmt.get("itag").and_then(Value::as_u64).unwrap_or(0);
        let bitrate = fmt.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
        let kbps = (bitrate / 1_000) as u32;
        let content_length = fmt
            .get("contentLength")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let duration_ms = fmt
            .get("approxDurationMs")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let (ext, codec) = if mime.starts_with("audio/mp4") {
            ("m4a", "AAC")
        } else if mime.starts_with("audio/webm") {
            ("webm", "OPUS")
        } else {
            continue;
        };

        let score: u32 = match quality {
            AudioQuality::Low => {
                // Prefer compact 48-72 kbps AAC/Opus streams for low bandwidth
                if itag == 139 {
                    950_000
                } else if itag == 250 || itag == 249 {
                    920_000
                } else if kbps <= 80 {
                    850_000 - kbps
                } else {
                    400_000u32.saturating_sub(kbps)
                }
            }
            AudioQuality::Normal => {
                // Standard 128 kbps AAC (itag 140)
                if itag == 140 {
                    980_000
                } else if (96..=135).contains(&kbps) {
                    900_000 + kbps
                } else {
                    500_000u32.saturating_sub(kbps.abs_diff(128))
                }
            }
            AudioQuality::High => {
                // High transparency Opus (itag 251) or top AAC
                if itag == 251 {
                    990_000u32.saturating_add(kbps)
                } else if itag == 140 {
                    920_000u32.saturating_add(kbps)
                } else if codec == "OPUS" {
                    880_000u32.saturating_add(kbps)
                } else {
                    800_000u32.saturating_add(kbps)
                }
            }
            AudioQuality::Best => {
                // Absolute highest bitrate available (256k AAC/Opus -> 160k Opus -> 128k AAC)
                let bonus = if itag == 141 || itag == 774 {
                    200_000
                } else if itag == 251 {
                    80_000
                } else if itag == 140 {
                    40_000
                } else {
                    0
                };
                bonus + (bitrate.min(999_999) as u32)
            }
        };

        let candidate = SelectedStreamFormat {
            url: url.to_string(),
            ext,
            codec,
            bitrate_kbps: kbps,
            content_length,
            duration_ms,
        };
        if best.as_ref().is_none_or(|(best_s, _)| score > *best_s) {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, f)| f)
}

fn open_verified_stream_get(
    initial_url: &str,
    range_hdr: Option<&str>,
    _timeout_secs: u64,
) -> ProviderResult<(u16, ureq::Body)> {
    let mut current_url = initial_url.to_string();
    let agent = shared_stream_agent();
    for _ in 0..=3 {
        if !is_allowed_stream_url(&current_url) {
            return Err(ProviderError::Http(
                "Refused untrusted media stream host".to_string(),
            ));
        }
        let mut req = agent.get(&current_url).header("User-Agent", VISIONOS_UA);
        if let Some(range) = range_hdr {
            req = req.header("Range", range);
        }
        let resp = req
            .call()
            .map_err(|e| ProviderError::Http(format!("Audio CDN request failed: {e}")))?;
        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| ProviderError::Http("Invalid stream redirect".to_string()))?;
            current_url = loc.to_string();
            continue;
        }
        return Ok((status, resp.into_body()));
    }
    Err(ProviderError::Http(
        "Too many audio stream redirects".to_string(),
    ))
}

fn download_single_stream(url: &str, expected_len: u64, dest: &Path) -> ProviderResult<()> {
    let (status, body) = open_verified_stream_get(url, None, 45)?;
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http(format!("Audio CDN HTTP {status}")));
    }
    let mut reader = body
        .into_with_config()
        .limit(MAX_SINGLE_STREAM_BYTES)
        .reader();
    let mut file = File::create(dest)?;
    let written = std::io::copy(&mut reader, &mut file)
        .map_err(|e| ProviderError::Http(format!("Audio stream read failed: {e}")))?;
    file.flush()?;
    if written == 0 {
        return Err(ProviderError::Http(
            "Audio stream returned empty body".to_string(),
        ));
    }
    if expected_len > 0 && written != expected_len {
        return Err(ProviderError::Http(format!(
            "Audio stream truncated ({written} of {expected_len} bytes)"
        )));
    }
    Ok(())
}

fn download_ranged_parallel(url: &str, content_length: u64, dest: &Path) -> ProviderResult<()> {
    if !is_allowed_stream_url(url) {
        return Err(ProviderError::Http(
            "Refused untrusted audio stream URL".to_string(),
        ));
    }
    if content_length > MAX_SINGLE_STREAM_BYTES {
        return Err(ProviderError::Http(
            "Audio stream exceeds 64 MiB cache safety cap".to_string(),
        ));
    }
    if content_length == 0 {
        return download_single_stream(url, 0, dest);
    }

    let mut ranges = Vec::new();
    let mut start = 0u64;
    while start < content_length {
        let end = (start + STREAM_CHUNK_BYTES - 1).min(content_length - 1);
        ranges.push((start, end));
        start = end + 1;
    }

    enum ChunkOutcome {
        Partial(Vec<u8>),
        Full200Fallback,
    }

    let mut file = File::create(dest)?;
    let mut total_written = 0u64;
    let mut idx = 0usize;
    while idx < ranges.len() {
        let batch_end = (idx + STREAM_CONCURRENCY).min(ranges.len());
        let batch = &ranges[idx..batch_end];
        let results: Vec<ProviderResult<ChunkOutcome>> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(batch.len());
            for &(s, e) in batch {
                handles.push(scope.spawn(move || {
                    let expected_len = e - s + 1;
                    let range_hdr = format!("bytes={s}-{e}");
                    let mut last_err = None;
                    for attempt in 0..3 {
                        if attempt > 0 {
                            std::thread::sleep(Duration::from_millis(150 * attempt as u64));
                        }
                        let (st, body) = match open_verified_stream_get(url, Some(&range_hdr), 25) {
                            Ok(pair) => pair,
                            Err(err) => {
                                last_err = Some(err);
                                continue;
                            }
                        };
                        if st == 200 {
                            // Server ignored Range header; do NOT concatenate full body N times!
                            return Ok(ChunkOutcome::Full200Fallback);
                        }
                        if st != 206 {
                            last_err = Some(ProviderError::Http(format!(
                                "Audio chunk returned HTTP {st}"
                            )));
                            if st >= 500 || st == 429 {
                                continue;
                            }
                            break;
                        }
                        let expected_max = expected_len.min(4 * 1024 * 1024);
                        match body.into_with_config().limit(expected_max).read_to_vec() {
                            Ok(bytes) if bytes.len() as u64 == expected_len => {
                                return Ok(ChunkOutcome::Partial(bytes));
                            }
                            Ok(bytes) => {
                                last_err = Some(ProviderError::Http(format!(
                                    "Audio chunk short read ({} of {expected_len} bytes)",
                                    bytes.len()
                                )));
                            }
                            Err(err) => {
                                last_err = Some(ProviderError::Http(format!(
                                    "Audio chunk read failed: {err}"
                                )));
                            }
                        }
                    }
                    Err(last_err.unwrap_or_else(|| {
                        ProviderError::Http("Audio chunk download failed".to_string())
                    }))
                }));
            }
            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        Err(ProviderError::Http(
                            "Audio worker thread panicked".to_string(),
                        ))
                    })
                })
                .collect()
        });
        for res in results {
            match res? {
                ChunkOutcome::Partial(chunk) => {
                    file.write_all(&chunk)?;
                    total_written = total_written.saturating_add(chunk.len() as u64);
                }
                ChunkOutcome::Full200Fallback => {
                    drop(file);
                    return download_single_stream(url, content_length, dest);
                }
            }
        }
        idx = batch_end;
    }

    file.flush()?;
    if total_written != content_length {
        return Err(ProviderError::Http(format!(
            "Audio download incomplete ({total_written} of {content_length} bytes)"
        )));
    }
    Ok(())
}

fn find_account_name(val: &Value) -> Option<String> {
    match val {
        Value::Object(map) => {
            if let Some(hdr) = map.get("activeAccountHeaderRenderer")
                && let Some(name) = extract_runs_text(hdr.get("accountName"))
            {
                return Some(name);
            }
            for child in map.values() {
                if let Some(found) = find_account_name(child) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for child in arr {
                if let Some(found) = find_account_name(child) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn collect_playlist_refs(val: &Value, limit: usize, out: &mut Vec<(String, String)>) {
    if out.len() >= limit {
        return;
    }
    match val {
        Value::Object(map) => {
            if let Some(two_row) = map.get("musicTwoRowItemRenderer") {
                let browse_id = two_row
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str);
                let title = extract_runs_text(two_row.get("title"));
                if let (Some(bid), Some(name)) = (browse_id, title)
                    && bid.starts_with("VL")
                    && bid != "VLLM"
                    && !out.iter().any(|(existing, _)| existing == bid)
                {
                    out.push((bid.to_string(), name));
                }
            }
            for child in map.values() {
                if out.len() >= limit {
                    break;
                }
                collect_playlist_refs(child, limit, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                if out.len() >= limit {
                    break;
                }
                collect_playlist_refs(child, limit, out);
            }
        }
        _ => {}
    }
}

pub fn collect_music_items(
    val: &Value,
    default_album: &str,
    limit: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<Track>,
) {
    if out.len() >= limit {
        return;
    }
    match val {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer")
                && let Some(track) = parse_responsive_item(item, default_album)
                && seen.insert(track.youtube_video_id.clone().unwrap_or_default())
            {
                out.push(track);
            }
            if let Some(two_row) = map.get("musicTwoRowItemRenderer")
                && let Some(track) = parse_two_row_song(two_row, default_album)
                && seen.insert(track.youtube_video_id.clone().unwrap_or_default())
            {
                out.push(track);
            }
            for child in map.values() {
                if out.len() >= limit {
                    break;
                }
                collect_music_items(child, default_album, limit, seen, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                if out.len() >= limit {
                    break;
                }
                collect_music_items(child, default_album, limit, seen, out);
            }
        }
        _ => {}
    }
}

fn is_non_music_subtitle(subtitle: &str) -> bool {
    let low = subtitle.to_ascii_lowercase();
    low.contains("watching")
        || low.contains("live now")
        || low.contains("livestream")
        || low.starts_with("episode •")
        || low.contains("• episode •")
        || low.contains("podcast")
}

fn parse_responsive_item(item: &Value, default_album: &str) -> Option<Track> {
    let vid = item
        .pointer("/playlistItemData/videoId")
        .or_else(|| item.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .or_else(|| item.pointer("/navigationEndpoint/watchEndpoint/videoId"))
        .and_then(Value::as_str)?;
    if !is_valid_video_id(vid) {
        return None;
    }

    let flex_cols = item.get("flexColumns").and_then(Value::as_array)?;
    let raw_title = flex_cols.first().and_then(|c| {
        extract_runs_text(c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
    })?;
    if raw_title.is_empty() {
        return None;
    }

    let mut extra_parts: Vec<String> = Vec::new();
    for col in flex_cols.iter().skip(1) {
        if let Some(txt) =
            extract_runs_text(col.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
            && !txt.trim().is_empty()
        {
            extra_parts.push(txt);
        }
    }
    let subtitle = extra_parts.join(" • ");
    if is_non_music_subtitle(&subtitle) {
        return None;
    }

    let (artist, album, mut duration_ms) = parse_ytm_subtitle(&subtitle, default_album);
    if duration_ms == 0
        && let Some(fixed) = item.get("fixedColumns").and_then(Value::as_array)
    {
        for fc in fixed {
            if let Some(txt) =
                extract_runs_text(fc.pointer("/musicResponsiveListItemFixedColumnRenderer/text"))
                && let Some(ms) = parse_clock_ms(&txt)
            {
                duration_ms = ms;
                break;
            }
        }
    }
    if duration_ms == 0
        && let Some(len_txt) = extract_runs_text(item.get("lengthText"))
        && let Some(ms) = parse_clock_ms(&len_txt)
    {
        duration_ms = ms;
    }

    let cleaned = clean_youtube_metadata(&raw_title, &artist);
    let thumbnail_url = extract_best_thumbnail(item, Some(vid));
    Some(Track {
        id: format!("yt:video:{vid}"),
        title: cleaned.title,
        artist: cleaned.artist,
        album,
        duration_ms,
        path: None,
        source: TrackSource::YouTubeMusic,
        codec: "YT AAC".to_string(),
        sample_rate: 44_100,
        bit_depth: 16,
        bitrate_kbps: 128,
        isrc: None,
        spotify_id: None,
        youtube_video_id: Some(vid.to_string()),
        thumbnail_url,
        replay_gain_db: 0,
        liked: false,
    })
}

fn parse_two_row_song(item: &Value, default_album: &str) -> Option<Track> {
    let vid = item
        .pointer("/navigationEndpoint/watchEndpoint/videoId")
        .or_else(|| {
            item.pointer("/thumbnailOverlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId")
        })
        .and_then(Value::as_str)?;
    if !is_valid_video_id(vid) {
        return None;
    }
    let raw_title = extract_runs_text(item.get("title"))?;
    let subtitle = extract_runs_text(item.get("subtitle")).unwrap_or_default();
    if is_non_music_subtitle(&subtitle) {
        return None;
    }
    let (artist, album, duration_ms) = parse_ytm_subtitle(&subtitle, default_album);
    let cleaned = clean_youtube_metadata(&raw_title, &artist);
    let thumbnail_url = extract_best_thumbnail(item, Some(vid));
    Some(Track {
        id: format!("yt:video:{vid}"),
        title: cleaned.title,
        artist: cleaned.artist,
        album,
        duration_ms,
        path: None,
        source: TrackSource::YouTubeMusic,
        codec: "YT AAC".to_string(),
        sample_rate: 44_100,
        bit_depth: 16,
        bitrate_kbps: 128,
        isrc: None,
        spotify_id: None,
        youtube_video_id: Some(vid.to_string()),
        thumbnail_url,
        replay_gain_db: 0,
        liked: false,
    })
}

fn collect_mood_chips(val: &Value, out: &mut Vec<YtmMoodChip>) {
    if out.len() >= 14 {
        return;
    }
    match val {
        Value::Object(map) => {
            if let Some(chip) = map.get("chipCloudChipRenderer")
                && let Some(title) = extract_runs_text(chip.get("text"))
                && let Some(params) = chip
                    .pointer("/navigationEndpoint/browseEndpoint/params")
                    .and_then(Value::as_str)
                && !params.is_empty()
                && !out.iter().any(|c| c.title.eq_ignore_ascii_case(&title))
            {
                out.push(YtmMoodChip {
                    title,
                    params: params.to_string(),
                });
            }
            for child in map.values() {
                if out.len() >= 14 {
                    break;
                }
                collect_mood_chips(child, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                if out.len() >= 14 {
                    break;
                }
                collect_mood_chips(child, out);
            }
        }
        _ => {}
    }
}

fn classify_page_type(page_type: Option<&str>, browse_id: Option<&str>) -> YtmCardKind {
    if let Some(pt) = page_type {
        if pt.contains("ALBUM") {
            return YtmCardKind::Album;
        }
        if pt.contains("ARTIST") {
            return YtmCardKind::Artist;
        }
        if pt.contains("PLAYLIST") {
            return YtmCardKind::Playlist;
        }
    }
    if let Some(bid) = browse_id {
        if bid.starts_with("MPREb_") || bid.starts_with("OLAK5uy_") {
            return YtmCardKind::Album;
        }
        if bid.starts_with("UC") {
            return YtmCardKind::Artist;
        }
        if bid.starts_with("VL") || bid.starts_with("PL") || bid.starts_with("RD") {
            return YtmCardKind::Playlist;
        }
    }
    YtmCardKind::Song
}

fn parse_two_row_card(item: &Value) -> Option<YtmShelfCard> {
    let title = extract_runs_text(item.get("title"))?;
    let subtitle = extract_runs_text(item.get("subtitle")).unwrap_or_default();
    let browse_id = item
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let page_type = item
        .pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(Value::as_str);
    let video_id = item
        .pointer("/navigationEndpoint/watchEndpoint/videoId")
        .or_else(|| {
            item.pointer("/thumbnailOverlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId")
        })
        .and_then(Value::as_str)
        .filter(|v| is_valid_video_id(v))
        .map(ToOwned::to_owned);
    let playlist_id = item
        .pointer("/navigationEndpoint/watchPlaylistEndpoint/playlistId")
        .or_else(|| item.pointer("/navigationEndpoint/watchEndpoint/playlistId"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());

    let resolved_browse = browse_id.or_else(|| {
        playlist_id.map(|pid| {
            if pid.starts_with("VL") {
                pid.to_string()
            } else {
                format!("VL{pid}")
            }
        })
    });

    if resolved_browse.is_none() && video_id.is_none() {
        return None;
    }

    let kind = if video_id.is_some() && page_type.is_none() && resolved_browse.is_none() {
        YtmCardKind::Song
    } else {
        classify_page_type(page_type, resolved_browse.as_deref())
    };
    let thumbnail_url = extract_best_thumbnail(item, video_id.as_deref());
    let id = resolved_browse
        .clone()
        .or_else(|| video_id.as_ref().map(|v| format!("yt:video:{v}")))
        .unwrap_or_else(|| title.clone());

    Some(YtmShelfCard {
        id,
        title,
        subtitle,
        kind,
        browse_id: resolved_browse,
        video_id,
        thumbnail_url,
    })
}

fn collect_home_shelves(
    val: &Value,
    quick_picks: &mut Vec<Track>,
    seen_tracks: &mut HashSet<String>,
    shelves: &mut Vec<YtmHomeShelf>,
) {
    match val {
        Value::Object(map) => {
            if let Some(carousel) = map.get("musicCarouselShelfRenderer") {
                let shelf_title = extract_runs_text(
                    carousel.pointer("/header/musicCarouselShelfBasicHeaderRenderer/title"),
                )
                .unwrap_or_default();
                let mut cards = Vec::new();
                if let Some(contents) = carousel.get("contents").and_then(Value::as_array) {
                    for entry in contents {
                        if let Some(resp) = entry.get("musicResponsiveListItemRenderer")
                            && let Some(track) = parse_responsive_item(resp, &shelf_title)
                            && !track.album.to_ascii_lowercase().contains("watching")
                            && seen_tracks
                                .insert(track.youtube_video_id.clone().unwrap_or_default())
                        {
                            quick_picks.push(track);
                        }
                        if let Some(two_row) = entry.get("musicTwoRowItemRenderer") {
                            if let Some(track) = parse_two_row_song(two_row, &shelf_title)
                                && !track.album.to_ascii_lowercase().contains("watching")
                                && seen_tracks
                                    .insert(track.youtube_video_id.clone().unwrap_or_default())
                            {
                                quick_picks.push(track);
                            }
                            if let Some(card) = parse_two_row_card(two_row)
                                && card.thumbnail_url.is_some()
                                && !cards.iter().any(|c: &YtmShelfCard| c.id == card.id)
                            {
                                cards.push(card);
                            }
                        }
                    }
                }
                if !shelf_title.is_empty()
                    && !cards.is_empty()
                    && shelves.len() < 24
                    && !shelves
                        .iter()
                        .any(|s| s.title.eq_ignore_ascii_case(&shelf_title))
                {
                    shelves.push(YtmHomeShelf {
                        title: shelf_title,
                        cards,
                    });
                }
            }
            for child in map.values() {
                collect_home_shelves(child, quick_picks, seen_tracks, shelves);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                collect_home_shelves(child, quick_picks, seen_tracks, shelves);
            }
        }
        _ => {}
    }
}

fn extract_browse_header(val: &Value) -> (String, String, String, Option<String>) {
    match val {
        Value::Object(map) => {
            for key in [
                "musicResponsiveHeaderRenderer",
                "musicDetailHeaderRenderer",
                "musicImmersiveHeaderRenderer",
                "musicVisualHeaderRenderer",
            ] {
                if let Some(hdr) = map.get(key) {
                    let title = extract_runs_text(hdr.get("title")).unwrap_or_default();
                    let subtitle = extract_runs_text(hdr.get("subtitle"))
                        .or_else(|| extract_runs_text(hdr.get("secondSubtitle")))
                        .unwrap_or_default();
                    let desc =
                        extract_runs_text(hdr.get("description"))
                            .or_else(|| {
                                extract_runs_text(hdr.pointer(
                                    "/description/musicDescriptionShelfRenderer/description",
                                ))
                            })
                            .unwrap_or_default();
                    let thumb = extract_best_thumbnail(hdr, None);
                    if !title.is_empty() {
                        return (title, subtitle, desc, thumb);
                    }
                }
            }
            for child in map.values() {
                let res = extract_browse_header(child);
                if !res.0.is_empty() {
                    return res;
                }
            }
            (String::new(), String::new(), String::new(), None)
        }
        Value::Array(arr) => {
            for child in arr {
                let res = extract_browse_header(child);
                if !res.0.is_empty() {
                    return res;
                }
            }
            (String::new(), String::new(), String::new(), None)
        }
        _ => (String::new(), String::new(), String::new(), None),
    }
}

fn collect_radio_panel_items(
    val: &Value,
    limit: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<Track>,
) {
    if out.len() >= limit {
        return;
    }
    match val {
        Value::Object(map) => {
            if let Some(panel) = map.get("playlistPanelVideoRenderer")
                && let Some(vid) = panel.get("videoId").and_then(Value::as_str)
                && is_valid_video_id(vid)
                && seen.insert(vid.to_string())
                && let Some(raw_title) = extract_runs_text(panel.get("title"))
            {
                let sub = extract_runs_text(panel.get("longBylineText"))
                    .or_else(|| extract_runs_text(panel.get("shortBylineText")))
                    .unwrap_or_default();
                let (artist, album, mut dur_ms) = parse_ytm_subtitle(&sub, "YouTube Music Radio");
                if dur_ms == 0
                    && let Some(len_txt) = extract_runs_text(panel.get("lengthText"))
                    && let Some(ms) = parse_clock_ms(&len_txt)
                {
                    dur_ms = ms;
                }
                let cleaned = clean_youtube_metadata(&raw_title, &artist);
                let thumb = extract_best_thumbnail(panel, Some(vid));
                out.push(Track {
                    id: format!("yt:video:{vid}"),
                    title: cleaned.title,
                    artist: cleaned.artist,
                    album,
                    duration_ms: dur_ms,
                    path: None,
                    source: TrackSource::YouTubeMusic,
                    codec: "YT AAC".to_string(),
                    sample_rate: 44_100,
                    bit_depth: 16,
                    bitrate_kbps: 128,
                    isrc: None,
                    spotify_id: None,
                    youtube_video_id: Some(vid.to_string()),
                    thumbnail_url: thumb,
                    replay_gain_db: 0,
                    liked: false,
                });
            }
            for child in map.values() {
                if out.len() >= limit {
                    break;
                }
                collect_radio_panel_items(child, limit, seen, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                if out.len() >= limit {
                    break;
                }
                collect_radio_panel_items(child, limit, seen, out);
            }
        }
        _ => {}
    }
}

fn collect_search_cards(
    val: &Value,
    limit: usize,
    seen: &mut HashSet<String>,
    out: &mut Vec<YtmShelfCard>,
) {
    if out.len() >= limit {
        return;
    }
    match val {
        Value::Object(map) => {
            if let Some(card_shelf) = map.get("musicCardShelfRenderer")
                && let Some(bid) = card_shelf
                    .pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseId")
                    .or_else(|| card_shelf.pointer("/onTap/browseEndpoint/browseId"))
                    .and_then(Value::as_str)
                && !bid.is_empty()
                && seen.insert(bid.to_string())
                && let Some(title) = extract_runs_text(card_shelf.get("title"))
            {
                let subtitle = extract_runs_text(card_shelf.get("subtitle")).unwrap_or_default();
                let page_type = card_shelf
                    .pointer("/title/runs/0/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
                    .and_then(Value::as_str);
                let kind = classify_page_type(page_type, Some(bid));
                if kind != YtmCardKind::Song {
                    out.push(YtmShelfCard {
                        id: bid.to_string(),
                        title,
                        subtitle,
                        kind,
                        browse_id: Some(bid.to_string()),
                        video_id: None,
                        thumbnail_url: extract_best_thumbnail(card_shelf, None),
                    });
                }
            }
            if let Some(resp) = map.get("musicResponsiveListItemRenderer")
                && let Some(bid) = resp
                    .pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(Value::as_str)
                && !bid.is_empty()
                && seen.insert(bid.to_string())
            {
                let page_type = resp
                    .pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
                    .and_then(Value::as_str);
                let kind = classify_page_type(page_type, Some(bid));
                if kind != YtmCardKind::Song
                    && let Some(flex) = resp.get("flexColumns").and_then(Value::as_array)
                    && let Some(title) = flex.first().and_then(|c| {
                        extract_runs_text(
                            c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"),
                        )
                    })
                {
                    let subtitle = flex
                        .get(1)
                        .and_then(|c| {
                            extract_runs_text(
                                c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"),
                            )
                        })
                        .unwrap_or_default();
                    out.push(YtmShelfCard {
                        id: bid.to_string(),
                        title,
                        subtitle,
                        kind,
                        browse_id: Some(bid.to_string()),
                        video_id: None,
                        thumbnail_url: extract_best_thumbnail(resp, None),
                    });
                }
            }
            if let Some(two_row) = map.get("musicTwoRowItemRenderer")
                && let Some(card) = parse_two_row_card(two_row)
                && card.kind != YtmCardKind::Song
                && seen.insert(card.id.clone())
            {
                out.push(card);
            }
            for child in map.values() {
                if out.len() >= limit {
                    break;
                }
                collect_search_cards(child, limit, seen, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                if out.len() >= limit {
                    break;
                }
                collect_search_cards(child, limit, seen, out);
            }
        }
        _ => {}
    }
}

fn parse_ytm_subtitle(subtitle: &str, default_album: &str) -> (String, String, u64) {
    let parts: Vec<&str> = subtitle
        .split('•')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();

    let mut artist = String::new();
    let mut album = String::new();
    let mut duration_ms = 0u64;
    let mut is_video = false;
    let mut is_song = false;

    for part in parts {
        let lower = part.to_ascii_lowercase();
        if lower == "video" {
            is_video = true;
            continue;
        }
        if lower == "song" || lower == "single" {
            is_song = true;
            continue;
        }
        if lower == "ep" || lower == "e" || lower == "explicit" {
            continue;
        }
        if let Some(ms) = parse_clock_ms(part) {
            duration_ms = ms;
            continue;
        }
        if lower.ends_with("plays")
            || lower.ends_with("views")
            || lower.ends_with("views.")
            || lower.contains("monthly audience")
            || lower.contains("subscribers")
            || lower.contains("listeners")
        {
            continue;
        }
        let cleaned = part.trim_matches(|c: char| matches!(c, ' ' | ':' | '-' | '|'));
        if cleaned.is_empty() {
            continue;
        }
        if artist.is_empty() {
            artist = cleaned.to_string();
        } else if album.is_empty() {
            album = cleaned.to_string();
        }
    }

    if artist.is_empty() {
        artist = "YouTube Music".to_string();
    }
    if album.is_empty() {
        album = if default_album == "YouTube Music" {
            if is_video {
                "Music Video".to_string()
            } else if is_song {
                "Single".to_string()
            } else {
                default_album.to_string()
            }
        } else {
            default_album.to_string()
        };
    }
    (artist, album, duration_ms)
}

fn extract_runs_text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Some(runs) = v.get("runs").and_then(Value::as_array) {
        let joined: String = runs
            .iter()
            .filter_map(|r| r.get("text").and_then(Value::as_str))
            .collect();
        let t = joined.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

fn parse_clock_ms(text: &str) -> Option<u64> {
    let mut parts = text
        .trim()
        .split(':')
        .filter_map(|p| p.trim().parse::<u64>().ok());
    let a = parts.next()?;
    let b = parts.next()?;
    match (parts.next(), parts.next()) {
        (None, None) => Some(a.saturating_mul(60).saturating_add(b).saturating_mul(1_000)),
        (Some(c), None) => Some(
            a.saturating_mul(3_600)
                .saturating_add(b.saturating_mul(60))
                .saturating_add(c)
                .saturating_mul(1_000),
        ),
        _ => None,
    }
}

fn clean_ytm_playlist_id(raw: &str) -> Option<&str> {
    let stripped = raw
        .trim()
        .strip_prefix("ytm:pl:")
        .or_else(|| raw.trim().strip_prefix("yt:playlist:"))
        .unwrap_or(raw.trim());
    let no_vl = stripped.strip_prefix("VL").unwrap_or(stripped);
    if no_vl.is_empty()
        || no_vl.eq_ignore_ascii_case("LM")
        || no_vl.len() > 128
        || !no_vl
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(no_vl)
}

/// Create a private playlist directly on the signed-in YouTube Music account via InnerTube
/// `playlist/create`, returning Ferric's canonical synced ID (`ytm:pl:VL<id>`).
pub fn create_ytm_playlist(
    session: &YtmSession,
    title: &str,
    description: &str,
    video_ids: &[String],
) -> ProviderResult<String> {
    if !session.is_signed_in() {
        return Err(ProviderError::Http(
            "Sign in to YouTube Music to save playlists to your account".to_string(),
        ));
    }
    let clean_title = title.trim();
    if clean_title.is_empty() {
        return Err(ProviderError::Metadata(
            "Playlist title cannot be empty".to_string(),
        ));
    }
    let clean_vids: Vec<&str> = video_ids
        .iter()
        .map(String::as_str)
        .filter(|v| is_valid_video_id(v))
        .take(100)
        .collect();
    let mut body = json!({
        "context": web_remix_context(session),
        "title": clean_title,
        "description": description.trim(),
        "privacyStatus": "PRIVATE",
    });
    if !clean_vids.is_empty() {
        body["videoIds"] = json!(clean_vids);
    }
    let resp = post_ytm_json_fast("playlist/create", &body, session)?;
    let pid = resp
        .get("playlistId")
        .and_then(Value::as_str)
        .and_then(clean_ytm_playlist_id)
        .ok_or_else(|| {
            ProviderError::Metadata("YouTube Music did not return a playlistId".to_string())
        })?;
    Ok(format!("ytm:pl:VL{pid}"))
}

/// Append one or more video IDs to an existing playlist on the signed-in YouTube Music account.
pub fn add_videos_to_ytm_playlist(
    session: &YtmSession,
    playlist_id: &str,
    video_ids: &[String],
) -> ProviderResult<()> {
    if !session.is_signed_in() {
        return Err(ProviderError::Http(
            "YouTube Music sign-in required".to_string(),
        ));
    }
    let Some(pid) = clean_ytm_playlist_id(playlist_id) else {
        return Err(ProviderError::Metadata(
            "Invalid YouTube playlist ID".to_string(),
        ));
    };
    let actions: Vec<Value> = video_ids
        .iter()
        .map(String::as_str)
        .filter(|v| is_valid_video_id(v))
        .map(|vid| {
            json!({
                "action": "ACTION_ADD_VIDEO",
                "addedVideoId": vid,
            })
        })
        .collect();
    if actions.is_empty() {
        return Ok(());
    }
    for chunk in actions.chunks(50) {
        let body = json!({
            "context": web_remix_context(session),
            "playlistId": pid,
            "actions": chunk,
        });
        let _ = post_ytm_json_fast("browse/edit_playlist", &body, session)?;
    }
    Ok(())
}

/// Remove a video from an existing playlist on the signed-in YouTube Music account.
pub fn remove_video_from_ytm_playlist(
    session: &YtmSession,
    playlist_id: &str,
    video_id: &str,
) -> ProviderResult<()> {
    if !session.is_signed_in() || !is_valid_video_id(video_id) {
        return Ok(());
    }
    let Some(pid) = clean_ytm_playlist_id(playlist_id) else {
        return Ok(());
    };
    let body = json!({
        "context": web_remix_context(session),
        "playlistId": pid,
        "actions": [{
            "action": "ACTION_REMOVE_VIDEO_BY_VIDEO_ID",
            "removedVideoId": video_id,
        }],
    });
    let _ = post_ytm_json_fast("browse/edit_playlist", &body, session)?;
    Ok(())
}

/// Rename a playlist on the signed-in YouTube Music account.
pub fn rename_ytm_playlist(
    session: &YtmSession,
    playlist_id: &str,
    new_name: &str,
) -> ProviderResult<()> {
    if !session.is_signed_in() || new_name.trim().is_empty() {
        return Ok(());
    }
    let Some(pid) = clean_ytm_playlist_id(playlist_id) else {
        return Ok(());
    };
    let body = json!({
        "context": web_remix_context(session),
        "playlistId": pid,
        "actions": [{
            "action": "ACTION_SET_PLAYLIST_NAME",
            "playlistName": new_name.trim(),
        }],
    });
    let _ = post_ytm_json_fast("browse/edit_playlist", &body, session)?;
    Ok(())
}

/// Delete a playlist from the signed-in YouTube Music account.
pub fn delete_ytm_playlist(session: &YtmSession, playlist_id: &str) -> ProviderResult<()> {
    if !session.is_signed_in() {
        return Ok(());
    }
    let Some(pid) = clean_ytm_playlist_id(playlist_id) else {
        return Ok(());
    };
    let body = json!({
        "context": web_remix_context(session),
        "playlistId": pid,
    });
    let _ = post_ytm_json_fast("playlist/delete", &body, session)?;
    Ok(())
}

/// Like or unlike a track on the signed-in YouTube Music account.
pub fn set_ytm_track_liked(
    session: &YtmSession,
    video_id: &str,
    liked: bool,
) -> ProviderResult<()> {
    if !session.is_signed_in() || !is_valid_video_id(video_id) {
        return Ok(());
    }
    let endpoint = if liked {
        "like/like"
    } else {
        "like/removelike"
    };
    let body = json!({
        "context": web_remix_context(session),
        "target": { "videoId": video_id },
    });
    let _ = post_ytm_json_fast(endpoint, &body, session)?;
    Ok(())
}

fn is_valid_video_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_rfc3174_nist_vector() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            clean_ytm_playlist_id("ytm:pl:VLPLabc_123"),
            Some("PLabc_123")
        );
        assert_eq!(
            clean_ytm_playlist_id("yt:playlist:PLxyz-9"),
            Some("PLxyz-9")
        );
        assert_eq!(clean_ytm_playlist_id("ytm:pl:VLLM"), None);
        assert_eq!(clean_ytm_playlist_id("ytm:pl:../bad"), None);
    }

    #[test]
    fn builds_sapisidhash_and_extracts_cookie() {
        let cookie = "PREF=tz=Asia.Jakarta; SAPISID=abc123/xyz; __Secure-3PAPISID=pref999/key";
        assert_eq!(
            extract_sapisid_from_cookie(cookie).as_deref(),
            Some("pref999/key")
        );
        let auth = build_sapisidhash_header("abc123", YTM_ORIGIN, 1_700_000_000);
        assert!(auth.starts_with("SAPISIDHASH 1700000000_"));
        assert_eq!(auth.len(), "SAPISIDHASH 1700000000_".len() + 40);
    }

    #[test]
    fn personalized_home_ranks_consensus_popular_tracks_and_filters_mismatched_locale() {
        let mk = |vid: &str, title: &str, artist: &str| Track {
            id: format!("yt:video:{vid}"),
            title: title.to_string(),
            artist: artist.to_string(),
            album: "Album".to_string(),
            duration_ms: 210_000,
            path: None,
            source: TrackSource::YouTubeMusic,
            codec: "OPUS".to_string(),
            sample_rate: 48_000,
            bit_depth: 16,
            bitrate_kbps: 140,
            isrc: None,
            spotify_id: None,
            youtube_video_id: Some(vid.to_string()),
            thumbnail_url: None,
            replay_gain_db: 0,
            liked: false,
        };
        let taste = YtmHomeTasteProfile {
            top_tracks: vec![
                mk("seed1aaaaaa", "WILDFLOWER", "Billie Eilish"),
                mk("seed2bbbbbb", "Runaway", "AURORA"),
            ],
            top_artists: vec!["Billie Eilish".to_string(), "AURORA".to_string()],
            recent_searches: Vec::new(),
            ..Default::default()
        };
        let locale = dominant_taste_locale(&taste.top_tracks);
        assert_eq!(locale, MusicLocale::GlobalEn);
        let pool1 = vec![
            mk("shared11111", "Sweater Weather", "The Neighbourhood"),
            mk("idspam22222", "Aku Dirimu Dirinya", "Della Monica"),
            mk("solo3333333", "Cinnamon Girl", "Lana Del Rey"),
        ];
        let pool2 = vec![
            mk("solo4444444", "happier", "Olivia Rodrigo"),
            mk("shared11111", "Sweater Weather", "The Neighbourhood"),
        ];
        let ranked = rank_personalized_home_candidates(&[pool1, pool2], &[], &taste, locale, 2);
        assert_eq!(ranked[0].title, "Sweater Weather");
        assert!(!ranked.iter().any(|t| t.title.contains("Aku Dirimu")));
    }

    #[test]
    fn parses_ytm_home_and_search_items_cleanly() {
        let sample = json!({
            "contents": {
                "musicResponsiveListItemRenderer": {
                    "playlistItemData": { "videoId": "E7kHvjvU6JY" },
                    "flexColumns": [
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{"text": "Hati-Hati di Jalan"}] } } },
                        { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{"text": "Tulus"}, {"text": " • "}, {"text": "Manusia"}, {"text": " • "}, {"text": "4:02"}] } } }
                    ]
                }
            }
        });
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_music_items(&sample, "Discover", 10, &mut seen, &mut tracks);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Hati-Hati di Jalan");
        assert_eq!(tracks[0].artist, "Tulus");
        assert_eq!(tracks[0].album, "Manusia");
        assert_eq!(tracks[0].duration_ms, 242_000);
        assert_eq!(tracks[0].youtube_video_id.as_deref(), Some("E7kHvjvU6JY"));
        assert_eq!(
            tracks[0].thumbnail_url.as_deref(),
            Some("https://i.ytimg.com/vi/E7kHvjvU6JY/mqdefault.jpg")
        );
    }

    #[test]
    fn normalizes_google_thumbnails_to_square_jpeg_and_parses_radio_panel() {
        let norm = normalize_ytm_thumbnail_url(
            "https://lh3.googleusercontent.com/abc123XYZ=w60-h60-l90-rj",
        );
        assert_eq!(
            norm,
            "https://lh3.googleusercontent.com/abc123XYZ=w300-h300-l90-rj"
        );
        assert_eq!(
            normalize_ytm_thumbnail_url("https://i.ytimg.com/vi/abc12345678/maxresdefault.jpg"),
            "https://i.ytimg.com/vi/abc12345678/mqdefault.jpg"
        );

        let radio_sample = json!({
            "contents": {
                "playlistPanelVideoRenderer": {
                    "videoId": "abc12345678",
                    "title": { "runs": [{"text": "Monokrom"}] },
                    "longBylineText": { "runs": [{"text": "Tulus"}, {"text": " • "}, {"text": "Monokrom"}] },
                    "lengthText": { "runs": [{"text": "3:35"}] },
                    "thumbnail": { "thumbnails": [{ "url": "https://lh3.googleusercontent.com/cover99=w120-h120-l90-rj", "width": 120 }] }
                }
            }
        });
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_radio_panel_items(&radio_sample, 10, &mut seen, &mut tracks);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Monokrom");
        assert_eq!(tracks[0].artist, "Tulus");
        assert_eq!(tracks[0].duration_ms, 215_000);
        assert_eq!(
            tracks[0].thumbnail_url.as_deref(),
            Some("https://lh3.googleusercontent.com/cover99=w300-h300-l90-rj")
        );
    }

    #[test]
    fn filters_podcast_episodes_and_labels_music_videos() {
        let sample = json!({
            "items": [
                {
                    "musicResponsiveListItemRenderer": {
                        "playlistItemData": { "videoId": "mX24T1uzsKI" },
                        "flexColumns": [
                            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{"text": "Podcast Episode Title"}] } } },
                            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{"text": "Episode • Nov 2 • Business Podcast"}] } } }
                        ]
                    }
                },
                {
                    "musicResponsiveListItemRenderer": {
                        "playlistItemData": { "videoId": "yWHrYNP6j4k" },
                        "flexColumns": [
                            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{"text": "STAY"}] } } },
                            { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{"text": "Video • Justin Bieber & The Kid LAROI • 283M views"}] } } }
                        ]
                    }
                }
            ]
        });
        let mut tracks = Vec::new();
        let mut seen = HashSet::new();
        collect_music_items(&sample, "YouTube Music", 10, &mut seen, &mut tracks);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "STAY");
        assert_eq!(tracks[0].artist, "Justin Bieber & The Kid LAROI");
        assert_eq!(tracks[0].album, "Music Video");
    }

    #[test]
    fn radio_locale_and_coherence_filter_keeps_english_queues_clean() {
        assert_eq!(
            detect_track_locale("STAY", "The Kid LAROI & Justin Bieber", "F*CK LOVE 3"),
            MusicLocale::GlobalEn
        );
        assert_eq!(
            detect_track_locale("Hati-Hati di Jalan", "Tulus", "Manusia"),
            MusicLocale::Indonesian
        );
        assert_eq!(
            detect_track_locale("Runaway", "AURORA", "All My Demons Greeting Me as a Friend"),
            MusicLocale::GlobalEn
        );

        let make = |id: &str, title: &str, artist: &str, album: &str| Track {
            id: id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_ms: 200_000,
            path: None,
            source: TrackSource::YouTubeMusic,
            codec: "YT AAC".to_string(),
            sample_rate: 44_100,
            bit_depth: 16,
            bitrate_kbps: 128,
            isrc: None,
            spotify_id: None,
            youtube_video_id: Some(id.to_string()),
            thumbnail_url: None,
            replay_gain_db: 0,
            liked: false,
        };
        let mixed = vec![
            make("s1", "Diva", "The Kid LAROI", "F*CK LOVE"),
            make("s2", "WRONG", "The Kid LAROI", "F*CK LOVE"),
            make("s3", "SPEED DEMON", "Justin Bieber", "Justice"),
            make("s4", "Stay", "Mini Pop Kids", "Mini Pop Kids 19"),
            make("s5", "Stay (Remix)", "Tanner Patrick", "Single"),
            make("s6", "STAY +4Key No Vocal", "STAY +4Key", "Karaoke"),
            make(
                "s7",
                "Stay (Extended)",
                "The Kid LAROI & Justin Bieber",
                "Single",
            ),
            make("a", "Blinding Lights", "The Weeknd", "After Hours"),
            make("a2", "Starboy", "The Weeknd", "Starboy"),
            make("b", "Hati-Hati di Jalan", "Tulus", "Manusia"),
            make("c", "STAY (Sped Up)", "TikTok Remix", "Single"),
            make("d", "As It Was", "Harry Styles", "Harry's House"),
        ];
        let out = filter_coherent_radio_tracks(
            MusicLocale::GlobalEn,
            "STAY",
            "The Kid LAROI & Justin Bieber",
            mixed,
            10,
        );
        // Only 1 song per artist, 0 same-song covers/pitch-shifts, and non-seed artist at #1
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].title, "Blinding Lights");
        assert_eq!(out[1].title, "As It Was");
        assert_eq!(out[2].title, "Diva");
    }

    #[test]
    fn music_video_candidate_scoring_picks_official_mv_over_unrelated_or_ugc() {
        let unrelated = MusicVideoCandidate {
            video_id: "U68MJz9DrI4".to_string(),
            title: "Wake Up in the Sky".to_string(),
            subtitle: "Gucci Mane, Bruno Mars & Kodak Black • 697M views • 3:27".to_string(),
            is_omv: true,
            duration_ms: 207_000,
        };
        let official_mv = MusicVideoCandidate {
            video_id: "PMivT7MJ41M".to_string(),
            title: "That's What I Like".to_string(),
            subtitle: "Bruno Mars • 2.5B views • 3:31".to_string(),
            is_omv: true,
            duration_ms: 211_000,
        };
        let slowed_ugc = MusicVideoCandidate {
            video_id: "slowed12345".to_string(),
            title: "Bruno Mars - That's What I Like (Slowed + Reverb Lyrics)".to_string(),
            subtitle: "Random Channel • 40K views • 4:10".to_string(),
            is_omv: false,
            duration_ms: 250_000,
        };

        let s_unrel =
            score_music_video_candidate(&unrelated, "Bruno Mars", "That's What I Like", 206_000);
        let s_omv =
            score_music_video_candidate(&official_mv, "Bruno Mars", "That's What I Like", 206_000);
        let s_ugc =
            score_music_video_candidate(&slowed_ugc, "Bruno Mars", "That's What I Like", 206_000);
        assert!(
            s_unrel < 0,
            "unrelated song must be rejected (got {s_unrel})"
        );
        assert!(s_omv > 200, "official OMV must score high (got {s_omv})");
        assert!(
            s_omv > s_ugc + 150,
            "official OMV must crush slowed/lyrics UGC ({s_omv} vs {s_ugc})"
        );
    }

    #[test]
    fn algorithmic_stations_preserve_multilocale_diversity_and_zero_consecutive_artists() {
        let make =
            |id: &str, title: &str, artist: &str, album: &str, thumb: &str, liked: bool| Track {
                id: id.to_string(),
                title: title.to_string(),
                artist: artist.to_string(),
                album: album.to_string(),
                duration_ms: 210_000,
                path: None,
                source: TrackSource::YouTubeMusic,
                codec: "YT AAC".to_string(),
                sample_rate: 44_100,
                bit_depth: 16,
                bitrate_kbps: 128,
                isrc: None,
                spotify_id: None,
                youtube_video_id: Some(format!("{id:0<11}")),
                thumbnail_url: Some(thumb.to_string()),
                replay_gain_db: 0,
                liked,
            };
        let lib = vec![
            make(
                "t1",
                "Kill Bill",
                "SZA",
                "SOS",
                "https://i.scdn.co/image/sza",
                true,
            ),
            make(
                "t2",
                "Blinding Lights",
                "The Weeknd",
                "After Hours",
                "https://i.scdn.co/image/wknd",
                true,
            ),
            make(
                "t3",
                "Golden Hour",
                "JVKE",
                "this is what ____ feels like",
                "https://i.scdn.co/image/jvke",
                true,
            ),
            make(
                "t4",
                "Akad",
                "Payung Teduh",
                "Ruang Tunggu",
                "https://i.scdn.co/image/payung",
                true,
            ),
            make(
                "t5",
                "Hati-Hati di Jalan",
                "Tulus",
                "Manusia",
                "https://i.scdn.co/image/tulus",
                true,
            ),
            make(
                "t6",
                "Gurenge",
                "LiSA",
                "LEO-NiNE",
                "https://i.scdn.co/image/lisa",
                true,
            ),
            make(
                "t7",
                "From The Start",
                "Laufey",
                "Bewitched",
                "https://i.scdn.co/image/laufey",
                false,
            ),
        ];
        let taste = YtmHomeTasteProfile {
            top_tracks: lib[..4].to_vec(),
            top_artists: vec!["SZA".into(), "The Weeknd".into(), "Payung Teduh".into()],
            recent_searches: Vec::new(),
            ..Default::default()
        };
        let stations = build_algorithmic_recommended_stations(&taste, &lib, &[], 6);
        assert_eq!(stations.len(), 6);
        // Must surface both English and Indonesian stations in the first 6 cards
        assert!(stations.iter().any(|s| s.seed_artist == "Payung Teduh"));
        assert!(stations.iter().any(|s| s.seed_artist == "SZA"));
        // Every station has "With ..." subtitle and non-repeating adjacent pastel colors
        for win in stations.windows(2) {
            assert_ne!(win[0].palette_idx, win[1].palette_idx);
        }
        assert!(stations[0].subtitle.starts_with("With "));

        // Interleaver must prevent back-to-back same artist
        let clumped = vec![
            make(
                "a1",
                "Song 1",
                "JVKE",
                "A",
                "https://i.scdn.co/image/1",
                false,
            ),
            make(
                "a2",
                "Song 2",
                "JVKE",
                "A",
                "https://i.scdn.co/image/2",
                false,
            ),
            make(
                "a3",
                "Song 3",
                "JVKE",
                "A",
                "https://i.scdn.co/image/3",
                false,
            ),
            make(
                "b1",
                "Song 4",
                "d4vd",
                "B",
                "https://i.scdn.co/image/4",
                false,
            ),
            make(
                "c1",
                "Song 5",
                "MAX",
                "C",
                "https://i.scdn.co/image/5",
                false,
            ),
        ];
        let seq = interleave_station_tracks(clumped, 5);
        assert_eq!(seq.len(), 5);
        for win in seq.windows(2) {
            assert_ne!(
                primary_artist_key(&win[0].artist),
                primary_artist_key(&win[1].artist)
            );
        }

        // Rotation epoch must produce genuinely different leading station order on Refresh
        let rotated = build_algorithmic_recommended_stations_rotated(&taste, &lib, &[], 6, 1);
        assert_eq!(rotated.len(), 6);
        assert_ne!(stations[0].seed_artist, rotated[0].seed_artist);
    }

    #[test]
    fn refresh_algorithm_handles_cjk_locale_exclusions_multilocale_indonesian_and_noisy_cast() {
        // 1. Kanji-only artist names must be classified as Japanese
        assert_eq!(
            detect_track_locale("IRIS OUT", "米津玄師", "IRIS OUT"),
            MusicLocale::Japanese
        );

        let make = |id: &str, title: &str, artist: &str, album: &str| Track {
            id: id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_ms: 210_000,
            path: None,
            source: TrackSource::YouTubeMusic,
            codec: "YT OPUS".to_string(),
            sample_rate: 48_000,
            bit_depth: 16,
            bitrate_kbps: 148,
            isrc: None,
            spotify_id: None,
            youtube_video_id: Some(format!("{id:0<11}")),
            thumbnail_url: Some(format!("https://i.scdn.co/image/{id}")),
            replay_gain_db: 0,
            liked: false,
        };

        // 2. Bilingual English + Indonesian taste must retain Indonesian candidates while
        // demoting excluded_track_ids strictly behind unseen tracks on fresh_seed Refresh.
        let taste = YtmHomeTasteProfile {
            top_tracks: vec![
                make("e1", "Sweater Weather", "The Neighbourhood", "I Love You."),
                make("e2", "End of Beginning", "Djo", "DECIDE"),
                make("e3", "Notion", "The Rare Occasions", "Notion"),
                make("id1", "Akad", "Payung Teduh", "Ruang Tunggu"),
                make("id2", "Hati-Hati di Jalan", "Tulus", "Manusia"),
            ],
            top_artists: vec![
                "The Neighbourhood".into(),
                "Djo".into(),
                "Payung Teduh".into(),
                "Tulus".into(),
            ],
            excluded_track_ids: ["old1", "old2"].into_iter().map(str::to_string).collect(),
            excluded_artists: ["sza", "the weeknd"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            ..Default::default()
        };
        assert!(taste_includes_locale(&taste, MusicLocale::Indonesian));

        let pools = vec![
            vec![
                make("old1", "Old Song One", "The Neighbourhood", "I Love You."),
                make("old2", "Old Song Two", "Djo", "DECIDE"),
                make(
                    "new_id",
                    "Untuk Perempuan Yang Sedang Di Pelukan",
                    "Payung Teduh",
                    "Dunia Batas",
                ),
                make(
                    "new_en",
                    "505",
                    "Arctic Monkeys",
                    "Favourite Worst Nightmare",
                ),
            ],
            vec![
                make("old1", "Old Song One", "The Neighbourhood", "I Love You."),
                make(
                    "new_en2",
                    "Apocalypse",
                    "Cigarettes After Sex",
                    "Cigarettes After Sex",
                ),
            ],
        ];
        let ranked = rank_personalized_home_candidates_ex(
            &pools,
            &[],
            &taste,
            MusicLocale::GlobalEn,
            1,
            true,
        );
        assert!(
            ranked.iter().any(|t| t.id == "new_id"),
            "Indonesian taste tracks must not be dropped when user has Indonesian library tracks"
        );
        let first_old_pos = ranked
            .iter()
            .position(|t| t.id == "old1" || t.id == "old2")
            .unwrap_or(ranked.len());
        let last_new_pos = ranked
            .iter()
            .rposition(|t| t.id.starts_with("new_"))
            .unwrap_or(0);
        assert!(
            last_new_pos < first_old_pos,
            "All unseen candidates must rank ahead of excluded_track_ids on Refresh (last_new={last_new_pos}, first_old={first_old_pos})"
        );

        // 3. Stations refresh must exclude recently shown artists and filter noisy anime cast strings
        let lib = vec![
            make("s1", "Kill Bill", "SZA", "SOS"),
            make("s2", "Blinding Lights", "The Weeknd", "After Hours"),
            make("s3", "505", "Arctic Monkeys", "FWN"),
            make("s4", "Golden Hour", "JVKE", "Single"),
            make("s5", "Akad", "Payung Teduh", "Ruang Tunggu"),
            make("s6", "Monokrom", "Tulus", "Monokrom"),
            make("s7", "Apocalypse", "Cigarettes After Sex", "CAS"),
            make("s8", "Runaway", "AURORA", "Single"),
            make(
                "noisy",
                "Seishun Satsubatsuron",
                "3年E組うた担(渚&茅野&業&磯貝&前原)",
                "OST",
            ),
        ];
        let stations = build_algorithmic_recommended_stations_rotated(&taste, &lib, &[], 5, 2);
        assert!(!stations.is_empty());
        assert!(
            !stations.iter().any(|s| s.seed_artist.contains("3年E組")),
            "Noisy low-score anime cast label must be filtered out of Recommended Stations"
        );
        for st in stations.iter().take(4) {
            let norm = primary_artist_key(&st.seed_artist).to_ascii_lowercase();
            assert!(
                !taste.excluded_artists.contains(&norm),
                "Recently shown station artist '{norm}' should be excluded when fresh artists exist"
            );
        }
    }
}
