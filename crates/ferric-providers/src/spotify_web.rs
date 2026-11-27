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
const MAX_WEB_BUNDLE_BYTES: u64 = 12 * 1024 * 1024;

const HASH_ACCOUNT_ATTRIBUTES: &str =
    "be4ce359daee01463ddb8e77536220e108e3d28105e12199f244d7a5954e9d14";
const HASH_PROFILE_ATTRIBUTES: &str =
    "08ffb4730af3746e04a8301396f20875dbbce10c75243803091a9274eacc8ac0";
const HASH_FETCH_LIBRARY_TRACKS: &str =
    "087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240";
const HASH_LIBRARY_V3: &str = "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455";
const HASH_FETCH_PLAYLIST: &str =
    "21ea11a6ac75a5f3e01fd365d29c18e0a4a8b7dc09f89bbb5d73910190c6acaf";
const HASH_HOME: &str = "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1";

/// A personalized Recommended Station returned directly by Spotify's Pathfinder `home` query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotifyStationCard {
    pub playlist_id: String,
    pub title: String,
    pub display_name: String,
    pub subtitle: String,
    pub thumbnail_url: Option<String>,
}

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
static RESOLVED_HASH_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyTierVerdict {
    Premium,
    NotPremium(String),
    Unknown,
}

/// Fail-closed classifier for Spotify subscription tier fields from Pathfinder `accountAttributes`
/// (`data.me.account.product` and `data.me.account.attributes.catalogue`) or REST `/v1/me`
/// (`product`). Non-premium tiers (`free`, `open`, `shuffle-restricted`, `ads`) are explicitly
/// rejected; missing/unknown values return `Unknown` so callers fail closed.
pub fn classify_spotify_premium(
    product: Option<&str>,
    catalogue: Option<&str>,
) -> SpotifyTierVerdict {
    let norm = |raw: Option<&str>| -> Option<String> {
        raw.map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase)
    };
    let prod = norm(product);
    let cat = norm(catalogue);

    for val in [prod.as_deref(), cat.as_deref()].into_iter().flatten() {
        if matches!(
            val,
            "free" | "open" | "shuffle-restricted" | "ads" | "ad-supported"
        ) || val.starts_with("free")
        {
            return SpotifyTierVerdict::NotPremium(val.to_string());
        }
    }

    if prod.as_deref() == Some("premium") || cat.as_deref() == Some("premium") {
        return SpotifyTierVerdict::Premium;
    }

    if let Some(other) = prod.or(cat) {
        return SpotifyTierVerdict::NotPremium(other);
    }

    SpotifyTierVerdict::Unknown
}

fn parse_account_attributes_tier(val: &Value) -> (Option<String>, Option<String>) {
    let product = val
        .pointer("/data/me/account/product")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let catalogue = val
        .pointer("/data/me/account/attributes/catalogue")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (product, catalogue)
}

fn extract_web_bundle_url_from_html(html: &str) -> Option<String> {
    const PREFIX: &str = "https://open.spotifycdn.com/cdn/build/web-player/web-player.";
    let start = html.find(PREFIX)?;
    let rest = &html[start..];
    let end = rest.find(".js")?;
    let candidate = &rest[..end + 3];
    if candidate.len() <= 200
        && candidate
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'.' | b'-' | b'_'))
    {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn extract_operation_hash_from_bundle(js: &str, operation: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{quote}{operation}{quote},{quote}query{quote},{quote}");
        if let Some(pos) = js.find(&needle) {
            let hash_start = pos + needle.len();
            if let Some(cand) = js.get(hash_start..hash_start + 64)
                && cand.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Some(cand.to_ascii_lowercase());
            }
        }
    }
    None
}

fn lookup_cached_hash(operation: &str, fallback: &str) -> String {
    let lock = RESOLVED_HASH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(h) = guard.get(operation) {
        return h.clone();
    }
    fallback.to_string()
}

fn refresh_hashes_from_web_bundle(agent: &ureq::Agent) -> Option<HashMap<String, String>> {
    let resp = agent
        .get("https://open.spotify.com/")
        .header("User-Agent", SPOTIFY_WEB_UA)
        .call()
        .ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    let html_bytes = resp
        .into_body()
        .into_with_config()
        .limit(2 * 1024 * 1024)
        .read_to_vec()
        .ok()?;
    let html = String::from_utf8_lossy(&html_bytes);
    let bundle_url = extract_web_bundle_url_from_html(&html)?;

    let js_resp = agent
        .get(&bundle_url)
        .header("User-Agent", SPOTIFY_WEB_UA)
        .call()
        .ok()?;
    if js_resp.status().as_u16() != 200 {
        return None;
    }
    let js_bytes = js_resp
        .into_body()
        .into_with_config()
        .limit(MAX_WEB_BUNDLE_BYTES)
        .read_to_vec()
        .ok()?;
    let js = String::from_utf8_lossy(&js_bytes);

    let mut refreshed = HashMap::new();
    for op in [
        "accountAttributes",
        "profileAttributes",
        "fetchLibraryTracks",
        "libraryV3",
        "fetchPlaylist",
        "home",
    ] {
        if let Some(hash) = extract_operation_hash_from_bundle(&js, op) {
            refreshed.insert(op.to_string(), hash);
        }
    }
    if !refreshed.is_empty() {
        let lock = RESOLVED_HASH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        for (k, v) in &refreshed {
            guard.insert(k.clone(), v.clone());
        }
    }
    Some(refreshed)
}

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
    crate::shared_http_agent_ipv4().clone()
}

