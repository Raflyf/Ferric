//! Pure-Rust YouTube Music InnerTube client for Zero-Login Guest Mode (Home feed, live Search,
//! and direct audio stream caching) and 1-Click Google Account Sign-In (`SAPISIDHASH` session).

use crate::yt_cleaner::clean_youtube_metadata;
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde_json::{Value, json};
use std::collections::HashSet;
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
const STREAM_CONCURRENCY: usize = 6;

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

    if full.contains("lh3.googleusercontent.com") || full.contains("yt3.ggpht.com") {
        let no_query = full.split('?').next().unwrap_or(&full);
        if let Some(slash_idx) = no_query.rfind('/')
            && let Some(eq_rel) = no_query[slash_idx..].find('=')
        {
            let base = &no_query[..slash_idx + eq_rel];
            return format!("{base}=w300-h300-l90-rj");
        }
        return format!("{no_query}=w300-h300-l90-rj");
    }

    if full.contains("i.ytimg.com") || full.contains("i9.ytimg.com") {
        let no_query = full.split('?').next().unwrap_or(&full);
        return no_query.to_string();
    }

    full
}

fn extract_best_thumbnail(val: &Value, fallback_video_id: Option<&str>) -> Option<String> {
    let arr = val
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
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

fn build_http_agent(timeout_secs: u64) -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(cfg)
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

fn post_ytm_json(endpoint: &str, body: &Value, session: &YtmSession) -> ProviderResult<Value> {
    let url = format!("{YTM_ORIGIN}/youtubei/v1/{endpoint}?prettyPrint=false");
    let agent = build_http_agent(16);
    let mut req = agent
        .post(&url)
        .header("Content-Type", "application/json")
        .header("User-Agent", WEB_REMIX_UA)
        .header("Origin", YTM_ORIGIN)
        .header("Referer", "https://music.youtube.com/")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("X-YouTube-Client-Name", "67")
        .header("X-YouTube-Client-Version", WEB_REMIX_VERSION);

    if !session.visitor_data.is_empty() {
        req = req.header("X-Goog-Visitor-Id", &session.visitor_data);
    }
    if session.is_signed_in() {
        let auth = build_sapisidhash_header(&session.sapisid, YTM_ORIGIN, unix_timestamp_secs());
        req = req
            .header("Cookie", &session.cookie)
            .header("Authorization", &auth)
            .header("X-Origin", YTM_ORIGIN)
            .header("X-Goog-AuthUser", "0");
    }

    let payload = serde_json::to_string(body)
        .map_err(|e| ProviderError::Metadata(format!("JSON encode failed: {e}")))?;
    let resp = req
        .send(&payload)
        .map_err(|e| ProviderError::Http(format!("YouTube Music {endpoint} failed: {e}")))?;
    let status = resp.status().as_u16();
    let text = resp
        .into_body()
        .into_with_config()
        .limit(10 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| ProviderError::Http(format!("YouTube Music read failed: {e}")))?;
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http(format!(
            "YouTube Music {endpoint} returned HTTP {status}"
        )));
    }
    serde_json::from_str(&text)
        .map_err(|e| ProviderError::Metadata(format!("YouTube Music JSON parse failed: {e}")))
}

