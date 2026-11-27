//! Shared pure library sorting, filtering, deduplication, and Discord/CSV share-link helpers
//! reused identically by Ferric Desktop and Ferric Android.

use crate::models::{Playlist, Track, TrackSource};
use crate::text::cmp_ascii_icase;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaylistSortOrder {
    #[default]
    Newest,
    Oldest,
    NameAZ,
}

impl PlaylistSortOrder {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::NameAZ => "A-Z",
        }
    }

    #[must_use]
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
                    cmp_ascii_icase(&a.name, &b.name).then_with(|| a.id.cmp(&b.id))
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
    #[must_use]
    pub fn matches_playlist(self, pl: &Playlist) -> bool {
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

    #[must_use]
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
pub enum DetailSortOrder {
    #[default]
    Original,
    Title,
    Artist,
    Duration,
}

impl DetailSortOrder {
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Original => Self::Title,
            Self::Title => Self::Artist,
            Self::Artist => Self::Duration,
            Self::Duration => Self::Original,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Sort: Default",
            Self::Title => "Sort: Title",
            Self::Artist => "Sort: Artist",
            Self::Duration => "Sort: Duration",
        }
    }
}

/// Remove duplicate tracks from a playlist while preserving original order.
/// Uses `HashSet` membership (`O(N)`) matching canonical ID, Spotify ID, YouTube ID, or `artist|title`.
#[must_use]
pub fn deduplicate_tracks_in_order(tracks: &[Track]) -> (Vec<Track>, usize) {
    let mut seen_ids: HashSet<String> = HashSet::with_capacity(tracks.len());
    let mut out = Vec::with_capacity(tracks.len());
    let mut removed = 0usize;
    for t in tracks {
        let mut keys: Vec<String> = Vec::with_capacity(4);
        keys.push(format!("id:{}", t.id));
        if let Some(sp) = t
            .spotify_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            keys.push(format!("sp:{sp}"));
        }
        if let Some(yt) = t
            .youtube_video_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            keys.push(format!("yt:{yt}"));
        }
        let norm_title = t.title.trim().to_ascii_lowercase();
        let norm_artist = t.artist.trim().to_ascii_lowercase();
        if !norm_title.is_empty() {
            keys.push(format!("meta:{norm_artist}|{norm_title}"));
        }
        if keys.iter().any(|k| seen_ids.contains(k)) {
            removed += 1;
            continue;
        }
        for k in keys {
            seen_ids.insert(k);
        }
        out.push(t.clone());
    }
    (out, removed)
}

#[must_use]
pub fn clean_url_id(raw: &str) -> Option<&str> {
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

/// Primary direct track URL for sharing or pasting into Discord music bots.
#[must_use]
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
#[must_use]
pub fn track_youtube_link(track: &Track) -> Option<String> {
    let vid = track.youtube_video_id.as_deref().and_then(clean_url_id)?;
    Some(format!("https://music.youtube.com/watch?v={vid}"))
}

/// Format a clean numbered tracklist for pasting into Discord, chat, or notes.
#[must_use]
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
#[must_use]
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
    use super::*;

    #[test]
    fn collections_dedup_sort_and_share_links_match_desktop() {
        let track = Track {
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
        let mut dup = track.clone();
        dup.id = "yt:video:kPa7bsKwL-c".to_string();
        let (deduped, removed) = deduplicate_tracks_in_order(&[track.clone(), dup]);
        assert_eq!(deduped.len(), 1);
        assert_eq!(removed, 1);

        assert_eq!(
            track_primary_link(&track),
            Some((
                "Spotify",
                "https://open.spotify.com/track/2plbrEY59IikOBgBGLjaoe".to_string()
            ))
        );
        assert_eq!(
            track_youtube_link(&track),
            Some("https://music.youtube.com/watch?v=kPa7bsKwL-c".to_string())
        );

        let csv = format_collection_csv(std::slice::from_ref(&track));
        assert!(csv.contains("\"Lady Gaga, Bruno Mars\""));
        let txt = format_collection_tracklist(
            "K",
            Some("https://open.spotify.com/playlist/abc123"),
            std::slice::from_ref(&track),
        );
        assert!(txt.contains("K (1 songs)"));
    }
}
