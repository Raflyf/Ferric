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
    fetch_rich_genre_browse, fetch_rich_home, fetch_rich_search, is_low_quality_radio_candidate,
    primary_artist_key, search_music,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::thread;

fn push_bounded_exclusion(
    set: &mut HashSet<String>,
    fifo: &mut VecDeque<String>,
    key: String,
    cap: usize,
) {
    if key.is_empty() {
        return;
    }
    if set.insert(key.clone()) {
        fifo.push_back(key);
        while fifo.len() > cap {
            if let Some(old) = fifo.pop_front() {
                set.remove(&old);
            }
        }
    }
}

fn merge_stations_rotated(
    sp_cards: &[ferric_providers::spotify_web::SpotifyStationCard],
    algo_stations: Vec<RecommendedStation>,
    excluded_artists: &HashSet<String>,
    rotation_epoch: usize,
    force_refresh: bool,
) -> Vec<RecommendedStation> {
    if sp_cards.is_empty() {
        return algo_stations.into_iter().take(10).collect();
    }
    let mut sp_converted: Vec<RecommendedStation> = sp_cards
        .iter()
        .enumerate()
        .map(|(idx, card)| RecommendedStation::from_spotify_card(card, (idx % 6) as u8))
        .collect();
    if rotation_epoch > 0 && sp_converted.len() > 1 {
        let shift = rotation_epoch % sp_converted.len();
        sp_converted.rotate_left(shift);
        // Partition unseen Spotify station artists ahead of recently shown ones on Refresh
        sp_converted
            .sort_by_key(|s| excluded_artists.contains(&s.seed_artist.trim().to_ascii_lowercase()));
    }
    let sp_cap = if force_refresh { 4 } else { 5 };
    let mut sp_iter = sp_converted.into_iter().take(sp_cap);
    let mut algo_iter = algo_stations.into_iter();
    let mut out = Vec::with_capacity(10);
    let mut seen_artists: HashSet<String> = HashSet::with_capacity(12);

    while out.len() < 10 {
        let pick_algo_first = force_refresh;
        let first = if pick_algo_first {
            algo_iter.next().or_else(|| sp_iter.next())
        } else {
            sp_iter.next().or_else(|| algo_iter.next())
        };
        if let Some(s) = first {
            let norm = s.seed_artist.trim().to_ascii_lowercase();
            if norm.is_empty() || seen_artists.insert(norm) {
                out.push(s);
            }
        } else {
            break;
        }
        if out.len() >= 10 {
            break;
        }
        let second = if pick_algo_first {
            sp_iter.next().or_else(|| algo_iter.next())
        } else {
            algo_iter.next().or_else(|| sp_iter.next())
        };
        if let Some(s) = second {
            let norm = s.seed_artist.trim().to_ascii_lowercase();
            if norm.is_empty() || seen_artists.insert(norm) {
                out.push(s);
            }
        }
    }
    out
}

fn clean_home_tracks(tracks: Vec<Track>) -> Vec<Track> {
    tracks
        .into_iter()
        .filter(|t| !is_low_quality_radio_candidate(&t.title, &t.artist, "", t.duration_ms))
        .collect()
}

struct HomeTasteSessionRef<'a> {
    recent_track_ids: &'a [String],
    track_play_counts: &'a HashMap<String, u32>,
    artist_play_counts: &'a HashMap<String, u32>,
    recent_searches: &'a [String],
}

struct HomeTasteExclusions {
    track_ids: HashSet<String>,
    card_ids: HashSet<String>,
    artists: HashSet<String>,
    continuation_token: Option<String>,
}

