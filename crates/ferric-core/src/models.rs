use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioQuality {
    Low,
    Normal,
    #[default]
    High,
    Best,
}

impl AudioQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low (64k)",
            Self::Normal => "Normal (128k)",
            Self::High => "High (160k Opus)",
            Self::Best => "Best Available",
        }
    }

    pub fn cache_suffix(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "std",
            Self::High => "high",
            Self::Best => "best",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoQuality {
    Auto,
    P360,
    P480,
    #[default]
    P720,
    P1080,
}

impl VideoQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::P360 => "360p",
            Self::P480 => "480p",
            Self::P720 => "720p",
            Self::P1080 => "1080p",
        }
    }

    pub fn target_height(self) -> u32 {
        match self {
            Self::Auto => 720,
            Self::P360 => 360,
            Self::P480 => 480,
            Self::P720 => 720,
            Self::P1080 => 1080,
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
    #[serde(default)]
    pub audio_quality: AudioQuality,
    #[serde(default)]
    pub video_quality: VideoQuality,
    #[serde(default)]
    pub close_to_tray: bool,
    #[serde(default)]
    pub output_device: Option<String>,
}

impl Default for PlaybackDspSettings {
    fn default() -> Self {
        Self {
            crossfade_ms: 6_000,
            fade_in_play_ms: 240,
            fade_curve: FadeCurve::BassSwapAutomix,
            gapless_album_lock: false,
            trim_silence: true,
            replay_gain: true,
            mono_audio: false,
            playback_speed: 1.0,
            sleep_timer_fade_out: true,
            audio_quality: AudioQuality::High,
            video_quality: VideoQuality::P720,
            close_to_tray: false,
            output_device: None,
        }
    }
}

impl PlaybackDspSettings {
    pub fn sanitized(&self) -> Self {
        let speed = if self.playback_speed.is_finite() {
            self.playback_speed.clamp(0.5, 2.0)
        } else {
            1.0
        };
        let output_device = self
            .output_device
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .map(ToOwned::to_owned);
        Self {
            crossfade_ms: self.crossfade_ms.min(12_000),
            fade_in_play_ms: self.fade_in_play_ms.min(2_000),
            fade_curve: self.fade_curve,
            gapless_album_lock: self.gapless_album_lock,
            trim_silence: self.trim_silence,
            replay_gain: self.replay_gain,
            mono_audio: self.mono_audio,
            playback_speed: speed,
            sleep_timer_fade_out: self.sleep_timer_fade_out,
            audio_quality: self.audio_quality,
            video_quality: self.video_quality,
            close_to_tray: self.close_to_tray,
            output_device,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscordStatusFormat {
    #[default]
    ArtistTitle,
    TrackTitle,
    Artist,
    AppName,
}

impl DiscordStatusFormat {
    pub fn next(self) -> Self {
        match self {
            Self::ArtistTitle => Self::TrackTitle,
            Self::TrackTitle => Self::Artist,
            Self::Artist => Self::AppName,
            Self::AppName => Self::ArtistTitle,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ArtistTitle => "Artist - Title",
            Self::TrackTitle => "Song Title",
            Self::Artist => "Artist Only",
            Self::AppName => "Ferric",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordRpcSettings {
    pub enabled: bool,
    pub client_id: String,
    pub status_format: DiscordStatusFormat,
    pub show_when_paused: bool,
    #[serde(default = "default_true")]
    pub show_cover_art: bool,
    #[serde(default = "default_true")]
    pub show_listen_button: bool,
    pub hide_details: bool,
}

impl Default for DiscordRpcSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            client_id: String::new(),
            status_format: DiscordStatusFormat::ArtistTitle,
            show_when_paused: false,
            show_cover_art: true,
            show_listen_button: true,
            hide_details: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountSettings {
    #[serde(default)]
    pub spotify_client_id: String,
    #[serde(default)]
    pub spotify_connected: bool,
    #[serde(default)]
    pub spotify_user_name: String,
    #[serde(default)]
    pub spotify_user_id: String,
    #[serde(default)]
    pub spotify_access_token: String,
    #[serde(default)]
    pub spotify_refresh_token: String,
    #[serde(default)]
    pub spotify_playlist_count: usize,
    #[serde(default)]
    pub spotify_liked_count: usize,
    #[serde(default)]
    pub spotify_web_connected: bool,
    #[serde(default)]
    pub spotify_web_user_name: String,
    #[serde(default)]
    pub spotify_web_access_token: String,
    #[serde(default)]
    pub spotify_web_cookie: String,
    #[serde(default)]
    pub youtube_client_id: String,
    #[serde(default)]
    pub youtube_client_secret: String,
    #[serde(default)]
    pub youtube_connected: bool,
    #[serde(default)]
    pub youtube_channel_name: String,
    #[serde(default)]
    pub youtube_account_id: String,
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

impl std::fmt::Debug for AccountSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |s: &str| if s.is_empty() { "" } else { "[REDACTED]" };
        f.debug_struct("AccountSettings")
            .field("spotify_client_id", &self.spotify_client_id)
            .field("spotify_connected", &self.spotify_connected)
            .field("spotify_user_name", &self.spotify_user_name)
            .field("spotify_user_id", &self.spotify_user_id)
            .field("spotify_access_token", &redact(&self.spotify_access_token))
            .field(
                "spotify_refresh_token",
                &redact(&self.spotify_refresh_token),
            )
            .field("spotify_playlist_count", &self.spotify_playlist_count)
            .field("spotify_liked_count", &self.spotify_liked_count)
            .field("spotify_web_connected", &self.spotify_web_connected)
            .field("spotify_web_user_name", &self.spotify_web_user_name)
            .field(
                "spotify_web_access_token",
                &redact(&self.spotify_web_access_token),
            )
            .field("spotify_web_cookie", &redact(&self.spotify_web_cookie))
            .field("youtube_client_id", &self.youtube_client_id)
            .field(
                "youtube_client_secret",
                &redact(&self.youtube_client_secret),
            )
            .field("youtube_connected", &self.youtube_connected)
            .field("youtube_channel_name", &self.youtube_channel_name)
            .field("youtube_account_id", &self.youtube_account_id)
            .field("youtube_access_token", &redact(&self.youtube_access_token))
            .field(
                "youtube_refresh_token",
                &redact(&self.youtube_refresh_token),
            )
            .field("youtube_playlist_count", &self.youtube_playlist_count)
            .field(
                "youtube_music_category_only",
                &self.youtube_music_category_only,
            )
            .field("youtube_cookie", &redact(&self.youtube_cookie))
            .field("youtube_sapisid", &redact(&self.youtube_sapisid))
            .field("youtube_visitor_data", &self.youtube_visitor_data)
            .field("subsonic_url", &self.subsonic_url)
            .field("subsonic_username", &self.subsonic_username)
            .field("subsonic_password", &redact(&self.subsonic_password))
            .field("subsonic_connected", &self.subsonic_connected)
            .finish()
    }
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            spotify_client_id: String::new(),
            spotify_connected: false,
            spotify_user_name: String::new(),
            spotify_user_id: String::new(),
            spotify_access_token: String::new(),
            spotify_refresh_token: String::new(),
            spotify_playlist_count: 0,
            spotify_liked_count: 0,
            spotify_web_connected: false,
            spotify_web_user_name: String::new(),
            spotify_web_access_token: String::new(),
            spotify_web_cookie: String::new(),
            youtube_client_id: String::new(),
            youtube_client_secret: String::new(),
            youtube_connected: false,
            youtube_channel_name: String::new(),
            youtube_account_id: String::new(),
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

impl AccountSettings {
    pub fn zeroize_spotify_direct_secrets(&mut self) {
        crate::secrets::zeroize_string(&mut self.spotify_access_token);
        crate::secrets::zeroize_string(&mut self.spotify_refresh_token);
    }

    pub fn zeroize_spotify_web_secrets(&mut self) {
        crate::secrets::zeroize_string(&mut self.spotify_web_access_token);
        crate::secrets::zeroize_string(&mut self.spotify_web_cookie);
    }

    pub fn zeroize_youtube_secrets(&mut self) {
        crate::secrets::zeroize_string(&mut self.youtube_client_secret);
        crate::secrets::zeroize_string(&mut self.youtube_access_token);
        crate::secrets::zeroize_string(&mut self.youtube_refresh_token);
        crate::secrets::zeroize_string(&mut self.youtube_cookie);
        crate::secrets::zeroize_string(&mut self.youtube_sapisid);
    }

    pub fn zeroize_subsonic_secrets(&mut self) {
        crate::secrets::zeroize_string(&mut self.subsonic_password);
    }
}

impl Drop for AccountSettings {
    fn drop(&mut self) {
        self.zeroize_spotify_direct_secrets();
        self.zeroize_spotify_web_secrets();
        self.zeroize_youtube_secrets();
        self.zeroize_subsonic_secrets();
    }
}

/// Persistent offline download metadata stored in `filesDir/ferric/offline_downloads/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineDownloadRecord {
    pub track_id: String,
    pub local_path: String,
    pub size_bytes: u64,
    pub codec: String,
    pub quality: String,
    pub downloaded_at: i64,
}

/// Timestamped qualified playback event persisted in `play_history`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayHistoryEntry {
    pub id: i64,
    pub track_id: String,
    pub artist: String,
    pub played_at: i64,
    pub listened_ms: u64,
}

/// Aggregated listening statistics and daily streak summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningStatsSummary {
    pub total_plays: u64,
    pub total_listened_ms: u64,
    pub current_streak_days: u32,
    pub longest_streak_days: u32,
    pub top_artists: Vec<(String, u64)>,
    pub top_track_ids: Vec<(String, u64)>,
}

/// True when a playback session qualifies to be logged in `play_history`:
/// listened `>= 30s`, or `>= 50%` of a short track (`< 60s`) with at least 5 seconds heard.
#[must_use]
pub fn qualifies_as_play(listened_ms: u64, duration_ms: u64) -> bool {
    if listened_ms >= 30_000 {
        return true;
    }
    if duration_ms > 0 && duration_ms < 60_000 && listened_ms >= 5_000 {
        return listened_ms.saturating_mul(2) >= duration_ms;
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
    1.0
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
    #[serde(default)]
    pub track_play_counts: HashMap<String, u32>,
    #[serde(default)]
    pub artist_play_counts: HashMap<String, u32>,
}

impl Default for SessionResumeState {
    fn default() -> Self {
        Self {
            last_track_id: None,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            total_tracks_played: 0,
            total_listened_secs: 0,
            recent_track_ids: Vec::new(),
            recent_searches: Vec::new(),
            pinned_ids: Vec::new(),
            autoplay_radio: true,
            track_play_counts: HashMap::new(),
            artist_play_counts: HashMap::new(),
        }
    }
}

impl SessionResumeState {
    pub fn sanitized(&self) -> Self {
        let mut out = self.clone();
        out.volume = if out.volume.is_finite() {
            out.volume.clamp(0.0, 1.5)
        } else {
            default_volume()
        };
        out.recent_track_ids.truncate(50);
        out.recent_searches.truncate(12);
        out.pinned_ids.truncate(16);
        if out.track_play_counts.len() > 200 {
            let mut entries: Vec<(String, u32)> = out.track_play_counts.into_iter().collect();
            entries.sort_by_key(|a| std::cmp::Reverse(a.1));
            entries.truncate(200);
            out.track_play_counts = entries.into_iter().collect();
        }
        if out.artist_play_counts.len() > 100 {
            let mut entries: Vec<(String, u32)> = out.artist_play_counts.into_iter().collect();
            entries.sort_by_key(|a| std::cmp::Reverse(a.1));
            entries.truncate(100);
            out.artist_play_counts = entries.into_iter().collect();
        }
        out
    }

    pub fn record_play(&mut self, track_id: &str, duration_ms: u64) {
        self.record_play_with_artist(track_id, "", duration_ms);
    }

    pub fn record_play_with_artist(&mut self, track_id: &str, artist: &str, duration_ms: u64) {
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
        let tc = self
            .track_play_counts
            .entry(track_id.to_string())
            .or_insert(0);
        *tc = tc.saturating_add(1);
        if self.track_play_counts.len() > 200
            && let Some(min_k) = self
                .track_play_counts
                .iter()
                .filter(|(k, _)| k.as_str() != track_id)
                .min_by_key(|(_, v)| **v)
                .map(|(k, _)| k.clone())
        {
            self.track_play_counts.remove(&min_k);
        }
        let primary_artist = primary_artist_key(artist);
        if !primary_artist.is_empty() {
            let ac = self
                .artist_play_counts
                .entry(primary_artist.clone())
                .or_insert(0);
            *ac = ac.saturating_add(1);
            if self.artist_play_counts.len() > 100
                && let Some(min_a) = self
                    .artist_play_counts
                    .iter()
                    .filter(|(k, _)| k.as_str() != primary_artist.as_str())
                    .min_by_key(|(_, v)| **v)
                    .map(|(k, _)| k.clone())
            {
                self.artist_play_counts.remove(&min_a);
            }
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

pub fn primary_artist_key(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("unknown artist")
        || trimmed.eq_ignore_ascii_case("youtube music")
    {
        return String::new();
    }
    let first = trimmed
        .split(['&', ',', '•', '·'])
        .next()
        .unwrap_or(trimmed)
        .split(" feat.")
        .next()
        .unwrap_or(trimmed)
        .split(" ft.")
        .next()
        .unwrap_or(trimmed)
        .split(" x ")
        .next()
        .unwrap_or(trimmed)
        .trim();
    first.to_string()
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
            .position(|h| crate::text::starts_with_ascii_icase(h, "artist name"));
        let idx_dur = headers
            .iter()
            .position(|h| crate::text::starts_with_ascii_icase(h, "duration"));
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
            let uri = idx_uri
                .and_then(|i| cols.get(i))
                .copied()
                .unwrap_or("")
                .trim();
            let isrc = idx_isrc
                .and_then(|i| cols.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let valid_spotify_id = uri
                .strip_prefix("spotify:track:")
                .filter(|id| id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()));
            let id = if let Some(sp_id) = valid_spotify_id {
                format!("spotify:track:{sp_id}")
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
                spotify_id: valid_spotify_id.map(ToOwned::to_owned),
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
                pending_meta = Some((secs.saturating_mul(1_000), artist, title));
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            let Some(resolved) = sanitize_m3u_local_path(line, base_dir) else {
                pending_meta = None;
                continue;
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

/// Reject UNC / Windows device paths, NTFS Alternate Data Streams, drive-relative/drive-rooted
/// ambiguities, symlinks, non-audio extensions, URL schemes, control characters, and `..` traversal
/// so importing an untrusted `.m3u` / `.m3u8` cannot trigger SMB Net-NTLMv2 leaks or arbitrary file reads.
fn sanitize_m3u_local_path(raw_line: &str, base_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    use std::path::{Component, Prefix};

    const ALLOWED_AUDIO_EXTS: &[&str] = &[
        "flac", "mp3", "m4a", "aac", "alac", "ogg", "opus", "wav", "aiff", "aif", "wma", "webm",
        "mka",
    ];

    let s = raw_line.trim();
    if s.is_empty() || s.bytes().any(|b| b.is_ascii_control()) {
        return None;
    }
    if crate::text::starts_with_ascii_icase(s, "http://")
        || crate::text::starts_with_ascii_icase(s, "https://")
        || crate::text::starts_with_ascii_icase(s, "file://")
        || crate::text::starts_with_ascii_icase(s, "ftp://")
        || s.starts_with(r"\\")
        || s.starts_with("//")
    {
        return None;
    }

    let p = PathBuf::from(s);
    let mut comps = p.components().peekable();
    let mut saw_disk_prefix = false;
    while let Some(comp) = comps.next() {
        match comp {
            Component::Prefix(pref) => match pref.kind() {
                Prefix::UNC(_, _)
                | Prefix::VerbatimUNC(_, _)
                | Prefix::DeviceNS(_)
                | Prefix::Verbatim(_) => return None,
                Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {
                    saw_disk_prefix = true;
                    // Reject drive-relative paths like `C:foo.mp3` (must be followed by root `\`).
                    if !matches!(comps.peek(), Some(Component::RootDir)) {
                        return None;
                    }
                }
            },
            Component::RootDir => {
                // On Windows, reject drive-rooted paths without a drive prefix (e.g. `\Windows\...`).
                if cfg!(target_os = "windows") && !saw_disk_prefix {
                    return None;
                }
            }
            Component::ParentDir => return None,
            Component::Normal(seg) => {
                let seg_str = seg.to_str()?;
                // Reject NTFS Alternate Data Streams (`file.mp3:zone`) or embedded colons.
                if seg_str.contains(':') {
                    return None;
                }
                // Reject Windows reserved device names (`CON`, `NUL`, `AUX`, `PRN`, `COM1..9`, `LPT1..9`).
                let stem = seg_str
                    .split('.')
                    .next()
                    .unwrap_or(seg_str)
                    .trim_end_matches([' ', '.'])
                    .to_ascii_uppercase();
                if matches!(
                    stem.as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                ) {
                    return None;
                }
            }
            Component::CurDir => {}
        }
    }

    let ext = p.extension().and_then(|e| e.to_str())?;
    if !ALLOWED_AUDIO_EXTS
        .iter()
        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
    {
        return None;
    }

    let resolved = if p.is_absolute() {
        p
    } else if let Some(base) = base_dir {
        base.join(&p)
    } else {
        p
    };

    let mut check_prefix = PathBuf::new();
    for comp in resolved.components() {
        check_prefix.push(comp);
        if matches!(comp, Component::Normal(_))
            && let Ok(meta) = std::fs::symlink_metadata(&check_prefix)
            && meta.file_type().is_symlink()
        {
            return None;
        }
    }
    if let Ok(canon) = std::fs::canonicalize(&resolved) {
        let canon_str = canon.to_string_lossy();
        if canon_str.starts_with(r"\\?\UNC\") || canon_str.starts_with(r"\\?\unc\") {
            return None;
        }
    }
    Some(resolved)
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
        assert!(!dsp.gapless_album_lock);
        assert!(dsp.trim_silence);
        assert_eq!(dsp.audio_quality, AudioQuality::High);
        assert_eq!(dsp.video_quality, VideoQuality::P720);
        assert!(!dsp.close_to_tray);
        assert_eq!(FadeCurve::all().len(), 3);
    }
}
