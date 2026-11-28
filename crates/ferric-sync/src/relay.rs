//! Encrypted cross-network MQTT-over-WSS relay client for Ferric Connect.
//!
//! Devices signed into the same Spotify or YouTube Music account (or sharing a QR pairing room)
//! automatically connect across arbitrary networks (home Wi-Fi, 4G/5G cellular, office networks,
//! behind NAT/CGNAT) without manual port forwarding or user-supplied relay credentials.
//!
//! Privacy & Security:
//! - Topic names are `ferric/v1/<hmac16>`, computed locally from the account's HKDF `room_key`.
//! - Every payload is encrypted with `ChaCha20-Poly1305` under a per-boot `epoch_salt` + monotonic
//!   counter (`FCR1`), with sender authentication and a bounded anti-replay sliding window.
//! - MQTT brokers only ever route ciphertext between opaque topic hashes using random ephemeral
//!   client IDs.

use crate::crypto::{
    MatchedRoomSlot, RoomKey, derive_relay_topic, hex_encode, open_relay_packet, seal_relay_packet,
};
use crate::lan::sanitize_device_name;
use crate::{DeviceKind, MAX_FRAME_BYTES, SyncError, SyncFrame, SyncResult};
use ferric_core::secrets::csprng_fill;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Response;
use tungstenite::protocol::WebSocket;
use tungstenite::stream::NoDelay;

pub const DEFAULT_RELAY_BROKERS: &[&str] = &[
    "wss://broker.emqx.io:8084/mqtt",
    "wss://broker.hivemq.com:8884/mqtt",
    "wss://test.mosquitto.org:8081/mqtt",
];

const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const SOCKET_READ_TIMEOUT: Duration = Duration::from_millis(90);
const SOCKET_WRITE_TIMEOUT: Duration = Duration::from_secs(4);
const MQTT_KEEPALIVE_SECS: u16 = 25;
const PING_INTERVAL: Duration = Duration::from_secs(14);
const RECONNECT_MIN_BACKOFF: Duration = Duration::from_secs(2);
const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(25);
const MAX_RELAY_PACKET_BYTES: usize = 56 * 1024;
const MAX_REPLAY_SENDERS: usize = 64;

const MQTT_CONNECT: u8 = 0x10;
const MQTT_CONNACK: u8 = 0x20;
const MQTT_PUBLISH: u8 = 0x30;
#[cfg(test)]
const MQTT_SUBSCRIBE: u8 = 0x80;
const MQTT_SUBSCRIBE_FLAGS: u8 = 0x82;
const MQTT_SUBACK: u8 = 0x90;
const MQTT_UNSUBSCRIBE_FLAGS: u8 = 0xA2;
const MQTT_UNSUBACK: u8 = 0xB0;
const MQTT_PINGREQ: u8 = 0xC0;
const MQTT_PINGRESP: u8 = 0xD0;

/// Authenticated inner envelope transported encrypted inside `FCR1` relay packets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelayEnvelope {
    pub sender_id: String,
    pub sender_name: String,
    pub sender_kind: DeviceKind,
    pub target_id: Option<String>,
    pub frame: SyncFrame,
}

#[derive(Debug, Clone)]
pub struct RelayRoomSpec {
    pub slot: MatchedRoomSlot,
    pub key: RoomKey,
    pub topic: String,
}

impl RelayRoomSpec {
    #[must_use]
    pub fn from_key(slot: MatchedRoomSlot, key: RoomKey) -> Self {
        let topic = derive_relay_topic(key.as_bytes());
        Self { slot, key, topic }
    }
}

#[derive(Debug, Clone)]
pub enum RelayEvent {
    StatusChanged {
        connected: bool,
        broker_label: String,
    },
    Inbound {
        slot: MatchedRoomSlot,
        envelope: Box<RelayEnvelope>,
    },
}

enum RelayCmd {
    UpdateRooms(Vec<RelayRoomSpec>),
    Publish {
        topic: String,
        room_key: RoomKey,
        envelope: Box<RelayEnvelope>,
    },
    Shutdown,
}

enum RelayTransportStream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for RelayTransportStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for RelayTransportStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

