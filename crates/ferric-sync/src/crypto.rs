//! Pure-Rust HMAC-SHA256, HKDF-SHA256, dual-provider account room identity, and ChaCha20-Poly1305
//! AEAD framing with monotonic directional nonces for Zero-Pairing Ferric Connect.

use crate::{MAX_FRAME_BYTES, SyncError, SyncResult};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use ferric_core::secrets::zeroize_bytes;
use sha2::{Digest, Sha256};

const ACCOUNT_ROOM_SALT: &[u8] = b"Ferric.Connect.AccountRoom.v1";
const QR_ROOM_SALT: &[u8] = b"Ferric.Connect.QrRoom.v1";
const SESSION_KEY_INFO: &[u8] = b"session-key";
const ROOM_TAG_LABEL: &[u8] = b"ferric-room-tag-v1";
const RELAY_TOPIC_LABEL: &[u8] = b"ferric-relay-topic-v1";
const RELAY_MSG_INFO: &[u8] = b"ferric-relay-msg-v1";
const RELAY_FRAME_MAGIC: &[u8; 4] = b"FCR1";
const RELAY_HEADER_LEN: usize = 4 + 16 + 8;
const HANDSHAKE_DOMAIN: &[u8] = b"ferric-connect-hs-v1";
const DIR_INIT_TO_RESP: u8 = 0x01;
const DIR_RESP_TO_INIT: u8 = 0x02;
const POLY1305_TAG_LEN: usize = 16;

/// Constant-time equality comparison for fixed-length MACs and tags.
#[must_use]
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// RFC 2104 HMAC-SHA256.
#[must_use]
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut k_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= k_block[i];
        opad[i] ^= k_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let out = outer.finalize();

    zeroize_bytes(&mut k_block);
    zeroize_bytes(&mut ipad);
    zeroize_bytes(&mut opad);

    let mut mac = [0u8; 32];
    mac.copy_from_slice(&out);
    mac
}

/// RFC 5869 HKDF-Extract(salt, IKM) -> PRK.
#[must_use]
pub fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    if salt.is_empty() {
        hmac_sha256(&[0u8; 32], ikm)
    } else {
        hmac_sha256(salt, ikm)
    }
}

/// RFC 5869 HKDF-Expand(PRK, info, 32).
#[must_use]
pub fn hkdf_expand_32(prk: &[u8; 32], info: &[u8]) -> [u8; 32] {
    let mut input = Vec::with_capacity(info.len() + 1);
    input.extend_from_slice(info);
    input.push(1u8);
    hmac_sha256(prk, &input)
}

/// Single-shot RFC 5869 HKDF-SHA256 deriving a 32-byte key.
#[must_use]
pub fn hkdf_sha256_32(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let mut prk = hkdf_extract(salt, ikm);
    let okm = hkdf_expand_32(&prk, info);
    zeroize_bytes(&mut prk);
    okm
}

/// Zeroize-on-drop wrapper for a 32-byte room key passed across handshake threads.
#[derive(Clone, PartialEq, Eq)]
pub struct RoomKey([u8; 32]);

impl RoomKey {
    #[must_use]
    pub fn from_bytes(src: &[u8; 32]) -> Self {
        let mut out = [0u8; 32];
        out.copy_from_slice(src);
        Self(out)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for RoomKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RoomKey([REDACTED])")
    }
}

impl Drop for RoomKey {
    fn drop(&mut self) {
        zeroize_bytes(&mut self.0);
    }
}

/// Deterministic 32-byte room key + 16-byte opaque LAN discovery tag (zero PII on the wire).
#[derive(Clone, PartialEq, Eq)]
pub struct RoomIdentity {
    pub key: [u8; 32],
    pub tag: [u8; 16],
}

impl RoomIdentity {
    #[must_use]
    pub fn room_key(&self) -> RoomKey {
        RoomKey::from_bytes(&self.key)
    }
}

impl std::fmt::Debug for RoomIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoomIdentity")
            .field("key", &"[REDACTED]")
            .field("tag_hex", &hex_encode(&self.tag))
            .finish()
    }
}

impl Drop for RoomIdentity {
    fn drop(&mut self) {
        zeroize_bytes(&mut self.key);
    }
}

