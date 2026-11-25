use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackSource {
    Local,
    Spotify,
    YouTubeMusic,
    Subsonic,
    P2pHome,
}

impl TrackSource {
    pub fn as_badge(self) -> &'static str {
        match self {
            Self::Local => "LOCAL",
            Self::Spotify => "SPOTIFY",
            Self::YouTubeMusic => "YT MUSIC",
            Self::Subsonic => "SUBSONIC",
            Self::P2pHome => "P2P HOME",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "SPOTIFY" => Self::Spotify,
            "YT MUSIC" => Self::YouTubeMusic,
            "SUBSONIC" => Self::Subsonic,
            "P2P HOME" => Self::P2pHome,
            _ => Self::Local,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub path: Option<PathBuf>,
    pub source: TrackSource,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub bitrate_kbps: u32,
    pub isrc: Option<String>,
    pub spotify_id: Option<String>,
    pub youtube_video_id: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    pub replay_gain_db: i32,
    pub liked: bool,
}

impl Track {
    pub fn quality_badge(&self) -> String {
        if self.codec.is_empty() {
            return "AUDIO".to_string();
        }
        if self.bit_depth >= 24 {
            format!("{} {}b", self.codec, self.bit_depth)
        } else if self.bitrate_kbps > 0 {
            format!("{} {}k", self.codec, self.bitrate_kbps)
        } else {
            self.codec.clone()
        }
    }

