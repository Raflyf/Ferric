//! Headless playback, queue, library, settings, and persistence orchestrator shared by Ferric mobile frontends.

pub mod accounts;
pub mod connect;
pub mod events;
pub mod home;
pub mod library;
pub mod media_tools;
pub mod offline;
pub mod types;

use crate::accounts::active_ytm_session;
use crate::events::ControllerEventSink;
use crate::types::{HomeSnapshot, LyricsSnapshot};
use ferric_audio::engine::{AudioEngine, EngineCommand, TelemetrySnapshot};
use ferric_core::db::LibraryDb;
use ferric_core::eq::EqConfig;
use ferric_core::models::{
    AccountSettings, ListeningStatsSummary, PlaybackDspSettings, Playlist, RepeatMode,
    SessionResumeState, Track, TrackSource, qualifies_as_play,
};
use ferric_core::queue::PlayQueue;
use ferric_core::theme::ThemeConfig;
use ferric_providers::subsonic::{SubsonicCredentials, download_and_cache_subsonic_stream};
use ferric_providers::ytmusic::{
    MusicLocale, RecommendedStation, detect_track_locale, fetch_radio_up_next_for_track,
    is_low_quality_radio_candidate, is_same_song_variant, normalize_core_title, primary_artist_key,
    resolve_and_cache_audio_stream_with_quality, resolve_best_video_id_for_track,
};

