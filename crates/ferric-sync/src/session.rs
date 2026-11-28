//! Authenticated, ChaCha20-Poly1305 AEAD encrypted Zero-Pairing Ferric Connect LAN runtime
//! shared identically by Ferric Android (`ferric-controller`) and Ferric Windows Desktop.

use crate::crypto::{
    AeadChannel, AeadReceiver, AeadSender, LocalRoomKeys, MatchedRoomSlot, compute_handshake_mac,
    ct_eq, decode_room_tags_hex, derive_qr_room, encode_room_tags_hex,
};
use crate::lan::{
    DiscoveredPeer, LanDiscoveryBeacon, detect_local_lan_ip, is_lan_or_loopback,
    sanitize_device_name,
};
use crate::{
    DeviceKind, MAX_FRAME_BYTES, PairingPayload, QueueEntry, RemoteAction, SessionState,
    SpeakerAuthority, SyncError, SyncFrame, SyncResult, decode_frame_payload, encode_frame_payload,
    resolve_active_speaker_tie_break,
};
use ferric_core::models::RepeatMode;
use ferric_core::secrets::{csprng_fill, csprng_hex};
use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HANDSHAKE_MAGIC: &[u8; 4] = b"FCH1";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
const BEACON_INTERVAL: Duration = Duration::from_secs(3);
const RECONNECT_BACKOFF: Duration = Duration::from_secs(4);
const MAX_CONNECTED_PEERS: usize = 8;
const MAX_HANDSHAKE_ID_LEN: usize = 64;

#[derive(Debug, Clone)]
pub struct ConnectConfig {
    pub device_id: String,
    pub device_name: String,
    pub device_kind: DeviceKind,
    /// When true (tests), bind UDP discovery to an ephemeral port instead of `43821`.
    pub bind_ephemeral_udp: bool,
}

#[derive(Debug, Clone)]
pub struct LocalPlaybackSnapshot {
    pub is_playing: bool,
    pub current_track: Option<QueueEntry>,
    pub queue: Vec<QueueEntry>,
    pub current_index: usize,
    pub position_ms: u64,
    pub volume_norm: f32,
    pub repeat_mode: RepeatMode,
    pub shuffle_enabled: bool,
    pub anchor_utc_ms: i64,
}