fn derive_identity(ikm: &[u8], salt: &[u8]) -> RoomIdentity {
    let key = hkdf_sha256_32(ikm, salt, SESSION_KEY_INFO);
    let full_tag = hmac_sha256(&key, ROOM_TAG_LABEL);
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&full_tag[..16]);
    RoomIdentity { key, tag }
}

/// Derive the Spotify account room key + 16-byte public discovery tag.
#[must_use]
pub fn derive_spotify_room(spotify_user_id: &str) -> Option<RoomIdentity> {
    let norm = spotify_user_id.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return None;
    }
    let mut ikm = format!("spotify:{norm}");
    let id = derive_identity(ikm.as_bytes(), ACCOUNT_ROOM_SALT);
    ferric_core::secrets::zeroize_string(&mut ikm);
    Some(id)
}

/// Derive the YouTube Music account room key + 16-byte public discovery tag.
#[must_use]
pub fn derive_ytm_room(youtube_account_id: &str) -> Option<RoomIdentity> {
    let norm = youtube_account_id.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return None;
    }
    let mut ikm = format!("ytm:{norm}");
    let id = derive_identity(ikm.as_bytes(), ACCOUNT_ROOM_SALT);
    ferric_core::secrets::zeroize_string(&mut ikm);
    Some(id)
}

/// Derive a Guest Mode room key + tag from a QR pairing token (`ferric://pair?tok=...`).
#[must_use]
pub fn derive_qr_room(token_hex: &str) -> Option<RoomIdentity> {
    let norm = token_hex.trim().to_ascii_lowercase();
    if norm.len() < 16 || !norm.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut ikm = format!("qr:{norm}");
    let id = derive_identity(ikm.as_bytes(), QR_ROOM_SALT);
    ferric_core::secrets::zeroize_string(&mut ikm);
    Some(id)
}

/// Local device's active room keys (up to two account providers plus optional Guest QR room).
#[derive(Debug, Clone, Default)]
pub struct LocalRoomKeys {
    pub spotify: Option<RoomIdentity>,
    pub ytm: Option<RoomIdentity>,
    pub guest: Option<RoomIdentity>,
}

/// Selector indicating which room key matched during LAN discovery / handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum MatchedRoomSlot {
    Spotify = 0,
    YouTubeMusic = 1,
    GuestQr = 2,
}

impl MatchedRoomSlot {
    #[must_use]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Spotify),
            1 => Some(Self::YouTubeMusic),
            2 => Some(Self::GuestQr),
            _ => None,
        }
    }
}

impl LocalRoomKeys {
    #[must_use]
    pub fn from_accounts(spotify_user_id: &str, youtube_account_id: &str) -> Self {
        Self {
            spotify: derive_spotify_room(spotify_user_id),
            ytm: derive_ytm_room(youtube_account_id),
            guest: None,
        }
    }

    #[must_use]
    pub fn has_any_room(&self) -> bool {
        self.spotify.is_some() || self.ytm.is_some() || self.guest.is_some()
    }

    /// Up to 2 opaque 16-byte discovery tags advertised in `FRC2` UDP beacons and mDNS TXT records.
    /// Slot 0 carries Spotify (or Guest QR if Spotify is absent); Slot 1 carries YouTube Music (or
    /// Guest QR when Spotify is present and YTM is absent so simultaneous Guest QR pairing remains discoverable).
    #[must_use]
    pub fn advertised_tags(&self) -> [Option<[u8; 16]>; 2] {
        let slot0 = self.spotify.as_ref().or(self.guest.as_ref()).map(|r| r.tag);
        let slot1 = self
            .ytm
            .as_ref()
            .or(if self.spotify.is_some() {
                self.guest.as_ref()
            } else {
                None
            })
            .map(|r| r.tag);
        [slot0, slot1]
    }