impl NoDelay for RelayTransportStream {
    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.set_nodelay(nodelay),
            Self::Tls(s) => s.get_ref().set_nodelay(nodelay),
        }
    }
}

struct ReplayWindow {
    highest_seq: u64,
    seen_mask: u64,
}

impl ReplayWindow {
    fn new(seq: u64) -> Self {
        Self {
            highest_seq: seq,
            seen_mask: 1,
        }
    }

    fn accept(&mut self, seq: u64) -> bool {
        if seq == 0 {
            return false;
        }
        if seq > self.highest_seq {
            let shift = seq - self.highest_seq;
            self.seen_mask = if shift >= 64 {
                1
            } else {
                (self.seen_mask << shift) | 1
            };
            self.highest_seq = seq;
            true
        } else {
            let delta = self.highest_seq - seq;
            if delta >= 64 {
                return false;
            }
            let bit = 1u64 << delta;
            if (self.seen_mask & bit) != 0 {
                return false;
            }
            self.seen_mask |= bit;
            true
        }
    }
}

struct ReplayGuard {
    windows: HashMap<(String, [u8; 16]), ReplayWindow>,
    order: VecDeque<(String, [u8; 16])>,
}

impl ReplayGuard {
    fn new() -> Self {
        Self {
            windows: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn accept(&mut self, sender_id: &str, epoch_salt: [u8; 16], seq: u64) -> bool {
        let key = (sender_id.to_string(), epoch_salt);
        if let Some(win) = self.windows.get_mut(&key) {
            return win.accept(seq);
        }
        if self.order.len() >= MAX_REPLAY_SENDERS
            && let Some(evicted) = self.order.pop_front()
        {
            self.windows.remove(&evicted);
        }
        self.order.push_back(key.clone());
        self.windows.insert(key, ReplayWindow::new(seq));
        true
    }
}

#[derive(Debug)]
struct BrokerEndpoint {
    url: String,
    host: String,
    port: u16,
    tls: bool,
    label: String,
}

fn parse_broker_url(raw: &str) -> Option<BrokerEndpoint> {
    let trimmed = raw.trim();
    let (tls, default_port, rest) = if let Some(r) = trimmed.strip_prefix("wss://") {
        (true, 443u16, r)
    } else {
        let r = trimmed.strip_prefix("ws://")?;
        (false, 80u16, r)
    };
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.is_empty() {
        return None;
    }
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        if let Some((h, tail)) = bracketed.split_once(']') {
            let p = tail
                .strip_prefix(':')
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(default_port);
            (h.to_string(), p)
        } else {
            return None;
        }
    } else if let Some((h, p_str)) = host_port.rsplit_once(':') {
        let p = p_str.parse::<u16>().ok()?;
        (h.to_string(), p)
    } else {
        (host_port.to_string(), default_port)
    };
    if host.is_empty() {
        return None;
    }
    Some(BrokerEndpoint {
        url: trimmed.to_string(),
        label: format!("{host}:{port}"),
        host,
        port,
        tls,
    })
}

fn resolve_broker_list(override_urls: Option<Vec<String>>) -> Vec<BrokerEndpoint> {
    let env_list: Option<Vec<String>> = override_urls.or_else(|| {
        std::env::var("FERRIC_CONNECT_RELAY_URL")
            .ok()
            .map(|raw| {
                raw.split([',', ';'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
    });

    match env_list {
        Some(list) => list.iter().filter_map(|u| parse_broker_url(u)).collect(),
        None => DEFAULT_RELAY_BROKERS
            .iter()
            .filter_map(|u| parse_broker_url(u))
            .collect(),
    }
}

/// Handle owning the background Cloud E2EE relay worker thread.
pub struct RelayClient {
    cmd_tx: mpsc::Sender<RelayCmd>,
    connected: Arc<AtomicBool>,
    broker_label: Arc<Mutex<String>>,
    shutdown: Arc<AtomicBool>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl RelayClient {
    /// Start the relay client thread (idle until `update_rooms` receives >=1 room key).
    pub fn start(
        local_device_id: String,
        override_urls: Option<Vec<String>>,
        on_event: Arc<dyn Fn(RelayEvent) + Send + Sync>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let broker_label = Arc::new(Mutex::new(String::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let conn_flag = Arc::clone(&connected);
        let label_arc = Arc::clone(&broker_label);
        let shut_flag = Arc::clone(&shutdown);
        let endpoints = resolve_broker_list(override_urls);

        let handle = thread::Builder::new()
            .name("ferric-connect-relay".to_string())
            .spawn(move || {
                relay_worker_loop(
                    local_device_id,
                    endpoints,
                    cmd_rx,
                    conn_flag,
                    label_arc,
                    shut_flag,
                    on_event,
                );
            })
            .ok();

        Self {
            cmd_tx,
            connected,
            broker_label,
            shutdown,
            worker: Mutex::new(handle),
        }
    }

    pub fn update_rooms(&self, rooms: Vec<RelayRoomSpec>) {
        let _ = self.cmd_tx.send(RelayCmd::UpdateRooms(rooms));
    }

    pub fn publish_envelope(&self, topic: &str, room_key: RoomKey, envelope: RelayEnvelope) {
        let _ = self.cmd_tx.send(RelayCmd::Publish {
            topic: topic.to_string(),
            room_key,
            envelope: Box::new(envelope),
        });
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn broker_label(&self) -> String {
        self.broker_label
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.cmd_tx.send(RelayCmd::Shutdown);
        if let Some(h) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take()
            && h.thread().id() != thread::current().id()
        {
            let _ = h.join();
        }
    }
}

impl Drop for RelayClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn make_tls_config() -> SyncResult<Arc<ClientConfig>> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| SyncError::Crypto("tls protocol init failed"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

fn connect_tcp_with_timeout(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
    let mut last_err = io::Error::new(ErrorKind::AddrNotAvailable, "no DNS records for relay host");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_err = err,
        }
    }
    Err(last_err)
}

fn dial_mqtt_websocket(
    endpoint: &BrokerEndpoint,
    tls_cfg: &Arc<ClientConfig>,
) -> SyncResult<WebSocket<RelayTransportStream>> {
    let tcp = connect_tcp_with_timeout(&endpoint.host, endpoint.port, CONNECT_TIMEOUT)?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(CONNECT_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT));

    let transport = if endpoint.tls {
        let server_name =
            ServerName::try_from(endpoint.host.clone()).map_err(|_| SyncError::AuthFailed)?;
        let conn = ClientConnection::new(Arc::clone(tls_cfg), server_name)
            .map_err(|_| SyncError::Crypto("tls client connection failed"))?;
        let mut tls_stream = Box::new(StreamOwned::new(conn, tcp));
        // Complete TLS handshake before handing off to tungstenite.
        while tls_stream.conn.is_handshaking() {
            if tls_stream.conn.complete_io(&mut tls_stream.sock).is_err() {
                return Err(SyncError::Io(io::Error::new(
                    ErrorKind::ConnectionRefused,
                    "relay TLS handshake failed",
                )));
            }
        }
        RelayTransportStream::Tls(tls_stream)
    } else {
        RelayTransportStream::Plain(tcp)
    };

    let mut req = endpoint
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| SyncError::InvalidPairingUri(e.to_string()))?;
    req.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "mqtt"
            .parse()
            .map_err(|_| SyncError::Crypto("invalid mqtt subprotocol header"))?,
    );

    let (mut ws, _resp): (WebSocket<RelayTransportStream>, Response) =
        tungstenite::client(req, transport).map_err(|e| {
            SyncError::Io(io::Error::new(
                ErrorKind::ConnectionAborted,
                format!("websocket upgrade failed: {e}"),
            ))
        })?;

    // Send MQTT v3.1.1 CONNECT packet with random ephemeral client ID (zero PII on wire).
    let mut rnd = [0u8; 8];
    csprng_fill(&mut rnd);
    let client_id = format!("fc-{}", hex_encode(&rnd));
    let connect_pkt = encode_mqtt_connect(&client_id, MQTT_KEEPALIVE_SECS);
    ws.send(Message::Binary(connect_pkt.into()))
        .map_err(|e| io::Error::new(ErrorKind::BrokenPipe, e.to_string()))?;

    // Wait for MQTT CONNACK.
    let ack_deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if Instant::now() >= ack_deadline {
            return Err(io::Error::new(ErrorKind::TimedOut, "mqtt connack timeout").into());
        }
        match ws.read() {
            Ok(Message::Binary(bytes)) => {
                if bytes.len() >= 4 && (bytes[0] & 0xF0) == MQTT_CONNACK && bytes[3] == 0 {
                    break;
                }
                return Err(SyncError::AuthFailed);
            }
            Ok(Message::Ping(p)) => {
                let _ = ws.send(Message::Pong(p));
            }
            Ok(Message::Close(_)) => {
                return Err(
                    io::Error::new(ErrorKind::ConnectionReset, "relay closed on connect").into(),
                );
            }
            Ok(_) => {}
            Err(err) => {
                return Err(io::Error::new(ErrorKind::ConnectionAborted, err.to_string()).into());
            }
        }
    }

    match ws.get_ref() {
        RelayTransportStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(SOCKET_READ_TIMEOUT));
        }
        RelayTransportStream::Tls(s) => {
            let _ = s.get_ref().set_read_timeout(Some(SOCKET_READ_TIMEOUT));
        }
    }

    Ok(ws)
}

fn relay_worker_loop(
    local_device_id: String,
    endpoints: Vec<BrokerEndpoint>,
    cmd_rx: mpsc::Receiver<RelayCmd>,
    connected_flag: Arc<AtomicBool>,
    broker_label_arc: Arc<Mutex<String>>,
    shutdown: Arc<AtomicBool>,
    on_event: Arc<dyn Fn(RelayEvent) + Send + Sync>,
) {
    let Ok(tls_cfg) = make_tls_config() else {
        return;
    };
    let mut epoch_salt = [0u8; 16];
    csprng_fill(&mut epoch_salt);
    let mut tx_seq = 1u64;
    let mut packet_id_seq = 1u16;
    let mut rooms: Vec<RelayRoomSpec> = Vec::new();
    let mut replay_guard = ReplayGuard::new();
    let mut broker_idx = 0usize;
    let mut backoff = RECONNECT_MIN_BACKOFF;

    let set_status = |is_conn: bool, label: &str| {
        connected_flag.store(is_conn, Ordering::Relaxed);
        {
            let mut slot = broker_label_arc.lock().unwrap_or_else(|p| p.into_inner());
            *slot = label.to_string();
        }
        on_event(RelayEvent::StatusChanged {
            connected: is_conn,
            broker_label: label.to_string(),
        });
    };

    'outer: while !shutdown.load(Ordering::Acquire) {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                RelayCmd::Shutdown => break 'outer,
                RelayCmd::UpdateRooms(next_rooms) => {
                    rooms = next_rooms;
                }
                RelayCmd::Publish { .. } => {}
            }
        }

