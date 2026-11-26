use crate::google_login::GoogleLoginOutcome;
use ferric_core::models::{Playlist, Track};
use ferric_providers::lyrics::SyncedLyrics;
use ferric_providers::updater::{DownloadEvent, ReleaseInfo};
use ferric_providers::ytmusic::{
    CachedAudioStream, RecommendedStation, VideoStreamOption, YtmAccountLibrary, YtmBrowseDetail,
    YtmCardKind, YtmRichHome, YtmRichSearch,
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
pub enum PlaylistSortOrder {
    #[default]
    Newest,
    Oldest,
    NameAZ,
}

impl PlaylistSortOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::NameAZ => "A-Z",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Newest => Self::Oldest,
            Self::Oldest => Self::NameAZ,
            Self::NameAZ => Self::Newest,
        }
    }

    /// Order playlists that come from SQLite (`list_playlists`, already sorted Newest -> Oldest).
    pub fn apply(self, playlists: &mut [Playlist]) {
        match self {
            Self::Newest => {}
            Self::Oldest => playlists.reverse(),
            Self::NameAZ => {
                playlists.sort_by(|a, b| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaylistSourceFilter {
    #[default]
    All,
    YouTube,
    Spotify,
    SpotifyPremium,
    Local,
}

impl PlaylistSourceFilter {
    pub fn matches_playlist(self, pl: &Playlist) -> bool {
        use ferric_core::models::TrackSource;
        match self {
            Self::All => true,
            Self::YouTube => pl.source == TrackSource::YouTubeMusic,
            Self::Spotify => {
                pl.source == TrackSource::Spotify && !pl.id.starts_with("spotify:premium:")
            }
            Self::SpotifyPremium => pl.id.starts_with("spotify:premium:"),
            Self::Local => matches!(
                pl.source,
                TrackSource::Local | TrackSource::Subsonic | TrackSource::P2pHome
            ),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::YouTube => "YouTube",
            Self::Spotify => "Spotify",
            Self::SpotifyPremium => "Spotify Premium",
            Self::Local => "Local",
        }
    }
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
    pub is_radio_station: bool,
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
            is_radio_station: false,
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
        seq: u64,
        mood_index: usize,
        home: Box<YtmRichHome>,
    },
    YtmRichHomeFailed {
        seq: u64,
        error: String,
    },
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
    RadioFailed {
        seed_track_id: String,
        error: String,
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
    UpdateDownload {
        gen_id: u64,
        event: DownloadEvent,
    },
    SpotifyLoginResult {
        display_name: String,
        tokens: ferric_providers::oauth::OAuthTokens,
    },
    SpotifyLoginFailed(String),
    SpotifyCatalogSynced {
        web_session: bool,
        report: Box<ferric_providers::oauth::CloudSyncReport>,
    },
    SpotifySyncFailed {
        web_session: bool,
        error: String,
    },
    SpotifyWebLoginResult {
        sp_dc: String,
        token_json: String,
    },
    SpotifyWebLoginFailed(String),
    RecommendedStationsLoaded {
        seq: u64,
        stations: Vec<RecommendedStation>,
    },
    StationTracksLoaded {
        station_id: String,
        tracks: Vec<Track>,
        subtitle: String,
        thumbnail_url: Option<String>,
        left_art: Option<String>,
        right_art: Option<String>,
    },
    StationTracksFailed {
        station_id: String,
        error: String,
    },
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

/// Truncate text cleanly at UTF-8 char boundaries while retaining international scripts
/// (Latin, Cyrillic, Greek, Japanese/Korean/Chinese, Thai, Arabic, Hebrew, etc.) and
/// stripping control chars, BiDi spoofing overrides, and color-emoji/private-use codepoints.
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
            !c.is_control()
                && !matches!(
                    cp,
                    0x061C
                        | 0x200D..=0x200F
                        | 0x202A..=0x202E
                        | 0x2066..=0x2069
                        | 0xE000..=0xF8FF
                        | 0xFE00..=0xFE0F
                        | 0xFEFF
                        | 0x2600..=0x27BF
                        | 0x1F000..=0x1FFFF
                )
        })
        .collect();
    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let final_str = if trimmed.is_empty() {
        "Unknown".to_string()
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

#[cfg(test)]
mod tests {
    use super::{PlaylistSortOrder, PlaylistSourceFilter};
    use ferric_core::models::{Playlist, TrackSource};

    fn make_pl(id: &str, source: TrackSource) -> Playlist {
        Playlist {
            id: id.to_string(),
            name: id.to_string(),
            source,
            etag: None,
            track_count: 5,
            description: String::new(),
            thumbnail_url: None,
        }
    }

    #[test]
    fn playlist_source_filter_separates_youtube_spotify_web_spotify_premium_and_local() {
        let yt = make_pl("ytm:pl:VL1", TrackSource::YouTubeMusic);
        let sp_web = make_pl("spotify:playlist:abc123", TrackSource::Spotify);
        let sp_prem = make_pl("spotify:premium:playlist:xyz789", TrackSource::Spotify);
        let local = make_pl("local:pl:1", TrackSource::Local);
        let sub = make_pl("sub:pl:1", TrackSource::Subsonic);

        assert!(PlaylistSourceFilter::All.matches_playlist(&yt));
        assert!(PlaylistSourceFilter::All.matches_playlist(&sp_web));
        assert!(PlaylistSourceFilter::All.matches_playlist(&sp_prem));
        assert!(PlaylistSourceFilter::All.matches_playlist(&local));

        assert!(PlaylistSourceFilter::YouTube.matches_playlist(&yt));
        assert!(!PlaylistSourceFilter::YouTube.matches_playlist(&sp_web));
        assert!(!PlaylistSourceFilter::YouTube.matches_playlist(&sp_prem));

        assert!(PlaylistSourceFilter::Spotify.matches_playlist(&sp_web));
        assert!(!PlaylistSourceFilter::Spotify.matches_playlist(&sp_prem));
        assert!(!PlaylistSourceFilter::Spotify.matches_playlist(&yt));

        assert!(PlaylistSourceFilter::SpotifyPremium.matches_playlist(&sp_prem));
        assert!(!PlaylistSourceFilter::SpotifyPremium.matches_playlist(&sp_web));
        assert!(!PlaylistSourceFilter::SpotifyPremium.matches_playlist(&yt));

        assert!(PlaylistSourceFilter::Local.matches_playlist(&local));
        assert!(PlaylistSourceFilter::Local.matches_playlist(&sub));
        assert!(!PlaylistSourceFilter::Local.matches_playlist(&sp_web));
    }

    #[test]
    fn playlist_sort_order_supports_newest_oldest_and_az() {
        // Input mirrors DB `list_playlists()` order: newest first -> oldest last.
        let base = vec![
            make_pl("YOASOBI Radio", TrackSource::Local),
            make_pl("Bruno Mars", TrackSource::Spotify),
            make_pl("AURORA Radio", TrackSource::Spotify),
        ];

        let mut newest = base.clone();
        PlaylistSortOrder::Newest.apply(&mut newest);
        assert_eq!(newest[0].name, "YOASOBI Radio");
        assert_eq!(newest[2].name, "AURORA Radio");

        let mut oldest = base.clone();
        PlaylistSortOrder::Oldest.apply(&mut oldest);
        assert_eq!(oldest[0].name, "AURORA Radio");
        assert_eq!(oldest[2].name, "YOASOBI Radio");

        let mut az = base;
        PlaylistSortOrder::NameAZ.apply(&mut az);
        assert_eq!(az[0].name, "AURORA Radio");
        assert_eq!(az[1].name, "Bruno Mars");
        assert_eq!(az[2].name, "YOASOBI Radio");
    }
}
