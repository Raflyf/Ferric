//! Zero-key public playlist, album, track, and search sync for Spotify and YouTube / YouTube Music.
//! Uses official public web embed/oEmbed metadata pages without requiring developer API keys
//! and without any DRM bypass or credential scraping.

use crate::yt_cleaner::clean_youtube_metadata;
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde_json::Value;
use std::time::Duration;

const PUBLIC_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";

fn build_http_agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .max_redirects(2)
        .timeout_global(Some(Duration::from_secs(14)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(cfg)
}

fn fetch_text(url: &str) -> ProviderResult<String> {
    let agent = build_http_agent();
    let resp = agent
        .get(url)
        .header("User-Agent", PUBLIC_USER_AGENT)
        .header("Accept-Language", "en-US,en;q=0.9")
        .call()
        .map_err(|e| ProviderError::Http(format!("Request failed: {e}")))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(ProviderError::Http(format!(
            "Remote page returned HTTP {status}"
        )));
    }
    resp.into_body()
        .into_with_config()
        .limit(8 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| ProviderError::Http(format!("Failed to read response body: {e}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyTarget {
    Playlist(String),
    Album(String),
    Track(String),
}

impl SpotifyTarget {
    fn kind_str(&self) -> &'static str {
        match self {
            Self::Playlist(_) => "playlist",
            Self::Album(_) => "album",
            Self::Track(_) => "track",
        }
    }

    fn id(&self) -> &str {
        match self {
            Self::Playlist(id) | Self::Album(id) | Self::Track(id) => id,
        }
    }
}

/// Parse a Spotify web URL (`https://open.spotify.com/playlist/...`) or URI (`spotify:playlist:...`).
pub fn parse_spotify_target(raw: &str) -> Option<SpotifyTarget> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("spotify:") {
        let mut parts = rest.split(':');
        let kind = parts.next()?;
        let id = parts.next()?.trim();
        if !is_valid_id(id) {
            return None;
        }
        return match kind {
            "playlist" => Some(SpotifyTarget::Playlist(id.to_string())),
            "album" => Some(SpotifyTarget::Album(id.to_string())),
            "track" => Some(SpotifyTarget::Track(id.to_string())),
            _ => None,
        };
    }

    let without_query = trimmed.split('?').next().unwrap_or(trimmed);
    let segments: Vec<&str> = without_query.split('/').filter(|s| !s.is_empty()).collect();
    for pair in segments.windows(2) {
        let kind = pair[0];
        let id = pair[1];
        if !is_valid_id(id) {
            continue;
        }
        match kind {
            "playlist" => return Some(SpotifyTarget::Playlist(id.to_string())),
            "album" => return Some(SpotifyTarget::Album(id.to_string())),
            "track" => return Some(SpotifyTarget::Track(id.to_string())),
            _ => {}
        }
    }
    None
}

/// Import tracks and playlist metadata from any public Spotify playlist, album, or track URL
/// without requiring a Spotify Developer Client ID.
pub fn import_spotify_public_url(raw_url: &str) -> ProviderResult<(Playlist, Vec<Track>)> {
    let target = parse_spotify_target(raw_url).ok_or_else(|| {
        ProviderError::Metadata(
            "Invalid Spotify link. Paste a URL like https://open.spotify.com/playlist/..."
                .to_string(),
        )
    })?;
    let embed_url = format!(
        "https://open.spotify.com/embed/{}/{}",
        target.kind_str(),
        target.id()
    );
    let html = fetch_text(&embed_url)?;
    if let Ok(parsed) = parse_spotify_embed_html(&target, &html) {
        return Ok(parsed);
    }

    // Fallback: Spotify public oEmbed endpoint for basic title metadata
    let canonical = format!(
        "https://open.spotify.com/{}/{}",
        target.kind_str(),
        target.id()
    );
    let oembed_url = format!(
        "https://open.spotify.com/oembed?url={}",
        percent_encode(&canonical)
    );
    let oembed_raw = fetch_text(&oembed_url)?;
    let v: Value = serde_json::from_str(&oembed_raw)
        .map_err(|e| ProviderError::Metadata(format!("Spotify oEmbed JSON error: {e}")))?;
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Spotify Import")
        .to_string();
    let thumb = v
        .get("thumbnail_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let track = Track {
        id: format!("spotify:{}:{}", target.kind_str(), target.id()),
        title: title.clone(),
        artist: "Spotify".to_string(),
        album: title.clone(),
        duration_ms: 0,
        path: None,
        source: TrackSource::Spotify,
        codec: "SPOTIFY".to_string(),
        sample_rate: 0,
        bit_depth: 0,
        bitrate_kbps: 0,
        isrc: None,
        spotify_id: Some(target.id().to_string()),
        youtube_video_id: None,
        thumbnail_url: thumb.clone(),
        replay_gain_db: 0,
        liked: false,
    };
    let pl = Playlist {
        id: format!("spotify:{}:{}", target.kind_str(), target.id()),
        name: title,
        source: TrackSource::Spotify,
        etag: None,
        track_count: 1,
        description: "Imported via Spotify Public Link".to_string(),
        thumbnail_url: thumb,
    };
    Ok((pl, vec![track]))
}

pub fn parse_spotify_embed_html(
    target: &SpotifyTarget,
    html: &str,
) -> ProviderResult<(Playlist, Vec<Track>)> {
    let marker = "id=\"__NEXT_DATA__\"";
    let start_tag = html
        .find(marker)
        .ok_or_else(|| ProviderError::Metadata("Spotify embed payload not found".to_string()))?;
    let after_tag = &html[start_tag..];
    let json_start = after_tag
        .find('>')
        .map(|i| i + 1)
        .ok_or_else(|| ProviderError::Metadata("Malformed Spotify embed tag".to_string()))?;
    let after_gt = &after_tag[json_start..];
    let json_end = after_gt
        .find("</script>")
        .ok_or_else(|| ProviderError::Metadata("Unterminated Spotify embed JSON".to_string()))?;
    let json_str = after_gt[..json_end].trim();

    let root: Value = serde_json::from_str(json_str)
        .map_err(|e| ProviderError::Metadata(format!("Spotify embed JSON parse error: {e}")))?;
    let entity = root
        .pointer("/props/pageProps/state/data/entity")
        .ok_or_else(|| ProviderError::Metadata("Spotify entity missing in payload".to_string()))?;

    let container_name = entity
        .get("name")
        .or_else(|| entity.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Spotify Playlist")
        .trim()
        .to_string();

    let cover_url = entity
        .pointer("/coverArt/sources")
        .or_else(|| entity.pointer("/visualIdentity/image"))
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .filter_map(|s| s.get("url").and_then(Value::as_str))
                .next()
        })
        .map(ToOwned::to_owned);

    let mut tracks = Vec::new();
    if let Some(list) = entity.get("trackList").and_then(Value::as_array) {
        for (idx, item) in list.iter().enumerate() {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                continue;
            }
            let artist = item
                .get("subtitle")
                .and_then(Value::as_str)
                .unwrap_or("Unknown Artist")
                .replace('\u{00A0}', " ")
                .trim()
                .to_string();
            let duration_ms = item.get("duration").and_then(Value::as_u64).unwrap_or(0);
            let uri = item.get("uri").and_then(Value::as_str).unwrap_or("");
            let sp_id = uri.strip_prefix("spotify:track:").map(ToOwned::to_owned);
            let id = if !uri.is_empty() {
                uri.to_string()
            } else {
                format!("spotify:{}:{}:{idx}", target.kind_str(), target.id())
            };
            tracks.push(Track {
                id,
                title: title.to_string(),
                artist,
                album: container_name.clone(),
                duration_ms,
                path: None,
                source: TrackSource::Spotify,
                codec: "SPOTIFY".to_string(),
                sample_rate: 0,
                bit_depth: 0,
                bitrate_kbps: 0,
                isrc: None,
                spotify_id: sp_id,
                youtube_video_id: None,
                thumbnail_url: cover_url.clone(),
                replay_gain_db: 0,
                liked: false,
            });
        }
    } else if !container_name.is_empty() {
        let artists: String = entity
            .get("artists")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|a| a.get("name").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Spotify".to_string());
        let duration_ms = entity.get("duration").and_then(Value::as_u64).unwrap_or(0);
        tracks.push(Track {
            id: format!("spotify:track:{}", target.id()),
            title: container_name.clone(),
            artist: artists,
            album: "Spotify Single".to_string(),
            duration_ms,
            path: None,
            source: TrackSource::Spotify,
            codec: "SPOTIFY".to_string(),
            sample_rate: 0,
            bit_depth: 0,
            bitrate_kbps: 0,
            isrc: None,
            spotify_id: Some(target.id().to_string()),
            youtube_video_id: None,
            thumbnail_url: cover_url.clone(),
            replay_gain_db: 0,
            liked: false,
        });
    }

    if tracks.is_empty() {
        return Err(ProviderError::Metadata(
            "No public tracks found in Spotify embed payload".to_string(),
        ));
    }

    let playlist = Playlist {
        id: format!("spotify:{}:{}", target.kind_str(), target.id()),
        name: container_name,
        source: TrackSource::Spotify,
        etag: None,
        track_count: tracks.len(),
        description: "Imported via Spotify Public Link".to_string(),
        thumbnail_url: cover_url,
    };
    Ok((playlist, tracks))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YouTubeTarget {
    Playlist(String),
    Video(String),
}