        if rooms.is_empty() || endpoints.is_empty() {
            set_status(false, "");
            match cmd_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(RelayCmd::Shutdown) => break 'outer,
                Ok(RelayCmd::UpdateRooms(next_rooms)) => rooms = next_rooms,
                _ => {}
            }
            continue;
        }

        let ep = &endpoints[broker_idx % endpoints.len()];
        let Ok(mut ws) = dial_mqtt_websocket(ep, &tls_cfg) else {
            broker_idx = broker_idx.wrapping_add(1);
            set_status(false, "");
            let wait_until = Instant::now() + backoff;
            backoff = (backoff * 2).min(RECONNECT_MAX_BACKOFF);
            while Instant::now() < wait_until && !shutdown.load(Ordering::Acquire) {
                match cmd_rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(RelayCmd::Shutdown) => break 'outer,
                    Ok(RelayCmd::UpdateRooms(next_rooms)) => {
                        let had_none = rooms.is_empty();
                        rooms = next_rooms;
                        if had_none && !rooms.is_empty() {
                            backoff = RECONNECT_MIN_BACKOFF;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        };

        // Reset backoff and subscribe to all active opaque room topics.
        backoff = RECONNECT_MIN_BACKOFF;
        let mut subscribed_topics: Vec<String> = rooms.iter().map(|r| r.topic.clone()).collect();
        if !subscribed_topics.is_empty() {
            packet_id_seq = packet_id_seq.wrapping_add(1).max(1);
            let sub = encode_mqtt_subscribe(packet_id_seq, &subscribed_topics);
            if ws.send(Message::Binary(sub.into())).is_err() {
                broker_idx = broker_idx.wrapping_add(1);
                continue;
            }
        }
        set_status(true, &ep.label);

        let mut last_ping_at = Instant::now();
        let mut rx_buf: Vec<u8> = Vec::with_capacity(4096);

        'session: while !shutdown.load(Ordering::Acquire) {
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    RelayCmd::Shutdown => break 'outer,
                    RelayCmd::UpdateRooms(next_rooms) => {
                        let next_topics: Vec<String> =
                            next_rooms.iter().map(|r| r.topic.clone()).collect();
                        let to_unsub: Vec<String> = subscribed_topics
                            .iter()
                            .filter(|t| !next_topics.contains(*t))
                            .cloned()
                            .collect();
                        let to_sub: Vec<String> = next_topics
                            .iter()
                            .filter(|t| !subscribed_topics.contains(*t))
                            .cloned()
                            .collect();
                        if !to_unsub.is_empty() {
                            packet_id_seq = packet_id_seq.wrapping_add(1).max(1);
                            let pkt = encode_mqtt_unsubscribe(packet_id_seq, &to_unsub);
                            if ws.send(Message::Binary(pkt.into())).is_err() {
                                break 'session;
                            }
                        }
                        if !to_sub.is_empty() {
                            packet_id_seq = packet_id_seq.wrapping_add(1).max(1);
                            let pkt = encode_mqtt_subscribe(packet_id_seq, &to_sub);
                            if ws.send(Message::Binary(pkt.into())).is_err() {
                                break 'session;
                            }
                        }
                        subscribed_topics = next_topics;
                        rooms = next_rooms;
                        if rooms.is_empty() {
                            break 'session;
                        }
                    }
                    RelayCmd::Publish {
                        topic,
                        room_key,
                        envelope,
                    } => {
                        if !subscribed_topics.iter().any(|t| t == &topic) {
                            continue;
                        }
                        let Ok(plain) = postcard::to_stdvec(&*envelope) else {
                            continue;
                        };
                        let seq = tx_seq;
                        tx_seq = tx_seq.saturating_add(1);
                        let Ok(sealed) =
                            seal_relay_packet(room_key.as_bytes(), &epoch_salt, seq, &plain)
                        else {
                            continue;
                        };
                        let mqtt_pkt = encode_mqtt_publish(&topic, &sealed);
                        if mqtt_pkt.len() > MAX_RELAY_PACKET_BYTES {
                            continue;
                        }
                        if ws.send(Message::Binary(mqtt_pkt.into())).is_err() {
                            break 'session;
                        }
                        last_ping_at = Instant::now();
                    }
                }
            }

            if last_ping_at.elapsed() >= PING_INTERVAL {
                if ws
                    .send(Message::Binary(vec![MQTT_PINGREQ, 0].into()))
                    .is_err()
                {
                    break 'session;
                }
                last_ping_at = Instant::now();
            }

            match ws.read() {
                Ok(Message::Binary(data)) => {
                    rx_buf.extend_from_slice(&data);
                    while let Some((pkt_type, body)) = try_pop_mqtt_packet(&mut rx_buf) {
                        match pkt_type & 0xF0 {
                            MQTT_PUBLISH => {
                                if let Some((topic, payload)) = parse_mqtt_publish_body(&body) {
                                    handle_inbound_publish(
                                        &local_device_id,
                                        &rooms,
                                        &mut replay_guard,
                                        &topic,
                                        &payload,
                                        &on_event,
                                    );
                                }
                            }
                            MQTT_PINGRESP | MQTT_SUBACK | MQTT_UNSUBACK | MQTT_CONNACK => {}
                            _ => {}
                        }
                    }
                }
                Ok(Message::Ping(p)) => {
                    if ws.send(Message::Pong(p)).is_err() {
                        break 'session;
                    }
                }
                Ok(Message::Close(_)) => break 'session,
                Ok(_) => {}
                Err(tungstenite::Error::Io(ref err))
                    if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => break 'session,
            }
        }

        let _ = ws.close(None);
        broker_idx = broker_idx.wrapping_add(1);
        set_status(false, "");
    }

    set_status(false, "");
}

