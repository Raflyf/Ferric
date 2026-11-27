//! Home taste aggregation, weighted Refresh, dual-engine Recommended Stations,
//! instant local + live online search, browse detail, and public URL import.

use crate::accounts::{active_spotify_token, active_ytm_session};
use crate::types::{
    CardKind, CollectionDetailSnapshot, HomeShelfSnapshot, HomeSnapshot, MoodChipSnapshot,
    SearchSnapshot, ShelfCardSnapshot, StationCardSnapshot,
};
use crate::{ControllerError, ControllerResult, FerricController};
use ferric_core::models::{Track, TrackSource};
use ferric_core::text::contains_ascii_icase;
use ferric_providers::public_sync::{
    import_spotify_public_url, import_youtube_public_url, parse_spotify_target,
    parse_youtube_target, search_youtube_public,
};
use ferric_providers::spotify_web::{
    SpotifyWebSyncError, fetch_spotify_playlist_by_id, fetch_spotify_recommended_stations,
};
use ferric_providers::ytmusic::{
    RecommendedStation, YtmHomeTasteProfile, build_algorithmic_recommended_stations_rotated,
    expand_recommended_station_tracks, fetch_browse_detail, fetch_rich_home, fetch_rich_search,
    search_music,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;

fn track_share_url_for_browse(browse_id: &str, kind: CardKind) -> Option<String> {
    let bid = browse_id.trim();
    if bid.is_empty() {
        return None;
    }
    match kind {
        CardKind::Playlist => {
            let clean = bid.strip_prefix("VL").unwrap_or(bid);
            Some(format!("https://music.youtube.com/playlist?list={clean}"))
        }
        CardKind::Album | CardKind::Artist => {
            Some(format!("https://music.youtube.com/browse/{bid}"))
        }
        CardKind::Song => Some(format!("https://music.youtube.com/watch?v={bid}")),
    }
}

impl FerricController {
    #[must_use]
    pub fn get_cached_home(&self) -> HomeSnapshot {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .home_cache
            .clone()
    }

    #[must_use]
    pub fn recent_searches(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .session
            .recent_searches
            .clone()
    }

    pub fn clear_recent_searches(&self) -> ControllerResult<()> {
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.session.recent_searches.clear();
        st.db.save_session_state(&st.session)?;
        Ok(())
    }

    /// Instant zero-allocation local search over SQLite library using `ferric_core::text`.
    pub fn instant_search_local(&self, query: &str) -> ControllerResult<Vec<Track>> {
        let q = query.trim();
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let all = st.db.list_tracks("")?;
        if q.is_empty() {
            return Ok(all.into_iter().take(60).collect());
        }
        let tokens: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<Track> = all
            .into_iter()
            .filter(|t| {
                tokens.iter().all(|tok| {
                    contains_ascii_icase(&t.title, tok)
                        || contains_ascii_icase(&t.artist, tok)
                        || contains_ascii_icase(&t.album, tok)
                })
            })
            .collect();
        hits.truncate(80);
        Ok(hits)
    }

    /// Load or weighted-refresh the Home screen asynchronously (Mood Chips, Quick Picks,
    /// Dual-Engine Recommended Stations, and personalized shelves).
    pub fn load_home(self: &Arc<Self>, force_refresh: bool, mood_idx: Option<usize>) {
        let (
            gen_id,
            session,
            spotify_token,
            taste,
            library_tracks,
            mood_params,
            rotation_epoch,
            existing_chips,
        ) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.home_gen = st.home_gen.wrapping_add(1);
            let gen_val = st.home_gen;
            if force_refresh {
                st.station_rotation_epoch = st.station_rotation_epoch.wrapping_add(1);
            }
            let lib = st.db.list_tracks("").unwrap_or_default();

            let mut top_tracks: Vec<(u32, Track)> = Vec::new();
            let recent_set: HashSet<&str> = st
                .session
                .recent_track_ids
                .iter()
                .map(String::as_str)
                .collect();
            for t in &lib {
                let plays = st
                    .session
                    .track_play_counts
                    .get(&t.id)
                    .copied()
                    .unwrap_or(0);
                let mut weight = plays.saturating_mul(4);
                if t.liked {
                    weight = weight.saturating_add(6);
                }
                if recent_set.contains(t.id.as_str()) {
                    weight = weight.saturating_add(5);
                }
                if weight > 0 {
                    top_tracks.push((weight, t.clone()));
                }
            }
            top_tracks.sort_by_key(|a| std::cmp::Reverse(a.0));
            let top_tracks: Vec<Track> = top_tracks.into_iter().take(48).map(|(_, t)| t).collect();

            let mut artist_scores: HashMap<String, u32> = st.session.artist_play_counts.clone();
            for t in &top_tracks {
                let entry = artist_scores.entry(t.artist.clone()).or_insert(0);
                *entry = entry.saturating_add(1);
            }
            let mut top_artists: Vec<(String, u32)> = artist_scores.into_iter().collect();
            top_artists.sort_by_key(|a| std::cmp::Reverse(a.1));
            let top_artists: Vec<String> =
                top_artists.into_iter().take(24).map(|(a, _)| a).collect();

            let chips_snapshot = st.home_cache.chips.clone();
            let mood_params = mood_idx
                .and_then(|idx| chips_snapshot.get(idx))
                .map(|c| c.params.clone())
                .filter(|p| !p.is_empty());

            let refresh_mood_params =
                if force_refresh && mood_idx.is_none() && !chips_snapshot.is_empty() {
                    let pick = st.station_rotation_epoch % chips_snapshot.len();
                    Some(chips_snapshot[pick].params.clone())
                } else {
                    None
                };

            let taste = YtmHomeTasteProfile {
                top_tracks,
                library_tracks: lib.iter().take(120).cloned().collect(),
                top_artists,
                recent_searches: st.session.recent_searches.clone(),
                excluded_track_ids: if force_refresh {
                    st.home_excluded_tracks.clone()
                } else {
                    HashSet::new()
                },
                excluded_card_ids: if force_refresh {
                    st.home_excluded_cards.clone()
                } else {
                    HashSet::new()
                },
                excluded_artists: if force_refresh {
                    st.home_excluded_artists.clone()
                } else {
                    HashSet::new()
                },
                continuation_token: st.home_continuation.clone(),
                refresh_mood_params,
            };

            st.home_cache.is_loading = true;
            st.home_cache.selected_mood_idx = mood_idx;
            let loading_snap = st.home_cache.clone();
            let sess = active_ytm_session(&st.accounts);
            let sp_tok = active_spotify_token(&st.accounts);
            let rot = st.station_rotation_epoch;
            drop(st);
            self.sink.on_home_updated(loading_snap);
            (
                gen_val,
                sess,
                sp_tok,
                taste,
                lib,
                mood_params,
                rot,
                chips_snapshot,
            )
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-home-load".to_string())
            .spawn(move || {
                let ytm_res =
                    fetch_rich_home(&session, mood_params.as_deref(), force_refresh, &taste);

                let mut stations: Vec<RecommendedStation> = Vec::new();
                if let Some(tok) = spotify_token.as_deref() {
                    match fetch_spotify_recommended_stations(tok) {
                        Ok(sp_cards) => {
                            for (idx, card) in sp_cards.iter().take(8).enumerate() {
                                stations.push(RecommendedStation::from_spotify_card(
                                    card,
                                    (idx % 6) as u8,
                                ));
                            }
                        }
                        Err(SpotifyWebSyncError::AuthExpired) => {
                            this.sink.on_spotify_auth_expired();
                        }
                        Err(_) => {}
                    }
                }

                match ytm_res {
                    Ok(rich) => {
                        let algo_needed = 10usize.saturating_sub(stations.len());
                        if algo_needed > 0 {
                            let algo = build_algorithmic_recommended_stations_rotated(
                                &taste,
                                &library_tracks,
                                &rich.all_tracks,
                                algo_needed,
                                rotation_epoch,
                            );
                            stations.extend(algo);
                        }

                        let chips: Vec<MoodChipSnapshot> = if rich.chips.is_empty() {
                            existing_chips
                        } else {
                            rich.chips.into_iter().map(Into::into).collect()
                        };
                        let quick_picks = if rich.quick_picks.is_empty() {
                            library_tracks.iter().take(12).cloned().collect()
                        } else {
                            rich.quick_picks
                        };
                        let shelves: Vec<HomeShelfSnapshot> =
                            rich.shelves.into_iter().map(Into::into).collect();
                        let station_snaps: Vec<StationCardSnapshot> =
                            stations.iter().map(Into::into).collect();

                        let final_snap = {
                            let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                            if st.home_gen != gen_id {
                                return;
                            }
                            if !rich.visitor_data.is_empty()
                                && st.accounts.youtube_visitor_data.is_empty()
                            {
                                st.accounts.youtube_visitor_data = rich.visitor_data;
                            }
                            st.home_continuation = rich.continuation_token;

                            st.home_excluded_tracks.clear();
                            for t in &quick_picks {
                                st.home_excluded_tracks.insert(t.id.clone());
                                if let Some(vid) = &t.youtube_video_id {
                                    st.home_excluded_tracks.insert(vid.clone());
                                }
                            }
                            st.home_excluded_cards.clear();
                            for sh in &shelves {
                                for c in &sh.cards {
                                    st.home_excluded_cards.insert(c.id.clone());
                                }
                            }
                            st.home_excluded_artists.clear();
                            for s in &stations {
                                if !s.seed_artist.is_empty() {
                                    st.home_excluded_artists.insert(s.seed_artist.clone());
                                }
                            }

                            st.stations_cache = stations;
                            st.home_cache = HomeSnapshot {
                                chips,
                                selected_mood_idx: mood_idx,
                                quick_picks,
                                stations: station_snaps,
                                shelves,
                                is_loading: false,
                            };
                            st.home_cache.clone()
                        };
                        this.sink.on_home_updated(final_snap);
                    }
                    Err(err) => {
                        // Offline or transient network fallback: keep local library Quick Picks & stations usable.
                        let algo = build_algorithmic_recommended_stations_rotated(
                            &taste,
                            &library_tracks,
                            &[],
                            6,
                            rotation_epoch,
                        );
                        if stations.is_empty() {
                            stations = algo;
                        }
                        let station_snaps: Vec<StationCardSnapshot> =
                            stations.iter().map(Into::into).collect();
                        let fallback_snap = {
                            let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                            if st.home_gen != gen_id {
                                return;
                            }
                            st.stations_cache = stations;
                            if st.home_cache.quick_picks.is_empty() {
                                st.home_cache.quick_picks =
                                    library_tracks.iter().take(12).cloned().collect();
                            }
                            st.home_cache.stations = station_snaps;
                            st.home_cache.is_loading = false;
                            st.home_cache.clone()
                        };
                        this.sink.on_home_updated(fallback_snap);
                        this.sink
                            .on_toast(format!("Showing offline Home (online refresh: {err})"));
                    }
                }
            });
    }

    /// Expand a Recommended Station (Spotify Pathfinder playlist or algorithmic YTM co-listening radio)
    /// off-thread and emit `on_detail_loaded`.
    pub fn open_station(self: &Arc<Self>, station_id: &str) -> ControllerResult<()> {
        let (station, session, spotify_token, library_pool, liked_ids, gen_id) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let Some(found) = st
                .stations_cache
                .iter()
                .find(|s| s.id == station_id)
                .cloned()
            else {
                return Err(ControllerError::State(format!(
                    "station not found: {station_id}"
                )));
            };
            st.detail_gen = st.detail_gen.wrapping_add(1);
            let lib = st.db.list_tracks("").unwrap_or_default();
            let liked: HashSet<String> = lib
                .iter()
                .filter(|t| t.liked)
                .filter_map(|t| t.spotify_id.clone())
                .collect();
            (
                found,
                active_ytm_session(&st.accounts),
                active_spotify_token(&st.accounts),
                lib,
                liked,
                st.detail_gen,
            )
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-station-open".to_string())
            .spawn(move || {
                let mut tracks = station.cached_tracks.clone();
                let mut subtitle = station.subtitle.clone();
                let mut share_url = None;

                if tracks.is_empty() {
                    if let (Some(pid), Some(tok)) = (
                        station.spotify_playlist_id.as_deref(),
                        spotify_token.as_deref(),
                    ) {
                        share_url = Some(format!("https://open.spotify.com/playlist/{pid}"));
                        match fetch_spotify_playlist_by_id(tok, pid, &liked_ids) {
                            Ok(bundle) => {
                                tracks = bundle.tracks;
                            }
                            Err(SpotifyWebSyncError::AuthExpired) => {
                                this.sink.on_spotify_auth_expired();
                                this.sink.on_toast(
                                    "Spotify session expired; refreshing token...".to_string(),
                                );
                                return;
                            }
                            Err(SpotifyWebSyncError::FallbackToRest(err)) => {
                                this.sink
                                    .on_toast(format!("Failed to load Spotify station: {err}"));
                                return;
                            }
                        }
                    } else {
                        match expand_recommended_station_tracks(&session, &station, &library_pool) {
                            Ok(exp) => {
                                tracks = exp.tracks;
                                if !exp.subtitle.trim().is_empty() {
                                    subtitle = exp.subtitle;
                                }
                            }
                            Err(err) => {
                                this.sink
                                    .on_toast(format!("Failed to expand station: {err}"));
                                return;
                            }
                        }
                    }
                }

                let still_active = {
                    let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                    if let Some(cached) = st.stations_cache.iter_mut().find(|s| s.id == station.id)
                    {
                        cached.cached_tracks = tracks.clone();
                    }
                    st.detail_gen == gen_id
                };
                if still_active {
                    let source = if station.is_spotify_origin {
                        TrackSource::Spotify
                    } else {
                        TrackSource::YouTubeMusic
                    };
                    this.sink.on_detail_loaded(CollectionDetailSnapshot {
                        id: station.id,
                        title: station.title,
                        subtitle,
                        description: format!("Personalized Radio ({} tracks)", tracks.len()),
                        kind: CardKind::Playlist,
                        source,
                        thumbnail_url: station.center_art_url,
                        share_url,
                        can_sync_to_account: false,
                        is_editable_playlist: false,
                        tracks,
                    });
                }
            });
        Ok(())
    }

    /// Fetch an online YouTube Music Album, Playlist, or Artist detail off-thread.
    pub fn open_browse_detail(
        self: &Arc<Self>,
        browse_id: String,
        kind_hint: CardKind,
    ) -> ControllerResult<()> {
        let bid = browse_id.trim().to_string();
        if bid.is_empty() {
            return Err(ControllerError::State(
                "browse_id cannot be empty".to_string(),
            ));
        }
        let (session, gen_id) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.detail_gen = st.detail_gen.wrapping_add(1);
            (active_ytm_session(&st.accounts), st.detail_gen)
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-browse-detail".to_string())
            .spawn(move || match fetch_browse_detail(&session, &bid) {
                Ok(detail) => {
                    let still_active = {
                        let st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        st.detail_gen == gen_id
                    };
                    if still_active {
                        let resolved_kind: CardKind = detail.kind.into();
                        let share_url =
                            track_share_url_for_browse(&detail.browse_id, resolved_kind);
                        this.sink.on_detail_loaded(CollectionDetailSnapshot {
                            id: detail.browse_id,
                            title: detail.title,
                            subtitle: detail.subtitle,
                            description: detail.description,
                            kind: if resolved_kind == CardKind::Song {
                                kind_hint
                            } else {
                                resolved_kind
                            },
                            source: TrackSource::YouTubeMusic,
                            thumbnail_url: detail.thumbnail_url,
                            share_url,
                            can_sync_to_account: false,
                            is_editable_playlist: false,
                            tracks: detail.tracks,
                        });
                    }
                }
                Err(err) => {
                    this.sink
                        .on_toast(format!("Failed to load collection: {err}"));
                }
            });
        Ok(())
    }

    /// Run instant local search immediately, record search history, and dispatch online YouTube Music
    /// rich search off-thread.
    pub fn search_online(self: &Arc<Self>, query: String) -> ControllerResult<()> {
        let q = query.trim().to_string();
        let local_tracks = self.instant_search_local(&q)?;
        if q.is_empty() {
            self.sink.on_search_results(SearchSnapshot {
                query: String::new(),
                local_tracks,
                online_tracks: Vec::new(),
                online_cards: Vec::new(),
                is_loading: false,
            });
            return Ok(());
        }

        let (session, gen_id) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.search_gen = st.search_gen.wrapping_add(1);
            st.session.record_search(&q);
            let _ = st.db.save_session_state(&st.session);
            (active_ytm_session(&st.accounts), st.search_gen)
        };

        self.sink.on_search_results(SearchSnapshot {
            query: q.clone(),
            local_tracks: local_tracks.clone(),
            online_tracks: Vec::new(),
            online_cards: Vec::new(),
            is_loading: true,
        });

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-search".to_string())
            .spawn(move || {
                let (online_tracks, online_cards) = match fetch_rich_search(&session, &q, 24) {
                    Ok(rich) => (
                        rich.tracks,
                        rich.cards
                            .into_iter()
                            .map(ShelfCardSnapshot::from)
                            .collect(),
                    ),
                    Err(_) => match search_music(&session, &q, 20)
                        .or_else(|_| search_youtube_public(&q, 20))
                    {
                        Ok(tracks) => (tracks, Vec::new()),
                        Err(err) => {
                            this.sink.on_toast(format!("Online search failed: {err}"));
                            (Vec::new(), Vec::new())
                        }
                    },
                };

                let still_active = {
                    let st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.search_gen == gen_id
                };
                if still_active {
                    this.sink.on_search_results(SearchSnapshot {
                        query: q,
                        local_tracks,
                        online_tracks,
                        online_cards,
                        is_loading: false,
                    });
                }
            });
        Ok(())
    }

    /// Import any public Spotify or YouTube / YouTube Music playlist, album, or track URL directly.
    pub fn import_public_url(self: &Arc<Self>, raw_url: String) -> ControllerResult<()> {
        let trimmed = raw_url.trim().to_string();
        if trimmed.is_empty() {
            return Err(ControllerError::State("URL cannot be empty".to_string()));
        }
        let is_sp = parse_spotify_target(&trimmed).is_some();
        let is_yt = parse_youtube_target(&trimmed).is_some();
        if !is_sp && !is_yt {
            return Err(ControllerError::State(
                "Paste a valid Spotify or YouTube / YouTube Music URL".to_string(),
            ));
        }

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-url-import".to_string())
            .spawn(move || {
                let res = if is_sp {
                    import_spotify_public_url(&trimmed)
                } else {
                    import_youtube_public_url(&trimmed)
                };
                match res {
                    Ok((playlist, tracks)) => {
                        let count = tracks.len();
                        let name = playlist.name.clone();
                        let pl_id = playlist.id.clone();
                        let source = playlist.source;
                        let thumb = playlist.thumbnail_url.clone();
                        let desc = playlist.description.clone();
                        {
                            let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                            let _ = st.db.upsert_tracks(&tracks);
                            let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
                            let _ = st.db.upsert_playlist(&playlist, &ids);
                            let _ = st.db.match_cloud_tracks_to_local();
                        }
                        this.sink.on_library_changed();
                        this.sink.on_detail_loaded(CollectionDetailSnapshot {
                            id: pl_id,
                            title: name.clone(),
                            subtitle: format!("{count} tracks imported"),
                            description: desc,
                            kind: CardKind::Playlist,
                            source,
                            thumbnail_url: thumb,
                            share_url: Some(trimmed),
                            can_sync_to_account: false,
                            is_editable_playlist: true,
                            tracks,
                        });
                        this.sink
                            .on_toast(format!("Imported \"{name}\" ({count} tracks)"));
                    }
                    Err(err) => {
                        this.sink.on_toast(format!("URL import failed: {err}"));
                    }
                }
            });
        Ok(())
    }
}