/// Parse a YouTube / YouTube Music playlist or video URL.
pub fn parse_youtube_target(raw: &str) -> Option<YouTubeTarget> {
    let trimmed = raw.trim();
    if let Some(list_id) = extract_query_param(trimmed, "list")
        && is_valid_id(&list_id)
    {
        return Some(YouTubeTarget::Playlist(list_id));
    }
    if let Some(vid) = extract_query_param(trimmed, "v")
        && is_valid_id(&vid)
    {
        return Some(YouTubeTarget::Video(vid));
    }
    if let Some(rest) = trimmed
        .strip_prefix("https://youtu.be/")
        .or_else(|| trimmed.strip_prefix("http://youtu.be/"))
    {
        let vid = rest.split(['?', '&', '/']).next().unwrap_or("").trim();
        if is_valid_id(vid) {
            return Some(YouTubeTarget::Video(vid.to_string()));
        }
    }
    None
}

/// Import tracks from any public YouTube / YouTube Music playlist or video URL without an API key.
pub fn import_youtube_public_url(raw_url: &str) -> ProviderResult<(Playlist, Vec<Track>)> {
    let target = parse_youtube_target(raw_url).ok_or_else(|| {
        ProviderError::Metadata(
            "Invalid YouTube link. Paste a playlist (?list=...) or video (?v=...) URL.".to_string(),
        )
    })?;

    match target {
        YouTubeTarget::Playlist(list_id) => {
            let url = format!("https://www.youtube.com/playlist?list={list_id}&hl=en");
            let html = fetch_text(&url)?;
            parse_youtube_playlist_html(&list_id, &html)
        }
        YouTubeTarget::Video(vid) => {
            let watch_url = format!("https://www.youtube.com/watch?v={vid}");
            let oembed_url = format!(
                "https://www.youtube.com/oembed?url={}&format=json",
                percent_encode(&watch_url)
            );
            let raw = fetch_text(&oembed_url)?;
            let v: Value = serde_json::from_str(&raw)
                .map_err(|e| ProviderError::Metadata(format!("YouTube oEmbed JSON error: {e}")))?;
            let raw_title = v
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("YouTube Track");
            let raw_author = v
                .get("author_name")
                .and_then(Value::as_str)
                .unwrap_or("YouTube Music");
            let cleaned = clean_youtube_metadata(raw_title, raw_author);
            let thumb = Some(format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"));
            let track = Track {
                id: format!("yt:video:{vid}"),
                title: cleaned.title.clone(),
                artist: cleaned.artist,
                album: "YouTube Music".to_string(),
                duration_ms: 0,
                path: None,
                source: TrackSource::YouTubeMusic,
                codec: "YT MUSIC".to_string(),
                sample_rate: 0,
                bit_depth: 0,
                bitrate_kbps: 0,
                isrc: None,
                spotify_id: None,
                youtube_video_id: Some(vid.clone()),
                thumbnail_url: thumb.clone(),
                replay_gain_db: 0,
                liked: false,
            };
            let pl = Playlist {
                id: format!("yt:single:{vid}"),
                name: cleaned.title,
                source: TrackSource::YouTubeMusic,
                etag: None,
                track_count: 1,
                description: "Imported via YouTube Link".to_string(),
                thumbnail_url: thumb,
            };
            Ok((pl, vec![track]))
        }
    }
}

/// Search public YouTube music tracks without requiring a Google API key.
pub fn search_youtube_public(query: &str, limit: usize) -> ProviderResult<Vec<Track>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://www.youtube.com/results?search_query={}&hl=en",
        percent_encode(q)
    );
    let html = fetch_text(&url)?;
    parse_youtube_search_html(&html, limit.max(1))
}

