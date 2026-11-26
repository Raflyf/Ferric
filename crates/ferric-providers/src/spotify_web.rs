//! Spotify web-session library access.
//!
//! The public Web API path needs an app Client ID, and Spotify now requires the app owner to
//! hold an active Premium subscription plus a 5-account development allowlist. This module
//! takes the other route: the user signs in to Spotify's own web player once and the desktop
//! login window captures the access token that the player fetched for itself.
//!
//! The token endpoint is deliberately not called from Rust anymore: it answers with HTML to
//! clients that do not reproduce the web player's own request, so the request has to stay in
//! the page. This module only validates the captured JSON.
//!
//! Scope of what this can do: read the signed-in user's own catalog (profile, Liked Songs,
//! playlists) so it can be mirrored into the local library. It does not obtain audio keys;
//! Spotify audio stays behind the account's own product tier.

use crate::oauth::{CloudSyncReport, SyncedPlaylistBundle};
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const SPOTIFY_WEB_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
const SPOTIFY_WEB_CLIENT_ID: &str = "d8a5ed958d274c2e8ee717e6a4b0971d";
const SPOTIFY_WEB_CLIENT_VERSION: &str = "1.2.58.0.g00000000";
const PATHFINDER_URL: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
const CLIENT_TOKEN_URL: &str = "https://clienttoken.spotify.com/v1/clienttoken";
const MAX_PATHFINDER_JSON_BYTES: u64 = 12 * 1024 * 1024;

const HASH_PROFILE_ATTRIBUTES: &str =
    "53bcb064f6cd18c23f752bc324a791194d20df612d8e1239c735144ab0399ced";
const HASH_FETCH_LIBRARY_TRACKS: &str =
    "087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240";
const HASH_LIBRARY_V3: &str = "973e511ca44261fda7eebac8b653155e7caee3675abb4fb110cc1b8c78b091c3";
const HASH_FETCH_PLAYLIST: &str =
    "7982b11e21535cd2594badc40030b745671b61a1fa66766e569d45e6364f3422";

/// Outcome when syncing a Spotify Web Player session through `api-partner.spotify.com/pathfinder`.
#[derive(Debug)]
pub enum SpotifyWebSyncError {
    /// The 1-hour web-player access token expired or was rejected (HTTP 401).
    AuthExpired,
    /// Pathfinder was unavailable or Spotify rotated a persisted query hash; caller may fall back
    /// to the public Web API v1 path.
    FallbackToRest(String),
}

struct CachedClientToken {
    token: String,
    expires_at: Instant,
}

static CLIENT_TOKEN_CACHE: OnceLock<Mutex<Option<CachedClientToken>>> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct WebTokenResponse {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "isAnonymous", default)]
    is_anonymous: bool,
}

/// Extract the `sp_dc` value out of a raw cookie header (`a=1; sp_dc=XYZ; b=2`).
pub fn extract_sp_dc(cookie_header: &str) -> Option<String> {
    for part in cookie_header.split(';') {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim() == "sp_dc" {
            let value = value.trim();
            if !value.is_empty() && value.len() <= 4096 {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Parse the token JSON captured from the signed-in web player.
///
/// WebView2's `ExecuteScript` returns the result JSON-encoded, so the payload may arrive as a
/// quoted, escaped string rather than raw JSON.
pub fn parse_web_token_json(raw: &str) -> ProviderResult<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(ProviderError::Http(
            "Spotify returned an empty token response".to_string(),
        ));
    }
    let text = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_string());
    let text = text.trim();
    if let Some(err) = text.strip_prefix("ERR:") {
        return Err(ProviderError::Http(format!(
            "Spotify token capture failed inside the page: {err}"
        )));
    }
    if text.starts_with('<') {
        return Err(ProviderError::Http(
            "Spotify restricted its web-player token endpoint (returned HTML). Use Instant Spotify Link Import below for public playlists/albums, or sign in with an official Spotify Client ID above."
                .to_string(),
        ));
    }
    let parsed: WebTokenResponse = serde_json::from_str(text)
        .map_err(|e| ProviderError::Http(format!("Invalid Spotify token JSON: {e}")))?;
    if parsed.is_anonymous {
        return Err(ProviderError::Metadata(
            "Spotify returned an anonymous token; the session is not signed in".to_string(),
        ));
    }
    match parsed.access_token {
        Some(token) if !token.trim().is_empty() => Ok(token),
        _ => Err(ProviderError::Http(
            "Spotify token response had no access token".to_string(),
        )),
    }
}

fn pathfinder_agent() -> ureq::Agent {
    // `api-partner.spotify.com` intermittently resets IPv6 rustls connections on Windows;
    // pinning IPv4 matches Spotify's web-player edge and avoids OS 10054 resets.
    let cfg = ureq::Agent::config_builder()
        .ip_family(ureq::config::IpFamily::Ipv4Only)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(16)))
        .timeout_global(Some(Duration::from_secs(24)))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(cfg)
}