    /// Deterministically pick the highest-priority shared room key with a remote peer's advertised tags:
    /// Spotify > YouTube Music > Guest QR.
    #[must_use]
    pub fn match_remote_tags(
        &self,
        remote: &[Option<[u8; 16]>; 2],
    ) -> Option<(MatchedRoomSlot, [u8; 32])> {
        if let Some(sp) = &self.spotify
            && remote.iter().flatten().any(|tag| ct_eq(tag, &sp.tag))
        {
            return Some((MatchedRoomSlot::Spotify, sp.key));
        }
        if let Some(yt) = &self.ytm
            && remote.iter().flatten().any(|tag| ct_eq(tag, &yt.tag))
        {
            return Some((MatchedRoomSlot::YouTubeMusic, yt.key));
        }
        if let Some(guest) = &self.guest
            && remote.iter().flatten().any(|tag| ct_eq(tag, &guest.tag))
        {
            return Some((MatchedRoomSlot::GuestQr, guest.key));
        }
        None
    }

    #[must_use]
    pub fn key_for_slot(&self, slot: MatchedRoomSlot) -> Option<[u8; 32]> {
        match slot {
            MatchedRoomSlot::Spotify => self.spotify.as_ref().map(|r| r.key),
            MatchedRoomSlot::YouTubeMusic => self.ytm.as_ref().map(|r| r.key),
            MatchedRoomSlot::GuestQr => self.guest.as_ref().map(|r| r.key),
        }
    }

    /// Match remote discovery tags using a Drop-zeroizing [`RoomKey`] and return the matched tag.
    #[must_use]
    pub fn match_remote_tags_zeroizing(
        &self,
        remote: &[Option<[u8; 16]>; 2],
    ) -> Option<(MatchedRoomSlot, [u8; 16], RoomKey)> {
        if let Some(sp) = &self.spotify
            && remote.iter().flatten().any(|tag| ct_eq(tag, &sp.tag))
        {
            return Some((MatchedRoomSlot::Spotify, sp.tag, sp.room_key()));
        }
        if let Some(yt) = &self.ytm
            && remote.iter().flatten().any(|tag| ct_eq(tag, &yt.tag))
        {
            return Some((MatchedRoomSlot::YouTubeMusic, yt.tag, yt.room_key()));
        }
        if let Some(guest) = &self.guest
            && remote.iter().flatten().any(|tag| ct_eq(tag, &guest.tag))
        {
            return Some((MatchedRoomSlot::GuestQr, guest.tag, guest.room_key()));
        }
        None
    }

    /// Return the Drop-zeroizing [`RoomKey`] and discovery tag for a specific room slot.
    #[must_use]
    pub fn key_and_tag_for_slot(&self, slot: MatchedRoomSlot) -> Option<(RoomKey, [u8; 16])> {
        match slot {
            MatchedRoomSlot::Spotify => self.spotify.as_ref().map(|r| (r.room_key(), r.tag)),
            MatchedRoomSlot::YouTubeMusic => self.ytm.as_ref().map(|r| (r.room_key(), r.tag)),
            MatchedRoomSlot::GuestQr => self.guest.as_ref().map(|r| (r.room_key(), r.tag)),
        }
    }

    /// Check in constant time whether `(slot, tag)` still matches an active local room key.
    #[must_use]
    pub fn matches_slot_tag(&self, slot: MatchedRoomSlot, tag: &[u8; 16]) -> bool {
        let current = match slot {
            MatchedRoomSlot::Spotify => self.spotify.as_ref(),
            MatchedRoomSlot::YouTubeMusic => self.ytm.as_ref(),
            MatchedRoomSlot::GuestQr => self.guest.as_ref(),
        };
        current.is_some_and(|r| ct_eq(&r.tag, tag))
    }
}

#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