pub fn parse_youtube_playlist_html(
    list_id: &str,
    html: &str,
) -> ProviderResult<(Playlist, Vec<Track>)> {
    let root = extract_yt_initial_data(html)?;
    let pl_title = root
        .pointer("/metadata/playlistMetadataRenderer/title")
        .or_else(|| root.pointer("/header/playlistHeaderRenderer/title/simpleText"))
        .and_then(Value::as_str)
        .unwrap_or("YouTube Playlist")
        .trim()
        .to_string();

    let mut tracks = Vec::new();
    collect_youtube_renderers(&root, "playlistVideoRenderer", &pl_title, 200, &mut tracks);

    if tracks.is_empty() {
        return Err(ProviderError::Metadata(
            "No public songs found in YouTube playlist (make sure playlist visibility is Public or Unlisted)."
                .to_string(),
        ));
    }

    let pl_thumb = tracks.first().and_then(|t| t.thumbnail_url.clone());
    let playlist = Playlist {
        id: format!("yt:playlist:{list_id}"),
        name: pl_title,
        source: TrackSource::YouTubeMusic,
        etag: None,
        track_count: tracks.len(),
        description: "Imported via YouTube Public Playlist".to_string(),
        thumbnail_url: pl_thumb,
    };
    Ok((playlist, tracks))
}

