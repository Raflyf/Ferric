pub mod lan;
pub mod qr;

pub use lan::{DiscoveredPeer, FERRIC_LAN_UDP_PORT, LanDiscoveryBeacon, detect_local_lan_ip};
pub use qr::QrMatrix;

use ferric_core::models::RepeatMode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum wire frame length (1 MiB) to prevent memory exhaustion on malformed peers.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_SYNC_QUEUE_TRACKS: usize = 2_000;
const MAX_SIMULCAST_PARTICIPANTS: usize = 16;
pub const FERRIC_CONNECT_ALPN: &[u8] = b"ferric-connect/v1";
pub const FERRIC_MDNS_SERVICE: &str = "_ferric-connect._udp.local.";

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("postcard codec error: {0}")]
    Codec(#[from] postcard::Error),
    #[error("frame exceeds maximum size ({0} > {MAX_FRAME_BYTES})")]
    FrameTooLarge(usize),
    #[error("incomplete frame header or payload")]
    TruncatedFrame,
    #[error("invalid pairing uri: {0}")]
    InvalidPairingUri(String),
    #[error("qr payload error: {0}")]
    QrPayload(String),
}

pub type SyncResult<T> = Result<T, SyncError>;

pub type DeviceId = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub revision: u64,
    pub active_speaker: Option<DeviceId>,
    pub is_playing: bool,
    pub track_id: Option<String>,
    pub base_position_ms: u64,
    pub anchor_utc_ms: i64,
    pub volume_norm: f32,
    pub repeat_mode: RepeatMode,
    pub shuffle_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RemoteAction {
    Play,
    Pause,
    TogglePlayPause,
    SeekTo {
        position_ms: u64,
    },
    SkipNext,
    SkipPrev,
    JumpToQueueIndex {
        index: usize,
    },
    SetVolume {
        target_device: DeviceId,
        volume_norm: f32,
    },
    SetRepeat(RepeatMode),
    SetShuffle(bool),
    TransferActiveSpeaker {
        new_speaker: DeviceId,
        start_playing: bool,
    },
    SetSimulcastMode {
        enabled: bool,
        participants: Vec<DeviceId>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueueOp {
    Append(QueueEntry),
    RemoveAt(usize),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SyncFrame {
    Ping {
        client_send_mono_ms: u64,
        client_wall_utc_ms: i64,
    },
    Pong {
        client_send_mono_ms: u64,
        server_recv_utc_ms: i64,
        server_send_utc_ms: i64,
    },
    StateSnapshot(Box<SessionState>),
    PlaybackDelta {
        revision: u64,
        active_speaker: Option<DeviceId>,
        is_playing: bool,
        track_id: Option<String>,
        base_position_ms: u64,
        anchor_utc_ms: i64,
        volume_norm: f32,
        repeat_mode: RepeatMode,
        shuffle_enabled: bool,
    },
    Command {
        cmd_id: u64,
        action: RemoteAction,
    },
    QueueFull {
        revision: u64,
        tracks: Vec<QueueEntry>,
        current_index: usize,
    },
    QueueMutation {
        revision: u64,
        op: QueueOp,
    },
}

/// Encode a `SyncFrame` with a 4-byte little-endian length prefix (`postcard` binary wire format).
pub fn encode_frame(frame: &SyncFrame) -> SyncResult<Vec<u8>> {
    let payload = postcard::to_stdvec(frame)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(SyncError::FrameTooLarge(payload.len()));
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode a single length-prefixed `SyncFrame` with strict semantic bounds validation.
pub fn decode_frame(bytes: &[u8]) -> SyncResult<SyncFrame> {
    if bytes.len() < 4 {
        return Err(SyncError::TruncatedFrame);
    }
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(SyncError::FrameTooLarge(len));
    }
    if bytes.len() < 4 + len {
        return Err(SyncError::TruncatedFrame);
    }
    let frame: SyncFrame = postcard::from_bytes(&bytes[4..4 + len])?;
    validate_frame(&frame)?;
    Ok(frame)
}

fn validate_frame(frame: &SyncFrame) -> SyncResult<()> {
    match frame {
        SyncFrame::StateSnapshot(st) if !st.volume_norm.is_finite() => Err(
            SyncError::InvalidPairingUri("non-finite volume in StateSnapshot".to_string()),
        ),
        SyncFrame::PlaybackDelta { volume_norm, .. } if !volume_norm.is_finite() => Err(
            SyncError::InvalidPairingUri("non-finite volume in PlaybackDelta".to_string()),
        ),
        SyncFrame::Command {
            action: RemoteAction::SetVolume { volume_norm, .. },
            ..
        } if !volume_norm.is_finite() => Err(SyncError::InvalidPairingUri(
            "non-finite volume in SetVolume".to_string(),
        )),
        SyncFrame::Command {
            action: RemoteAction::SetSimulcastMode { participants, .. },
            ..
        } if participants.len() > MAX_SIMULCAST_PARTICIPANTS => {
            Err(SyncError::FrameTooLarge(participants.len()))
        }
        SyncFrame::QueueFull { tracks, .. } if tracks.len() > MAX_SYNC_QUEUE_TRACKS => {
            Err(SyncError::FrameTooLarge(tracks.len()))
        }
        _ => Ok(()),
    }
}

/// Active Speaker Lease tracker ensuring only one paired device outputs audio unless simulcast is active.
#[derive(Debug, Clone)]
pub struct ActiveSpeakerLease {
    pub local_device_id: DeviceId,
    pub local_device_name: String,
    pub active_speaker: Option<DeviceId>,
    pub revision: u64,
}

impl ActiveSpeakerLease {
    pub fn new(local_device_id: impl Into<String>, local_device_name: impl Into<String>) -> Self {
        let id = local_device_id.into();
        Self {
            active_speaker: Some(id.clone()),
            local_device_id: id,
            local_device_name: local_device_name.into(),
            revision: 1,
        }
    }

    pub fn is_local_active(&self) -> bool {
        self.active_speaker.as_deref() == Some(self.local_device_id.as_str())
    }

    pub fn transfer_to(&mut self, new_speaker: DeviceId) -> u64 {
        self.revision += 1;
        self.active_speaker = Some(new_speaker);
        self.revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingPayload {
    pub endpoint_id: String,
    pub token_hex: String,
    pub device_name: String,
}

fn pct_encode(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len() + 8);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0x0F)]));
        }
    }
    out
}

fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl PairingPayload {
    pub fn to_uri(&self) -> String {
        format!(
            "ferric://pair?id={}&tok={}&name={}",
            pct_encode(&self.endpoint_id),
            pct_encode(&self.token_hex),
            pct_encode(&self.device_name),
        )
    }

    pub fn parse_uri(uri: &str) -> SyncResult<Self> {
        let query = uri
            .strip_prefix("ferric://pair?")
            .ok_or_else(|| SyncError::InvalidPairingUri(uri.to_string()))?;
        let mut endpoint_id = None;
        let mut token_hex = None;
        let mut device_name = None;
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                match k {
                    "id" => endpoint_id = Some(pct_decode(v)),
                    "tok" => token_hex = Some(pct_decode(v)),
                    "name" => device_name = Some(pct_decode(v)),
                    _ => {}
                }
            }
        }
        Ok(Self {
            endpoint_id: endpoint_id
                .ok_or_else(|| SyncError::InvalidPairingUri(uri.to_string()))?,
            token_hex: token_hex.ok_or_else(|| SyncError::InvalidPairingUri(uri.to_string()))?,
            device_name: device_name.unwrap_or_else(|| "Ferric Device".to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_frame_postcard_framing_roundtrips_compactly() {
        let frame = SyncFrame::Command {
            cmd_id: 42,
            action: RemoteAction::TransferActiveSpeaker {
                new_speaker: "pixel-9-pro".to_string(),
                start_playing: true,
            },
        };
        let encoded = encode_frame(&frame).expect("encode");
        assert!(encoded.len() < 64);
        let decoded = decode_frame(&encoded).expect("decode");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn pairing_uri_roundtrips_cleanly() {
        let payload = PairingPayload {
            endpoint_id: "ed25519_ferric_pc_01".to_string(),
            token_hex: "9f86d081884c7d65".to_string(),
            device_name: "Desktop PC".to_string(),
        };
        let uri = payload.to_uri();
        let parsed = PairingPayload::parse_uri(&uri).expect("parse uri");
        assert_eq!(parsed, payload);
    }

    #[test]
    fn active_speaker_lease_transfers_cleanly() {
        let mut lease = ActiveSpeakerLease::new("desktop-pc", "Desktop-PC");
        assert!(lease.is_local_active());
        let rev = lease.transfer_to("pixel-9-pro".to_string());
        assert_eq!(rev, 2);
        assert!(!lease.is_local_active());
    }
}