impl Default for LocalPlaybackSnapshot {
    fn default() -> Self {
        Self {
            is_playing: false,
            current_track: None,
            queue: Vec::new(),
            current_index: 0,
            position_ms: 0,
            volume_norm: 1.0,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            anchor_utc_ms: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectPeerInfo {
    pub device_id: String,
    pub device_name: String,
    pub device_kind: DeviceKind,
    pub addr: String,
    pub connected: bool,
    pub is_active_speaker: bool,
    pub is_playing: bool,
    pub volume_norm: f32,
    pub now_playing_title: String,
    pub now_playing_artist: String,
    pub now_playing_thumbnail_url: Option<String>,
    pub position_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectSnapshot {
    pub local_device_id: String,
    pub local_device_name: String,
    pub local_device_kind: DeviceKind,
    pub local_ip: String,
    pub local_tcp_port: u16,
    pub room_tags_hex: String,
    pub active_speaker_id: String,
    pub is_local_active_speaker: bool,
    pub is_account_ready: bool,
    pub peers: Vec<ConnectPeerInfo>,
}

#[derive(Debug, Clone)]
pub enum ConnectEvent {
    StateUpdated(ConnectSnapshot),
    ExecuteRemoteAction(RemoteAction),
    BecameActiveSpeaker {
        state: SessionState,
        queue: Vec<QueueEntry>,
        current_index: usize,
    },
    YieldedActiveSpeaker {
        new_speaker_id: String,
        new_speaker_name: String,
    },
    Toast(String),
}

enum EngineCommand {
    UpdateAccounts {
        spotify_user_id: String,
        youtube_account_id: String,
    },
    UpdateLocalPlayback(Box<LocalPlaybackSnapshot>),
    IngestMdnsPeer {
        device_id: String,
        device_name: String,
        device_kind: DeviceKind,
        ip: IpAddr,
        tcp_port: u16,
        room_tags: [Option<[u8; 16]>; 2],
    },
    TransferActiveSpeaker {
        target_device_id: String,
    },
    SendRemoteAction(RemoteAction),
    PairWithQrUri {
        uri: String,
    },
    RegenerateQrToken {
        reply: mpsc::Sender<String>,
    },
    Shutdown,
}

enum InternalMsg {
    IncomingTcp(TcpStream, SocketAddr),
    PeerEstablished {
        device_id: String,
        device_name: String,
        device_kind: DeviceKind,
        addr: SocketAddr,
        writer: Arc<Mutex<PeerWriter>>,
    },
    PeerFrame {
        device_id: String,
        frame: SyncFrame,
    },
    PeerClosed {
        device_id: String,
    },
}

struct PeerWriter {
    stream: TcpStream,
    tx: AeadSender,
}

impl PeerWriter {
    fn send_frame(&mut self, frame: &SyncFrame) -> SyncResult<()> {
        let plain = encode_frame_payload(frame)?;
        let ct = self.tx.seal(&plain)?;
        let len = u32::try_from(ct.len()).map_err(|_| SyncError::FrameTooLarge(ct.len()))?;
        self.stream.write_all(&len.to_le_bytes())?;
        self.stream.write_all(&ct)?;
        self.stream.flush()?;
        Ok(())
    }
}

struct ConnectedPeer {
    device_id: String,
    device_name: String,
    device_kind: DeviceKind,
    addr: SocketAddr,
    writer: Arc<Mutex<PeerWriter>>,
    last_state: Option<SessionState>,
    last_queue: Vec<QueueEntry>,
    last_queue_index: usize,
}

/// Shared Zero-Pairing Ferric Connect background engine.
pub struct ConnectEngine {
    local_device_id: String,
    local_device_name: String,
    local_tcp_port: u16,
    local_udp_port: u16,
    cmd_tx: mpsc::Sender<EngineCommand>,
    latest_snapshot: Arc<Mutex<ConnectSnapshot>>,
    qr_uri: Arc<Mutex<String>>,
    shutdown_flag: Arc<AtomicBool>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn unix_now_utc_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ConnectEngine {
    /// Start the background Connect engine with an event callback invoked whenever peers,
    /// active speaker status, or incoming remote commands arrive.
    pub fn start(
        cfg: ConnectConfig,
        on_event: Arc<dyn Fn(ConnectEvent) + Send + Sync>,
    ) -> SyncResult<Self> {
        let listener = TcpListener::bind(("0.0.0.0", 0))?;
        listener.set_nonblocking(true)?;
        let local_tcp_port = listener.local_addr()?.port();

        let beacon = if cfg.bind_ephemeral_udp {
            LanDiscoveryBeacon::bind_nonblocking(&cfg.device_id, 0)?
        } else {
            LanDiscoveryBeacon::bind_default(&cfg.device_id)?
        };
        let local_udp_port = beacon.local_port();
        let local_ip = detect_local_lan_ip();
        let clean_name = sanitize_device_name(&cfg.device_name);
        let initial_token = csprng_hex(16);
        let initial_qr_payload = PairingPayload {
            endpoint_id: cfg.device_id.clone(),
            token_hex: initial_token.clone(),
            device_name: clean_name.clone(),
        };
        let initial_qr_uri =
            initial_qr_payload.to_uri_with_addr(Some(&format!("{local_ip}:{local_tcp_port}")));

        let initial_snapshot = ConnectSnapshot {
            local_device_id: cfg.device_id.clone(),
            local_device_name: clean_name.clone(),
            local_device_kind: cfg.device_kind,
            local_ip: local_ip.clone(),
            local_tcp_port,
            room_tags_hex: encode_room_tags_hex(&[None, None]),
            active_speaker_id: cfg.device_id.clone(),
            is_local_active_speaker: true,
            is_account_ready: false,
            peers: Vec::new(),
        };

        let latest_snapshot = Arc::new(Mutex::new(initial_snapshot));
        let qr_uri = Arc::new(Mutex::new(initial_qr_uri));
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (int_tx, int_rx) = mpsc::channel();

        let accept_tx = int_tx.clone();
        let accept_shutdown = Arc::clone(&shutdown_flag);
        let _ = thread::Builder::new()
            .name("ferric-conn-acc".to_string())
            .spawn(move || {
                while !accept_shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, addr)) => {
                            if is_lan_or_loopback(addr.ip()) {
                                let _ = accept_tx.send(InternalMsg::IncomingTcp(stream, addr));
                            }
                        }
                        Err(err) if err.kind() == ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(60));
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(120));
                        }
                    }
                }
            });

        let state_snap_arc = Arc::clone(&latest_snapshot);
        let qr_uri_arc = Arc::clone(&qr_uri);
        let worker_shutdown = Arc::clone(&shutdown_flag);
        let dev_id = cfg.device_id.clone();
        let dev_name = clean_name.clone();
        let dev_kind = cfg.device_kind;

        let handle = thread::Builder::new()
            .name("ferric-connect".to_string())
            .spawn(move || {
                let mut runner = EngineRunner {
                    local_device_id: dev_id.clone(),
                    local_device_name: dev_name,
                    local_device_kind: dev_kind,
                    local_ip,
                    local_tcp_port,
                    keys: LocalRoomKeys {
                        spotify: None,
                        ytm: None,
                        guest: derive_qr_room(&initial_token),
                    },
                    qr_token_hex: initial_token,
                    beacon,
                    local_playback: LocalPlaybackSnapshot::default(),
                    active_speaker_id: dev_id,
                    revision: 1,
                    cmd_seq: 1,
                    discovered: HashMap::new(),
                    connected: HashMap::new(),
                    last_dial_attempt: HashMap::new(),
                    last_beacon_at: None,
                    int_tx,
                    on_event,
                    latest_snapshot: state_snap_arc,
                    qr_uri: qr_uri_arc,
                };
                runner.run(cmd_rx, int_rx, worker_shutdown);
            })?;

        Ok(Self {
            local_device_id: cfg.device_id,
            local_device_name: clean_name,
            local_tcp_port,
            local_udp_port,
            cmd_tx,
            latest_snapshot,
            qr_uri,
            shutdown_flag,
            worker: Mutex::new(Some(handle)),
        })
    }

    #[must_use]
    pub fn local_device_id(&self) -> &str {
        &self.local_device_id
    }

    #[must_use]
    pub fn local_device_name(&self) -> &str {
        &self.local_device_name
    }

    #[must_use]
    pub fn local_tcp_port(&self) -> u16 {
        self.local_tcp_port
    }

    #[must_use]
    pub fn local_udp_port(&self) -> u16 {
        self.local_udp_port
    }

    #[must_use]
    pub fn snapshot(&self) -> ConnectSnapshot {
        self.latest_snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    #[must_use]
    pub fn qr_pairing_uri(&self) -> String {
        self.qr_uri
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn update_accounts(&self, spotify_user_id: String, youtube_account_id: String) {
        let _ = self.cmd_tx.send(EngineCommand::UpdateAccounts {
            spotify_user_id,
            youtube_account_id,
        });
    }

    pub fn update_local_playback(&self, snap: LocalPlaybackSnapshot) {
        let _ = self
            .cmd_tx
            .send(EngineCommand::UpdateLocalPlayback(Box::new(snap)));
    }

    /// Feed a peer discovered via Android `NsdManager` / OS mDNS (`_ferric-connect._udp.local.`).
    pub fn notify_mdns_peer(
        &self,
        device_id: String,
        device_name: String,
        device_kind: DeviceKind,
        ip_str: &str,
        tcp_port: u16,
        room_tags_hex: &str,
    ) {
        let Ok(ip) = ip_str.trim().parse::<IpAddr>() else {
            return;
        };
        if !is_lan_or_loopback(ip) || tcp_port < 1024 {
            return;
        }
        let room_tags = decode_room_tags_hex(room_tags_hex);
        let _ = self.cmd_tx.send(EngineCommand::IngestMdnsPeer {
            device_id,
            device_name,
            device_kind,
            ip,
            tcp_port,
            room_tags,
        });
    }

    pub fn transfer_active_speaker(&self, target_device_id: String) {
        let _ = self
            .cmd_tx
            .send(EngineCommand::TransferActiveSpeaker { target_device_id });
    }

    pub fn send_remote_action(&self, action: RemoteAction) {
        let _ = self.cmd_tx.send(EngineCommand::SendRemoteAction(action));
    }

    pub fn pair_with_qr_uri(&self, uri: String) {
        let _ = self.cmd_tx.send(EngineCommand::PairWithQrUri { uri });
    }

    pub fn regenerate_qr_uri(&self) -> String {
        let (tx, rx) = mpsc::channel();
        if self
            .cmd_tx
            .send(EngineCommand::RegenerateQrToken { reply: tx })
            .is_ok()
            && let Ok(uri) = rx.recv_timeout(Duration::from_secs(2))
        {
            return uri;
        }
        self.qr_pairing_uri()
    }

    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
        if let Some(handle) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ConnectEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct EngineRunner {
    local_device_id: String,
    local_device_name: String,
    local_device_kind: DeviceKind,
    local_ip: String,
    local_tcp_port: u16,
    keys: LocalRoomKeys,
    qr_token_hex: String,
    beacon: LanDiscoveryBeacon,
    local_playback: LocalPlaybackSnapshot,
    active_speaker_id: String,
    revision: u64,
    cmd_seq: u64,
    discovered: HashMap<String, DiscoveredPeer>,
    connected: HashMap<String, ConnectedPeer>,
    last_dial_attempt: HashMap<String, Instant>,
    last_beacon_at: Option<Instant>,
    int_tx: mpsc::Sender<InternalMsg>,
    on_event: Arc<dyn Fn(ConnectEvent) + Send + Sync>,
    latest_snapshot: Arc<Mutex<ConnectSnapshot>>,
    qr_uri: Arc<Mutex<String>>,
}

impl EngineRunner {
    fn run(
        &mut self,
        cmd_rx: mpsc::Receiver<EngineCommand>,
        int_rx: mpsc::Receiver<InternalMsg>,
        shutdown: Arc<AtomicBool>,
    ) {
        self.publish_snapshot();

        while !shutdown.load(Ordering::Relaxed) {
            let mut dirty = false;

            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    EngineCommand::Shutdown => return,
                    EngineCommand::UpdateAccounts {
                        spotify_user_id,
                        youtube_account_id,
                    } => {
                        let new_keys = LocalRoomKeys {
                            spotify: crate::crypto::derive_spotify_room(&spotify_user_id),
                            ytm: crate::crypto::derive_ytm_room(&youtube_account_id),
                            guest: self.keys.guest.clone(),
                        };
                        let changed = new_keys.advertised_tags() != self.keys.advertised_tags();
                        self.keys = new_keys;
                        if changed {
                            // Revoke existing sessions whose room keys no longer match the updated account state.
                            for peer in self.connected.values() {
                                if let Ok(w) = peer.writer.lock() {
                                    let _ = w.stream.shutdown(std::net::Shutdown::Both);
                                }
                            }
                            self.connected.clear();
                            if self.active_speaker_id != self.local_device_id {
                                self.active_speaker_id = self.local_device_id.clone();
                            }
                            self.last_beacon_at = None;
                            dirty = true;
                        }
                    }
                    EngineCommand::UpdateLocalPlayback(snap) => {
                        let was_playing = self.local_playback.is_playing;
                        let prev_track = self
                            .local_playback
                            .current_track
                            .as_ref()
                            .map(|t| t.track_id.clone());
                        let next_track = snap.current_track.as_ref().map(|t| t.track_id.clone());
                        let queue_changed = self.local_playback.queue.len() != snap.queue.len()
                            || self.local_playback.current_index != snap.current_index
                            || prev_track != next_track;

                        self.local_playback = *snap;
                        // If local playback just started playing, claim active speaker authority.
                        if self.local_playback.is_playing
                            && !was_playing
                            && self.active_speaker_id != self.local_device_id
                        {
                            self.active_speaker_id = self.local_device_id.clone();
                            self.revision += 1;
                            dirty = true;
                        }
                        if self.active_speaker_id == self.local_device_id {
                            if queue_changed {
                                self.revision += 1;
                                self.broadcast_local_queue();
                            }
                            self.broadcast_local_state();
                        }
                    }
                    EngineCommand::IngestMdnsPeer {
                        device_id,
                        device_name,
                        device_kind,
                        ip,
                        tcp_port,
                        room_tags,
                    } => {
                        let now_ms = unix_now_ms();
                        if self.beacon.ingest_mdns_peer(
                            &device_id,
                            &device_name,
                            device_kind,
                            ip,
                            tcp_port,
                            room_tags,
                            now_ms,
                        ) {
                            self.poll_and_connect(now_ms);
                            dirty = true;
                        }
                    }
                    EngineCommand::TransferActiveSpeaker { target_device_id } => {
                        self.handle_local_transfer_request(target_device_id);
                        dirty = true;
                    }
                    EngineCommand::SendRemoteAction(action) => {
                        self.handle_local_remote_action(action);
                    }
                    EngineCommand::PairWithQrUri { uri } => {
                        self.handle_pair_qr_uri(&uri);
                        dirty = true;
                    }
                    EngineCommand::RegenerateQrToken { reply } => {
                        let tok = csprng_hex(16);
                        self.qr_token_hex = tok.clone();
                        self.keys.guest = derive_qr_room(&tok);
                        self.local_ip = detect_local_lan_ip();
                        let payload = PairingPayload {
                            endpoint_id: self.local_device_id.clone(),
                            token_hex: tok,
                            device_name: self.local_device_name.clone(),
                        };
                        let uri = payload.to_uri_with_addr(Some(&format!(
                            "{}:{}",
                            self.local_ip, self.local_tcp_port
                        )));
                        {
                            let mut slot = self.qr_uri.lock().unwrap_or_else(|p| p.into_inner());
                            *slot = uri.clone();
                        }
                        self.last_beacon_at = None;
                        let _ = reply.send(uri);
                        dirty = true;
                    }
                }
            }

            while let Ok(msg) = int_rx.try_recv() {
                match msg {
                    InternalMsg::IncomingTcp(stream, addr) => {
                        self.spawn_inbound_handshake(stream, addr);
                    }
                    InternalMsg::PeerEstablished {
                        device_id,
                        device_name,
                        device_kind,
                        addr,
                        writer,
                    } => {
                        self.on_peer_established(device_id, device_name, device_kind, addr, writer);
                        dirty = true;
                    }
                    InternalMsg::PeerFrame { device_id, frame } => {
                        if self.on_peer_frame(&device_id, frame) {
                            dirty = true;
                        }
                    }
                    InternalMsg::PeerClosed { device_id } => {
                        if self.connected.remove(&device_id).is_some() {
                            if self.active_speaker_id == device_id {
                                self.active_speaker_id = self.local_device_id.clone();
                            }
                            dirty = true;
                        }
                    }
                }
            }

            let now_ms = unix_now_ms();
            let should_beacon = self
                .last_beacon_at
                .is_none_or(|t| t.elapsed() >= BEACON_INTERVAL);
            if should_beacon {
                self.last_beacon_at = Some(Instant::now());
                let detected_ip = detect_local_lan_ip();
                if detected_ip != "127.0.0.1" && detected_ip != self.local_ip {
                    self.local_ip = detected_ip;
                    dirty = true;
                }
                if self.keys.has_any_room() {
                    let (title, artist) = self
                        .local_playback
                        .current_track
                        .as_ref()
                        .map(|t| (t.title.as_str(), t.artist.as_str()))
                        .unwrap_or(("", ""));
                    let _ = self.beacon.broadcast_announce_v2(
                        &self.local_device_name,
                        self.local_device_kind,
                        self.keys.advertised_tags(),
                        self.local_tcp_port,
                        self.active_speaker_id == self.local_device_id,
                        title,
                        artist,
                    );
                }
            }

            if self.poll_and_connect(now_ms) {
                dirty = true;
            }

            if dirty {
                self.publish_snapshot();
            }

            let sleep_dur = if self.connected.is_empty() {
                Duration::from_millis(120)
            } else {
                Duration::from_millis(50)
            };
            thread::sleep(sleep_dur);
        }
    }

    fn poll_and_connect(&mut self, now_ms: u64) -> bool {
        let peers = self.beacon.poll_peers(now_ms);
        let mut changed = false;
        let mut fresh_map = HashMap::with_capacity(peers.len());

        for p in peers {
            if let Some((slot, key)) = self.keys.match_remote_tags(&p.room_tags) {
                if !self.connected.contains_key(&p.device_id)
                    && self.connected.len() < MAX_CONNECTED_PEERS
                {
                    let should_dial = self.local_device_id < p.device_id
                        || self
                            .last_dial_attempt
                            .get(&p.device_id)
                            .is_some_and(|t| t.elapsed() >= RECONNECT_BACKOFF * 2);
                    let not_throttled = self
                        .last_dial_attempt
                        .get(&p.device_id)
                        .is_none_or(|t| t.elapsed() >= RECONNECT_BACKOFF);
                    if !should_dial && !self.last_dial_attempt.contains_key(&p.device_id) {
                        // Start grace timer so the lexicographically smaller device dials first.
                        self.last_dial_attempt
                            .insert(p.device_id.clone(), Instant::now());
                    }
                    if should_dial
                        && not_throttled
                        && let Ok(addr) = p.addr.parse::<SocketAddr>()
                        && is_lan_or_loopback(addr.ip())
                    {
                        self.last_dial_attempt
                            .insert(p.device_id.clone(), Instant::now());
                        self.spawn_outbound_handshake(addr, slot, key, p.device_id.clone());
                    }
                }
                fresh_map.insert(p.device_id.clone(), p);
            }
        }

        if fresh_map.len() != self.discovered.len()
            || fresh_map.keys().any(|k| !self.discovered.contains_key(k))
        {
            changed = true;
        }
        self.discovered = fresh_map;
        changed
    }

    fn spawn_outbound_handshake(
        &self,
        addr: SocketAddr,
        slot: MatchedRoomSlot,
        room_key: [u8; 32],
        expected_peer_id: String,
    ) {
        let local_id = self.local_device_id.clone();
        let local_name = self.local_device_name.clone();
        let local_kind = self.local_device_kind;
        let int_tx = self.int_tx.clone();
        let _ = thread::Builder::new()
            .name("ferric-conn-out".to_string())
            .spawn(move || {
                if let Ok((peer_id, peer_name, peer_kind, writer, rx_stream, aead_rx)) =
                    perform_outbound_handshake(
                        addr,
                        slot,
                        &room_key,
                        &local_id,
                        &local_name,
                        local_kind,
                    )
                {
                    if !expected_peer_id.is_empty() && peer_id != expected_peer_id {
                        return;
                    }
                    let writer_arc = Arc::new(Mutex::new(writer));
                    let _ = int_tx.send(InternalMsg::PeerEstablished {
                        device_id: peer_id.clone(),
                        device_name: peer_name,
                        device_kind: peer_kind,
                        addr,
                        writer: Arc::clone(&writer_arc),
                    });
                    run_peer_reader(peer_id, rx_stream, aead_rx, int_tx);
                }
            });
    }

    fn spawn_inbound_handshake(&self, stream: TcpStream, addr: SocketAddr) {
        if self.connected.len() >= MAX_CONNECTED_PEERS || !self.keys.has_any_room() {
            return;
        }
        let keys = self.keys.clone();
        let local_id = self.local_device_id.clone();
        let local_name = self.local_device_name.clone();
        let local_kind = self.local_device_kind;
        let int_tx = self.int_tx.clone();
        let _ = thread::Builder::new()
            .name("ferric-conn-in".to_string())
            .spawn(move || {
                if let Ok((peer_id, peer_name, peer_kind, writer, rx_stream, aead_rx)) =
                    perform_inbound_handshake(stream, &keys, &local_id, &local_name, local_kind)
                {
                    let writer_arc = Arc::new(Mutex::new(writer));
                    let _ = int_tx.send(InternalMsg::PeerEstablished {
                        device_id: peer_id.clone(),
                        device_name: peer_name,
                        device_kind: peer_kind,
                        addr,
                        writer: Arc::clone(&writer_arc),
                    });
                    run_peer_reader(peer_id, rx_stream, aead_rx, int_tx);
                }
            });
    }

    fn on_peer_established(
        &mut self,
        device_id: String,
        device_name: String,
        device_kind: DeviceKind,
        addr: SocketAddr,
        writer: Arc<Mutex<PeerWriter>>,
    ) {
        if device_id == self.local_device_id {
            return;
        }
        let clean_name = sanitize_device_name(&device_name);
        let peer = ConnectedPeer {
            device_id: device_id.clone(),
            device_name: clean_name.clone(),
            device_kind,
            addr,
            writer: Arc::clone(&writer),
            last_state: None,
            last_queue: Vec::new(),
            last_queue_index: 0,
        };
        self.connected.insert(device_id, peer);

        // Immediately sync our current playback snapshot & queue over the authenticated channel.
        let state_frame = SyncFrame::StateSnapshot(Box::new(self.build_local_session_state()));
        let queue_frame = SyncFrame::QueueFull {
            revision: self.revision,
            tracks: self.local_playback.queue.clone(),
            current_index: self.local_playback.current_index,
        };
        if let Ok(mut w) = writer.lock() {
            let _ = w.send_frame(&state_frame);
            let _ = w.send_frame(&queue_frame);
        }
        (self.on_event)(ConnectEvent::Toast(format!("Connected to {clean_name}")));
    }

    fn on_peer_frame(&mut self, peer_id: &str, frame: SyncFrame) -> bool {
        match frame {
            SyncFrame::Hello {
                device_name,
                device_kind,
                ..
            } => {
                if let Some(peer) = self.connected.get_mut(peer_id) {
                    peer.device_name = sanitize_device_name(&device_name);
                    peer.device_kind = device_kind;
                    return true;
                }
                false
            }
            SyncFrame::StateSnapshot(st) => {
                let mut changed = false;
                let remote_is_claiming_self = st.active_speaker.as_deref() == Some(peer_id);
                let remote_assigned_local =
                    st.active_speaker.as_deref() == Some(self.local_device_id.as_str());

                if let Some(peer) = self.connected.get_mut(peer_id) {
                    if !st.device_name.trim().is_empty() {
                        peer.device_name = sanitize_device_name(&st.device_name);
                    }
                    peer.last_state = Some((*st).clone());
                    changed = true;
                }

                if remote_assigned_local && self.active_speaker_id != self.local_device_id {
                    self.active_speaker_id = self.local_device_id.clone();
                    self.revision = self.revision.max(st.revision) + 1;
                    let (queue, idx) = self
                        .connected
                        .get(peer_id)
                        .map(|p| (p.last_queue.clone(), p.last_queue_index))
                        .unwrap_or_default();
                    (self.on_event)(ConnectEvent::BecameActiveSpeaker {
                        state: *st,
                        queue,
                        current_index: idx,
                    });
                    return true;
                }

                if remote_is_claiming_self && self.active_speaker_id == self.local_device_id {
                    // Initial connect or simultaneous play tie-break
                    let local_auth = SpeakerAuthority {
                        device_id: self.local_device_id.clone(),
                        is_playing: self.local_playback.is_playing,
                        revision: self.revision,
                        anchor_utc_ms: self.local_playback.anchor_utc_ms,
                    };
                    let remote_auth = SpeakerAuthority {
                        device_id: peer_id.to_string(),
                        is_playing: st.is_playing,
                        revision: st.revision,
                        anchor_utc_ms: st.anchor_utc_ms,
                    };
                    let winner = resolve_active_speaker_tie_break(&local_auth, &remote_auth);
                    if winner == peer_id {
                        self.active_speaker_id = peer_id.to_string();
                        self.revision = self.revision.max(st.revision);
                        let speaker_name = self
                            .connected
                            .get(peer_id)
                            .map(|p| p.device_name.clone())
                            .unwrap_or_else(|| "Remote Device".to_string());
                        if self.local_playback.is_playing {
                            (self.on_event)(ConnectEvent::YieldedActiveSpeaker {
                                new_speaker_id: peer_id.to_string(),
                                new_speaker_name: speaker_name,
                            });
                        }
                        changed = true;
                    }
                } else if let Some(active) = &st.active_speaker
                    && st.revision > self.revision
                    && active != &self.local_device_id
                {
                    self.active_speaker_id = active.clone();
                    self.revision = st.revision;
                    changed = true;
                }

                changed
            }
            SyncFrame::QueueFull {
                tracks,
                current_index,
                ..
            } => {
                if let Some(peer) = self.connected.get_mut(peer_id) {
                    peer.last_queue = tracks;
                    peer.last_queue_index = current_index;
                    return true;
                }
                false
            }
            SyncFrame::Command { action, .. } => match action {
                RemoteAction::TransferActiveSpeaker {
                    new_speaker,
                    start_playing,
                } => {
                    self.handle_incoming_transfer(peer_id, new_speaker, start_playing);
                    true
                }
                other => {
                    (self.on_event)(ConnectEvent::ExecuteRemoteAction(other));
                    false
                }
            },
            SyncFrame::Ping {
                client_send_mono_ms,
                ..
            } => {
                let now = unix_now_utc_ms();
                if let Some(peer) = self.connected.get(peer_id)
                    && let Ok(mut w) = peer.writer.lock()
                {
                    let _ = w.send_frame(&SyncFrame::Pong {
                        client_send_mono_ms,
                        server_recv_utc_ms: now,
                        server_send_utc_ms: now,
                    });
                }
                false
            }
            _ => false,
        }
    }

    fn handle_incoming_transfer(
        &mut self,
        from_peer_id: &str,
        new_speaker: String,
        start_playing: bool,
    ) {
        if new_speaker == self.local_device_id {
            let source_peer_id = if self.connected.contains_key(&self.active_speaker_id) {
                self.active_speaker_id.clone()
            } else {
                from_peer_id.to_string()
            };
            self.active_speaker_id = self.local_device_id.clone();
            self.revision += 1;
            if let Some(peer) = self.connected.get(&source_peer_id) {
                let mut st = peer
                    .last_state
                    .clone()
                    .unwrap_or_else(|| self.build_local_session_state());
                st.active_speaker = Some(self.local_device_id.clone());
                st.is_playing = start_playing;
                let queue = peer.last_queue.clone();
                let current_index = peer.last_queue_index;
                (self.on_event)(ConnectEvent::BecameActiveSpeaker {
                    state: st,
                    queue,
                    current_index,
                });
            }
            self.broadcast_local_state();
        } else if self.active_speaker_id == self.local_device_id {
            // Yield active speaker cleanly: ship latest queue + exact position snapshot first.
            let mut handoff_state = self.build_local_session_state();
            self.revision += 1;
            handoff_state.revision = self.revision;
            handoff_state.active_speaker = Some(new_speaker.clone());
            handoff_state.is_playing = start_playing;
            handoff_state.anchor_utc_ms = unix_now_utc_ms();

            self.active_speaker_id = new_speaker.clone();
            let target_name = self
                .connected
                .get(&new_speaker)
                .map(|p| p.device_name.clone())
                .unwrap_or_else(|| "Remote Device".to_string());

            let queue_frame = SyncFrame::QueueFull {
                revision: self.revision,
                tracks: self.local_playback.queue.clone(),
                current_index: self.local_playback.current_index,
            };
            let state_frame = SyncFrame::StateSnapshot(Box::new(handoff_state));
            self.broadcast_frame(&queue_frame);
            self.broadcast_frame(&state_frame);

            (self.on_event)(ConnectEvent::YieldedActiveSpeaker {
                new_speaker_id: new_speaker,
                new_speaker_name: target_name,
            });
        }
    }

    fn handle_local_transfer_request(&mut self, target_device_id: String) {
        if target_device_id == self.active_speaker_id {
            return;
        }
        if self.active_speaker_id == self.local_device_id {
            // We are currently playing -> hand off directly to `target_device_id`.
            if let Some(peer) = self.connected.get(&target_device_id) {
                let target_name = peer.device_name.clone();
                let mut handoff_state = self.build_local_session_state();
                self.revision += 1;
                handoff_state.revision = self.revision;
                handoff_state.active_speaker = Some(target_device_id.clone());
                handoff_state.is_playing = self.local_playback.is_playing;
                handoff_state.anchor_utc_ms = unix_now_utc_ms();

                self.active_speaker_id = target_device_id.clone();

                let queue_frame = SyncFrame::QueueFull {
                    revision: self.revision,
                    tracks: self.local_playback.queue.clone(),
                    current_index: self.local_playback.current_index,
                };
                let state_frame = SyncFrame::StateSnapshot(Box::new(handoff_state));
                self.broadcast_frame(&queue_frame);
                self.broadcast_frame(&state_frame);

                (self.on_event)(ConnectEvent::YieldedActiveSpeaker {
                    new_speaker_id: target_device_id,
                    new_speaker_name: target_name,
                });
            }
        } else {
            // Ask current active speaker to transfer playback to `target_device_id`.
            let active_id = self.active_speaker_id.clone();
            self.cmd_seq += 1;
            let cmd = SyncFrame::Command {
                cmd_id: self.cmd_seq,
                action: RemoteAction::TransferActiveSpeaker {
                    new_speaker: target_device_id.clone(),
                    start_playing: true,
                },
            };
            if let Some(peer) = self.connected.get(&active_id) {
                if let Ok(mut w) = peer.writer.lock() {
                    let _ = w.send_frame(&cmd);
                }
            } else if target_device_id == self.local_device_id {
                self.active_speaker_id = self.local_device_id.clone();
            }
        }
    }

    fn handle_local_remote_action(&mut self, action: RemoteAction) {
        let target_id = match &action {
            RemoteAction::SetVolume { target_device, .. } if !target_device.is_empty() => {
                target_device.clone()
            }
            _ => self.active_speaker_id.clone(),
        };

        if target_id == self.local_device_id {
            (self.on_event)(ConnectEvent::ExecuteRemoteAction(action));
            return;
        }

        if let Some(peer) = self.connected.get(&target_id) {
            self.cmd_seq += 1;
            let frame = SyncFrame::Command {
                cmd_id: self.cmd_seq,
                action,
            };
            if let Ok(mut w) = peer.writer.lock() {
                let _ = w.send_frame(&frame);
            }
        }
    }

    fn handle_pair_qr_uri(&mut self, uri: &str) {
        let Ok((payload, addr_opt)) = PairingPayload::parse_uri_with_addr(uri) else {
            (self.on_event)(ConnectEvent::Toast(
                "Invalid Ferric pairing QR/URI".to_string(),
            ));
            return;
        };
        let Some(guest_room) = derive_qr_room(&payload.token_hex) else {
            (self.on_event)(ConnectEvent::Toast("Invalid pairing token".to_string()));
            return;
        };
        let key = guest_room.key;
        self.qr_token_hex = payload.token_hex;
        self.keys.guest = Some(guest_room);
        self.last_beacon_at = None;

        if let Some(addr_str) = addr_opt
            && let Ok(addr) = addr_str.parse::<SocketAddr>()
            && is_lan_or_loopback(addr.ip())
        {
            self.spawn_outbound_handshake(addr, MatchedRoomSlot::GuestQr, key, String::new());
            (self.on_event)(ConnectEvent::Toast(format!(
                "Pairing with {}...",
                payload.device_name
            )));
        } else {
            (self.on_event)(ConnectEvent::Toast(format!(
                "Paired room active; searching LAN for {}...",
                payload.device_name
            )));
        }
    }

    fn build_local_session_state(&self) -> SessionState {
        let cur = self.local_playback.current_track.as_ref();
        SessionState {
            revision: self.revision,
            active_speaker: Some(self.active_speaker_id.clone()),
            is_playing: self.local_playback.is_playing,
            track_id: cur.map(|t| t.track_id.clone()),
            track_title: cur.map(|t| t.title.clone()).unwrap_or_default(),
            track_artist: cur.map(|t| t.artist.clone()).unwrap_or_default(),
            track_thumbnail_url: cur.and_then(|t| t.thumbnail_url.clone()),
            duration_ms: cur.map(|t| t.duration_ms).unwrap_or(0),
            base_position_ms: self.local_playback.position_ms,
            anchor_utc_ms: if self.local_playback.anchor_utc_ms > 0 {
                self.local_playback.anchor_utc_ms
            } else {
                unix_now_utc_ms()
            },
            volume_norm: self.local_playback.volume_norm.clamp(0.0, 1.5),
            repeat_mode: self.local_playback.repeat_mode,
            shuffle_enabled: self.local_playback.shuffle_enabled,
            device_name: self.local_device_name.clone(),
        }
    }

    fn broadcast_local_state(&self) {
        if self.connected.is_empty() {
            return;
        }
        let frame = SyncFrame::StateSnapshot(Box::new(self.build_local_session_state()));
        self.broadcast_frame(&frame);
    }

    fn broadcast_local_queue(&self) {
        if self.connected.is_empty() {
            return;
        }
        let frame = SyncFrame::QueueFull {
            revision: self.revision,
            tracks: self.local_playback.queue.clone(),
            current_index: self.local_playback.current_index,
        };
        self.broadcast_frame(&frame);
    }

    fn broadcast_frame(&self, frame: &SyncFrame) {
        for peer in self.connected.values() {
            if let Ok(mut w) = peer.writer.lock() {
                let _ = w.send_frame(frame);
            }
        }
    }

    fn publish_snapshot(&self) {
        let mut peers = Vec::new();
        for cp in self.connected.values() {
            let st = cp.last_state.as_ref();
            peers.push(ConnectPeerInfo {
                device_id: cp.device_id.clone(),
                device_name: cp.device_name.clone(),
                device_kind: cp.device_kind,
                addr: cp.addr.to_string(),
                connected: true,
                is_active_speaker: self.active_speaker_id == cp.device_id,
                is_playing: st.is_some_and(|s| s.is_playing),
                volume_norm: st.map_or(1.0, |s| s.volume_norm),
                now_playing_title: st.map(|s| s.track_title.clone()).unwrap_or_default(),
                now_playing_artist: st.map(|s| s.track_artist.clone()).unwrap_or_default(),
                now_playing_thumbnail_url: st.and_then(|s| s.track_thumbnail_url.clone()),
                position_ms: st.map_or(0, |s| s.base_position_ms),
                duration_ms: st.map_or(0, |s| s.duration_ms),
            });
        }
        for dp in self.discovered.values() {
            if self.connected.contains_key(&dp.device_id) {
                continue;
            }
            peers.push(ConnectPeerInfo {
                device_id: dp.device_id.clone(),
                device_name: dp.device_name.clone(),
                device_kind: dp.device_kind,
                addr: dp.addr.clone(),
                connected: false,
                is_active_speaker: dp.is_active_speaker,
                is_playing: false,
                volume_norm: 1.0,
                now_playing_title: dp.now_playing_title.clone(),
                now_playing_artist: dp.now_playing_artist.clone(),
                now_playing_thumbnail_url: None,
                position_ms: 0,
                duration_ms: 0,
            });
        }
        peers.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| b.is_active_speaker.cmp(&a.is_active_speaker))
                .then_with(|| a.device_name.cmp(&b.device_name))
                .then_with(|| a.device_id.cmp(&b.device_id))
        });

        let snap = ConnectSnapshot {
            local_device_id: self.local_device_id.clone(),
            local_device_name: self.local_device_name.clone(),
            local_device_kind: self.local_device_kind,
            local_ip: self.local_ip.clone(),
            local_tcp_port: self.local_tcp_port,
            room_tags_hex: encode_room_tags_hex(&self.keys.advertised_tags()),
            active_speaker_id: self.active_speaker_id.clone(),
            is_local_active_speaker: self.active_speaker_id == self.local_device_id,
            is_account_ready: self.keys.spotify.is_some() || self.keys.ytm.is_some(),
            peers,
        };
        {
            let mut slot = self
                .latest_snapshot
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            *slot = snap.clone();
        }
        (self.on_event)(ConnectEvent::StateUpdated(snap));
    }
}