fn invalidate_cached_client_token() {
    let lock = CLIENT_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    *guard = None;
}

fn fetch_client_token(agent: &ureq::Agent) -> Result<String, SpotifyWebSyncError> {
    let lock = CLIENT_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = guard.as_ref()
            && Instant::now() < cached.expires_at
        {
            return Ok(cached.token.clone());
        }
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

    {
        let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
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
    default_sha256_hash: &str,
    variables: Value,
) -> Result<Value, SpotifyWebSyncError> {
    let mut active_hash = lookup_cached_hash(operation, default_sha256_hash);
    let mut refreshed_hashes = false;

    for attempt in 0..3 {
        let body = json!({
            "variables": variables,
            "operationName": operation,
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": active_hash
                }
            }
        })
        .to_string();

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
            if msg.contains("PersistedQueryNotFound")
                && !refreshed_hashes
                && let Some(map) = refresh_hashes_from_web_bundle(agent)
                && let Some(new_hash) = map.get(operation)
                && new_hash != &active_hash
            {
                refreshed_hashes = true;
                active_hash = new_hash.clone();
                continue;
            }
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

/// Verify that the signed-in Spotify OAuth account holds an active Spotify Premium subscription
/// (fail-closed).
///
/// Queries Spotify's Pathfinder `accountAttributes` endpoint first (avoiding `api.spotify.com/v1`
/// shared rate limits) and only falls back to `/v1/me` if Pathfinder cannot return a tier. Free,
/// open, or unverifiable accounts are rejected immediately.
pub fn verify_spotify_premium_account(access_token: &str) -> ProviderResult<String> {
    let agent = pathfinder_agent();
    let pathfinder_err: Option<String> = match fetch_client_token(&agent) {
        Ok(mut client_token) => {
            match call_pathfinder(
                &agent,
                access_token,
                &mut client_token,
                "accountAttributes",
                HASH_ACCOUNT_ATTRIBUTES,
                json!({}),
            ) {
                Ok(acct_val) => {
                    let (product, catalogue) = parse_account_attributes_tier(&acct_val);
                    match classify_spotify_premium(product.as_deref(), catalogue.as_deref()) {
                        SpotifyTierVerdict::Premium => {
                            let display_name = call_pathfinder(
                                &agent,
                                access_token,
                                &mut client_token,
                                "profileAttributes",
                                HASH_PROFILE_ATTRIBUTES,
                                json!({}),
                            )
                            .map(|v| parse_profile_display_name(&v))
                            .unwrap_or_else(|_| "Spotify Premium".to_string());
                            return Ok(display_name);
                        }
                        SpotifyTierVerdict::NotPremium(tier) => {
                            return Err(ProviderError::Http(format!(
                                "Spotify Premium required: this account is on Spotify Free ({tier}). Use 'Spotify Account (Free & Premium)' to sign in."
                            )));
                        }
                        SpotifyTierVerdict::Unknown => {
                            Some("accountAttributes did not include a product tier".to_string())
                        }
                    }
                }
                Err(SpotifyWebSyncError::AuthExpired) => {
                    return Err(ProviderError::Http(
                        "Spotify rejected the OAuth access token (HTTP 401). Please try signing in again."
                            .to_string(),
                    ));
                }
                Err(SpotifyWebSyncError::FallbackToRest(msg)) => Some(msg),
            }
        }
        Err(SpotifyWebSyncError::AuthExpired) => {
            return Err(ProviderError::Http(
                "Spotify rejected the OAuth access token (HTTP 401).".to_string(),
            ));
        }
        Err(SpotifyWebSyncError::FallbackToRest(msg)) => Some(msg),
    };

    match crate::oauth::fetch_spotify_profile_tier(access_token) {
        Ok((display_name, product)) => match classify_spotify_premium(product.as_deref(), None) {
            SpotifyTierVerdict::Premium => Ok(display_name),
            SpotifyTierVerdict::NotPremium(tier) => Err(ProviderError::Http(format!(
                "Spotify Premium required: this account is on Spotify Free ({tier}). Use 'Spotify Account (Free & Premium)' to sign in."
            ))),
            SpotifyTierVerdict::Unknown => Err(ProviderError::Http(
                "Could not verify Spotify Premium subscription status from your account profile. Direct OAuth sign-in requires Spotify Premium; use 'Spotify Web Sign-In' below for Free accounts."
                    .to_string(),
            )),
        },
        Err(rest_err) => {
            let detail = pathfinder_err
                .map(|p| format!("{p}; REST fallback: {rest_err}"))
                .unwrap_or_else(|| rest_err.to_string());
            Err(ProviderError::Http(format!(
                "Could not verify Spotify Premium subscription status ({detail}). Sign-in blocked; retry shortly or use 'Spotify Web Sign-In' below."
            )))
        }
    }
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
    sync_spotify_web_catalog_with_prefix(access_token, existing_snapshots, "spotify:playlist:")
}

/// Sync the signed-in user's Spotify profile, Liked Songs (up to 500), and playlists (up to 150,
/// with up to 500 tracks per playlist) via Spotify's Web Player Pathfinder GraphQL API, using
/// `playlist_id_prefix` (`"spotify:playlist:"` for Web Sign-In or `"spotify:premium:playlist:"`
/// for Direct OAuth Premium) so both sessions stay isolated in SQLite.
pub fn sync_spotify_web_catalog_with_prefix(
    access_token: &str,
    existing_snapshots: &HashMap<String, String>,
    playlist_id_prefix: &str,
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
        let local_key = format!("{playlist_id_prefix}{}", stub.id);
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
        bundle.playlist.id = local_key;
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
            let mut partial_page_failed = false;
            for page in 1..5_usize {
                let offset = page * 100;
                if total_pl_tracks.is_some_and(|tot| offset >= tot) {
                    break;
                }
                thread::sleep(Duration::from_millis(140));
                let next_val = match call_pathfinder(
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
                ) {
                    Ok(v) => v,
                    Err(SpotifyWebSyncError::AuthExpired) => {
                        return Err(SpotifyWebSyncError::AuthExpired);
                    }
                    Err(_) => {
                        partial_page_failed = true;
                        break;
                    }
                };
                let next_node = next_val.pointer("/data/playlistV2").unwrap_or(&next_val);
                let (more_tracks, _, raw_count) = parse_playlist_v2_items(next_node, &liked_ids);
                bundle.tracks.extend(more_tracks);
                if raw_count < 100 || total_pl_tracks.is_some_and(|tot| offset + raw_count >= tot) {
                    break;
                }
            }
            if partial_page_failed {
                // Do not persist revision snapshot on partial playlist pagination so next sync fetches missing pages.
                bundle.playlist.etag = None;
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

fn strip_html_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_radio_suffix(title: &str) -> String {
    let trimmed = title.trim();
    for suffix in [" Radio", " radio", " RADIO"] {
        if let Some(base) = trimmed.strip_suffix(suffix) {
            let clean = base.trim();
            if !clean.is_empty() {
                return clean.to_string();
            }
        }
    }
    trimmed.to_string()
}

fn extract_station_from_home_item(item: &Value) -> Option<SpotifyStationCard> {
    let content = item
        .get("content")
        .or_else(|| item.get("item"))
        .unwrap_or(item);
    let data = content.get("data").unwrap_or(content);
    let typename = data.get("__typename").and_then(Value::as_str);
    if let Some(tn) = typename
        && tn != "Playlist"
        && tn != "PlaylistResponseWrapper"
    {
        return None;
    }
    let uri = item
        .get("uri")
        .or_else(|| content.get("_uri"))
        .or_else(|| content.get("uri"))
        .or_else(|| data.get("uri"))
        .and_then(Value::as_str)?
        .trim();
    let playlist_id = uri.strip_prefix("spotify:playlist:")?.trim();
    if playlist_id.is_empty()
        || playlist_id.len() > 64
        || !playlist_id.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    let title = data
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let subtitle = data
        .get("description")
        .and_then(Value::as_str)
        .map(strip_html_tags)
        .unwrap_or_default();
    let thumbnail_url = pick_best_cover_url(data.pointer("/images/items/0/sources"))
        .or_else(|| pick_best_cover_url(data.pointer("/coverArt/sources")));
    let display_name = strip_radio_suffix(&title);
    Some(SpotifyStationCard {
        playlist_id: playlist_id.to_string(),
        title,
        display_name,
        subtitle,
        thumbnail_url,
    })
}

fn section_title_of(section: &Value) -> String {
    section
        .pointer("/data/title/transformedLabel")
        .or_else(|| section.pointer("/data/title/text"))
        .or_else(|| section.pointer("/data/title"))
        .or_else(|| section.pointer("/title/transformedLabel"))
        .or_else(|| section.pointer("/title/text"))
        .or_else(|| section.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

fn is_radio_station_section_title(title: &str) -> bool {
    let lower = title.to_ascii_lowercase();
    lower.contains("recommended station")
        || lower.contains("stasiun")
        || lower.contains("radio station")
        || lower == "stations"
        || lower.contains("popular radio")
        || lower.contains("recommended radio")
}

fn is_radio_station_card(card: &SpotifyStationCard) -> bool {
    let lower_title = card.title.to_ascii_lowercase();
    let lower_sub = card.subtitle.to_ascii_lowercase();
    card.playlist_id.starts_with("37i9dQZF1E4")
        || card.playlist_id.starts_with("37i9dQZF1E8")
        || lower_title.ends_with(" radio")
        || lower_sub.starts_with("with ")
}

fn parse_home_recommended_stations(val: &Value) -> Vec<SpotifyStationCard> {
    let Some(sections) = val
        .pointer("/data/home/sectionContainer/sections/items")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // Pass 1: Dedicated "Recommended Stations" / "Popular Radio" sections returned by Spotify Home.
    for section in sections {
        let title = section_title_of(section);
        if !is_radio_station_section_title(&title) {
            continue;
        }
        let Some(items) = section
            .pointer("/sectionItems/items")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for item in items {
            if let Some(card) = extract_station_from_home_item(item)
                && seen.insert(card.playlist_id.clone())
            {
                out.push(card);
                if out.len() >= 10 {
                    return out;
                }
            }
        }
    }

    // Pass 2: Any other Spotify Home shelf containing Radio Station playlists (`37i9dQZF1E4/8` or `"* Radio"`),
    // ensuring localized Spotify accounts still populate authentic Recommended Stations.
    for section in sections {
        let Some(items) = section
            .pointer("/sectionItems/items")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for item in items {
            if let Some(card) = extract_station_from_home_item(item)
                && is_radio_station_card(&card)
                && seen.insert(card.playlist_id.clone())
            {
                out.push(card);
                if out.len() >= 10 {
                    return out;
                }
            }
        }
    }

    out
}

/// Fetch the signed-in user's personalized Spotify "Recommended Stations" shelf directly from
/// Spotify's Pathfinder `home` GraphQL endpoint.
pub fn fetch_spotify_recommended_stations(
    access_token: &str,
) -> Result<Vec<SpotifyStationCard>, SpotifyWebSyncError> {
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let val = call_pathfinder(
        &agent,
        access_token,
        &mut client_token,
        "home",
        HASH_HOME,
        json!({
            "timeZone": "Asia/Jakarta",
            "sp_t": "",
            "facet": "",
            "sectionItemsLimit": 12,
            "includeHomeChapterVideoCards": false,
            "homeEndUserIntegration": "INTEGRATION_WEB_PLAYER",
            "includeEpisodeContentRatingsV2": false
        }),
    )?;
    Ok(parse_home_recommended_stations(&val))
}

/// Fetch the full tracklist (~50 songs) of any Spotify Radio Station or playlist by its 22-char
/// playlist ID via Pathfinder `fetchPlaylist`.
pub fn fetch_spotify_playlist_by_id(
    access_token: &str,
    playlist_id: &str,
    liked_ids: &HashSet<String>,
) -> Result<SyncedPlaylistBundle, SpotifyWebSyncError> {
    let pid = playlist_id.trim();
    if pid.is_empty() || pid.len() > 64 || !pid.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(SpotifyWebSyncError::FallbackToRest(
            "Invalid Spotify playlist ID".to_string(),
        ));
    }
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let uri = format!("spotify:playlist:{pid}");
    let pl_val = call_pathfinder(
        &agent,
        access_token,
        &mut client_token,
        "fetchPlaylist",
        HASH_FETCH_PLAYLIST,
        json!({
            "uri": uri,
            "offset": 0,
            "limit": 100,
            "enableWatchFeedEntrypoint": false
        }),
    )?;
    let stub = PathfinderPlaylistStub {
        id: pid.to_string(),
        uri,
        name: "Spotify Radio".to_string(),
        description: String::new(),
        revision_id: None,
        thumbnail_url: None,
    };
    let mut bundle = parse_playlist_v2_bundle(&stub, &pl_val, liked_ids);
    bundle.playlist.description = strip_html_tags(&bundle.playlist.description);
    Ok(bundle)
}

fn clean_spotify_playlist_id(raw: &str) -> Option<&str> {
    let stripped = raw
        .trim()
        .strip_prefix("spotify:premium:playlist:")
        .or_else(|| raw.trim().strip_prefix("spotify:playlist:"))
        .unwrap_or(raw.trim());
    if stripped.is_empty()
        || stripped.len() > 64
        || !stripped
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    Some(stripped)
}

fn clean_spotify_track_uri(raw: &str) -> Option<String> {
    let stripped = raw
        .trim()
        .strip_prefix("spotify:track:")
        .unwrap_or(raw.trim());
    if stripped.is_empty()
        || stripped.len() > 64
        || !stripped.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(format!("spotify:track:{stripped}"))
}

fn spotify_rest_json(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    access_token: &str,
    client_token: &mut String,
    body: Option<&Value>,
) -> Result<Value, SpotifyWebSyncError> {
    let payload = body
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("encode json: {e}")))?;

    for attempt in 0..2 {
        let resp = match method {
            "GET" => agent
                .get(url)
                .header("Authorization", &format!("Bearer {access_token}"))
                .header("client-token", client_token.as_str())
                .header("Accept", "application/json")
                .header("User-Agent", SPOTIFY_WEB_UA)
                .header("Origin", "https://open.spotify.com")
                .header("Referer", "https://open.spotify.com/")
                .call(),
            "POST" => agent
                .post(url)
                .header("Authorization", &format!("Bearer {access_token}"))
                .header("client-token", client_token.as_str())
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("User-Agent", SPOTIFY_WEB_UA)
                .header("Origin", "https://open.spotify.com")
                .header("Referer", "https://open.spotify.com/")
                .send(payload.as_deref().unwrap_or("{}").as_bytes()),
            "PUT" => agent
                .put(url)
                .header("Authorization", &format!("Bearer {access_token}"))
                .header("client-token", client_token.as_str())
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("User-Agent", SPOTIFY_WEB_UA)
                .header("Origin", "https://open.spotify.com")
                .header("Referer", "https://open.spotify.com/")
                .send(payload.as_deref().unwrap_or("{}").as_bytes()),
            "DELETE" => {
                if let Some(p) = payload.as_deref() {
                    agent
                        .delete(url)
                        .header("Authorization", &format!("Bearer {access_token}"))
                        .header("client-token", client_token.as_str())
                        .header("Accept", "application/json")
                        .header("Content-Type", "application/json")
                        .header("User-Agent", SPOTIFY_WEB_UA)
                        .header("Origin", "https://open.spotify.com")
                        .header("Referer", "https://open.spotify.com/")
                        .force_send_body()
                        .send(p.as_bytes())
                } else {
                    agent
                        .delete(url)
                        .header("Authorization", &format!("Bearer {access_token}"))
                        .header("client-token", client_token.as_str())
                        .header("Accept", "application/json")
                        .header("User-Agent", SPOTIFY_WEB_UA)
                        .header("Origin", "https://open.spotify.com")
                        .header("Referer", "https://open.spotify.com/")
                        .call()
                }
            }
            _ => {
                return Err(SpotifyWebSyncError::FallbackToRest(format!(
                    "unsupported method {method}"
                )));
            }
        }
        .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("{method} {url} net: {e}")))?;

        let status = resp.status().as_u16();
        let bytes = resp
            .into_body()
            .into_with_config()
            .limit(4 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| SpotifyWebSyncError::FallbackToRest(format!("read body: {e}")))?;

        if status == 401 {
            return Err(SpotifyWebSyncError::AuthExpired);
        }
        if status == 403 && attempt == 0 {
            invalidate_cached_client_token();
            *client_token = fetch_client_token(agent)?;
            continue;
        }
        if !(200..300).contains(&status) {
            let snippet: String = String::from_utf8_lossy(&bytes).chars().take(160).collect();
            return Err(SpotifyWebSyncError::FallbackToRest(format!(
                "Spotify {method} HTTP {status}: {snippet}"
            )));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        return Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null));
    }

    Err(SpotifyWebSyncError::FallbackToRest(
        "Spotify request exhausted retries".to_string(),
    ))
}

fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                use std::fmt::Write as _;
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

/// Resolve a Spotify track URI (`spotify:track:<id>`) for a non-Spotify song by title + artist.
pub fn search_spotify_track_uri(
    access_token: &str,
    title: &str,
    artist: &str,
) -> Result<Option<String>, SpotifyWebSyncError> {
    let q_raw = if artist.trim().is_empty() {
        title.trim().to_string()
    } else {
        format!("{} {}", title.trim(), artist.trim())
    };
    if q_raw.is_empty() {
        return Ok(None);
    }
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let url = format!(
        "https://api.spotify.com/v1/search?q={}&type=track&limit=1",
        percent_encode_query(&q_raw)
    );
    let val = spotify_rest_json(&agent, "GET", &url, access_token, &mut client_token, None)?;
    let uri = val
        .pointer("/tracks/items/0/uri")
        .and_then(Value::as_str)
        .and_then(clean_spotify_track_uri);
    Ok(uri)
}

/// Create a new private playlist on the signed-in Spotify account and populate any track URIs.
/// Returns the canonical Ferric playlist ID (`spotify:playlist:<id>`).
pub fn create_spotify_playlist(
    access_token: &str,
    name: &str,
    description: &str,
    track_uris: &[String],
) -> Result<String, SpotifyWebSyncError> {
    let clean_name = name.trim();
    if clean_name.is_empty() {
        return Err(SpotifyWebSyncError::FallbackToRest(
            "Playlist name cannot be empty".to_string(),
        ));
    }
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let body = json!({
        "name": clean_name,
        "description": description.trim(),
        "public": false
    });

    let created = match spotify_rest_json(
        &agent,
        "POST",
        "https://api.spotify.com/v1/me/playlists",
        access_token,
        &mut client_token,
        Some(&body),
    ) {
        Ok(v) => v,
        Err(SpotifyWebSyncError::AuthExpired) => return Err(SpotifyWebSyncError::AuthExpired),
        Err(_) => {
            let me = spotify_rest_json(
                &agent,
                "GET",
                "https://api.spotify.com/v1/me",
                access_token,
                &mut client_token,
                None,
            )?;
            let uid = me
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    SpotifyWebSyncError::FallbackToRest("Spotify user id missing".to_string())
                })?;
            let fallback_url = format!(
                "https://api.spotify.com/v1/users/{}/playlists",
                percent_encode_query(uid)
            );
            spotify_rest_json(
                &agent,
                "POST",
                &fallback_url,
                access_token,
                &mut client_token,
                Some(&body),
            )?
        }
    };

    let pid = created
        .get("id")
        .and_then(Value::as_str)
        .and_then(clean_spotify_playlist_id)
        .ok_or_else(|| {
            SpotifyWebSyncError::FallbackToRest("Spotify created playlist ID missing".to_string())
        })?
        .to_string();

    let clean_uris: Vec<String> = track_uris
        .iter()
        .filter_map(|u| clean_spotify_track_uri(u))
        .collect();
    for chunk in clean_uris.chunks(100) {
        let add_url = format!("https://api.spotify.com/v1/playlists/{pid}/tracks");
        let add_body = json!({ "uris": chunk });
        let _ = spotify_rest_json(
            &agent,
            "POST",
            &add_url,
            access_token,
            &mut client_token,
            Some(&add_body),
        )?;
    }

    Ok(format!("spotify:playlist:{pid}"))
}

