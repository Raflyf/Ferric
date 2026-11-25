use crate::google_login::GoogleLoginOutcome;
use ferric_core::models::{Playlist, Track};
use ferric_providers::lyrics::SyncedLyrics;
use ferric_providers::updater::{DownloadEvent, ReleaseInfo};
use ferric_providers::ytmusic::{
    CachedAudioStream, VideoStreamOption, YtmAccountLibrary, YtmBrowseDetail, YtmCardKind,
    YtmRichHome, YtmRichSearch,
};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MainPage {
    #[default]
    Home,
    Search,
    Recents,
    Songs,
    Albums,
    Artists,
    Playlists,
    Detail,
    Settings,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsTab {
    #[default]
    Accounts,
    Playback,
    Equalizer,
    Appearance,
    Integrations,
    Storage,
    Connect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LibraryFilter {
    #[default]
    All,
    Liked,
    Local,
    Spotify,
    YouTube,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Albums,
    Artists,
    Playlists,
    Local,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortField {
    #[default]
    Recent,
    Artist,
    Title,
    Album,
    Duration,
}

impl SortField {
    pub fn next(self) -> Self {
        match self {
            Self::Recent => Self::Artist,
            Self::Artist => Self::Title,
            Self::Title => Self::Album,
            Self::Album => Self::Duration,
            Self::Duration => Self::Recent,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Recent => "Sort: Recent",
            Self::Artist => "Sort: Artist",
            Self::Title => "Sort: Title",
            Self::Album => "Sort: Album",
            Self::Duration => "Sort: Duration",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SleepTimerMode {
    #[default]
    Off,
    Deadline(Instant),
    EndOfTrack,
}

#[derive(Debug, Clone)]
pub struct CollectionGroup {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub kind: YtmCardKind,
    pub thumbnail_url: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Clone)]
pub struct DetailViewState {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub kind: YtmCardKind,
    pub thumbnail_url: Option<String>,
    pub loading: bool,
    pub tracks: Vec<Track>,
}

impl Default for DetailViewState {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: "Collection".to_string(),
            subtitle: String::new(),
            description: String::new(),
            kind: YtmCardKind::Playlist,
            thumbnail_url: None,
            loading: false,
            tracks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NavEntry {
    pub page: MainPage,
    pub detail: Option<DetailViewState>,
}

pub struct NavHistory {
    entries: Vec<NavEntry>,
    cursor: usize,
}

impl NavHistory {
    pub fn new(initial: MainPage) -> Self {
        Self {
            entries: vec![NavEntry {
                page: initial,
                detail: None,
            }],
            cursor: 0,
        }
    }

    pub fn push(&mut self, page: MainPage, detail: Option<DetailViewState>) {
        if let Some(cur) = self.entries.get(self.cursor) {
            let same_page = cur.page == page;
            let same_detail = cur.detail.as_ref().map(|d| d.id.as_str())
                == detail.as_ref().map(|d| d.id.as_str());
            if same_page && (page != MainPage::Detail || same_detail) {
                if page == MainPage::Detail
                    && let Some(slot) = self.entries.get_mut(self.cursor)
                {
                    slot.detail = detail;
                }
                return;
            }
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(NavEntry { page, detail });
        if self.entries.len() > 32 {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len().saturating_sub(1);
    }

    pub fn update_current_detail(&mut self, detail: &DetailViewState) {
        if let Some(cur) = self.entries.get_mut(self.cursor)
            && cur.page == MainPage::Detail
            && cur.detail.as_ref().is_some_and(|d| d.id == detail.id)
        {
            cur.detail = Some(detail.clone());
        }
    }

    pub fn prune_detail_id(&mut self, detail_id: &str) {
        let mut i = 0;
        while i < self.entries.len() {
            let matches = self.entries[i].page == MainPage::Detail
                && self.entries[i]
                    .detail
                    .as_ref()
                    .is_some_and(|d| d.id == detail_id);
            if matches && self.entries.len() > 1 {
                self.entries.remove(i);
                if i <= self.cursor && self.cursor > 0 {
                    self.cursor -= 1;
                }
            } else {
                i += 1;
            }
        }
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
    }

    pub fn can_back(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    pub fn go_back(&mut self) -> Option<NavEntry> {
        if self.can_back() {
            self.cursor -= 1;
            self.entries.get(self.cursor).cloned()
        } else {
            None
        }
    }

    pub fn go_forward(&mut self) -> Option<NavEntry> {
        if self.can_forward() {
            self.cursor += 1;
            self.entries.get(self.cursor).cloned()
        } else {
            None
        }
    }
}

pub enum BgMessage {
    ScanCompleted(Vec<Track>),
    LyricsLoaded {
        track_id: String,
        lrc_raw: String,
        parsed: SyncedLyrics,
    },
    YtmRichHomeLoaded {
        mood_index: usize,
        home: Box<YtmRichHome>,
    },
    YtmRichHomeFailed(String),
    YtmHomeMoreLoaded(Box<YtmRichHome>),
    YtmHomeMoreFailed(String),
    YtmBrowseDetailLoaded(Box<YtmBrowseDetail>),
    YtmBrowseDetailFailed {
        browse_id: String,
        error: String,
    },
    YtmRichSearchCompleted {
        seq: u64,
        query: String,
        result: Box<YtmRichSearch>,
    },
    YtmSearchFailed {
        seq: u64,
        query: String,
        error: String,
    },
    RadioLoaded {
        seed_track_id: String,
        seed_title: String,
        tracks: Vec<Track>,
    },
    GoogleLoginResult(GoogleLoginOutcome),
    YtmAccountSynced(Box<YtmAccountLibrary>),
    YtmAccountSyncFailed(String),
    StreamReady {
        track_id: String,
        video_id: String,
        stream: CachedAudioStream,
        auto_play: bool,
    },
    StreamFailed {
        track_id: String,
        error: String,
        auto_play: bool,
    },
    VideoStreamsLoaded {
        track_id: String,
        video_id: String,
        is_official_mv: bool,
        streams: Vec<VideoStreamOption>,
    },
    VideoStreamsFailed {
        track_id: String,
        error: String,
    },
    PublicSyncCompleted {
        provider: &'static str,
        payload: Box<(Playlist, Vec<Track>)>,
    },
    PublicSyncFailed {
        provider: &'static str,
        error: String,
    },
    SubsonicSynced {
        tracks: Vec<Track>,
        playlists: Vec<(Playlist, Vec<Track>)>,
    },
    SubsonicFailed(String),
    UpdateChecked {
        manual: bool,
        result: Result<Option<ReleaseInfo>, String>,
    },
    UpdateDownload(DownloadEvent),
}

fn resolved_track_thumb(track: &Track) -> Option<String> {
    if let Some(url) = track
        .thumbnail_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(url.to_string());
    }
    if let Some(vid) = track
        .youtube_video_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Some(format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"));
    }
    if let Some(vid) = track.id.strip_prefix("yt:video:").filter(|s| !s.is_empty()) {
        return Some(format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"));
    }
    None
}

fn is_bogus_album_label(raw: &str) -> bool {
    let low = raw.trim().to_ascii_lowercase();
    if low.is_empty() {
        return true;
    }
    matches!(
        low.as_str(),
        "unknown album"
            | "youtube music"
            | "youtube music radio"
            | "music video"
            | "trending"
            | "quick picks"
            | "single"
            | "singles"
            | "discover"
            | "radio"
    ) || low.contains(" likes")
        || low.contains(" views")
        || low.contains(" plays")
        || low.contains(" watching")
        || low.contains(" subscribers")
}

/// Group all library tracks into visual Album cards with real cover artwork.
pub fn build_album_groups(tracks: &[Track]) -> Vec<CollectionGroup> {
    let mut map: HashMap<String, (bool, CollectionGroup)> = HashMap::new();
    for track in tracks {
        let raw_album = track.album.trim();
        let (is_single, album_name) = if is_bogus_album_label(raw_album) {
            let fallback = track.title.trim();
            if fallback.is_empty() {
                continue;
            }
            (true, fallback.to_string())
        } else {
            (false, raw_album.to_string())
        };
        let key = format!(
            "{}::{}",
            album_name.to_ascii_lowercase(),
            track.artist.trim().to_ascii_lowercase()
        );
        let thumb = resolved_track_thumb(track);
        let entry = map.entry(key).or_insert_with(|| {
            (
                is_single,
                CollectionGroup {
                    id: format!("album:{}", album_name.to_ascii_lowercase()),
                    title: album_name.clone(),
                    subtitle: track.artist.clone(),
                    kind: YtmCardKind::Album,
                    thumbnail_url: thumb.clone(),
                    tracks: Vec::new(),
                },
            )
        });
        if entry.1.thumbnail_url.is_none() && thumb.is_some() {
            entry.1.thumbnail_url = thumb;
        }
        entry.1.tracks.push(track.clone());
    }
    let mut out: Vec<CollectionGroup> = map
        .into_values()
        .map(|(is_single, mut g)| {
            g.subtitle = if is_single && g.tracks.len() == 1 {
                format!("{} · Single", g.subtitle)
            } else {
                format!("{} · {} songs", g.subtitle, g.tracks.len())
            };
            g
        })
        .collect();
    out.sort_by(|a, b| {
        b.tracks.len().cmp(&a.tracks.len()).then_with(|| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
        })
    });
    out
}

/// Group all library tracks into circular Artist cards with real artwork.
pub fn build_artist_groups(tracks: &[Track]) -> Vec<CollectionGroup> {
    let mut map: HashMap<String, CollectionGroup> = HashMap::new();
    for track in tracks {
        let artist_name = if track.artist.trim().is_empty() {
            "Unknown Artist".to_string()
        } else {
            track.artist.trim().to_string()
        };
        let key = artist_name.to_ascii_lowercase();
        let thumb = resolved_track_thumb(track);
        let entry = map.entry(key.clone()).or_insert_with(|| CollectionGroup {
            id: format!("artist:{key}"),
            title: artist_name.clone(),
            subtitle: String::new(),
            kind: YtmCardKind::Artist,
            thumbnail_url: thumb.clone(),
            tracks: Vec::new(),
        });
        if entry.thumbnail_url.is_none() && thumb.is_some() {
            entry.thumbnail_url = thumb;
        }
        entry.tracks.push(track.clone());
    }
    let mut out: Vec<CollectionGroup> = map.into_values().collect();
    for g in &mut out {
        g.subtitle = if g.tracks.len() == 1 {
            "1 song".to_string()
        } else {
            format!("{} songs", g.tracks.len())
        };
    }
    out.sort_by(|a, b| {
        b.tracks.len().cmp(&a.tracks.len()).then_with(|| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
        })
    });
    out
}

/// Truncate text cleanly at UTF-8 char boundaries while retaining all scripts supported by
/// bundled Inter v4.1 (Latin, Latin Extended / Vietnamese, Greek, Cyrillic, and standard symbols)
/// and filtering unsupported emoji/private-use blocks so cards stay single-line with zero tofu boxes.
pub fn clip_text(raw: &str, max_chars: usize) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            }
        })
        .filter(|&c| {
            let cp = c as u32;
            (0x0020..=0x052F).contains(&cp) || (0x1E00..=0x25FF).contains(&cp)
        })
        .collect();
    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let final_str = if trimmed.is_empty() {
        let raw_single = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if raw_single.is_empty() {
            "Unknown".to_string()
        } else {
            raw_single
        }
    } else {
        trimmed
    };
    if final_str.chars().count() <= max_chars {
        return final_str;
    }
    let take = max_chars.saturating_sub(1);
    let mut out: String = final_str.chars().take(take).collect();
    out.push('…');
    out
}

/// Compute total size in bytes of files inside a cache directory (non-recursive).
pub fn dir_size_bytes(dir: &Path) -> u64 {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in read_dir.flatten() {
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

pub fn format_bytes_mb(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