fn handle_inbound_publish(
    local_device_id: &str,
    rooms: &[RelayRoomSpec],
    replay_guard: &mut ReplayGuard,
    topic: &str,
    payload: &[u8],
    on_event: &Arc<dyn Fn(RelayEvent) + Send + Sync>,
) {
    let Some(room) = rooms.iter().find(|r| r.topic == topic) else {
        return;
    };
    let Ok((epoch_salt, seq, pt)) = open_relay_packet(room.key.as_bytes(), payload) else {
        return;
    };
    let Ok(mut env) = postcard::from_bytes::<RelayEnvelope>(&pt) else {
        return;
    };
    let sender_id = env.sender_id.trim();
    if sender_id.is_empty() || sender_id.len() > 64 || sender_id == local_device_id {
        return;
    }
    if let Some(ref tgt) = env.target_id
        && tgt != local_device_id
    {
        return;
    }
    if !replay_guard.accept(sender_id, epoch_salt, seq) {
        return;
    }
    env.sender_id = sender_id.to_string();
    env.sender_name = sanitize_device_name(&env.sender_name);
    on_event(RelayEvent::Inbound {
        slot: room.slot,
        envelope: Box::new(env),
    });
}

fn push_mqtt_utf8(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&bytes[..usize::from(len)]);
}