fn perform_outbound_handshake(
    addr: SocketAddr,
    slot: MatchedRoomSlot,
    room_key: &[u8; 32],
    local_id: &str,
    local_name: &str,
    local_kind: DeviceKind,
) -> SyncResult<(
    String,
    String,
    DeviceKind,
    PeerWriter,
    TcpStream,
    AeadReceiver,
)> {
    if !is_lan_or_loopback(addr.ip()) {
        return Err(SyncError::NonLanPeer(addr.to_string()));
    }
    let mut stream = TcpStream::connect_timeout(&addr, HANDSHAKE_TIMEOUT)?;
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;

    let mut nonce_a = [0u8; 16];
    csprng_fill(&mut nonce_a);
    let id_bytes = local_id.as_bytes();
    if id_bytes.is_empty() || id_bytes.len() > MAX_HANDSHAKE_ID_LEN {
        return Err(SyncError::AuthFailed);
    }

    // ClientHello: [FCH1][slot:u8][id_len:u8][id][nonce_a:16]
    let mut hello = Vec::with_capacity(6 + id_bytes.len() + 16);
    hello.extend_from_slice(HANDSHAKE_MAGIC);
    hello.push(slot as u8);
    hello.push(id_bytes.len() as u8);
    hello.extend_from_slice(id_bytes);
    hello.extend_from_slice(&nonce_a);
    stream.write_all(&hello)?;
    stream.flush()?;

    // ServerHello: [FCH1][status:u8][peer_id_len:u8][peer_id][nonce_b:16][mac_b:32]
    let mut hdr = [0u8; 6];
    stream.read_exact(&mut hdr)?;
    if &hdr[..4] != HANDSHAKE_MAGIC || hdr[4] != 1 {
        return Err(SyncError::AuthFailed);
    }
    let peer_id_len = usize::from(hdr[5]);
    if peer_id_len == 0 || peer_id_len > MAX_HANDSHAKE_ID_LEN {
        return Err(SyncError::AuthFailed);
    }
    let mut peer_id_buf = vec![0u8; peer_id_len];
    stream.read_exact(&mut peer_id_buf)?;
    let peer_id = String::from_utf8(peer_id_buf).map_err(|_| SyncError::AuthFailed)?;

    let mut nonce_b = [0u8; 16];
    stream.read_exact(&mut nonce_b)?;
    let mut mac_b = [0u8; 32];
    stream.read_exact(&mut mac_b)?;

    let expected_mac_b = compute_handshake_mac(room_key, &nonce_a, &nonce_b, local_id, &peer_id);
    if !ct_eq(&mac_b, &expected_mac_b) {
        return Err(SyncError::AuthFailed);
    }

    // ClientFinish: [mac_a:32]
    let mac_a = compute_handshake_mac(room_key, &nonce_b, &nonce_a, &peer_id, local_id);
    stream.write_all(&mac_a)?;
    stream.flush()?;

    let (tx, mut rx) = AeadChannel::from_handshake(room_key, &nonce_a, &nonce_b, true).split();
    let rx_stream = stream.try_clone()?;
    let mut writer = PeerWriter { stream, tx };

    // Exchange encrypted Hello carrying friendly name and device kind
    writer.send_frame(&SyncFrame::Hello {
        device_id: local_id.to_string(),
        device_name: local_name.to_string(),
        device_kind: local_kind,
    })?;
    let mut rx_stream_mut = rx_stream;
    let first = read_encrypted_frame(&mut rx_stream_mut, &mut rx)?;
    let (peer_name, peer_kind) = match first {
        SyncFrame::Hello {
            device_name,
            device_kind,
            ..
        } => (sanitize_device_name(&device_name), device_kind),
        _ => return Err(SyncError::AuthFailed),
    };

    let _ = rx_stream_mut.set_read_timeout(None);
    let _ = writer
        .stream
        .set_write_timeout(Some(Duration::from_secs(5)));
    Ok((peer_id, peer_name, peer_kind, writer, rx_stream_mut, rx))
}