#[must_use]
pub fn decode_tag16_hex(s: &str) -> Option<[u8; 16]> {
    let clean = s.trim();
    if clean.len() != 32 {
        return None;
    }
    let bytes = clean.as_bytes();
    let mut out = [0u8; 16];
    for i in 0..16 {
        let hi = (bytes[i * 2] as char).to_digit(16)?;
        let lo = (bytes[i * 2 + 1] as char).to_digit(16)?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Some(out)
}

/// Encode up to 2 opaque room tags as comma-separated lowercase hex for mDNS/NSD TXT records.
#[must_use]
pub fn encode_room_tags_hex(tags: &[Option<[u8; 16]>; 2]) -> String {
    let t0 = tags[0].map(|t| hex_encode(&t)).unwrap_or_default();
    let t1 = tags[1].map(|t| hex_encode(&t)).unwrap_or_default();
    format!("{t0},{t1}")
}

/// Decode comma-separated hex room tags from an mDNS/NSD TXT record.
#[must_use]
pub fn decode_room_tags_hex(raw: &str) -> [Option<[u8; 16]>; 2] {
    let mut parts = raw.split(',');
    let t0 = parts.next().and_then(decode_tag16_hex);
    let t1 = parts.next().and_then(decode_tag16_hex);
    [t0, t1]
}

/// Compute mutual handshake authentication MAC over both CSPRNG nonces and device IDs.
#[must_use]
pub fn compute_handshake_mac(
    room_key: &[u8; 32],
    nonce_first: &[u8; 16],
    nonce_second: &[u8; 16],
    device_first: &str,
    device_second: &str,
) -> [u8; 32] {
    let mut msg = Vec::with_capacity(
        HANDSHAKE_DOMAIN.len() + 32 + device_first.len() + device_second.len() + 2,
    );
    msg.extend_from_slice(HANDSHAKE_DOMAIN);
    msg.extend_from_slice(nonce_first);
    msg.extend_from_slice(nonce_second);
    msg.extend_from_slice(device_first.as_bytes());
    msg.push(0u8);
    msg.extend_from_slice(device_second.as_bytes());
    hmac_sha256(room_key, &msg)
}

/// Stateful per-connection ChaCha20-Poly1305 AEAD framing channel with directional keys and
/// strict monotonic `u64` nonces preventing replay or cross-direction reflection.
pub struct AeadChannel {
    tx_cipher: ChaCha20Poly1305,
    rx_cipher: ChaCha20Poly1305,
    tx_dir: u8,
    rx_dir: u8,
    tx_seq: u64,
    rx_seq: u64,
}

impl std::fmt::Debug for AeadChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadChannel")
            .field("tx_dir", &self.tx_dir)
            .field("rx_dir", &self.rx_dir)
            .field("tx_seq", &self.tx_seq)
            .field("rx_seq", &self.rx_seq)
            .finish()
    }
}

/// Transmit half of an established [`AeadChannel`] (owns monotonic `tx_seq`).
pub struct AeadSender {
    cipher: ChaCha20Poly1305,
    dir: u8,
    seq: u64,
}

impl std::fmt::Debug for AeadSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadSender")
            .field("dir", &self.dir)
            .field("seq", &self.seq)
            .finish()
    }
}

impl AeadSender {
    pub fn seal(&mut self, plaintext: &[u8]) -> SyncResult<Vec<u8>> {
        if plaintext.len() > MAX_FRAME_BYTES {
            return Err(SyncError::FrameTooLarge(plaintext.len()));
        }
        let seq = self.seq;
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or(SyncError::Crypto("tx nonce exhausted"))?;
        let (nonce_bytes, aad) = AeadChannel::build_nonce(self.dir, seq);
        self.cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| SyncError::Crypto("aead seal failed"))
    }
}

/// Receive half of an established [`AeadChannel`] (owns monotonic `rx_seq`).
pub struct AeadReceiver {
    cipher: ChaCha20Poly1305,
    dir: u8,
    seq: u64,
}

impl std::fmt::Debug for AeadReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AeadReceiver")
            .field("dir", &self.dir)
            .field("seq", &self.seq)
            .finish()
    }
}

impl AeadReceiver {
    pub fn open(&mut self, ciphertext: &[u8]) -> SyncResult<Vec<u8>> {
        if ciphertext.len() < POLY1305_TAG_LEN {
            return Err(SyncError::TruncatedFrame);
        }
        if ciphertext.len() > MAX_FRAME_BYTES + POLY1305_TAG_LEN {
            return Err(SyncError::FrameTooLarge(ciphertext.len()));
        }
        let seq = self.seq;
        let (nonce_bytes, aad) = AeadChannel::build_nonce(self.dir, seq);
        let pt = self
            .cipher
            .decrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| SyncError::AuthFailed)?;
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or(SyncError::Crypto("rx nonce exhausted"))?;
        Ok(pt)
    }
}

