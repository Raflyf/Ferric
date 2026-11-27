use crate::{DeviceKind, SyncResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, ToSocketAddrs, UdpSocket};

pub const FERRIC_LAN_UDP_PORT: u16 = 43821;

const BEACON_MAGIC_V1: &[u8; 4] = b"FRC1";
const BEACON_MAGIC_V2: &[u8; 4] = b"FRC2";
const PEER_STALE_TIMEOUT_MS: u64 = 30_000;
const MAX_DATAGRAM_BYTES: usize = 1024;
const MAX_POLL_PACKETS_PER_TICK: usize = 64;
const MAX_DISCOVERED_PEERS: usize = 32;
const MAX_PEERS_PER_IP: usize = 2;
const MAX_DEVICE_ID_LEN: usize = 64;
const MAX_DEVICE_NAME_CHARS: usize = 64;
const MAX_NOW_PLAYING_CHARS: usize = 96;

#[must_use]
pub fn is_lan_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || ((v6.segments()[0] & 0xfe00) == 0xfc00) // ULA fc00::/7
                || ((v6.segments()[0] & 0xffc0) == 0xfe80) // link-local fe80::/10
        }
    }
}

fn is_bidi_or_control(c: char) -> bool {
    c.is_control()
        || matches!(
            c as u32,
            0x061C
                | 0x200E
                | 0x200F
                | 0x202A..=0x202E
                | 0x2066..=0x2069
                | 0xFEFF
        )
}

#[must_use]
pub fn sanitize_device_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !is_bidi_or_control(*c))
        .take(MAX_DEVICE_NAME_CHARS)
        .collect();
    if cleaned.trim().is_empty() {
        "Ferric Peer".to_string()
    } else {
        cleaned.trim().to_string()
    }
}

