use crate::oauth::url_encode;
use crate::{ProviderError, ProviderResult};
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use serde::Deserialize;
use std::path::Path;

const FERRIC_LYRICS_USER_AGENT: &str = concat!(
    "Ferric/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/Raflyf/Ferric)"
);
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
        let raw = raw.trim_start_matches('\u{feff}');
        let source = source_name.into();
        let mut lines = Vec::new();
        let mut timestamps = Vec::with_capacity(4);

        for row in raw.lines() {
            let mut rest = row.trim();
            if !rest.starts_with('[') {
                continue;
            }
            timestamps.clear();
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
            for &ts_ms in &timestamps {
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
    let frac_trim = frac_s.trim();
    if !frac_trim.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let frac_ms: u64 = match frac_trim.len() {
        0 => 0,
        1 => frac_trim.parse::<u64>().ok()?.checked_mul(100)?,
        2 => frac_trim.parse::<u64>().ok()?.checked_mul(10)?,
        _ => frac_trim[..3].parse::<u64>().ok()?,
    };
    mins.checked_mul(60_000)?
        .checked_add(secs.checked_mul(1_000)?)?
        .checked_add(frac_ms)
}

/// Check sidecar `<track>.lrc` next to local audio file first (0 network cost, 2 MiB safety cap).
pub fn read_local_sidecar_lrc(audio_path: &Path) -> Option<String> {
    use std::io::Read;
    let lrc_path = audio_path.with_extension("lrc");
    let file = std::fs::File::open(lrc_path).ok()?;
    let mut buf = Vec::new();
    file.take(MAX_LYRICS_RESPONSE_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    let text = String::from_utf8(buf).ok()?;
    (!text.trim().is_empty()).then_some(text)
}

/// Read embedded `USLT` / `LYRICS` / Vorbis `LYRICS` tag from a local audio file via `lofty`.
pub fn read_embedded_lyrics(audio_path: &Path) -> Option<String> {
    let opts = lofty::config::ParseOptions::new()
        .read_cover_art(false)
        .read_properties(false);
    let tagged = Probe::open(audio_path).ok()?.options(opts).read().ok()?;
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

fn lyrics_http_agent() -> &'static ureq::Agent {
    crate::shared_http_agent()
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

fn is_jp_kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{FF66}'..='\u{FF9F}')
}

fn is_kr_hangul(c: char) -> bool {
    matches!(c, '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}

fn is_cjk_or_kana_or_hangul(c: char) -> bool {
    is_jp_kana(c)
        || is_kr_hangul(c)
        || matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

fn strip_lrc_timestamps(line: &str) -> &str {
    let mut rest = line.trim();
    while let Some(after_open) = rest.strip_prefix('[') {
        let Some((tag, remainder)) = after_open.split_once(']') else {
            break;
        };
        if parse_lrc_timestamp_ms(tag).is_some() {
            rest = remainder.trim_start();
        } else {
            break;
        }
    }
    rest.trim()
}

/// Score content-level pollution in community LRCLIB uploads (e.g. triple-column fan-subs
/// mixing Japanese + Korean pronunciation + Korean translation with `" / "`, or CP949/Shift-JIS
/// `?` mojibake) while leaving legitimate bilingual songs (English + Japanese / English + Korean)
/// completely untouched (`0` penalty).
fn lyrics_content_penalty(raw: &str) -> i64 {
    let mut total_lines = 0usize;
    let mut mixed_jp_kr = 0usize;
    let mut double_slash = 0usize;
    let mut single_slash = 0usize;
    let mut mojibake_lines = 0usize;
    let mut replacement_chars = 0usize;
    let mut heavy_furigana = 0usize;

    for raw_line in raw.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || is_lrc_metadata_header(trimmed) {
            continue;
        }
        let text = strip_lrc_timestamps(trimmed);
        if text.is_empty() {
            continue;
        }
        total_lines += 1;

        let mut has_jp = false;
        let mut has_kr = false;
        let mut prev_q = false;
        let mut has_mojibake_q = false;
        let mut kana_paren_groups = 0usize;
        let mut in_paren = false;
        let mut paren_has_kana = false;

        for ch in text.chars() {
            if ch == '\u{FFFD}' {
                replacement_chars += 1;
            }
            if prev_q && is_cjk_or_kana_or_hangul(ch) {
                has_mojibake_q = true;
            }
            prev_q = ch == '?';

            if is_jp_kana(ch) {
                has_jp = true;
                if in_paren {
                    paren_has_kana = true;
                }
            } else if is_kr_hangul(ch) {
                has_kr = true;
            }

            if ch == '(' || ch == '\u{FF08}' {
                in_paren = true;
                paren_has_kana = false;
            } else if ch == ')' || ch == '\u{FF09}' {
                if in_paren && paren_has_kana {
                    kana_paren_groups += 1;
                }
                in_paren = false;
                paren_has_kana = false;
            }
        }

        if has_jp && has_kr {
            mixed_jp_kr += 1;
        }
        if has_mojibake_q {
            mojibake_lines += 1;
        }
        if kana_paren_groups >= 3 {
            heavy_furigana += 1;
        }
        let slash_count = text.matches(" / ").count();
        if slash_count >= 2 {
            double_slash += 1;
        } else if slash_count == 1 {
            single_slash += 1;
        }
    }

    if total_lines < 3 {
        return ((replacement_chars as i64) * 600).min(4_000);
    }

    let mut penalty = 0_i64;
    if mixed_jp_kr * 4 >= total_lines {
        penalty += 4_500;
    }
    if double_slash * 5 >= total_lines {
        penalty += 4_000;
    } else if (single_slash + double_slash) * 3 >= total_lines {
        penalty += 3_000;
    }
    if mojibake_lines >= 2 || replacement_chars > 0 {
        penalty += 3_500;
    }
    if heavy_furigana * 3 >= total_lines {
        penalty += 400;
    }
    penalty
}

/// Returns `true` when cached/fetched lyrics show strong multi-language fan-sub pollution
/// (e.g. Japanese + Korean translation columns or mojibake `?`) so Ferric can prefer clean originals.
pub fn is_polluted_multilingual_lrc(raw: &str) -> bool {
    lyrics_content_penalty(raw) >= 2_500
}

fn score_lrclib_candidate(
    cand: &LrcLibResponse,
    target_artist: &str,
    target_title: &str,
    duration_ms: u64,
) -> i64 {
    let synced_text = cand
        .synced_lyrics
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let plain_text = cand
        .plain_lyrics
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let has_synced = synced_text.is_some();
    let mut score = if has_synced { 10_000_i64 } else { 2_000_i64 };

    if let Some(name) = cand.track_name.as_deref() {
        if name.trim().eq_ignore_ascii_case(target_title) {
            score += 500;
        } else if ferric_core::text::contains_ascii_icase(name, target_title) {
            score += 200;
        }
    }

    if !target_artist.is_empty()
        && let Some(art) = cand.artist_name.as_deref()
    {
        if art.trim().eq_ignore_ascii_case(target_artist) {
            score += 500;
        } else if ferric_core::text::contains_ascii_icase(art, target_artist) {
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

    if let Some(body_text) = synced_text.or(plain_text) {
        score -= lyrics_content_penalty(body_text);
        let non_empty_lines = body_text
            .lines()
            .filter(|l| !strip_lrc_timestamps(l).is_empty())
            .count();
        if non_empty_lines >= 8 {
            score += 150;
        }
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
    // Return immediately only when exact match is a clean synced lyric; if it is plain-only
    // or a polluted multi-language fan-sub (e.g. JP/KR slash columns or mojibake `?`), keep it
    // as a fallback candidate and evaluate `/api/search` for a clean original version.
    let mut exact_fallback: Option<LrcLibResponse> = None;
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
                    if let Some(synced) = item
                        .synced_lyrics
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        && !is_polluted_multilingual_lrc(synced)
                    {
                        let parsed =
                            SyncedLyrics::parse_with_context(synced, duration_ms, "LRCLIB");
                        if !parsed.lines.is_empty() {
                            return Ok(Some((parsed, synced.to_string())));
                        }
                    }
                    exact_fallback = Some(item);
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

    let mut search_candidates: Vec<LrcLibResponse> = Vec::new();
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
                search_candidates = candidates;
            }
        }
        Err(err) => {
            last_network_err = Some(ProviderError::Http(err.to_string()));
        }
    }

    if let Some(exact) = exact_fallback {
        search_candidates.push(exact);
    }

    if !search_candidates.is_empty() {
        let has_vocal_candidate = search_candidates.iter().any(|c| {
            !c.instrumental
                && (c
                    .synced_lyrics
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                    || c.plain_lyrics
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty()))
        });

        let best = search_candidates
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
            .max_by_key(|c| score_lrclib_candidate(c, clean_artist, clean_title, duration_ms));

        if let Some(winner) = best {
            if let Some(synced) = winner.synced_lyrics.filter(|s| !s.trim().is_empty()) {
                let parsed = SyncedLyrics::parse_with_context(&synced, duration_ms, "LRCLIB");
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

    #[test]
    fn lrc_timestamp_parser_rejects_non_ascii_and_overflow_without_panicking() {
        assert_eq!(parse_lrc_timestamp_ms("00:12.😀"), None);
        assert_eq!(parse_lrc_timestamp_ms("9999999999999999:00.00"), None);
        assert_eq!(parse_lrc_timestamp_ms("01:02.3456"), Some(62_345));
    }

    #[test]
    fn clean_japanese_beats_polluted_jp_kr_fansub_and_preserves_bilingual_songs() {
        let polluted = concat!(
            "[00:18.29]?\u{76ee}(\u{3060}\u{3081})?\u{76ee}(\u{3060}\u{3081}) / \u{b2e4}\u{ba54} / \u{c548} \u{b3fc}\n",
            "[00:20.00]?\u{5473}?(\u{306e}\u{3046}\u{307f}\u{305d}) / \u{b178}\u{bbf8}\u{c18c} / \u{ba38}\u{b9bf}\u{c18d}\n",
            "[00:25.00]\u{30c0}?\u{30ea}\u{30f3} / \u{b2e4}\u{b9b0} / \u{b2ec}\u{b9c1}\n",
            "[00:30.00]\u{534a}\u{7aef}\u{306a}\u{304f}\u{30e9}\u{30d6} / \u{d55c}\u{d30c}\u{b098}\u{cfe0} / \u{c5c4}\u{ccad}\u{b098}\u{ac8c}\n",
        );
        let clean_jp = concat!(
            "[00:18.39]\u{99c4}\u{76ee}\u{99c4}\u{76ee}\u{99c4}\u{76ee}\n",
            "[00:20.00]\u{8133}\u{5473}\u{564c}\u{306e}\u{4e2d}\u{304b}\u{3089}\n",
            "[00:25.00]\u{30c0}\u{30fc}\u{30ea}\u{30f3}\u{30d9}\u{30a4}\u{30d3}\u{30fc}\n",
            "[00:30.00]\u{534a}\u{7aef}\u{306a}\u{304f}\u{30e9}\u{30d6}\n",
        );
        let clean_kpop = concat!(
            "[00:10.00]How you like that\n",
            "[00:14.00]\u{bcf4}\u{b780} \u{b4ef}\u{c774} \u{bb34}\u{b108}\u{c84c}\u{c5b4}\n",
            "[00:18.00]Look up in the sky it's a bird it's a plane\n",
            "[00:22.00]\u{c544}\u{b984}\u{b2e4}\u{c6b4} \u{bc24}\u{c774}\u{c57c}\n",
        );

        assert!(is_polluted_multilingual_lrc(polluted));
        assert!(!is_polluted_multilingual_lrc(clean_jp));
        assert!(!is_polluted_multilingual_lrc(clean_kpop));

        let cand_polluted = LrcLibResponse {
            instrumental: false,
            duration: Some(151.0),
            track_name: Some("IRIS OUT".to_string()),
            artist_name: Some("Kenshi Yonezu".to_string()),
            synced_lyrics: Some(polluted.to_string()),
            plain_lyrics: None,
        };
        let cand_clean = LrcLibResponse {
            instrumental: false,
            duration: Some(151.0),
            track_name: Some("IRIS OUT".to_string()),
            artist_name: Some("Kenshi Yonezu".to_string()),
            synced_lyrics: Some(clean_jp.to_string()),
            plain_lyrics: None,
        };
        assert!(
            score_lrclib_candidate(&cand_clean, "Kenshi Yonezu", "IRIS OUT", 151_000)
                > score_lrclib_candidate(&cand_polluted, "Kenshi Yonezu", "IRIS OUT", 151_000)
        );
    }
}
