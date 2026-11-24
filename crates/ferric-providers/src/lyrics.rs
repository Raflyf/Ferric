use crate::oauth::url_encode;
use crate::{ProviderError, ProviderResult};
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use serde::Deserialize;
use std::path::Path;
use std::time::Duration;

const FERRIC_LYRICS_USER_AGENT: &str = "Ferric/0.1.0 (https://github.com/Raflyf/Ferric)";
const LYRICS_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_LYRICS_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_PLAIN_DURATION_MS: u64 = 180_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    pub timestamp_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct SyncedLyrics {
    pub lines: Vec<LyricLine>,
    pub source_name: String,
    pub is_synced: bool,
}

fn is_lrc_metadata_header(trimmed: &str) -> bool {
    let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return false;
    };
    let Some((key, _)) = inner.split_once(':') else {
        return false;
    };
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "ar" | "ti" | "al" | "au" | "by" | "length" | "offset" | "re" | "ve" | "id"
    )
}

impl SyncedLyrics {
    /// Build evenly-paced lyric lines from unsynced plain text across `duration_ms.max(60_000)`.
    pub fn from_plain_text(raw: &str, duration_ms: u64, source_name: impl Into<String>) -> Self {
        let cleaned_lines: Vec<String> = raw
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !is_lrc_metadata_header(line))
            .map(ToOwned::to_owned)
            .collect();

        if cleaned_lines.is_empty() {
            return Self {
                lines: Vec::new(),
                source_name: source_name.into(),
                is_synced: false,
            };
        }

        let span_ms = duration_ms.max(60_000);
        let total = cleaned_lines.len() as u64;
        let lines = cleaned_lines
            .into_iter()
            .enumerate()
            .map(|(idx, text)| LyricLine {
                timestamp_ms: ((idx as u64).saturating_mul(span_ms)) / total,
                text,
            })
            .collect();

        Self {
            lines,
            source_name: source_name.into(),
            is_synced: false,
        }
    }

    /// Parse timestamped `.lrc` text, falling back to `from_plain_text` when no `[mm:ss]` tags exist.
    pub fn parse_lrc(raw: &str) -> Self {
        Self::parse_with_context(raw, DEFAULT_PLAIN_DURATION_MS, "LRCLIB")
    }

    pub fn parse_with_context(raw: &str, duration_ms: u64, source_name: impl Into<String>) -> Self {
        let source = source_name.into();
        let mut lines = Vec::new();

        for row in raw.lines() {
            let mut rest = row.trim();
            if !rest.starts_with('[') {
                continue;
            }
            let mut timestamps = Vec::new();
            while let Some(after_open) = rest.strip_prefix('[') {
                let Some((tag, remainder)) = after_open.split_once(']') else {
                    break;
                };
                if let Some(ts_ms) = parse_lrc_timestamp_ms(tag) {
                    timestamps.push(ts_ms);
                    rest = remainder;
                } else {
                    break;
                }
            }
            if timestamps.is_empty() {
                continue;
            }
            let text = rest.trim();
            if text.is_empty() {
                continue;
            }
            for ts_ms in timestamps {
                lines.push(LyricLine {
                    timestamp_ms: ts_ms,
                    text: text.to_string(),
                });
            }
        }

        if lines.is_empty() {
            return Self::from_plain_text(raw, duration_ms, source);
        }

        lines.sort_by_key(|l| l.timestamp_ms);
        Self {
            lines,
            source_name: source,
            is_synced: true,
        }
    }

    /// Return `(prev_line, active_line, next_line)` for the given playback position.
    pub fn active_window(&self, position_ms: u64) -> (Option<&str>, Option<&str>, Option<&str>) {
        if self.lines.is_empty() {
            return (None, None, None);
        }
        let idx = match self
            .lines
            .binary_search_by_key(&position_ms, |l| l.timestamp_ms)
        {
            Ok(i) => i,
            Err(0) => return (None, None, Some(self.lines[0].text.as_str())),
            Err(i) => i - 1,
        };
        let prev = idx.checked_sub(1).map(|i| self.lines[i].text.as_str());
        let active = Some(self.lines[idx].text.as_str());
        let next = self.lines.get(idx + 1).map(|l| l.text.as_str());
        (prev, active, next)
    }
}

fn parse_lrc_timestamp_ms(tag: &str) -> Option<u64> {
    let (min_s, sec_part) = tag.split_once(':')?;
    let mins: u64 = min_s.trim().parse().ok()?;
    let (sec_s, frac_s) = sec_part
        .split_once('.')
        .or_else(|| sec_part.split_once(':'))
        .unwrap_or((sec_part, "0"));
    let secs: u64 = sec_s.trim().parse().ok()?;
    let frac_ms: u64 = match frac_s.len() {
        0 => 0,
        1 => frac_s.parse::<u64>().ok()? * 100,
        2 => frac_s.parse::<u64>().ok()? * 10,
        _ => frac_s[..3].parse::<u64>().ok()?,
    };
    Some(mins * 60_000 + secs * 1000 + frac_ms)
}