/// Append validated Spotify track URIs to an existing playlist on the user's Spotify account.
pub fn add_tracks_to_spotify_playlist(
    access_token: &str,
    playlist_id: &str,
    track_uris: &[String],
) -> Result<(), SpotifyWebSyncError> {
    let pid = clean_spotify_playlist_id(playlist_id).ok_or_else(|| {
        SpotifyWebSyncError::FallbackToRest("Invalid Spotify playlist ID".to_string())
    })?;
    let clean_uris: Vec<String> = track_uris
        .iter()
        .filter_map(|u| clean_spotify_track_uri(u))
        .collect();
    if clean_uris.is_empty() {
        return Ok(());
    }
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let url = format!("https://api.spotify.com/v1/playlists/{pid}/tracks");
    for chunk in clean_uris.chunks(100) {
        let body = json!({ "uris": chunk });
        let _ = spotify_rest_json(
            &agent,
            "POST",
            &url,
            access_token,
            &mut client_token,
            Some(&body),
        )?;
    }
    Ok(())
}

/// Remove one or more Spotify track URIs from an existing playlist on the user's Spotify account.
pub fn remove_tracks_from_spotify_playlist(
    access_token: &str,
    playlist_id: &str,
    track_uris: &[String],
) -> Result<(), SpotifyWebSyncError> {
    let pid = clean_spotify_playlist_id(playlist_id).ok_or_else(|| {
        SpotifyWebSyncError::FallbackToRest("Invalid Spotify playlist ID".to_string())
    })?;
    let items: Vec<Value> = track_uris
        .iter()
        .filter_map(|u| clean_spotify_track_uri(u))
        .map(|uri| json!({ "uri": uri }))
        .collect();
    if items.is_empty() {
        return Ok(());
    }
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let url = format!("https://api.spotify.com/v1/playlists/{pid}/tracks");
    for chunk in items.chunks(100) {
        let body = json!({ "tracks": chunk });
        let _ = spotify_rest_json(
            &agent,
            "DELETE",
            &url,
            access_token,
            &mut client_token,
            Some(&body),
        )?;
    }
    Ok(())
}

