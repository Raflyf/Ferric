use crate::yt_cleaner::clean_youtube_metadata;
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_OAUTH_CALLBACK_PORT: u16 = 43824;
pub const DEFAULT_OAUTH_CALLBACK_PATH: &str = "/callback";

pub const SPOTIFY_DESKTOP_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
pub const SPOTIFY_DESKTOP_CALLBACK_PORT: u16 = 8989;
pub const SPOTIFY_DESKTOP_CALLBACK_PATH: &str = "/login";
pub const SPOTIFY_DESKTOP_REDIRECT_URI: &str = "http://127.0.0.1:8989/login";
pub const SPOTIFY_DESKTOP_SCOPES: &str = "playlist-read-collaborative playlist-read-private streaming user-follow-read user-library-read user-read-email user-read-playback-state user-read-private user-read-recently-played user-top-read";

pub const SPOTIFY_OAUTH_SCOPES: &str =
    "user-read-private playlist-read-private playlist-read-collaborative user-library-read";
pub const GOOGLE_YOUTUBE_READONLY_SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";

static ACTIVE_OAUTH_CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

pub fn effective_spotify_client_id(custom_client_id: &str) -> &str {
    let trimmed = custom_client_id.trim();
    if trimmed.is_empty() {
        SPOTIFY_DESKTOP_CLIENT_ID
    } else {
        trimmed
    }
}

pub fn is_builtin_spotify_desktop_client(client_id: &str) -> bool {
    let trimmed = client_id.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case(SPOTIFY_DESKTOP_CLIENT_ID)
}

pub fn spotify_oauth_endpoint_config(custom_client_id: &str) -> (&str, u16, &'static str) {
    if is_builtin_spotify_desktop_client(custom_client_id) {
        (
            SPOTIFY_DESKTOP_CLIENT_ID,
            SPOTIFY_DESKTOP_CALLBACK_PORT,
            SPOTIFY_DESKTOP_CALLBACK_PATH,
        )
    } else {
        (
            custom_client_id.trim(),
            DEFAULT_OAUTH_CALLBACK_PORT,
            DEFAULT_OAUTH_CALLBACK_PATH,
        )
    }
}

/// Cancel any previously active OAuth loopback listener and return a fresh token for the new attempt.
pub fn activate_oauth_cancel_token() -> Arc<AtomicBool> {
    let next = Arc::new(AtomicBool::new(false));
    let mut guard = ACTIVE_OAUTH_CANCEL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(prev) = guard.replace(Arc::clone(&next)) {
        prev.store(true, Ordering::SeqCst);
    }
    next
}

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const BASE64URL_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Adaptive pacing state shared across Spotify Web API calls so library sync stays well inside
/// Spotify's rolling rate-limit window (especially for web-player tokens that share limits with
/// the player itself).
struct SpotifyPacerState {
    last_request: Option<Instant>,
    cooldown_until: Option<Instant>,
    min_gap_ms: u64,
}

impl SpotifyPacerState {
    const fn new() -> Self {
        Self {
            last_request: None,
            cooldown_until: None,
            min_gap_ms: 450,
        }
    }
}

static SPOTIFY_PACER: std::sync::Mutex<SpotifyPacerState> =
    std::sync::Mutex::new(SpotifyPacerState::new());

fn pace_spotify_api(web_session: bool) {
    let floor_ms = if web_session { 700 } else { 450 };
    let sleep_dur = {
        let mut state = SPOTIFY_PACER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let gap = Duration::from_millis(state.min_gap_ms.max(floor_ms));
        let mut target = now;
        if let Some(prev) = state.last_request {
            let next_allowed = prev + gap;
            if next_allowed > target {
                target = next_allowed;
            }
        }
        if let Some(cd) = state.cooldown_until
            && cd > target
        {
            target = cd;
        }
        state.last_request = Some(target);
        target.saturating_duration_since(now)
    };
    if !sleep_dur.is_zero() {
        std::thread::sleep(sleep_dur);
    }
}

fn note_spotify_rate_limit(wait_secs: u64) {
    let mut state = SPOTIFY_PACER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    state.min_gap_ms = state.min_gap_ms.saturating_add(350).min(2_000);
    let until = Instant::now() + Duration::from_secs(wait_secs);
    state.cooldown_until = Some(match state.cooldown_until {
        Some(prev) if prev > until => prev,
        _ => until,
    });
}

fn note_spotify_success(web_session: bool) {
    let floor_ms = if web_session { 700 } else { 450 };
    let mut state = SPOTIFY_PACER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state.min_gap_ms > floor_ms {
        state.min_gap_ms = state.min_gap_ms.saturating_sub(50).max(floor_ms);
    }
}

fn parse_retry_after_secs(raw: Option<&str>) -> Option<u64> {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&s| s > 0)
}

#[derive(Clone, PartialEq, Eq)]
pub struct PkcePair {
    pub verifier: String,
    pub challenge_s256: String,
    pub state: String,
}

impl std::fmt::Debug for PkcePair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PkcePair")
            .field("verifier", &"[REDACTED]")
            .field("challenge_s256", &self.challenge_s256)
            .field("state", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in_secs: u64,
}

impl std::fmt::Debug for OAuthTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokens")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_in_secs", &self.expires_in_secs)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedPlaylistBundle {
    pub playlist: Playlist,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudSyncReport {
    pub display_name: String,
    pub liked_tracks: Vec<Track>,
    pub playlists: Vec<SyncedPlaylistBundle>,
    pub etag_skipped: usize,
    #[serde(default)]
    pub rate_limited_skipped: usize,
}

#[derive(Debug)]
pub struct LoopbackListener {
    listener: TcpListener,
    pub redirect_uri: String,
    expected_state: String,
    expected_path: String,
    bound_port: u16,
}

fn fill_entropy_bytes(out: &mut [u8]) {
    ferric_core::secrets::csprng_fill(out);
}

pub fn base64url_encode_nopad(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    let (chunks, rem) = bytes.as_chunks::<3>();
    for &[b0, b1, b2] in chunks {
        out.push(char::from(BASE64URL_ALPHABET[usize::from(b0 >> 2)]));
        out.push(char::from(
            BASE64URL_ALPHABET[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))],
        ));
        out.push(char::from(
            BASE64URL_ALPHABET[usize::from(((b1 & 0x0F) << 2) | (b2 >> 6))],
        ));
        out.push(char::from(BASE64URL_ALPHABET[usize::from(b2 & 0x3F)]));
    }

    if rem.len() == 1 {
        let b0 = rem[0];
        out.push(char::from(BASE64URL_ALPHABET[usize::from(b0 >> 2)]));
        out.push(char::from(
            BASE64URL_ALPHABET[usize::from((b0 & 0x03) << 4)],
        ));
    } else if rem.len() == 2 {
        let b0 = rem[0];
        let b1 = rem[1];
        out.push(char::from(BASE64URL_ALPHABET[usize::from(b0 >> 2)]));
        out.push(char::from(
            BASE64URL_ALPHABET[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))],
        ));
        out.push(char::from(
            BASE64URL_ALPHABET[usize::from((b1 & 0x0F) << 2)],
        ));
    }

    out
}