const UP_NEXT_EMIT_LIMIT: usize = 250;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("core error: {0}")]
    Core(#[from] ferric_core::CoreError),
    #[error("audio error: {0}")]
    Audio(#[from] ferric_audio::AudioError),
    #[error("provider error: {0}")]
    Provider(#[from] ferric_providers::ProviderError),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("controller state error: {0}")]
    State(String),
}

pub type ControllerResult<T> = Result<T, ControllerError>;

#[derive(Debug, Clone)]
pub struct ControllerConfig {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub device_id: String,
    pub device_name: String,
    pub device_kind: ferric_sync::DeviceKind,
    pub enable_connect: bool,
    pub headless_audio: bool,
}

#[derive(Debug, Clone)]
struct ActivePlayTracker {
    track_id: String,
    artist: String,
    duration_ms: u64,
    listened_ms: u64,
    logged: bool,
}

struct ControllerState {
    db: LibraryDb,
    queue: PlayQueue,
    queue_context: String,
    theme: ThemeConfig,
    eq: EqConfig,
    dsp: PlaybackDspSettings,
    session: SessionResumeState,
    accounts: AccountSettings,
    last_finished_seq: u64,
    play_tracker: Option<ActivePlayTracker>,
    resolve_gen: u64,
    home_cache: HomeSnapshot,
    stations_cache: Vec<RecommendedStation>,
    home_continuation: Option<String>,
    home_excluded_tracks: HashSet<String>,
    home_excluded_cards: HashSet<String>,
    home_excluded_artists: HashSet<String>,
    station_rotation_epoch: usize,
    home_more_cycle: usize,
    is_loading_more_home: bool,
    home_gen: u64,
    search_gen: u64,
    detail_gen: u64,
    prefetch_gen: u64,
    extend_queue_gen: u64,
    is_extending_queue: bool,
    lyrics_gen: u64,
    video_gen: u64,
    lyrics_offset_ms: i64,
    lyrics_cache: LyricsSnapshot,
    pending_handoff_seek_ms: Option<u64>,
    paused_while_resolving: bool,
    last_emitted_idle: bool,
    sleep_timer_end: Option<Instant>,
    last_tick_at: Option<Instant>,
    connect_sync_ticks: u8,
}

struct MonitorSync {
    wake_lock: Mutex<bool>,
    wake_cv: Condvar,
    shutdown_flag: AtomicBool,
}

/// Thread-safe headless Ferric controller owning SQLite persistence, PlayQueue, lazy `AudioEngine`,
/// Deck B pre-queuing, online stream cache resolution, Zero-Pairing Connect, and qualified `play_history` logging.
pub struct FerricController {
    cfg: ControllerConfig,
    sink: Arc<dyn ControllerEventSink>,
    state: Mutex<ControllerState>,
    audio: Mutex<Option<AudioEngine>>,
    connect: Mutex<Option<Arc<ferric_sync::ConnectEngine>>>,
    monitor_sync: Arc<MonitorSync>,
    monitor_handle: Mutex<Option<thread::JoinHandle<()>>>,
}

fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_playable_local_path(path: &std::path::Path) -> bool {
    if let Some(raw) = path.to_str()
        && raw.starts_with("content://")
    {
        return true;
    }
    path.is_file()
}

fn same_album_consecutive(current: &Track, next: &Track) -> bool {
    let a = current.album.trim();
    let b = next.album.trim();
    !a.is_empty() && a.eq_ignore_ascii_case(b) && current.artist.eq_ignore_ascii_case(&next.artist)
}

impl FerricController {
    /// Construct a new controller without starting `AudioEngine` (lazy audio start for Android 12+).
    pub fn new(
        cfg: ControllerConfig,
        sink: Arc<dyn ControllerEventSink>,
    ) -> ControllerResult<Arc<Self>> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        std::fs::create_dir_all(&cfg.cache_dir)?;
        ferric_core::set_app_data_dir_override(cfg.data_dir.clone());

        let db = LibraryDb::open_in_dir(&cfg.data_dir)?;
        let theme = db.load_theme()?;
        let eq = db.load_eq()?;
        let dsp = db.load_playback_dsp()?;
        let session = db.load_session_state()?;
        let accounts = db.load_accounts()?;

        let mut queue = PlayQueue::new();
        queue.shuffle = session.shuffle;
        queue.repeat = session.repeat;

        let ctrl = Arc::new(Self {
            cfg,
            sink,
            state: Mutex::new(ControllerState {
                db,
                queue,
                queue_context: String::new(),
                theme,
                eq,
                dsp,
                session,
                accounts,
                last_finished_seq: 0,
                play_tracker: None,
                resolve_gen: 0,
                home_cache: HomeSnapshot::default(),
                stations_cache: Vec::new(),
                home_continuation: None,
                home_excluded_tracks: HashSet::new(),
                home_excluded_cards: HashSet::new(),
                home_excluded_artists: HashSet::new(),
                station_rotation_epoch: 0,
                home_more_cycle: 0,
                is_loading_more_home: false,
                home_gen: 0,
                search_gen: 0,
                detail_gen: 0,
                prefetch_gen: 0,
                extend_queue_gen: 0,
                is_extending_queue: false,
                lyrics_gen: 0,
                video_gen: 0,
                lyrics_offset_ms: 0,
                lyrics_cache: LyricsSnapshot::default(),
                pending_handoff_seek_ms: None,
                paused_while_resolving: false,
                last_emitted_idle: false,
                sleep_timer_end: None,
                last_tick_at: None,
                connect_sync_ticks: 0,
            }),
            audio: Mutex::new(None),
            connect: Mutex::new(None),
            monitor_sync: Arc::new(MonitorSync {
                wake_lock: Mutex::new(false),
                wake_cv: Condvar::new(),
                shutdown_flag: AtomicBool::new(false),
            }),
            monitor_handle: Mutex::new(None),
        });

        ctrl.start_connect_engine();

        let weak = Arc::downgrade(&ctrl);
        let mon_sync = Arc::clone(&ctrl.monitor_sync);
        if let Ok(handle) = thread::Builder::new()
            .name("ferric-ctrl-mon".to_string())
            .spawn(move || monitor_loop(weak, mon_sync))
        {
            let mut slot = ctrl
                .monitor_handle
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            *slot = Some(handle);
        }

        Ok(ctrl)
    }

    #[must_use]
    pub fn is_audio_started(&self) -> bool {
        self.audio
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Lazily start `AudioEngine` (called inside `FerricPlaybackService` or on first play request).
    pub fn ensure_audio_started(&self) -> ControllerResult<()> {
        {
            let guard = self.audio.lock().unwrap_or_else(|p| p.into_inner());
            if guard.is_some() {
                return Ok(());
            }
        }
        let (eq, dsp, volume) = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            (st.eq.clone(), st.dsp.clone(), st.session.volume)
        };
        let engine = if self.cfg.headless_audio {
            AudioEngine::start_headless(eq)
        } else {
            AudioEngine::start_supervised(eq, dsp.output_device.clone())?
        };
        engine.send(EngineCommand::SetDspSettings(dsp));
        engine.send(EngineCommand::SetVolume(volume));

        let mut guard = self.audio.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            *guard = Some(engine);
        }
        Ok(())
    }

    /// Release idle audio engine resources when paused/stopped for prolonged background standby.
    pub fn suspend_audio_if_idle(&self) {
        let playing = self.telemetry().is_playing;
        if !playing {
            let mut guard = self.audio.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(eng) = guard.as_mut() {
                eng.poll_device_recovery();
            }
        }
    }

    /// Generate and persist Ferric's 4 out-of-the-box musical WAV demo tracks when the library is empty.
    pub fn ensure_demo_seed(&self) -> ControllerResult<usize> {
        let existing_empty = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.list_tracks("")?.is_empty()
        };
        if !existing_empty {
            return Ok(0);
        }
        let demo_dir = self.cfg.data_dir.join("demo_tracks");
        let tracks = ferric_audio::demo_synth::ensure_demo_tracks(&demo_dir)?;
        let count = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.upsert_tracks(&tracks)?
        };
        self.sink.on_library_changed();
        Ok(count)
    }

    pub fn list_songs(&self, query: &str) -> ControllerResult<Vec<Track>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Ok(st.db.list_tracks(query)?)
    }

    pub fn list_playlists(&self) -> ControllerResult<Vec<Playlist>> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Ok(st.db.list_playlists()?)
    }

    pub fn play_track(self: &Arc<Self>, track_id: &str) -> ControllerResult<()> {
        let songs = self.list_songs("")?;
        let Some(idx) = songs.iter().position(|t| t.id == track_id) else {
            return Err(ControllerError::State(format!(
                "track not found: {track_id}"
            )));
        };
        self.play_queue(songs, idx, "Library".to_string())
    }

    pub fn play_queue(
        self: &Arc<Self>,
        tracks: Vec<Track>,
        start_index: usize,
        context: String,
    ) -> ControllerResult<()> {
        if tracks.is_empty() {
            return Err(ControllerError::State(
                "cannot play empty queue".to_string(),
            ));
        }
        if self.is_controlling_remote() {
            let entries: Vec<ferric_sync::QueueEntry> = tracks
                .iter()
                .take(200)
                .map(ferric_sync::QueueEntry::from_track)
                .collect();
            let clamped = start_index.min(entries.len().saturating_sub(1));
            self.forward_remote_action(ferric_sync::RemoteAction::PlayQueue {
                tracks: entries,
                start_index: clamped,
                context,
            });
            return Ok(());
        }
        self.play_queue_local_only(tracks, start_index, context)
    }

    pub(crate) fn play_queue_local_only(
        self: &Arc<Self>,
        tracks: Vec<Track>,
        start_index: usize,
        context: String,
    ) -> ControllerResult<()> {
        if tracks.is_empty() {
            return Err(ControllerError::State(
                "cannot play empty queue".to_string(),
            ));
        }
        self.ensure_audio_started()?;
        let clamped_idx = start_index.min(tracks.len() - 1);
        let current_seq = self.telemetry().track_finished_seq;
        let (current, up_next, gen_id, should_auto_extend) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.queue.set_tracks(tracks, Some(clamped_idx));
            st.queue_context = context;
            st.last_finished_seq = current_seq;
            st.paused_while_resolving = false;
            st.resolve_gen = st.resolve_gen.wrapping_add(1);
            st.extend_queue_gen = st.extend_queue_gen.wrapping_add(1);
            st.is_extending_queue = false;
            let cur = st.queue.current().cloned();
            let next_list: Vec<Track> = st
                .queue
                .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                .into_iter()
                .cloned()
                .collect();
            let auto_ext = next_list.len() < 8
                && st.queue.repeat != RepeatMode::All
                && cur.as_ref().is_some_and(|t| {
                    t.source != TrackSource::Local || t.youtube_video_id.is_some()
                });
            if let Some(t) = &cur {
                st.play_tracker = Some(ActivePlayTracker {
                    track_id: t.id.clone(),
                    artist: t.artist.clone(),
                    duration_ms: t.duration_ms,
                    listened_ms: 0,
                    logged: false,
                });
            }
            (cur, next_list, st.resolve_gen, auto_ext)
        };

        self.sink.on_now_playing(current.clone());
        self.sink.on_queue_changed(up_next);

        if let Some(track) = current {
            self.dispatch_track_playback(&track, gen_id)?;
        }
        if should_auto_extend {
            let _ = self.extend_queue_up_next();
        }
        self.push_connect_playback_snapshot();
        self.wake_monitor();
        Ok(())
    }

    fn dispatch_track_playback(
        self: &Arc<Self>,
        track: &Track,
        gen_id: u64,
    ) -> ControllerResult<()> {
        let local_path = self.resolve_immediate_path(track);
        if let Some(path) = local_path {
            self.send_audio(EngineCommand::LoadAndPlayTrack {
                path,
                replay_gain_db: track.replay_gain_db,
                fallback_duration_ms: track.duration_ms,
            });
            self.queue_upcoming_deck_b();
            self.prefetch_upcoming_online_track();
            return Ok(());
        }

        // Online streaming track: pause stale Deck A and clear Deck B before resolving off-thread
        // so an old track cannot finish or crossfade mid-download and hijack the queue.
        self.send_audio(EngineCommand::Pause);
        self.send_audio(EngineCommand::QueueNext(None));
        let this = Arc::clone(self);
        let track_clone = track.clone();
        let _ = thread::Builder::new()
            .name("ferric-stream-res".to_string())
            .spawn(move || {
                this.resolve_online_track_worker(track_clone, gen_id);
            });
        Ok(())
    }

    fn resolve_immediate_path(&self, track: &Track) -> Option<PathBuf> {
        {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if let Ok(Some(dl)) = st.db.get_offline_download(&track.id) {
                let p = PathBuf::from(dl.local_path);
                if is_playable_local_path(&p) {
                    return Some(p);
                }
            }
        }
        track
            .path
            .as_ref()
            .filter(|p| is_playable_local_path(p))
            .cloned()
    }

    fn resolve_stream_path_blocking(
        &self,
        track: &Track,
    ) -> Result<(PathBuf, Option<String>, String, u32, u64), String> {
        let (session, quality, cache_dir, subsonic_creds) = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let sub_creds =
                if st.accounts.subsonic_connected && !st.accounts.subsonic_url.is_empty() {
                    Some(SubsonicCredentials {
                        base_url: st.accounts.subsonic_url.clone(),
                        username: st.accounts.subsonic_username.clone(),
                        password: st.accounts.subsonic_password.clone(),
                    })
                } else {
                    None
                };
            (
                active_ytm_session(&st.accounts),
                st.dsp.audio_quality,
                self.cfg.cache_dir.join("stream_cache"),
                sub_creds,
            )
        };

        if track.source == TrackSource::Subsonic {
            let creds =
                subsonic_creds.ok_or_else(|| "Subsonic server is not connected".to_string())?;
            let cached =
                download_and_cache_subsonic_stream(&creds, &track.id, &track.codec, &cache_dir)
                    .map_err(|e| e.to_string())?;
            return Ok((
                cached.path,
                None,
                cached.codec,
                track.bitrate_kbps,
                track.duration_ms,
            ));
        }

        let mut video_id = track
            .youtube_video_id
            .clone()
            .or_else(|| track.id.strip_prefix("yt:video:").map(ToOwned::to_owned));
        if video_id.is_none() {
            video_id = resolve_best_video_id_for_track(&session, &track.artist, &track.title)
                .map_err(|e| e.to_string())?;
        }
        let vid =
            video_id.ok_or_else(|| format!("No playable stream source for {}", track.title))?;
        let cached =
            resolve_and_cache_audio_stream_with_quality(&vid, &session, &cache_dir, quality)
                .map_err(|e| e.to_string())?;
        Ok((
            cached.path,
            Some(vid),
            cached.codec,
            cached.bitrate_kbps,
            cached.duration_ms,
        ))
    }

    fn resolve_online_track_worker(self: &Arc<Self>, track: Track, gen_id: u64) {
        match self.resolve_stream_path_blocking(&track) {
            Ok((path, vid, codec, bitrate_kbps, duration_ms)) => {
                let (is_still_current, pending_seek, was_paused) = {
                    let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.queue.update_track_cached_stream(
                        &track.id,
                        path.clone(),
                        vid,
                        &codec,
                        bitrate_kbps,
                        duration_ms,
                    );
                    let current = st.resolve_gen == gen_id;
                    let seek = if current {
                        st.pending_handoff_seek_ms.take()
                    } else {
                        None
                    };
                    let paused = st.paused_while_resolving;
                    (current, seek, paused)
                };
                if is_still_current {
                    self.send_audio(EngineCommand::LoadAndPlayTrack {
                        path,
                        replay_gain_db: track.replay_gain_db,
                        fallback_duration_ms: if duration_ms > 0 {
                            duration_ms
                        } else {
                            track.duration_ms
                        },
                    });
                    if let Some(ms) = pending_seek
                        && ms > 1_000
                    {
                        self.send_audio(EngineCommand::SeekMs(ms));
                    }
                    if was_paused {
                        self.send_audio(EngineCommand::Pause);
                    }
                    self.queue_upcoming_deck_b();
                    self.prefetch_upcoming_online_track();
                    self.push_connect_playback_snapshot();
                    self.wake_monitor();
                }
            }
            Err(err) => {
                let is_still_current = {
                    let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.resolve_gen == gen_id
                };
                if !is_still_current {
                    return;
                }
                if track.source == TrackSource::Local && track.path.is_none() {
                    self.sink.on_toast(format!(
                        "Track '{}' is stored locally on the other device",
                        track.title
                    ));
                } else {
                    self.sink.on_toast(format!("Stream failed: {err}"));
                }
            }
        }
    }

    /// Background prefetch for the next uncached online track so Deck B loads seamlessly.
    fn prefetch_upcoming_online_track(self: &Arc<Self>) {
        let (candidate, gen_id) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let next_track = st.queue.peek_upcoming(1).into_iter().next().cloned();
            let needs_fetch = next_track.filter(|next| {
                let has_offline = st
                    .db
                    .get_offline_download(&next.id)
                    .ok()
                    .flatten()
                    .is_some_and(|d| is_playable_local_path(&PathBuf::from(d.local_path)));
                let has_local = next
                    .path
                    .as_ref()
                    .is_some_and(|p| is_playable_local_path(p));
                !has_offline && !has_local
            });
            let Some(track) = needs_fetch else {
                return;
            };
            st.prefetch_gen = st.prefetch_gen.wrapping_add(1);
            (track, st.prefetch_gen)
        };

        let this = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("ferric-deckb-pre".to_string())
            .spawn(move || {
                if let Ok((path, vid, codec, bitrate_kbps, duration_ms)) =
                    this.resolve_stream_path_blocking(&candidate)
                {
                    let still_relevant = {
                        let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                        st.queue.update_track_cached_stream(
                            &candidate.id,
                            path,
                            vid,
                            &codec,
                            bitrate_kbps,
                            duration_ms,
                        );
                        st.prefetch_gen == gen_id
                    };
                    if still_relevant {
                        this.queue_upcoming_deck_b();
                    }
                }
            });
    }

    fn queue_upcoming_deck_b(&self) {
        let next_spec = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let cur = st.queue.current();
            let upcoming = st.queue.peek_upcoming(1).into_iter().next().cloned();
            upcoming.and_then(|next| {
                let path = st
                    .db
                    .get_offline_download(&next.id)
                    .ok()
                    .flatten()
                    .map(|d| PathBuf::from(d.local_path))
                    .filter(|p| is_playable_local_path(p))
                    .or_else(|| next.path.clone().filter(|p| is_playable_local_path(p)))?;
                let same_album = cur.is_some_and(|c| same_album_consecutive(c, &next));
                Some((path, same_album, next.replay_gain_db, next.duration_ms))
            })
        };
        match next_spec {
            Some((path, same_album_consecutive, replay_gain_db, fallback_duration_ms)) => {
                self.send_audio(EngineCommand::QueueNextTrack {
                    path: Some(path),
                    same_album_consecutive,
                    replay_gain_db,
                    fallback_duration_ms,
                });
            }
            None => {
                self.send_audio(EngineCommand::QueueNext(None));
            }
        }
    }

    fn send_audio(&self, cmd: EngineCommand) {
        let guard = self.audio.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(eng) = guard.as_ref() {
            eng.send(cmd);
        }
    }

    fn wake_monitor(&self) {
        let mut active = self
            .monitor_sync
            .wake_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        *active = true;
        self.monitor_sync.wake_cv.notify_one();
    }

    pub fn set_sleep_timer_minutes(&self, minutes: u32) {
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.sleep_timer_end = if minutes == 0 {
                None
            } else {
                Some(Instant::now() + Duration::from_secs(u64::from(minutes) * 60))
            };
        }
        if minutes == 0 {
            self.sink.on_toast("Sleep timer cancelled".to_string());
        } else {
            self.sink
                .on_toast(format!("Sleep timer set for {minutes} min"));
            self.wake_monitor();
        }
    }

    #[must_use]
    pub fn sleep_timer_remaining_secs(&self) -> u64 {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.sleep_timer_end
            .map(|end| end.saturating_duration_since(Instant::now()).as_secs())
            .unwrap_or(0)
    }

    pub fn toggle_play_pause(&self) -> ControllerResult<()> {
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::TogglePlayPause);
            return Ok(());
        }
        self.toggle_play_pause_local_only()
    }

    pub(crate) fn toggle_play_pause_local_only(&self) -> ControllerResult<()> {
        self.ensure_audio_started()?;
        let currently_playing = self.telemetry().is_playing;
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.paused_while_resolving = currently_playing;
        }
        self.send_audio(EngineCommand::TogglePlayPause);
        self.push_connect_playback_snapshot();
        self.wake_monitor();
        Ok(())
    }

    pub fn play(&self) -> ControllerResult<()> {
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::Play);
            return Ok(());
        }
        self.play_local_only()
    }

    pub(crate) fn play_local_only(&self) -> ControllerResult<()> {
        self.ensure_audio_started()?;
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.paused_while_resolving = false;
        }
        self.send_audio(EngineCommand::Play);
        self.push_connect_playback_snapshot();
        self.wake_monitor();
        Ok(())
    }

    pub fn pause(&self) {
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::Pause);
            return;
        }
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.paused_while_resolving = true;
        }
        self.send_audio(EngineCommand::Pause);
        self.push_connect_playback_snapshot();
    }

    pub fn next(self: &Arc<Self>) -> ControllerResult<()> {
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::SkipNext);
            return Ok(());
        }
        self.next_local_only()
    }

    pub(crate) fn next_local_only(self: &Arc<Self>) -> ControllerResult<()> {
        self.ensure_audio_started()?;
        let current_seq = self.telemetry().track_finished_seq;
        let (next_track, up_next, gen_id, should_extend) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let next_t = st.queue.next_track_manual().cloned();
            st.last_finished_seq = current_seq;
            st.paused_while_resolving = false;
            st.resolve_gen = st.resolve_gen.wrapping_add(1);
            let upcoming: Vec<Track> = st
                .queue
                .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                .into_iter()
                .cloned()
                .collect();
            let auto_ext = upcoming.len() <= 5
                && st.queue.repeat != RepeatMode::All
                && next_t.as_ref().is_some_and(|t| {
                    t.source != TrackSource::Local || t.youtube_video_id.is_some()
                });
            if let Some(t) = &next_t {
                st.play_tracker = Some(ActivePlayTracker {
                    track_id: t.id.clone(),
                    artist: t.artist.clone(),
                    duration_ms: t.duration_ms,
                    listened_ms: 0,
                    logged: false,
                });
            }
            (next_t, upcoming, st.resolve_gen, auto_ext)
        };
        self.sink.on_now_playing(next_track.clone());
        self.sink.on_queue_changed(up_next);
        if let Some(t) = next_track {
            self.dispatch_track_playback(&t, gen_id)?;
        }
        if should_extend {
            let _ = self.extend_queue_up_next();
        }
        self.push_connect_playback_snapshot();
        self.wake_monitor();
        Ok(())
    }

    /// Halt playback, clear the active queue & session tracker, and dismiss Now Playing UI.
    pub fn stop_playback(&self) {
        self.send_audio(EngineCommand::Pause);
        self.send_audio(EngineCommand::QueueNext(None));
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.resolve_gen = st.resolve_gen.wrapping_add(1);
            st.prefetch_gen = st.prefetch_gen.wrapping_add(1);
            st.extend_queue_gen = st.extend_queue_gen.wrapping_add(1);
            st.is_extending_queue = false;
            st.paused_while_resolving = true;
            st.pending_handoff_seek_ms = None;
            st.play_tracker = None;
            st.queue.set_tracks(Vec::new(), None);
            st.queue_context.clear();
        }
        self.sink.on_now_playing(None);
        self.sink.on_queue_changed(Vec::new());
        self.push_connect_playback_snapshot();
        self.wake_monitor();
    }

    /// Extend the active Up Next queue using Desktop Anchor + Horizon triangulation combined with
    /// Spotify-style cohesive sequencing (80% cohesive multi-seed radio + 20% affinity library mix,
    /// strict 3-slot artist cooldown ring, normalized title deduplication, and album de-clumping).
    pub fn extend_queue_up_next(self: &Arc<Self>) -> bool {
        let (
            anchor_seed,
            horizon_seed,
            seed_locale,
            session,
            existing_ids,
            existing_cores,
            tail_artists,
            tail_album,
            top_artist_norms,
            library_pool,
            req_gen,
        ) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if st.is_extending_queue {
                return false;
            }
            let upcoming: Vec<Track> = st
                .queue
                .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                .into_iter()
                .cloned()
                .collect();
            let cur = st.queue.current().cloned();
            let tail = upcoming
                .last()
                .cloned()
                .or_else(|| st.queue.tracks().last().cloned());
            // Anchor on currently playing song when available so Up Next never drifts off-vibe;
            // bridge smoothly with the tail song when its artist differs.
            let Some(anchor) = cur.clone().or_else(|| tail.clone()) else {
                return false;
            };
            let horizon = tail.filter(|t| {
                t.id != anchor.id
                    && primary_artist_key(&t.artist) != primary_artist_key(&anchor.artist)
            });
            let locale = detect_track_locale(&anchor.title, &anchor.artist, &anchor.album);

            let q_tracks = st.queue.tracks();
            let existing: HashSet<String> = q_tracks.iter().map(|t| t.id.clone()).collect();
            let cores: Vec<String> = q_tracks
                .iter()
                .rev()
                .take(40)
                .map(|t| normalize_core_title(&t.title))
                .filter(|c| !c.is_empty())
                .collect();
            let recent_tail_artists: Vec<String> = upcoming
                .iter()
                .rev()
                .take(3)
                .chain(cur.iter())
                .map(|t| primary_artist_key(&t.artist))
                .filter(|a| !a.is_empty())
                .collect();
            let last_alb = upcoming
                .last()
                .or(cur.as_ref())
                .map(|t| t.album.trim().to_ascii_lowercase())
                .unwrap_or_default();

            let mut top_art_scores: Vec<(&String, u32)> = st
                .session
                .artist_play_counts
                .iter()
                .map(|(k, &v)| (k, v))
                .collect();
            top_art_scores.sort_by_key(|a| std::cmp::Reverse(a.1));
            let top_artists: HashSet<String> = top_art_scores
                .into_iter()
                .take(32)
                .map(|(a, _)| primary_artist_key(a))
                .filter(|a| !a.is_empty())
                .collect();

            let lib = st.db.list_tracks("").unwrap_or_default();
            st.extend_queue_gen = st.extend_queue_gen.wrapping_add(1);
            st.is_extending_queue = true;
            (
                anchor,
                horizon,
                locale,
                active_ytm_session(&st.accounts),
                existing,
                cores,
                recent_tail_artists,
                last_alb,
                top_artists,
                lib,
                st.extend_queue_gen,
            )
        };

        let this = Arc::clone(self);
        let spawn_res = thread::Builder::new()
            .name("ferric-queue-extend".to_string())
            .spawn(move || {
                let mut radio_pools: Vec<Vec<Track>> = Vec::with_capacity(2);
                for seed_opt in [Some(anchor_seed.clone()), horizon_seed] {
                    let Some(candidate) = seed_opt else {
                        continue;
                    };
                    let vid = candidate
                        .youtube_video_id
                        .clone()
                        .or_else(|| {
                            candidate
                                .id
                                .strip_prefix("yt:video:")
                                .map(ToOwned::to_owned)
                        })
                        .or_else(|| {
                            resolve_best_video_id_for_track(
                                &session,
                                &candidate.artist,
                                &candidate.title,
                            )
                            .ok()
                            .flatten()
                        });
                    if let Some(video_id) = vid
                        && let Ok(batch) = fetch_radio_up_next_for_track(
                            &session,
                            &video_id,
                            &candidate.title,
                            &candidate.artist,
                            &candidate.album,
                            28,
                            true,
                        )
                        && !batch.is_empty()
                    {
                        radio_pools.push(batch);
                    }
                }

                let sequenced = sequence_spotify_up_next(
                    &radio_pools,
                    library_pool,
                    seed_locale,
                    &anchor_seed.title,
                    &existing_ids,
                    &existing_cores,
                    &tail_artists,
                    &tail_album,
                    &top_artist_norms,
                    28,
                );

                let appended_upcoming = {
                    let mut st = this.state.lock().unwrap_or_else(|p| p.into_inner());
                    let valid_gen = st.extend_queue_gen == req_gen;
                    st.is_extending_queue = false;
                    if !valid_gen || st.queue.is_empty() || sequenced.is_empty() {
                        None
                    } else {
                        let added = st.queue.append_tracks(sequenced);
                        if added > 0 {
                            Some(
                                st.queue
                                    .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                                    .into_iter()
                                    .cloned()
                                    .collect::<Vec<Track>>(),
                            )
                        } else {
                            None
                        }
                    }
                };

                if let Some(upcoming) = appended_upcoming {
                    this.queue_upcoming_deck_b();
                    this.sink.on_queue_changed(upcoming);
                    this.push_connect_playback_snapshot();
                }
            });

        if spawn_res.is_err() {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.is_extending_queue = false;
            return false;
        }
        true
    }

    pub fn prev(self: &Arc<Self>) -> ControllerResult<()> {
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::SkipPrev);
            return Ok(());
        }
        self.prev_local_only()
    }

    pub(crate) fn prev_local_only(self: &Arc<Self>) -> ControllerResult<()> {
        self.ensure_audio_started()?;
        if self.telemetry().position_ms > 3_500 {
            self.send_audio(EngineCommand::SeekMs(0));
            self.push_connect_playback_snapshot();
            return Ok(());
        }
        let current_seq = self.telemetry().track_finished_seq;
        let (prev_track, up_next, gen_id) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let prev_t = st.queue.prev_track().cloned();
            st.last_finished_seq = current_seq;
            st.paused_while_resolving = false;
            st.resolve_gen = st.resolve_gen.wrapping_add(1);
            let upcoming: Vec<Track> = st
                .queue
                .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                .into_iter()
                .cloned()
                .collect();
            if let Some(t) = &prev_t {
                st.play_tracker = Some(ActivePlayTracker {
                    track_id: t.id.clone(),
                    artist: t.artist.clone(),
                    duration_ms: t.duration_ms,
                    listened_ms: 0,
                    logged: false,
                });
            }
            (prev_t, upcoming, st.resolve_gen)
        };
        self.sink.on_now_playing(prev_track.clone());
        self.sink.on_queue_changed(up_next);
        if let Some(t) = prev_track {
            self.dispatch_track_playback(&t, gen_id)?;
        }
        self.push_connect_playback_snapshot();
        self.wake_monitor();
        Ok(())
    }

    pub fn seek_ms(&self, pos_ms: u64) {
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::SeekTo {
                position_ms: pos_ms,
            });
            return;
        }
        self.send_audio(EngineCommand::SeekMs(pos_ms));
        self.push_connect_playback_snapshot();
    }

    pub fn set_volume(&self, vol: f32) {
        let clamped = if vol.is_finite() {
            vol.clamp(0.0, 1.5)
        } else {
            0.85
        };
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.session.volume = clamped;
        }
        self.send_audio(EngineCommand::SetVolume(clamped));
        self.push_connect_playback_snapshot();
    }

    pub fn set_ducking(&self, duck: bool) {
        self.send_audio(EngineCommand::SetDucking(duck));
    }

    #[must_use]
    pub fn toggle_shuffle(&self) -> bool {
        let (enabled, up_next) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let next_val = !st.queue.shuffle;
            st.queue.set_shuffle(next_val);
            st.session.shuffle = next_val;
            let upcoming: Vec<Track> = st
                .queue
                .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                .into_iter()
                .cloned()
                .collect();
            (next_val, upcoming)
        };
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::SetShuffle(enabled));
        } else {
            self.queue_upcoming_deck_b();
            self.push_connect_playback_snapshot();
        }
        self.sink.on_queue_changed(up_next);
        enabled
    }

    #[must_use]
    pub fn cycle_repeat(&self) -> RepeatMode {
        let (mode, up_next) = {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let next_mode = st.queue.repeat.next();
            st.queue.repeat = next_mode;
            st.session.repeat = next_mode;
            let upcoming: Vec<Track> = st
                .queue
                .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                .into_iter()
                .cloned()
                .collect();
            (next_mode, upcoming)
        };
        if self.is_controlling_remote() {
            self.forward_remote_action(ferric_sync::RemoteAction::SetRepeat(mode));
        } else {
            self.queue_upcoming_deck_b();
            self.push_connect_playback_snapshot();
        }
        self.sink.on_queue_changed(up_next);
        mode
    }

    #[must_use]
    pub fn telemetry(&self) -> TelemetrySnapshot {
        let guard = self.audio.lock().unwrap_or_else(|p| p.into_inner());
        guard
            .as_ref()
            .map_or_else(TelemetrySnapshot::default, AudioEngine::telemetry)
    }

    #[must_use]
    pub fn current_track(&self) -> Option<Track> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.queue.current().cloned()
    }

    #[must_use]
    pub fn up_next(&self, limit: usize) -> Vec<Track> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.queue.current().is_some() {
            st.queue.peek_upcoming(limit).into_iter().cloned().collect()
        } else {
            st.queue.tracks().iter().take(limit).cloned().collect()
        }
    }

    #[must_use]
    pub fn get_theme(&self) -> ThemeConfig {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .theme
            .clone()
    }

    pub fn set_theme(&self, theme: ThemeConfig) -> ControllerResult<()> {
        let sanitized = theme.sanitized();
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.db.save_theme(&sanitized)?;
        st.theme = sanitized;
        Ok(())
    }

    #[must_use]
    pub fn get_eq(&self) -> EqConfig {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .eq
            .clone()
    }

    pub fn set_eq(&self, eq: EqConfig) -> ControllerResult<()> {
        let sanitized = eq.sanitized();
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.save_eq(&sanitized)?;
            st.eq = sanitized.clone();
        }
        self.send_audio(EngineCommand::SetEq(sanitized));
        Ok(())
    }

    #[must_use]
    pub fn get_dsp(&self) -> PlaybackDspSettings {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .dsp
            .clone()
    }

    pub fn set_dsp(&self, dsp: PlaybackDspSettings) -> ControllerResult<()> {
        let sanitized = dsp.sanitized();
        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db.save_playback_dsp(&sanitized)?;
            st.dsp = sanitized.clone();
        }
        self.send_audio(EngineCommand::SetDspSettings(sanitized));
        Ok(())
    }

    #[cfg(test)]
    pub fn record_qualified_play_for_test(
        &self,
        track_id: &str,
        artist: &str,
        listened_ms: u64,
        duration_ms: u64,
    ) -> ControllerResult<bool> {
        if !qualifies_as_play(listened_ms, duration_ms) {
            return Ok(false);
        }
        let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.db
            .record_play_history(track_id, artist, unix_now_secs(), listened_ms)?;
        st.session
            .record_play_with_artist(track_id, artist, listened_ms);
        st.db.save_session_state(&st.session)?;
        Ok(true)
    }

    pub fn get_listening_stats(
        &self,
        tz_offset_secs: i64,
    ) -> ControllerResult<ListeningStatsSummary> {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        Ok(st.db.get_listening_stats(unix_now_secs(), tz_offset_secs)?)
    }

    /// Reclaim idle SQLite page cache on Android `onTrimMemory`.
    pub fn trim_memory(&self) {
        let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
        st.db.shrink_memory();
    }

    /// Flush session resume state, stop background monitor thread, and tear down audio cleanly.
    pub fn shutdown(&self) {
        self.monitor_sync
            .shutdown_flag
            .store(true, Ordering::Release);
        self.wake_monitor();
        if let Some(handle) = self
            .monitor_handle
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            && handle.thread().id() != thread::current().id()
        {
            let _ = handle.join();
        }
        if let Some(conn) = self
            .connect
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            conn.shutdown();
        }
        let dropped_audio = self.audio.lock().unwrap_or_else(|p| p.into_inner()).take();
        drop(dropped_audio);
        {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let _ = st.db.save_session_state(&st.session);
            st.db.shrink_memory_aggressive();
        }
    }

    fn monitor_tick(self: &Arc<Self>) -> bool {
        let snap = self.telemetry();
        let mut track_changed: Option<(Option<Track>, Vec<Track>, bool, u64)> = None;
        let mut sleep_expired = false;
        let emit_telemetry;

        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let now = Instant::now();
            let elapsed_ms = st
                .last_tick_at
                .map(|prev| {
                    u64::try_from(now.saturating_duration_since(prev).as_millis())
                        .unwrap_or(200)
                        .clamp(1, 2_000)
                })
                .unwrap_or(200);
            st.last_tick_at = Some(now);

            if let Some(end_at) = st.sleep_timer_end
                && now >= end_at
            {
                st.sleep_timer_end = None;
                sleep_expired = true;
            }

            if snap.is_playing
                && let Some(mut tracker) = st.play_tracker.take()
            {
                tracker.listened_ms = tracker.listened_ms.saturating_add(elapsed_ms);
                let dur = if tracker.duration_ms > 0 {
                    tracker.duration_ms
                } else {
                    snap.duration_ms
                };
                if !tracker.logged && qualifies_as_play(tracker.listened_ms, dur) {
                    tracker.logged = true;
                    let _ = st.db.record_play_history(
                        &tracker.track_id,
                        &tracker.artist,
                        unix_now_secs(),
                        tracker.listened_ms,
                    );
                    st.session.record_play_with_artist(
                        &tracker.track_id,
                        &tracker.artist,
                        dur.max(tracker.listened_ms),
                    );
                    let _ = st.db.save_session_state(&st.session);
                }
                st.play_tracker = Some(tracker);
            }

            if snap.track_finished_seq != st.last_finished_seq {
                st.last_finished_seq = snap.track_finished_seq;
                let next_t = st.queue.next_track().cloned();
                st.resolve_gen = st.resolve_gen.wrapping_add(1);
                let upcoming: Vec<Track> = st
                    .queue
                    .peek_upcoming(UP_NEXT_EMIT_LIMIT)
                    .into_iter()
                    .cloned()
                    .collect();
                if let Some(t) = &next_t {
                    st.play_tracker = Some(ActivePlayTracker {
                        track_id: t.id.clone(),
                        artist: t.artist.clone(),
                        duration_ms: t.duration_ms,
                        listened_ms: 0,
                        logged: false,
                    });
                } else {
                    st.play_tracker = None;
                }
                track_changed = Some((next_t, upcoming, snap.auto_transitioned, st.resolve_gen));
            }

            // Avoid redundant JNI callbacks every 5s while playback is idle/paused.
            if snap.is_playing || track_changed.is_some() || sleep_expired {
                st.last_emitted_idle = false;
                emit_telemetry = true;
            } else if !st.last_emitted_idle {
                st.last_emitted_idle = true;
                emit_telemetry = true;
            } else {
                emit_telemetry = false;
            }
        }

        if sleep_expired {
            self.pause();
            self.sink
                .on_toast("Sleep timer finished; playback paused".to_string());
        }

        if emit_telemetry {
            self.sink.on_telemetry(snap);
        }

        if let Some((next_track, up_next, auto_transitioned, gen_id)) = track_changed {
            let should_extend = up_next.len() <= 5
                && next_track.as_ref().is_some_and(|t| {
                    t.source != TrackSource::Local || t.youtube_video_id.is_some()
                });
            self.sink.on_now_playing(next_track.clone());
            self.sink.on_queue_changed(up_next);
            if auto_transitioned {
                self.queue_upcoming_deck_b();
                self.prefetch_upcoming_online_track();
            } else if let Some(t) = next_track {
                let _ = self.dispatch_track_playback(&t, gen_id);
            }
            if should_extend {
                let _ = self.extend_queue_up_next();
            }
            self.push_connect_playback_snapshot();
        } else if snap.is_playing {
            let should_sync = {
                let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                st.connect_sync_ticks = st.connect_sync_ticks.wrapping_add(1);
                st.connect_sync_ticks.is_multiple_of(5)
            };
            if should_sync {
                self.push_connect_playback_snapshot();
            }
        }

        snap.is_playing || self.sleep_timer_remaining_secs() > 0
    }
}