    pub fn telemetry_label(&self) -> String {
        if self.sample_rate > 0 {
            let khz = self.sample_rate as f32 / 1000.0;
            if self.bit_depth > 0 {
                format!("{} {}b/{:.1}k", self.codec, self.bit_depth, khz)
            } else {
                format!("{} {:.1}k", self.codec, khz)
            }
        } else {
            self.quality_badge()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub source: TrackSource,
    pub etag: Option<String>,
    pub track_count: usize,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FadeCurve {
    #[default]
    EqualPower,
    SCurveVelvet,
    BassSwapAutomix,
}

impl FadeCurve {
    pub fn all() -> [Self; 3] {
        [Self::EqualPower, Self::SCurveVelvet, Self::BassSwapAutomix]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::EqualPower => "Equal-Power",
            Self::SCurveVelvet => "S-Curve Velvet",
            Self::BassSwapAutomix => "Bass-Swap Automix",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::EqualPower => Self::SCurveVelvet,
            Self::SCurveVelvet => Self::BassSwapAutomix,
            Self::BassSwapAutomix => Self::EqualPower,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackDspSettings {
    pub crossfade_ms: u32,
    pub fade_in_play_ms: u32,
    pub fade_curve: FadeCurve,
    pub gapless_album_lock: bool,
    pub trim_silence: bool,
    pub replay_gain: bool,
    pub mono_audio: bool,
    pub playback_speed: f32,
    pub sleep_timer_fade_out: bool,
}

impl Default for PlaybackDspSettings {
    fn default() -> Self {
        Self {
            crossfade_ms: 6_000,
            fade_in_play_ms: 240,
            fade_curve: FadeCurve::BassSwapAutomix,
            gapless_album_lock: true,
            trim_silence: true,
            replay_gain: true,
            mono_audio: false,
            playback_speed: 1.0,
            sleep_timer_fade_out: true,
        }
    }
}

impl PlaybackDspSettings {
    pub fn sanitized(&self) -> Self {
        Self {
            crossfade_ms: self.crossfade_ms.min(12_000),
            fade_in_play_ms: self.fade_in_play_ms.min(2_000),
            fade_curve: self.fade_curve,
            gapless_album_lock: self.gapless_album_lock,
            trim_silence: self.trim_silence,
            replay_gain: self.replay_gain,
            mono_audio: self.mono_audio,
            playback_speed: self.playback_speed.clamp(0.5, 2.0),
            sleep_timer_fade_out: self.sleep_timer_fade_out,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat: Off",
            Self::All => "Repeat: All",
            Self::One => "Repeat: One",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountKind {
    Spotify,
    YouTubeMusic,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountSettings {
    #[serde(default)]
    pub spotify_client_id: String,
    #[serde(default)]
    pub spotify_connected: bool,
    #[serde(default)]
    pub spotify_user_name: String,
    #[serde(default)]
    pub spotify_access_token: String,
    #[serde(default)]
    pub spotify_refresh_token: String,
    #[serde(default)]
    pub spotify_playlist_count: usize,
    #[serde(default)]
    pub spotify_liked_count: usize,
    #[serde(default)]
    pub youtube_client_id: String,
    #[serde(default)]
    pub youtube_client_secret: String,
    #[serde(default)]
    pub youtube_connected: bool,
    #[serde(default)]
    pub youtube_channel_name: String,
    #[serde(default)]
    pub youtube_access_token: String,
    #[serde(default)]
    pub youtube_refresh_token: String,
    #[serde(default)]
    pub youtube_playlist_count: usize,
    #[serde(default = "default_true")]
    pub youtube_music_category_only: bool,
    #[serde(default)]
    pub youtube_cookie: String,
    #[serde(default)]
    pub youtube_sapisid: String,
    #[serde(default)]
    pub youtube_visitor_data: String,
    #[serde(default)]
    pub subsonic_url: String,
    #[serde(default)]
    pub subsonic_username: String,
    #[serde(default)]
    pub subsonic_password: String,
    #[serde(default)]
    pub subsonic_connected: bool,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            spotify_client_id: String::new(),
            spotify_connected: false,
            spotify_user_name: String::new(),
            spotify_access_token: String::new(),
            spotify_refresh_token: String::new(),
            spotify_playlist_count: 0,
            spotify_liked_count: 0,
            youtube_client_id: String::new(),
            youtube_client_secret: String::new(),
            youtube_connected: false,
            youtube_channel_name: String::new(),
            youtube_access_token: String::new(),
            youtube_refresh_token: String::new(),
            youtube_playlist_count: 0,
            youtube_music_category_only: true,
            youtube_cookie: String::new(),
            youtube_sapisid: String::new(),
            youtube_visitor_data: String::new(),
            subsonic_url: String::new(),
            subsonic_username: String::new(),
            subsonic_password: String::new(),
            subsonic_connected: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSettings {
    pub auto_check_on_startup: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check_on_startup: true,
        }
    }
}

fn default_volume() -> f32 {
    0.85
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionResumeState {
    pub last_track_id: Option<String>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub total_tracks_played: u64,
    pub total_listened_secs: u64,
    pub recent_track_ids: Vec<String>,
    #[serde(default)]
    pub recent_searches: Vec<String>,
    #[serde(default)]
    pub pinned_ids: Vec<String>,
    #[serde(default = "default_true")]
    pub autoplay_radio: bool,
}

impl Default for SessionResumeState {
    fn default() -> Self {
        Self {
            last_track_id: None,
            volume: 0.85,
            shuffle: false,
            repeat: RepeatMode::Off,
            total_tracks_played: 0,
            total_listened_secs: 0,
            recent_track_ids: Vec::new(),
            recent_searches: Vec::new(),
            pinned_ids: Vec::new(),
            autoplay_radio: true,
        }
    }
}

impl SessionResumeState {
    pub fn record_play(&mut self, track_id: &str, duration_ms: u64) {
        self.last_track_id = Some(track_id.to_string());
        self.total_tracks_played = self.total_tracks_played.saturating_add(1);
        self.total_listened_secs = self
            .total_listened_secs
            .saturating_add((duration_ms / 1_000).min(900));
        self.recent_track_ids.retain(|id| id != track_id);
        self.recent_track_ids.insert(0, track_id.to_string());
        if self.recent_track_ids.len() > 50 {
            self.recent_track_ids.truncate(50);
        }
    }

    pub fn record_search(&mut self, query: &str) {
        let q = query.trim();
        if q.is_empty() {
            return;
        }
        self.recent_searches.retain(|s| !s.eq_ignore_ascii_case(q));
        self.recent_searches.insert(0, q.to_string());
        if self.recent_searches.len() > 12 {
            self.recent_searches.truncate(12);
        }
    }

    pub fn toggle_pin(&mut self, item_id: &str) -> bool {
        if let Some(pos) = self.pinned_ids.iter().position(|x| x == item_id) {
            self.pinned_ids.remove(pos);
            false
        } else {
            self.pinned_ids.insert(0, item_id.to_string());
            if self.pinned_ids.len() > 16 {
                self.pinned_ids.truncate(16);
            }
            true
        }
    }
}

/// Parse an `.m3u` / `.m3u8` or Exportify `.csv` playlist file into a `Playlist` and `Vec<Track>`.
pub fn parse_playlist_import_text(
    file_stem: &str,
    content: &str,
    base_dir: Option<&std::path::Path>,
) -> (Playlist, Vec<Track>) {
    let name = if file_stem.trim().is_empty() {
        "Imported Playlist".to_string()
    } else {
        file_stem.trim().to_string()
    };
    let pl_id = format!("imported:{}", name.to_ascii_lowercase().replace(' ', "_"));
    let mut tracks = Vec::new();

    let trimmed = content.trim_start_matches('\u{feff}');
    let first_line = trimmed.lines().next().unwrap_or("");
    if first_line.contains("Track Name") && first_line.contains(',') {
        // Exportify / Spotify CSV format
        let headers: Vec<&str> = split_csv_line(first_line);
        let idx_uri = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("Track URI"));
        let idx_title = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("Track Name"));
        let idx_album = headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case("Album Name"));
        let idx_artist = headers
            .iter()
            .position(|h| h.to_ascii_lowercase().starts_with("artist name"));
        let idx_dur = headers
            .iter()
            .position(|h| h.to_ascii_lowercase().starts_with("duration"));
        let idx_isrc = headers.iter().position(|h| h.eq_ignore_ascii_case("ISRC"));

        for (line_idx, row) in trimmed.lines().skip(1).enumerate() {
            if row.trim().is_empty() {
                continue;
            }
            let cols = split_csv_line(row);
            let title = idx_title
                .and_then(|i| cols.get(i))
                .copied()
                .unwrap_or("")
                .trim();
            if title.is_empty() {
                continue;
            }
            let artist = idx_artist
                .and_then(|i| cols.get(i))
                .copied()
                .unwrap_or("Unknown Artist")
                .replace(';', ", ");
            let album = idx_album
                .and_then(|i| cols.get(i))
                .copied()
                .unwrap_or("Imported")
                .to_string();
            let dur_ms = idx_dur
                .and_then(|i| cols.get(i))
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            let uri = idx_uri.and_then(|i| cols.get(i)).copied().unwrap_or("");
            let isrc = idx_isrc
                .and_then(|i| cols.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let id = if !uri.is_empty() {
                uri.to_string()
            } else {
                format!("{pl_id}:{line_idx}")
            };
            tracks.push(Track {
                id,
                title: title.to_string(),
                artist,
                album,
                duration_ms: dur_ms,
                path: None,
                source: TrackSource::Spotify,
                codec: "SPOTIFY".to_string(),
                sample_rate: 0,
                bit_depth: 0,
                bitrate_kbps: 0,
                isrc,
                spotify_id: uri.strip_prefix("spotify:track:").map(ToOwned::to_owned),
                youtube_video_id: None,
                thumbnail_url: None,
                replay_gain_db: 0,
                liked: false,
            });
        }
    } else {
        // M3U / M3U8 format
        let mut pending_meta: Option<(u64, String, String)> = None;
        for (line_idx, raw_line) in trimmed.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.eq_ignore_ascii_case("#EXTM3U") {
                continue;
            }
            if let Some(ext) = line.strip_prefix("#EXTINF:") {
                let (dur_part, info_part) = ext.split_once(',').unwrap_or((ext, ""));
                let secs = dur_part.trim().parse::<i64>().unwrap_or(0).max(0) as u64;
                let (artist, title) = info_part
                    .split_once(" - ")
                    .map(|(a, t)| (a.trim().to_string(), t.trim().to_string()))
                    .unwrap_or_else(|| {
                        ("Unknown Artist".to_string(), info_part.trim().to_string())
                    });
                pending_meta = Some((secs * 1_000, artist, title));
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            let p = PathBuf::from(line);
            let resolved = if p.is_absolute() {
                p
            } else if let Some(base) = base_dir {
                base.join(&p)
            } else {
                p
            };
            let fallback_title = resolved
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Track")
                .to_string();
            let (dur_ms, artist, title) =
                pending_meta
                    .take()
                    .unwrap_or((0, "Local File".to_string(), fallback_title));
            let ext_upper = resolved
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("AUDIO")
                .to_ascii_uppercase();
            tracks.push(Track {
                id: format!("local:{}", resolved.to_string_lossy()),
                title: if title.is_empty() {
                    format!("Track {}", line_idx + 1)
                } else {
                    title
                },
                artist,
                album: name.clone(),
                duration_ms: dur_ms,
                path: Some(resolved),
                source: TrackSource::Local,
                codec: ext_upper,
                sample_rate: 0,
                bit_depth: 0,
                bitrate_kbps: 0,
                isrc: None,
                spotify_id: None,
                youtube_video_id: None,
                thumbnail_url: None,
                replay_gain_db: 0,
                liked: false,
            });
        }
    }

    let playlist = Playlist {
        id: pl_id,
        name,
        source: TrackSource::Local,
        etag: None,
        track_count: tracks.len(),
        description: "Imported playlist".to_string(),
        thumbnail_url: None,
    };
    (playlist, tracks)
}

fn split_csv_line(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut start = 0usize;
    let mut in_quotes = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b',' && !in_quotes {
            let slice = line[start..i].trim().trim_matches('"');
            out.push(slice);
            start = i + 1;
        }
    }
    out.push(line[start..].trim().trim_matches('"'));
    out
}

/// Format milliseconds as `MM:SS` (or `H:MM:SS` if >= 1 hour).
pub fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_formats_minutes_and_hours() {
        assert_eq!(format_duration_ms(0), "00:00");
        assert_eq!(format_duration_ms(134_000), "02:14");
        assert_eq!(format_duration_ms(3_723_000), "1:02:03");
    }

    #[test]
    fn legacy_account_settings_json_deserializes_with_defaults() {
        let legacy = r#"{"spotify_client_id":"abc","spotify_connected":true}"#;
        let parsed: AccountSettings = serde_json::from_str(legacy).expect("deserialize legacy");
        assert_eq!(parsed.spotify_client_id, "abc");
        assert!(parsed.spotify_connected);
        assert!(parsed.youtube_music_category_only);
        assert!(!parsed.subsonic_connected);
    }

    #[test]
    fn playback_dsp_defaults_enable_seamless_velvet_automix() {
        let dsp = PlaybackDspSettings::default();
        assert_eq!(dsp.crossfade_ms, 6_000);
        assert_eq!(dsp.fade_in_play_ms, 240);
        assert_eq!(dsp.fade_curve, FadeCurve::BassSwapAutomix);
        assert!(dsp.gapless_album_lock);
        assert!(dsp.trim_silence);
        assert_eq!(FadeCurve::all().len(), 3);
    }
}
