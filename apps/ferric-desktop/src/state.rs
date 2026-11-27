use crate::google_login::GoogleLoginOutcome;
use ferric_core::models::{Playlist, Track, TrackSource};
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
    About,
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
    Local,
}

impl PlaylistSourceFilter {
    pub fn matches_playlist(self, pl: &Playlist) -> bool {
        use ferric_core::models::TrackSource;
        match self {
            Self::All => true,
            Self::YouTube => pl.source == TrackSource::YouTubeMusic,
            Self::Spotify => pl.source == TrackSource::Spotify,
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
    pub share_url: Option<String>,
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
            share_url: None,
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
        if self.entries.len() > 12 {
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

pub struct StreamReadyPayload {
    pub track_id: String,
    pub video_id: String,
    pub stream: CachedAudioStream,
    pub auto_play: bool,
}

pub struct StationTracksPayload {
    pub station_id: String,
    pub tracks: Vec<Track>,
    pub subtitle: String,
    pub thumbnail_url: Option<String>,
    pub left_art: Option<String>,
    pub right_art: Option<String>,
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
    StreamReady(Box<StreamReadyPayload>),
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
    SpotifyCatalogSynced {
        web_session: bool,
        report: Box<ferric_providers::oauth::CloudSyncReport>,
    },
    SpotifySyncFailed(String),
    SpotifyWebLoginResult {
        sp_dc: String,
        token_json: String,
    },
    SpotifyWebLoginFailed(String),
    RecommendedStationsLoaded {
        seq: u64,
        stations: Vec<RecommendedStation>,
    },
    StationTracksLoaded(Box<StationTracksPayload>),
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
    let mut map: HashMap<String, (bool, CollectionGroup)> =
        HashMap::with_capacity((tracks.len() / 4).max(8));
    let mut key_buf = String::with_capacity(64);
    for track in tracks {
        let raw_album = track.album.trim();
        let (is_single, album_name) = if is_bogus_album_label(raw_album) {
            let fallback = track.title.trim();
            if fallback.is_empty() {
                continue;
            }
            (true, fallback)
        } else {
            (false, raw_album)
        };
        let artist_trimmed = track.artist.trim();
        key_buf.clear();
        key_buf.extend(album_name.chars().map(|c| c.to_ascii_lowercase()));
        key_buf.push_str("::");
        key_buf.extend(artist_trimmed.chars().map(|c| c.to_ascii_lowercase()));

        if let Some(entry) = map.get_mut(&key_buf) {
            if entry.1.thumbnail_url.is_none() {
                entry.1.thumbnail_url = resolved_track_thumb(track);
            }
            entry.1.tracks.push(track.clone());
        } else {
            let mut id = String::with_capacity(6 + album_name.len());
            id.push_str("album:");
            id.extend(album_name.chars().map(|c| c.to_ascii_lowercase()));
            map.insert(
                key_buf.clone(),
                (
                    is_single,
                    CollectionGroup {
                        id,
                        title: album_name.to_string(),
                        subtitle: track.artist.clone(),
                        kind: YtmCardKind::Album,
                        thumbnail_url: resolved_track_thumb(track),
                        tracks: vec![track.clone()],
                    },
                ),
            );
        }
    }
    let mut out: Vec<CollectionGroup> = map
        .into_values()
        .map(|(is_single, mut g)| {
            g.tracks.shrink_to_fit();
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
    out.shrink_to_fit();
    out
}

/// Group all library tracks into circular Artist cards with real artwork.
pub fn build_artist_groups(tracks: &[Track]) -> Vec<CollectionGroup> {
    let mut map: HashMap<String, CollectionGroup> =
        HashMap::with_capacity((tracks.len() / 4).max(8));
    let mut key_buf = String::with_capacity(48);
    for track in tracks {
        let trimmed = track.artist.trim();
        let artist_name = if trimmed.is_empty() {
            "Unknown Artist"
        } else {
            trimmed
        };
        key_buf.clear();
        key_buf.extend(artist_name.chars().map(|c| c.to_ascii_lowercase()));
        if let Some(entry) = map.get_mut(&key_buf) {
            if entry.thumbnail_url.is_none() {
                entry.thumbnail_url = resolved_track_thumb(track);
            }
            entry.tracks.push(track.clone());
        } else {
            let mut id = String::with_capacity(7 + key_buf.len());
            id.push_str("artist:");
            id.push_str(&key_buf);
            map.insert(
                key_buf.clone(),
                CollectionGroup {
                    id,
                    title: artist_name.to_string(),
                    subtitle: String::new(),
                    kind: YtmCardKind::Artist,
                    thumbnail_url: resolved_track_thumb(track),
                    tracks: vec![track.clone()],
                },
            );
        }
    }
    let mut out: Vec<CollectionGroup> = map.into_values().collect();
    for g in &mut out {
        g.tracks.shrink_to_fit();
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
    out.shrink_to_fit();
    out
}

#[inline]
fn is_stripped_display_char(c: char) -> bool {
    let cp = c as u32;
    c.is_control()
        || matches!(
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
}

/// Single-pass text sanitizer + UTF-8 char truncation into a caller-owned scratch `String`
/// (zero intermediate `String` or `Vec<&str>` allocations).
pub fn clean_clip_into(
    raw: &str,
    max_chars: usize,
    fallback_if_empty: Option<&str>,
    out: &mut String,
) {
    out.clear();
    let mut written_chars = 0usize;
    let mut pending_space = false;
    let mut truncated = false;
    let mut truncate_byte_idx = 0usize;
    let keep_limit = max_chars.saturating_sub(1);

    for ch in raw.chars() {
        let normalized = if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else if is_stripped_display_char(ch) {
            continue;
        } else {
            ch
        };
        if normalized.is_whitespace() {
            if written_chars > 0 {
                pending_space = true;
            }
            continue;
        }
        if pending_space {
            if written_chars == keep_limit {
                truncate_byte_idx = out.len();
            }
            if written_chars >= max_chars {
                truncated = true;
                break;
            }
            out.push(' ');
            written_chars += 1;
            pending_space = false;
        }
        if written_chars == keep_limit {
            truncate_byte_idx = out.len();
        }
        if written_chars >= max_chars {
            truncated = true;
            break;
        }
        out.push(normalized);
        written_chars += 1;
    }

    if truncated {
        out.truncate(truncate_byte_idx);
        out.push('…');
    } else if out.is_empty()
        && let Some(fb) = fallback_if_empty
    {
        out.push_str(fb);
    }
}

/// Truncate text cleanly at UTF-8 char boundaries while retaining international scripts
/// (Latin, Cyrillic, Greek, Japanese/Korean/Chinese, Thai, Arabic, Hebrew, etc.) and
/// stripping control chars, BiDi spoofing overrides, and color-emoji/private-use codepoints.
pub fn clip_text(raw: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_chars.saturating_mul(4)));
    clean_clip_into(raw, max_chars, Some("Unknown"), &mut out);
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

fn clean_url_id(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return None;
    }
    Some(trimmed)
}

/// Canonical external web URL for a playlist, album, artist, or radio station.
/// Returns `(platform_label, url)` suitable for sharing or pasting into Discord music bots.
pub fn collection_share_url(id: &str, kind: YtmCardKind) -> Option<(&'static str, String)> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return None;
    }
    for prefix in [
        "spotify:playlist:",
        "spotify:premium:playlist:",
        "spotify:station:",
        "station:spotify:",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && let Some(clean) = clean_url_id(rest)
        {
            return Some((
                "Spotify",
                format!("https://open.spotify.com/playlist/{clean}"),
            ));
        }
    }
    if let Some(rest) = trimmed.strip_prefix("spotify:album:")
        && let Some(clean) = clean_url_id(rest)
    {
        return Some(("Spotify", format!("https://open.spotify.com/album/{clean}")));
    }
    if let Some(rest) = trimmed.strip_prefix("spotify:artist:")
        && let Some(clean) = clean_url_id(rest)
    {
        return Some((
            "Spotify",
            format!("https://open.spotify.com/artist/{clean}"),
        ));
    }
    if let Some(rest) = trimmed.strip_prefix("yt:playlist:")
        && let Some(clean) = clean_url_id(rest)
    {
        return Some((
            "YouTube Music",
            format!("https://music.youtube.com/playlist?list={clean}"),
        ));
    }
    let browse = trimmed.strip_prefix("ytm:pl:").unwrap_or(trimmed);
    if let Some(rest) = browse.strip_prefix("VL") {
        if rest.eq_ignore_ascii_case("LM") {
            return None;
        }
        if let Some(clean) = clean_url_id(rest) {
            return Some((
                "YouTube Music",
                format!("https://music.youtube.com/playlist?list={clean}"),
            ));
        }
    }
    if (browse.starts_with("PL") || browse.starts_with("OLAK") || browse.starts_with("RD"))
        && let Some(clean) = clean_url_id(browse)
    {
        return Some((
            "YouTube Music",
            format!("https://music.youtube.com/playlist?list={clean}"),
        ));
    }
    if browse.starts_with("MPRE")
        && let Some(clean) = clean_url_id(browse)
    {
        return Some((
            "YouTube Music",
            format!("https://music.youtube.com/browse/{clean}"),
        ));
    }
    if (browse.starts_with("UC") || kind == YtmCardKind::Artist)
        && browse.starts_with("UC")
        && let Some(clean) = clean_url_id(browse)
    {
        return Some((
            "YouTube Music",
            format!("https://music.youtube.com/channel/{clean}"),
        ));
    }
    None
}

/// Primary direct track URL for sharing or pasting into Discord music bots.
pub fn track_primary_link(track: &Track) -> Option<(&'static str, String)> {
    let spotify_clean = track.spotify_id.as_deref().and_then(|raw| {
        let stripped = raw.trim().strip_prefix("spotify:track:").unwrap_or(raw);
        clean_url_id(stripped)
    });
    let yt_clean = track.youtube_video_id.as_deref().and_then(clean_url_id);

    if track.source == TrackSource::Spotify
        && let Some(sp) = spotify_clean
    {
        return Some(("Spotify", format!("https://open.spotify.com/track/{sp}")));
    }
    if let Some(vid) = yt_clean {
        return Some((
            "YouTube Music",
            format!("https://music.youtube.com/watch?v={vid}"),
        ));
    }
    if let Some(sp) = spotify_clean {
        return Some(("Spotify", format!("https://open.spotify.com/track/{sp}")));
    }
    None
}

/// Secondary direct YouTube Music URL for tracks that also have a Spotify ID.
pub fn track_youtube_link(track: &Track) -> Option<String> {
    let vid = track.youtube_video_id.as_deref().and_then(clean_url_id)?;
    Some(format!("https://music.youtube.com/watch?v={vid}"))
}

/// Format a clean numbered tracklist for pasting into Discord, chat, or notes.
pub fn format_collection_tracklist(
    title: &str,
    share_url: Option<&str>,
    tracks: &[Track],
) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(96 + tracks.len() * 48);
    let _ = writeln!(out, "{} ({} songs)", title.trim(), tracks.len());
    if let Some(url) = share_url
        && !url.trim().is_empty()
    {
        let _ = writeln!(out, "{url}");
    }
    if !tracks.is_empty() {
        out.push('\n');
    }
    for (idx, t) in tracks.iter().enumerate() {
        let artist = t.artist.replace(['\r', '\n'], " ");
        let song = t.title.replace(['\r', '\n'], " ");
        if artist.trim().is_empty() {
            let _ = writeln!(out, "{}. {}", idx + 1, song.trim());
        } else {
            let _ = writeln!(out, "{}. {} - {}", idx + 1, artist.trim(), song.trim());
        }
    }
    out
}

fn csv_escape_field(raw: &str) -> String {
    let clean = raw.replace(['\r', '\n'], " ");
    if clean.contains([',', '"']) {
        format!("\"{}\"", clean.replace('"', "\"\""))
    } else {
        clean
    }
}

/// Export tracks as RFC 4180 CSV with direct Spotify / YouTube Music URLs.
pub fn format_collection_csv(tracks: &[Track]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64 + tracks.len() * 112);
    out.push_str("Title,Artist,Album,Duration,Source,URL\n");
    for t in tracks {
        let dur_s = t.duration_ms / 1000;
        let dur = format!("{}:{:02}", dur_s / 60, dur_s % 60);
        let url = track_primary_link(t).map_or_else(String::new, |(_, u)| u);
        let _ = writeln!(
            out,
            "{},{},{},{},{},{}",
            csv_escape_field(&t.title),
            csv_escape_field(&t.artist),
            csv_escape_field(&t.album),
            dur,
            t.source.as_badge(),
            csv_escape_field(&url),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        PlaylistSortOrder, PlaylistSourceFilter, collection_share_url, format_collection_csv,
        format_collection_tracklist, track_primary_link, track_youtube_link,
    };
    use ferric_core::models::{Playlist, Track, TrackSource};
    use ferric_providers::ytmusic::YtmCardKind;

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
        assert!(PlaylistSourceFilter::Spotify.matches_playlist(&sp_prem));
        assert!(!PlaylistSourceFilter::Spotify.matches_playlist(&yt));

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

    #[test]
    fn share_urls_handle_spotify_youtube_and_reject_unsafe_ids() {
        assert_eq!(
            collection_share_url(
                "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M",
                YtmCardKind::Playlist
            ),
            Some((
                "Spotify",
                "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M".to_string()
            ))
        );
        assert_eq!(
            collection_share_url("spotify:premium:playlist:abc123", YtmCardKind::Playlist),
            Some((
                "Spotify",
                "https://open.spotify.com/playlist/abc123".to_string()
            ))
        );
        assert_eq!(
            collection_share_url("ytm:pl:VLPL12345_abc", YtmCardKind::Playlist),
            Some((
                "YouTube Music",
                "https://music.youtube.com/playlist?list=PL12345_abc".to_string()
            ))
        );
        assert_eq!(
            collection_share_url("yt:playlist:PL999-xyz", YtmCardKind::Playlist),
            Some((
                "YouTube Music",
                "https://music.youtube.com/playlist?list=PL999-xyz".to_string()
            ))
        );
        assert_eq!(
            collection_share_url("MPREb_abc123", YtmCardKind::Album),
            Some((
                "YouTube Music",
                "https://music.youtube.com/browse/MPREb_abc123".to_string()
            ))
        );
        assert_eq!(
            collection_share_url("ytm:pl:VLLM", YtmCardKind::Playlist),
            None
        );
        assert_eq!(
            collection_share_url("local:pl:42", YtmCardKind::Playlist),
            None
        );
        assert_eq!(
            collection_share_url("spotify:playlist:../evil", YtmCardKind::Playlist),
            None
        );

        let sp_track = Track {
            id: "sp:1".to_string(),
            title: "Die With A Smile".to_string(),
            artist: "Lady Gaga, Bruno Mars".to_string(),
            album: "Single".to_string(),
            duration_ms: 251_000,
            path: None,
            source: TrackSource::Spotify,
            codec: "OPUS".to_string(),
            sample_rate: 48_000,
            bit_depth: 16,
            bitrate_kbps: 144,
            isrc: None,
            spotify_id: Some("2plbrEY59IikOBgBGLjaoe".to_string()),
            youtube_video_id: Some("kPa7bsKwL-c".to_string()),
            thumbnail_url: None,
            replay_gain_db: 0,
            liked: false,
        };
        assert_eq!(
            track_primary_link(&sp_track),
            Some((
                "Spotify",
                "https://open.spotify.com/track/2plbrEY59IikOBgBGLjaoe".to_string()
            ))
        );
        assert_eq!(
            track_youtube_link(&sp_track),
            Some("https://music.youtube.com/watch?v=kPa7bsKwL-c".to_string())
        );

        let csv = format_collection_csv(std::slice::from_ref(&sp_track));
        assert!(csv.contains("\"Lady Gaga, Bruno Mars\""));
        assert!(csv.contains("https://open.spotify.com/track/2plbrEY59IikOBgBGLjaoe"));
        let txt = format_collection_tracklist(
            "K",
            Some("https://open.spotify.com/playlist/abc123"),
            std::slice::from_ref(&sp_track),
        );
        assert!(txt.contains("K (1 songs)"));
        assert!(txt.contains("1. Lady Gaga, Bruno Mars - Die With A Smile"));
    }
}