fn web_remix_context(session: &YtmSession) -> Value {
    let mut client = json!({
        "clientName": "WEB_REMIX",
        "clientVersion": WEB_REMIX_VERSION,
        "hl": "en",
        "gl": "ID"
    });
    if !session.visitor_data.is_empty() {
        client["visitorData"] = json!(session.visitor_data);
    }
    json!({ "client": client })
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

/// Fetch Sonora-style rich YouTube Music Home feed: Mood Chips, Quick Picks songs,
/// and horizontal Album/Playlist/Song-Mix shelves (plus `FEmusic_explore` New Albums & Singles).
pub fn fetch_rich_home(
    session: &YtmSession,
    mood_params: Option<&str>,
) -> ProviderResult<YtmRichHome> {
    let mut body = json!({
        "browseId": "FEmusic_home",
        "context": web_remix_context(session),
    });
    if let Some(params) = mood_params.map(str::trim).filter(|p| !p.is_empty()) {
        body["params"] = json!(params);
    }

    let root = post_ytm_json("browse", &body, session)?;
    let visitor_data = extract_visitor_data(&root, &session.visitor_data);

    let mut chips = Vec::new();
    collect_mood_chips(&root, &mut chips);

    let mut quick_picks = Vec::new();
    let mut seen_tracks = HashSet::new();
    let mut shelves = Vec::new();
    collect_home_shelves(&root, &mut quick_picks, &mut seen_tracks, &mut shelves);

    // Ensure every song in the home payload is also discoverable in `all_tracks`
    let mut all_tracks = quick_picks.clone();
    collect_music_items(
        &root,
        "YouTube Music",
        48,
        &mut seen_tracks,
        &mut all_tracks,
    );
    if quick_picks.len() < 8 {
        for t in &all_tracks {
            if quick_picks.len() >= 16 {
                break;
            }
            if !quick_picks.iter().any(|q| q.id == t.id) {
                quick_picks.push(t.clone());
            }
        }
    }

    // Supplement default Home with `FEmusic_explore` (New Albums & Singles + Trending)
    if mood_params.map(str::trim).is_none_or(str::is_empty) {
        let mut next_sess = session.clone();
        next_sess.visitor_data = visitor_data.clone();
        let explore_body = json!({
            "browseId": "FEmusic_explore",
            "context": web_remix_context(&next_sess),
        });
        if let Ok(explore_root) = post_ytm_json("browse", &explore_body, &next_sess) {
            let mut explore_picks = Vec::new();
            let mut explore_shelves = Vec::new();
            collect_home_shelves(
                &explore_root,
                &mut explore_picks,
                &mut seen_tracks,
                &mut explore_shelves,
            );
            for t in explore_picks {
                if quick_picks.len() < 24 && !quick_picks.iter().any(|q| q.id == t.id) {
                    quick_picks.push(t.clone());
                }
                if !all_tracks.iter().any(|a| a.id == t.id) {
                    all_tracks.push(t);
                }
            }
            for shelf in explore_shelves {
                if !shelves
                    .iter()
                    .any(|s| s.title.eq_ignore_ascii_case(&shelf.title))
                {
                    shelves.push(shelf);
                }
            }
            collect_music_items(
                &explore_root,
                "YouTube Music Explore",
                72,
                &mut seen_tracks,
                &mut all_tracks,
            );
        }
    }

    Ok(YtmRichHome {
        visitor_data,
        chips,
        quick_picks,
        shelves,
        all_tracks,
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

/// Fetch infinite Radio / Up Next recommendations for a playing YouTube Music song via `youtubei/v1/next`.
pub fn fetch_radio_up_next(
    session: &YtmSession,
    video_id: &str,
    limit: usize,
) -> ProviderResult<Vec<Track>> {
    if !is_valid_video_id(video_id) {
        return Err(ProviderError::Metadata(
            "Invalid videoId for radio".to_string(),
        ));
    }
    let body = json!({
        "videoId": video_id,
        "playlistId": format!("RDAMVM{video_id}"),
        "isAudioOnly": true,
        "context": web_remix_context(session),
    });
    let root = post_ytm_json("next", &body, session)?;
    let mut tracks = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(video_id.to_string());
    collect_radio_panel_items(&root, limit.max(1), &mut seen, &mut tracks);
    Ok(tracks)
}

/// Search YouTube Music returning both playable songs and clickable Album / Artist / Playlist cards.
pub fn fetch_rich_search(
    session: &YtmSession,
    query: &str,
    limit: usize,
) -> ProviderResult<YtmRichSearch> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(YtmRichSearch::default());
    }
    let general_body = json!({
        "query": q,
        "context": web_remix_context(session),
    });
    let root = post_ytm_json("search", &general_body, session)?;
    let mut tracks = Vec::new();
    let mut seen_vids = HashSet::new();
    let mut cards = Vec::new();
    let mut seen_cards = HashSet::new();
    collect_music_items(
        &root,
        "YouTube Music",
        limit.max(1),
        &mut seen_vids,
        &mut tracks,
    );
    collect_search_cards(&root, 18, &mut seen_cards, &mut cards);

    if tracks.len() < 12 {
        let songs_body = json!({
            "query": q,
            "params": "EgWKAQIIAWoMEAMQBBAJEAoQBRAV",
            "context": web_remix_context(session),
        });
        if let Ok(songs_root) = post_ytm_json("search", &songs_body, session) {
            collect_music_items(
                &songs_root,
                "YouTube Music",
                limit.max(1),
                &mut seen_vids,
                &mut tracks,
            );
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

/// Resolve direct YouTube audio stream via `VISIONOS 1.02` (zero PO-token, zero 403) and download
/// in 6 parallel 512KB Range chunks into `cache_dir`, returning a local audio file playable by `ferric-audio`.
pub fn resolve_and_cache_audio_stream(
    video_id: &str,
    session: &YtmSession,
    cache_dir: &Path,
) -> ProviderResult<CachedAudioStream> {
    if !is_valid_video_id(video_id) {
        return Err(ProviderError::Metadata(
            "Invalid YouTube video ID".to_string(),
        ));
    }
    fs::create_dir_all(cache_dir)?;

    // 0 ms cache hit if already downloaded
    for (ext, codec, br) in [("m4a", "AAC", 128), ("webm", "OPUS", 150)] {
        let candidate = cache_dir.join(format!("{video_id}.{ext}"));
        if let Ok(meta) = fs::metadata(&candidate)
            && meta.len() > 16_384
        {
            return Ok(CachedAudioStream {
                path: candidate,
                codec: codec.to_string(),
                bitrate_kbps: br,
                duration_ms: 0,
            });
        }
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

    let selected = select_best_audio_format(&root).ok_or_else(|| {
        ProviderError::Metadata("No playable audio format found in stream response".to_string())
    })?;

    let dest_path = cache_dir.join(format!("{video_id}.{}", selected.ext));
    let part_path = cache_dir.join(format!("{video_id}.{}.part", selected.ext));
    download_ranged_parallel(&selected.url, selected.content_length, &part_path)?;
    fs::rename(&part_path, &dest_path)?;

    Ok(CachedAudioStream {
        path: dest_path,
        codec: selected.codec.to_string(),
        bitrate_kbps: selected.bitrate_kbps,
        duration_ms: selected.duration_ms,
    })
}

struct SelectedStreamFormat {
    url: String,
    ext: &'static str,
    codec: &'static str,
    bitrate_kbps: u32,
    content_length: u64,
    duration_ms: u64,
}

fn select_best_audio_format(root: &Value) -> Option<SelectedStreamFormat> {
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

        let (ext, codec, priority) = if itag == 140 {
            ("m4a", "AAC", 100)
        } else if mime.starts_with("audio/mp4") {
            ("m4a", "AAC", 80)
        } else if itag == 251 {
            ("webm", "OPUS", 70)
        } else if mime.starts_with("audio/webm") {
            ("webm", "OPUS", 50)
        } else {
            continue;
        };
        let score = priority * 1_000_000 + (bitrate.min(999_999) as u32);
        let candidate = SelectedStreamFormat {
            url: url.to_string(),
            ext,
            codec,
            bitrate_kbps: (bitrate / 1_000) as u32,
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

    let subtitle = flex_cols
        .get(1)
        .and_then(|c| {
            extract_runs_text(c.pointer("/musicResponsiveListItemFlexColumnRenderer/text"))
        })
        .unwrap_or_default();

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
                    && shelves.len() < 8
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

    for part in parts {
        let lower = part.to_ascii_lowercase();
        if lower == "song" || lower == "video" || lower == "single" || lower == "ep" {
            continue;
        }
        if let Some(ms) = parse_clock_ms(part) {
            duration_ms = ms;
            continue;
        }
        if lower.ends_with("plays") || lower.ends_with("views") || lower.ends_with("views.") {
            continue;
        }
        if artist.is_empty() {
            artist = part.to_string();
        } else if album.is_empty() {
            album = part.to_string();
        }
    }

    if artist.is_empty() {
        artist = "YouTube Music".to_string();
    }
    if album.is_empty() {
        album = default_album.to_string();
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
}
