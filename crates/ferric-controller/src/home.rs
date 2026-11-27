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
    expand_recommended_station_tracks, fetch_browse_detail, fetch_home_continuation,
    fetch_rich_home, fetch_rich_search, is_low_quality_radio_candidate, primary_artist_key,
    search_music,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;

fn clean_home_tracks(tracks: Vec<Track>) -> Vec<Track> {
    tracks
        .into_iter()
        .filter(|t| {
            !is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms)
                && !contains_ascii_icase(&t.album, "watching")
                && !contains_ascii_icase(&t.artist, "watching")
        })
        .collect()
}

/// Build the exact Desktop/Spotify-parity weighted taste profile over the entire SQLite library:
/// sublinear play weighting + exponential recency decay + liked boost + cross-library artist
/// frequency + evenly strided 400-track library sample so Home matches Desktop quality 1:1.
fn build_home_taste_profile(
    st: &crate::ControllerState,
    lib: &[Track],
    apply_exclusions: bool,
) -> YtmHomeTasteProfile {
    let mut by_id: HashMap<&str, usize> = HashMap::with_capacity(lib.len());
    for (idx, t) in lib.iter().enumerate() {
        by_id.insert(t.id.as_str(), idx);
    }
    let mut scored_tracks: Vec<(i32, usize)> = Vec::with_capacity(96);
    let mut seen_ids: HashSet<&str> = HashSet::with_capacity(128);

    let play_weight = |plays: u32| -> i32 { ((1.0 + plays as f32).ln() * 22.0).round() as i32 };

    for (pos, id) in st.session.recent_track_ids.iter().enumerate() {
        if let Some(&idx) = by_id.get(id.as_str())
            && seen_ids.insert(lib[idx].id.as_str())
        {
            let plays = st.session.track_play_counts.get(id).copied().unwrap_or(1);
            let recency = (44.0 * (-0.09 * pos as f32).exp()).round() as i32;
            let liked_bonus = if lib[idx].liked { 18 } else { 0 };
            scored_tracks.push((play_weight(plays) + recency + liked_bonus, idx));
        }
    }
    for (id, &plays) in &st.session.track_play_counts {
        if let Some(&idx) = by_id.get(id.as_str())
            && seen_ids.insert(lib[idx].id.as_str())
        {
            let liked_bonus = if lib[idx].liked { 18 } else { 0 };
            scored_tracks.push((play_weight(plays) + liked_bonus, idx));
        }
    }
    for (idx, t) in lib.iter().enumerate() {
        if t.liked && seen_ids.insert(t.id.as_str()) {
            scored_tracks.push((24, idx));
        }
    }

    let mut lib_artist_freq: HashMap<String, (String, i32, usize)> = HashMap::new();
    for (idx, t) in lib.iter().enumerate() {
        let key = primary_artist_key(&t.artist);
        if key.is_empty() || key.eq_ignore_ascii_case("unknown artist") {
            continue;
        }
        let norm = key.to_ascii_lowercase();
        lib_artist_freq
            .entry(norm)
            .and_modify(|e| {
                e.1 += if t.liked { 5 } else { 2 };
                if lib[e.2].youtube_video_id.is_none() && t.youtube_video_id.is_some() {
                    e.2 = idx;
                }
            })
            .or_insert_with(|| (key, if t.liked { 10 } else { 5 }, idx));
    }
    let mut lib_artists_sorted: Vec<(String, i32, usize)> = lib_artist_freq.into_values().collect();
    lib_artists_sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (_, freq_score, rep_idx) in lib_artists_sorted.iter().take(64) {
        if seen_ids.insert(lib[*rep_idx].id.as_str()) {
            scored_tracks.push(((*freq_score).min(20), *rep_idx));
        }
    }

    scored_tracks.sort_by_key(|a| std::cmp::Reverse(a.0));
    let top_tracks: Vec<Track> = scored_tracks
        .into_iter()
        .take(60)
        .map(|(_, idx)| lib[idx].clone())
        .collect();

    // Stratified evenly strided sample up to 400 tracks across the whole library
    let total_lib = lib.len();
    let mut library_tracks: Vec<Track> = Vec::with_capacity(total_lib.min(400));
    let mut lib_seen: HashSet<&str> = HashSet::with_capacity(total_lib.min(400));
    if total_lib > 0 {
        let stride = (total_lib / 240).max(1);
        for t in lib.iter().step_by(stride) {
            if (t.youtube_video_id.is_some() || t.liked) && lib_seen.insert(t.id.as_str()) {
                library_tracks.push(t.clone());
                if library_tracks.len() >= 240 {
                    break;
                }
            }
        }
        if library_tracks.len() < 400 {
            let stride2 = (total_lib / 320).max(1);
            for t in lib.iter().step_by(stride2) {
                if lib_seen.insert(t.id.as_str()) {
                    library_tracks.push(t.clone());
                    if library_tracks.len() >= 400 {
                        break;
                    }
                }
            }
        }
    }

    let mut artist_scores: HashMap<String, (String, i32)> = HashMap::new();
    for (artist, &cnt) in &st.session.artist_play_counts {
        let key = primary_artist_key(artist);
        if !key.is_empty() {
            let norm = key.to_ascii_lowercase();
            let entry = artist_scores.entry(norm).or_insert((key, 0));
            entry.1 += play_weight(cnt);
        }
    }
    for (idx, t) in top_tracks.iter().enumerate() {
        let key = primary_artist_key(&t.artist);
        if !key.is_empty() {
            let norm = key.to_ascii_lowercase();
            let w = 28_i32.saturating_sub((idx as i32) / 2).max(4) + if t.liked { 10 } else { 0 };
            let entry = artist_scores.entry(norm).or_insert((key, 0));
            entry.1 += w;
        }
    }
    for (disp, freq_score, _) in lib_artists_sorted.into_iter().take(56) {
        let norm = disp.to_ascii_lowercase();
        let entry = artist_scores.entry(norm).or_insert((disp, 0));
        entry.1 += freq_score.min(24);
    }
    let mut artist_list: Vec<(String, i32)> = artist_scores.into_values().collect();
    artist_list.sort_by_key(|a| std::cmp::Reverse(a.1));
    let top_artists: Vec<String> = artist_list.into_iter().take(48).map(|(a, _)| a).collect();

    let refresh_mood_params = if !st.home_cache.chips.is_empty() {
        let idx = st.home_more_cycle % st.home_cache.chips.len();
        Some(st.home_cache.chips[idx].params.clone())
    } else {
        None
    };

    YtmHomeTasteProfile {
        top_tracks,
        library_tracks,
        top_artists,
        recent_searches: st.session.recent_searches.clone(),
        excluded_track_ids: if apply_exclusions {
            st.home_excluded_tracks.clone()
        } else {
            HashSet::new()
        },
        excluded_card_ids: if apply_exclusions {
            st.home_excluded_cards.clone()
        } else {
            HashSet::new()
        },
        excluded_artists: if apply_exclusions {
            st.home_excluded_artists.clone()
        } else {
            HashSet::new()
        },
        continuation_token: st.home_continuation.clone(),
        refresh_mood_params,
    }
}

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
        let mut hits: Vec<Track> = Vec::with_capacity(80);
        for t in all {
            if tokens.iter().all(|tok| {
                contains_ascii_icase(&t.title, tok)
                    || contains_ascii_icase(&t.artist, tok)
                    || contains_ascii_icase(&t.album, tok)
            }) {
                hits.push(t);
                if hits.len() >= 80 {
                    break;
                }
            }
        }
        Ok(hits)
    }

    /// Load or weighted-refresh the Home screen asynchronously (Mood Chips, Quick Picks,
    /// Dual-Engine Recommended Stations, and personalized shelves matching Desktop).
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
                st.home_more_cycle = st.home_more_cycle.wrapping_add(1);
            }
            st.is_loading_more_home = false;
            let lib: Vec<Track> = st
                .db
                .list_tracks("")
                .unwrap_or_default()
                .into_iter()
                .filter(|t| {
                    !t.id.starts_with("demo:") && !contains_ascii_icase(&t.album, "watching")
                })
                .collect();

            let chips_snapshot = st.home_cache.chips.clone();
            let mood_params = mood_idx
                .and_then(|idx| chips_snapshot.get(idx))
                .map(|c| c.params.clone())
                .filter(|p| !p.is_empty());

            let taste = build_home_taste_profile(&st, &lib, force_refresh);

            st.home_cache.is_loading = true;
            st.home_cache.is_loading_more = false;
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

                let mut stations: Vec<RecommendedStation> = Vec::with_capacity(12);
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
                        let cleaned_picks = clean_home_tracks(rich.quick_picks);
                        let quick_picks = if cleaned_picks.is_empty() {
                            clean_home_tracks(taste.top_tracks.clone())
                                .into_iter()
                                .take(24)
                                .collect()
                        } else {
                            cleaned_picks
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
                                let _ = st.db.save_accounts(&st.accounts);
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
                                    st.home_excluded_artists
                                        .insert(s.seed_artist.trim().to_ascii_lowercase());
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
                                is_loading_more: false,
                            };
                            st.home_cache.clone()
                        };
                        this.sink.on_home_updated(final_snap);
                    }
                    Err(err) => {
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
                                st.home_cache.quick_picks = clean_home_tracks(taste.top_tracks)
                                    .into_iter()
                                    .take(24)
                                    .collect();
                            }
                            st.home_cache.stations = station_snaps;
                            st.home_cache.is_loading = false;
                            st.home_cache.is_loading_more = false;
                            st.home_cache.clone()
                        };
                        this.sink.on_home_updated(fallback_snap);
                        this.sink
                            .on_toast(format!("Showing offline Home (online refresh: {err})"));
                    }
                }
            });
    }

    /// Endless scroll continuation for Home: appends fresh YTM continuation shelves, personalized
    /// deep-catalog artist mixes, and additional Quick Picks mirroring Desktop `Load More`.
    pub fn load_home_more(self: &Arc<Self>) -> bool {
        let (gen_id, session, taste, fallback_mood, lib_pool) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if st.is_loading_more_home
                || st.home_cache.is_loading
                || st.home_cache.shelves.len() >= 48
            {
                return false;
            }
            let lib: Vec<Track> = st
                .db
                .list_tracks("")
                .unwrap_or_default()
                .into_iter()
                .filter(|t| {
                    !t.id.starts_with("demo:") && !contains_ascii_icase(&t.album, "watching")
                })
                .collect();
            let taste = build_home_taste_profile(&st, &lib, true);
            let mood = if !st.home_cache.chips.is_empty() {
                let idx = st.home_more_cycle % st.home_cache.chips.len();
                st.home_more_cycle = st.home_more_cycle.wrapping_add(1);
                Some(st.home_cache.chips[idx].params.clone())
            } else {
                None
            };
            st.is_loading_more_home = true;
            st.home_cache.is_loading_more = true;
            let snap = st.home_cache.clone();
            let sess = active_ytm_session(&st.accounts);
            let gen_val = st.home_gen;
            drop(st);
            self.sink.on_home_updated(snap);
            (gen_val, sess, taste, mood, lib)
        };

        let this = Arc::clone(self);
        let spawn_res = thread::Builder::new()
            .name("ferric-home-more".to_string())
            .spawn(move || {
                let cont_tok = taste.continuation_token.clone();
                let res = fetch_home_continuation(
                    &session,
                    cont_tok.as_deref(),
                    fallback_mood.as_deref(),
                    &taste,
                );

                let final_snap = {
                    let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.is_loading_more_home = false;
                    st.home_cache.is_loading_more = false;
                    if st.home_gen != gen_id {
                        return;
                    }

                    match res {
                        Ok(more) => {
                            st.home_continuation = more.continuation_token;
                            if !more.visitor_data.is_empty() {
                                st.accounts.youtube_visitor_data = more.visitor_data;
                            }

                            let mut existing_qp: HashSet<String> = st
                                .home_cache
                                .quick_picks
                                .iter()
                                .map(|t| t.id.clone())
                                .collect();
                            for t in clean_home_tracks(more.quick_picks) {
                                if existing_qp.insert(t.id.clone())
                                    && st.home_cache.quick_picks.len() < 48
                                {
                                    st.home_excluded_tracks.insert(t.id.clone());
                                    if let Some(vid) = &t.youtube_video_id {
                                        st.home_excluded_tracks.insert(vid.clone());
                                    }
                                    st.home_cache.quick_picks.push(t);
                                }
                            }

                            let mut existing_shelves: HashSet<String> = st
                                .home_cache
                                .shelves
                                .iter()
                                .map(|s| s.title.to_ascii_lowercase())
                                .collect();
                            for sh in more.shelves {
                                if !sh.cards.is_empty()
                                    && existing_shelves.insert(sh.title.to_ascii_lowercase())
                                {
                                    for c in &sh.cards {
                                        st.home_excluded_cards.insert(c.id.clone());
                                    }
                                    st.home_cache.shelves.push(sh.into());
                                    if st.home_cache.shelves.len() >= 48 {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            // Offline / transient fallback: synthesize a fresh artist spotlight shelf from library
                            let shown_titles: HashSet<String> = st
                                .home_cache
                                .shelves
                                .iter()
                                .map(|s| s.title.to_ascii_lowercase())
                                .collect();
                            if let Some(artist) = taste.top_artists.iter().find(|a| {
                                !shown_titles
                                    .contains(&format!("more from {}", a.to_ascii_lowercase()))
                            }) {
                                let cards: Vec<ShelfCardSnapshot> = lib_pool
                                    .into_iter()
                                    .filter(|t| contains_ascii_icase(&t.artist, artist))
                                    .take(12)
                                    .map(|t| ShelfCardSnapshot {
                                        id: t.id.clone(),
                                        title: t.title,
                                        subtitle: t.artist,
                                        kind: CardKind::Song,
                                        browse_id: None,
                                        video_id: t.youtube_video_id,
                                        thumbnail_url: t.thumbnail_url,
                                    })
                                    .collect();
                                if cards.len() >= 2 {
                                    st.home_cache.shelves.push(HomeShelfSnapshot {
                                        title: format!("More from {artist}"),
                                        cards,
                                    });
                                }
                            }
                        }
                    }

                    st.home_cache.clone()
                };
                this.sink.on_home_updated(final_snap);
            });

        if spawn_res.is_err() {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.is_loading_more_home = false;
            st.home_cache.is_loading_more = false;
            return false;
        }
        true
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