/// Check sidecar `<track>.lrc` next to local audio file first (0 network cost).
pub fn read_local_sidecar_lrc(audio_path: &Path) -> Option<String> {
    let lrc_path = audio_path.with_extension("lrc");
    std::fs::read_to_string(lrc_path)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Read embedded `USLT` / `LYRICS` / Vorbis `LYRICS` tag from a local audio file via `lofty`.
pub fn read_embedded_lyrics(audio_path: &Path) -> Option<String> {
    let tagged = Probe::open(audio_path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.get_string(ItemKey::Lyrics)
        .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Deserialize)]
struct LrcLibResponse {
    #[serde(default)]
    instrumental: bool,
    duration: Option<f64>,
    #[serde(rename = "trackName")]
    track_name: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LyricsOvhResponse {
    lyrics: Option<String>,
}

fn lyrics_http_agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(LYRICS_HTTP_TIMEOUT))
        .build();
    ureq::Agent::new_with_config(cfg)
}

/// Fetch synced `.lrc` lyrics from open community database `lrclib.net`.
pub fn fetch_lrclib_synced(
    artist: &str,
    title: &str,
    album: &str,
    duration_secs: u64,
) -> ProviderResult<Option<String>> {
    let agent = lyrics_http_agent();
    let resp = agent
        .get("https://lrclib.net/api/get")
        .header("User-Agent", FERRIC_LYRICS_USER_AGENT)
        .header("Accept", "application/json")
        .query("artist_name", artist)
        .query("track_name", title)
        .query("album_name", album)
        .query("duration", duration_secs.to_string())
        .call()
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    let status = resp.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if status != 200 {
        return Err(ProviderError::Http(format!(
            "LRCLIB returned HTTP {status}"
        )));
    }

    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_LYRICS_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    let body: LrcLibResponse =
        serde_json::from_slice(&bytes).map_err(|e| ProviderError::Http(e.to_string()))?;

    Ok(body
        .synced_lyrics
        .filter(|s| !s.trim().is_empty())
        .or_else(|| body.plain_lyrics.filter(|s| !s.trim().is_empty())))
}

fn score_lrclib_candidate(
    cand: &LrcLibResponse,
    target_artist: &str,
    target_title: &str,
    duration_ms: u64,
) -> i64 {
    let has_synced = cand
        .synced_lyrics
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty());
    let mut score = if has_synced { 10_000_i64 } else { 2_000_i64 };

    if let Some(name) = cand.track_name.as_deref() {
        if name.trim().eq_ignore_ascii_case(target_title) {
            score += 500;
        } else if name
            .to_ascii_lowercase()
            .contains(&target_title.to_ascii_lowercase())
        {
            score += 200;
        }
    }

    if !target_artist.is_empty()
        && let Some(art) = cand.artist_name.as_deref()
    {
        if art.trim().eq_ignore_ascii_case(target_artist) {
            score += 500;
        } else if art
            .to_ascii_lowercase()
            .contains(&target_artist.to_ascii_lowercase())
        {
            score += 200;
        }
    }

    if duration_ms > 0
        && let Some(cand_secs) = cand.duration
    {
        let target_secs = duration_ms as f64 / 1000.0;
        let diff_secs = (cand_secs - target_secs).abs();
        let penalty = (diff_secs * 40.0).min(2_500.0) as i64;
        score -= penalty;
    }

    score
}

fn normalize_plain_text(raw: &str) -> String {
    let unified = raw.replace("\r\n", "\n").replace('\r', "\n");
    unified.trim().to_string()
}

