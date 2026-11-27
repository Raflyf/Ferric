//! M3 UniFFI types bridging Synced Lyrics, Video Clip streams, and AutoEq profiles to Kotlin.

use crate::FfiVideoQuality;
use ferric_controller::types::{
    AutoEqProfileSnapshot, LyricLineSnapshot, LyricsSnapshot, VideoStreamOptionSnapshot,
    VideoStreamsSnapshot,
};

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiLyricLine {
    pub timestamp_ms: u64,
    pub text: String,
}

impl From<LyricLineSnapshot> for FfiLyricLine {
    fn from(l: LyricLineSnapshot) -> Self {
        Self {
            timestamp_ms: l.timestamp_ms,
            text: l.text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiLyricsState {
    pub track_id: String,
    pub lines: Vec<FfiLyricLine>,
    pub source_name: String,
    pub is_synced: bool,
    pub offset_ms: i64,
    pub is_loading: bool,
}

impl From<LyricsSnapshot> for FfiLyricsState {
    fn from(s: LyricsSnapshot) -> Self {
        Self {
            track_id: s.track_id,
            lines: s.lines.into_iter().map(Into::into).collect(),
            source_name: s.source_name,
            is_synced: s.is_synced,
            offset_ms: s.offset_ms,
            is_loading: s.is_loading,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiVideoStreamOption {
    pub quality: FfiVideoQuality,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: u32,
    pub url: String,
    pub mime: String,
}

impl From<VideoStreamOptionSnapshot> for FfiVideoStreamOption {
    fn from(o: VideoStreamOptionSnapshot) -> Self {
        Self {
            quality: o.quality.into(),
            label: o.label,
            width: o.width,
            height: o.height,
            bitrate_kbps: o.bitrate_kbps,
            url: o.url,
            mime: o.mime,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiVideoStreamsState {
    pub track_id: String,
    pub video_id: String,
    pub is_official_mv: bool,
    pub options: Vec<FfiVideoStreamOption>,
    pub is_loading: bool,
}

impl From<VideoStreamsSnapshot> for FfiVideoStreamsState {
    fn from(v: VideoStreamsSnapshot) -> Self {
        Self {
            track_id: v.track_id,
            video_id: v.video_id,
            is_official_mv: v.is_official_mv,
            options: v.options.into_iter().map(Into::into).collect(),
            is_loading: v.is_loading,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiAutoEqProfile {
    pub name: String,
    pub category: String,
    pub preamp_db: f32,
}

impl From<AutoEqProfileSnapshot> for FfiAutoEqProfile {
    fn from(p: AutoEqProfileSnapshot) -> Self {
        Self {
            name: p.name,
            category: p.category,
            preamp_db: p.preamp_db,
        }
    }
}
