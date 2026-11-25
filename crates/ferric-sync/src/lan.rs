use crate::SyncResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, ToSocketAddrs, UdpSocket};

pub const FERRIC_LAN_UDP_PORT: u16 = 43821;

const BEACON_MAGIC: &[u8; 4] = b"FRC1";
const PEER_STALE_TIMEOUT_MS: u64 = 30_000;
const MAX_DATAGRAM_BYTES: usize = 1024;
const MAX_POLL_PACKETS_PER_TICK: usize = 64;
const MAX_DISCOVERED_PEERS: usize = 32;
const MAX_DEVICE_ID_LEN: usize = 64;
const MAX_DEVICE_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub device_name: String,
    pub addr: String,
    pub is_active_speaker: bool,
    pub last_seen_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BeaconAnnouncement {
    device_id: String,
    device_name: String,
    service_port: u16,
    is_active_speaker: bool,
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

    pub fn local_port(&self) -> u16 {
        self.port
    }

    fn encode_packet(
        &self,
        device_name: &str,
        service_port: u16,
        is_active_speaker: bool,
    ) -> SyncResult<Vec<u8>> {
        let msg = BeaconAnnouncement {
            device_id: self.local_device_id.clone(),
            device_name: device_name.to_string(),
            service_port,
            is_active_speaker,
        };
        let body = postcard::to_stdvec(&msg)?;
        let mut frame = Vec::with_capacity(BEACON_MAGIC.len() + body.len());
        frame.extend_from_slice(BEACON_MAGIC);
        frame.extend_from_slice(&body);
        Ok(frame)
    }

    /// Broadcast a LAN peer presence packet over `255.255.255.255:FERRIC_LAN_UDP_PORT`.
    pub fn broadcast_announce(
        &self,
        device_name: &str,
        service_port: u16,
        is_active_speaker: bool,
    ) -> SyncResult<()> {
        let packet = self.encode_packet(device_name, service_port, is_active_speaker)?;
        match self
            .socket
            .send_to(&packet, ("255.255.255.255", FERRIC_LAN_UDP_PORT))
        {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => {}
            Err(err) => return Err(err.into()),
        }
        if self.port != FERRIC_LAN_UDP_PORT {
            let _ = self.socket.send_to(&packet, ("255.255.255.255", self.port));
        }
        Ok(())
    }

    /// Send a directed presence announcement to a specific socket address (used for unicast/loopback peers).
    pub fn send_announce_to(
        &self,
        target: impl ToSocketAddrs,
        device_name: &str,
        service_port: u16,
        is_active_speaker: bool,
    ) -> SyncResult<()> {
        let packet = self.encode_packet(device_name, service_port, is_active_speaker)?;
        match self.socket.send_to(&packet, target) {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == ErrorKind::WouldBlock => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    /// Drain non-blocking UDP socket, update peer table, prune stale entries, and return active LAN peers.
    pub fn poll_peers(&mut self, now_ms: u64) -> Vec<DiscoveredPeer> {
        let mut buf = [0u8; MAX_DATAGRAM_BYTES];

        for _ in 0..MAX_POLL_PACKETS_PER_TICK {
            let (n, src_addr) = match self.socket.recv_from(&mut buf) {
                Ok(pair) => pair,
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(_) => break,
            };
            if n <= BEACON_MAGIC.len() || &buf[..4] != BEACON_MAGIC {
                continue;
            }
            let Ok(ann) = postcard::from_bytes::<BeaconAnnouncement>(&buf[4..n]) else {
                continue;
            };
            let clean_id = ann.device_id.trim();
            if clean_id.is_empty()
                || clean_id.len() > MAX_DEVICE_ID_LEN
                || !clean_id
                    .bytes()
                    .all(|b| b.is_ascii_graphic() && !b.is_ascii_control())
                || clean_id == self.local_device_id
            {
                continue;
            }

            // Drop unknown peers when table is saturated (existing peers still refresh).
            if !self.peers.contains_key(clean_id) && self.peers.len() >= MAX_DISCOVERED_PEERS {
                self.peers.retain(|_, peer| {
                    now_ms.saturating_sub(peer.last_seen_ms) <= PEER_STALE_TIMEOUT_MS
                });
                if self.peers.len() >= MAX_DISCOVERED_PEERS {
                    continue;
                }
            }

            let effective_port = if ann.service_port >= 1024 {
                ann.service_port
            } else {
                FERRIC_LAN_UDP_PORT
            };
            let peer_addr = format!("{}:{effective_port}", src_addr.ip());
            let sanitized_name: String = ann
                .device_name
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_DEVICE_NAME_CHARS)
                .collect();
            let device_name = if sanitized_name.trim().is_empty() {
                "Ferric Peer".to_string()
            } else {
                sanitized_name.trim().to_string()
            };

            self.peers.insert(
                clean_id.to_string(),
                DiscoveredPeer {
                    device_id: clean_id.to_string(),
                    device_name,
                    addr: peer_addr,
                    is_active_speaker: ann.is_active_speaker,
                    last_seen_ms: now_ms,
                },
            );
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

    #[test]
    fn lan_beacon_discovers_peer_and_prunes_stale_entries() {
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

        // Wait briefly for loopback datagram delivery
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