/// Compute RFC 7636 `S256` PKCE challenge (`BASE64URL-ENCODE(SHA256(ASCII(verifier)))` without padding).
pub fn pkce_s256_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64url_encode_nopad(&digest)
}

/// Generate a cryptographic 64-char URL-safe PKCE verifier, its `S256` challenge, and a 32-hex (128-bit) CSRF state.
pub fn generate_pkce_pair() -> PkcePair {
    let mut raw = [0u8; 80];
    fill_entropy_bytes(&mut raw);

    let mut verifier = String::with_capacity(64);
    for &b in &raw[..64] {
        verifier.push(char::from(BASE64URL_ALPHABET[usize::from(b & 0x3F)]));
    }

    let mut state = String::with_capacity(32);
    for &b in &raw[64..80] {
        state.push(char::from(HEX_LOWER[usize::from(b >> 4)]));
        state.push(char::from(HEX_LOWER[usize::from(b & 0x0F)]));
    }
    for b in &mut raw {
        // SAFETY: Zero stack entropy buffer after deriving PKCE material.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(Ordering::SeqCst);

    let challenge_s256 = pkce_s256_challenge(&verifier);
    PkcePair {
        verifier,
        challenge_s256,
        state,
    }
}

/// RFC 3986 percent-encoding for URL query values and form fields.
pub fn url_encode(input: &str) -> String {
    const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len() + 8);
    for &b in input.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX_UPPER[usize::from(b >> 4)]));
            out.push(char::from(HEX_UPPER[usize::from(b & 0x0F)]));
        }
    }
    out
}

pub fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                decoded.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    decoded.push(((h << 4) | l) as u8);
                    i += 3;
                } else {
                    decoded.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                decoded.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Parse URL query string (`?a=1&b=2` or `a=1&b=2`) into percent-decoded key/value pairs.
pub fn parse_query_params(query: &str) -> HashMap<String, String> {
    let trimmed = query.strip_prefix('?').unwrap_or(query);
    let mut map = HashMap::new();
    for pair in trimmed.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(url_decode(k), url_decode(v));
    }
    map
}

