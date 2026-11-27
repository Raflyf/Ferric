//! Library grouping, local MediaStore/SAF scan ingestion, Playlist CRUD + 2-way cloud sync,
//! `.m3u`/`.csv` import/export, Discord share-link formatting, and Queue editing.

use crate::accounts::{active_spotify_token, active_ytm_session};
use crate::types::{
    AlbumSummary, ArtistSummary, CardKind, CollectionDetailSnapshot, LocalScanEntry,
};
use crate::{ControllerError, ControllerResult, FerricController};
use ferric_core::collections::{
    DetailSortOrder, PlaylistSortOrder, PlaylistSourceFilter, deduplicate_tracks_in_order,
    format_collection_csv, format_collection_tracklist, track_primary_link,
};
use ferric_core::models::{Playlist, Track, TrackSource};
use ferric_core::text::cmp_ascii_icase;
use ferric_providers::spotify_web::{
    SpotifyWebSyncError, add_tracks_to_spotify_playlist, create_spotify_playlist,
    remove_tracks_from_spotify_playlist, rename_spotify_playlist, replace_spotify_playlist_tracks,
    search_spotify_track_uri, set_spotify_track_liked, unfollow_spotify_playlist,
};
use ferric_providers::ytmusic::{
    add_videos_to_ytm_playlist, create_ytm_playlist, delete_ytm_playlist,
    remove_video_from_ytm_playlist, rename_ytm_playlist, resolve_best_video_id_for_track,
    set_ytm_track_liked,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

fn hash_prefix_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(20);
    for &b in &digest[..10] {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

fn sort_detail_tracks(tracks: &mut [Track], sort: DetailSortOrder) {
    match sort {
        DetailSortOrder::Original => {}
        DetailSortOrder::Title => {
            tracks.sort_by(|a, b| cmp_ascii_icase(&a.title, &b.title));
        }
        DetailSortOrder::Artist => {
            tracks.sort_by(|a, b| {
                cmp_ascii_icase(&a.artist, &b.artist)
                    .then_with(|| cmp_ascii_icase(&a.title, &b.title))
            });
        }
        DetailSortOrder::Duration => {
            tracks.sort_by_key(|t| t.duration_ms);
        }
    }
}

fn playlist_share_url(pl: &Playlist) -> Option<String> {
    if let Some(pid) = pl
        .id
        .strip_prefix("spotify:playlist:")
        .or_else(|| pl.id.strip_prefix("spotify:premium:playlist:"))
        .filter(|s| !s.is_empty())
    {
        return Some(format!("https://open.spotify.com/playlist/{pid}"));
    }
    if let Some(pid) = pl.id.strip_prefix("ytm:pl:").filter(|s| !s.is_empty()) {
        let clean = pid.strip_prefix("VL").unwrap_or(pid);
        return Some(format!("https://music.youtube.com/playlist?list={clean}"));
    }
    None
}

fn is_safe_playlist_import_path(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty()
        || s.bytes().any(|b| b.is_ascii_control())
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("file://")
        || s.starts_with("\\\\")
        || s.contains("..")
    {
        return false;
    }
    s.starts_with("content://") || PathBuf::from(s).is_file()
}

fn parse_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    fields
}

impl FerricController {
    /// Persist local audio entries discovered via Android `MediaStore` or SAF `DocumentFile`.
    pub fn scan_local_entries(&self, entries: Vec<LocalScanEntry>) -> ControllerResult<usize> {
        if entries.is_empty() {
            return Ok(0);
        }
        let mut tracks = Vec::with_capacity(entries.len());
        for e in entries {
            let uri = e.uri.trim();
            if uri.is_empty() || uri.bytes().any(|b| b.is_ascii_control()) {
                continue;
            }
            let title = if e.title.trim().is_empty() {
                "Untitled Track".to_string()
            } else {
                e.title.trim().to_string()
            };
            let artist = if e.artist.trim().is_empty() {
                "Unknown Artist".to_string()
            } else {
                e.artist.trim().to_string()
            };
            let album = if e.album.trim().is_empty() {
                "Local Music".to_string()
            } else {
                e.album.trim().to_string()
            };
            let codec = if e.codec.trim().is_empty() {
                "AUDIO".to_string()
            } else {
                e.codec.trim().to_ascii_uppercase()
            };
            tracks.push(Track {
                id: format!("local:{}", hash_prefix_hex(uri)),
                title,
                artist,
                album,
                duration_ms: e.duration_ms,
                path: Some(PathBuf::from(uri)),
                source: TrackSource::Local,
                codec,
                sample_rate: if e.sample_rate > 0 {
                    e.sample_rate
                } else {
                    44_100
                },
                bit_depth: if e.bit_depth > 0 { e.bit_depth } else { 16 },
                bitrate_kbps: e.bitrate_kbps,
                isrc: None,
                spotify_id: None,
                youtube_video_id: None,
                thumbnail_url: None,
                replay_gain_db: 0,
                liked: false,
            });
        }
        if tracks.is_empty() {
            return Ok(0);
        }
        let count = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let _ = st.db.delete_demo_tracks();
            let inserted = st.db.upsert_tracks(&tracks)?;
            let _ = st.db.match_cloud_tracks_to_local();
            inserted
        };
        self.sink.on_library_changed();
        self.sink
            .on_toast(format!("Scanned {count} local audio tracks"));
        Ok(count)
    }

    pub fn list_liked_songs(&self) -> ControllerResult<Vec<Track>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        Ok(all.into_iter().filter(|t| t.liked).collect())
    }

    pub fn list_local_songs(&self) -> ControllerResult<Vec<Track>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        Ok(all
            .into_iter()
            .filter(|t| t.source == TrackSource::Local)
            .collect())
    }

    pub fn list_recent_songs(&self) -> ControllerResult<Vec<Track>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        let by_id: HashMap<&str, &Track> = all.iter().map(|t| (t.id.as_str(), t)).collect();
        let mut out = Vec::new();
        for id in &st.session.recent_track_ids {
            if let Some(t) = by_id.get(id.as_str()) {
                out.push((*t).clone());
            }
        }
        Ok(out)
    }

    pub fn list_albums(&self) -> ControllerResult<Vec<AlbumSummary>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        let mut map: HashMap<(String, String), AlbumSummary> = HashMap::new();
        for t in all {
            let album = if t.album.trim().is_empty() {
                "Unknown Album".to_string()
            } else {
                t.album.trim().to_string()
            };
            let artist = if t.artist.trim().is_empty() {
                "Unknown Artist".to_string()
            } else {
                t.artist.trim().to_string()
            };
            let key = (album.to_ascii_lowercase(), artist.to_ascii_lowercase());
            let entry = map.entry(key).or_insert_with(|| AlbumSummary {
                name: album,
                artist,
                track_count: 0,
                thumbnail_url: None,
            });
            entry.track_count += 1;
            if entry.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                entry.thumbnail_url = t.thumbnail_url;
            }
        }
        let mut list: Vec<AlbumSummary> = map.into_values().collect();
        list.sort_by(|a, b| cmp_ascii_icase(&a.name, &b.name));
        Ok(list)
    }

    pub fn list_artists(&self) -> ControllerResult<Vec<ArtistSummary>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        let mut map: HashMap<String, (ArtistSummary, HashSet<String>)> = HashMap::new();
        for t in all {
            let artist = if t.artist.trim().is_empty() {
                "Unknown Artist".to_string()
            } else {
                t.artist.trim().to_string()
            };
            let key = artist.to_ascii_lowercase();
            let (summary, albums) = map.entry(key).or_insert_with(|| {
                (
                    ArtistSummary {
                        name: artist,
                        track_count: 0,
                        album_count: 0,
                        thumbnail_url: None,
                    },
                    HashSet::new(),
                )
            });
            summary.track_count += 1;
            if !t.album.trim().is_empty() {
                albums.insert(t.album.trim().to_ascii_lowercase());
                summary.album_count = albums.len();
            }
            if summary.thumbnail_url.is_none() && t.thumbnail_url.is_some() {
                summary.thumbnail_url = t.thumbnail_url;
            }
        }
        let mut list: Vec<ArtistSummary> = map.into_values().map(|(s, _)| s).collect();
        list.sort_by(|a, b| {
            b.track_count
                .cmp(&a.track_count)
                .then_with(|| cmp_ascii_icase(&a.name, &b.name))
        });
        Ok(list)
    }

    pub fn list_playlists_filtered(
        &self,
        source: PlaylistSourceFilter,
        sort: PlaylistSortOrder,
    ) -> ControllerResult<Vec<Playlist>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut list: Vec<Playlist> = st
            .db
            .list_playlists()?
            .into_iter()
            .filter(|p| source.matches_playlist(p))
            .collect();
        sort.apply(&mut list);
        Ok(list)
    }

    pub fn get_playlist_detail(
        &self,
        playlist_id: &str,
        sort: DetailSortOrder,
    ) -> ControllerResult<CollectionDetailSnapshot> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let playlists = st.db.list_playlists()?;
        let Some(pl) = playlists.into_iter().find(|p| p.id == playlist_id) else {
            return Err(ControllerError::State(format!(
                "playlist not found: {playlist_id}"
            )));
        };
        let mut tracks = st.db.list_playlist_tracks(&pl.id)?;
        sort_detail_tracks(&mut tracks, sort);
        let thumb = pl
            .thumbnail_url
            .clone()
            .or_else(|| tracks.iter().find_map(|t| t.thumbnail_url.clone()));
        let share_url = playlist_share_url(&pl);
        let can_sync = pl.source == TrackSource::Local
            && (active_spotify_token(&st.accounts).is_some() || st.accounts.youtube_connected);
        Ok(CollectionDetailSnapshot {
            id: pl.id,
            title: pl.name,
            subtitle: format!("{} · {} songs", pl.source.as_badge(), tracks.len()),
            description: pl.description,
            kind: CardKind::Playlist,
            source: pl.source,
            thumbnail_url: thumb,
            share_url,
            can_sync_to_account: can_sync,
            is_editable_playlist: true,
            tracks,
        })
    }

    pub fn get_album_detail(
        &self,
        album_name: &str,
        artist_name: &str,
        sort: DetailSortOrder,
    ) -> ControllerResult<CollectionDetailSnapshot> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        let mut tracks: Vec<Track> = all
            .into_iter()
            .filter(|t| {
                t.album.trim().eq_ignore_ascii_case(album_name.trim())
                    && (artist_name.trim().is_empty()
                        || t.artist.trim().eq_ignore_ascii_case(artist_name.trim()))
            })
            .collect();
        sort_detail_tracks(&mut tracks, sort);
        let thumb = tracks.iter().find_map(|t| t.thumbnail_url.clone());
        let source = tracks.first().map_or(TrackSource::Local, |t| t.source);
        Ok(CollectionDetailSnapshot {
            id: format!(
                "album:{}",
                hash_prefix_hex(&format!("{album_name}|{artist_name}"))
            ),
            title: album_name.trim().to_string(),
            subtitle: format!("{} · {} songs", artist_name.trim(), tracks.len()),
            description: String::new(),
            kind: CardKind::Album,
            source,
            thumbnail_url: thumb,
            share_url: None,
            can_sync_to_account: false,
            is_editable_playlist: false,
            tracks,
        })
    }

    pub fn get_artist_detail(
        &self,
        artist_name: &str,
        sort: DetailSortOrder,
    ) -> ControllerResult<CollectionDetailSnapshot> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        let target = artist_name.trim();
        let mut tracks: Vec<Track> = all
            .into_iter()
            .filter(|t| {
                t.artist.trim().eq_ignore_ascii_case(target)
                    || ferric_core::text::contains_ascii_icase(&t.artist, target)
            })
            .collect();
        sort_detail_tracks(&mut tracks, sort);
        let thumb = tracks.iter().find_map(|t| t.thumbnail_url.clone());
        let source = tracks.first().map_or(TrackSource::Local, |t| t.source);
        Ok(CollectionDetailSnapshot {
            id: format!("artist:{}", hash_prefix_hex(target)),
            title: target.to_string(),
            subtitle: format!("Artist · {} songs in library", tracks.len()),
            description: String::new(),
            kind: CardKind::Artist,
            source,
            thumbnail_url: thumb,
            share_url: None,
            can_sync_to_account: false,
            is_editable_playlist: false,
            tracks,
        })
    }

    pub fn get_liked_detail(
        &self,
        sort: DetailSortOrder,
    ) -> ControllerResult<CollectionDetailSnapshot> {
        let mut tracks = self.list_liked_songs()?;
        sort_detail_tracks(&mut tracks, sort);
        let thumb = tracks.iter().find_map(|t| t.thumbnail_url.clone());
        Ok(CollectionDetailSnapshot {
            id: "liked:songs".to_string(),
            title: "Liked Songs".to_string(),
            subtitle: format!("{} songs", tracks.len()),
            description: "Your liked tracks across local and synced accounts".to_string(),
            kind: CardKind::Playlist,
            source: TrackSource::Local,
            thumbnail_url: thumb,
            share_url: None,
            can_sync_to_account: false,
            is_editable_playlist: false,
            tracks,
        })
    }

    /// Create a local playlist (optionally populated with `track_ids`).
    pub fn create_playlist(
        &self,
        name: String,
        description: String,
        track_ids: Vec<String>,
    ) -> ControllerResult<Playlist> {
        let clean_name = name.trim().to_string();
        if clean_name.is_empty() {
            return Err(ControllerError::State(
                "Playlist name cannot be empty".to_string(),
            ));
        }
        let id = format!("local:pl:{:016x}", fastrand::u64(..));
        let pl = Playlist {
            id,
            name: clean_name,
            source: TrackSource::Local,
            etag: None,
            track_count: track_ids.len(),
            description: description.trim().to_string(),
            thumbnail_url: None,
        };
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.upsert_playlist(&pl, &track_ids)?;
        }
        self.sink.on_library_changed();
        Ok(pl)
    }

    /// Rename a playlist locally and mirror to Spotify / YouTube Music if synced.
    pub fn rename_playlist(
        self: &Arc<Self>,
        playlist_id: String,
        new_name: String,
    ) -> ControllerResult<()> {
        let clean_name = new_name.trim().to_string();
        if clean_name.is_empty() {
            return Err(ControllerError::State(
                "Playlist name cannot be empty".to_string(),
            ));
        }
        let (sp_tok, ytm_sess) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.rename_playlist(&playlist_id, &clean_name)?;
            (
                active_spotify_token(&st.accounts),
                active_ytm_session(&st.accounts),
            )
        };
        self.sink.on_library_changed();

        if playlist_id.starts_with("spotify:")
            && let Some(tok) = sp_tok
        {
            let this = Arc::clone(self);
            let pid = playlist_id;
            let _ = thread::Builder::new()
                .name("ferric-sp-rename".to_string())
                .spawn(move || {
                    if let Err(SpotifyWebSyncError::AuthExpired) =
                        rename_spotify_playlist(&tok, &pid, &clean_name)
                    {
                        this.sink.on_spotify_auth_expired();
                    }
                });
        } else if playlist_id.starts_with("ytm:pl:") && ytm_sess.is_signed_in() {
            let pid = playlist_id;
            let _ = thread::Builder::new()
                .name("ferric-ytm-rename".to_string())
                .spawn(move || {
                    let _ = rename_ytm_playlist(&ytm_sess, &pid, &clean_name);
                });
        }
        Ok(())
    }

    /// Delete a playlist locally and unfollow/delete on Spotify or YouTube Music if synced.
    pub fn delete_playlist(self: &Arc<Self>, playlist_id: String) -> ControllerResult<()> {
        let (sp_tok, ytm_sess) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.delete_playlist(&playlist_id)?;
            (
                active_spotify_token(&st.accounts),
                active_ytm_session(&st.accounts),
            )
        };
        self.sink.on_library_changed();

        if playlist_id.starts_with("spotify:")
            && let Some(tok) = sp_tok
        {
            let this = Arc::clone(self);
            let pid = playlist_id;
            let _ = thread::Builder::new()
                .name("ferric-sp-del".to_string())
                .spawn(move || {
                    if let Err(SpotifyWebSyncError::AuthExpired) =
                        unfollow_spotify_playlist(&tok, &pid)
                    {
                        this.sink.on_spotify_auth_expired();
                    }
                });
        } else if playlist_id.starts_with("ytm:pl:") && ytm_sess.is_signed_in() {
            let pid = playlist_id;
            let _ = thread::Builder::new()
                .name("ferric-ytm-del".to_string())
                .spawn(move || {
                    let _ = delete_ytm_playlist(&ytm_sess, &pid);
                });
        }
        Ok(())
    }

    /// Add a track to a playlist locally (upserting track first if needed) and sync to cloud if linked.
    pub fn add_track_to_playlist(
        self: &Arc<Self>,
        playlist_id: String,
        track: Track,
    ) -> ControllerResult<()> {
        let (sp_tok, ytm_sess) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.upsert_tracks(std::slice::from_ref(&track))?;
            st.db.add_track_to_playlist(&playlist_id, &track.id)?;
            (
                active_spotify_token(&st.accounts),
                active_ytm_session(&st.accounts),
            )
        };
        self.sink.on_library_changed();

        if playlist_id.starts_with("spotify:")
            && let Some(tok) = sp_tok
        {
            let this = Arc::clone(self);
            let pid = playlist_id;
            let _ = thread::Builder::new()
                .name("ferric-sp-add".to_string())
                .spawn(move || {
                    let uri = track.spotify_id.clone().or_else(|| {
                        search_spotify_track_uri(&tok, &track.artist, &track.title)
                            .ok()
                            .flatten()
                    });
                    if let Some(u) = uri
                        && let Err(SpotifyWebSyncError::AuthExpired) =
                            add_tracks_to_spotify_playlist(&tok, &pid, &[u])
                    {
                        this.sink.on_spotify_auth_expired();
                    }
                });
        } else if playlist_id.starts_with("ytm:pl:") && ytm_sess.is_signed_in() {
            let pid = playlist_id;
            let _ = thread::Builder::new()
                .name("ferric-ytm-add".to_string())
                .spawn(move || {
                    let vid = track.youtube_video_id.clone().or_else(|| {
                        resolve_best_video_id_for_track(&ytm_sess, &track.artist, &track.title)
                            .ok()
                            .flatten()
                    });
                    if let Some(v) = vid {
                        let _ = add_videos_to_ytm_playlist(&ytm_sess, &pid, &[v]);
                    }
                });
        }
        Ok(())
    }

    /// Remove a track from a playlist locally and mirror to Spotify / YouTube Music if synced.
    pub fn remove_track_from_playlist(
        self: &Arc<Self>,
        playlist_id: String,
        track_id: String,
    ) -> ControllerResult<()> {
        let (track_opt, sp_tok, ytm_sess) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let track = st
                .db
                .list_playlist_tracks(&playlist_id)
                .ok()
                .and_then(|list| list.into_iter().find(|t| t.id == track_id));
            st.db.remove_track_from_playlist(&playlist_id, &track_id)?;
            (
                track,
                active_spotify_token(&st.accounts),
                active_ytm_session(&st.accounts),
            )
        };
        self.sink.on_library_changed();

        if let Some(track) = track_opt {
            if playlist_id.starts_with("spotify:")
                && let (Some(tok), Some(sp_id)) = (sp_tok, track.spotify_id)
            {
                let this = Arc::clone(self);
                let pid = playlist_id;
                let _ = thread::Builder::new()
                    .name("ferric-sp-rem".to_string())
                    .spawn(move || {
                        if let Err(SpotifyWebSyncError::AuthExpired) =
                            remove_tracks_from_spotify_playlist(&tok, &pid, &[sp_id])
                        {
                            this.sink.on_spotify_auth_expired();
                        }
                    });
            } else if playlist_id.starts_with("ytm:pl:")
                && ytm_sess.is_signed_in()
                && let Some(vid) = track.youtube_video_id
            {
                let pid = playlist_id;
                let _ = thread::Builder::new()
                    .name("ferric-ytm-rem".to_string())
                    .spawn(move || {
                        let _ = remove_video_from_ytm_playlist(&ytm_sess, &pid, &vid);
                    });
            }
        }
        Ok(())
    }

    /// Deduplicate tracks inside a playlist in-place (`O(N)`) and mirror replacement to Spotify if synced.
    pub fn deduplicate_playlist(self: &Arc<Self>, playlist_id: String) -> ControllerResult<usize> {
        let (removed, kept_tracks, sp_tok) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let tracks = st.db.list_playlist_tracks(&playlist_id)?;
            let (deduped, removed) = deduplicate_tracks_in_order(&tracks);
            if removed > 0 {
                let ids: Vec<String> = deduped.iter().map(|t| t.id.clone()).collect();
                st.db.replace_playlist_tracks(&playlist_id, &ids)?;
            }
            (removed, deduped, active_spotify_token(&st.accounts))
        };
        if removed > 0 {
            self.sink.on_library_changed();
            self.sink
                .on_toast(format!("Removed {removed} duplicate tracks"));
            if playlist_id.starts_with("spotify:")
                && let Some(tok) = sp_tok
            {
                let uris: Vec<String> = kept_tracks
                    .into_iter()
                    .filter_map(|t| t.spotify_id)
                    .collect();
                let this = Arc::clone(self);
                let pid = playlist_id;
                let _ = thread::Builder::new()
                    .name("ferric-sp-dedup".to_string())
                    .spawn(move || {
                        if let Err(SpotifyWebSyncError::AuthExpired) =
                            replace_spotify_playlist_tracks(&tok, &pid, &uris)
                        {
                            this.sink.on_spotify_auth_expired();
                        }
                    });
            }
        } else {
            self.sink.on_toast("No duplicates found".to_string());
        }
        Ok(removed)
    }

    /// Push a local playlist up to the user's connected Spotify or YouTube Music account (`Sync to Account`).
    pub fn push_local_playlist_to_cloud(
        self: &Arc<Self>,
        playlist_id: String,
    ) -> ControllerResult<()> {
        let (pl, tracks, sp_tok, ytm_sess) = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let Some(pl) = st
                .db
                .list_playlists()?
                .into_iter()
                .find(|p| p.id == playlist_id)
            else {
                return Err(ControllerError::State(format!(
                    "playlist not found: {playlist_id}"
                )));
            };
            let tracks = st.db.list_playlist_tracks(&playlist_id)?;
            let sp = active_spotify_token(&st.accounts);
            let ytm = active_ytm_session(&st.accounts);
            if sp.is_none() && !ytm.is_signed_in() {
                return Err(ControllerError::State(
                    "Sign in to Spotify or YouTube Music first".to_string(),
                ));
            }
            (pl, tracks, sp, ytm)
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-push-pl".to_string())
            .spawn(move || {
                if let Some(tok) = sp_tok {
                    let mut uris = Vec::with_capacity(tracks.len());
                    for t in &tracks {
                        if let Some(sp) = &t.spotify_id {
                            uris.push(sp.clone());
                        } else if let Ok(Some(found)) =
                            search_spotify_track_uri(&tok, &t.artist, &t.title)
                        {
                            uris.push(found);
                        }
                    }
                    match create_spotify_playlist(&tok, &pl.name, &pl.description, &uris) {
                        Ok(new_id) => {
                            {
                                let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                                let _ = st.db.migrate_playlist_id(
                                    &pl.id,
                                    &new_id,
                                    TrackSource::Spotify,
                                );
                            }
                            this.sink.on_library_changed();
                            this.sink
                                .on_toast(format!("Synced \"{}\" to Spotify", pl.name));
                            return;
                        }
                        Err(SpotifyWebSyncError::AuthExpired) => {
                            this.sink.on_spotify_auth_expired();
                            this.sink.on_toast(
                                "Spotify token expired; refreshing session...".to_string(),
                            );
                            return;
                        }
                        Err(SpotifyWebSyncError::FallbackToRest(err)) => {
                            this.sink
                                .on_toast(format!("Spotify playlist sync failed: {err}"));
                        }
                    }
                }

                if ytm_sess.is_signed_in() {
                    let mut vids = Vec::with_capacity(tracks.len());
                    for t in &tracks {
                        if let Some(v) = &t.youtube_video_id {
                            vids.push(v.clone());
                        } else if let Ok(Some(found)) =
                            resolve_best_video_id_for_track(&ytm_sess, &t.artist, &t.title)
                        {
                            vids.push(found);
                        }
                    }
                    match create_ytm_playlist(&ytm_sess, &pl.name, &pl.description, &vids) {
                        Ok(new_id) => {
                            {
                                let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                                let _ = st.db.migrate_playlist_id(
                                    &pl.id,
                                    &new_id,
                                    TrackSource::YouTubeMusic,
                                );
                            }
                            this.sink.on_library_changed();
                            this.sink
                                .on_toast(format!("Synced \"{}\" to YouTube Music", pl.name));
                        }
                        Err(err) => {
                            this.sink
                                .on_toast(format!("YouTube Music playlist sync failed: {err}"));
                        }
                    }
                }
            });
        Ok(())
    }

    /// Toggle Like locally + in active queue + mirror asynchronously to Spotify and/or YTM.
    pub fn toggle_like(self: &Arc<Self>, track_id: &str) -> ControllerResult<bool> {
        let (new_liked, track_opt, cur_changed, sp_tok, ytm_sess) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut found = st
                .db
                .list_tracks("")?
                .into_iter()
                .find(|t| t.id == track_id);
            if found.is_none() {
                found = st.queue.tracks().iter().find(|t| t.id == track_id).cloned();
                if let Some(ref t) = found {
                    let _ = st.db.upsert_tracks(std::slice::from_ref(t));
                }
            }
            let next_liked = !found.as_ref().is_some_and(|t| t.liked);
            st.db.set_track_liked(track_id, next_liked)?;
            st.queue.set_track_liked(track_id, next_liked);
            let cur = st.queue.current().cloned();
            let is_cur = cur.as_ref().is_some_and(|t| t.id == track_id);
            (
                next_liked,
                found,
                if is_cur { Some(cur) } else { None },
                active_spotify_token(&st.accounts),
                active_ytm_session(&st.accounts),
            )
        };
        if let Some(cur) = cur_changed {
            self.sink.on_now_playing(cur);
        }
        self.sink.on_library_changed();

        if let Some(track) = track_opt {
            if let (Some(tok), Some(sp_id)) = (sp_tok, track.spotify_id.clone()) {
                let this = Arc::clone(self);
                let _ = thread::Builder::new()
                    .name("ferric-sp-like".to_string())
                    .spawn(move || {
                        if let Err(SpotifyWebSyncError::AuthExpired) =
                            set_spotify_track_liked(&tok, &sp_id, new_liked)
                        {
                            this.sink.on_spotify_auth_expired();
                        }
                    });
            }
            if ytm_sess.is_signed_in()
                && let Some(vid) = track.youtube_video_id
            {
                let _ = thread::Builder::new()
                    .name("ferric-ytm-like".to_string())
                    .spawn(move || {
                        let _ = set_ytm_track_liked(&ytm_sess, &vid, new_liked);
                    });
            }
        }
        Ok(new_liked)
    }

    /// Build a direct Spotify / YouTube Music share URL for a track (Discord music bot friendly).
    #[must_use]
    pub fn track_share_link(&self, track: &Track) -> Option<String> {
        track_primary_link(track).map(|(_, url)| url)
    }

    /// Format any collection's numbered tracklist for clipboard sharing.
    #[must_use]
    pub fn format_share_tracklist(
        &self,
        title: &str,
        share_url: Option<&str>,
        tracks: &[Track],
    ) -> String {
        format_collection_tracklist(title, share_url, tracks)
    }

    /// Format any collection's tracks as RFC 4180 CSV.
    #[must_use]
    pub fn format_share_csv(&self, tracks: &[Track]) -> String {
        format_collection_csv(tracks)
    }

    /// Import a `.m3u` / `.m3u8` or `.csv` playlist text safely (rejecting traversal & remote paths).
    pub fn import_playlist_text(
        &self,
        playlist_name: String,
        raw_text: String,
    ) -> ControllerResult<Playlist> {
        let clean_name = if playlist_name.trim().is_empty() {
            "Imported Playlist".to_string()
        } else {
            playlist_name.trim().to_string()
        };
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let library = st.db.list_tracks("")?;
        let mut matched_ids: Vec<String> = Vec::new();
        let mut new_local_tracks: Vec<Track> = Vec::new();

        let trimmed = raw_text.trim();
        let is_csv = trimmed
            .lines()
            .next()
            .is_some_and(|hdr| hdr.to_ascii_lowercase().starts_with("title,artist"));

        if is_csv {
            for line in trimmed.lines().skip(1) {
                let cols = parse_csv_row(line);
                if cols.len() < 2 {
                    continue;
                }
                let title = cols[0].trim();
                let artist = cols[1].trim();
                if title.is_empty() {
                    continue;
                }
                if let Some(found) = library.iter().find(|t| {
                    t.title.trim().eq_ignore_ascii_case(title)
                        && (artist.is_empty() || t.artist.trim().eq_ignore_ascii_case(artist))
                }) {
                    matched_ids.push(found.id.clone());
                }
            }
        } else {
            let mut pending_meta: Option<(String, String)> = None;
            for raw_line in trimmed.lines() {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(rest) = line.strip_prefix("#EXTINF:") {
                    if let Some((_, label)) = rest.split_once(',') {
                        if let Some((a, t)) = label.split_once(" - ") {
                            pending_meta = Some((a.trim().to_string(), t.trim().to_string()));
                        } else {
                            pending_meta = Some((String::new(), label.trim().to_string()));
                        }
                    }
                    continue;
                }
                if line.starts_with('#') {
                    continue;
                }
                if is_safe_playlist_import_path(line) {
                    if let Some(existing) = library.iter().find(|t| {
                        t.path
                            .as_ref()
                            .and_then(|p| p.to_str())
                            .is_some_and(|p| p == line)
                    }) {
                        matched_ids.push(existing.id.clone());
                    } else {
                        let (artist, title) = pending_meta.take().unwrap_or_else(|| {
                            (
                                "Unknown Artist".to_string(),
                                PathBuf::from(line)
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("Imported Track")
                                    .to_string(),
                            )
                        });
                        let id = format!("local:{}", hash_prefix_hex(line));
                        new_local_tracks.push(Track {
                            id: id.clone(),
                            title,
                            artist: if artist.is_empty() {
                                "Unknown Artist".to_string()
                            } else {
                                artist
                            },
                            album: clean_name.clone(),
                            duration_ms: 0,
                            path: Some(PathBuf::from(line)),
                            source: TrackSource::Local,
                            codec: "AUDIO".to_string(),
                            sample_rate: 44_100,
                            bit_depth: 16,
                            bitrate_kbps: 0,
                            isrc: None,
                            spotify_id: None,
                            youtube_video_id: None,
                            thumbnail_url: None,
                            replay_gain_db: 0,
                            liked: false,
                        });
                        matched_ids.push(id);
                    }
                } else if let Some((artist, title)) = pending_meta.take()
                    && let Some(found) = library.iter().find(|t| {
                        t.title.trim().eq_ignore_ascii_case(&title)
                            && (artist.is_empty() || t.artist.trim().eq_ignore_ascii_case(&artist))
                    })
                {
                    matched_ids.push(found.id.clone());
                }
            }
        }

        if !new_local_tracks.is_empty() {
            st.db.upsert_tracks(&new_local_tracks)?;
        }
        let pl = Playlist {
            id: format!("local:pl:{:016x}", fastrand::u64(..)),
            name: clean_name,
            source: TrackSource::Local,
            etag: None,
            track_count: matched_ids.len(),
            description: "Imported playlist".to_string(),
            thumbnail_url: None,
        };
        st.db.upsert_playlist(&pl, &matched_ids)?;
        drop(st);
        self.sink.on_library_changed();
        self.sink.on_toast(format!(
            "Imported playlist \"{}\" ({} tracks)",
            pl.name, pl.track_count
        ));
        Ok(pl)
    }

    // --- Queue operations ---

    pub fn play_next(&self, track: Track) {
        let up_next = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.queue.insert_play_next(track.clone());
            if st.queue.current().is_some() {
                st.queue.peek_upcoming(50).into_iter().cloned().collect()
            } else {
                st.queue.tracks().iter().take(50).cloned().collect()
            }
        };
        self.queue_upcoming_deck_b();
        self.sink.on_queue_changed(up_next);
        self.sink.on_toast(format!("Playing next: {}", track.title));
    }

    pub fn append_to_queue(&self, track: Track) {
        let up_next = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.queue.append_single_to_end(track.clone());
            if st.queue.current().is_some() {
                st.queue.peek_upcoming(50).into_iter().cloned().collect()
            } else {
                st.queue.tracks().iter().take(50).cloned().collect()
            }
        };
        self.queue_upcoming_deck_b();
        self.sink.on_queue_changed(up_next);
        self.sink
            .on_toast(format!("Added to queue: {}", track.title));
    }

    pub fn jump_to_upcoming(self: &Arc<Self>, offset: usize) -> ControllerResult<()> {
        self.ensure_audio_started()?;
        let (target, up_next, gen_id) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let next_t = st.queue.jump_to_upcoming(offset).cloned();
            st.resolve_gen = st.resolve_gen.wrapping_add(1);
            let upcoming: Vec<Track> = st.queue.peek_upcoming(50).into_iter().cloned().collect();
            (next_t, upcoming, st.resolve_gen)
        };
        self.sink.on_now_playing(target.clone());
        self.sink.on_queue_changed(up_next);
        if let Some(t) = target {
            self.dispatch_track_playback(&t, gen_id)?;
        }
        self.wake_monitor();
        Ok(())
    }

    pub fn move_queue_item_up(&self, offset: usize) -> bool {
        let (moved, up_next) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let ok = st.queue.move_upcoming_up(offset);
            let list: Vec<Track> = st.queue.peek_upcoming(50).into_iter().cloned().collect();
            (ok, list)
        };
        if moved {
            self.queue_upcoming_deck_b();
            self.sink.on_queue_changed(up_next);
        }
        moved
    }

    pub fn move_queue_item_down(&self, offset: usize) -> bool {
        self.move_queue_item_up(offset.saturating_add(1))
    }

    pub fn remove_queue_item(&self, offset: usize) -> bool {
        let (removed, up_next) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let ok = st.queue.remove_upcoming(offset).is_some();
            let list: Vec<Track> = st.queue.peek_upcoming(50).into_iter().cloned().collect();
            (ok, list)
        };
        if removed {
            self.queue_upcoming_deck_b();
            self.sink.on_queue_changed(up_next);
        }
        removed
    }

    pub fn clear_up_next(&self) {
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(cur) = st.queue.current().cloned() {
                st.queue.set_tracks(vec![cur], Some(0));
            } else {
                st.queue.set_tracks(Vec::new(), None);
            }
        }
        self.queue_upcoming_deck_b();
        self.sink.on_queue_changed(Vec::new());
    }

    pub fn save_queue_as_playlist(&self, name: String) -> ControllerResult<Playlist> {
        let tracks: Vec<Track> = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut list = Vec::new();
            if let Some(cur) = st.queue.current() {
                list.push(cur.clone());
                list.extend(st.queue.peek_upcoming(200).into_iter().cloned());
            } else {
                list.extend(st.queue.tracks().iter().take(200).cloned());
            }
            list
        };
        if tracks.is_empty() {
            return Err(ControllerError::State("Queue is empty".to_string()));
        }
        let ids: Vec<String> = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.upsert_tracks(&tracks)?;
            tracks.iter().map(|t| t.id.clone()).collect()
        };
        self.create_playlist(name, "Saved from Play Queue".to_string(), ids)
    }

    #[must_use]
    pub fn copy_queue_tracklist(&self) -> String {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let mut list = Vec::new();
        if let Some(cur) = st.queue.current() {
            list.push(cur.clone());
            list.extend(st.queue.peek_upcoming(200).into_iter().cloned());
        } else {
            list.extend(st.queue.tracks().iter().take(200).cloned());
        }
        format_collection_tracklist("Ferric Queue", None, &list)
    }
}
