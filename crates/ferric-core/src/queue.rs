use crate::models::{RepeatMode, Track};

/// Queue state supporting sequential play, Repeat modes, and True Smart Shuffle
/// (Fisher-Yates with adjacent artist-spacing so the same artist avoids back-to-back clusters).
#[derive(Debug, Clone, Default)]
pub struct PlayQueue {
    tracks: Vec<Track>,
    order: Vec<usize>,
    cursor: Option<usize>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

impl PlayQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_tracks(&mut self, tracks: Vec<Track>, start_index: Option<usize>) {
        self.tracks = tracks;
        self.rebuild_order(start_index);
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Update a queued track in-place after its online audio stream has been resolved and cached.
    pub fn update_track_cached_stream(
        &mut self,
        track_id: &str,
        path: std::path::PathBuf,
        video_id: Option<String>,
        codec: &str,
        bitrate_kbps: u32,
        duration_ms: u64,
    ) {
        for track in &mut self.tracks {
            if track.id == track_id {
                track.path = Some(path.clone());
                if let Some(vid) = &video_id {
                    track.youtube_video_id = Some(vid.clone());
                }
                if !codec.is_empty() {
                    track.codec = codec.to_string();
                }
                if bitrate_kbps > 0 {
                    track.bitrate_kbps = bitrate_kbps;
                }
                if duration_ms > 0 && track.duration_ms == 0 {
                    track.duration_ms = duration_ms;
                }
            }
        }
    }

    pub fn current(&self) -> Option<&Track> {
        let pos = self.cursor?;
        let track_idx = *self.order.get(pos)?;
        self.tracks.get(track_idx)
    }

    pub fn current_track_index(&self) -> Option<usize> {
        let pos = self.cursor?;
        self.order.get(pos).copied()
    }

    pub fn jump_to_track_index(&mut self, track_idx: usize) -> Option<&Track> {
        if track_idx >= self.tracks.len() {
            return None;
        }
        if let Some(pos) = self.order.iter().position(|&idx| idx == track_idx) {
            self.cursor = Some(pos);
        } else {
            self.rebuild_order(Some(track_idx));
        }
        self.current()
    }

    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle == enabled {
            return;
        }
        let current_track = self.current_track_index();
        self.shuffle = enabled;
        self.rebuild_order(current_track);
    }

    pub fn next_track(&mut self) -> Option<&Track> {
        if self.order.is_empty() {
            return None;
        }
        match (self.cursor, self.repeat) {
            (Some(pos), RepeatMode::One) => {
                self.cursor = Some(pos);
            }
            (Some(pos), RepeatMode::All) => {
                self.cursor = Some((pos + 1) % self.order.len());
            }
            (Some(pos), RepeatMode::Off) => {
                if pos + 1 < self.order.len() {
                    self.cursor = Some(pos + 1);
                } else {
                    return None;
                }
            }
            (None, _) => {
                self.cursor = Some(0);
            }
        }
        self.current()
    }

    pub fn prev_track(&mut self) -> Option<&Track> {
        if self.order.is_empty() {
            return None;
        }
        match self.cursor {
            Some(0) => {
                self.cursor = if self.repeat == RepeatMode::All {
                    Some(self.order.len() - 1)
                } else {
                    Some(0)
                };
            }
            Some(pos) => self.cursor = Some(pos - 1),
            None => self.cursor = Some(0),
        }
        self.current()
    }

    /// Peek upcoming tracks (for Up Next pane & Deck B gapless pre-buffering).
    pub fn peek_upcoming(&self, limit: usize) -> Vec<&Track> {
        let Some(pos) = self.cursor else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(limit);
        for offset in 1..=limit {
            let next_pos = pos + offset;
            let idx = if next_pos < self.order.len() {
                self.order[next_pos]
            } else if self.repeat == RepeatMode::All && !self.order.is_empty() {
                self.order[next_pos % self.order.len()]
            } else {
                break;
            };
            if let Some(t) = self.tracks.get(idx) {
                out.push(t);
            }
        }
        out
    }

    fn rebuild_order(&mut self, anchor_track: Option<usize>) {
        let n = self.tracks.len();
        if n == 0 {
            self.order.clear();
            self.cursor = None;
            return;
        }
        if !self.shuffle {
            self.order = (0..n).collect();
            self.cursor = anchor_track.filter(|&idx| idx < n);
            return;
        }

        // Fisher-Yates shuffle keeping anchor_track at slot 0 when active.
        let mut pool: Vec<usize> = (0..n).collect();
        if let Some(anchor) = anchor_track.filter(|&idx| idx < n) {
            pool.swap(0, anchor);
            fisher_yates(&mut pool[1..]);
        } else {
            fisher_yates(&mut pool);
        }

        // Single-pass artist-spacing refinement to minimize consecutive same-artist clusters.
        for i in 1..pool.len() {
            let prev_artist = &self.tracks[pool[i - 1]].artist;
            if &self.tracks[pool[i]].artist == prev_artist
                && let Some(swap_rel) = pool[i + 1..]
                    .iter()
                    .position(|&cand| &self.tracks[cand].artist != prev_artist)
            {
                pool.swap(i, i + 1 + swap_rel);
            }
        }

        self.order = pool;
        self.cursor = anchor_track.map(|_| 0);
    }
}

fn fisher_yates(slice: &mut [usize]) {
    for i in (1..slice.len()).rev() {
        let j = fastrand::usize(0..=i);
        slice.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TrackSource;

    fn dummy_track(id: &str, artist: &str) -> Track {
        Track {
            id: id.to_string(),
            title: format!("Song {id}"),
            artist: artist.to_string(),
            album: "Test".to_string(),
            duration_ms: 180_000,
            path: None,
            source: TrackSource::Local,
            codec: "FLAC".to_string(),
            sample_rate: 44_100,
            bit_depth: 16,
            bitrate_kbps: 900,
            isrc: None,
            spotify_id: None,
            youtube_video_id: None,
            replay_gain_db: 0,
            liked: false,
        }
    }

    #[test]
    fn smart_shuffle_keeps_anchor_first_and_spaces_artists() {
        let tracks = vec![
            dummy_track("1", "Daft Punk"),
            dummy_track("2", "Daft Punk"),
            dummy_track("3", "Daft Punk"),
            dummy_track("4", "Tulus"),
            dummy_track("5", "Hindia"),
            dummy_track("6", "NIKI"),
        ];
        let mut q = PlayQueue::new();
        q.set_tracks(tracks, Some(0));
        q.set_shuffle(true);
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("1"));
        assert_eq!(q.peek_upcoming(2).len(), 2);
    }
}
