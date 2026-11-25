use crate::oauth::{SyncedPlaylistBundle, url_encode};
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;

const SUBSONIC_API_VERSION: &str = "1.16.1";
const SUBSONIC_CLIENT_NAME: &str = "Ferric";
const SUBSONIC_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_SUBSONIC_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SUBSONIC_PLAYLISTS: usize = 100;
const MAX_SUBSONIC_PLAYLIST_TRACKS: usize = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubsonicCredentials {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

/// Pure-Rust RFC 1321 MD5 hex digest used strictly for Subsonic v1.13+ salted token authentication.
fn md5_hex(input: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    let (blocks, _) = msg.as_chunks::<64>();
    for block in blocks {
        let mut m = [0u32; 16];
        let (words, _) = block.as_chunks::<4>();
        for (slot, &w) in m.iter_mut().zip(words.iter()) {
            *slot = u32::from_le_bytes(w);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for b in digest {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

/// Encode UTF-8 password bytes as Subsonic `enc:<hex>` parameter (retained for compatibility tests).
pub fn encode_subsonic_password_hex(password: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(4 + password.len() * 2);
    out.push_str("enc:");
    for &b in password.as_bytes() {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

fn is_private_or_local_host(host: &str) -> bool {
    let h = host
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if h == "localhost"
        || h.ends_with(".local")
        || h.ends_with(".lan")
        || h.ends_with(".internal")
        || !h.contains('.')
    {
        return true;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
            std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        };
    }
    false
}

fn is_blocked_metadata_host(host: &str) -> bool {
    let h = host
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_link_local() || v4.is_unspecified(),
            std::net::IpAddr::V6(v6) => {
                v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
    }
    false
}

fn normalized_base_url(raw: &str) -> ProviderResult<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(ProviderError::Http(
            "Subsonic server URL cannot be empty".to_string(),
        ));
    }
    if trimmed.bytes().any(|b| b.is_ascii_control() || b == b' ') {
        return Err(ProviderError::Http(
            "Subsonic server URL contains invalid characters".to_string(),
        ));
    }
    let (is_https, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(ProviderError::Http(
            "Subsonic server URL must start with http:// or https://".to_string(),
        ));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(ProviderError::Http(
            "Subsonic server URL has an invalid host".to_string(),
        ));
    }
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .unwrap_or("")
            .trim_start_matches('[')
    } else {
        authority.split(':').next().unwrap_or("")
    };
    if host.is_empty() || is_blocked_metadata_host(host) {
        return Err(ProviderError::Http(
            "Refused to connect to link-local or unspecified Subsonic host".to_string(),
        ));
    }
    if !is_https && !is_private_or_local_host(host) {
        return Err(ProviderError::Http(
            "Public Subsonic servers must use https:// (http:// is allowed only for local/LAN hosts)"
                .to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn build_auth_query_with_salt(creds: &SubsonicCredentials, salt: &str) -> String {
    let token_input = format!("{}{}", creds.password, salt);
    let token = md5_hex(token_input.as_bytes());
    format!(
        "f=json&v={SUBSONIC_API_VERSION}&c={SUBSONIC_CLIENT_NAME}&u={}&t={token}&s={}",
        url_encode(creds.username.trim()),
        url_encode(salt)
    )
}

fn build_auth_query(creds: &SubsonicCredentials) -> String {
    let salt = ferric_core::secrets::csprng_hex(8);
    build_auth_query_with_salt(creds, &salt)
}

/// Construct direct OpenSubsonic/Navidrome audio stream URL for `song_id`.
pub fn subsonic_stream_url(creds: &SubsonicCredentials, song_id: &str) -> String {
    let base = creds.base_url.trim().trim_end_matches('/');
    let raw_id = song_id.strip_prefix("subsonic:").unwrap_or(song_id);
    format!(
        "{base}/rest/stream.view?id={}&{}",
        url_encode(raw_id),
        build_auth_query(creds)
    )
}

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let val = serde_json::Value::deserialize(deserializer)?;
    match val {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Ok(other.to_string()),
    }
}

fn deserialize_vec_or_single<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(val) = val else {
        return Ok(Vec::new());
    };
    match val {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(serde_json::from_value(item).map_err(serde::de::Error::custom)?);
            }
            Ok(out)
        }
        single => Ok(vec![
            serde_json::from_value(single).map_err(serde::de::Error::custom)?,
        ]),
    }
}

fn deserialize_first_isrc<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(val) = val else {
        return Ok(None);
    };
    match val {
        serde_json::Value::String(s) => Ok(Some(s).filter(|v| !v.trim().is_empty())),
        serde_json::Value::Array(arr) => Ok(arr.into_iter().find_map(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        })),
        _ => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
struct SubsonicEnvelope {
    #[serde(rename = "subsonic-response")]
    response: SubsonicResponseBody,
}

#[derive(Debug, Deserialize)]
struct SubsonicErrorBody {
    code: Option<i32>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubsonicResponseBody {
    status: String,
    error: Option<SubsonicErrorBody>,
    #[serde(rename = "searchResult3")]
    search_result3: Option<SubsonicSearchResult3>,
    playlists: Option<SubsonicPlaylistsContainer>,
    playlist: Option<SubsonicPlaylistDetail>,
}

#[derive(Debug, Deserialize)]
struct SubsonicSearchResult3 {
    #[serde(default, deserialize_with = "deserialize_vec_or_single")]
    song: Vec<SubsonicSong>,
}

#[derive(Debug, Deserialize)]
struct SubsonicPlaylistsContainer {
    #[serde(default, deserialize_with = "deserialize_vec_or_single")]
    playlist: Vec<SubsonicPlaylistSummary>,
}

#[derive(Debug, Deserialize)]
struct SubsonicPlaylistSummary {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    id: String,
    name: Option<String>,
    changed: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubsonicPlaylistDetail {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    id: String,
    name: Option<String>,
    comment: Option<String>,
    changed: Option<String>,
    #[serde(default, deserialize_with = "deserialize_vec_or_single")]
    entry: Vec<SubsonicSong>,
}

#[derive(Debug, Deserialize)]
struct SubsonicSong {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    id: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<u64>,
    suffix: Option<String>,
    #[serde(rename = "transcodedSuffix")]
    transcoded_suffix: Option<String>,
    #[serde(rename = "bitRate")]
    bit_rate: Option<u32>,
    #[serde(rename = "samplingRate")]
    sampling_rate: Option<u32>,
    #[serde(rename = "bitDepth")]
    bit_depth: Option<u16>,
    starred: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_first_isrc")]
    isrc: Option<String>,
}

fn map_subsonic_song(song: SubsonicSong) -> Option<Track> {
    let raw_id = song.id.trim();
    if raw_id.is_empty() {
        return None;
    }
    let title = song
        .title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Untitled Track".to_string());
    let artist = song
        .artist
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = song
        .album
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Subsonic Library".to_string());

    let codec = song
        .suffix
        .or(song.transcoded_suffix)
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "FLAC".to_string());

    let sample_rate = song.sampling_rate.filter(|&sr| sr > 0).unwrap_or(44_100);
    let bit_depth = song.bit_depth.filter(|&bd| bd > 0).unwrap_or(16);
    let bitrate_kbps = song.bit_rate.unwrap_or(0);

    Some(Track {
        id: format!("subsonic:{raw_id}"),
        title,
        artist,
        album,
        duration_ms: song.duration.unwrap_or(0).saturating_mul(1000),
        path: None,
        source: TrackSource::Subsonic,
        codec,
        sample_rate,
        bit_depth,
        bitrate_kbps,
        isrc: song.isrc,
        spotify_id: None,
        youtube_video_id: None,
        thumbnail_url: None,
        replay_gain_db: 0,
        liked: song.starred.is_some(),
    })
}

fn fetch_subsonic_response(agent: &ureq::Agent, url: &str) -> ProviderResult<SubsonicResponseBody> {
    let resp = agent
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", "Ferric/0.1.0")
        .call()
        .map_err(|e| ProviderError::Http(format!("Subsonic request failed: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_SUBSONIC_JSON_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Could not read Subsonic body: {e}")))?;

    if status != 200 {
        return Err(ProviderError::Http(format!(
            "Subsonic server returned HTTP {status}"
        )));
    }

    let env: SubsonicEnvelope = serde_json::from_slice(&bytes)
        .map_err(|e| ProviderError::Http(format!("Invalid OpenSubsonic JSON: {e}")))?;

    if !env.response.status.eq_ignore_ascii_case("ok") {
        let msg = env
            .response
            .error
            .and_then(|err| {
                err.message.map(|m| match err.code {
                    Some(c) => format!("{m} (code {c})"),
                    None => m,
                })
            })
            .unwrap_or_else(|| "Subsonic API returned status failed".to_string());
        return Err(ProviderError::Http(msg));
    }

    Ok(env.response)
}

/// Synchronize up to 200 catalog songs and all playlists from an OpenSubsonic / Navidrome server.
pub fn sync_subsonic_catalog(
    creds: &SubsonicCredentials,
) -> ProviderResult<(Vec<Track>, Vec<SyncedPlaylistBundle>)> {
    let base = normalized_base_url(&creds.base_url)?;
    let auth = build_auth_query(creds);

    let cfg = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(SUBSONIC_TIMEOUT))
        .build();
    let agent = ureq::Agent::new_with_config(cfg);

    let search_url =
        format!("{base}/rest/search3.view?query=&songCount=200&artistCount=0&albumCount=0&{auth}");
    let search_body = fetch_subsonic_response(&agent, &search_url)?;
    let mut catalog_tracks = Vec::new();
    if let Some(sr) = search_body.search_result3 {
        for song in sr.song {
            if let Some(track) = map_subsonic_song(song) {
                catalog_tracks.push(track);
            }
        }
    }

    let playlists_url = format!("{base}/rest/getPlaylists.view?{auth}");
    let playlists_body = fetch_subsonic_response(&agent, &playlists_url)?;
    let summaries = playlists_body
        .playlists
        .map(|p| p.playlist)
        .unwrap_or_default();

    let mut bundles = Vec::with_capacity(summaries.len().min(MAX_SUBSONIC_PLAYLISTS));
    for summary in summaries.into_iter().take(MAX_SUBSONIC_PLAYLISTS) {
        if summary.id.trim().is_empty() {
            continue;
        }
        let detail_url = format!(
            "{base}/rest/getPlaylist.view?id={}&{auth}",
            url_encode(&summary.id)
        );
        let detail_body = fetch_subsonic_response(&agent, &detail_url)?;
        let Some(detail) = detail_body.playlist else {
            continue;
        };

        let mut pl_tracks =
            Vec::with_capacity(detail.entry.len().min(MAX_SUBSONIC_PLAYLIST_TRACKS));
        for entry in detail.entry.into_iter().take(MAX_SUBSONIC_PLAYLIST_TRACKS) {
            if let Some(track) = map_subsonic_song(entry) {
                pl_tracks.push(track);
            }
        }

        let name = detail
            .name
            .or(summary.name)
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Subsonic Playlist".to_string());

        let playlist = Playlist {
            id: format!("subsonic:playlist:{}", detail.id),
            name,
            source: TrackSource::Subsonic,
            etag: detail.changed.or(summary.changed),
            track_count: pl_tracks.len(),
            description: detail
                .comment
                .map(|c| c.trim().to_string())
                .unwrap_or_default(),
            thumbnail_url: None,
        };
        bundles.push(SyncedPlaylistBundle {
            playlist,
            tracks: pl_tracks,
        });
    }

    Ok((catalog_tracks, bundles))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_subsonic_salted_token_and_validates_urls() {
        assert_eq!(encode_subsonic_password_hex("sesame"), "enc:736573616d65");
        // RFC 1321 MD5 vectors + Subsonic v1.13+ salted token md5("sesame" + "c19b2d")
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b"sesamec19b2d"), "26719a1196d2a940705a59634eb18eab");
        let creds = SubsonicCredentials {
            base_url: "https://music.example.lan/".to_string(),
            username: "audiophile".to_string(),
            password: "secret".to_string(),
        };
        let url = subsonic_stream_url(&creds, "subsonic:song_42");
        assert!(url.starts_with(
            "https://music.example.lan/rest/stream.view?id=song_42&f=json&v=1.16.1&c=Ferric&u=audiophile&t="
        ));
        assert!(url.contains("&s="));
        assert!(!url.contains("enc:"));
        assert!(!url.contains("secret"));

        assert!(normalized_base_url("http://169.254.169.254").is_err());
        assert!(normalized_base_url("http://public.example.com").is_err());
        assert!(normalized_base_url("https://public.example.com").is_ok());
        assert!(normalized_base_url("http://192.168.1.50:4533").is_ok());
    }

    #[test]
    fn parses_opensubsonic_lossless_song_fields() {
        let sample = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "searchResult3": {
                    "song": [{
                        "id": "trk_99",
                        "title": "Giorgio by Moroder",
                        "artist": "Daft Punk",
                        "album": "Random Access Memories",
                        "duration": 544,
                        "suffix": "flac",
                        "bitRate": 1580,
                        "samplingRate": 88200,
                        "bitDepth": 24,
                        "starred": "2026-01-12T00:00:00Z",
                        "isrc": ["USQX91300103"]
                    }]
                }
            }
        }"#;
        let env: SubsonicEnvelope = serde_json::from_str(sample).expect("parse subsonic json");
        let songs = env.response.search_result3.expect("searchResult3").song;
        assert_eq!(songs.len(), 1);
        let track = map_subsonic_song(songs.into_iter().next().expect("song")).expect("map track");
        assert_eq!(track.id, "subsonic:trk_99");
        assert_eq!(track.source, TrackSource::Subsonic);
        assert_eq!(track.codec, "FLAC");
        assert_eq!(track.sample_rate, 88_200);
        assert_eq!(track.bit_depth, 24);
        assert_eq!(track.bitrate_kbps, 1580);
        assert_eq!(track.duration_ms, 544_000);
        assert_eq!(track.isrc.as_deref(), Some("USQX91300103"));
        assert!(track.liked);
    }
}
