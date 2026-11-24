use crate::{ProviderError, ProviderResult};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLine {
    pub timestamp_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct SyncedLyrics {
    pub lines: Vec<LyricLine>,
}

impl SyncedLyrics {
    pub fn parse_lrc(raw: &str) -> Self {
        let mut lines = Vec::new();
        for row in raw.lines() {
            let trimmed = row.trim();
            if !trimmed.starts_with('[') {
                continue;
            }
            let Some((tag, body)) = trimmed.split_once(']') else {
                continue;
            };
            let tag = tag.trim_start_matches('[');
            if let Some(ts_ms) = parse_lrc_timestamp_ms(tag) {
                let text = body.trim().to_string();
                if !text.is_empty() {
                    lines.push(LyricLine {
                        timestamp_ms: ts_ms,
                        text,
                    });
                }
            }
        }
        lines.sort_by_key(|l| l.timestamp_ms);
        Self { lines }
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
    let mins: u64 = min_s.parse().ok()?;
    let (sec_s, frac_s) = sec_part.split_once('.').unwrap_or((sec_part, "0"));
    let secs: u64 = sec_s.parse().ok()?;
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
    std::fs::read_to_string(lrc_path).ok()
}

#[derive(Debug, Deserialize)]
struct LrcLibResponse {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

/// Fetch synced `.lrc` lyrics from open community database `lrclib.net`.
pub fn fetch_lrclib_synced(
    artist: &str,
    title: &str,
    album: &str,
    duration_secs: u64,
) -> ProviderResult<Option<String>> {
    let resp = ureq::get("https://lrclib.net/api/get")
        .header(
            "User-Agent",
            "Ferric/0.1.0 (https://github.com/Raflyf/Ferric)",
        )
        .query("artist_name", artist)
        .query("track_name", title)
        .query("album_name", album)
        .query("duration", duration_secs.to_string())
        .call()
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    let body: LrcLibResponse = resp
        .into_body()
        .read_json()
        .map_err(|e| ProviderError::Http(e.to_string()))?;

    Ok(body.synced_lyrics.or(body.plain_lyrics))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_and_tracks_active_window() {
        let lrc = "[00:12.50]I listened to your problems\n[00:18.00]Now listen to mine\n[00:24.25]I didn't want to anymore\n";
        let parsed = SyncedLyrics::parse_lrc(lrc);
        assert_eq!(parsed.lines.len(), 3);
        let (prev, active, next) = parsed.active_window(19_500);
        assert_eq!(prev, Some("I listened to your problems"));
        assert_eq!(active, Some("Now listen to mine"));
        assert_eq!(next, Some("I didn't want to anymore"));
    }
}
