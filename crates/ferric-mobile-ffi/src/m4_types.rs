//! Milestone M4 UniFFI DTOs for Zero-Pairing Ferric Connect & QR Guest Pairing.

use ferric_sync::{ConnectPeerInfo, ConnectSnapshot, DeviceKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FfiDeviceKind {
    Desktop,
    Phone,
}

impl From<DeviceKind> for FfiDeviceKind {
    fn from(k: DeviceKind) -> Self {
        match k {
            DeviceKind::Desktop => Self::Desktop,
            DeviceKind::Phone => Self::Phone,
        }
    }
}

impl From<FfiDeviceKind> for DeviceKind {
    fn from(k: FfiDeviceKind) -> Self {
        match k {
            FfiDeviceKind::Desktop => Self::Desktop,
            FfiDeviceKind::Phone => Self::Phone,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiConnectPeer {
    pub device_id: String,
    pub device_name: String,
    pub device_kind: FfiDeviceKind,
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

impl From<ConnectPeerInfo> for FfiConnectPeer {
    fn from(p: ConnectPeerInfo) -> Self {
        Self {
            device_id: p.device_id,
            device_name: p.device_name,
            device_kind: p.device_kind.into(),
            addr: p.addr,
            connected: p.connected,
            is_active_speaker: p.is_active_speaker,
            is_playing: p.is_playing,
            volume_norm: p.volume_norm,
            now_playing_title: p.now_playing_title,
            now_playing_artist: p.now_playing_artist,
            now_playing_thumbnail_url: p.now_playing_thumbnail_url,
            position_ms: p.position_ms,
            duration_ms: p.duration_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FfiConnectState {
    pub local_device_id: String,
    pub local_device_name: String,
    pub local_device_kind: FfiDeviceKind,
    pub local_ip: String,
    pub local_tcp_port: u16,
    pub room_tags_hex: String,
    pub active_speaker_id: String,
    pub is_local_active_speaker: bool,
    pub is_account_ready: bool,
    pub peers: Vec<FfiConnectPeer>,
}

impl From<ConnectSnapshot> for FfiConnectState {
    fn from(s: ConnectSnapshot) -> Self {
        Self {
            local_device_id: s.local_device_id,
            local_device_name: s.local_device_name,
            local_device_kind: s.local_device_kind.into(),
            local_ip: s.local_ip,
            local_tcp_port: s.local_tcp_port,
            room_tags_hex: s.room_tags_hex,
            active_speaker_id: s.active_speaker_id,
            is_local_active_speaker: s.is_local_active_speaker,
            is_account_ready: s.is_account_ready,
            peers: s.peers.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FfiQrCode {
    pub uri: String,
    pub size: u32,
    pub modules: Vec<bool>,
}