fn sanitize_short_text(raw: &str) -> String {
    raw.chars()
        .filter(|c| !is_bidi_or_control(*c))
        .take(MAX_NOW_PLAYING_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub device_name: String,
    #[serde(default)]
    pub device_kind: DeviceKind,
    pub addr: String,
    #[serde(default)]
    pub tcp_port: u16,
    #[serde(default)]
    pub room_tags: [Option<[u8; 16]>; 2],
    pub is_active_speaker: bool,
    #[serde(default)]
    pub now_playing_title: String,
    #[serde(default)]
    pub now_playing_artist: String,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BeaconAnnouncementV1 {
    device_id: String,
    device_name: String,
    service_port: u16,
    is_active_speaker: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BeaconAnnouncementV2 {
    device_id: String,
    device_name: String,
    device_kind: DeviceKind,
    room_tags: [Option<[u8; 16]>; 2],
    tcp_port: u16,
    is_active_speaker: bool,
    now_playing_title: String,
    now_playing_artist: String,
}

#[derive(Debug)]
pub struct LanDiscoveryBeacon {
    socket: UdpSocket,
    local_device_id: String,
    port: u16,
    peers: HashMap<String, DiscoveredPeer>,
}

impl LanDiscoveryBeacon {
    /// Bind a non-blocking UDP discovery socket on `0.0.0.0:{port}` (falling back to ephemeral port if busy).
    pub fn bind_nonblocking(local_device_id: impl Into<String>, port: u16) -> SyncResult<Self> {
        let socket = match UdpSocket::bind(("0.0.0.0", port)) {
            Ok(sock) => sock,
            Err(err) if err.kind() == ErrorKind::AddrInUse && port != 0 => {
                UdpSocket::bind(("0.0.0.0", 0))?
            }
            Err(err) => return Err(err.into()),
        };
        socket.set_nonblocking(true)?;
        let _ = socket.set_broadcast(true);

        let actual_port = socket.local_addr()?.port();
        Ok(Self {
            socket,
            local_device_id: local_device_id.into(),
            port: actual_port,
            peers: HashMap::new(),
        })
    }

    /// Bind on the default `FERRIC_LAN_UDP_PORT` (`43821`).
    pub fn bind_default(local_device_id: impl Into<String>) -> SyncResult<Self> {
        Self::bind_nonblocking(local_device_id, FERRIC_LAN_UDP_PORT)
    }

    #[must_use]
    pub fn local_port(&self) -> u16 {
        self.port
    }

    fn encode_v1_packet(
        &self,
        device_name: &str,
        service_port: u16,
        is_active_speaker: bool,
    ) -> SyncResult<Vec<u8>> {
        let msg = BeaconAnnouncementV1 {
            device_id: self.local_device_id.clone(),
            device_name: device_name.to_string(),
            service_port,
            is_active_speaker,
        };
        let body = postcard::to_stdvec(&msg)?;
        let mut frame = Vec::with_capacity(BEACON_MAGIC_V1.len() + body.len());
        frame.extend_from_slice(BEACON_MAGIC_V1);
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_v2_packet(
        &self,
        device_name: &str,
        device_kind: DeviceKind,
        room_tags: [Option<[u8; 16]>; 2],
        tcp_port: u16,
        is_active_speaker: bool,
        now_playing_title: &str,
        now_playing_artist: &str,
    ) -> SyncResult<Vec<u8>> {
        let msg = BeaconAnnouncementV2 {
            device_id: self.local_device_id.clone(),
            device_name: sanitize_device_name(device_name),
            device_kind,
            room_tags,
            tcp_port,
            is_active_speaker,
            now_playing_title: sanitize_short_text(now_playing_title),
            now_playing_artist: sanitize_short_text(now_playing_artist),
        };
        let body = postcard::to_stdvec(&msg)?;
        let mut frame = Vec::with_capacity(BEACON_MAGIC_V2.len() + body.len());
        frame.extend_from_slice(BEACON_MAGIC_V2);
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    /// Broadcast a legacy `FRC1` LAN peer presence packet.
    pub fn broadcast_announce(
        &self,
        device_name: &str,
        service_port: u16,
        is_active_speaker: bool,
    ) -> SyncResult<()> {
        let packet = self.encode_v1_packet(device_name, service_port, is_active_speaker)?;
        self.send_broadcast_packet(&packet)
    }

    /// Broadcast an `FRC2` dual-tag account discovery packet (zero raw PII on LAN).
    #[allow(clippy::too_many_arguments)]
    pub fn broadcast_announce_v2(
        &self,
        device_name: &str,
        device_kind: DeviceKind,
        room_tags: [Option<[u8; 16]>; 2],
        tcp_port: u16,
        is_active_speaker: bool,
        now_playing_title: &str,
        now_playing_artist: &str,
    ) -> SyncResult<()> {
        let packet = self.encode_v2_packet(
            device_name,
            device_kind,
            room_tags,
            tcp_port,
            is_active_speaker,
            now_playing_title,
            now_playing_artist,
        )?;
        self.send_broadcast_packet(&packet)
    }

    fn send_broadcast_packet(&self, packet: &[u8]) -> SyncResult<()> {
        match self
            .socket
            .send_to(packet, ("255.255.255.255", FERRIC_LAN_UDP_PORT))
        {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => {}
            Err(err) => return Err(err.into()),
        }
        if self.port != FERRIC_LAN_UDP_PORT {
            let _ = self.socket.send_to(packet, ("255.255.255.255", self.port));
        }
        Ok(())
    }

    /// Send a directed legacy presence announcement (unicast/loopback).
    pub fn send_announce_to(
        &self,
        target: impl ToSocketAddrs,
        device_name: &str,
        service_port: u16,
        is_active_speaker: bool,
    ) -> SyncResult<()> {
        let packet = self.encode_v1_packet(device_name, service_port, is_active_speaker)?;
        match self.socket.send_to(&packet, target) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Send a directed `FRC2` announcement (unicast/loopback).
    #[allow(clippy::too_many_arguments)]
    pub fn send_announce_v2_to(
        &self,
        target: impl ToSocketAddrs,
        device_name: &str,
        device_kind: DeviceKind,
        room_tags: [Option<[u8; 16]>; 2],
        tcp_port: u16,
        is_active_speaker: bool,
        now_playing_title: &str,
        now_playing_artist: &str,
    ) -> SyncResult<()> {
        let packet = self.encode_v2_packet(
            device_name,
            device_kind,
            room_tags,
            tcp_port,
            is_active_speaker,
            now_playing_title,
            now_playing_artist,
        )?;
        match self.socket.send_to(&packet, target) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Ingest a peer discovered via OS mDNS / Android `NsdManager` (`_ferric-connect._udp.local.`).
    #[allow(clippy::too_many_arguments)]
    pub fn ingest_mdns_peer(
        &mut self,
        device_id: &str,
        device_name: &str,
        device_kind: DeviceKind,
        ip: IpAddr,
        tcp_port: u16,
        room_tags: [Option<[u8; 16]>; 2],
        now_ms: u64,
    ) -> bool {
        if !is_lan_or_loopback(ip) || tcp_port < 1024 {
            return false;
        }
        self.upsert_peer(
            device_id,
            device_name,
            device_kind,
            ip,
            tcp_port,
            room_tags,
            false,
            "",
            "",
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_peer(
        &mut self,
        raw_id: &str,
        raw_name: &str,
        device_kind: DeviceKind,
        src_ip: IpAddr,
        service_port: u16,
        room_tags: [Option<[u8; 16]>; 2],
        is_active_speaker: bool,
        now_playing_title: &str,
        now_playing_artist: &str,
        now_ms: u64,
    ) -> bool {
        let clean_id = raw_id.trim();
        if clean_id.is_empty()
            || clean_id.len() > MAX_DEVICE_ID_LEN
            || !clean_id
                .bytes()
                .all(|b| b.is_ascii_graphic() && !b.is_ascii_control())
            || clean_id == self.local_device_id
        {
            return false;
        }

        let src_ip_str = src_ip.to_string();
        if let Some(existing) = self.peers.get(clean_id) {
            let existing_ip = existing
                .addr
                .rsplit_once(':')
                .map(|(ip, _)| ip)
                .unwrap_or("");
            let is_fresh = now_ms.saturating_sub(existing.last_seen_ms) <= PEER_STALE_TIMEOUT_MS;
            if is_fresh && existing_ip != src_ip_str {
                return false;
            }
        } else {
            let same_ip_count = self
                .peers
                .values()
                .filter(|p| {
                    now_ms.saturating_sub(p.last_seen_ms) <= PEER_STALE_TIMEOUT_MS
                        && p.addr
                            .rsplit_once(':')
                            .is_some_and(|(ip, _)| ip == src_ip_str)
                })
                .count();
            if same_ip_count >= MAX_PEERS_PER_IP {
                return false;
            }
        }

        if !self.peers.contains_key(clean_id) && self.peers.len() >= MAX_DISCOVERED_PEERS {
            self.peers.retain(|_, peer| {
                now_ms.saturating_sub(peer.last_seen_ms) <= PEER_STALE_TIMEOUT_MS
            });
            if self.peers.len() >= MAX_DISCOVERED_PEERS {
                return false;
            }
        }

        let effective_port = if service_port >= 1024 {
            service_port
        } else {
            FERRIC_LAN_UDP_PORT
        };
        let peer_addr = format!("{src_ip_str}:{effective_port}");
        let device_name = sanitize_device_name(raw_name);

        let prev = self.peers.get(clean_id);
        let merged_title = if now_playing_title.is_empty() {
            prev.map(|p| p.now_playing_title.clone())
                .unwrap_or_default()
        } else {
            sanitize_short_text(now_playing_title)
        };
        let merged_artist = if now_playing_artist.is_empty() {
            prev.map(|p| p.now_playing_artist.clone())
                .unwrap_or_default()
        } else {
            sanitize_short_text(now_playing_artist)
        };

        self.peers.insert(
            clean_id.to_string(),
            DiscoveredPeer {
                device_id: clean_id.to_string(),
                device_name,
                device_kind,
                addr: peer_addr,
                tcp_port: effective_port,
                room_tags,
                is_active_speaker,
                now_playing_title: merged_title,
                now_playing_artist: merged_artist,
                last_seen_ms: now_ms,
            },
        );
        true
    }

    /// Drain non-blocking UDP socket, update peer table, prune stale entries, and return active LAN peers.
    pub fn poll_peers(&mut self, now_ms: u64) -> Vec<DiscoveredPeer> {
        let mut buf = [0u8; MAX_DATAGRAM_BYTES];

        for _ in 0..MAX_POLL_PACKETS_PER_TICK {
            let (n, src_addr) = match self.socket.recv_from(&mut buf) {
                Ok(pair) => pair,
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err)
                    if matches!(
                        err.kind(),
                        ErrorKind::ConnectionReset | ErrorKind::Interrupted
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
            };
            if !is_lan_or_loopback(src_addr.ip()) || n <= 4 {
                continue;
            }
            let magic = &buf[..4];
            if magic == BEACON_MAGIC_V2 {
                if let Ok(ann) = postcard::from_bytes::<BeaconAnnouncementV2>(&buf[4..n]) {
                    self.upsert_peer(
                        &ann.device_id,
                        &ann.device_name,
                        ann.device_kind,
                        src_addr.ip(),
                        ann.tcp_port,
                        ann.room_tags,
                        ann.is_active_speaker,
                        &ann.now_playing_title,
                        &ann.now_playing_artist,
                        now_ms,
                    );
                }
            } else if magic == BEACON_MAGIC_V1
                && let Ok(ann) = postcard::from_bytes::<BeaconAnnouncementV1>(&buf[4..n])
            {
                self.upsert_peer(
                    &ann.device_id,
                    &ann.device_name,
                    DeviceKind::Desktop,
                    src_addr.ip(),
                    ann.service_port,
                    [None, None],
                    ann.is_active_speaker,
                    "",
                    "",
                    now_ms,
                );
            }
        }

        self.peers
            .retain(|_, peer| now_ms.saturating_sub(peer.last_seen_ms) <= PEER_STALE_TIMEOUT_MS);

        let mut list: Vec<DiscoveredPeer> = self.peers.values().cloned().collect();
        list.sort_by(|a, b| {
            a.device_name
                .cmp(&b.device_name)
                .then_with(|| a.device_id.cmp(&b.device_id))
        });
        list
    }
}

/// Detect the primary local LAN IPv4 address via OS routing table lookup (`connect("1.1.1.1:80")` on UDP sends zero packets).
#[must_use]
pub fn detect_local_lan_ip() -> String {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return "127.0.0.1".to_string();
    };
    if sock.connect("1.1.1.1:80").is_err() {
        return "127.0.0.1".to_string();
    }
    match sock.local_addr() {
        Ok(addr) => match addr.ip() {
            IpAddr::V4(v4) if !v4.is_unspecified() => v4.to_string(),
            IpAddr::V6(v6) if !v6.is_unspecified() => v6.to_string(),
            _ => "127.0.0.1".to_string(),
        },
        Err(_) => "127.0.0.1".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::derive_spotify_room;

    #[test]
    fn lan_beacon_discovers_v1_and_v2_peers_and_prunes_stale_entries() {
        let mut beacon_a = LanDiscoveryBeacon::bind_nonblocking("dev_desktop", 0).expect("bind a");
        let beacon_b = LanDiscoveryBeacon::bind_nonblocking("dev_laptop", 0).expect("bind b");

        beacon_b
            .send_announce_to(
                ("127.0.0.1", beacon_a.local_port()),
                "Studio Laptop",
                43822,
                true,
            )
            .expect("send unicast announce");

        let mut discovered = Vec::new();
        for _ in 0..10 {
            discovered = beacon_a.poll_peers(1_000);
            if !discovered.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].device_id, "dev_laptop");
        assert_eq!(discovered[0].device_name, "Studio Laptop");
        assert_eq!(discovered[0].addr, "127.0.0.1:43822");
        assert!(discovered[0].is_active_speaker);

        // FRC2 dual-tag announcement updates peer with room_tags & kind
        let sp = derive_spotify_room("rafly").expect("sp room");
        beacon_b
            .send_announce_v2_to(
                ("127.0.0.1", beacon_a.local_port()),
                "Pixel Phone",
                DeviceKind::Phone,
                [Some(sp.tag), None],
                51234,
                false,
                "Locked Out of Heaven",
                "Bruno Mars",
            )
            .expect("send v2");

        for _ in 0..10 {
            discovered = beacon_a.poll_peers(2_000);
            if discovered
                .first()
                .is_some_and(|p| p.device_kind == DeviceKind::Phone)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(discovered[0].device_kind, DeviceKind::Phone);
        assert_eq!(discovered[0].tcp_port, 51234);
        assert_eq!(discovered[0].room_tags[0], Some(sp.tag));
        assert_eq!(discovered[0].now_playing_title, "Locked Out of Heaven");

        // Stale peer after > 30s
        let after_expiry = beacon_a.poll_peers(35_000);
        assert!(after_expiry.is_empty());
    }

    #[test]
    fn detect_local_lan_ip_returns_valid_ip_string() {
        let ip = detect_local_lan_ip();
        assert!(ip.parse::<IpAddr>().is_ok(), "invalid ip: {ip}");
    }
}