fn monitor_loop(weak: Weak<FerricController>, mon_sync: Arc<MonitorSync>) {
    loop {
        if mon_sync.shutdown_flag.load(Ordering::Acquire) {
            return;
        }
        let active = {
            let Some(ctrl) = weak.upgrade() else {
                return;
            };
            ctrl.monitor_tick()
        };
        if mon_sync.shutdown_flag.load(Ordering::Acquire) {
            return;
        }
        if active {
            thread::sleep(Duration::from_millis(200));
        } else {
            let mut guard = mon_sync.wake_lock.lock().unwrap_or_else(|p| p.into_inner());
            *guard = false;
            while !*guard && !mon_sync.shutdown_flag.load(Ordering::Acquire) {
                let (next_g, _) = mon_sync
                    .wake_cv
                    .wait_timeout(guard, Duration::from_secs(5))
                    .unwrap_or_else(|p| p.into_inner());
                guard = next_g;
                if !*guard {
                    break;
                }
            }
        }
    }
}

impl Drop for FerricController {
    fn drop(&mut self) {
        self.monitor_sync
            .shutdown_flag
            .store(true, Ordering::Release);
        self.wake_monitor();
        if let Some(conn) = self
            .connect
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            conn.shutdown();
        }
        if let Some(handle) = self
            .monitor_handle
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            && handle.thread().id() != thread::current().id()
        {
            let _ = handle.join();
        }
    }
}