/// Replace all tracks in a Spotify playlist (used when deduplicating a synced Spotify playlist).
pub fn replace_spotify_playlist_tracks(
    access_token: &str,
    playlist_id: &str,
    track_uris: &[String],
) -> Result<(), SpotifyWebSyncError> {
    let pid = clean_spotify_playlist_id(playlist_id).ok_or_else(|| {
        SpotifyWebSyncError::FallbackToRest("Invalid Spotify playlist ID".to_string())
    })?;
    let clean_uris: Vec<String> = track_uris
        .iter()
        .filter_map(|u| clean_spotify_track_uri(u))
        .collect();
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let url = format!("https://api.spotify.com/v1/playlists/{pid}/tracks");
    let first = &clean_uris[..clean_uris.len().min(100)];
    let _ = spotify_rest_json(
        &agent,
        "PUT",
        &url,
        access_token,
        &mut client_token,
        Some(&json!({ "uris": first })),
    )?;
    if clean_uris.len() > 100 {
        for chunk in clean_uris[100..].chunks(100) {
            let _ = spotify_rest_json(
                &agent,
                "POST",
                &url,
                access_token,
                &mut client_token,
                Some(&json!({ "uris": chunk })),
            )?;
        }
    }
    Ok(())
}

/// Rename a playlist on the user's Spotify account.
pub fn rename_spotify_playlist(
    access_token: &str,
    playlist_id: &str,
    new_name: &str,
) -> Result<(), SpotifyWebSyncError> {
    let pid = clean_spotify_playlist_id(playlist_id).ok_or_else(|| {
        SpotifyWebSyncError::FallbackToRest("Invalid Spotify playlist ID".to_string())
    })?;
    let clean_name = new_name.trim();
    if clean_name.is_empty() {
        return Ok(());
    }
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let url = format!("https://api.spotify.com/v1/playlists/{pid}");
    let _ = spotify_rest_json(
        &agent,
        "PUT",
        &url,
        access_token,
        &mut client_token,
        Some(&json!({ "name": clean_name })),
    )?;
    Ok(())
}

