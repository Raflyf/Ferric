pub mod crypto;
pub mod lan;
pub mod qr;
pub mod session;

pub use crypto::{
    AeadChannel, AeadReceiver, AeadSender, LocalRoomKeys, MatchedRoomSlot, RoomIdentity,
    decode_room_tags_hex, derive_qr_room, derive_spotify_room, derive_ytm_room,
    encode_room_tags_hex,
};
pub use lan::{
    DiscoveredPeer, FERRIC_LAN_UDP_PORT, LanDiscoveryBeacon, detect_local_lan_ip,
    is_lan_or_loopback, sanitize_device_name,
};
pub use qr::QrMatrix;
pub use session::{
    ConnectConfig, ConnectEngine, ConnectEvent, ConnectPeerInfo, ConnectSnapshot,
    LocalPlaybackSnapshot,
};

use ferric_core::models::{RepeatMode, Track, TrackSource};
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
    #[error("peer authentication failed")]
    AuthFailed,
    #[error("crypto error: {0}")]
    Crypto(&'static str),
    #[error("rejected non-LAN peer address: {0}")]
    NonLanPeer(String),
}

pub type SyncResult<T> = Result<T, SyncError>;

pub type DeviceId = String;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    #[default]
    Desktop,
    Phone,
}

impl DeviceKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Phone => "phone",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("phone") {
            Self::Phone
        } else {
            Self::Desktop
        }
    }
}

/// Wire track descriptor shared across Ferric Connect peers so the receiving device can resolve
/// and play any online song even when not yet cached locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub spotify_id: Option<String>,
    #[serde(default)]
    pub youtube_video_id: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

impl QueueEntry {
    #[must_use]
    pub fn from_track(track: &Track) -> Self {
        Self {
            track_id: track.id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            duration_ms: track.duration_ms,
            spotify_id: track.spotify_id.clone(),
            youtube_video_id: track.youtube_video_id.clone(),
            thumbnail_url: track.thumbnail_url.clone(),
        }
    }