fn encode_mqtt_packet(header: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(header);
    let mut rem = body.len();
    loop {
        let mut digit = (rem % 128) as u8;
        rem /= 128;
        if rem > 0 {
            digit |= 0x80;
        }
        out.push(digit);
        if rem == 0 {
            break;
        }
    }
    out.extend_from_slice(body);
    out
}

fn encode_mqtt_connect(client_id: &str, keepalive_secs: u16) -> Vec<u8> {
    let mut body = Vec::with_capacity(14 + client_id.len());
    push_mqtt_utf8(&mut body, "MQTT");
    body.push(4); // Protocol level 4 (MQTT 3.1.1)
    body.push(0x02); // Clean Session
    body.extend_from_slice(&keepalive_secs.to_be_bytes());
    push_mqtt_utf8(&mut body, client_id);
    encode_mqtt_packet(MQTT_CONNECT, &body)
}

fn encode_mqtt_subscribe(packet_id: u16, topics: &[String]) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + topics.len() * 44);
    body.extend_from_slice(&packet_id.to_be_bytes());
    for topic in topics {
        push_mqtt_utf8(&mut body, topic);
        body.push(0); // QoS 0
    }
    encode_mqtt_packet(MQTT_SUBSCRIBE_FLAGS, &body)
}

fn encode_mqtt_unsubscribe(packet_id: u16, topics: &[String]) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + topics.len() * 44);
    body.extend_from_slice(&packet_id.to_be_bytes());
    for topic in topics {
        push_mqtt_utf8(&mut body, topic);
    }
    encode_mqtt_packet(MQTT_UNSUBSCRIBE_FLAGS, &body)
}