pub fn parse_youtube_search_html(html: &str, limit: usize) -> ProviderResult<Vec<Track>> {
    let root = extract_yt_initial_data(html)?;
    let mut tracks = Vec::new();
    collect_youtube_renderers(&root, "videoRenderer", "YouTube Search", limit, &mut tracks);
    Ok(tracks)
}

fn extract_yt_initial_data(html: &str) -> ProviderResult<Value> {
    let markers = [
        "var ytInitialData = ",
        "window[\"ytInitialData\"] = ",
        "ytInitialData = ",
    ];
    let Some(start_idx) = markers
        .iter()
        .find_map(|m| html.find(m).map(|i| i + m.len()))
    else {
        return Err(ProviderError::Metadata(
            "YouTube page initial data not found".to_string(),
        ));
    };
    let slice = &html[start_idx..];
    let json_slice = extract_balanced_json_object(slice)
        .ok_or_else(|| ProviderError::Metadata("Malformed YouTube JSON object".to_string()))?;
    serde_json::from_str(json_slice)
        .map_err(|e| ProviderError::Metadata(format!("YouTube JSON parse error: {e}")))
}

fn extract_balanced_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_youtube_renderers(
    val: &Value,
    renderer_key: &str,
    album_name: &str,
    limit: usize,
    out: &mut Vec<Track>,
) {
    if out.len() >= limit {
        return;
    }
    match val {
        Value::Object(map) => {
            if let Some(obj) = map.get(renderer_key).and_then(Value::as_object)
                && let Some(vid) = obj.get("videoId").and_then(Value::as_str)
                && is_valid_id(vid)
            {
                let raw_title = extract_yt_text(obj.get("title")).unwrap_or_default();
                if !raw_title.is_empty()
                    && !out
                        .iter()
                        .any(|t| t.youtube_video_id.as_deref() == Some(vid))
                {
                    let raw_author = extract_yt_text(obj.get("shortBylineText"))
                        .or_else(|| extract_yt_text(obj.get("ownerText")))
                        .or_else(|| extract_yt_text(obj.get("longBylineText")))
                        .unwrap_or_else(|| "YouTube Music".to_string());
                    let dur_ms = obj
                        .get("lengthSeconds")
                        .and_then(Value::as_str)
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|s| s * 1_000)
                        .or_else(|| {
                            extract_yt_text(obj.get("lengthText")).and_then(|t| parse_clock_ms(&t))
                        })
                        .unwrap_or(0);
                    let cleaned = clean_youtube_metadata(&raw_title, &raw_author);
                    out.push(Track {
                        id: format!("yt:video:{vid}"),
                        title: cleaned.title,
                        artist: cleaned.artist,
                        album: album_name.to_string(),
                        duration_ms: dur_ms,
                        path: None,
                        source: TrackSource::YouTubeMusic,
                        codec: "YT MUSIC".to_string(),
                        sample_rate: 0,
                        bit_depth: 0,
                        bitrate_kbps: 0,
                        isrc: None,
                        spotify_id: None,
                        youtube_video_id: Some(vid.to_string()),
                        thumbnail_url: Some(format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg")),
                        replay_gain_db: 0,
                        liked: false,
                    });
                }
            }
            for child in map.values() {
                if out.len() >= limit {
                    break;
                }
                collect_youtube_renderers(child, renderer_key, album_name, limit, out);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                if out.len() >= limit {
                    break;
                }
                collect_youtube_renderers(child, renderer_key, album_name, limit, out);
            }
        }
        _ => {}
    }
}