    /// Convert a wire descriptor into a playable [`Track`] (online resolution if no local path exists).
    #[must_use]
    pub fn into_track(self) -> Track {
        let source = if self.youtube_video_id.is_some() || self.track_id.starts_with("yt:") {
            TrackSource::YouTubeMusic
        } else if self.spotify_id.is_some() || self.track_id.starts_with("sp:") {
            TrackSource::Spotify
        } else if self.track_id.starts_with("subsonic:") {
            TrackSource::Subsonic
        } else {
            TrackSource::Local
        };
        Track {
            id: self.track_id,
            title: self.title,
            artist: self.artist,
            album: self.album,
            duration_ms: self.duration_ms,
            path: None,
            source,
            codec: "OPUS".to_string(),
            sample_rate: 48_000,
            bit_depth: 16,
            bitrate_kbps: 128,
            isrc: None,
            spotify_id: self.spotify_id,
            youtube_video_id: self.youtube_video_id,
            thumbnail_url: self.thumbnail_url,
            replay_gain_db: 0,
            liked: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub revision: u64,
    pub active_speaker: Option<DeviceId>,
    pub is_playing: bool,
    pub track_id: Option<String>,
    #[serde(default)]
    pub track_title: String,
    #[serde(default)]
    pub track_artist: String,
    #[serde(default)]
    pub track_thumbnail_url: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
    pub base_position_ms: u64,
    pub anchor_utc_ms: i64,
    pub volume_norm: f32,
    pub repeat_mode: RepeatMode,
    pub shuffle_enabled: bool,
    #[serde(default)]
    pub device_name: String,
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
    PlayQueue {
        tracks: Vec<QueueEntry>,
        start_index: usize,
        context: String,
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
    Hello {
        device_id: DeviceId,
        device_name: String,
        device_kind: DeviceKind,
    },
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

/// Authority tuple used for deterministic active-speaker tie-break when two devices first connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerAuthority {
    pub device_id: DeviceId,
    pub is_playing: bool,
    pub revision: u64,
    pub anchor_utc_ms: i64,
}

/// Resolve active speaker authority deterministically when two devices connect:
/// `actively playing > highest revision > newest anchor_utc_ms > lexicographically smaller device_id`.
#[must_use]
pub fn resolve_active_speaker_tie_break(a: &SpeakerAuthority, b: &SpeakerAuthority) -> DeviceId {
    let rank_a = (a.is_playing, a.revision, a.anchor_utc_ms);
    let rank_b = (b.is_playing, b.revision, b.anchor_utc_ms);
    match rank_a.cmp(&rank_b) {
        std::cmp::Ordering::Greater => a.device_id.clone(),
        std::cmp::Ordering::Less => b.device_id.clone(),
        std::cmp::Ordering::Equal => {
            if a.device_id <= b.device_id {
                a.device_id.clone()
            } else {
                b.device_id.clone()
            }
        }
    }
}

/// Encode a `SyncFrame` with a 4-byte little-endian length prefix (`postcard` binary wire format).
pub fn encode_frame(frame: &SyncFrame) -> SyncResult<Vec<u8>> {
    validate_frame(frame)?;
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

/// Encode a raw `postcard` payload (without 4-byte length prefix) for AEAD encryption.
pub fn encode_frame_payload(frame: &SyncFrame) -> SyncResult<Vec<u8>> {
    validate_frame(frame)?;
    let payload = postcard::to_stdvec(frame)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(SyncError::FrameTooLarge(payload.len()));
    }
    Ok(payload)
}

/// Decode a decrypted raw `postcard` payload and enforce strict semantic bounds.
pub fn decode_frame_payload(payload: &[u8]) -> SyncResult<SyncFrame> {
    if payload.len() > MAX_FRAME_BYTES {
        return Err(SyncError::FrameTooLarge(payload.len()));
    }
    let frame: SyncFrame = postcard::from_bytes(payload)?;
    validate_frame(&frame)?;
    Ok(frame)
}

fn validate_frame(frame: &SyncFrame) -> SyncResult<()> {
    let valid_vol = |v: f32| v.is_finite() && (0.0..=1.5).contains(&v);
    match frame {
        SyncFrame::StateSnapshot(st) if !valid_vol(st.volume_norm) => Err(
            SyncError::InvalidPairingUri("out-of-range volume in StateSnapshot".to_string()),
        ),
        SyncFrame::PlaybackDelta { volume_norm, .. } if !valid_vol(*volume_norm) => Err(
            SyncError::InvalidPairingUri("out-of-range volume in PlaybackDelta".to_string()),
        ),
        SyncFrame::Command {
            action: RemoteAction::SetVolume { volume_norm, .. },
            ..
        } if !valid_vol(*volume_norm) => Err(SyncError::InvalidPairingUri(
            "out-of-range volume in SetVolume".to_string(),
        )),
        SyncFrame::Command {
            action: RemoteAction::PlayQueue { tracks, .. },
            ..
        } if tracks.len() > MAX_SYNC_QUEUE_TRACKS => Err(SyncError::FrameTooLarge(tracks.len())),
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

    #[must_use]
    pub fn is_local_active(&self) -> bool {
        self.active_speaker.as_deref() == Some(self.local_device_id.as_str())
    }

    pub fn transfer_to(&mut self, new_speaker: DeviceId) -> u64 {
        self.revision += 1;
        self.active_speaker = Some(new_speaker);
        self.revision
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PairingPayload {
    pub endpoint_id: String,
    pub token_hex: String,
    pub device_name: String,
}

impl std::fmt::Debug for PairingPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingPayload")
            .field("endpoint_id", &self.endpoint_id)
            .field(
                "token_hex",
                &if self.token_hex.is_empty() {
                    ""
                } else {
                    "[REDACTED]"
                },
            )
            .field("device_name", &self.device_name)
            .finish()
    }
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
    #[must_use]
    pub fn to_uri(&self) -> String {
        self.to_uri_with_addr(None)
    }

    #[must_use]
    pub fn to_uri_with_addr(&self, addr: Option<&str>) -> String {
        let mut uri = format!(
            "ferric://pair?id={}&tok={}&name={}",
            pct_encode(&self.endpoint_id),
            pct_encode(&self.token_hex),
            pct_encode(&self.device_name),
        );
        if let Some(a) = addr.map(str::trim).filter(|s| !s.is_empty()) {
            uri.push_str("&addr=");
            uri.push_str(&pct_encode(a));
        }
        uri
    }

    pub fn parse_uri(uri: &str) -> SyncResult<Self> {
        let (payload, _) = Self::parse_uri_with_addr(uri)?;
        Ok(payload)
    }

    pub fn parse_uri_with_addr(uri: &str) -> SyncResult<(Self, Option<String>)> {
        let query = uri
            .trim()
            .strip_prefix("ferric://pair?")
            .ok_or_else(|| SyncError::InvalidPairingUri(uri.to_string()))?;
        let mut endpoint_id = None;
        let mut token_hex = None;
        let mut device_name = None;
        let mut addr = None;
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                match k {
                    "id" => endpoint_id = Some(pct_decode(v)),
                    "tok" => token_hex = Some(pct_decode(v)),
                    "name" => device_name = Some(pct_decode(v)),
                    "addr" => addr = Some(pct_decode(v)),
                    _ => {}
                }
            }
        }
        let endpoint_id = endpoint_id
            .map(|s| s.trim().to_string())
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 64
                    && s.bytes()
                        .all(|b| b.is_ascii_graphic() && !b.is_ascii_control())
            })
            .ok_or_else(|| SyncError::InvalidPairingUri("invalid or empty id".to_string()))?;
        let token_hex = token_hex
            .map(|s| s.trim().to_string())
            .filter(|s| s.len() >= 16 && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| SyncError::InvalidPairingUri("invalid or empty token".to_string()))?;
        let clean_name = sanitize_device_name(device_name.as_deref().unwrap_or("Ferric Device"));
        let clean_addr = addr
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.len() <= 64);
        Ok((
            Self {
                endpoint_id,
                token_hex,
                device_name: clean_name,
            },
            clean_addr,
        ))
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
    fn pairing_uri_roundtrips_cleanly_with_optional_addr() {
        let payload = PairingPayload {
            endpoint_id: "ed25519_ferric_pc_01".to_string(),
            token_hex: "9f86d081884c7d65".to_string(),
            device_name: "Desktop PC".to_string(),
        };
        let uri = payload.to_uri();
        let parsed = PairingPayload::parse_uri(&uri).expect("parse uri");
        assert_eq!(parsed, payload);

        let uri_addr = payload.to_uri_with_addr(Some("192.168.1.42:51234"));
        let (parsed2, addr) = PairingPayload::parse_uri_with_addr(&uri_addr).expect("parse addr");
        assert_eq!(parsed2, payload);
        assert_eq!(addr.as_deref(), Some("192.168.1.42:51234"));
    }

