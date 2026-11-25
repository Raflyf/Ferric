use ferric_core::models::{Track, TrackSource};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use std::path::Path;
use walkdir::WalkDir;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "m4a", "aac", "alac", "ogg", "opus", "wav", "aiff", "aif", "webm", "mka",
];

pub fn is_supported_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|&s| s.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

/// Recursively scan a directory for supported local audio files and read tags via `lofty`.
pub fn scan_local_folder(root: &Path) -> Vec<Track> {
    let mut tracks = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_supported_audio_file(path) {
            continue;
        }
        if let Some(track) = read_track_metadata(path) {
            tracks.push(track);
        }
    }
    tracks
}

/// Read metadata for a single local audio file with clean filename fallback.
pub fn read_track_metadata(path: &Path) -> Option<Track> {
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled Track")
        .to_string();
    let ext_codec = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("AUDIO")
        .to_ascii_uppercase();

    let canonical_id = format!("local:{}", path.to_string_lossy());

    let probed = Probe::open(path).ok().and_then(|p| p.read().ok());
    if let Some(tagged) = probed {
        let props = tagged.properties();
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

        let title = tag
            .and_then(|t| t.title())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_title);
        let artist = tag
            .and_then(|t| t.artist())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let album = tag
            .and_then(|t| t.album())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Local Library".to_string());
        let isrc = tag
            .and_then(|t| t.get_string(ItemKey::Isrc))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        return Some(Track {
            id: canonical_id,
            title,
            artist,
            album,
            duration_ms: props.duration().as_millis() as u64,
            path: Some(path.to_path_buf()),
            source: TrackSource::Local,
            codec: ext_codec,
            sample_rate: props.sample_rate().unwrap_or(44_100),
            bit_depth: u16::from(props.bit_depth().unwrap_or(16)),
            bitrate_kbps: props.audio_bitrate().unwrap_or(0),
            isrc,
            spotify_id: None,
            youtube_video_id: None,
            thumbnail_url: None,
            replay_gain_db: 0,
            liked: false,
        });
    }

    Some(Track {
        id: canonical_id,
        title: fallback_title,
        artist: "Unknown Artist".to_string(),
        album: "Local Library".to_string(),
        duration_ms: 0,
        path: Some(path.to_path_buf()),
        source: TrackSource::Local,
        codec: ext_codec,
        sample_rate: 44_100,
        bit_depth: 16,
        bitrate_kbps: 0,
        isrc: None,
        spotify_id: None,
        youtube_video_id: None,
        thumbnail_url: None,
        replay_gain_db: 0,
        liked: false,
    })
}

const MAX_EMBEDDED_COVER_BYTES: usize = 8 * 1024 * 1024;

/// Extract embedded album art raw bytes (JPEG/PNG) if present in the audio file tags (8 MiB cap).
pub fn read_embedded_cover_bytes(path: &Path) -> Option<Vec<u8>> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let pic = tag.pictures().first()?;
    let data = pic.data();
    if data.is_empty() || data.len() > MAX_EMBEDDED_COVER_BYTES {
        return None;
    }
    Some(data.to_vec())
}