fn encode_mqtt_publish(topic: &str, payload: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + topic.len() + payload.len());
    push_mqtt_utf8(&mut body, topic);
    body.extend_from_slice(payload);
    encode_mqtt_packet(MQTT_PUBLISH, &body)
}

fn try_pop_mqtt_packet(buf: &mut Vec<u8>) -> Option<(u8, Vec<u8>)> {
    if buf.len() < 2 {
        return None;
    }
    let pkt_type = buf[0];
    let mut mul = 1usize;
    let mut rem_len = 0usize;
    let mut idx = 1usize;
    loop {
        if idx >= buf.len() || idx > 4 {
            return None;
        }
        let b = buf[idx];
        idx += 1;
        rem_len += usize::from(b & 0x7F) * mul;
        if rem_len > MAX_FRAME_BYTES + 4096 {
            buf.clear();
            return None;
        }
        if (b & 0x80) == 0 {
            break;
        }
        mul *= 128;
    }
    if buf.len() < idx + rem_len {
        return None;
    }
    let body = buf[idx..idx + rem_len].to_vec();
    buf.drain(..idx + rem_len);
    Some((pkt_type, body))
}

fn parse_mqtt_publish_body(body: &[u8]) -> Option<(String, Vec<u8>)> {
    if body.len() < 2 {
        return None;
    }
    let topic_len = usize::from(u16::from_be_bytes([body[0], body[1]]));
    if body.len() < 2 + topic_len {
        return None;
    }
    let topic = String::from_utf8(body[2..2 + topic_len].to_vec()).ok()?;
    let payload = body[2 + topic_len..].to_vec();
    Some((topic, payload))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::crypto::derive_spotify_room;
    use std::net::TcpListener;

    /// Spawn a minimal in-process MQTT v3.1.1-over-WebSocket broker verifying topic fan-out.
    pub(crate) fn spawn_mock_mqtt_ws_broker() -> (String, Arc<AtomicBool>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock ws");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_srv = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            struct ClientConn {
                ws: WebSocket<TcpStream>,
                topics: Vec<String>,
            }
            let mut clients: Vec<ClientConn> = Vec::new();
            let mut bufs: Vec<Vec<u8>> = Vec::new();

            while !stop_srv.load(Ordering::Acquire) {
                while let Ok((stream, _)) = listener.accept() {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    #[allow(clippy::result_large_err)]
                    let callback = |req: &tungstenite::handshake::server::Request,
                                    mut resp: tungstenite::handshake::server::Response|
                     -> Result<
                        tungstenite::handshake::server::Response,
                        tungstenite::handshake::server::ErrorResponse,
                    > {
                        if req.headers().get("Sec-WebSocket-Protocol").is_some() {
                            resp.headers_mut()
                                .insert("Sec-WebSocket-Protocol", "mqtt".parse().expect("hdr"));
                        }
                        Ok(resp)
                    };
                    if let Ok(mut ws) = tungstenite::accept_hdr(stream, callback) {
                        let _ = ws
                            .get_mut()
                            .set_read_timeout(Some(Duration::from_millis(20)));
                        clients.push(ClientConn {
                            ws,
                            topics: Vec::new(),
                        });
                        bufs.push(Vec::new());
                    }
                }

                let mut pending_publishes: Vec<(usize, String, Vec<u8>)> = Vec::new();
                let mut dead = Vec::new();
                for (i, client) in clients.iter_mut().enumerate() {
                    match client.ws.read() {
                        Ok(Message::Binary(data)) => {
                            bufs[i].extend_from_slice(&data);
                            while let Some((ptype, body)) = try_pop_mqtt_packet(&mut bufs[i]) {
                                match ptype & 0xF0 {
                                    MQTT_CONNECT => {
                                        let _ = client.ws.send(Message::Binary(
                                            vec![MQTT_CONNACK, 2, 0, 0].into(),
                                        ));
                                    }
                                    MQTT_SUBSCRIBE => {
                                        if body.len() >= 4 {
                                            let pid = u16::from_be_bytes([body[0], body[1]]);
                                            let mut pos = 2usize;
                                            let mut acks = Vec::new();
                                            while pos + 2 < body.len() {
                                                let tlen = usize::from(u16::from_be_bytes([
                                                    body[pos],
                                                    body[pos + 1],
                                                ]));
                                                pos += 2;
                                                if pos + tlen + 1 > body.len() {
                                                    break;
                                                }
                                                let topic =
                                                    String::from_utf8_lossy(&body[pos..pos + tlen])
                                                        .into_owned();
                                                pos += tlen + 1;
                                                if !client.topics.contains(&topic) {
                                                    client.topics.push(topic);
                                                }
                                                acks.push(0u8);
                                            }
                                            let mut suback = Vec::with_capacity(2 + acks.len());
                                            suback.extend_from_slice(&pid.to_be_bytes());
                                            suback.extend_from_slice(&acks);
                                            let _ = client.ws.send(Message::Binary(
                                                encode_mqtt_packet(MQTT_SUBACK, &suback).into(),
                                            ));
                                        }
                                    }
                                    MQTT_PINGREQ => {
                                        let _ = client
                                            .ws
                                            .send(Message::Binary(vec![MQTT_PINGRESP, 0].into()));
                                    }
                                    MQTT_PUBLISH => {
                                        if let Some((topic, payload)) =
                                            parse_mqtt_publish_body(&body)
                                        {
                                            pending_publishes.push((i, topic, payload));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Ok(Message::Close(_)) => dead.push(i),
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(ref e))
                            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                        Err(_) => dead.push(i),
                    }
                }

                for (from_idx, topic, payload) in pending_publishes {
                    let pkt = encode_mqtt_publish(&topic, &payload);
                    for (j, client) in clients.iter_mut().enumerate() {
                        if j != from_idx && client.topics.iter().any(|t| t == &topic) {
                            let _ = client.ws.send(Message::Binary(pkt.clone().into()));
                        }
                    }
                }

                for idx in dead.into_iter().rev() {
                    clients.remove(idx);
                    bufs.remove(idx);
                }
                thread::sleep(Duration::from_millis(10));
            }
        });

        (format!("ws://127.0.0.1:{port}/mqtt"), stop, handle)
    }

    #[test]
    fn relay_client_exchanges_e2ee_envelopes_across_websocket_broker() {
        let (broker_url, stop, broker_handle) = spawn_mock_mqtt_ws_broker();
        let room = derive_spotify_room("RaflyCrossNet").expect("room");
        let topic = derive_relay_topic(&room.key);

        let rx_events_b: Arc<Mutex<Vec<RelayEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let rx_clone_b = Arc::clone(&rx_events_b);

        let client_a = RelayClient::start(
            "pc_01".to_string(),
            Some(vec![broker_url.clone()]),
            Arc::new(|_| {}),
        );
        let client_b = RelayClient::start(
            "phone_01".to_string(),
            Some(vec![broker_url]),
            Arc::new(move |ev| {
                rx_clone_b.lock().expect("lock").push(ev);
            }),
        );

        client_a.update_rooms(vec![RelayRoomSpec::from_key(
            MatchedRoomSlot::Spotify,
            room.room_key(),
        )]);
        client_b.update_rooms(vec![RelayRoomSpec::from_key(
            MatchedRoomSlot::Spotify,
            room.room_key(),
        )]);

        let mut both_connected = false;
        for _ in 0..80 {
            if client_a.is_connected() && client_b.is_connected() {
                both_connected = true;
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            both_connected,
            "both relay clients must connect to WS broker"
        );
        thread::sleep(Duration::from_millis(250));

        client_a.publish_envelope(
            &topic,
            room.room_key(),
            RelayEnvelope {
                sender_id: "pc_01".to_string(),
                sender_name: "Desktop-PC".to_string(),
                sender_kind: DeviceKind::Desktop,
                target_id: None,
                frame: SyncFrame::Hello {
                    device_id: "pc_01".to_string(),
                    device_name: "Desktop-PC".to_string(),
                    device_kind: DeviceKind::Desktop,
                },
            },
        );

        let mut received = false;
        for _ in 0..80 {
            let guard = rx_events_b.lock().expect("lock");
            if guard.iter().any(|ev| {
                matches!(
                    ev,
                    RelayEvent::Inbound {
                        slot: MatchedRoomSlot::Spotify,
                        envelope,
                    } if envelope.sender_id == "pc_01"
                )
            }) {
                received = true;
                break;
            }
            drop(guard);
            thread::sleep(Duration::from_millis(25));
        }
        assert!(
            received,
            "client_b must decrypt inbound E2EE envelope from client_a"
        );

        client_a.shutdown();
        client_b.shutdown();
        stop.store(true, Ordering::Release);
        let _ = broker_handle.join();
    }
}