fn perform_inbound_handshake(
    mut stream: TcpStream,
    keys: &LocalRoomKeys,
    local_id: &str,
    local_name: &str,
    local_kind: DeviceKind,
) -> SyncResult<(
    String,
    String,
    DeviceKind,
    PeerWriter,
    TcpStream,
    AeadReceiver,
)> {
    let peer_addr = stream.peer_addr()?;
    if !is_lan_or_loopback(peer_addr.ip()) {
        return Err(SyncError::NonLanPeer(peer_addr.to_string()));
    }
    stream.set_nonblocking(false)?;
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT))?;

    let mut hdr = [0u8; 6];
    stream.read_exact(&mut hdr)?;
    if &hdr[..4] != HANDSHAKE_MAGIC {
        return Err(SyncError::AuthFailed);
    }
    let slot = MatchedRoomSlot::from_u8(hdr[4]).ok_or(SyncError::AuthFailed)?;
    let Some(room_key) = keys.key_for_slot(slot) else {
        let _ = stream.write_all(&[b'F', b'C', b'H', b'1', 0, 0]);
        return Err(SyncError::AuthFailed);
    };
    let peer_id_len = usize::from(hdr[5]);
    if peer_id_len == 0 || peer_id_len > MAX_HANDSHAKE_ID_LEN {
        return Err(SyncError::AuthFailed);
    }
    let mut peer_id_buf = vec![0u8; peer_id_len];
    stream.read_exact(&mut peer_id_buf)?;
    let peer_id = String::from_utf8(peer_id_buf).map_err(|_| SyncError::AuthFailed)?;
    if peer_id == local_id {
        return Err(SyncError::AuthFailed);
    }

    let mut nonce_a = [0u8; 16];
    stream.read_exact(&mut nonce_a)?;

    let mut nonce_b = [0u8; 16];
    csprng_fill(&mut nonce_b);
    let mac_b = compute_handshake_mac(&room_key, &nonce_a, &nonce_b, &peer_id, local_id);

    let local_id_bytes = local_id.as_bytes();
    let mut resp = Vec::with_capacity(6 + local_id_bytes.len() + 16 + 32);
    resp.extend_from_slice(HANDSHAKE_MAGIC);
    resp.push(1u8);
    resp.push(local_id_bytes.len() as u8);
    resp.extend_from_slice(local_id_bytes);
    resp.extend_from_slice(&nonce_b);
    resp.extend_from_slice(&mac_b);
    stream.write_all(&resp)?;
    stream.flush()?;

    let mut mac_a = [0u8; 32];
    stream.read_exact(&mut mac_a)?;
    let expected_mac_a = compute_handshake_mac(&room_key, &nonce_b, &nonce_a, local_id, &peer_id);
    if !ct_eq(&mac_a, &expected_mac_a) {
        return Err(SyncError::AuthFailed);
    }

    let (tx, mut rx) = AeadChannel::from_handshake(&room_key, &nonce_a, &nonce_b, false).split();
    let rx_stream = stream.try_clone()?;
    let mut writer = PeerWriter { stream, tx };

    writer.send_frame(&SyncFrame::Hello {
        device_id: local_id.to_string(),
        device_name: local_name.to_string(),
        device_kind: local_kind,
    })?;

    let mut rx_stream_mut = rx_stream;
    let first = read_encrypted_frame(&mut rx_stream_mut, &mut rx)?;
    let (peer_name, peer_kind) = match first {
        SyncFrame::Hello {
            device_name,
            device_kind,
            ..
        } => (sanitize_device_name(&device_name), device_kind),
        _ => return Err(SyncError::AuthFailed),
    };

    let _ = rx_stream_mut.set_read_timeout(None);
    let _ = writer
        .stream
        .set_write_timeout(Some(Duration::from_secs(5)));
    Ok((peer_id, peer_name, peer_kind, writer, rx_stream_mut, rx))
}