impl AeadChannel {
    /// Split into independent transmit and receive halves for lock-free full-duplex TCP I/O.
    #[must_use]
    pub fn split(self) -> (AeadSender, AeadReceiver) {
        (
            AeadSender {
                cipher: self.tx_cipher,
                dir: self.tx_dir,
                seq: self.tx_seq,
            },
            AeadReceiver {
                cipher: self.rx_cipher,
                dir: self.rx_dir,
                seq: self.rx_seq,
            },
        )
    }

    /// Derive directional keys from the shared `room_key` and mutual handshake nonces.
    #[must_use]
    pub fn from_handshake(
        room_key: &[u8; 32],
        nonce_init: &[u8; 16],
        nonce_resp: &[u8; 16],
        is_initiator: bool,
    ) -> Self {
        let mut salt = [0u8; 32];
        salt[..16].copy_from_slice(nonce_init);
        salt[16..].copy_from_slice(nonce_resp);

        let mut i2r = hkdf_sha256_32(room_key, &salt, b"ferric-connect-i2r-v1");
        let mut r2i = hkdf_sha256_32(room_key, &salt, b"ferric-connect-r2i-v1");

        let (tx_key, rx_key, tx_dir, rx_dir) = if is_initiator {
            (&i2r, &r2i, DIR_INIT_TO_RESP, DIR_RESP_TO_INIT)
        } else {
            (&r2i, &i2r, DIR_RESP_TO_INIT, DIR_INIT_TO_RESP)
        };

        let tx_cipher = ChaCha20Poly1305::new(Key::from_slice(tx_key));
        let rx_cipher = ChaCha20Poly1305::new(Key::from_slice(rx_key));

        zeroize_bytes(&mut salt);
        zeroize_bytes(&mut i2r);
        zeroize_bytes(&mut r2i);

        Self {
            tx_cipher,
            rx_cipher,
            tx_dir,
            rx_dir,
            tx_seq: 0,
            rx_seq: 0,
        }
    }

    fn build_nonce(dir: u8, seq: u64) -> ([u8; 12], [u8; 9]) {
        let mut nonce = [0u8; 12];
        nonce[0] = dir;
        nonce[4..12].copy_from_slice(&seq.to_le_bytes());
        let mut aad = [0u8; 9];
        aad[0] = dir;
        aad[1..9].copy_from_slice(&seq.to_le_bytes());
        (nonce, aad)
    }

    /// Encrypt `plaintext` (<= `MAX_FRAME_BYTES`) and advance the monotonic transmit sequence counter.
    pub fn seal(&mut self, plaintext: &[u8]) -> SyncResult<Vec<u8>> {
        if plaintext.len() > MAX_FRAME_BYTES {
            return Err(SyncError::FrameTooLarge(plaintext.len()));
        }
        let seq = self.tx_seq;
        self.tx_seq = self
            .tx_seq
            .checked_add(1)
            .ok_or(SyncError::Crypto("tx nonce exhausted"))?;
        let (nonce_bytes, aad) = Self::build_nonce(self.tx_dir, seq);
        let ct = self
            .tx_cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| SyncError::Crypto("aead seal failed"))?;
        Ok(ct)
    }

    /// Decrypt and authenticate `ciphertext` with the expected monotonic receive sequence counter.
    pub fn open(&mut self, ciphertext: &[u8]) -> SyncResult<Vec<u8>> {
        if ciphertext.len() < POLY1305_TAG_LEN {
            return Err(SyncError::TruncatedFrame);
        }
        if ciphertext.len() > MAX_FRAME_BYTES + POLY1305_TAG_LEN {
            return Err(SyncError::FrameTooLarge(ciphertext.len()));
        }
        let seq = self.rx_seq;
        let (nonce_bytes, aad) = Self::build_nonce(self.rx_dir, seq);
        let pt = self
            .rx_cipher
            .decrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| SyncError::AuthFailed)?;
        self.rx_seq = self
            .rx_seq
            .checked_add(1)
            .ok_or(SyncError::Crypto("rx nonce exhausted"))?;
        Ok(pt)
    }
}

/// Derive an unlinkable opaque MQTT topic name (`ferric/v1/<32 hex chars>`) from a 32-byte room key.
/// Brokers only ever observe this HMAC digest, never account emails, IDs, or handles.
#[must_use]
pub fn derive_relay_topic(room_key: &[u8; 32]) -> String {
    let mac = hmac_sha256(room_key, RELAY_TOPIC_LABEL);
    format!("ferric/v1/{}", hex_encode(&mac[..16]))
}

