use crate::ProviderResult;
use ferric_core::models::Track;
use std::path::PathBuf;

/// Clean-room Metadata-Audio Decoupling query contract.
/// Catalog accounts (Spotify Web API / YouTube Data API v3) populate `TrackQuery`,
/// while compliant local/P2P/OpenSubsonic resolvers locate playable audio streams.
#[derive(Debug, Clone)]
pub struct TrackQuery {
    pub canonical_id: String,
    pub spotify_id: Option<String>,
    pub youtube_video_id: Option<String>,
    pub isrc: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
}

impl From<&Track> for TrackQuery {
    fn from(t: &Track) -> Self {
        Self {
            canonical_id: t.id.clone(),
            spotify_id: t.spotify_id.clone(),
            youtube_video_id: t.youtube_video_id.clone(),
            isrc: t.isrc.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamLocator {
    LocalPath(PathBuf),
    IrohBlob {
        peer: String,
        blake3_hex: String,
        size_bytes: u64,
    },
    HttpRangeUrl {
        url: String,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub provider_id: &'static str,
    pub locator: StreamLocator,
    pub confidence: f32,
}

pub trait AudioResolver: Send + Sync {
    fn id(&self) -> &'static str;
    fn priority(&self) -> u8;
    fn resolve(&self, query: &TrackQuery) -> ProviderResult<Option<ResolvedStream>>;
}

/// Priority-0 local library resolver matching exact path, ISRC, or normalized artist + title.
pub struct LocalIndexResolver<'a> {
    tracks: &'a [Track],
}

impl<'a> LocalIndexResolver<'a> {
    pub fn new(tracks: &'a [Track]) -> Self {
        Self { tracks }
    }
}

impl AudioResolver for LocalIndexResolver<'_> {
    fn id(&self) -> &'static str {
        "local_files"
    }

    fn priority(&self) -> u8 {
        0
    }

    fn resolve(&self, query: &TrackQuery) -> ProviderResult<Option<ResolvedStream>> {
        for track in self.tracks {
            let Some(path) = &track.path else { continue };
            if !path.exists() {
                continue;
            }
            if track.id == query.canonical_id {
                return Ok(Some(ResolvedStream {
                    provider_id: self.id(),
                    locator: StreamLocator::LocalPath(path.clone()),
                    confidence: 1.0,
                }));
            }
            if let (Some(a), Some(b)) = (&track.isrc, &query.isrc)
                && a.eq_ignore_ascii_case(b)
            {
                return Ok(Some(ResolvedStream {
                    provider_id: self.id(),
                    locator: StreamLocator::LocalPath(path.clone()),
                    confidence: 0.99,
                }));
            }
            if track.title.eq_ignore_ascii_case(&query.title)
                && track.artist.eq_ignore_ascii_case(&query.artist)
            {
                return Ok(Some(ResolvedStream {
                    provider_id: self.id(),
                    locator: StreamLocator::LocalPath(path.clone()),
                    confidence: 0.92,
                }));
            }
        }
        Ok(None)
    }
}