fn extract_yt_text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return Some(s.trim().to_string());
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

fn extract_query_param(url: &str, key: &str) -> Option<String> {
    let (_, query) = url.split_once('?')?;
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == key {
            return Some(v.split('#').next().unwrap_or(v).trim().to_string());
        }
    }
    None
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spotify_urls_and_uris() {
        assert_eq!(
            parse_spotify_target("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc"),
            Some(SpotifyTarget::Playlist(
                "37i9dQZF1DXcBWIGoYBM5M".to_string()
            ))
        );
        assert_eq!(
            parse_spotify_target("spotify:album:4m2880jivSbbyEGAKfITCa"),
            Some(SpotifyTarget::Album("4m2880jivSbbyEGAKfITCa".to_string()))
        );
        assert_eq!(
            parse_spotify_target("https://open.spotify.com/track/2cGxRwrMyEAp8dEbuZaVv6"),
            Some(SpotifyTarget::Track("2cGxRwrMyEAp8dEbuZaVv6".to_string()))
        );
    }

    #[test]
    fn parses_spotify_embed_next_data_tracks() {
        let sample = r#"<html><script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"state":{"data":{"entity":{"name":"Late Night Lossless","trackList":[{"uri":"spotify:track:2cGxRwrMyEAp8dEbuZaVv6","title":"Instant Crush","subtitle":"Daft Punk, Julian Casablancas","duration":337000}]}}}}}}</script></html>"#;
        let target = SpotifyTarget::Playlist("test123".to_string());
        let (pl, tracks) = parse_spotify_embed_html(&target, sample).expect("parse embed");
        assert_eq!(pl.name, "Late Night Lossless");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Instant Crush");
        assert_eq!(tracks[0].artist, "Daft Punk, Julian Casablancas");
        assert_eq!(tracks[0].duration_ms, 337_000);
    }

    #[test]
    fn parses_youtube_playlist_and_search_initial_data() {
        assert_eq!(
            parse_youtube_target("https://music.youtube.com/playlist?list=PL12345_abc"),
            Some(YouTubeTarget::Playlist("PL12345_abc".to_string()))
        );
        assert_eq!(
            parse_youtube_target("https://youtu.be/a5uQMwRMHcs?t=10"),
            Some(YouTubeTarget::Video("a5uQMwRMHcs".to_string()))
        );
        let html = r#"<script>var ytInitialData = {"metadata":{"playlistMetadataRenderer":{"title":"Indie Mix"}},"contents":{"playlistVideoRenderer":{"videoId":"a5uQMwRMHcs","title":{"runs":[{"text":"Daft Punk - Instant Crush (Official Video)"}]},"shortBylineText":{"runs":[{"text":"Daft Punk - Topic"}]},"lengthSeconds":"337"}}};</script>"#;
        let (pl, tracks) = parse_youtube_playlist_html("PL12345_abc", html).expect("parse yt");
        assert_eq!(pl.name, "Indie Mix");
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].youtube_video_id.as_deref(), Some("a5uQMwRMHcs"));
        assert_eq!(tracks[0].duration_ms, 337_000);
    }
}