    #[test]
    fn active_speaker_lease_transfers_cleanly() {
        let mut lease = ActiveSpeakerLease::new("desktop-pc", "Desktop-PC");
        assert!(lease.is_local_active());
        let rev = lease.transfer_to("pixel-9-pro".to_string());
        assert_eq!(rev, 2);
        assert!(!lease.is_local_active());
    }

    #[test]
    fn tie_break_prefers_playing_then_revision_then_anchor_then_smaller_id() {
        let a = SpeakerAuthority {
            device_id: "phone-b".to_string(),
            is_playing: true,
            revision: 1,
            anchor_utc_ms: 100,
        };
        let b = SpeakerAuthority {
            device_id: "desktop-a".to_string(),
            is_playing: false,
            revision: 99,
            anchor_utc_ms: 999,
        };
        assert_eq!(resolve_active_speaker_tie_break(&a, &b), "phone-b");

        let c = SpeakerAuthority {
            device_id: "phone-b".to_string(),
            is_playing: false,
            revision: 5,
            anchor_utc_ms: 500,
        };
        let d = SpeakerAuthority {
            device_id: "desktop-a".to_string(),
            is_playing: false,
            revision: 5,
            anchor_utc_ms: 500,
        };
        assert_eq!(resolve_active_speaker_tie_break(&c, &d), "desktop-a");
    }
}
