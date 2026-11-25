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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const YTM_ORIGIN: &str = "https://music.youtube.com";
const YT_ORIGIN: &str = "https://www.youtube.com";
const WEB_REMIX_VERSION: &str = "1.20260707.12.00";
const WEB_REMIX_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";
const VISIONOS_VERSION: &str = "1.02";
const VISIONOS_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
const STREAM_CHUNK_BYTES: u64 = 512 * 1024;
const STREAM_CONCURRENCY: usize = 3;

#[derive(Debug, Clone, Default)]
pub struct YtmSession {
    pub visitor_data: String,
    pub cookie: String,
    pub sapisid: String,
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

    if full.contains("lh3.googleusercontent.com")
        || full.contains("yt3.googleusercontent.com")
        || full.contains("yt3.ggpht.com")
    {
        let no_query = full.split('?').next().unwrap_or(&full);
        if let Some(slash_idx) = no_query.rfind('/')
            && let Some(eq_rel) = no_query[slash_idx..].find('=')
        {
            let base = &no_query[..slash_idx + eq_rel];
            return format!("{base}=w180-h180-l85-rj");
        }
        return format!("{no_query}=w180-h180-l85-rj");
    }

    if full.contains("i.ytimg.com") || full.contains("i9.ytimg.com") {
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

/// Download and disk-cache album/playlist/artist artwork bytes (JPEG or PNG) with 0 ms cache hits.
pub fn download_artwork_bytes(url: &str, cache_dir: &Path) -> ProviderResult<Vec<u8>> {
    let normalized = normalize_ytm_thumbnail_url(url);
    if normalized.is_empty() {
        return Err(ProviderError::Metadata("Empty artwork URL".to_string()));
    }
    fs::create_dir_all(cache_dir)?;
    let key = sha1_hex(normalized.as_bytes());
    let dest = cache_dir.join(format!("{key}.img"));
    if let Ok(existing) = fs::read(&dest)
        && is_supported_image_bytes(&existing)
    {
        return Ok(existing);
    }

    let agent = build_http_agent(12);
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

    let part = cache_dir.join(format!("{key}.part"));
    if fs::write(&part, &bytes).is_ok() {
        let _ = fs::rename(&part, &dest);
    }
    Ok(bytes)
}

fn build_http_agent_with_family(timeout_secs: u64, ipv4_only: bool) -> ureq::Agent {
    let body_secs = timeout_secs.max(14);
    let mut cfg = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(body_secs.min(18))))
        .timeout_per_read(Some(Duration::from_secs(12)))
        .timeout_recv_body(Some(Duration::from_secs(body_secs)))
        .timeout_global(Some(Duration::from_secs(body_secs + 10)))
        .http_status_as_error(false);
    if ipv4_only {
        cfg = cfg.ip_family(ureq::config::IpFamily::Ipv4Only);
    }
    ureq::Agent::new_with_config(cfg.build())
}

