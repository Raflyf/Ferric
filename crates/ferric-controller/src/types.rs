//! Shared immutable snapshots and input DTOs produced/consumed by [`crate::FerricController`].

use ferric_core::models::{Track, TrackSource, VideoQuality};
use ferric_providers::ytmusic::{
    RecommendedStation, YtmCardKind, YtmHomeShelf, YtmMoodChip, YtmShelfCard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Song,
    Album,
    Playlist,
    Artist,
}

impl From<YtmCardKind> for CardKind {
    fn from(k: YtmCardKind) -> Self {
        match k {
            YtmCardKind::Song => Self::Song,
            YtmCardKind::Album => Self::Album,
            YtmCardKind::Playlist => Self::Playlist,
            YtmCardKind::Artist => Self::Artist,
        }
    }
}

impl From<CardKind> for YtmCardKind {
    fn from(k: CardKind) -> Self {
        match k {
            CardKind::Song => Self::Song,
            CardKind::Album => Self::Album,
            CardKind::Playlist => Self::Playlist,
            CardKind::Artist => Self::Artist,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoodChipSnapshot {
    pub title: String,
    pub params: String,
}

impl From<YtmMoodChip> for MoodChipSnapshot {
    fn from(c: YtmMoodChip) -> Self {
        Self {
            title: c.title,
            params: c.params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfCardSnapshot {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: CardKind,
    pub browse_id: Option<String>,
    pub video_id: Option<String>,
    pub thumbnail_url: Option<String>,
}

impl From<YtmShelfCard> for ShelfCardSnapshot {
    fn from(c: YtmShelfCard) -> Self {
        Self {
            id: c.id,
            title: c.title,
            subtitle: c.subtitle,
            kind: c.kind.into(),
            browse_id: c.browse_id,
            video_id: c.video_id,
            thumbnail_url: c.thumbnail_url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeShelfSnapshot {
    pub title: String,
    pub cards: Vec<ShelfCardSnapshot>,
}

impl From<YtmHomeShelf> for HomeShelfSnapshot {
    fn from(s: YtmHomeShelf) -> Self {
        Self {
            title: s.title,
            cards: s.cards.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationCardSnapshot {
    pub id: String,
    pub title: String,
    pub display_name: String,
    pub subtitle: String,
    pub palette_idx: u8,
    pub thumbnail_url: Option<String>,
    pub left_art_url: Option<String>,
    pub right_art_url: Option<String>,
    pub is_spotify: bool,
}

impl From<&RecommendedStation> for StationCardSnapshot {
    fn from(s: &RecommendedStation) -> Self {
        Self {
            id: s.id.clone(),
            title: s.title.clone(),
            display_name: s.display_name.clone(),
            subtitle: s.subtitle.clone(),
            palette_idx: s.palette_idx,
            thumbnail_url: s.center_art_url.clone(),
            left_art_url: s.left_art_url.clone(),
            right_art_url: s.right_art_url.clone(),
            is_spotify: s.is_spotify_origin,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HomeSnapshot {
    pub chips: Vec<MoodChipSnapshot>,
    pub selected_mood_idx: Option<usize>,
    pub quick_picks: Vec<Track>,
    pub stations: Vec<StationCardSnapshot>,
    pub shelves: Vec<HomeShelfSnapshot>,
    pub is_loading: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SearchSnapshot {
    pub query: String,
    pub local_tracks: Vec<Track>,
    pub online_tracks: Vec<Track>,
    pub online_cards: Vec<ShelfCardSnapshot>,
    pub is_loading: bool,
}

#[derive(Debug, Clone)]
pub struct CollectionDetailSnapshot {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub kind: CardKind,
    pub source: TrackSource,
    pub thumbnail_url: Option<String>,
    pub share_url: Option<String>,
    pub can_sync_to_account: bool,
    pub is_editable_playlist: bool,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumSummary {
    pub name: String,
    pub artist: String,
    pub track_count: usize,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtistSummary {
    pub name: String,
    pub track_count: usize,
    pub album_count: usize,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountStatusSnapshot {
    pub spotify_connected: bool,
    pub spotify_user_name: String,
    pub spotify_user_id: String,
    pub spotify_playlist_count: usize,
    pub spotify_liked_count: usize,
    pub youtube_connected: bool,
    pub youtube_channel_name: String,
    pub youtube_account_id: String,
    pub youtube_playlist_count: usize,
    pub subsonic_connected: bool,
    pub subsonic_url: String,
    pub subsonic_username: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalScanEntry {
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricLineSnapshot {
    pub timestamp_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricsSnapshot {
    pub track_id: String,
    pub lines: Vec<LyricLineSnapshot>,
    pub source_name: String,
    pub is_synced: bool,
    pub offset_ms: i64,
    pub is_loading: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoStreamOptionSnapshot {
    pub quality: VideoQuality,
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: u32,
    pub url: String,
    pub mime: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoStreamsSnapshot {
    pub track_id: String,
    pub video_id: String,
    pub is_official_mv: bool,
    pub options: Vec<VideoStreamOptionSnapshot>,
    pub is_loading: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutoEqProfileSnapshot {
    pub name: String,
    pub category: String,
    pub preamp_db: f32,
}
