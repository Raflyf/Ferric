//! Zero-Pairing Ferric Connect bridge for [`crate::FerricController`].

use crate::FerricController;
use ferric_audio::engine::EngineCommand;
use ferric_core::models::{Track, TrackSource};
use ferric_sync::{
    ConnectConfig, ConnectEngine, ConnectEvent, ConnectSnapshot, DeviceKind, LocalPlaybackSnapshot,
    QrMatrix, QueueEntry, RemoteAction, SessionState,
};
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_now_utc_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl FerricController {
    pub(crate) fn start_connect_engine(self: &Arc<Self>) {
        if !self.cfg.enable_connect {
            return;
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        let cfg = ConnectConfig {
            device_id: self.cfg.device_id.clone(),
            device_name: self.cfg.device_name.clone(),
            device_kind: self.cfg.device_kind,
            bind_ephemeral_udp: self.cfg.headless_audio,
        };
        let on_event = Arc::new(move |ev: ConnectEvent| {
            let Some(ctrl) = weak.upgrade() else {
                return;
            };
            ctrl.handle_connect_event(ev);
        });
        if let Ok(engine) = ConnectEngine::start(cfg, on_event) {
            let engine = Arc::new(engine);
            {
                let mut slot = self.connect.lock().unwrap_or_else(|p| p.into_inner());
                *slot = Some(Arc::clone(&engine));
            }
            self.sync_connect_accounts();
            self.push_connect_playback_snapshot();
        }
    }

    pub(crate) fn sync_connect_accounts(&self) {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        let Some(engine) = engine else {
            return;
        };
        let (sp_id, yt_id) = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let sp = if st.accounts.spotify_connected || st.accounts.spotify_web_connected {
                st.accounts.spotify_user_id.clone()
            } else {
                String::new()
            };
            let yt = if st.accounts.youtube_connected {
                st.accounts.youtube_account_id.clone()
            } else {
                String::new()
            };
            (sp, yt)
        };
        engine.update_accounts(sp_id, yt_id);
    }

    pub(crate) fn push_connect_playback_snapshot(&self) {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        let Some(engine) = engine else {
            return;
        };
        let tele = self.telemetry();
        let snap = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let current_track = st.queue.current().map(QueueEntry::from_track);
            let queue: Vec<QueueEntry> = st
                .queue
                .tracks()
                .iter()
                .take(200)
                .map(QueueEntry::from_track)
                .collect();
            let current_index = current_track
                .as_ref()
                .and_then(|cur| queue.iter().position(|t| t.track_id == cur.track_id))
                .unwrap_or(0);
            LocalPlaybackSnapshot {
                is_playing: tele.is_playing,
                current_track,
                queue,
                current_index,
                position_ms: tele.position_ms,
                volume_norm: st.session.volume,
                repeat_mode: st.queue.repeat,
                shuffle_enabled: st.queue.shuffle,
                anchor_utc_ms: unix_now_utc_ms(),
            }
        };
        engine.update_local_playback(snap);
    }

    /// Returns `true` when another Ferric device is currently the active speaker and connected.
    pub(crate) fn is_controlling_remote(&self) -> bool {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        engine.is_some_and(|e| {
            let snap = e.snapshot();
            !snap.is_local_active_speaker && snap.peers.iter().any(|p| p.connected)
        })
    }

    pub(crate) fn forward_remote_action(&self, action: RemoteAction) -> bool {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        if let Some(e) = engine {
            e.send_remote_action(action);
            return true;
        }
        false
    }

    #[must_use]
    pub fn get_connect_state(&self) -> ConnectSnapshot {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        if let Some(e) = engine {
            e.snapshot()
        } else {
            ConnectSnapshot {
                local_device_id: self.cfg.device_id.clone(),
                local_device_name: self.cfg.device_name.clone(),
                local_device_kind: self.cfg.device_kind,
                local_ip: "127.0.0.1".to_string(),
                local_tcp_port: 0,
                room_tags_hex: ",".to_string(),
                active_speaker_id: self.cfg.device_id.clone(),
                is_local_active_speaker: true,
                is_account_ready: false,
                relay_connected: false,
                peers: Vec::new(),
                active_queue: Vec::new(),
                active_queue_index: 0,
            }
        }
    }

    pub fn notify_mdns_peer(
        &self,
        device_id: String,
        device_name: String,
        device_kind: DeviceKind,
        ip: String,
        tcp_port: u16,
        room_tags_hex: String,
    ) {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        if let Some(e) = engine {
            e.notify_mdns_peer(
                device_id,
                device_name,
                device_kind,
                &ip,
                tcp_port,
                &room_tags_hex,
            );
        }
    }

    pub fn connect_switch_speaker(&self, target_device_id: String) {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        if let Some(e) = engine {
            e.transfer_active_speaker(target_device_id);
        }
    }

    pub fn connect_set_remote_volume(&self, target_device_id: String, volume_norm: f32) {
        let clamped = if volume_norm.is_finite() {
            volume_norm.clamp(0.0, 1.5)
        } else {
            1.0
        };
        if target_device_id == self.cfg.device_id {
            self.set_volume(clamped);
            return;
        }
        let _ = self.forward_remote_action(RemoteAction::SetVolume {
            target_device: target_device_id,
            volume_norm: clamped,
        });
    }

    pub fn connect_remote_action(&self, action: RemoteAction) {
        let _ = self.forward_remote_action(action);
    }

    pub fn connect_pair_qr(&self, uri: String) {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        if let Some(e) = engine {
            e.pair_with_qr_uri(uri);
        }
    }

    #[must_use]
    pub fn connect_get_qr_uri(&self) -> String {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        engine.map(|e| e.qr_pairing_uri()).unwrap_or_default()
    }

    #[must_use]
    pub fn connect_regenerate_qr_uri(&self) -> String {
        let engine = {
            self.connect
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
        };
        engine.map(|e| e.regenerate_qr_uri()).unwrap_or_default()
    }

    #[must_use]
    pub fn connect_qr_matrix(&self, regenerate: bool) -> (String, usize, Vec<bool>) {
        let uri = if regenerate {
            self.connect_regenerate_qr_uri()
        } else {
            self.connect_get_qr_uri()
        };
        match QrMatrix::encode(&uri) {
            Ok(m) => (uri, m.size, m.modules),
            Err(_) => (uri, 0, Vec::new()),
        }
    }

    fn handle_connect_event(self: &Arc<Self>, ev: ConnectEvent) {
        match ev {
            ConnectEvent::StateUpdated(snap) => {
                self.sink.on_connect_state_changed(snap);
            }
            ConnectEvent::ExecuteRemoteAction(action) => {
                self.apply_incoming_remote_action(action);
            }
            ConnectEvent::BecameActiveSpeaker {
                state,
                queue,
                current_index,
            } => {
                self.apply_became_active_speaker(state, queue, current_index);
            }
            ConnectEvent::YieldedActiveSpeaker {
                new_speaker_name, ..
            } => {
                self.send_audio(EngineCommand::Pause);
                self.sink.on_toast(format!("Playing on {new_speaker_name}"));
            }
            ConnectEvent::Toast(msg) => {
                self.sink.on_toast(msg);
            }
        }
    }

    fn apply_incoming_remote_action(self: &Arc<Self>, action: RemoteAction) {
        match action {
            RemoteAction::Play => {
                let _ = self.play_local_only();
            }
            RemoteAction::Pause => {
                self.send_audio(EngineCommand::Pause);
                self.push_connect_playback_snapshot();
            }
            RemoteAction::TogglePlayPause => {
                let _ = self.toggle_play_pause_local_only();
            }
            RemoteAction::SeekTo { position_ms } => {
                self.send_audio(EngineCommand::SeekMs(position_ms));
                self.push_connect_playback_snapshot();
            }
            RemoteAction::SkipNext => {
                let _ = self.next_local_only();
            }
            RemoteAction::SkipPrev => {
                let _ = self.prev_local_only();
            }
            RemoteAction::JumpToQueueIndex { index } => {
                let (tracks, ctx) = {
                    let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    (st.queue.tracks().to_vec(), st.queue_context.clone())
                };
                if !tracks.is_empty() {
                    let clamped = index.min(tracks.len() - 1);
                    let _ = self.play_queue_local_only(tracks, clamped, ctx);
                }
            }
            RemoteAction::PlayQueue {
                tracks,
                start_index,
                context,
            } => {
                let hydrated = self.hydrate_connect_tracks(tracks);
                if !hydrated.is_empty() {
                    let idx = start_index.min(hydrated.len() - 1);
                    let _ = self.play_queue_local_only(hydrated, idx, context);
                }
            }
            RemoteAction::SetVolume { volume_norm, .. } => {
                self.set_volume(volume_norm);
            }
            RemoteAction::SetRepeat(mode) => {
                let up_next = {
                    let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.queue.repeat = mode;
                    st.session.repeat = mode;
                    st.queue.peek_upcoming(250).into_iter().cloned().collect()
                };
                self.queue_upcoming_deck_b();
                self.sink.on_queue_changed(up_next);
                self.push_connect_playback_snapshot();
            }
            RemoteAction::SetShuffle(enabled) => {
                let up_next = {
                    let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    st.queue.set_shuffle(enabled);
                    st.session.shuffle = enabled;
                    st.queue.peek_upcoming(250).into_iter().cloned().collect()
                };
                self.queue_upcoming_deck_b();
                self.sink.on_queue_changed(up_next);
                self.push_connect_playback_snapshot();
            }
            RemoteAction::TransferActiveSpeaker { .. } | RemoteAction::SetSimulcastMode { .. } => {}
        }
    }

    fn apply_became_active_speaker(
        self: &Arc<Self>,
        state: SessionState,
        queue: Vec<QueueEntry>,
        current_index: usize,
    ) {
        let mut tracks = self.hydrate_connect_tracks(queue);
        if tracks.is_empty()
            && let Some(tid) = state.track_id.clone()
        {
            tracks.push(
                QueueEntry {
                    track_id: tid,
                    title: state.track_title.clone(),
                    artist: state.track_artist.clone(),
                    album: String::new(),
                    duration_ms: state.duration_ms,
                    spotify_id: None,
                    youtube_video_id: None,
                    thumbnail_url: state.track_thumbnail_url.clone(),
                }
                .into_track(),
            );
        }
        if tracks.is_empty() {
            self.sink
                .on_toast("Switched active output to this device".to_string());
            return;
        }
        let idx = current_index.min(tracks.len() - 1);
        let target_track = &tracks[idx];
        let is_unmatched_local = target_track.source == TrackSource::Local
            && target_track.path.is_none()
            && target_track.spotify_id.is_none()
            && target_track.youtube_video_id.is_none();

        let now_ms = unix_now_utc_ms();
        let drift_ms = if state.is_playing && state.anchor_utc_ms > 0 {
            u64::try_from((now_ms - state.anchor_utc_ms).clamp(0, 5_000)).unwrap_or(0)
        } else {
            0
        };
        let seek_target_ms = state.base_position_ms.saturating_add(drift_ms);

        {
            let mut st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.queue.repeat = state.repeat_mode;
            st.queue.set_shuffle(state.shuffle_enabled);
            st.pending_handoff_seek_ms = Some(seek_target_ms);
        }

        if is_unmatched_local {
            // v0.1 policy: attempt online title+artist match via standard resolver; warn if strictly local.
            let sender_name = if state.device_name.is_empty() {
                "other device".to_string()
            } else {
                state.device_name
            };
            self.sink
                .on_toast(format!("Matching local track from {sender_name} online..."));
        }

        if self
            .play_queue_local_with_mode(
                tracks,
                idx,
                "Ferric Connect".to_string(),
                !state.is_playing,
            )
            .is_ok()
        {
            if seek_target_ms > 1_000 {
                self.send_audio(EngineCommand::SeekMs(seek_target_ms));
            }
            if !state.is_playing {
                self.send_audio(EngineCommand::Pause);
            }
            self.sink
                .on_toast("Switched playback to this device".to_string());
        }
    }

    fn hydrate_connect_tracks(&self, entries: Vec<QueueEntry>) -> Vec<Track> {
        let local_map: HashMap<String, Track> = {
            let st = self.state.lock().unwrap_or_else(|p| p.into_inner());
            st.db
                .list_tracks("")
                .unwrap_or_default()
                .into_iter()
                .map(|t| (t.id.clone(), t))
                .collect()
        };
        entries
            .into_iter()
            .map(|e| {
                if let Some(existing) = local_map.get(&e.track_id) {
                    existing.clone()
                } else {
                    e.into_track()
                }
            })
            .collect()
    }
}