/// Seal a relay plaintext payload under `room_key` using a per-boot `epoch_salt` and monotonic
/// `seq >= 1` (`[FCR1][epoch_salt:16][seq:u64 LE][ChaCha20-Poly1305 ct+tag]`).
pub fn seal_relay_packet(
    room_key: &[u8; 32],
    epoch_salt: &[u8; 16],
    seq: u64,
    plaintext: &[u8],
) -> SyncResult<Vec<u8>> {
    if seq == 0 {
        return Err(SyncError::Crypto("relay seq must be non-zero"));
    }
    if plaintext.len() > MAX_FRAME_BYTES {
        return Err(SyncError::FrameTooLarge(plaintext.len()));
    }
    let mut msg_key = hkdf_sha256_32(room_key, epoch_salt, RELAY_MSG_INFO);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&msg_key));
    zeroize_bytes(&mut msg_key);

    let mut header = [0u8; RELAY_HEADER_LEN];
    header[..4].copy_from_slice(RELAY_FRAME_MAGIC);
    header[4..20].copy_from_slice(epoch_salt);
    header[20..28].copy_from_slice(&seq.to_le_bytes());

    let mut nonce = [0u8; 12];
    nonce[0] = 0x09;
    nonce[4..12].copy_from_slice(&seq.to_le_bytes());

    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &header,
            },
        )
        .map_err(|_| SyncError::Crypto("relay seal failed"))?;

    let mut out = Vec::with_capacity(RELAY_HEADER_LEN + ct.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt and authenticate a sealed relay packet, returning `(epoch_salt, seq, plaintext)`.
pub fn open_relay_packet(
    room_key: &[u8; 32],
    packet: &[u8],
) -> SyncResult<([u8; 16], u64, Vec<u8>)> {
    if packet.len() < RELAY_HEADER_LEN + POLY1305_TAG_LEN {
        return Err(SyncError::TruncatedFrame);
    }
    if packet.len() > RELAY_HEADER_LEN + MAX_FRAME_BYTES + POLY1305_TAG_LEN {
        return Err(SyncError::FrameTooLarge(packet.len()));
    }
    if &packet[..4] != RELAY_FRAME_MAGIC {
        return Err(SyncError::AuthFailed);
    }
    let mut epoch_salt = [0u8; 16];
    epoch_salt.copy_from_slice(&packet[4..20]);
    let seq = u64::from_le_bytes([
        packet[20], packet[21], packet[22], packet[23], packet[24], packet[25], packet[26],
        packet[27],
    ]);
    if seq == 0 {
        return Err(SyncError::AuthFailed);
    }

    let mut msg_key = hkdf_sha256_32(room_key, &epoch_salt, RELAY_MSG_INFO);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&msg_key));
    zeroize_bytes(&mut msg_key);

    let mut nonce = [0u8; 12];
    nonce[0] = 0x09;
    nonce[4..12].copy_from_slice(&seq.to_le_bytes());

    let pt = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &packet[RELAY_HEADER_LEN..],
                aad: &packet[..RELAY_HEADER_LEN],
            },
        )
        .map_err(|_| SyncError::AuthFailed)?;

    Ok((epoch_salt, seq, pt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha256_matches_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex_encode(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hkdf_sha256_matches_rfc5869_case_1() {
        let ikm = [0x0bu8; 22];
        let salt = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
        ];
        let info = [0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9];
        let prk = hkdf_extract(&salt, &ikm);
        assert_eq!(
            hex_encode(&prk),
            "077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"
        );
        let okm = hkdf_expand_32(&prk, &info);
        assert_eq!(
            hex_encode(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
        );
    }

    #[test]
    fn dual_provider_room_matching_handles_mixed_and_dual_logins_deterministically() {
        let desktop = LocalRoomKeys::from_accounts("RaflyUser123", "rafly@gmail.com");
        let phone_ytm_only = LocalRoomKeys::from_accounts("", "RAFLY@GMAIL.COM");
        let phone_both = LocalRoomKeys::from_accounts("raflyuser123", "rafly@gmail.com");
        let stranger = LocalRoomKeys::from_accounts("other_user", "other@gmail.com");

        // Mixed login: Desktop has Spotify+YTM, Phone has YTM-only -> matches YTM deterministically
        let (slot, key) = desktop
            .match_remote_tags(&phone_ytm_only.advertised_tags())
            .expect("ytm match");
        assert_eq!(slot, MatchedRoomSlot::YouTubeMusic);
        assert_eq!(key, phone_ytm_only.ytm.as_ref().expect("ytm").key);

        // Dual login: both match -> deterministically selects Spotify key on both sides
        let (slot_d, key_d) = desktop
            .match_remote_tags(&phone_both.advertised_tags())
            .expect("spotify priority match");
        let (slot_p, key_p) = phone_both
            .match_remote_tags(&desktop.advertised_tags())
            .expect("spotify priority match");
        assert_eq!(slot_d, MatchedRoomSlot::Spotify);
        assert_eq!(slot_p, MatchedRoomSlot::Spotify);
        assert_eq!(key_d, key_p);

        // Unrelated accounts -> never match
        assert!(
            desktop
                .match_remote_tags(&stranger.advertised_tags())
                .is_none()
        );

        // Hex roundtrip for mDNS/NSD TXT records
        let encoded = encode_room_tags_hex(&desktop.advertised_tags());
        assert_eq!(decode_room_tags_hex(&encoded), desktop.advertised_tags());
    }

    #[test]
    fn aead_channel_encrypts_detects_tampering_and_blocks_replay() {
        let room = derive_spotify_room("rafly_account").expect("room");
        let n_init = [0x11u8; 16];
        let n_resp = [0x22u8; 16];
        let mut init = AeadChannel::from_handshake(&room.key, &n_init, &n_resp, true);
        let mut resp = AeadChannel::from_handshake(&room.key, &n_init, &n_resp, false);

        let ct0 = init.seal(b"play track 1").expect("seal 0");
        let ct1 = init.seal(b"volume 0.75").expect("seal 1");

        assert_eq!(resp.open(&ct0).expect("open 0"), b"play track 1");

        // Replay of ct0 at seq 1 must fail closed!
        assert!(resp.open(&ct0).is_err());

        // Tampered ciphertext must fail closed and NOT advance rx_seq
        let mut tampered = ct1.clone();
        tampered[0] ^= 0xFF;
        assert!(resp.open(&tampered).is_err());

        // Valid ct1 still opens cleanly because failed tamper did not poison rx_seq
        assert_eq!(resp.open(&ct1).expect("open 1"), b"volume 0.75");

        // Wrong room key fails closed
        let wrong_room = derive_spotify_room("attacker").expect("wrong room");
        let mut attacker = AeadChannel::from_handshake(&wrong_room.key, &n_init, &n_resp, true);
        let bad_ct = attacker.seal(b"hijack").expect("seal");
        assert!(resp.open(&bad_ct).is_err());
    }

    #[test]
    fn relay_topic_is_opaque_and_packet_aead_rejects_wrong_key_or_tamper() {
        let room_a = derive_spotify_room("RaflyUser").expect("room a");
        let room_b = derive_spotify_room("raflyuser").expect("room b");
        let other = derive_spotify_room("someone_else").expect("other");

        let topic_a = derive_relay_topic(&room_a.key);
        let topic_b = derive_relay_topic(&room_b.key);
        assert_eq!(topic_a, topic_b);
        assert!(topic_a.starts_with("ferric/v1/"));
        assert!(!topic_a.to_ascii_lowercase().contains("rafly"));
        assert_ne!(topic_a, derive_relay_topic(&other.key));

        let salt = [0x42u8; 16];
        let sealed = seal_relay_packet(&room_a.key, &salt, 1, b"hello relay").expect("seal");
        let (dec_salt, dec_seq, pt) = open_relay_packet(&room_b.key, &sealed).expect("open");
        assert_eq!(dec_salt, salt);
        assert_eq!(dec_seq, 1);
        assert_eq!(pt, b"hello relay");

        let mut tampered = sealed.clone();
        tampered[12] ^= 0x01;
        assert!(open_relay_packet(&room_b.key, &tampered).is_err());
        assert!(open_relay_packet(&other.key, &sealed).is_err());
    }
}