/// Multi-provider free no-key lyrics waterfall:
/// 1. Local sidecar `.lrc` (`"Local .lrc"`) + embedded audio tag (`"Embedded Tag"`)
/// 2. LRCLIB exact `/api/get` (`"LRCLIB"`)
/// 3. LRCLIB fuzzy `/api/search` (`"LRCLIB"`)
/// 4. Lyrics.ovh free fallback `/v1/<artist>/<title>` (`"Lyrics.ovh"`)
pub fn fetch_best_free_lyrics(
    artist: &str,
    title: &str,
    album: &str,
    duration_ms: u64,
    local_path: Option<&Path>,
) -> ProviderResult<Option<(SyncedLyrics, String)>> {
    // Step A: Local sidecar .lrc, then embedded audio file lyrics tag
    if let Some(path) = local_path {
        if let Some(sidecar_raw) = read_local_sidecar_lrc(path) {
            let parsed = SyncedLyrics::parse_with_context(&sidecar_raw, duration_ms, "Local .lrc");
            if !parsed.lines.is_empty() {
                return Ok(Some((parsed, sidecar_raw)));
            }
        }
        if let Some(embedded_raw) = read_embedded_lyrics(path) {
            let parsed =
                SyncedLyrics::parse_with_context(&embedded_raw, duration_ms, "Embedded Tag");
            if !parsed.lines.is_empty() {
                return Ok(Some((parsed, embedded_raw)));
            }
        }
    }

    let clean_title = title.trim();
    if clean_title.is_empty() {
        return Ok(None);
    }
    let clean_artist = artist.trim();
    let clean_album = album.trim();

    let agent = lyrics_http_agent();
    let mut last_network_err: Option<ProviderError> = None;
    let mut any_http_reached = false;

    // Step B: LRCLIB Exact (/api/get)
    if !clean_album.is_empty() && clean_album != "Unknown Album" && duration_ms > 0 {
        let dur_secs = (duration_ms / 1000).max(1);
        match agent
            .get("https://lrclib.net/api/get")
            .header("User-Agent", FERRIC_LYRICS_USER_AGENT)
            .header("Accept", "application/json")
            .query("artist_name", clean_artist)
            .query("track_name", clean_title)
            .query("album_name", clean_album)
            .query("duration", dur_secs.to_string())
            .call()
        {
            Ok(resp) => {
                any_http_reached = true;
                if resp.status().as_u16() == 200
                    && let Ok(bytes) = resp
                        .into_body()
                        .into_with_config()
                        .limit(MAX_LYRICS_RESPONSE_BYTES)
                        .read_to_vec()
                    && let Ok(item) = serde_json::from_slice::<LrcLibResponse>(&bytes)
                {
                    if let Some(synced) = item.synced_lyrics.filter(|s| !s.trim().is_empty()) {
                        let parsed =
                            SyncedLyrics::parse_with_context(&synced, duration_ms, "LRCLIB");
                        if !parsed.lines.is_empty() {
                            return Ok(Some((parsed, synced)));
                        }
                    }
                    if let Some(plain) = item.plain_lyrics.filter(|s| !s.trim().is_empty()) {
                        let norm = normalize_plain_text(&plain);
                        let parsed = SyncedLyrics::from_plain_text(&norm, duration_ms, "LRCLIB");
                        if !parsed.lines.is_empty() {
                            return Ok(Some((parsed, norm)));
                        }
                    }
                }
            }
            Err(err) => {
                last_network_err = Some(ProviderError::Http(err.to_string()));
            }
        }
    }

    // Step C: LRCLIB Fuzzy Search (/api/search?q=<artist> <title>)
    let search_q = if clean_artist.is_empty() || clean_artist.eq_ignore_ascii_case("Unknown Artist")
    {
        clean_title.to_string()
    } else {
        format!("{clean_artist} {clean_title}")
    };

    match agent
        .get("https://lrclib.net/api/search")
        .header("User-Agent", FERRIC_LYRICS_USER_AGENT)
        .header("Accept", "application/json")
        .query("q", &search_q)
        .call()
    {
        Ok(resp) => {
            any_http_reached = true;
            if resp.status().as_u16() == 200
                && let Ok(bytes) = resp
                    .into_body()
                    .into_with_config()
                    .limit(MAX_LYRICS_RESPONSE_BYTES)
                    .read_to_vec()
                && let Ok(candidates) = serde_json::from_slice::<Vec<LrcLibResponse>>(&bytes)
            {
                let has_vocal_candidate = candidates.iter().any(|c| {
                    !c.instrumental
                        && (c
                            .synced_lyrics
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty())
                            || c.plain_lyrics
                                .as_deref()
                                .is_some_and(|s| !s.trim().is_empty()))
                });

                let best = candidates
                    .into_iter()
                    .filter(|c| {
                        let has_text = c
                            .synced_lyrics
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty())
                            || c.plain_lyrics
                                .as_deref()
                                .is_some_and(|s| !s.trim().is_empty());
                        has_text && (!has_vocal_candidate || !c.instrumental)
                    })
                    .max_by_key(|c| {
                        score_lrclib_candidate(c, clean_artist, clean_title, duration_ms)
                    });

                if let Some(winner) = best {
                    if let Some(synced) = winner.synced_lyrics.filter(|s| !s.trim().is_empty()) {
                        let parsed =
                            SyncedLyrics::parse_with_context(&synced, duration_ms, "LRCLIB");
                        if !parsed.lines.is_empty() {
                            return Ok(Some((parsed, synced)));
                        }
                    }
                    if let Some(plain) = winner.plain_lyrics.filter(|s| !s.trim().is_empty()) {
                        let norm = normalize_plain_text(&plain);
                        let parsed = SyncedLyrics::from_plain_text(&norm, duration_ms, "LRCLIB");
                        if !parsed.lines.is_empty() {
                            return Ok(Some((parsed, norm)));
                        }
                    }
                }
            }
        }
        Err(err) => {
            last_network_err = Some(ProviderError::Http(err.to_string()));
        }
    }

    // Step D: Lyrics.ovh Free Fallback
    if !clean_artist.is_empty() && !clean_artist.eq_ignore_ascii_case("Unknown Artist") {
        let ovh_url = format!(
            "https://api.lyrics.ovh/v1/{}/{}",
            url_encode(clean_artist),
            url_encode(clean_title)
        );
        match agent
            .get(&ovh_url)
            .header("User-Agent", FERRIC_LYRICS_USER_AGENT)
            .header("Accept", "application/json")
            .call()
        {
            Ok(resp) => {
                any_http_reached = true;
                if resp.status().as_u16() == 200
                    && let Ok(bytes) = resp
                        .into_body()
                        .into_with_config()
                        .limit(MAX_LYRICS_RESPONSE_BYTES)
                        .read_to_vec()
                    && let Ok(ovh) = serde_json::from_slice::<LyricsOvhResponse>(&bytes)
                    && let Some(raw_lyrics) = ovh.lyrics
                {
                    let norm = normalize_plain_text(&raw_lyrics);
                    let parsed = SyncedLyrics::from_plain_text(&norm, duration_ms, "Lyrics.ovh");
                    if !parsed.lines.is_empty() {
                        return Ok(Some((parsed, norm)));
                    }
                }
            }
            Err(err) => {
                last_network_err = Some(ProviderError::Http(err.to_string()));
            }
        }
    }

    if !any_http_reached && let Some(err) = last_network_err {
        return Err(err);
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_and_tracks_active_window() {
        let lrc = "[00:12.50]I listened to your problems\n[00:18.00]Now listen to mine\n[00:24.25]I didn't want to anymore\n";
        let parsed = SyncedLyrics::parse_lrc(lrc);
        assert!(parsed.is_synced);
        assert_eq!(parsed.lines.len(), 3);
        let (prev, active, next) = parsed.active_window(19_500);
        assert_eq!(prev, Some("I listened to your problems"));
        assert_eq!(active, Some("Now listen to mine"));
        assert_eq!(next, Some("I didn't want to anymore"));
    }

    #[test]
    fn plain_text_lyrics_assign_evenly_spaced_timestamps_and_scroll() {
        let plain = "First line\n\nSecond line\nThird line\nFourth line\n";
        let parsed = SyncedLyrics::from_plain_text(plain, 120_000, "Lyrics.ovh");
        assert!(!parsed.is_synced);
        assert_eq!(parsed.source_name, "Lyrics.ovh");
        assert_eq!(parsed.lines.len(), 4);
        assert_eq!(parsed.lines[0].timestamp_ms, 0);
        assert_eq!(parsed.lines[1].timestamp_ms, 30_000);
        assert_eq!(parsed.lines[2].timestamp_ms, 60_000);
        assert_eq!(parsed.lines[3].timestamp_ms, 90_000);

        // parse_lrc falls back to from_plain_text when no [mm:ss] tags exist
        let fallback = SyncedLyrics::parse_lrc(plain);
        assert!(!fallback.is_synced);
        assert_eq!(fallback.lines.len(), 4);
    }

    #[test]
    fn lrclib_candidate_scoring_prefers_synced_and_matching_duration() {
        let synced_close = LrcLibResponse {
            instrumental: false,
            duration: Some(259.0),
            track_name: Some("Instant Crush".to_string()),
            artist_name: Some("Daft Punk".to_string()),
            synced_lyrics: Some("[00:10.00]Line".to_string()),
            plain_lyrics: Some("Line".to_string()),
        };
        let plain_far = LrcLibResponse {
            instrumental: false,
            duration: Some(310.0),
            track_name: Some("Instant Crush".to_string()),
            artist_name: Some("Daft Punk".to_string()),
            synced_lyrics: None,
            plain_lyrics: Some("Line".to_string()),
        };

        let s1 = score_lrclib_candidate(&synced_close, "Daft Punk", "Instant Crush", 259_000);
        let s2 = score_lrclib_candidate(&plain_far, "Daft Punk", "Instant Crush", 259_000);
        assert!(s1 > s2);
    }
}