fn invalidate_cached_client_token() {
    let lock = CLIENT_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = None;
    }
}

fn fetch_client_token(agent: &ureq::Agent) -> Result<String, SpotifyWebSyncError> {
    let lock = CLIENT_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = lock.lock()
        && let Some(cached) = guard.as_ref()
        && Instant::now() < cached.expires_at
    {
        return Ok(cached.token.clone());
    }

    let device_id = format!("ferric-{}", ferric_core::secrets::csprng_hex(12));
    let payload = json!({
        "client_data": {
            "client_version": SPOTIFY_WEB_CLIENT_VERSION,
            "client_id": SPOTIFY_WEB_CLIENT_ID,
            "js_sdk_data": {
                "device_brand": "unknown",
                "device_model": "unknown",
                "os": "windows",
                "os_version": "NT 10.0",
                "device_id": device_id,
                "device_type": "computer"
            }
        }
    });

    let resp = agent
        .post(CLIENT_TOKEN_URL)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("User-Agent", SPOTIFY_WEB_UA)
        .send(payload.to_string().as_bytes())
        .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("clienttoken request: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(256 * 1024)
        .read_to_vec()
        .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("clienttoken read: {e}")))?;

    if status != 200 {
        return Err(SpotifyWebSyncError::FallbackToRest(format!(
            "clienttoken HTTP {status}"
        )));
    }

    let val: Value = serde_json::from_slice(&bytes)
        .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("clienttoken json: {e}")))?;
    let token = val
        .pointer("/granted_token/token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            SpotifyWebSyncError::FallbackToRest("clienttoken missing granted_token".to_string())
        })?
        .to_string();
    let refresh_secs = val
        .pointer("/granted_token/refresh_after_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(1800)
        .clamp(300, 3600);

    if let Ok(mut guard) = lock.lock() {
        *guard = Some(CachedClientToken {
            token: token.clone(),
            expires_at: Instant::now() + Duration::from_secs(refresh_secs.saturating_sub(60)),
        });
    }
    Ok(token)
}

fn call_pathfinder(
    agent: &ureq::Agent,
    access_token: &str,
    client_token: &mut String,
    operation: &str,
    sha256_hash: &str,
    variables: Value,
) -> Result<Value, SpotifyWebSyncError> {
    let body = json!({
        "variables": variables,
        "operationName": operation,
        "extensions": {
            "persistedQuery": {
                "version": 1,
                "sha256Hash": sha256_hash
            }
        }
    })
    .to_string();

    for attempt in 0..2 {
        let resp = agent
            .post(PATHFINDER_URL)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("client-token", client_token.as_str())
            .header("app-platform", "WebPlayer")
            .header("Origin", "https://open.spotify.com")
            .header("Referer", "https://open.spotify.com/")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json;charset=UTF-8")
            .header("User-Agent", SPOTIFY_WEB_UA)
            .send(body.as_bytes())
            .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("{operation} net: {e}")))?;

        let status = resp.status().as_u16();
        let bytes = resp
            .into_body()
            .into_with_config()
            .limit(MAX_PATHFINDER_JSON_BYTES)
            .read_to_vec()
            .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("{operation} read: {e}")))?;

        if status == 401 {
            return Err(SpotifyWebSyncError::AuthExpired);
        }
        if status == 403 && attempt == 0 {
            invalidate_cached_client_token();
            *client_token = fetch_client_token(agent)?;
            continue;
        }
        if status != 200 {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(180).collect();
            return Err(SpotifyWebSyncError::FallbackToRest(format!(
                "{operation} HTTP {status}: {snippet}"
            )));
        }

        let val: Value = serde_json::from_slice(&bytes)
            .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("{operation} json: {e}")))?;

        if let Some(errors) = val.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
            && val.get("data").is_none_or(Value::is_null)
        {
            let msg = errors[0]
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("GraphQL error");
            return Err(SpotifyWebSyncError::FallbackToRest(format!(
                "{operation}: {msg}"
            )));
        }

        return Ok(val);
    }

    Err(SpotifyWebSyncError::FallbackToRest(format!(
        "{operation} exhausted retries"
    )))
}