fn build_http_agent(timeout_secs: u64) -> ureq::Agent {
    build_http_agent_with_family(timeout_secs, false)
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
        let req_timeout = timeout_secs + (attempt as u64) * 6;
        let agent = build_http_agent_with_family(req_timeout, ipv4_only);
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
    let combined = format!("{title} {artist} {album}");
    let mut has_jp = false;
    let mut has_kr = false;
    for ch in combined.chars() {
        let cp = ch as u32;
        if (0x3040..=0x30FF).contains(&cp) {
            has_jp = true;
        } else if (0xAC00..=0xD7AF).contains(&cp) || (0x1100..=0x11FF).contains(&cp) {
            has_kr = true;
        }
    }
    if has_kr {
        return MusicLocale::Korean;
    }
    if has_jp {
        return MusicLocale::Japanese;
    }

    // Check title + album tokens (and genre hints) for Indonesian language signals.
    // Deliberately avoids classifying English songs by Indonesian-born international artists as ID.
    const ID_WORDS: &[&str] = &[
        "aku",
        "kamu",
        "kau",
        "cinta",
        "rindu",
        "hati",
        "bukan",
        "yang",
        "untuk",
        "untukmu",
        "dengan",
        "dalam",
        "malam",
        "hujan",
        "senja",
        "kisah",
        "patah",
        "selalu",
        "berdua",
        "pergi",
        "pergilah",
        "tidak",
        "jangan",
        "bila",
        "saat",
        "sampai",
        "telah",
        "lagi",
        "saja",
        "cuma",
        "terlalu",
        "dirimu",
        "diriku",
        "tentang",
        "seperti",
        "bersama",
        "bahagia",
        "terindah",
        "sial",
        "tak",
        "di",
        "ke",
        "dari",
        "dan",
        "atau",
        "jalan",
        "koplo",
        "dangdut",
        "galau",
        "melayu",
        "lagu",
        "indonesia",
        "nanti",
        "kita",
        "hari",
        "ini",
        "itu",
        "semua",
        "kembali",
        "menunggu",
        "mengapa",
        "kenapa",
        "sendiri",
        "sepi",
        "luka",
        "rasa",
        "janji",
        "mimpi",
        "jiwa",
        "tuhan",
        "indah",
        "manusia",
        "komang",
        "monokrom",
        "sepatu",
    ];
    let title_alb = format!("{title} {album}").to_ascii_lowercase();
    let mut id_hits = 0usize;
    for tok in title_alb.split(|c: char| !c.is_ascii_alphabetic()) {
        if tok.len() >= 2 && ID_WORDS.contains(&tok) {
            id_hits += 1;
            if matches!(
                tok,
                "koplo" | "dangdut" | "galau" | "melayu" | "indonesia" | "lagu"
            ) {
                return MusicLocale::Indonesian;
            }
        }
    }
    if id_hits >= 1 {
        let en_hits = title_alb
            .split(|c: char| !c.is_ascii_alphabetic())
            .filter(|w| {
                matches!(
                    *w,
                    "the"
                        | "you"
                        | "your"
                        | "love"
                        | "with"
                        | "from"
                        | "that"
                        | "this"
                        | "have"
                        | "don"
                        | "can"
                        | "stay"
                        | "night"
                        | "heart"
                        | "world"
                        | "feat"
                        | "remix"
                )
            })
            .count();
        if id_hits > en_hits {
            return MusicLocale::Indonesian;
        }
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
    /// User's most-played primary artists (highest affinity first).
    pub top_artists: Vec<String>,
    /// Recent user search queries (secondary taste signal).
    pub recent_searches: Vec<String>,
}

fn dominant_taste_locale(tracks: &[Track]) -> MusicLocale {
    if tracks.is_empty() {
        return MusicLocale::GlobalEn;
    }
    let mut en_w = 0usize;
    let mut id_w = 0usize;
    let mut jp_w = 0usize;
    let mut kr_w = 0usize;
    for (idx, t) in tracks.iter().take(16).enumerate() {
        let w = 16usize.saturating_sub(idx).max(1);
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
) -> bool {
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
/// while enforcing artist diversity and locale coherence.
pub fn rank_personalized_home_candidates(
    radio_pools: &[Vec<Track>],
    artist_popular: &[Track],
    taste: &YtmHomeTasteProfile,
    taste_locale: MusicLocale,
    max_per_artist: usize,
) -> Vec<Track> {
    let top_artist_norms: HashSet<String> = taste
        .top_artists
        .iter()
        .map(|a| primary_artist_norm(a))
        .filter(|a| !a.is_empty())
        .collect();
    let recent_ids: HashSet<String> = taste.top_tracks.iter().map(|t| t.id.clone()).collect();

    struct ScoredTrack {
        track: Track,
        score: i32,
        pool_hits: usize,
    }
    let mut by_key: HashMap<String, ScoredTrack> = HashMap::new();

    for pool in radio_pools {
        let mut seen_in_pool = HashSet::new();
        for (rank, t) in pool.iter().enumerate() {
            if is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms) {
                continue;
            }
            if taste_locale == MusicLocale::GlobalEn
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
                14
            } else {
                0
            };
            let freshness_bonus = if recent_ids.contains(&t.id) { 0 } else { 10 };
            let entry = by_key.entry(key).or_insert_with(|| ScoredTrack {
                track: t.clone(),
                score: 0,
                pool_hits: 0,
            });
            entry.pool_hits += 1;
            // Multi-seed collaborative consensus boost: tracks appearing across 2+ seed radios
            // are strongly preferred by listeners who share this user's exact combined taste.
            let consensus_bonus = if entry.pool_hits >= 2 { 44 } else { 0 };
            entry.score += pos_score + affinity_bonus + freshness_bonus + consensus_bonus;
        }
    }

    for (rank, t) in artist_popular.iter().enumerate() {
        if is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms) {
            continue;
        }
        let key = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
        let pos_score = 22_i32.saturating_sub((rank as i32) * 2).max(4);
        let entry = by_key.entry(key).or_insert_with(|| ScoredTrack {
            track: t.clone(),
            score: 0,
            pool_hits: 0,
        });
        entry.score += pos_score;
    }

    let mut ranked: Vec<ScoredTrack> = by_key.into_values().collect();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
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

/// Fetch rich YouTube Music Home feed:
/// - When user has listening history, runs a multi-seed collaborative co-listening & artist popularity
///   recommendation fan-out in parallel (~1 RTT) so Home mirrors the user's real listening taste
///   and globally popular songs enjoyed by similar listeners (never drowned in regional GeoIP charts).
/// - On cold start (empty history) or when browsing a specific Mood Chip, queries locale-matched
///   `FEmusic_home` + `FEmusic_explore` + `FEmusic_charts`.
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
    let has_taste = !taste.top_tracks.is_empty();
    let taste_locale = dominant_taste_locale(&taste.top_tracks);

    let mut body = json!({
        "browseId": "FEmusic_home",
        "context": web_remix_context_for_locale(&effective_session, taste_locale),
    });
    if let Some(params) = mood_params.map(str::trim).filter(|p| !p.is_empty()) {
        body["params"] = json!(params);
    }

    // Pick up to 3 distinct-artist seed tracks from user's top-played / liked / recent tracks
    let mut seed_tracks: Vec<Track> = Vec::new();
    if is_default_home && has_taste {
        let mut used_artists: HashSet<String> = HashSet::new();
        let candidates: Vec<Track> = taste
            .top_tracks
            .iter()
            .filter(|t| t.youtube_video_id.as_deref().is_some_and(is_valid_video_id))
            .cloned()
            .collect();
        let start_offset = if fresh_seed && candidates.len() > 3 {
            fastrand::usize(..candidates.len().min(6))
        } else {
            0
        };
        for pass in 0..2 {
            for i in 0..candidates.len() {
                if seed_tracks.len() >= 3 {
                    break;
                }
                let t = &candidates[(i + start_offset) % candidates.len()];
                let an = primary_artist_norm(&t.artist);
                if pass == 0 && !an.is_empty() && !used_artists.insert(an) {
                    continue;
                }
                if !seed_tracks.iter().any(|s| s.id == t.id) {
                    seed_tracks.push(t.clone());
                }
            }
        }
    }

    let mut top_artists: Vec<String> = taste
        .top_artists
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    for t in &taste.top_tracks {
        let a = t
            .artist
            .split(['&', ',', '•', '·'])
            .next()
            .unwrap_or(&t.artist)
            .trim()
            .to_string();
        if !a.is_empty()
            && !a.eq_ignore_ascii_case("unknown artist")
            && !top_artists.iter().any(|x| x.eq_ignore_ascii_case(&a))
        {
            top_artists.push(a);
        }
        if top_artists.len() >= 4 {
            break;
        }
    }
    let artist_1 = top_artists.first().cloned();
    let artist_2 = top_artists.get(1).cloned();

    // Parallel fan-out:
    // - Personalized Home (has listening history): FEmusic_home (for Mood Chips) + 3 collaborative
    //   seed radios + 2 top-artist popular playlists/albums searches (~1 RTT total).
    // - Cold start (no history): FEmusic_home + FEmusic_explore + FEmusic_charts.
    let (home_res, explore_res, charts_res, radio_pools, artist_search_1, artist_search_2) =
        if is_default_home && !seed_tracks.is_empty() {
            let s0 = seed_tracks.first().cloned();
            let s1 = seed_tracks.get(1).cloned();
            let s2 = seed_tracks.get(2).cloned();
            let a1_q = artist_1.as_ref().map(|a| format!("{a} popular hits"));
            let a2_q = artist_2.as_ref().map(|a| format!("{a} mix playlist"));
            std::thread::scope(|s| {
                let h_home = s.spawn(|| post_ytm_json_fast("browse", &body, &effective_session));
                let h_r0 = s.spawn(|| {
                    s0.as_ref().and_then(|t| {
                        let vid = t.youtube_video_id.as_deref()?;
                        fetch_radio_up_next_for_track(
                            &effective_session,
                            vid,
                            &t.title,
                            &t.artist,
                            &t.album,
                            18,
                            fresh_seed,
                        )
                        .ok()
                    })
                });
                let h_r1 = s.spawn(|| {
                    s1.as_ref().and_then(|t| {
                        let vid = t.youtube_video_id.as_deref()?;
                        fetch_radio_up_next_for_track(
                            &effective_session,
                            vid,
                            &t.title,
                            &t.artist,
                            &t.album,
                            18,
                            fresh_seed,
                        )
                        .ok()
                    })
                });
                let h_r2 = s.spawn(|| {
                    s2.as_ref().and_then(|t| {
                        let vid = t.youtube_video_id.as_deref()?;
                        fetch_radio_up_next_for_track(
                            &effective_session,
                            vid,
                            &t.title,
                            &t.artist,
                            &t.album,
                            16,
                            fresh_seed,
                        )
                        .ok()
                    })
                });
                let h_a1 = s.spawn(|| {
                    a1_q.as_deref()
                        .and_then(|q| fetch_rich_search(&effective_session, q, 14).ok())
                });
                let h_a2 = s.spawn(|| {
                    a2_q.as_deref()
                        .and_then(|q| fetch_rich_search(&effective_session, q, 12).ok())
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
                    None,
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
                    let sup_body = json!({
                        "browseId": "FEmusic_explore",
                        "context": web_remix_context_for_locale(&effective_session, taste_locale),
                    });
                    post_ytm_json_fast("browse", &sup_body, &effective_session)
                });
                let h_charts = s.spawn(|| {
                    let sup_body = json!({
                        "browseId": "FEmusic_charts",
                        "context": web_remix_context_for_locale(&effective_session, taste_locale),
                    });
                    post_ytm_json_fast("browse", &sup_body, &effective_session)
                });
                (
                    h_home.join().unwrap_or_else(|_| {
                        Err(ProviderError::Http(
                            "YouTube Music Home thread failed".to_string(),
                        ))
                    }),
                    h_explore.join().ok().and_then(Result::ok),
                    h_charts.join().ok().and_then(Result::ok),
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
            if explore_res.is_none() && charts_res.is_none() && radio_pools.is_empty() {
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

    let mut next_sess = effective_session;
    next_sess.visitor_data = visitor_data.clone();

    // Cold-start or Mood-chip browse: follow up to 2 fast continuations + Explore/Charts
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
                if !raw_home_shelves
                    .iter()
                    .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
                {
                    raw_home_shelves.push(shelf);
                }
            }
        }
    }

    let mut quick_picks: Vec<Track> = Vec::new();
    let mut shelves: Vec<YtmHomeShelf> = Vec::new();

    if is_default_home && has_taste && !radio_pools.is_empty() {
        let mut artist_popular_tracks: Vec<Track> = Vec::new();
        let mut taste_cards: Vec<YtmShelfCard> = Vec::new();
        let mut seen_card_ids: HashSet<String> = HashSet::new();

        for search_opt in [artist_search_1.as_ref(), artist_search_2.as_ref()]
            .into_iter()
            .flatten()
        {
            for t in search_opt.tracks.iter().take(10) {
                if !artist_popular_tracks.iter().any(|x| x.id == t.id) {
                    artist_popular_tracks.push(t.clone());
                }
            }
            for c in &search_opt.cards {
                if matches!(
                    c.kind,
                    YtmCardKind::Playlist | YtmCardKind::Album | YtmCardKind::Artist
                ) && seen_card_ids.insert(c.id.clone())
                {
                    taste_cards.push(c.clone());
                }
            }
        }

        let ranked_discovery = rank_personalized_home_candidates(
            &radio_pools,
            &artist_popular_tracks,
            taste,
            taste_locale,
            2,
        );

        // Quick Picks: blend 30% user's top-played favorites + 70% top collaborative crowd-favorites
        // matching the user's exact listening taste (zero regional GeoIP spam).
        let mut qp_seen: HashSet<String> = HashSet::new();
        let mut fav_iter = taste
            .top_tracks
            .iter()
            .filter(|t| t.youtube_video_id.is_some())
            .take(8)
            .cloned()
            .peekable();
        let mut disc_iter = ranked_discovery.iter().cloned().peekable();
        let mut slot = 0usize;
        while quick_picks.len() < 32 && (fav_iter.peek().is_some() || disc_iter.peek().is_some()) {
            let pick = if slot.is_multiple_of(3) {
                fav_iter.next().or_else(|| disc_iter.next())
            } else {
                disc_iter.next().or_else(|| fav_iter.next())
            };
            slot += 1;
            if let Some(t) = pick {
                let k = t.youtube_video_id.clone().unwrap_or_else(|| t.id.clone());
                if qp_seen.insert(k) {
                    quick_picks.push(t);
                }
            }
        }

        // Shelf 1: Listen again (user's most frequently played & liked songs)
        let listen_again_cards = tracks_to_song_cards(
            &taste
                .top_tracks
                .iter()
                .take(15)
                .cloned()
                .collect::<Vec<_>>(),
        );
        if listen_again_cards.len() >= 3 {
            shelves.push(YtmHomeShelf {
                title: "Listen again".to_string(),
                cards: listen_again_cards,
            });
        }

        // Shelf 2: Popular with listeners like you (unseen/fresh collaborative hits)
        let top_ids: HashSet<&str> = taste.top_tracks.iter().map(|t| t.id.as_str()).collect();
        let popular_discovery: Vec<Track> = ranked_discovery
            .iter()
            .filter(|t| !top_ids.contains(t.id.as_str()))
            .take(15)
            .cloned()
            .collect();
        let pop_cards = tracks_to_song_cards(&popular_discovery);
        if pop_cards.len() >= 4 {
            shelves.push(YtmHomeShelf {
                title: "Popular with listeners like you".to_string(),
                cards: pop_cards,
            });
        }

        // Shelf 3: Because you listen to <Top Artist 1>
        if let Some(a1) = &artist_1 {
            let mut artist_mix: Vec<Track> = Vec::new();
            if let Some(first_pool) = radio_pools.first() {
                for t in first_pool {
                    if !artist_mix.iter().any(|x| x.id == t.id) {
                        artist_mix.push(t.clone());
                    }
                }
            }
            for t in &artist_popular_tracks {
                if !artist_mix.iter().any(|x| x.id == t.id) {
                    artist_mix.push(t.clone());
                }
            }
            let a1_cards =
                tracks_to_song_cards(&artist_mix.into_iter().take(15).collect::<Vec<_>>());
            if a1_cards.len() >= 4 {
                shelves.push(YtmHomeShelf {
                    title: format!("Because you listen to {a1}"),
                    cards: a1_cards,
                });
            }
        }

        // Shelf 4: Albums, Artists & Playlists matching user's taste (replaces generic regional charts!)
        if taste_cards.len() >= 3 {
            shelves.push(YtmHomeShelf {
                title: "Recommended albums & playlists for you".to_string(),
                cards: taste_cards.into_iter().take(15).collect(),
            });
        }

        // Shelf 5+: Second seed's vibe mix (if distinct)
        if let Some(a2) = &artist_2
            && let Some(second_pool) = radio_pools.get(1)
        {
            let a2_cards = tracks_to_song_cards(second_pool);
            if a2_cards.len() >= 4 {
                shelves.push(YtmHomeShelf {
                    title: format!("More like {a2}"),
                    cards: a2_cards,
                });
            }
        }

        // Append ONLY locale-compatible non-regional-spam shelves from FEmusic_home below
        for shelf in raw_home_shelves {
            if is_regional_geoip_chart_shelf(&shelf.title, &shelf.cards, taste_locale) {
                continue;
            }
            if !shelves
                .iter()
                .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
            {
                shelves.push(shelf);
            }
        }

        // Aggregate all_tracks cleanly
        for t in quick_picks.iter().chain(ranked_discovery.iter()) {
            if !all_tracks.iter().any(|a| a.id == t.id) {
                all_tracks.push(t.clone());
            }
        }
    } else {
        // Cold-start or Mood-chip view
        quick_picks = raw_home_picks;
        shelves = raw_home_shelves;
        if taste_locale == MusicLocale::GlobalEn && has_taste {
            shelves.retain(|s| !is_regional_geoip_chart_shelf(&s.title, &s.cards, taste_locale));
        }
        all_tracks.extend(quick_picks.iter().cloned());
        if !session.is_signed_in() && quick_picks.len() > 8 {
            fastrand::shuffle(&mut quick_picks);
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

/// Load additional endless recommendation shelves using a `continuation` token (or a random Mood chip fallback).
pub fn fetch_home_continuation(
    session: &YtmSession,
    continuation: Option<&str>,
    fallback_mood_params: Option<&str>,
) -> ProviderResult<YtmRichHome> {
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

    let root = post_ytm_json("browse", &body, session)?;
    let visitor_data = extract_visitor_data(&root, &session.visitor_data);
    let mut quick_picks = Vec::new();
    let mut seen_tracks = HashSet::new();
    let mut shelves = Vec::new();
    collect_home_shelves(&root, &mut quick_picks, &mut seen_tracks, &mut shelves);

    let mut all_tracks = quick_picks.clone();
    collect_music_items(
        &root,
        "YouTube Music",
        48,
        &mut seen_tracks,
        &mut all_tracks,
    );
    let next_tok = extract_continuation_token(&root);
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
    for sep in ["(", "[", "|", " - ", " – ", " — "] {
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
        && (cand_core.starts_with(&format!("{seed_core} "))
            || seed_core.starts_with(&format!("{cand_core} ")))
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

fn is_low_quality_radio_candidate(
    title: &str,
    artist: &str,
    seed_title: &str,
    duration_ms: u64,
) -> bool {
    if duration_ms > 0 && !(75_000..=540_000).contains(&duration_ms) {
        return true;
    }
    let t = title.to_ascii_lowercase();
    let a = artist.to_ascii_lowercase();
    let seed_is_remix = seed_title.to_ascii_lowercase().contains("remix");
    if !seed_is_remix && (t.contains("remix") || a.contains("remix")) {
        return true;
    }
    for bad in [
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
    ] {
        if t.contains(bad) || a.contains(bad) {
            return true;
        }
    }
    false
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
    let fresh_params = [
        "wAEB8gECKAE%3D", // Discover
        "wAEB8gECGAE%3D", // Deep Cuts
        "wAEB8gECIAE%3D", // Popular
        "wAEB8gECMAE%3D", // Upbeat / Chill mix
    ];
    let mut body = json!({
        "videoId": video_id,
        "playlistId": format!("RDAMVM{video_id}"),
        "isAudioOnly": true,
        "enablePersistentPlaylistPanel": true,
        "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "context": web_remix_context_for_locale(&radio_session, seed_locale),
    });
    if force_fresh {
        body["params"] = json!(fresh_params[tick % fresh_params.len()]);
    }
    let root = post_ytm_json("next", &body, &radio_session)?;
    let mut raw_tracks = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(video_id.to_string());
    collect_radio_panel_items(
        &root,
        (target_limit * 3).max(36),
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
    if !hop_candidates.is_empty() {
        let hop_idx = tick % hop_candidates.len();
        let hop_vid = &hop_candidates[hop_idx];
        let mut hop_body = json!({
            "videoId": hop_vid,
            "playlistId": format!("RDAMVM{hop_vid}"),
            "isAudioOnly": true,
            "enablePersistentPlaylistPanel": true,
            "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            "context": web_remix_context_for_locale(&radio_session, seed_locale),
        });
        if force_fresh {
            hop_body["params"] = json!(fresh_params[(tick + 1) % fresh_params.len()]);
        }
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
    if coherent.len() < target_limit.min(10) {
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

    // 1. Official Songs results first — always carry verified Artist, Album, and exact Duration
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

fn fetch_player_root(video_id: &str, session: &YtmSession) -> ProviderResult<Value> {
    if !is_valid_video_id(video_id) {
        return Err(ProviderError::Metadata(
            "Invalid YouTube video ID".to_string(),
        ));
    }

    // Ensure fresh visitorData (required by VISIONOS 1.02)
    let mut visitor_data = session.visitor_data.clone();
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

    let agent = build_http_agent(18);
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
    }

    // Legacy cache compatibility for Normal/High if already downloaded
    if matches!(quality, AudioQuality::Normal | AudioQuality::High) {
        for (ext, codec, br) in [("m4a", "AAC", 128), ("webm", "OPUS", 150)] {
            let legacy = cache_dir.join(format!("{video_id}.{ext}"));
            if let Ok(meta) = fs::metadata(&legacy)
                && meta.len() > 16_384
            {
                return Ok(CachedAudioStream {
                    path: legacy,
                    codec: codec.to_string(),
                    bitrate_kbps: br,
                    duration_ms: 0,
                });
            }
        }
    }

    let root = fetch_player_root(video_id, session)?;
    let selected = select_audio_format_for_quality(&root, quality).ok_or_else(|| {
        ProviderError::Metadata("No playable audio format found in stream response".to_string())
    })?;

    let dest_path = cache_dir.join(format!("{video_id}_{suffix}.{}", selected.ext));
    let part_path = cache_dir.join(format!("{video_id}_{suffix}.{}.part", selected.ext));
    download_ranged_parallel(&selected.url, selected.content_length, &part_path)?;
    fs::rename(&part_path, &dest_path)?;

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
    // YouTube Music "Videos" search tab filter — surfaces `MUSIC_VIDEO_TYPE_OMV` official music videos
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
                    990_000 + kbps
                } else if itag == 140 {
                    920_000 + kbps
                } else if codec == "OPUS" {
                    880_000 + kbps
                } else {
                    800_000 + kbps
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

fn download_ranged_parallel(url: &str, content_length: u64, dest: &Path) -> ProviderResult<()> {
    if content_length == 0 {
        let agent = build_http_agent(45);
        let resp = agent
            .get(url)
            .header("User-Agent", VISIONOS_UA)
            .call()
            .map_err(|e| ProviderError::Http(format!("Audio fetch failed: {e}")))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(ProviderError::Http(format!("Audio CDN HTTP {status}")));
        }
        let bytes = resp
            .into_body()
            .into_with_config()
            .limit(40 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| ProviderError::Http(format!("Audio stream read failed: {e}")))?;
        let mut f = File::create(dest)?;
        f.write_all(&bytes)?;
        return Ok(());
    }

    let mut ranges = Vec::new();
    let mut start = 0u64;
    while start < content_length {
        let end = (start + STREAM_CHUNK_BYTES - 1).min(content_length - 1);
        ranges.push((start, end));
        start = end + 1;
    }

    let mut ordered_chunks: Vec<Vec<u8>> = vec![Vec::new(); ranges.len()];
    let mut idx = 0usize;
    while idx < ranges.len() {
        let batch_end = (idx + STREAM_CONCURRENCY).min(ranges.len());
        let batch = &ranges[idx..batch_end];
        let results: Vec<ProviderResult<Vec<u8>>> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(batch.len());
            for &(s, e) in batch {
                handles.push(scope.spawn(move || {
                    let agent = build_http_agent(25);
                    let range_hdr = format!("bytes={s}-{e}");
                    let resp = agent
                        .get(url)
                        .header("User-Agent", VISIONOS_UA)
                        .header("Range", &range_hdr)
                        .call()
                        .map_err(|err| ProviderError::Http(format!("Audio chunk failed: {err}")))?;
                    let st = resp.status().as_u16();
                    if !(200..300).contains(&st) {
                        return Err(ProviderError::Http(format!(
                            "Audio chunk returned HTTP {st}"
                        )));
                    }
                    resp.into_body()
                        .into_with_config()
                        .limit(4 * 1024 * 1024)
                        .read_to_vec()
                        .map_err(|err| {
                            ProviderError::Http(format!("Audio chunk read failed: {err}"))
                        })
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
        for (offset, res) in results.into_iter().enumerate() {
            ordered_chunks[idx + offset] = res?;
        }
        idx = batch_end;
    }

    let mut file = File::create(dest)?;
    for chunk in ordered_chunks {
        file.write_all(&chunk)?;
    }
    file.flush()?;
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
    let parts: Vec<u64> = text
        .trim()
        .split(':')
        .filter_map(|p| p.trim().parse::<u64>().ok())
        .collect();
    match parts.as_slice() {
        [m, s] => Some((m * 60 + s) * 1_000),
        [h, m, s] => Some((h * 3600 + m * 60 + s) * 1_000),
        _ => None,
    }
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
            "https://lh3.googleusercontent.com/abc123XYZ=w180-h180-l85-rj"
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
            Some("https://lh3.googleusercontent.com/cover99=w180-h180-l85-rj")
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
}
