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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSettings {
    pub spotify_client_id: String,
    pub spotify_connected: bool,
    pub youtube_client_id: String,
    pub youtube_connected: bool,
    pub youtube_music_category_only: bool,
    pub subsonic_url: String,
}

impl Default for AccountSettings {
    fn default() -> Self {
        Self {
            spotify_client_id: String::new(),
            spotify_connected: false,
            youtube_client_id: String::new(),
            youtube_connected: false,
            youtube_music_category_only: true,
            subsonic_url: String::new(),
        }
    }
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
}