/// Score and sequence Up Next candidates using Spotify-style blend (80% cohesive radio
/// discovery + 20% personal affinity mix) and strict 3-slot artist cooldown separation.
#[allow(clippy::too_many_arguments)]
fn sequence_spotify_up_next(
    radio_pools: &[Vec<Track>],
    library_pool: Vec<Track>,
    seed_locale: MusicLocale,
    seed_title: &str,
    existing_ids: &HashSet<String>,
    existing_cores: &[String],
    tail_artists: &[String],
    tail_album: &str,
    top_artist_norms: &HashSet<String>,
    limit: usize,
) -> Vec<Track> {
    let seed_core = normalize_core_title(seed_title);
    let mut seen_ids: HashSet<String> = HashSet::with_capacity(64);
    let mut seen_cores: Vec<String> = Vec::with_capacity(64);

    struct RankedCand {
        track: Track,
        artist_key: String,
        album_low: String,
        score: i32,
        hits: usize,
        from_affinity: bool,
    }

    let mut by_id: HashMap<String, RankedCand> = HashMap::with_capacity(64);

    let is_duplicate_variant = |core: &str, extras: &[String]| -> bool {
        if core.is_empty() {
            return false;
        }
        if is_same_song_variant(&seed_core, core) {
            return true;
        }
        existing_cores
            .iter()
            .chain(extras.iter())
            .any(|ec| is_same_song_variant(ec, core))
    };

    for (pool_idx, pool) in radio_pools.iter().enumerate() {
        let mut seen_in_pool: HashSet<&str> = HashSet::with_capacity(pool.len());
        for (rank, t) in pool.iter().enumerate() {
            if existing_ids.contains(&t.id)
                || !seen_in_pool.insert(t.id.as_str())
                || is_low_quality_radio_candidate(&t.title, &t.artist, seed_title, t.duration_ms)
            {
                continue;
            }
            let cand_core = normalize_core_title(&t.title);
            let cand_locale = detect_track_locale(&t.title, &t.artist, &t.album);
            let artist_key = primary_artist_key(&t.artist);
            let locale_score = if cand_locale == seed_locale {
                24
            } else if seed_locale == MusicLocale::GlobalEn && cand_locale == MusicLocale::Indonesian
            {
                -42
            } else if cand_locale == MusicLocale::GlobalEn {
                8
            } else {
                -6
            };
            let rank_score = 32_i32.saturating_sub(rank as i32).max(4);
            let affinity_score = if top_artist_norms.contains(&artist_key) {
                14
            } else {
                0
            } + if t.liked { 10 } else { 0 };
            let anchor_bonus = if pool_idx == 0 { 10 } else { 4 };

            let entry = by_id.entry(t.id.clone()).or_insert_with(|| RankedCand {
                track: t.clone(),
                artist_key,
                album_low: t.album.trim().to_ascii_lowercase(),
                score: fastrand::i32(-6..=8),
                hits: 0,
                from_affinity: false,
            });
            entry.hits += 1;
            let consensus = if entry.hits >= 2 { 22 } else { 0 };
            entry.score += rank_score + locale_score + affinity_score + anchor_bonus + consensus;
            let _ = cand_core;
        }
    }

    // Inject Spotify-style personal affinity bridges (~20% mix) from unseen library tracks
    let mut lib_candidates: Vec<(i32, Track)> = Vec::with_capacity(32);
    for t in library_pool {
        if existing_ids.contains(&t.id)
            || by_id.contains_key(&t.id)
            || is_low_quality_radio_candidate(&t.title, &t.artist, seed_title, t.duration_ms)
        {
            continue;
        }
        let loc = detect_track_locale(&t.title, &t.artist, &t.album);
        if seed_locale == MusicLocale::GlobalEn && loc == MusicLocale::Indonesian && !t.liked {
            continue;
        }
        let ak = primary_artist_key(&t.artist);
        let s = if t.liked { 26 } else { 8 }
            + if loc == seed_locale { 22 } else { 0 }
            + if top_artist_norms.contains(&ak) {
                16
            } else {
                0
            }
            + fastrand::i32(0..=14);
        lib_candidates.push((s, t));
    }
    lib_candidates.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (score, t) in lib_candidates.into_iter().take(12) {
        let ak = primary_artist_key(&t.artist);
        let alb = t.album.trim().to_ascii_lowercase();
        by_id.insert(
            t.id.clone(),
            RankedCand {
                track: t,
                artist_key: ak,
                album_low: alb,
                score,
                hits: 1,
                from_affinity: true,
            },
        );
    }

    let mut radio_bucket: Vec<RankedCand> = Vec::with_capacity(by_id.len());
    let mut affinity_bucket: Vec<RankedCand> = Vec::with_capacity(12);
    for c in by_id.into_values() {
        if c.from_affinity {
            affinity_bucket.push(c);
        } else {
            radio_bucket.push(c);
        }
    }
    radio_bucket.sort_by_key(|c| std::cmp::Reverse(c.score));
    affinity_bucket.sort_by_key(|c| std::cmp::Reverse(c.score));

    let mut artist_window: VecDeque<String> = VecDeque::with_capacity(4);
    for a in tail_artists.iter().rev().take(3) {
        artist_window.push_back(a.clone());
    }
    let mut prev_album = tail_album.to_string();
    let mut out = Vec::with_capacity(limit);

    let pop_next_coherent = |bucket: &mut Vec<RankedCand>,
                             window: &VecDeque<String>,
                             prev_alb: &str,
                             seen_id_set: &HashSet<String>,
                             seen_core_list: &[String],
                             strict_cooldown: usize|
     -> Option<RankedCand> {
        let idx = bucket.iter().position(|c| {
            if seen_id_set.contains(&c.track.id) {
                return false;
            }
            let core = normalize_core_title(&c.track.title);
            if is_duplicate_variant(&core, seen_core_list) {
                return false;
            }
            let cooldown_violation = !c.artist_key.is_empty()
                && window
                    .iter()
                    .rev()
                    .take(strict_cooldown)
                    .any(|w| *w == c.artist_key);
            if cooldown_violation {
                return false;
            }
            if strict_cooldown >= 2
                && !prev_alb.is_empty()
                && !c.album_low.is_empty()
                && c.album_low == prev_alb
            {
                return false;
            }
            true
        });
        idx.map(|i| bucket.remove(i))
    };

    // Spotify cadence: 4 radio discovery tracks -> 1 affinity bridge track, respecting 3-artist cooldown.
    let mut slot = 0usize;
    while out.len() < limit && (!radio_bucket.is_empty() || !affinity_bucket.is_empty()) {
        let prefer_affinity =
            slot % 5 == 4 && !affinity_bucket.is_empty() && !radio_bucket.is_empty();
        let picked = if prefer_affinity {
            pop_next_coherent(
                &mut affinity_bucket,
                &artist_window,
                &prev_album,
                &seen_ids,
                &seen_cores,
                3,
            )
            .or_else(|| {
                pop_next_coherent(
                    &mut radio_bucket,
                    &artist_window,
                    &prev_album,
                    &seen_ids,
                    &seen_cores,
                    3,
                )
            })
        } else {
            pop_next_coherent(
                &mut radio_bucket,
                &artist_window,
                &prev_album,
                &seen_ids,
                &seen_cores,
                3,
            )
            .or_else(|| {
                pop_next_coherent(
                    &mut affinity_bucket,
                    &artist_window,
                    &prev_album,
                    &seen_ids,
                    &seen_cores,
                    3,
                )
            })
            .or_else(|| {
                pop_next_coherent(
                    &mut radio_bucket,
                    &artist_window,
                    &prev_album,
                    &seen_ids,
                    &seen_cores,
                    1,
                )
            })
            .or_else(|| {
                pop_next_coherent(
                    &mut affinity_bucket,
                    &artist_window,
                    &prev_album,
                    &seen_ids,
                    &seen_cores,
                    1,
                )
            })
            .or_else(|| {
                pop_next_coherent(
                    &mut radio_bucket,
                    &artist_window,
                    &prev_album,
                    &seen_ids,
                    &seen_cores,
                    0,
                )
            })
        };

        let Some(cand) = picked else {
            break;
        };
        let core = normalize_core_title(&cand.track.title);
        if !core.is_empty() {
            seen_cores.push(core);
        }
        seen_ids.insert(cand.track.id.clone());
        if !cand.artist_key.is_empty() {
            if artist_window.len() >= 3 {
                artist_window.pop_front();
            }
            artist_window.push_back(cand.artist_key);
        }
        if !cand.album_low.is_empty() {
            prev_album = cand.album_low;
        }
        out.push(cand.track);
        slot += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        now_playing: Mutex<Vec<Option<String>>>,
        telemetry_count: std::sync::atomic::AtomicUsize,
        library_updates: std::sync::atomic::AtomicUsize,
    }

    impl ControllerEventSink for RecordingSink {
        fn on_telemetry(&self, _telemetry: TelemetrySnapshot) {
            self.telemetry_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        fn on_now_playing(&self, track: Option<Track>) {
            self.now_playing.lock().unwrap().push(track.map(|t| t.id));
        }

        fn on_library_changed(&self) {
            self.library_updates
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn temp_cfg(label: &str) -> (PathBuf, ControllerConfig) {
        let root = std::env::temp_dir().join(format!(
            "ferric_ctrl_{label}_{}_{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let cfg = ControllerConfig {
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            device_id: format!("test_dev_{label}"),
            device_name: "Test Phone".to_string(),
            device_kind: ferric_sync::DeviceKind::Phone,
            enable_connect: false,
            headless_audio: true,
        };
        (root, cfg)
    }

    #[test]
    fn controller_starts_lazily_without_audio_engine_and_loads_demo_tracks() {
        let (root, cfg) = temp_cfg("lazy");
        let sink = Arc::new(RecordingSink::default());
        let ctrl = FerricController::new(cfg, Arc::clone(&sink) as Arc<dyn ControllerEventSink>)
            .expect("new controller");

        assert!(
            !ctrl.is_audio_started(),
            "AudioEngine must NOT start during controller construction"
        );
        let seeded = ctrl.ensure_demo_seed().expect("seed demo");
        assert_eq!(seeded, 4);
        assert_eq!(ctrl.list_songs("").expect("list songs").len(), 4);
        assert!(
            !ctrl.is_audio_started(),
            "Seeding and listing library must not start AudioEngine"
        );
        assert!(
            sink.library_updates
                .load(std::sync::atomic::Ordering::Relaxed)
                >= 1
        );

        ctrl.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_plays_queue_advances_tracks_and_emits_events() {
        let (root, cfg) = temp_cfg("play");
        let sink = Arc::new(RecordingSink::default());
        let ctrl = FerricController::new(cfg, Arc::clone(&sink) as Arc<dyn ControllerEventSink>)
            .expect("new controller");
        ctrl.ensure_demo_seed().expect("seed");
        let songs = ctrl.list_songs("").expect("songs");
        assert!(songs.len() >= 2);

        ctrl.play_queue(songs.clone(), 0, "Demo".to_string())
            .expect("play_queue");
        assert!(ctrl.is_audio_started());
        assert_eq!(
            ctrl.current_track().map(|t| t.id),
            Some(songs[0].id.clone())
        );

        let mut playing = false;
        for _ in 0..80 {
            if ctrl.telemetry().is_playing {
                playing = true;
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        assert!(playing, "controller must start playing track 0");

        ctrl.next().expect("next track");
        assert_eq!(
            ctrl.current_track().map(|t| t.id),
            Some(songs[1].id.clone())
        );
        ctrl.prev().expect("prev track");
        assert_eq!(
            ctrl.current_track().map(|t| t.id),
            Some(songs[0].id.clone())
        );

        let events = sink.now_playing.lock().unwrap().clone();
        assert!(events.contains(&Some(songs[0].id.clone())));
        assert!(events.contains(&Some(songs[1].id.clone())));

        ctrl.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_records_qualified_play_history_and_persists_session_on_shutdown() {
        let (root, cfg) = temp_cfg("persist");
        let sink = Arc::new(RecordingSink::default());
        let ctrl = FerricController::new(
            cfg.clone(),
            Arc::clone(&sink) as Arc<dyn ControllerEventSink>,
        )
        .expect("ctrl");

        assert!(
            !ctrl
                .record_qualified_play_for_test("t1", "Bruno Mars", 12_000, 180_000)
                .expect("short play rejected")
        );
        assert!(
            ctrl.record_qualified_play_for_test("t1", "Bruno Mars", 45_000, 180_000)
                .expect("qualified play recorded")
        );

        let stats = ctrl.get_listening_stats(0).expect("stats");
        assert_eq!(stats.total_plays, 1);
        assert_eq!(stats.current_streak_days, 1);

        let mut theme = ThemeConfig::liquid_glass();
        theme.editorial_mode = true;
        ctrl.set_theme(theme).expect("save theme");
        ctrl.set_volume(0.65);
        ctrl.shutdown();

        // Reopen on the same directory and verify persisted theme & stats survive restart.
        let ctrl2 = FerricController::new(cfg, sink).expect("reopen ctrl");
        assert_eq!(ctrl2.get_theme().player_layout, "full_bleed");
        assert!(ctrl2.get_theme().editorial_mode);
        assert_eq!(ctrl2.get_listening_stats(0).expect("stats2").total_plays, 1);
        ctrl2.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn controller_scans_local_entries_searches_groups_and_manages_playlists_and_queue() {
        use crate::types::LocalScanEntry;
        use ferric_core::collections::{DetailSortOrder, PlaylistSortOrder, PlaylistSourceFilter};

        let (root, cfg) = temp_cfg("m2_lib");
        let sink = Arc::new(RecordingSink::default());
        let ctrl = FerricController::new(cfg, Arc::clone(&sink) as Arc<dyn ControllerEventSink>)
            .expect("ctrl");

        let scanned = ctrl
            .scan_local_entries(vec![
                LocalScanEntry {
                    uri: "content://media/external/audio/media/101".to_string(),
                    title: "Midnight Tape".to_string(),
                    artist: "Lively Studio".to_string(),
                    album: "Analog Nights".to_string(),
                    duration_ms: 210_000,
                    codec: "FLAC".to_string(),
                    sample_rate: 96_000,
                    bit_depth: 24,
                    bitrate_kbps: 2400,
                },
                LocalScanEntry {
                    uri: "content://media/external/audio/media/102".to_string(),
                    title: "Dawn Chorus".to_string(),
                    artist: "Lively Studio".to_string(),
                    album: "Analog Nights".to_string(),
                    duration_ms: 195_000,
                    codec: "FLAC".to_string(),
                    sample_rate: 48_000,
                    bit_depth: 24,
                    bitrate_kbps: 1500,
                },
            ])
            .expect("scan local");
        assert_eq!(scanned, 2);
        assert_eq!(ctrl.list_local_songs().expect("local").len(), 2);
        assert_eq!(ctrl.list_albums().expect("albums").len(), 1);
        assert_eq!(ctrl.list_artists().expect("artists").len(), 1);

        let hits = ctrl
            .instant_search_local("midnight lively")
            .expect("instant search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Midnight Tape");

        let liked_now = ctrl.toggle_like(&hits[0].id).expect("like");
        assert!(liked_now);
        assert_eq!(ctrl.list_liked_songs().expect("liked").len(), 1);

        let all_songs = ctrl.list_songs("").expect("songs");
        let pl = ctrl
            .create_playlist(
                "Studio Mix".to_string(),
                "Hi-Res".to_string(),
                vec![
                    all_songs[0].id.clone(),
                    all_songs[1].id.clone(),
                    all_songs[0].id.clone(),
                ],
            )
            .expect("create pl");
        let removed_dups = ctrl.deduplicate_playlist(pl.id.clone()).expect("dedup");
        assert_eq!(removed_dups, 1);

        let detail = ctrl
            .get_playlist_detail(&pl.id, DetailSortOrder::Title)
            .expect("detail");
        assert_eq!(detail.tracks.len(), 2);
        assert_eq!(detail.tracks[0].title, "Dawn Chorus");

        let csv = ctrl.format_share_csv(&detail.tracks);
        let imported = ctrl
            .import_playlist_text("CSV Copy".to_string(), csv)
            .expect("import csv");
        assert_eq!(imported.track_count, 2);

        let filtered = ctrl
            .list_playlists_filtered(PlaylistSourceFilter::Local, PlaylistSortOrder::NameAZ)
            .expect("filtered");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "CSV Copy");

        // Queue editing: play_next, move_up/down, remove, save, copy.
        ctrl.play_next(all_songs[0].clone());
        ctrl.append_to_queue(all_songs[1].clone());
        let saved_q = ctrl
            .save_queue_as_playlist("Saved Q".to_string())
            .expect("save queue");
        assert_eq!(saved_q.track_count, 2);
        assert!(ctrl.copy_queue_tracklist().contains("Ferric Queue"));
        ctrl.clear_up_next();
        ctrl.play_next(all_songs[0].clone());
        ctrl.stop_playback();
        assert!(ctrl.current_track().is_none());
        assert!(ctrl.up_next(10).is_empty());

        ctrl.shutdown();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn spotify_up_next_sequencer_enforces_cooldown_variants_and_mix() {
        let mk = |id: &str, title: &str, artist: &str, album: &str, liked: bool| Track {
            id: id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_ms: 210_000,
            path: None,
            source: TrackSource::YouTubeMusic,
            codec: "OPUS".to_string(),
            sample_rate: 48_000,
            bit_depth: 16,
            bitrate_kbps: 128,
            isrc: None,
            spotify_id: None,
            youtube_video_id: Some(id.to_string()),
            thumbnail_url: None,
            replay_gain_db: 0,
            liked,
        };

        let radio_pool = vec![
            mk("r0", "[Playlist] Long Mix 1 Hour", "Spammer", "Mix", false),
            mk(
                "r1",
                "Glimpse of Us (Official Video)",
                "Joji",
                "Smithereens",
                false,
            ),
            mk("r2", "Sanctuary", "Joji", "Nectar", false),
            mk("r3", "Slow Dancing in the Dark", "Joji", "BALLADS 1", false),
            mk("r4", "Attention", "Charlie Puth", "Voicenotes", false),
            mk(
                "r5",
                "Espresso",
                "Sabrina Carpenter",
                "Short n' Sweet",
                false,
            ),
            mk("r6", "Kill Bill", "SZA", "SOS", false),
            mk("r7", "Die For You", "The Weeknd", "Starboy", false),
        ];
        let lib_pool = vec![
            mk("l1", "Back to Friends", "sombr", "Singles", true),
            mk("l2", "Glimpse of Us", "Joji", "Smithereens", true),
        ];

        let existing_ids: HashSet<String> = ["cur"].into_iter().map(String::from).collect();
        let existing_cores = vec!["glimpse of us".to_string()];
        let tail_artists = vec!["joji".to_string()];
        let top_artists: HashSet<String> = ["sombr".to_string()].into_iter().collect();

        let seq = sequence_spotify_up_next(
            &[radio_pool],
            lib_pool,
            MusicLocale::GlobalEn,
            "Glimpse of Us",
            &existing_ids,
            &existing_cores,
            &tail_artists,
            "",
            &top_artists,
            10,
        );

        assert!(!seq.iter().any(|t| t.title.contains("[Playlist]")));
        assert!(!seq.iter().any(|t| t.id == "r1" || t.id == "l2"));
        assert!(seq.iter().any(|t| t.id == "l1"));
        for w in seq.windows(2) {
            assert_ne!(
                primary_artist_key(&w[0].artist),
                primary_artist_key(&w[1].artist),
                "consecutive tracks must enforce artist cooldown: {} vs {}",
                w[0].artist,
                w[1].artist
            );
        }
    }
}