fn read_encrypted_frame(stream: &mut TcpStream, rx: &mut AeadReceiver) -> SyncResult<SyncFrame> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES + 16 {
        return Err(SyncError::FrameTooLarge(len));
    }
    let mut ct = vec![0u8; len];
    stream.read_exact(&mut ct)?;
    let pt = rx.open(&ct)?;
    decode_frame_payload(&pt)
}

fn run_peer_reader(
    peer_id: String,
    mut stream: TcpStream,
    mut rx: AeadReceiver,
    int_tx: mpsc::Sender<InternalMsg>,
) {
    loop {
        match read_encrypted_frame(&mut stream, &mut rx) {
            Ok(frame) => {
                if int_tx
                    .send(InternalMsg::PeerFrame {
                        device_id: peer_id.clone(),
                        frame,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => {
                let _ = int_tx.send(InternalMsg::PeerClosed { device_id: peer_id });
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_until<F: Fn() -> bool>(timeout_ms: u64, cond: F) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(timeout_ms) {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        cond()
    }

    #[test]
    fn zero_pairing_same_account_connects_controls_and_hands_off_speaker() {
        let desktop_events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let phone_events: Arc<Mutex<Vec<ConnectEvent>>> = Arc::new(Mutex::new(Vec::new()));

        let d_ev = Arc::clone(&desktop_events);
        let desktop = ConnectEngine::start(
            ConnectConfig {
                device_id: "a_desktop_pc".to_string(),
                device_name: "RAFLYF Desktop".to_string(),
                device_kind: DeviceKind::Desktop,
                bind_ephemeral_udp: true,
            },
            Arc::new(move |ev| d_ev.lock().expect("lock").push(ev)),
        )
        .expect("start desktop");

        let p_ev = Arc::clone(&phone_events);
        let phone = ConnectEngine::start(
            ConnectConfig {
                device_id: "z_android_phone".to_string(),
                device_name: "Pixel Phone".to_string(),
                device_kind: DeviceKind::Phone,
                bind_ephemeral_udp: true,
            },
            Arc::new(move |ev| p_ev.lock().expect("lock").push(ev)),
        )
        .expect("start phone");

        // Desktop signed into BOTH Spotify + YTM; Phone signed into YTM ONLY -> must still auto-connect!
        desktop.update_accounts("rafly_sp".to_string(), "rafly@gmail.com".to_string());
        phone.update_accounts(String::new(), "RAFLY@GMAIL.COM".to_string());

        // Desktop is actively playing a song
        let song = QueueEntry {
            track_id: "yt:video:e-fA-gBCkj0".to_string(),
            title: "Locked Out of Heaven".to_string(),
            artist: "Bruno Mars".to_string(),
            album: "Unorthodox Jukebox".to_string(),
            duration_ms: 233_000,
            spotify_id: Some("3w3y8KPTfNeOKPiqUTakBh".to_string()),
            youtube_video_id: Some("e-fA-gBCkj0".to_string()),
            thumbnail_url: None,
        };
        desktop.update_local_playback(LocalPlaybackSnapshot {
            is_playing: true,
            current_track: Some(song.clone()),
            queue: vec![song],
            current_index: 0,
            position_ms: 64_000,
            volume_norm: 0.90,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            anchor_utc_ms: unix_now_utc_ms(),
        });

        // Wait until room tags are derived
        assert!(wait_until(2_000, || desktop.snapshot().is_account_ready
            && phone.snapshot().is_account_ready));

        // Simulate mDNS/NSD discovery hint on loopback
        let phone_snap = phone.snapshot();
        desktop.notify_mdns_peer(
            phone_snap.local_device_id.clone(),
            phone_snap.local_device_name.clone(),
            DeviceKind::Phone,
            "127.0.0.1",
            phone_snap.local_tcp_port,
            &phone_snap.room_tags_hex,
        );

        assert!(
            wait_until(4_000, || {
                desktop.snapshot().peers.iter().any(|p| p.connected)
                    && phone.snapshot().peers.iter().any(|p| p.connected)
            }),
            "desktop and phone should auto-connect via YTM room tag"
        );

        // Phone should recognize Desktop as active speaker with live song metadata
        assert!(wait_until(2_000, || {
            let s = phone.snapshot();
            !s.is_local_active_speaker
                && s.active_speaker_id == "a_desktop_pc"
                && s.peers
                    .iter()
                    .any(|p| p.now_playing_title == "Locked Out of Heaven")
        }));

        // Phone adjusts Desktop volume remotely
        phone.send_remote_action(RemoteAction::SetVolume {
            target_device: "a_desktop_pc".to_string(),
            volume_norm: 0.45,
        });
        assert!(wait_until(2_000, || {
            desktop_events.lock().expect("lock").iter().any(|ev| {
                matches!(
                    ev,
                    ConnectEvent::ExecuteRemoteAction(RemoteAction::SetVolume { volume_norm, .. })
                        if (*volume_norm - 0.45).abs() < 0.01
                )
            })
        }));

        // Phone taps "This phone" -> hands off active speaker cleanly at exact timestamp + queue!
        phone.transfer_active_speaker("z_android_phone".to_string());
        assert!(wait_until(3_000, || {
            phone_events.lock().expect("lock").iter().any(|ev| {
                matches!(
                    ev,
                    ConnectEvent::BecameActiveSpeaker { state, queue, .. }
                        if state.track_title == "Locked Out of Heaven"
                            && state.base_position_ms == 64_000
                            && queue.len() == 1
                )
            }) && desktop_events.lock().expect("lock").iter().any(|ev| {
                matches!(
                    ev,
                    ConnectEvent::YieldedActiveSpeaker { new_speaker_id, .. }
                        if new_speaker_id == "z_android_phone"
                )
            })
        }));

        desktop.shutdown();
        phone.shutdown();
    }

    #[test]
    fn mismatched_account_is_rejected_by_mutual_handshake() {
        let desktop = ConnectEngine::start(
            ConnectConfig {
                device_id: "a_desk".to_string(),
                device_name: "Desk".to_string(),
                device_kind: DeviceKind::Desktop,
                bind_ephemeral_udp: true,
            },
            Arc::new(|_| {}),
        )
        .expect("start desktop");
        let stranger = ConnectEngine::start(
            ConnectConfig {
                device_id: "z_stranger".to_string(),
                device_name: "Stranger".to_string(),
                device_kind: DeviceKind::Phone,
                bind_ephemeral_udp: true,
            },
            Arc::new(|_| {}),
        )
        .expect("start stranger");

        desktop.update_accounts("alice".to_string(), String::new());
        stranger.update_accounts("mallory".to_string(), String::new());
        assert!(wait_until(2_000, || desktop.snapshot().is_account_ready
            && stranger.snapshot().is_account_ready));

        // Even if Mallory spoofs Alice's public room tag in mDNS, mutual HMAC handshake MUST reject!
        let alice_tags = desktop.snapshot().room_tags_hex;
        desktop.notify_mdns_peer(
            "z_stranger".to_string(),
            "Stranger".to_string(),
            DeviceKind::Phone,
            "127.0.0.1",
            stranger.local_tcp_port(),
            &alice_tags,
        );
        thread::sleep(Duration::from_millis(450));
        assert!(
            desktop.snapshot().peers.iter().all(|p| !p.connected),
            "spoofed peer without matching room_key must never connect"
        );

        desktop.shutdown();
        stranger.shutdown();
    }
}
