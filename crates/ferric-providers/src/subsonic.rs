use crate::oauth::{SyncedPlaylistBundle, url_encode};
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;

const SUBSONIC_API_VERSION: &str = "1.16.1";
const SUBSONIC_CLIENT_NAME: &str = "Ferric";
const SUBSONIC_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_SUBSONIC_JSON_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubsonicCredentials {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

/// Encode UTF-8 password bytes as Subsonic `enc:<hex>` parameter.
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

fn normalized_base_url(raw: &str) -> ProviderResult<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(ProviderError::Http(
            "Subsonic server URL cannot be empty".to_string(),
        ));
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(ProviderError::Http(
            "Subsonic server URL must start with http:// or https://".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn build_auth_query(creds: &SubsonicCredentials) -> String {
    format!(
        "f=json&v={SUBSONIC_API_VERSION}&c={SUBSONIC_CLIENT_NAME}&u={}&p={}",
        url_encode(creds.username.trim()),
        encode_subsonic_password_hex(&creds.password)
    )
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

    let mut bundles = Vec::with_capacity(summaries.len());
    for summary in summaries {
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

        let mut pl_tracks = Vec::with_capacity(detail.entry.len());
        for entry in detail.entry {
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
    fn encodes_subsonic_hex_password_and_stream_url() {
        assert_eq!(encode_subsonic_password_hex("sesame"), "enc:736573616d65");
        let creds = SubsonicCredentials {
            base_url: "https://music.example.lan/".to_string(),
            username: "audiophile".to_string(),
            password: "secret".to_string(),
        };
        let url = subsonic_stream_url(&creds, "subsonic:song_42");
        assert_eq!(
            url,
            "https://music.example.lan/rest/stream.view?id=song_42&f=json&v=1.16.1&c=Ferric&u=audiophile&p=enc:736563726574"
        );
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
