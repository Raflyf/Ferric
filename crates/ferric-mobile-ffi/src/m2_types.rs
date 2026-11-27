//! M2 UniFFI record and enum types bridging `ferric-controller` Home, Search, Library,
//! Detail, Queue, Local Scan, and Account status snapshots to Kotlin.

use crate::{FfiTrack, FfiTrackSource};
use ferric_controller::types::{
    AccountStatusSnapshot, AlbumSummary, ArtistSummary, CardKind, CollectionDetailSnapshot,
    HomeShelfSnapshot, HomeSnapshot, LocalScanEntry, MoodChipSnapshot, SearchSnapshot,
    ShelfCardSnapshot, StationCardSnapshot,
};
use ferric_core::collections::{DetailSortOrder, PlaylistSortOrder, PlaylistSourceFilter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiCardKind {
    Song,
    Album,
    Playlist,
    Artist,
}

impl From<CardKind> for FfiCardKind {
    fn from(k: CardKind) -> Self {
        match k {
            CardKind::Song => Self::Song,
            CardKind::Album => Self::Album,
            CardKind::Playlist => Self::Playlist,
            CardKind::Artist => Self::Artist,
        }
    }
}

impl From<FfiCardKind> for CardKind {
    fn from(k: FfiCardKind) -> Self {
        match k {
            FfiCardKind::Song => Self::Song,
            FfiCardKind::Album => Self::Album,
            FfiCardKind::Playlist => Self::Playlist,
            FfiCardKind::Artist => Self::Artist,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiPlaylistSort {
    Newest,
    Oldest,
    NameAz,
}

impl From<FfiPlaylistSort> for PlaylistSortOrder {
    fn from(s: FfiPlaylistSort) -> Self {
        match s {
            FfiPlaylistSort::Newest => Self::Newest,
            FfiPlaylistSort::Oldest => Self::Oldest,
            FfiPlaylistSort::NameAz => Self::NameAZ,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiPlaylistSourceFilter {
    All,
    YouTube,
    Spotify,
    Local,
}

impl From<FfiPlaylistSourceFilter> for PlaylistSourceFilter {
    fn from(f: FfiPlaylistSourceFilter) -> Self {
        match f {
            FfiPlaylistSourceFilter::All => Self::All,
            FfiPlaylistSourceFilter::YouTube => Self::YouTube,
            FfiPlaylistSourceFilter::Spotify => Self::Spotify,
            FfiPlaylistSourceFilter::Local => Self::Local,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiDetailSort {
    Original,
    Title,
    Artist,
    Duration,
}

impl From<FfiDetailSort> for DetailSortOrder {
    fn from(s: FfiDetailSort) -> Self {
        match s {
            FfiDetailSort::Original => Self::Original,
            FfiDetailSort::Title => Self::Title,
            FfiDetailSort::Artist => Self::Artist,
            FfiDetailSort::Duration => Self::Duration,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiMoodChip {
    pub title: String,
    pub params: String,
}

impl From<MoodChipSnapshot> for FfiMoodChip {
    fn from(c: MoodChipSnapshot) -> Self {
        Self {
            title: c.title,
            params: c.params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiShelfCard {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: FfiCardKind,
    pub browse_id: Option<String>,
    pub video_id: Option<String>,
    pub thumbnail_url: Option<String>,
}

impl From<ShelfCardSnapshot> for FfiShelfCard {
    fn from(c: ShelfCardSnapshot) -> Self {
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

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiHomeShelf {
    pub title: String,
    pub cards: Vec<FfiShelfCard>,
}

impl From<HomeShelfSnapshot> for FfiHomeShelf {
    fn from(s: HomeShelfSnapshot) -> Self {
        Self {
            title: s.title,
            cards: s.cards.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiStationCard {
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

impl From<StationCardSnapshot> for FfiStationCard {
    fn from(s: StationCardSnapshot) -> Self {
        Self {
            id: s.id,
            title: s.title,
            display_name: s.display_name,
            subtitle: s.subtitle,
            palette_idx: s.palette_idx,
            thumbnail_url: s.thumbnail_url,
            left_art_url: s.left_art_url,
            right_art_url: s.right_art_url,
            is_spotify: s.is_spotify,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiHomeState {
    pub chips: Vec<FfiMoodChip>,
    pub selected_mood_idx: Option<u32>,
    pub quick_picks: Vec<FfiTrack>,
    pub stations: Vec<FfiStationCard>,
    pub shelves: Vec<FfiHomeShelf>,
    pub is_loading: bool,
    pub is_loading_more: bool,
}

impl From<HomeSnapshot> for FfiHomeState {
    fn from(h: HomeSnapshot) -> Self {
        Self {
            chips: h.chips.into_iter().map(Into::into).collect(),
            selected_mood_idx: h.selected_mood_idx.and_then(|i| u32::try_from(i).ok()),
            quick_picks: h.quick_picks.into_iter().map(Into::into).collect(),
            stations: h.stations.into_iter().map(Into::into).collect(),
            shelves: h.shelves.into_iter().map(Into::into).collect(),
            is_loading: h.is_loading,
            is_loading_more: h.is_loading_more,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSearchResults {
    pub query: String,
    pub local_tracks: Vec<FfiTrack>,
    pub online_tracks: Vec<FfiTrack>,
    pub online_cards: Vec<FfiShelfCard>,
    pub is_loading: bool,
}

impl From<SearchSnapshot> for FfiSearchResults {
    fn from(s: SearchSnapshot) -> Self {
        Self {
            query: s.query,
            local_tracks: s.local_tracks.into_iter().map(Into::into).collect(),
            online_tracks: s.online_tracks.into_iter().map(Into::into).collect(),
            online_cards: s.online_cards.into_iter().map(Into::into).collect(),
            is_loading: s.is_loading,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiCollectionDetail {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub kind: FfiCardKind,
    pub source: FfiTrackSource,
    pub thumbnail_url: Option<String>,
    pub share_url: Option<String>,
    pub can_sync_to_account: bool,
    pub is_editable_playlist: bool,
    pub tracks: Vec<FfiTrack>,
}

impl From<CollectionDetailSnapshot> for FfiCollectionDetail {
    fn from(d: CollectionDetailSnapshot) -> Self {
        Self {
            id: d.id,
            title: d.title,
            subtitle: d.subtitle,
            description: d.description,
            kind: d.kind.into(),
            source: d.source.into(),
            thumbnail_url: d.thumbnail_url,
            share_url: d.share_url,
            can_sync_to_account: d.can_sync_to_account,
            is_editable_playlist: d.is_editable_playlist,
            tracks: d.tracks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiAlbumSummary {
    pub name: String,
    pub artist: String,
    pub track_count: u32,
    pub thumbnail_url: Option<String>,
}

impl From<AlbumSummary> for FfiAlbumSummary {
    fn from(a: AlbumSummary) -> Self {
        Self {
            name: a.name,
            artist: a.artist,
            track_count: u32::try_from(a.track_count).unwrap_or(u32::MAX),
            thumbnail_url: a.thumbnail_url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiArtistSummary {
    pub name: String,
    pub track_count: u32,
    pub album_count: u32,
    pub thumbnail_url: Option<String>,
}

impl From<ArtistSummary> for FfiArtistSummary {
    fn from(a: ArtistSummary) -> Self {
        Self {
            name: a.name,
            track_count: u32::try_from(a.track_count).unwrap_or(u32::MAX),
            album_count: u32::try_from(a.album_count).unwrap_or(u32::MAX),
            thumbnail_url: a.thumbnail_url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiAccountStatus {
    pub spotify_connected: bool,
    pub spotify_user_name: String,
    pub spotify_user_id: String,
    pub spotify_playlist_count: u32,
    pub spotify_liked_count: u32,
    pub youtube_connected: bool,
    pub youtube_channel_name: String,
    pub youtube_account_id: String,
    pub youtube_playlist_count: u32,
    pub subsonic_connected: bool,
    pub subsonic_url: String,
    pub subsonic_username: String,
}

impl From<AccountStatusSnapshot> for FfiAccountStatus {
    fn from(a: AccountStatusSnapshot) -> Self {
        Self {
            spotify_connected: a.spotify_connected,
            spotify_user_name: a.spotify_user_name,
            spotify_user_id: a.spotify_user_id,
            spotify_playlist_count: u32::try_from(a.spotify_playlist_count).unwrap_or(u32::MAX),
            spotify_liked_count: u32::try_from(a.spotify_liked_count).unwrap_or(u32::MAX),
            youtube_connected: a.youtube_connected,
            youtube_channel_name: a.youtube_channel_name,
            youtube_account_id: a.youtube_account_id,
            youtube_playlist_count: u32::try_from(a.youtube_playlist_count).unwrap_or(u32::MAX),
            subsonic_connected: a.subsonic_connected,
            subsonic_url: a.subsonic_url,
            subsonic_username: a.subsonic_username,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiLocalScanEntry {
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

impl From<FfiLocalScanEntry> for LocalScanEntry {
    fn from(e: FfiLocalScanEntry) -> Self {
        Self {
            uri: e.uri,
            title: e.title,
            artist: e.artist,
            album: e.album,
            duration_ms: e.duration_ms,
            codec: e.codec,
            sample_rate: e.sample_rate,
            bit_depth: e.bit_depth,
            bitrate_kbps: e.bitrate_kbps,
        }
    }
}