/// Bind a loopback HTTP listener on `127.0.0.1:{preferred_port}` first, falling back to `127.0.0.1:0`
/// only when a non-fixed port is requested.
pub fn bind_loopback_callback(
    preferred_port: u16,
    path: &str,
    expected_state: &str,
) -> ProviderResult<LoopbackListener> {
    let normalized_path = if path.is_empty() {
        DEFAULT_OAUTH_CALLBACK_PATH.to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let listener = if preferred_port == 0 {
        TcpListener::bind(("127.0.0.1", 0))?
    } else {
        let mut bound = None;
        let mut last_err = None;
        for attempt in 0..15 {
            match TcpListener::bind(("127.0.0.1", preferred_port)) {
                Ok(sock) => {
                    bound = Some(sock);
                    break;
                }
                Err(err) => {
                    last_err = Some(err);
                    if attempt + 1 < 15 {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            }
        }
        match bound {
            Some(sock) => sock,
            None => {
                let err = last_err
                    .unwrap_or_else(|| std::io::Error::other("could not bind loopback port"));
                if preferred_port == SPOTIFY_DESKTOP_CALLBACK_PORT {
                    return Err(ProviderError::Http(format!(
                        "Could not bind http://127.0.0.1:{preferred_port}{normalized_path}: {err}"
                    )));
                }
                TcpListener::bind(("127.0.0.1", 0))?
            }
        }
    };
    listener.set_nonblocking(true)?;

    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{normalized_path}");
    Ok(LoopbackListener {
        listener,
        redirect_uri,
        expected_state: expected_state.to_string(),
        expected_path: normalized_path,
        bound_port: port,
    })
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

fn build_callback_html(title: &str, message: &str, is_error: bool) -> String {
    let safe_title = escape_html(title);
    let safe_message = escape_html(message);
    let status_label = if is_error {
        "AUTHORIZATION ERROR"
    } else {
        "CONNECTED"
    };
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<title>{safe_title}</title>\
<style>\
body{{margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
background:#000000;color:#F3EEE3;font-family:system-ui,-apple-system,Segoe UI,sans-serif;}}\
.card{{max-width:440px;padding:32px;border:1px solid rgba(243,238,227,0.16);border-radius:14px;\
background:#0A0A0A;box-shadow:0 16px 48px rgba(0,0,0,0.65);}}\
.badge{{display:inline-block;font-size:11px;font-weight:700;letter-spacing:0.12em;\
color:#D9541E;margin-bottom:12px;}}\
h1{{margin:0 0 10px 0;font-size:20px;font-weight:600;color:#F3EEE3;}}\
p{{margin:0;font-size:14px;line-height:1.55;color:rgba(243,238,227,0.78);}}\
</style></head><body><div class=\"card\">\
<div class=\"badge\">{status_label}</div>\
<h1>{safe_title}</h1><p>{safe_message}</p></div></body></html>"
    )
}

fn write_http_html(stream: &mut std::net::TcpStream, status_line: &str, html: &str) {
    let response = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn ct_eq_str(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

impl LoopbackListener {
    /// Poll non-blocking `accept()` every 80ms up to `timeout` and extract verified OAuth authorization `code`.
    pub fn wait_for_auth_code(
        self,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> ProviderResult<String> {
        let deadline = Instant::now() + timeout;

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Http(
                    "OAuth authorization cancelled".to_string(),
                ));
            }
            if Instant::now() >= deadline {
                return Err(ProviderError::Http(
                    "Timed out waiting for OAuth callback".to_string(),
                ));
            }

            let (mut stream, _) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(80));
                    continue;
                }
                Err(err) => return Err(ProviderError::Io(err)),
            };

            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));

            let mut buf = [0u8; 8192];
            let mut total_read = 0usize;
            while total_read < buf.len() {
                match stream.read(&mut buf[total_read..]) {
                    Ok(0) => break,
                    Ok(n) => {
                        total_read += n;
                        if buf[..total_read].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            if total_read == 0 {
                continue;
            }

            let raw_req = String::from_utf8_lossy(&buf[..total_read]);
            let Some(request_line) = raw_req.lines().next() else {
                continue;
            };
            let mut parts = request_line.split_whitespace();
            let Some(method) = parts.next() else {
                continue;
            };
            let Some(target) = parts.next() else {
                continue;
            };

            if !method.eq_ignore_ascii_case("GET") {
                let html = build_callback_html(
                    "Unsupported Request",
                    "Ferric OAuth callback expects an HTTP GET redirect.",
                    true,
                );
                write_http_html(&mut stream, "405 Method Not Allowed", &html);
                continue;
            }

            let (req_path, query_str) = target.split_once('?').unwrap_or((target, ""));
            if req_path.eq_ignore_ascii_case("/favicon.ico") {
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                let _ = stream.flush();
                continue;
            }
            if req_path != self.expected_path {
                let html = build_callback_html(
                    "Invalid Callback Path",
                    "Unexpected OAuth callback path.",
                    true,
                );
                write_http_html(&mut stream, "404 Not Found", &html);
                continue;
            }
            let expected_host_ip = format!("127.0.0.1:{}", self.bound_port);
            let expected_host_lh = format!("localhost:{}", self.bound_port);
            let host_headers: Vec<&str> = raw_req
                .lines()
                .skip(1)
                .take_while(|line| !line.is_empty())
                .filter_map(|line| {
                    let (k, v) = line.split_once(':')?;
                    k.trim().eq_ignore_ascii_case("host").then(|| v.trim())
                })
                .collect();
            let host_ok = host_headers.len() == 1
                && (host_headers[0].eq_ignore_ascii_case(&expected_host_ip)
                    || host_headers[0].eq_ignore_ascii_case(&expected_host_lh)
                    || host_headers[0].eq_ignore_ascii_case("127.0.0.1")
                    || host_headers[0].eq_ignore_ascii_case("localhost"));
            if !host_ok {
                let html = build_callback_html(
                    "Invalid Host Header",
                    "OAuth callback rejected due to unexpected Host header.",
                    true,
                );
                write_http_html(&mut stream, "400 Bad Request", &html);
                continue;
            }

            let params = parse_query_params(query_str);
            let state_matches = params
                .get("state")
                .is_some_and(|s| ct_eq_str(s, &self.expected_state));

            if let Some(oauth_err) = params.get("error") {
                if !state_matches {
                    let html = build_callback_html(
                        "Ferric Authorization Ignored",
                        "Unverified callback request ignored.",
                        true,
                    );
                    write_http_html(&mut stream, "400 Bad Request", &html);
                    continue;
                }
                let html = build_callback_html(
                    "Ferric Authorization Failed",
                    "Authorization was declined or interrupted. You can close this browser tab and return to Ferric.",
                    true,
                );
                write_http_html(&mut stream, "400 Bad Request", &html);
                return Err(ProviderError::Http(format!(
                    "OAuth authorization error: {oauth_err}"
                )));
            }

            if !state_matches {
                let html = build_callback_html(
                    "Ferric Authorization Rejected",
                    "Security state verification failed. Please retry connecting from Ferric.",
                    true,
                );
                write_http_html(&mut stream, "400 Bad Request", &html);
                return Err(ProviderError::Http(
                    "OAuth callback state mismatch".to_string(),
                ));
            }

            let Some(code) = params.get("code").filter(|c| !c.is_empty()).cloned() else {
                let html = build_callback_html(
                    "Ferric Authorization Incomplete",
                    "No authorization code was received in the callback.",
                    true,
                );
                write_http_html(&mut stream, "400 Bad Request", &html);
                return Err(ProviderError::Http(
                    "Missing authorization code in OAuth callback".to_string(),
                ));
            };

            let html = build_callback_html(
                "Ferric Authorization Complete",
                "Ferric Authorization Complete. You can close this browser tab and return to Ferric.",
                false,
            );
            write_http_html(&mut stream, "200 OK", &html);
            return Ok(code);
        }
    }
}

fn http_agent() -> ureq::Agent {
    crate::shared_http_agent().clone()
}

fn encode_form_pairs(pairs: &[(&str, &str)]) -> String {
    let mut body = String::new();
    for (idx, &(k, v)) in pairs.iter().enumerate() {
        if idx > 0 {
            body.push('&');
        }
        body.push_str(&url_encode(k));
        body.push('=');
        body.push_str(&url_encode(v));
    }
    body
}

#[derive(Debug, Deserialize)]
struct TokenEndpointResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<serde_json::Value>,
    error_description: Option<String>,
}

fn post_token_form(
    endpoint: &str,
    pairs: &[(&str, &str)],
    fallback_refresh: Option<&str>,
) -> ProviderResult<OAuthTokens> {
    let form_body = encode_form_pairs(pairs);
    let resp = http_agent()
        .post(endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .header("User-Agent", concat!("Ferric/", env!("CARGO_PKG_VERSION")))
        .send(form_body.as_bytes())
        .map_err(|e| ProviderError::Http(format!("Token request failed: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Could not read token response: {e}")))?;

    let parsed: TokenEndpointResponse = serde_json::from_slice(&bytes)
        .map_err(|e| ProviderError::Http(format!("Invalid token JSON (HTTP {status}): {e}")))?;

    if status != 200 {
        let detail = parsed
            .error_description
            .or_else(|| parsed.error.map(|v| v.to_string()))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(ProviderError::Http(format!(
            "OAuth token exchange failed: {detail}"
        )));
    }

    let access_token = parsed
        .access_token
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProviderError::Http("OAuth response missing access_token".to_string()))?;

    Ok(OAuthTokens {
        access_token,
        refresh_token: parsed
            .refresh_token
            .filter(|s| !s.is_empty())
            .or_else(|| fallback_refresh.map(ToOwned::to_owned)),
        expires_in_secs: parsed.expires_in.unwrap_or(3600),
    })
}

fn get_bearer_json<T: for<'de> Deserialize<'de>>(
    agent: &ureq::Agent,
    url: &str,
    access_token: &str,
) -> ProviderResult<T> {
    let resp = agent
        .get(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", concat!("Ferric/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| ProviderError::Http(format!("API request failed: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Could not read API body: {e}")))?;

    if status != 200 {
        let snippet = String::from_utf8_lossy(&bytes);
        let short: String = snippet.chars().take(180).collect();
        return Err(ProviderError::Http(format!(
            "API returned HTTP {status}: {short}"
        )));
    }

    serde_json::from_slice(&bytes)
        .map_err(|e| ProviderError::Http(format!("Failed to decode API JSON: {e}")))
}

enum SpotifyFetchError {
    RateLimited { retry_after_secs: Option<u64> },
    Other(ProviderError),
}

impl SpotifyFetchError {
    fn into_provider_error(self) -> ProviderError {
        match self {
            Self::RateLimited {
                retry_after_secs: Some(secs),
            } => ProviderError::Http(format!(
                "Spotify is rate-limiting this session (HTTP 429, Retry-After {secs}s). Wait ~{}s, then press Sync again.",
                secs.max(10)
            )),
            Self::RateLimited {
                retry_after_secs: None,
            } => ProviderError::Http(
                "Spotify is rate-limiting this session (HTTP 429). Wait a minute, then press Sync again."
                    .to_string(),
            ),
            Self::Other(err) => err,
        }
    }
}

fn get_spotify_bearer_json<T: for<'de> Deserialize<'de>>(
    agent: &ureq::Agent,
    url: &str,
    access_token: &str,
    web_session: bool,
) -> Result<T, SpotifyFetchError> {
    const MAX_ATTEMPTS: usize = 4;
    const MAX_INLINE_WAIT_SECS: u64 = 35;

    for attempt in 0..MAX_ATTEMPTS {
        pace_spotify_api(web_session);
        let resp = agent
            .get(url)
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "application/json")
            .header("User-Agent", concat!("Ferric/", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|e| {
                SpotifyFetchError::Other(ProviderError::Http(format!("API request failed: {e}")))
            })?;

        let status = resp.status().as_u16();
        let retry_after = parse_retry_after_secs(
            resp.headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
        );
        let bytes = resp
            .into_body()
            .into_with_config()
            .limit(MAX_JSON_BYTES)
            .read_to_vec()
            .map_err(|e| {
                SpotifyFetchError::Other(ProviderError::Http(format!(
                    "Could not read API body: {e}"
                )))
            })?;

        if status == 429 {
            // If Spotify asks for a long cooldown (>35s), do not stall the worker silently —
            // record cooldown, preserve partial sync progress, and tell the user clearly.
            if let Some(secs) = retry_after
                && secs > MAX_INLINE_WAIT_SECS
            {
                note_spotify_rate_limit(secs.min(120));
                return Err(SpotifyFetchError::RateLimited {
                    retry_after_secs: Some(secs),
                });
            }
            let wait_secs = retry_after
                .map(|s| s.saturating_add(1))
                .unwrap_or_else(|| match attempt {
                    0 => 4,
                    1 => 8,
                    _ => 15,
                })
                .clamp(2, MAX_INLINE_WAIT_SECS);
            note_spotify_rate_limit(wait_secs);
            if attempt + 1 == MAX_ATTEMPTS {
                return Err(SpotifyFetchError::RateLimited {
                    retry_after_secs: retry_after.or(Some(wait_secs)),
                });
            }
            continue;
        }

        if status != 200 {
            let snippet = String::from_utf8_lossy(&bytes);
            let short: String = snippet.chars().take(180).collect();
            return Err(SpotifyFetchError::Other(ProviderError::Http(format!(
                "API returned HTTP {status}: {short}"
            ))));
        }

        note_spotify_success(web_session);
        return serde_json::from_slice(&bytes).map_err(|e| {
            SpotifyFetchError::Other(ProviderError::Http(format!(
                "Failed to decode API JSON: {e}"
            )))
        });
    }
    Err(SpotifyFetchError::RateLimited {
        retry_after_secs: None,
    })
}

// ============================================================================
// Spotify OAuth 2.0 PKCE + Web API v1 Catalog Sync
// ============================================================================

pub fn spotify_authorize_url(client_id: &str, redirect_uri: &str, pkce: &PkcePair) -> String {
    let effective_id = effective_spotify_client_id(client_id);
    let scopes = if is_builtin_spotify_desktop_client(client_id) {
        SPOTIFY_DESKTOP_SCOPES
    } else {
        SPOTIFY_OAUTH_SCOPES
    };
    format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&state={}&scope={}&code_challenge_method=S256&code_challenge={}&show_dialog=true",
        url_encode(effective_id),
        url_encode(redirect_uri),
        url_encode(&pkce.state),
        url_encode(scopes),
        url_encode(&pkce.challenge_s256),
    )
}

pub fn spotify_exchange_code(
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> ProviderResult<OAuthTokens> {
    let effective_id = effective_spotify_client_id(client_id);
    post_token_form(
        "https://accounts.spotify.com/api/token",
        &[
            ("grant_type", "authorization_code"),
            ("client_id", effective_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ],
        None,
    )
}

pub fn spotify_refresh_token(client_id: &str, refresh_token: &str) -> ProviderResult<OAuthTokens> {
    let effective_id = effective_spotify_client_id(client_id);
    post_token_form(
        "https://accounts.spotify.com/api/token",
        &[
            ("grant_type", "refresh_token"),
            ("client_id", effective_id),
            ("refresh_token", refresh_token),
        ],
        Some(refresh_token),
    )
}

#[derive(Debug, Deserialize)]
struct SpotifyProfile {
    id: String,
    display_name: Option<String>,
    product: Option<String>,
}

pub(crate) fn fetch_spotify_profile_tier(
    access_token: &str,
) -> ProviderResult<(String, Option<String>)> {
    let agent = http_agent();
    let profile: SpotifyProfile =
        get_bearer_json(&agent, "https://api.spotify.com/v1/me", access_token)?;
    let display_name = profile
        .display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(profile.id);
    Ok((display_name, profile.product))
}

#[derive(Debug, Deserialize)]
struct SpotifyArtist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbum {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyExternalIds {
    isrc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrackObject {
    id: Option<String>,
    name: Option<String>,
    duration_ms: Option<u64>,
    #[serde(default)]
    artists: Vec<SpotifyArtist>,
    album: Option<SpotifyAlbum>,
    external_ids: Option<SpotifyExternalIds>,
}

#[derive(Debug, Deserialize)]
struct SpotifySavedTrackItem {
    track: Option<SpotifyTrackObject>,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct SpotifyPaging<T> {
    #[serde(default)]
    items: Vec<T>,
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaylistSummary {
    id: String,
    name: String,
    description: Option<String>,
    snapshot_id: Option<String>,
}

fn map_spotify_track(raw: SpotifyTrackObject, liked: bool) -> Option<Track> {
    let spotify_id = raw.id?.trim().to_string();
    if spotify_id.is_empty() {
        return None;
    }
    let title = raw
        .name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Untitled Track".to_string());

    let mut artist = String::new();
    for a in raw.artists {
        let name = a.name.trim();
        if name.is_empty() {
            continue;
        }
        if !artist.is_empty() {
            artist.push_str(", ");
        }
        artist.push_str(name);
    }
    if artist.is_empty() {
        artist.push_str("Unknown Artist");
    }

    let album = raw
        .album
        .map(|a| a.name.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Spotify".to_string());

    let isrc = raw
        .external_ids
        .and_then(|ext| ext.isrc)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some(Track {
        id: format!("spotify:{spotify_id}"),
        title,
        artist,
        album,
        duration_ms: raw.duration_ms.unwrap_or(0),
        path: None,
        source: TrackSource::Spotify,
        codec: "OGG".to_string(),
        sample_rate: 44_100,
        bit_depth: 16,
        bitrate_kbps: 320,
        isrc,
        spotify_id: Some(spotify_id),
        youtube_video_id: None,
        thumbnail_url: None,
        replay_gain_db: 0,
        liked,
    })
}

/// Synchronize user profile, up to 200 liked songs, and up to 30 playlists from Spotify Web API v1.
pub fn sync_spotify_catalog(access_token: &str) -> ProviderResult<CloudSyncReport> {
    sync_spotify_catalog_with_snapshots(access_token, &HashMap::new(), false)
}

#[cfg(test)]
fn should_skip_spotify_playlist(
    playlist_id: &str,
    snapshot_id: Option<&str>,
    existing_snapshots: &HashMap<String, String>,
) -> bool {
    should_skip_spotify_playlist_with_prefix(
        playlist_id,
        snapshot_id,
        existing_snapshots,
        "spotify:playlist:",
    )
}

fn should_skip_spotify_playlist_with_prefix(
    playlist_id: &str,
    snapshot_id: Option<&str>,
    existing_snapshots: &HashMap<String, String>,
    playlist_id_prefix: &str,
) -> bool {
    let Some(remote_snap) = snapshot_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let key = format!("{playlist_id_prefix}{playlist_id}");
    existing_snapshots
        .get(&key)
        .is_some_and(|local_snap| local_snap.trim() == remote_snap)
}

/// Synchronize Spotify profile, Liked Songs, and playlists while skipping playlists whose
/// `snapshot_id` matches the local database and preserving partial progress if Spotify rate-limits
/// mid-sync.
pub fn sync_spotify_catalog_with_snapshots(
    access_token: &str,
    existing_snapshots: &HashMap<String, String>,
    web_session: bool,
) -> ProviderResult<CloudSyncReport> {
    let playlist_id_prefix = if web_session {
        "spotify:playlist:"
    } else {
        "spotify:premium:playlist:"
    };
    // Prefer Spotify's Pathfinder GraphQL endpoint for both Direct OAuth and Web Sign-In
    // (same transport used by desktop clients like Sonora) to avoid shared `api.spotify.com/v1`
    // HTTP 429 buckets and fetch real cover art URLs directly.
    match crate::spotify_web::sync_spotify_web_catalog_with_prefix(
        access_token,
        existing_snapshots,
        playlist_id_prefix,
    ) {
        Ok(report) => return Ok(report),
        Err(crate::spotify_web::SpotifyWebSyncError::AuthExpired) => {
            return Err(ProviderError::Http(if web_session {
                "Spotify web session expired (1h token). Click 'Sign in via Web' to refresh."
                    .to_string()
            } else {
                "Spotify OAuth session expired (HTTP 401). Click 'Sign in with Spotify' to refresh."
                    .to_string()
            }));
        }
        Err(crate::spotify_web::SpotifyWebSyncError::FallbackToRest(_)) => {
            // Fall through to the paced Web API v1 path if Pathfinder is temporarily unavailable.
        }
    }

    let agent = http_agent();
    let profile: SpotifyProfile = get_spotify_bearer_json(
        &agent,
        "https://api.spotify.com/v1/me",
        access_token,
        web_session,
    )
    .map_err(SpotifyFetchError::into_provider_error)?;
    let display_name = profile
        .display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(profile.id);

    let mut liked_tracks = Vec::new();
    let mut liked_ids = HashSet::new();
    let mut etag_skipped = 0usize;
    let mut rate_limited_skipped = 0usize;
    let mut hit_rate_limit = false;

    for page in 0..4_usize {
        let offset = page * 50;
        let url = format!("https://api.spotify.com/v1/me/tracks?limit=50&offset={offset}");
        let batch: SpotifyPaging<SpotifySavedTrackItem> =
            match get_spotify_bearer_json(&agent, &url, access_token, web_session) {
                Ok(b) => b,
                Err(SpotifyFetchError::RateLimited { .. }) => {
                    hit_rate_limit = true;
                    rate_limited_skipped += 1;
                    break;
                }
                Err(other) => return Err(other.into_provider_error()),
            };
        if batch.items.is_empty() {
            break;
        }
        for item in batch.items {
            if let Some(raw_track) = item.track
                && let Some(track) = map_spotify_track(raw_track, true)
            {
                if let Some(sid) = &track.spotify_id {
                    liked_ids.insert(sid.clone());
                }
                liked_tracks.push(track);
            }
        }
        if batch.next.is_none() || liked_tracks.len() >= 200 {
            break;
        }
    }

    let mut playlists = Vec::new();
    if !hit_rate_limit {
        let playlists_page: Option<SpotifyPaging<SpotifyPlaylistSummary>> =
            match get_spotify_bearer_json(
                &agent,
                "https://api.spotify.com/v1/me/playlists?limit=30",
                access_token,
                web_session,
            ) {
                Ok(p) => Some(p),
                Err(SpotifyFetchError::RateLimited { .. }) => {
                    rate_limited_skipped += 1;
                    None
                }
                Err(other) => return Err(other.into_provider_error()),
            };

        if let Some(playlists_page) = playlists_page {
            let total_items = playlists_page.items.len();
            playlists.reserve(total_items);
            for (idx, pl) in playlists_page.items.into_iter().enumerate() {
                if pl.id.trim().is_empty() {
                    continue;
                }
                if should_skip_spotify_playlist_with_prefix(
                    &pl.id,
                    pl.snapshot_id.as_deref(),
                    existing_snapshots,
                    playlist_id_prefix,
                ) {
                    etag_skipped += 1;
                    continue;
                }
                let tracks_url = format!(
                    "https://api.spotify.com/v1/playlists/{}/tracks?limit=100&fields=items(track(id,name,duration_ms,artists(name),album(name),external_ids(isrc))),next",
                    url_encode(&pl.id)
                );
                let tracks_page: SpotifyPaging<SpotifySavedTrackItem> =
                    match get_spotify_bearer_json(&agent, &tracks_url, access_token, web_session) {
                        Ok(p) => p,
                        Err(SpotifyFetchError::RateLimited { .. }) => {
                            rate_limited_skipped += total_items.saturating_sub(idx);
                            break;
                        }
                        Err(other) => return Err(other.into_provider_error()),
                    };

                let mut bundle_tracks = Vec::with_capacity(tracks_page.items.len());
                for item in tracks_page.items {
                    if let Some(raw_track) = item.track {
                        let is_liked = raw_track
                            .id
                            .as_deref()
                            .is_some_and(|sid| liked_ids.contains(sid));
                        if let Some(track) = map_spotify_track(raw_track, is_liked) {
                            bundle_tracks.push(track);
                        }
                    }
                }

                let playlist = Playlist {
                    id: format!("{playlist_id_prefix}{}", pl.id),
                    name: if pl.name.trim().is_empty() {
                        "Spotify Playlist".to_string()
                    } else {
                        pl.name.trim().to_string()
                    },
                    source: TrackSource::Spotify,
                    etag: pl.snapshot_id,
                    track_count: bundle_tracks.len(),
                    description: pl
                        .description
                        .map(|d| d.trim().to_string())
                        .unwrap_or_default(),
                    thumbnail_url: None,
                };
                playlists.push(SyncedPlaylistBundle {
                    playlist,
                    tracks: bundle_tracks,
                });
            }
        }
    }

    Ok(CloudSyncReport {
        display_name,
        liked_tracks,
        playlists,
        etag_skipped,
        rate_limited_skipped,
    })
}

// ============================================================================
// Google / YouTube & YouTube Music Official OAuth 2.0 PKCE + ETag Sync
// ============================================================================

pub fn google_authorize_url(client_id: &str, redirect_uri: &str, pkce: &PkcePair) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        url_encode(client_id.trim()),
        url_encode(redirect_uri),
        url_encode(GOOGLE_YOUTUBE_READONLY_SCOPE),
        url_encode(&pkce.state),
        url_encode(&pkce.challenge_s256),
    )
}

pub fn google_exchange_code(
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> ProviderResult<OAuthTokens> {
    let trimmed_id = client_id.trim();
    if let Some(secret) = client_secret.map(str::trim).filter(|s| !s.is_empty()) {
        post_token_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", trimmed_id),
                ("client_secret", secret),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
            ],
            None,
        )
    } else {
        post_token_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "authorization_code"),
                ("client_id", trimmed_id),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
            ],
            None,
        )
    }
}

pub fn google_refresh_token(
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
) -> ProviderResult<OAuthTokens> {
    let trimmed_id = client_id.trim();
    if let Some(secret) = client_secret.map(str::trim).filter(|s| !s.is_empty()) {
        post_token_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", trimmed_id),
                ("client_secret", secret),
                ("refresh_token", refresh_token),
            ],
            Some(refresh_token),
        )
    } else {
        post_token_form(
            "https://oauth2.googleapis.com/token",
            &[
                ("grant_type", "refresh_token"),
                ("client_id", trimmed_id),
                ("refresh_token", refresh_token),
            ],
            Some(refresh_token),
        )
    }
}

/// Parse ISO-8601 duration strings returned by YouTube Data API v3 (e.g. `"PT4M19S"` -> `259_000`).
pub fn parse_iso8601_duration_ms(iso: &str) -> u64 {
    let Some(rest) = iso.trim().strip_prefix('P') else {
        return 0;
    };

    let mut days = 0u64;
    let mut hours = 0u64;
    let mut minutes = 0u64;
    let mut seconds = 0u64;
    let mut frac_ms = 0u64;

    let mut in_time = false;
    let mut num_acc = 0u64;
    let mut in_fraction = false;
    let mut frac_digits = 0u32;

    for ch in rest.chars() {
        if ch == 'T' {
            in_time = true;
            num_acc = 0;
            in_fraction = false;
            continue;
        }
        if ch == '.' || ch == ',' {
            seconds = seconds.saturating_add(num_acc);
            num_acc = 0;
            in_fraction = true;
            frac_digits = 0;
            continue;
        }
        if let Some(digit) = ch.to_digit(10) {
            if in_fraction {
                if frac_digits < 3 {
                    frac_ms = frac_ms * 10 + u64::from(digit);
                    frac_digits += 1;
                }
            } else {
                num_acc = num_acc.saturating_mul(10).saturating_add(u64::from(digit));
            }
            continue;
        }

        match (in_time, ch) {
            (false, 'D') => days = days.saturating_add(num_acc),
            (false, 'W') => days = days.saturating_add(num_acc.saturating_mul(7)),
            (true, 'H') => hours = hours.saturating_add(num_acc),
            (true, 'M') => minutes = minutes.saturating_add(num_acc),
            (true, 'S') => {
                if in_fraction {
                    while frac_digits < 3 {
                        frac_ms *= 10;
                        frac_digits += 1;
                    }
                } else {
                    seconds = seconds.saturating_add(num_acc);
                }
            }
            _ => {}
        }
        num_acc = 0;
        in_fraction = false;
        frac_digits = 0;
    }

    let total_secs = days
        .saturating_mul(86_400)
        .saturating_add(hours.saturating_mul(3600))
        .saturating_add(minutes.saturating_mul(60))
        .saturating_add(seconds);
    total_secs.saturating_mul(1000).saturating_add(frac_ms)
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
struct YouTubeListResponse<T> {
    etag: Option<String>,
    #[serde(default)]
    items: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelItem {
    snippet: Option<YouTubeChannelSnippet>,
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelSnippet {
    title: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubeVideoItem {
    id: String,
    snippet: Option<YouTubeVideoSnippet>,
    #[serde(rename = "contentDetails")]
    content_details: Option<YouTubeVideoContentDetails>,
}

#[derive(Debug, Deserialize)]
struct YouTubeVideoSnippet {
    title: Option<String>,
    #[serde(rename = "channelTitle")]
    channel_title: Option<String>,
    #[serde(rename = "categoryId")]
    category_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubeVideoContentDetails {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistSummary {
    id: String,
    etag: Option<String>,
    snippet: Option<YouTubeChannelSnippet>,
}

#[derive(Debug, Deserialize)]
struct YouTubeResourceId {
    #[serde(rename = "videoId")]
    video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistItemSnippet {
    title: Option<String>,
    #[serde(rename = "channelTitle")]
    channel_title: Option<String>,
    #[serde(rename = "videoOwnerChannelTitle")]
    video_owner_channel_title: Option<String>,
    #[serde(rename = "resourceId")]
    resource_id: Option<YouTubeResourceId>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistItemContentDetails {
    #[serde(rename = "videoId")]
    video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistItem {
    snippet: Option<YouTubePlaylistItemSnippet>,
    #[serde(rename = "contentDetails")]
    content_details: Option<YouTubePlaylistItemContentDetails>,
}

fn build_yt_track(
    video_id: String,
    raw_title: &str,
    raw_channel: &str,
    duration_ms: u64,
    liked: bool,
) -> Track {
    let cleaned = clean_youtube_metadata(raw_title, raw_channel);
    let album = if cleaned.is_yt_music_topic {
        "YouTube Music".to_string()
    } else {
        "YouTube".to_string()
    };
    let thumb = Some(format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg"));
    Track {
        id: format!("ytmusic:{video_id}"),
        title: if cleaned.title.is_empty() {
            "Untitled Video".to_string()
        } else {
            cleaned.title
        },
        artist: cleaned.artist,
        album,
        duration_ms,
        path: None,
        source: TrackSource::YouTubeMusic,
        codec: "OPUS".to_string(),
        sample_rate: 48_000,
        bit_depth: 16,
        bitrate_kbps: 160,
        isrc: None,
        spotify_id: None,
        youtube_video_id: Some(video_id),
        thumbnail_url: thumb,
        replay_gain_db: 0,
        liked,
    }
}

enum EtagGetOutcome<T> {
    NotModified,
    Modified {
        body: T,
        header_etag: Option<String>,
    },
}

fn get_bearer_json_conditional<T: for<'de> Deserialize<'de>>(
    agent: &ureq::Agent,
    url: &str,
    access_token: &str,
    if_none_match: Option<&str>,
) -> ProviderResult<EtagGetOutcome<T>> {
    let mut req = agent
        .get(url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", concat!("Ferric/", env!("CARGO_PKG_VERSION")));
    if let Some(etag) = if_none_match.filter(|s| !s.is_empty()) {
        req = req.header("If-None-Match", etag);
    }

    let resp = req
        .call()
        .map_err(|e| ProviderError::Http(format!("YouTube API request failed: {e}")))?;

    let status = resp.status().as_u16();
    if status == 304 {
        return Ok(EtagGetOutcome::NotModified);
    }

    let header_etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);

    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_JSON_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Could not read YouTube API body: {e}")))?;

    if status != 200 {
        let snippet = String::from_utf8_lossy(&bytes);
        let short: String = snippet.chars().take(180).collect();
        return Err(ProviderError::Http(format!(
            "YouTube API returned HTTP {status}: {short}"
        )));
    }

    let body: T = serde_json::from_slice(&bytes)
        .map_err(|e| ProviderError::Http(format!("Failed to decode YouTube JSON: {e}")))?;
    Ok(EtagGetOutcome::Modified { body, header_etag })
}

/// Synchronize YouTube / YouTube Music liked tracks and playlists using conditional `If-None-Match` ETags.
pub fn sync_youtube_catalog(
    access_token: &str,
    music_category_only: bool,
    known_etags: &HashMap<String, String>,
) -> ProviderResult<CloudSyncReport> {
    let agent = http_agent();

    let channels: YouTubeListResponse<YouTubeChannelItem> = get_bearer_json(
        &agent,
        "https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true",
        access_token,
    )?;
    let display_name = channels
        .items
        .into_iter()
        .next()
        .and_then(|c| c.snippet)
        .and_then(|s| s.title)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "YouTube Music".to_string());

    let liked_resp: YouTubeListResponse<YouTubeVideoItem> = get_bearer_json(
        &agent,
        "https://www.googleapis.com/youtube/v3/videos?part=snippet,contentDetails&myRating=like&maxResults=50",
        access_token,
    )?;

    let mut liked_tracks = Vec::new();
    let mut liked_video_ids = HashSet::new();
    for item in liked_resp.items {
        let vid = item.id.trim().to_string();
        if vid.is_empty() {
            continue;
        }
        let Some(snippet) = item.snippet else {
            continue;
        };
        if music_category_only && snippet.category_id.as_deref() != Some("10") {
            continue;
        }
        let raw_title = snippet.title.unwrap_or_default();
        if raw_title.is_empty() {
            continue;
        }
        let raw_channel = snippet.channel_title.unwrap_or_default();
        let duration_ms = item
            .content_details
            .and_then(|cd| cd.duration)
            .map(|d| parse_iso8601_duration_ms(&d))
            .unwrap_or(0);

        liked_video_ids.insert(vid.clone());
        liked_tracks.push(build_yt_track(
            vid,
            &raw_title,
            &raw_channel,
            duration_ms,
            true,
        ));
    }

    let playlists_resp: YouTubeListResponse<YouTubePlaylistSummary> = get_bearer_json(
        &agent,
        "https://www.googleapis.com/youtube/v3/playlists?part=snippet,contentDetails&mine=true&maxResults=25",
        access_token,
    )?;

    let mut playlists = Vec::new();
    let mut etag_skipped = 0usize;

    for pl in playlists_resp.items {
        let pl_id = pl.id.trim();
        if pl_id.is_empty() {
            continue;
        }
        let canonical_pl_id = format!("ytmusic:playlist:{pl_id}");
        let prev_etag = known_etags
            .get(&canonical_pl_id)
            .or_else(|| known_etags.get(pl_id))
            .map(String::as_str);

        if let (Some(prev), Some(curr)) = (prev_etag, pl.etag.as_deref())
            && prev == curr
        {
            etag_skipped += 1;
            continue;
        }

        let items_url = format!(
            "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet,contentDetails&playlistId={}&maxResults=50",
            url_encode(pl_id)
        );
        let outcome: EtagGetOutcome<YouTubeListResponse<YouTubePlaylistItem>> =
            get_bearer_json_conditional(&agent, &items_url, access_token, prev_etag)?;

        let (items_page, header_etag) = match outcome {
            EtagGetOutcome::NotModified => {
                etag_skipped += 1;
                continue;
            }
            EtagGetOutcome::Modified { body, header_etag } => (body, header_etag),
        };

        let mut bundle_tracks = Vec::new();
        for entry in items_page.items {
            let video_id = entry
                .content_details
                .as_ref()
                .and_then(|c| c.video_id.as_deref())
                .or_else(|| {
                    entry
                        .snippet
                        .as_ref()
                        .and_then(|s| s.resource_id.as_ref())
                        .and_then(|r| r.video_id.as_deref())
                })
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToOwned::to_owned);
            let Some(video_id) = video_id else {
                continue;
            };
            let Some(snippet) = entry.snippet else {
                continue;
            };
            let raw_title = snippet.title.unwrap_or_default();
            let trimmed_title = raw_title.trim();
            if trimmed_title.is_empty()
                || trimmed_title.eq_ignore_ascii_case("Deleted video")
                || trimmed_title.eq_ignore_ascii_case("Private video")
            {
                continue;
            }
            let owner_channel = snippet
                .video_owner_channel_title
                .or(snippet.channel_title)
                .unwrap_or_default();
            let liked = liked_video_ids.contains(&video_id);
            bundle_tracks.push(build_yt_track(
                video_id,
                trimmed_title,
                &owner_channel,
                0,
                liked,
            ));
        }

        let (pl_name, pl_desc) = match pl.snippet {
            Some(s) => {
                let name = s
                    .title
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "YouTube Playlist".to_string());
                let desc = s
                    .description
                    .map(|d| d.trim().to_string())
                    .unwrap_or_default();
                (name, desc)
            }
            None => ("YouTube Playlist".to_string(), String::new()),
        };

        let effective_etag = header_etag.or(items_page.etag).or(pl.etag);
        let pl_thumb = bundle_tracks.first().and_then(|t| t.thumbnail_url.clone());
        playlists.push(SyncedPlaylistBundle {
            playlist: Playlist {
                id: canonical_pl_id,
                name: pl_name,
                source: TrackSource::YouTubeMusic,
                etag: effective_etag,
                track_count: bundle_tracks.len(),
                description: pl_desc,
                thumbnail_url: pl_thumb,
            },
            tracks: bundle_tracks,
        });
    }

    Ok(CloudSyncReport {
        display_name,
        liked_tracks,
        playlists,
        etag_skipped,
        rate_limited_skipped: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    #[test]
    fn pkce_s256_matches_rfc7636_appendix_b_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(pkce_s256_challenge(verifier), expected);
    }

    #[test]
    fn generate_pkce_pair_produces_valid_lengths_and_matching_s256() {
        let p1 = generate_pkce_pair();
        let p2 = generate_pkce_pair();
        assert_eq!(p1.verifier.len(), 64);
        assert_eq!(p1.challenge_s256.len(), 43);
        assert_eq!(p1.state.len(), 32);
        assert!(p1.state.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(pkce_s256_challenge(&p1.verifier), p1.challenge_s256);
        assert_ne!(p1.verifier, p2.verifier);
        assert_ne!(p1.state, p2.state);
    }

    #[test]
    fn url_encoding_and_query_parsing_roundtrip_cleanly() {
        let encoded = url_encode("http://127.0.0.1:43824/callback?foo=bar baz");
        assert_eq!(
            encoded,
            "http%3A%2F%2F127.0.0.1%3A43824%2Fcallback%3Ffoo%3Dbar%20baz"
        );
        let parsed = parse_query_params("?code=auth_code_123&state=deadbeef01&scope=a+b%20c");
        assert_eq!(
            parsed.get("code").map(String::as_str),
            Some("auth_code_123")
        );
        assert_eq!(parsed.get("state").map(String::as_str), Some("deadbeef01"));
        assert_eq!(parsed.get("scope").map(String::as_str), Some("a b c"));
    }

    #[test]
    fn parse_iso8601_duration_handles_youtube_formats() {
        assert_eq!(parse_iso8601_duration_ms("PT4M19S"), 259_000);
        assert_eq!(parse_iso8601_duration_ms("PT1H2M3S"), 3_723_000);
        assert_eq!(parse_iso8601_duration_ms("PT45S"), 45_000);
        assert_eq!(parse_iso8601_duration_ms("PT3M"), 180_000);
        assert_eq!(parse_iso8601_duration_ms("P0D"), 0);
    }

    #[test]
    fn loopback_listener_extracts_verified_code_and_renders_ferric_html() {
        let loopback = bind_loopback_callback(0, "/callback", "state_token_abc123")
            .expect("bind ephemeral loopback");
        let addr = loopback
            .redirect_uri
            .strip_prefix("http://")
            .and_then(|s| s.strip_suffix("/callback"))
            .expect("redirect format")
            .to_string();

        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(&addr).expect("connect loopback");
            stream
                .write_all(
                    b"GET /callback?code=sp_code_987&state=state_token_abc123 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                )
                .expect("write get");
            let mut resp = String::new();
            let _ = stream.read_to_string(&mut resp);
            resp
        });

        let cancel = AtomicBool::new(false);
        let code = loopback
            .wait_for_auth_code(&cancel, Duration::from_secs(5))
            .expect("receive oauth code");
        assert_eq!(code, "sp_code_987");

        let html_resp = client.join().expect("join client");
        assert!(html_resp.contains("200 OK"));
        assert!(html_resp.contains("#000000"));
        assert!(html_resp.contains("#F3EEE3"));
        assert!(html_resp.contains("#D9541E"));
        assert!(html_resp.contains(
            "Ferric Authorization Complete. You can close this browser tab and return to Ferric."
        ));
    }

    #[test]
    fn loopback_listener_rejects_mismatched_state() {
        let loopback = bind_loopback_callback(0, "/callback", "expected_state")
            .expect("bind ephemeral loopback");
        let addr = loopback
            .redirect_uri
            .strip_prefix("http://")
            .and_then(|s| s.strip_suffix("/callback"))
            .expect("redirect format")
            .to_string();

        std::thread::spawn(move || {
            if let Ok(mut stream) = TcpStream::connect(&addr) {
                let _ = stream.write_all(
                    b"GET /callback?code=abc&state=wrong_state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                );
            }
        });

        let cancel = AtomicBool::new(false);
        let err = loopback
            .wait_for_auth_code(&cancel, Duration::from_secs(5))
            .expect_err("should fail state check");
        assert!(err.to_string().contains("state mismatch"));
    }

    #[test]
    fn authorize_urls_include_pkce_s256_and_scopes() {
        let pkce = generate_pkce_pair();
        let sp = spotify_authorize_url("sp_client", "http://127.0.0.1:43824/callback", &pkce);
        assert!(sp.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(sp.contains("code_challenge_method=S256"));
        assert!(sp.contains("user-library-read"));

        let yt = google_authorize_url("yt_client", "http://127.0.0.1:43824/callback", &pkce);
        assert!(yt.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(yt.contains("youtube.readonly"));
        assert!(yt.contains("code_challenge_method=S256"));
    }

    #[test]
    fn spotify_snapshot_and_retry_after_helpers_skip_unchanged_and_parse_headers() {
        let mut existing = HashMap::new();
        existing.insert("spotify:playlist:pl123".to_string(), "snap_abc".to_string());
        assert!(should_skip_spotify_playlist(
            "pl123",
            Some("snap_abc"),
            &existing
        ));
        assert!(!should_skip_spotify_playlist(
            "pl123",
            Some("snap_new"),
            &existing
        ));
        assert!(!should_skip_spotify_playlist("pl123", None, &existing));
        assert!(!should_skip_spotify_playlist(
            "pl999",
            Some("snap_abc"),
            &existing
        ));

        assert_eq!(parse_retry_after_secs(Some(" 12 ")), Some(12));
        assert_eq!(parse_retry_after_secs(Some("0")), None);
        assert_eq!(parse_retry_after_secs(Some("invalid")), None);
        assert_eq!(parse_retry_after_secs(None), None);
    }

    #[test]
    fn builtin_spotify_desktop_oauth_resolves_without_manual_client_id() {
        let (cid, port, path) = spotify_oauth_endpoint_config("");
        assert_eq!(cid, SPOTIFY_DESKTOP_CLIENT_ID);
        assert_eq!(port, 8989);
        assert_eq!(path, "/login");

        let pkce = generate_pkce_pair();
        let url = spotify_authorize_url("", SPOTIFY_DESKTOP_REDIRECT_URI, &pkce);
        assert!(url.contains(&format!("client_id={SPOTIFY_DESKTOP_CLIENT_ID}")));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8989%2Flogin"));
        assert!(url.contains("user-read-playback-state"));
        assert!(url.contains("streaming"));

        let (custom_cid, custom_port, custom_path) = spotify_oauth_endpoint_config("custom_id_123");
        assert_eq!(custom_cid, "custom_id_123");
        assert_eq!(custom_port, DEFAULT_OAUTH_CALLBACK_PORT);
        assert_eq!(custom_path, DEFAULT_OAUTH_CALLBACK_PATH);
    }

    #[test]
    fn activate_oauth_cancel_token_cancels_previous_listener() {
        let first = activate_oauth_cancel_token();
        assert!(!first.load(Ordering::SeqCst));
        let second = activate_oauth_cancel_token();
        assert!(first.load(Ordering::SeqCst));
        assert!(!second.load(Ordering::SeqCst));
    }
}