fn pick_best_cover_url(sources: Option<&Value>) -> Option<String> {
    let arr = sources?.as_array()?;
    let mut best: Option<(u64, String)> = None;
    for src in arr {
        let Some(url) = src
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|u| u.starts_with("https://"))
        else {
            continue;
        };
        let height = src
            .get("height")
            .and_then(Value::as_u64)
            .or_else(|| src.get("width").and_then(Value::as_u64))
            .unwrap_or(300);
        // Prefer ~300px square covers for crisp cards without wasting RAM on huge 640px+ textures.
        let dist = height.abs_diff(300);
        if best.as_ref().is_none_or(|(best_dist, _)| dist < *best_dist) {
            best = Some((dist, url.to_string()));
        }
    }
    best.map(|(_, url)| url)
}

fn extract_artists_from_node(node: &Value) -> String {
    let mut names: Vec<&str> = Vec::new();
    for path in [
        "/artists/items",
        "/firstArtist/items",
        "/albumOfTrack/artists/items",
    ] {
        if let Some(items) = node.pointer(path).and_then(Value::as_array) {
            for item in items {
                if let Some(name) = item
                    .pointer("/profile/name")
                    .or_else(|| item.get("name"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    && !names.contains(&name)
                {
                    names.push(name);
                }
            }
            if !names.is_empty() {
                break;
            }
        }
    }
    if names.is_empty() {
        "Unknown Artist".to_string()
    } else {
        names.join(", ")
    }
}

fn map_pathfinder_track(uri_hint: Option<&str>, data: &Value, liked: bool) -> Option<Track> {
    let type_name = data.get("__typename").and_then(Value::as_str);
    if let Some(tn) = type_name
        && tn != "Track"
    {
        return None;
    }
    let raw_uri = uri_hint
        .or_else(|| data.get("uri").and_then(Value::as_str))
        .or_else(|| data.get("_uri").and_then(Value::as_str))?
        .trim();
    let spotify_id = raw_uri
        .strip_prefix("spotify:track:")
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();

    let title = data
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Untitled Track")
        .to_string();
    let artist = extract_artists_from_node(data);
    let album = data
        .pointer("/albumOfTrack/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Spotify")
        .to_string();
    let duration_ms = data
        .pointer("/duration/totalMilliseconds")
        .or_else(|| data.pointer("/trackDuration/totalMilliseconds"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let thumbnail_url = pick_best_cover_url(data.pointer("/albumOfTrack/coverArt/sources"));

    Some(Track {
        id: format!("spotify:{spotify_id}"),
        title,
        artist,
        album,
        duration_ms,
        path: None,
        source: TrackSource::Spotify,
        codec: "OGG".to_string(),
        sample_rate: 44_100,
        bit_depth: 16,
        bitrate_kbps: 320,
        isrc: None,
        spotify_id: Some(spotify_id),
        youtube_video_id: None,
        thumbnail_url,
        replay_gain_db: 0,
        liked,
    })
}

fn parse_profile_display_name(val: &Value) -> String {
    val.pointer("/data/me/profile/name")
        .or_else(|| val.pointer("/data/me/profile/username"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Spotify")
        .to_string()
}

fn parse_library_tracks_page(val: &Value) -> (Vec<Track>, Option<usize>) {
    let total = val
        .pointer("/data/me/library/tracks/totalCount")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    let Some(items) = val
        .pointer("/data/me/library/tracks/items")
        .and_then(Value::as_array)
    else {
        return (Vec::new(), total);
    };
    let mut out = Vec::with_capacity(items.len());
    for entry in items {
        let Some(track_wrap) = entry.get("track") else {
            continue;
        };
        let uri_hint = track_wrap
            .get("_uri")
            .or_else(|| track_wrap.get("uri"))
            .and_then(Value::as_str);
        let data = track_wrap.get("data").unwrap_or(track_wrap);
        if let Some(track) = map_pathfinder_track(uri_hint, data, true) {
            out.push(track);
        }
    }
    (out, total)
}

#[derive(Debug, Clone)]
struct PathfinderPlaylistStub {
    id: String,
    uri: String,
    name: String,
    description: String,
    revision_id: Option<String>,
    thumbnail_url: Option<String>,
}

fn parse_library_playlists(val: &Value) -> Vec<PathfinderPlaylistStub> {
    let Some(items) = val
        .pointer("/data/me/libraryV3/items")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for entry in items {
        let item_wrap = entry.get("item").unwrap_or(entry);
        let data = item_wrap.get("data").unwrap_or(item_wrap);
        if data.get("__typename").and_then(Value::as_str) == Some("PseudoPlaylist") {
            continue;
        }
        let Some(uri) = item_wrap
            .get("_uri")
            .or_else(|| data.get("uri"))
            .and_then(Value::as_str)
            .map(str::trim)
        else {
            continue;
        };
        let Some(pl_id) = uri.strip_prefix("spotify:playlist:").map(str::trim) else {
            continue;
        };
        if pl_id.is_empty() || !seen.insert(pl_id.to_string()) {
            continue;
        }
        let name = data
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Spotify Playlist")
            .to_string();
        let description = data
            .get("description")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let revision_id = data
            .get("revisionId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let thumbnail_url = pick_best_cover_url(data.pointer("/images/items/0/sources"));

        out.push(PathfinderPlaylistStub {
            id: pl_id.to_string(),
            uri: uri.to_string(),
            name,
            description,
            revision_id,
            thumbnail_url,
        });
    }
    out
}

fn parse_playlist_v2_items(
    pl_node: &Value,
    liked_ids: &HashSet<String>,
) -> (Vec<Track>, Option<usize>, usize) {
    let total = pl_node
        .pointer("/content/totalCount")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    let Some(items) = pl_node.pointer("/content/items").and_then(Value::as_array) else {
        return (Vec::new(), total, 0);
    };
    let raw_count = items.len();
    let mut tracks = Vec::with_capacity(raw_count);
    for entry in items {
        let item_v2 = entry.get("itemV2").unwrap_or(entry);
        let data = item_v2.get("data").unwrap_or(item_v2);
        let uri_hint = data
            .get("uri")
            .or_else(|| item_v2.get("_uri"))
            .and_then(Value::as_str);
        let is_liked = uri_hint
            .and_then(|u| u.strip_prefix("spotify:track:"))
            .is_some_and(|sid| liked_ids.contains(sid));
        if let Some(track) = map_pathfinder_track(uri_hint, data, is_liked) {
            tracks.push(track);
        }
    }
    (tracks, total, raw_count)
}

fn parse_playlist_v2_bundle(
    stub: &PathfinderPlaylistStub,
    val: &Value,
    liked_ids: &HashSet<String>,
) -> SyncedPlaylistBundle {
    let pl_node = val.pointer("/data/playlistV2").unwrap_or(val);
    let name = pl_node
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| stub.name.clone());
    let description = pl_node
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| stub.description.clone());
    let revision_id = pl_node
        .get("revisionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| stub.revision_id.clone());
    let thumbnail_url = pick_best_cover_url(pl_node.pointer("/images/items/0/sources"))
        .or_else(|| stub.thumbnail_url.clone());

    let (tracks, _, _) = parse_playlist_v2_items(pl_node, liked_ids);

    let playlist = Playlist {
        id: format!("spotify:playlist:{}", stub.id),
        name,
        source: TrackSource::Spotify,
        etag: revision_id,
        track_count: tracks.len(),
        description,
        thumbnail_url,
    };
    SyncedPlaylistBundle { playlist, tracks }
}

/// Sync the signed-in user's Spotify profile, Liked Songs (up to 500), and playlists (up to 150,
/// with up to 500 tracks per playlist) via Spotify's Web Player Pathfinder GraphQL API
/// (`api-partner.spotify.com/pathfinder/v2/query`).
///
/// Unlike `api.spotify.com/v1`, Pathfinder does not throttle web-player tokens with shared HTTP 429
/// buckets and returns album/playlist artwork URLs directly.
pub fn sync_spotify_web_catalog(
    access_token: &str,
    existing_snapshots: &HashMap<String, String>,
) -> Result<CloudSyncReport, SpotifyWebSyncError> {
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;

    let profile_val = call_pathfinder(
        &agent,
        access_token,
        &mut client_token,
        "profileAttributes",
        HASH_PROFILE_ATTRIBUTES,
        json!({}),
    )?;
    let display_name = parse_profile_display_name(&profile_val);

    let mut liked_tracks = Vec::new();
    let mut liked_ids = HashSet::new();
    for page in 0..10_usize {
        thread::sleep(Duration::from_millis(140));
        let offset = page * 50;
        let liked_val = match call_pathfinder(
            &agent,
            access_token,
            &mut client_token,
            "fetchLibraryTracks",
            HASH_FETCH_LIBRARY_TRACKS,
            json!({ "offset": offset, "limit": 50 }),
        ) {
            Ok(v) => v,
            Err(SpotifyWebSyncError::AuthExpired) => return Err(SpotifyWebSyncError::AuthExpired),
            Err(err) if page == 0 => return Err(err),
            Err(_) => break,
        };
        let (batch, total_count) = parse_library_tracks_page(&liked_val);
        let batch_len = batch.len();
        for track in batch {
            if let Some(sid) = &track.spotify_id {
                liked_ids.insert(sid.clone());
            }
            liked_tracks.push(track);
        }
        if batch_len < 50
            || liked_tracks.len() >= 500
            || total_count.is_some_and(|tot| offset + batch_len >= tot)
        {
            break;
        }
    }

    let mut stubs: Vec<PathfinderPlaylistStub> = Vec::new();
    let mut seen_pl_ids: HashSet<String> = HashSet::new();
    for page in 0..3_usize {
        thread::sleep(Duration::from_millis(140));
        let offset = page * 50;
        let library_val = match call_pathfinder(
            &agent,
            access_token,
            &mut client_token,
            "libraryV3",
            HASH_LIBRARY_V3,
            json!({
                "filters": ["Playlists"],
                "order": null,
                "textFilter": "",
                "features": ["LIKED_SONGS", "YOUR_EPISODES_V2", "PRERELEASES", "EVENTS"],
                "limit": 50,
                "offset": offset,
                "flatten": true,
                "expandedFolders": [],
                "folderUri": null,
                "includeFoldersWhenFlattening": true
            }),
        ) {
            Ok(v) => v,
            Err(SpotifyWebSyncError::AuthExpired) => return Err(SpotifyWebSyncError::AuthExpired),
            Err(err) if page == 0 => return Err(err),
            Err(_) => break,
        };
        let raw_len = library_val
            .pointer("/data/me/libraryV3/items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let total_lib = library_val
            .pointer("/data/me/libraryV3/totalCount")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        for stub in parse_library_playlists(&library_val) {
            if seen_pl_ids.insert(stub.id.clone()) {
                stubs.push(stub);
            }
        }
        if raw_len < 50 || total_lib.is_some_and(|tot| offset + raw_len >= tot) {
            break;
        }
    }

    let mut playlists = Vec::with_capacity(stubs.len());
    let mut etag_skipped = 0usize;
    let mut rate_limited_skipped = 0usize;
    let total_stubs = stubs.len();

    for (idx, stub) in stubs.into_iter().enumerate() {
        let local_key = format!("spotify:playlist:{}", stub.id);
        if let Some(rev) = stub.revision_id.as_deref()
            && existing_snapshots
                .get(&local_key)
                .is_some_and(|local| local.trim() == rev)
        {
            etag_skipped += 1;
            continue;
        }
        thread::sleep(Duration::from_millis(160));
        let pl_val = match call_pathfinder(
            &agent,
            access_token,
            &mut client_token,
            "fetchPlaylist",
            HASH_FETCH_PLAYLIST,
            json!({
                "uri": stub.uri,
                "offset": 0,
                "limit": 100,
                "enableWatchFeedEntrypoint": false
            }),
        ) {
            Ok(v) => v,
            Err(SpotifyWebSyncError::AuthExpired) => return Err(SpotifyWebSyncError::AuthExpired),
            Err(SpotifyWebSyncError::FallbackToRest(msg)) if msg.contains("HTTP 429") => {
                rate_limited_skipped += total_stubs.saturating_sub(idx);
                break;
            }
            Err(_) => continue,
        };
        let mut bundle = parse_playlist_v2_bundle(&stub, &pl_val, &liked_ids);
        let first_node = pl_val.pointer("/data/playlistV2").unwrap_or(&pl_val);
        let first_raw_count = first_node
            .pointer("/content/items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let total_pl_tracks = first_node
            .pointer("/content/totalCount")
            .and_then(Value::as_u64)
            .map(|n| n as usize);

        if first_raw_count >= 100 && total_pl_tracks.is_none_or(|tot| tot > 100) {
            for page in 1..5_usize {
                let offset = page * 100;
                if total_pl_tracks.is_some_and(|tot| offset >= tot) {
                    break;
                }
                thread::sleep(Duration::from_millis(140));
                let Ok(next_val) = call_pathfinder(
                    &agent,
                    access_token,
                    &mut client_token,
                    "fetchPlaylist",
                    HASH_FETCH_PLAYLIST,
                    json!({
                        "uri": stub.uri,
                        "offset": offset,
                        "limit": 100,
                        "enableWatchFeedEntrypoint": false
                    }),
                ) else {
                    break;
                };
                let next_node = next_val.pointer("/data/playlistV2").unwrap_or(&next_val);
                let (more_tracks, _, raw_count) = parse_playlist_v2_items(next_node, &liked_ids);
                bundle.tracks.extend(more_tracks);
                if raw_count < 100 || total_pl_tracks.is_some_and(|tot| offset + raw_count >= tot) {
                    break;
                }
            }
            bundle.playlist.track_count = bundle.tracks.len();
        }
        playlists.push(bundle);
    }

    Ok(CloudSyncReport {
        display_name,
        liked_tracks,
        playlists,
        etag_skipped,
        rate_limited_skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sp_dc_from_cookie_header() {
        let header = "sp_t=abc; sp_dc=AQBc123XYZ; sp_key=def";
        assert_eq!(extract_sp_dc(header).as_deref(), Some("AQBc123XYZ"));
    }

    #[test]
    fn ignores_missing_or_empty_sp_dc() {
        assert_eq!(extract_sp_dc("sp_t=abc; sp_key=def"), None);
        assert_eq!(extract_sp_dc("sp_dc=; sp_t=abc"), None);
        assert_eq!(extract_sp_dc(""), None);
    }

    #[test]
    fn parses_raw_and_quoted_token_json() {
        let raw = r#"{"accessToken":"tok_123","isAnonymous":false}"#;
        assert_eq!(parse_web_token_json(raw).unwrap(), "tok_123");

        let quoted = serde_json::to_string(raw).unwrap();
        assert_eq!(parse_web_token_json(&quoted).unwrap(), "tok_123");
    }

    #[test]
    fn rejects_anonymous_and_error_payloads() {
        let anon = r#"{"accessToken":"x","isAnonymous":true}"#;
        assert!(parse_web_token_json(anon).is_err());
        assert!(parse_web_token_json("ERR:boom").is_err());
        assert!(parse_web_token_json("<!DOCTYPE html><html></html>").is_err());
        assert!(parse_web_token_json("").is_err());
    }

    #[test]
    fn parses_pathfinder_profile_liked_library_and_playlist() {
        let profile = json!({
            "data": { "me": { "profile": { "name": "Rafly", "username": "u123" } } }
        });
        assert_eq!(parse_profile_display_name(&profile), "Rafly");

        let liked = json!({
            "data": {
                "me": {
                    "library": {
                        "tracks": {
                            "totalCount": 1,
                            "items": [{
                                "track": {
                                    "_uri": "spotify:track:7HDdZDjTsBdBCGiM7yZWeI",
                                    "data": {
                                        "__typename": "Track",
                                        "name": "Midnight Drive",
                                        "duration": { "totalMilliseconds": 201000 },
                                        "artists": { "items": [{ "profile": { "name": "LA UVA" } }] },
                                        "albumOfTrack": {
                                            "name": "Night EP",
                                            "coverArt": {
                                                "sources": [
                                                    { "height": 640, "url": "https://i.scdn.co/image/big" },
                                                    { "height": 300, "url": "https://i.scdn.co/image/med" }
                                                ]
                                            }
                                        }
                                    }
                                }
                            }]
                        }
                    }
                }
            }
        });
        let (tracks, total) = parse_library_tracks_page(&liked);
        assert_eq!(total, Some(1));
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, "spotify:7HDdZDjTsBdBCGiM7yZWeI");
        assert_eq!(tracks[0].artist, "LA UVA");
        assert_eq!(tracks[0].album, "Night EP");
        assert_eq!(tracks[0].duration_ms, 201_000);
        assert_eq!(
            tracks[0].thumbnail_url.as_deref(),
            Some("https://i.scdn.co/image/med")
        );
        assert!(tracks[0].liked);

        let library = json!({
            "data": {
                "me": {
                    "libraryV3": {
                        "items": [
                            { "item": { "_uri": "spotify:collection:tracks", "data": { "__typename": "PseudoPlaylist", "name": "Liked Songs" } } },
                            { "item": { "_uri": "spotify:playlist:abc123", "data": { "__typename": "Playlist", "name": "Late Night", "revisionId": "rev-9", "images": { "items": [{ "sources": [{ "height": 300, "url": "https://i.scdn.co/image/pl" }] }] } } } }
                        ]
                    }
                }
            }
        });
        let stubs = parse_library_playlists(&library);
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].id, "abc123");
        assert_eq!(stubs[0].revision_id.as_deref(), Some("rev-9"));

        let playlist_v2 = json!({
            "data": {
                "playlistV2": {
                    "name": "Late Night",
                    "description": "Chill tracks",
                    "revisionId": "rev-10",
                    "content": {
                        "items": [
                            {
                                "itemV2": {
                                    "data": {
                                        "__typename": "Track",
                                        "uri": "spotify:track:7HDdZDjTsBdBCGiM7yZWeI",
                                        "name": "Midnight Drive",
                                        "trackDuration": { "totalMilliseconds": 201000 },
                                        "artists": { "items": [{ "profile": { "name": "LA UVA" } }] },
                                        "albumOfTrack": { "name": "Night EP" }
                                    }
                                }
                            },
                            {
                                "itemV2": {
                                    "data": {
                                        "__typename": "Episode",
                                        "uri": "spotify:episode:ep1",
                                        "name": "Podcast Ep"
                                    }
                                }
                            }
                        ]
                    }
                }
            }
        });
        let liked_set: HashSet<String> =
            ["7HDdZDjTsBdBCGiM7yZWeI".to_string()].into_iter().collect();
        let bundle = parse_playlist_v2_bundle(&stubs[0], &playlist_v2, &liked_set);
        assert_eq!(bundle.playlist.id, "spotify:playlist:abc123");
        assert_eq!(bundle.playlist.etag.as_deref(), Some("rev-10"));
        assert_eq!(bundle.playlist.track_count, 1);
        assert_eq!(bundle.tracks.len(), 1);
        assert!(bundle.tracks[0].liked);
    }
}
