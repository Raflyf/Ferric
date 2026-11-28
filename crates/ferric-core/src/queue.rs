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

    /// Append tracks to the end of the active queue (skipping duplicates by ID).
    pub fn append_tracks(&mut self, extra: Vec<Track>) -> usize {
        if extra.is_empty() {
            return 0;
        }
        if self.tracks.len().saturating_mul(extra.len()) < 128 {
            let mut added = 0usize;
            self.tracks.reserve(extra.len());
            self.order.reserve(extra.len());
            for t in extra {
                if self.tracks.iter().any(|existing| existing.id == t.id) {
                    continue;
                }
                let idx = self.tracks.len();
                self.tracks.push(t);
                self.order.push(idx);
                added += 1;
            }
            return added;
        }

        let existing: std::collections::HashSet<&str> =
            self.tracks.iter().map(|t| t.id.as_str()).collect();
        let mut batch_seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(extra.len());
        let mut to_add = Vec::with_capacity(extra.len());
        for t in extra {
            if existing.contains(t.id.as_str()) || !batch_seen.insert(t.id.clone()) {
                continue;
            }
            to_add.push(t);
        }
        drop(existing);
        drop(batch_seen);

        let added = to_add.len();
        if added == 0 {
            return 0;
        }
        self.tracks.reserve(added);
        self.order.reserve(added);
        for t in to_add {
            let idx = self.tracks.len();
            self.tracks.push(t);
            self.order.push(idx);
        }
        added
    }

    /// Insert a single track right after the currently playing track ("Play Next").
    pub fn insert_play_next(&mut self, track: Track) {
        let idx = self.tracks.len();
        self.tracks.push(track);
        let insert_pos = self.cursor.map_or(0, |c| (c + 1).min(self.order.len()));
        self.order.insert(insert_pos, idx);
    }

    /// Append a single track to the end of the active queue (allowing explicit re-queueing).
    pub fn append_single_to_end(&mut self, track: Track) {
        let idx = self.tracks.len();
        self.tracks.push(track);
        self.order.push(idx);
    }

    /// Resolve the `offset`-th upcoming slot in `self.order`, mirroring `peek_upcoming`
    /// (wrapping modulo length under `RepeatMode::All` while never targeting the active cursor).
    fn resolve_upcoming_slot(&self, offset: usize) -> Option<usize> {
        let pos = self.cursor?;
        let len = self.order.len();
        if len <= 1 {
            return None;
        }
        let raw = pos.checked_add(1)?.checked_add(offset)?;
        if raw < len {
            Some(raw)
        } else if self.repeat == RepeatMode::All && offset < len - 1 {
            let wrapped = raw % len;
            (wrapped != pos).then_some(wrapped)
        } else {
            None
        }
    }

    /// Remove the `offset`-th upcoming track from Up Next (`0` = very next song).
    pub fn remove_upcoming(&mut self, offset: usize) -> Option<Track> {
        let pos = self.cursor?;
        let target = self.resolve_upcoming_slot(offset)?;
        let track_idx = self.order.remove(target);
        if target < pos {
            self.cursor = Some(pos - 1);
        }
        let removed = self.tracks.get(track_idx).cloned();
        self.compact_unreferenced_tracks();
        removed
    }

    /// Move the `offset`-th upcoming track one slot higher in Up Next (`offset >= 1`).
    pub fn move_upcoming_up(&mut self, offset: usize) -> bool {
        if offset == 0 {
            return false;
        }
        let Some(target) = self.resolve_upcoming_slot(offset) else {
            return false;
        };
        let Some(prev) = self.resolve_upcoming_slot(offset - 1) else {
            return false;
        };
        if target == prev {
            return false;
        }
        self.order.swap(prev, target);
        true
    }

    /// Remove all queued occurrences of a deleted track ID while keeping current cursor valid
    /// and compacting `self.tracks` so toggling Shuffle never resurrects deleted tracks.
    pub fn remove_track_by_id(&mut self, track_id: &str) -> bool {
        let mut removed_any = false;
        let orig_cp = self.cursor;
        let mut new_order = Vec::with_capacity(self.order.len());
        let mut removed_before_cursor = 0usize;
        let mut removed_cursor = false;
        for (ord_pos, &track_idx) in self.order.iter().enumerate() {
            let matches = self.tracks.get(track_idx).is_some_and(|t| t.id == track_id);
            if matches {
                removed_any = true;
                if let Some(cp) = orig_cp {
                    if ord_pos < cp {
                        removed_before_cursor += 1;
                    } else if ord_pos == cp {
                        removed_cursor = true;
                    }
                }
            } else {
                new_order.push(track_idx);
            }
        }
        if removed_any {
            self.order = new_order;
            self.cursor = if self.order.is_empty() {
                None
            } else {
                orig_cp.and_then(|cp| {
                    let adjusted = cp.saturating_sub(removed_before_cursor);
                    if removed_cursor {
                        if adjusted < self.order.len() {
                            Some(adjusted)
                        } else if self.repeat == RepeatMode::All {
                            Some(0)
                        } else {
                            None
                        }
                    } else {
                        Some(adjusted.min(self.order.len() - 1))
                    }
                })
            };
            self.compact_unreferenced_tracks();
        }
        removed_any
    }

    fn compact_unreferenced_tracks(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let mut used = vec![false; self.tracks.len()];
        for &idx in &self.order {
            if let Some(slot) = used.get_mut(idx) {
                *slot = true;
            }
        }
        if used.iter().all(|&u| u) {
            return;
        }
        let mut remap = vec![0usize; self.tracks.len()];
        let mut new_tracks = Vec::with_capacity(self.order.len());
        for (old_idx, track) in std::mem::take(&mut self.tracks).into_iter().enumerate() {
            if used[old_idx] {
                remap[old_idx] = new_tracks.len();
                new_tracks.push(track);
            }
        }
        self.order.retain(|&idx| idx < remap.len() && used[idx]);
        for idx in &mut self.order {
            if let Some(&mapped) = remap.get(*idx) {
                *idx = mapped;
            }
        }
        new_tracks.shrink_to_fit();
        self.tracks = new_tracks;
        if self.order.capacity() > self.order.len().saturating_mul(2).max(64) {
            self.order.shrink_to_fit();
        }
    }

    /// Jump directly to the `offset`-th upcoming track (0 = very next song in Up Next,
    /// or slot `offset` from the start of the queue when playback has not started yet).
    pub fn jump_to_upcoming(&mut self, offset: usize) -> Option<&Track> {
        let target = match self.cursor {
            Some(pos) => pos.checked_add(1)?.checked_add(offset)?,
            None => offset,
        };
        if target < self.order.len() {
            self.cursor = Some(target);
            self.current()
        } else if self.repeat == RepeatMode::All && !self.order.is_empty() {
            self.cursor = Some(target % self.order.len());
            self.current()
        } else {
            None
        }
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Returns an ordered queue slice up to `max_len` items following `self.order` (respecting
    /// active Shuffle ordering) plus the cursor index inside that slice, used by Ferric Connect.
    pub fn ordered_queue_window(&self, max_len: usize) -> (Vec<&Track>, usize) {
        let len = self.order.len();
        if len == 0 || max_len == 0 {
            return (Vec::new(), 0);
        }
        let cur_pos = self.cursor.unwrap_or(0).min(len - 1);
        let start = if len <= max_len {
            0
        } else {
            cur_pos.saturating_sub(25).min(len - max_len)
        };
        let end = (start + max_len).min(len);
        let items = self.order[start..end]
            .iter()
            .filter_map(|&idx| self.tracks.get(idx))
            .collect();
        (items, cur_pos.saturating_sub(start))
    }

    /// Jumps to a position inside the active `ordered_queue_window(max_len)` without re-shuffling.
    pub fn jump_to_connect_window_index(
        &mut self,
        window_idx: usize,
        max_len: usize,
    ) -> Option<&Track> {
        let len = self.order.len();
        if len == 0 || max_len == 0 {
            return None;
        }
        let cur_pos = self.cursor.unwrap_or(0).min(len - 1);
        let start = if len <= max_len {
            0
        } else {
            cur_pos.saturating_sub(25).min(len - max_len)
        };
        let target = start.saturating_add(window_idx).min(len - 1);
        self.cursor = Some(target);
        self.current()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Update the `liked` flag for all queued occurrences of a track ID.
    pub fn set_track_liked(&mut self, track_id: &str, liked: bool) {
        for track in &mut self.tracks {
            if track.id == track_id {
                track.liked = liked;
            }
        }
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

    pub fn backfill_current_duration(&mut self, duration_ms: u64) -> Option<Track> {
        if duration_ms == 0 {
            return None;
        }
        let pos = self.cursor?;
        let track_idx = *self.order.get(pos)?;
        let cur = self.tracks.get_mut(track_idx)?;
        if cur.duration_ms == 0 {
            cur.duration_ms = duration_ms;
            return Some(cur.clone());
        }
        None
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

    /// Advance at natural end-of-track (honors `RepeatMode::One` by keeping the current cursor).
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

    /// Explicit user skip or dead-stream fallback: always advances past the current track even
    /// when `RepeatMode::One` is active (wrapping to index 0 when repeat is `All` or `One`).
    pub fn next_track_manual(&mut self) -> Option<&Track> {
        if self.order.is_empty() {
            return None;
        }
        match self.cursor {
            Some(pos) => {
                if pos + 1 < self.order.len() {
                    self.cursor = Some(pos + 1);
                } else if self.repeat != RepeatMode::Off {
                    self.cursor = Some(0);
                } else {
                    return None;
                }
            }
            None => {
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
        let capped = limit.min(self.order.len());
        let mut out = Vec::with_capacity(capped);
        for offset in 1..=capped {
            let Some(next_pos) = pos.checked_add(offset) else {
                break;
            };
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
        let valid_anchor = anchor_track.filter(|&idx| idx < n);
        if let Some(anchor) = valid_anchor {
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
        self.cursor = valid_anchor.map(|_| 0);
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
            thumbnail_url: None,
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

    #[test]
    fn remove_track_by_id_compacts_and_keeps_cursor_stable_across_shuffle() {
        let tracks = vec![
            dummy_track("1", "A"),
            dummy_track("2", "B"),
            dummy_track("3", "C"),
            dummy_track("4", "D"),
        ];
        let mut q = PlayQueue::new();
        q.set_tracks(tracks, Some(2)); // playing "3"
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("3"));
        assert!(q.remove_track_by_id("1"));
        assert!(q.remove_track_by_id("2"));
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("3"));
        assert_eq!(q.len(), 2);
        // Toggling shuffle must never resurrect deleted tracks "1" or "2"
        q.set_shuffle(true);
        q.set_shuffle(false);
        assert_eq!(q.len(), 2);
        assert_eq!(q.tracks().len(), 2);
        assert!(q.tracks().iter().all(|t| t.id != "1" && t.id != "2"));
    }

    #[test]
    fn insert_play_next_and_jump_to_upcoming_work_before_first_playback() {
        let mut q = PlayQueue::new();
        q.set_tracks(vec![dummy_track("1", "A"), dummy_track("2", "B")], None);
        assert!(q.current().is_none());

        q.remove_track_by_id("2");
        q.insert_play_next(dummy_track("2", "B"));
        let played = q.jump_to_upcoming(0).cloned();
        assert_eq!(played.as_ref().map(|t| t.id.as_str()), Some("2"));
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("2"));
        assert_eq!(q.peek_upcoming(5).len(), 1);
        assert_eq!(q.peek_upcoming(5)[0].id, "1");
    }

    #[test]
    fn repeat_one_allows_manual_skip_while_auto_advance_repeats() {
        let mut q = PlayQueue::new();
        q.set_tracks(vec![dummy_track("1", "A"), dummy_track("2", "B")], Some(0));
        q.repeat = RepeatMode::One;

        assert_eq!(q.next_track().map(|t| t.id.as_str()), Some("1"));
        assert_eq!(q.next_track_manual().map(|t| t.id.as_str()), Some("2"));
        assert_eq!(q.next_track_manual().map(|t| t.id.as_str()), Some("1"));
    }

    #[test]
    fn remove_last_active_track_stops_when_repeat_off_and_wraps_when_repeat_all() {
        let mut q = PlayQueue::new();
        q.set_tracks(vec![dummy_track("1", "A"), dummy_track("2", "B")], Some(1));
        q.repeat = RepeatMode::Off;
        assert!(q.remove_track_by_id("2"));
        assert!(q.current().is_none());

        q.set_tracks(vec![dummy_track("1", "A"), dummy_track("2", "B")], Some(1));
        q.repeat = RepeatMode::All;
        assert!(q.remove_track_by_id("2"));
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("1"));
    }

    #[test]
    fn wrapped_upcoming_remove_and_move_work_under_repeat_all() {
        let mut q = PlayQueue::new();
        q.set_tracks(
            vec![
                dummy_track("1", "A"),
                dummy_track("2", "B"),
                dummy_track("3", "C"),
            ],
            Some(2), // playing "3"; upcoming with RepeatAll = ["1", "2"]
        );
        q.repeat = RepeatMode::All;
        assert!(q.move_upcoming_up(1)); // swap wrapped "2" above wrapped "1"
        let up: Vec<&str> = q.peek_upcoming(2).iter().map(|t| t.id.as_str()).collect();
        assert_eq!(up, vec!["2", "1"]);
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("3"));

        let removed = q.remove_upcoming(0);
        assert_eq!(removed.as_ref().map(|t| t.id.as_str()), Some("2"));
        assert_eq!(q.current().map(|t| t.id.as_str()), Some("3"));
    }

    #[test]
    fn ordered_queue_window_and_jump_preserve_shuffle_order() {
        let tracks: Vec<Track> = (0..10)
            .map(|i| dummy_track(&format!("{i}"), &format!("Artist {i}")))
            .collect();
        let mut q = PlayQueue::new();
        q.set_tracks(tracks, Some(4));
        q.set_shuffle(true);
        let (win, cur) = q.ordered_queue_window(200);
        assert_eq!(cur, 0);
        assert_eq!(win.len(), 10);
        assert_eq!(win[0].id, "4");
        let expected_third = win[3].id.clone();
        let jumped = q.jump_to_connect_window_index(3, 200).cloned();
        assert_eq!(
            jumped.as_ref().map(|t| t.id.as_str()),
            Some(expected_third.as_str())
        );
        let (win_after, cur_after) = q.ordered_queue_window(200);
        assert_eq!(cur_after, 3);
        assert_eq!(win_after[0].id, "4");
    }
}