/// Unfollow / remove a playlist from the signed-in user's Spotify library.
pub fn unfollow_spotify_playlist(
    access_token: &str,
    playlist_id: &str,
) -> Result<(), SpotifyWebSyncError> {
    let pid = clean_spotify_playlist_id(playlist_id).ok_or_else(|| {
        SpotifyWebSyncError::FallbackToRest("Invalid Spotify playlist ID".to_string())
    })?;
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let url = format!("https://api.spotify.com/v1/playlists/{pid}/followers");
    let _ = spotify_rest_json(
        &agent,
        "DELETE",
        &url,
        access_token,
        &mut client_token,
        None,
    )?;
    Ok(())
}

/// Save or remove a track from the user's Spotify Liked Songs.
pub fn set_spotify_track_liked(
    access_token: &str,
    spotify_id: &str,
    liked: bool,
) -> Result<(), SpotifyWebSyncError> {
    let Some(uri) = clean_spotify_track_uri(spotify_id) else {
        return Ok(());
    };
    let clean_id = uri.strip_prefix("spotify:track:").unwrap_or(&uri);
    let agent = pathfinder_agent();
    let mut client_token = fetch_client_token(&agent)?;
    let method = if liked { "PUT" } else { "DELETE" };
    let _ = spotify_rest_json(
        &agent,
        method,
        "https://api.spotify.com/v1/me/tracks",
        access_token,
        &mut client_token,
        Some(&json!({ "ids": [clean_id] })),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sp_dc_from_cookie_header() {
        let header = "sp_t=abc; sp_dc=AQBc123XYZ; sp_key=def";
        assert_eq!(extract_sp_dc(header).as_deref(), Some("AQBc123XYZ"));
        assert_eq!(
            clean_spotify_playlist_id("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M"),
            Some("37i9dQZF1DXcBWIGoYBM5M")
        );
        assert_eq!(clean_spotify_playlist_id("spotify:playlist:../bad"), None);
        assert_eq!(
            clean_spotify_track_uri("2plbrEY59IikOBgBGLjaoe").as_deref(),
            Some("spotify:track:2plbrEY59IikOBgBGLjaoe")
        );
        assert_eq!(
            clean_spotify_track_uri("spotify:track:2plbrEY59IikOBgBGLjaoe").as_deref(),
            Some("spotify:track:2plbrEY59IikOBgBGLjaoe")
        );
        assert_eq!(clean_spotify_track_uri("bad/uri"), None);
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

    #[test]
    fn classifies_spotify_premium_tier_fail_closed() {
        assert_eq!(
            classify_spotify_premium(Some("premium"), Some("premium")),
            SpotifyTierVerdict::Premium
        );
        assert_eq!(
            classify_spotify_premium(Some("PREMIUM"), None),
            SpotifyTierVerdict::Premium
        );
        assert_eq!(
            classify_spotify_premium(None, Some("premium")),
            SpotifyTierVerdict::Premium
        );
        assert_eq!(
            classify_spotify_premium(Some("free"), Some("free")),
            SpotifyTierVerdict::NotPremium("free".to_string())
        );
        assert_eq!(
            classify_spotify_premium(Some("open"), None),
            SpotifyTierVerdict::NotPremium("open".to_string())
        );
        assert_eq!(
            classify_spotify_premium(Some("premium"), Some("shuffle-restricted")),
            SpotifyTierVerdict::NotPremium("shuffle-restricted".to_string())
        );
        assert_eq!(
            classify_spotify_premium(None, None),
            SpotifyTierVerdict::Unknown
        );
        assert_eq!(
            classify_spotify_premium(Some("   "), Some("")),
            SpotifyTierVerdict::Unknown
        );
    }

    #[test]
    fn parses_account_attributes_and_bundle_hashes() {
        let acct = json!({
            "data": {
                "me": {
                    "account": {
                        "product": "free",
                        "attributes": { "catalogue": "free", "ads": true }
                    }
                }
            }
        });
        let (prod, cat) = parse_account_attributes_tier(&acct);
        assert_eq!(prod.as_deref(), Some("free"));
        assert_eq!(cat.as_deref(), Some("free"));
        assert_eq!(
            classify_spotify_premium(prod.as_deref(), cat.as_deref()),
            SpotifyTierVerdict::NotPremium("free".to_string())
        );

        let html = r#"<script src="https://open.spotifycdn.com/cdn/build/web-player/web-player.641ce497.js"></script>"#;
        assert_eq!(
            extract_web_bundle_url_from_html(html).as_deref(),
            Some("https://open.spotifycdn.com/cdn/build/web-player/web-player.641ce497.js")
        );

        let js = r#"new aX.l("accountAttributes","query","be4ce359daee01463ddb8e77536220e108e3d28105e12199f244d7a5954e9d14",null)"#;
        assert_eq!(
            extract_operation_hash_from_bundle(js, "accountAttributes").as_deref(),
            Some("be4ce359daee01463ddb8e77536220e108e3d28105e12199f244d7a5954e9d14")
        );
    }

    #[test]
    fn parses_home_recommended_stations_and_strips_html() {
        let payload = json!({
            "data": {
                "home": {
                    "sectionContainer": {
                        "sections": {
                            "items": [
                                {
                                    "data": {
                                        "__typename": "HomeGenericSectionData",
                                        "title": { "transformedLabel": "Recommended Stations" }
                                    },
                                    "sectionItems": {
                                        "items": [
                                            {
                                                "uri": "spotify:playlist:37i9dQZF1E4v1234567890",
                                                "content": {
                                                    "data": {
                                                        "__typename": "Playlist",
                                                        "uri": "spotify:playlist:37i9dQZF1E4v1234567890",
                                                        "name": "JVKE Radio",
                                                        "description": "With <a href=\"spotify:artist:1\">d4vd</a>, MAX, ZVC and more",
                                                        "images": {
                                                            "items": [{
                                                                "sources": [
                                                                    { "height": 300, "url": "https://pickasso.spotifycdn.com/image/ab67c0de0000deef/jvke" }
                                                                ]
                                                            }]
                                                        }
                                                    }
                                                }
                                            },
                                            {
                                                "uri": "spotify:playlist:37i9dQZF1E8a9876543210",
                                                "content": {
                                                    "data": {
                                                        "__typename": "Playlist",
                                                        "uri": "spotify:playlist:37i9dQZF1E8a9876543210",
                                                        "name": "Kill Bill Radio",
                                                        "description": "With SZA, Ariana Grande, Kendrick Lamar and more",
                                                        "images": {
                                                            "items": [{
                                                                "sources": [
                                                                    { "height": 300, "url": "https://seed-mix-image.spotifycdn.com/v6/img/killbill" }
                                                                ]
                                                            }]
                                                        }
                                                    }
                                                }
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });
        let stations = parse_home_recommended_stations(&payload);
        assert_eq!(stations.len(), 2);
        assert_eq!(stations[0].playlist_id, "37i9dQZF1E4v1234567890");
        assert_eq!(stations[0].title, "JVKE Radio");
        assert_eq!(stations[0].display_name, "JVKE");
        assert_eq!(stations[0].subtitle, "With d4vd, MAX, ZVC and more");
        assert_eq!(
            stations[0].thumbnail_url.as_deref(),
            Some("https://pickasso.spotifycdn.com/image/ab67c0de0000deef/jvke")
        );
        assert_eq!(stations[1].display_name, "Kill Bill");
    }
}