/// Build the exact Desktop/Spotify-parity weighted taste profile from borrowed session/library
/// slices so callers can assemble inputs under a short lock and score off-lock when desired.
fn build_home_taste_profile_from_parts(
    session: HomeTasteSessionRef<'_>,
    lib: &[Track],
    exclusions: Option<HomeTasteExclusions>,
) -> YtmHomeTasteProfile {
    let mut by_id: HashMap<&str, usize> = HashMap::with_capacity(lib.len());
    for (idx, t) in lib.iter().enumerate() {
        by_id.insert(t.id.as_str(), idx);
    }
    let mut scored_tracks: Vec<(i32, usize)> = Vec::with_capacity(96);
    let mut seen_ids: HashSet<&str> = HashSet::with_capacity(128);

    let play_weight = |plays: u32| -> i32 { ((1.0 + plays as f32).ln() * 22.0).round() as i32 };

    for (pos, id) in session.recent_track_ids.iter().enumerate() {
        if let Some(&idx) = by_id.get(id.as_str())
            && seen_ids.insert(lib[idx].id.as_str())
        {
            let plays = session.track_play_counts.get(id).copied().unwrap_or(1);
            let recency = (44.0 * (-0.09 * pos as f32).exp()).round() as i32;
            let liked_bonus = if lib[idx].liked { 18 } else { 0 };
            scored_tracks.push((play_weight(plays) + recency + liked_bonus, idx));
        }
    }
    for (id, &plays) in session.track_play_counts {
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
                e.1 += if t.liked { 4 } else { 1 };
                if lib[e.2].youtube_video_id.is_none() && t.youtube_video_id.is_some() {
                    e.2 = idx;
                }
            })
            .or_insert_with(|| (key, if t.liked { 8 } else { 2 }, idx));
    }
    let mut lib_artists_sorted: Vec<(String, i32, usize)> = lib_artist_freq.into_values().collect();
    lib_artists_sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (_, freq_score, rep_idx) in lib_artists_sorted.iter().take(48) {
        if seen_ids.insert(lib[*rep_idx].id.as_str()) {
            let capped = if lib[*rep_idx].liked {
                (*freq_score).min(18)
            } else {
                (*freq_score).min(10)
            };
            scored_tracks.push((capped, *rep_idx));
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
    for (artist, &cnt) in session.artist_play_counts {
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
    let excl = exclusions.unwrap_or(HomeTasteExclusions {
        track_ids: HashSet::new(),
        card_ids: HashSet::new(),
        artists: HashSet::new(),
        continuation_token: None,
    });

    YtmHomeTasteProfile {
        top_tracks,
        library_tracks,
        top_artists,
        recent_searches: session.recent_searches.to_vec(),
        excluded_track_ids: excl.track_ids,
        excluded_card_ids: excl.card_ids,
        excluded_artists: excl.artists,
        continuation_token: excl.continuation_token,
        refresh_mood_params: None,
    }
}

/// Build the exact Desktop/Spotify-parity weighted taste profile from `ControllerState`.
fn build_home_taste_profile(
    st: &crate::ControllerState,
    lib: &[Track],
    apply_exclusions: bool,
) -> YtmHomeTasteProfile {
    build_home_taste_profile_from_parts(
        HomeTasteSessionRef {
            recent_track_ids: &st.session.recent_track_ids,
            track_play_counts: &st.session.track_play_counts,
            artist_play_counts: &st.session.artist_play_counts,
            recent_searches: &st.session.recent_searches,
        },
        lib,
        apply_exclusions.then(|| HomeTasteExclusions {
            track_ids: st.home_excluded_tracks.clone(),
            card_ids: st.home_excluded_cards.clone(),
            artists: st.home_excluded_artists.clone(),
            continuation_token: st.home_continuation.clone(),
        }),
    )
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
                .filter(|t| !t.id.starts_with("demo:"))
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

                let sp_cards = if let Some(tok) = spotify_token.as_deref() {
                    match fetch_spotify_recommended_stations(tok) {
                        Ok(cards) => cards,
                        Err(SpotifyWebSyncError::AuthExpired) => {
                            this.sink.on_spotify_auth_expired();
                            Vec::new()
                        }
                        Err(_) => Vec::new(),
                    }
                } else {
                    Vec::new()
                };

                match ytm_res {
                    Ok(rich) => {
                        let algo = build_algorithmic_recommended_stations_rotated(
                            &taste,
                            &library_tracks,
                            &rich.all_tracks,
                            8,
                            rotation_epoch,
                        );
                        let stations = merge_stations_rotated(
                            &sp_cards,
                            algo,
                            &taste.excluded_artists,
                            rotation_epoch,
                            force_refresh,
                        );

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

                            // Maintain rolling FIFO windows instead of clearing every load so
                            // consecutive Refreshes avoid oscillating back into an A -> B -> A loop.
                            let crate::ControllerState {
                                home_excluded_tracks,
                                home_excluded_track_fifo,
                                home_excluded_cards,
                                home_excluded_card_fifo,
                                home_excluded_artists,
                                home_excluded_artist_fifo,
                                ..
                            } = &mut *st;
                            for t in quick_picks.iter().take(24) {
                                push_bounded_exclusion(
                                    home_excluded_tracks,
                                    home_excluded_track_fifo,
                                    t.id.clone(),
                                    96,
                                );
                                if let Some(vid) = &t.youtube_video_id {
                                    push_bounded_exclusion(
                                        home_excluded_tracks,
                                        home_excluded_track_fifo,
                                        vid.clone(),
                                        96,
                                    );
                                }
                            }
                            for sh in &shelves {
                                for c in sh.cards.iter().take(12) {
                                    push_bounded_exclusion(
                                        home_excluded_cards,
                                        home_excluded_card_fifo,
                                        c.id.clone(),
                                        64,
                                    );
                                }
                            }
                            for s in &stations {
                                if !s.seed_artist.is_empty() {
                                    push_bounded_exclusion(
                                        home_excluded_artists,
                                        home_excluded_artist_fifo,
                                        s.seed_artist.trim().to_ascii_lowercase(),
                                        12,
                                    );
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
                            8,
                            rotation_epoch,
                        );
                        let stations = merge_stations_rotated(
                            &sp_cards,
                            algo,
                            &taste.excluded_artists,
                            rotation_epoch,
                            force_refresh,
                        );
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
                .filter(|t| !t.id.starts_with("demo:"))
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
                                    let tid = t.id.clone();
                                    let vid = t.youtube_video_id.clone();
                                    {
                                        let s = &mut *st;
                                        push_bounded_exclusion(
                                            &mut s.home_excluded_tracks,
                                            &mut s.home_excluded_track_fifo,
                                            tid,
                                            240,
                                        );
                                        if let Some(v) = vid {
                                            push_bounded_exclusion(
                                                &mut s.home_excluded_tracks,
                                                &mut s.home_excluded_track_fifo,
                                                v,
                                                240,
                                            );
                                        }
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
                                    let card_ids: Vec<String> =
                                        sh.cards.iter().map(|c| c.id.clone()).collect();
                                    {
                                        let s = &mut *st;
                                        for cid in card_ids {
                                            push_bounded_exclusion(
                                                &mut s.home_excluded_cards,
                                                &mut s.home_excluded_card_fifo,
                                                cid,
                                                120,
                                            );
                                        }
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
            let found = st
                .stations_cache
                .iter()
                .find(|s| s.id == station_id)
                .cloned()
                .or_else(|| {
                    if let Some(pid) = station_id.strip_prefix("sp:station:") {
                        let clean_pid = pid.trim();
                        if !clean_pid.is_empty() {
                            return Some(RecommendedStation {
                                id: station_id.to_string(),
                                title: "Recommended Radio".to_string(),
                                display_name: "Radio".to_string(),
                                subtitle: "Non-stop personalized radio".to_string(),
                                palette_idx: 0,
                                is_spotify_prebuilt_art: false,
                                center_art_url: None,
                                left_art_url: None,
                                right_art_url: None,
                                spotify_playlist_id: Some(clean_pid.to_string()),
                                seed_video_id: None,
                                seed_artist: String::new(),
                                seed_title: String::new(),
                                is_spotify_origin: true,
                                cached_tracks: Vec::new(),
                            });
                        }
                    }
                    if let Some(rest) = station_id.strip_prefix("ferric:station:") {
                        let slug = rest.split_once(':').map_or(rest, |(_, s)| s);
                        let mut seed_name = String::with_capacity(slug.len());
                        for word in slug.split('-').filter(|w| !w.is_empty()) {
                            if !seed_name.is_empty() {
                                seed_name.push(' ');
                            }
                            seed_name.push_str(word);
                        }
                        if !seed_name.is_empty() {
                            return Some(RecommendedStation {
                                id: station_id.to_string(),
                                title: format!("{seed_name} Radio"),
                                display_name: seed_name.clone(),
                                subtitle: format!("Non-stop radio inspired by {seed_name}"),
                                palette_idx: 0,
                                is_spotify_prebuilt_art: false,
                                center_art_url: None,
                                left_art_url: None,
                                right_art_url: None,
                                spotify_playlist_id: None,
                                seed_video_id: None,
                                seed_artist: seed_name.clone(),
                                seed_title: seed_name,
                                is_spotify_origin: false,
                                cached_tracks: Vec::new(),
                            });
                        }
                    }
                    None
                });
            let Some(found) = found else {
                return Err(ControllerError::State(format!(
                    "station not found: {station_id}"
                )));
            };
            st.detail_gen = st.detail_gen.wrapping_add(1);
            let sp_tok = active_spotify_token(&st.accounts);
            let needs_fetch = found.cached_tracks.is_empty();
            let is_spotify_fetch =
                needs_fetch && found.spotify_playlist_id.is_some() && sp_tok.is_some();
            let liked: HashSet<String> = if is_spotify_fetch {
                st.db
                    .list_liked_tracks()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|t| t.spotify_id)
                    .collect()
            } else {
                HashSet::new()
            };
            let lib = if needs_fetch && !is_spotify_fetch {
                crate::sample_library_pool_bounded(&st.db, &st.session.recent_track_ids, 240)
            } else {
                Vec::new()
            };
            (
                found,
                active_ytm_session(&st.accounts),
                sp_tok,
                lib,
                liked,
                st.detail_gen,
            )
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-station-open".to_string())
            .spawn(move || {
                let mut station = station;
                let mut tracks = std::mem::take(&mut station.cached_tracks);
                let mut subtitle = std::mem::take(&mut station.subtitle);
                let was_cached = !tracks.is_empty();
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
                    if !was_cached
                        && let Some(cached) =
                            st.stations_cache.iter_mut().find(|s| s.id == station.id)
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

    /// Browse a curated genre mix ("Pop", "Indie", "Rock", etc.) with epoch-based rotation,
    /// parallel multi-query + radio expansion (~32-44 songs + cards), and per-genre rolling exclusions.
    pub fn browse_genre(self: &Arc<Self>, genre: String) -> ControllerResult<()> {
        let g = genre.trim().to_string();
        if g.is_empty() {
            return self.search_online(String::new());
        }
        let local_tracks = self.instant_search_local(&g)?;
        let g_key = g.to_ascii_lowercase();

        let (
            session,
            gen_id,
            epoch,
            lib,
            recent_track_ids,
            track_play_counts,
            artist_play_counts,
            recent_searches,
            excluded_ids,
        ) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.search_gen = st.search_gen.wrapping_add(1);
            let gen_id = st.search_gen;
            let epoch_entry = st.genre_browse_epochs.entry(g_key.clone()).or_insert(0);
            let epoch = *epoch_entry;
            *epoch_entry = epoch_entry.wrapping_add(1);
            let excluded_ids: HashSet<String> = st
                .genre_excluded_tracks
                .get(&g_key)
                .map(|dq| dq.iter().cloned().collect())
                .unwrap_or_default();
            let lib = crate::sample_library_pool_bounded(&st.db, &st.session.recent_track_ids, 320);
            (
                active_ytm_session(&st.accounts),
                gen_id,
                epoch,
                lib,
                st.session.recent_track_ids.clone(),
                st.session.track_play_counts.clone(),
                st.session.artist_play_counts.clone(),
                st.session.recent_searches.clone(),
                excluded_ids,
            )
        };

        self.sink.on_search_results(SearchSnapshot {
            query: g.clone(),
            local_tracks: local_tracks.clone(),
            online_tracks: Vec::new(),
            online_cards: Vec::new(),
            is_loading: true,
        });

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-genre-browse".to_string())
            .spawn(move || {
                let taste = build_home_taste_profile_from_parts(
                    HomeTasteSessionRef {
                        recent_track_ids: &recent_track_ids,
                        track_play_counts: &track_play_counts,
                        artist_play_counts: &artist_play_counts,
                        recent_searches: &recent_searches,
                    },
                    &lib,
                    None,
                );
                let (online_tracks, online_cards) =
                    match fetch_rich_genre_browse(&session, &g, epoch, &taste, &excluded_ids) {
                        Ok(rich) => (
                            rich.tracks,
                            rich.cards
                                .into_iter()
                                .map(ShelfCardSnapshot::from)
                                .collect(),
                        ),
                        Err(_) => match fetch_rich_search(&session, &g, 28) {
                            Ok(rich) => (
                                rich.tracks,
                                rich.cards
                                    .into_iter()
                                    .map(ShelfCardSnapshot::from)
                                    .collect(),
                            ),
                            Err(err) => {
                                this.sink.on_toast(format!("Genre browse failed: {err}"));
                                (Vec::new(), Vec::new())
                            }
                        },
                    };

                let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                if st.search_gen != gen_id {
                    return;
                }
                let dq = st.genre_excluded_tracks.entry(g_key).or_default();
                for t in online_tracks.iter().take(24) {
                    let key_ref = t.youtube_video_id.as_deref().unwrap_or(&t.id);
                    if !dq.iter().any(|existing| existing == key_ref) {
                        dq.push_back(key_ref.to_owned());
                        while dq.len() > 72 {
                            dq.pop_front();
                        }
                    }
                }
                drop(st);

                this.sink.on_search_results(SearchSnapshot {
                    query: g,
                    local_tracks,
                    online_tracks,
                    online_cards,
                    is_loading: false,
                });
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

#[cfg(test)]
mod tests {
    use super::{merge_stations_rotated, push_bounded_exclusion};
    use ferric_providers::spotify_web::SpotifyStationCard;
    use ferric_providers::ytmusic::RecommendedStation;
    use std::collections::{HashSet, VecDeque};

    fn fake_algo(artist: &str) -> RecommendedStation {
        RecommendedStation {
            id: format!("algo:{artist}"),
            title: format!("{artist} Radio"),
            display_name: artist.to_string(),
            subtitle: format!("Inspired by {artist}"),
            palette_idx: 0,
            is_spotify_prebuilt_art: false,
            center_art_url: None,
            left_art_url: None,
            right_art_url: None,
            spotify_playlist_id: None,
            seed_video_id: Some("dQw4w9WgXcQ".to_string()),
            seed_artist: artist.to_string(),
            seed_title: "Song".to_string(),
            is_spotify_origin: false,
            cached_tracks: Vec::new(),
        }
    }

    fn fake_sp(id: &str, artist: &str) -> SpotifyStationCard {
        SpotifyStationCard {
            playlist_id: id.to_string(),
            title: format!("{artist} Radio"),
            display_name: artist.to_string(),
            subtitle: String::new(),
            thumbnail_url: None,
        }
    }

    #[test]
    fn rolling_fifo_exclusions_evict_oldest_without_full_clear() {
        let mut set = HashSet::new();
        let mut fifo = VecDeque::new();
        for i in 0..5 {
            push_bounded_exclusion(&mut set, &mut fifo, format!("id_{i}"), 3);
        }
        assert_eq!(fifo.len(), 3);
        assert!(!set.contains("id_0"));
        assert!(!set.contains("id_1"));
        assert!(set.contains("id_2"));
        assert!(set.contains("id_3"));
        assert!(set.contains("id_4"));
    }

    #[test]
    fn merge_stations_rotated_changes_order_and_respects_excluded_artists() {
        let sp = vec![
            fake_sp("sp1", "Artist A"),
            fake_sp("sp2", "Artist B"),
            fake_sp("sp3", "Artist C"),
            fake_sp("sp4", "Artist D"),
        ];
        let algo = vec![
            fake_algo("Algo 1"),
            fake_algo("Algo 2"),
            fake_algo("Algo 3"),
            fake_algo("Algo 4"),
        ];
        let init = merge_stations_rotated(&sp, algo.clone(), &HashSet::new(), 0, false);
        let excluded: HashSet<String> = ["artist a", "artist b"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let refreshed = merge_stations_rotated(&sp, algo, &excluded, 1, true);
        assert_ne!(init[0].seed_artist, refreshed[0].seed_artist);
        assert_eq!(refreshed[0].seed_artist, "Algo 1");
    }
}
