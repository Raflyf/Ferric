use crate::oauth::{SyncedPlaylistBundle, url_encode};
use crate::{ProviderError, ProviderResult};
use ferric_core::models::{Playlist, Track, TrackSource};
use serde::{Deserialize, Deserializer, Serialize};

const SUBSONIC_API_VERSION: &str = "1.16.1";
const SUBSONIC_CLIENT_NAME: &str = "Ferric";
const MAX_SUBSONIC_JSON_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SUBSONIC_STREAM_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SUBSONIC_PLAYLISTS: usize = 100;
const MAX_SUBSONIC_PLAYLIST_TRACKS: usize = 5_000;

static SUBSONIC_HTTP_AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub struct CachedSubsonicStream {
    pub path: std::path::PathBuf,
    pub codec: String,
}

#[cfg(target_os = "android")]
fn load_android_system_and_user_certs() -> (Vec<ureq::tls::Certificate<'static>>, bool) {
    use ureq::tls::{Certificate, PemItem, parse_pem};
    let mut certs = Vec::new();
    let mut loaded_system_root = false;
    let system_dirs = [
        "/apex/com.android.conscrypt/cacerts",
        "/system/etc/security/cacerts",
    ];
    let user_dirs = ["/data/misc/user/0/cacerts-added"];
    let mut ingest = |dir: &str, is_system: bool| {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            if bytes.is_empty() || bytes.len() > 256 * 1024 {
                continue;
            }
            let mut parsed_any = false;
            for item in parse_pem(&bytes).flatten() {
                if let PemItem::Certificate(c) = item {
                    certs.push(c);
                    parsed_any = true;
                }
            }
            if !parsed_any && bytes.first() == Some(&0x30) {
                certs.push(Certificate::from_der(&bytes).to_owned());
                parsed_any = true;
            }
            if parsed_any && is_system {
                loaded_system_root = true;
            }
        }
    };
    for dir in system_dirs {
        ingest(dir, true);
    }
    for dir in user_dirs {
        ingest(dir, false);
    }
    (certs, loaded_system_root)
}

/// Shared HTTP client for OpenSubsonic / Navidrome servers.
/// On Android, trusts Android system + user-installed CA certificates when system roots are
/// readable (preserving public CAs alongside private LAN CAs); falls back to built-in WebPKI
/// roots if SELinux restricts system CA directories so public HTTPS servers never break.
pub fn subsonic_http_agent() -> &'static ureq::Agent {
    SUBSONIC_HTTP_AGENT.get_or_init(|| {
        let builder = ureq::Agent::config_builder()
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_connect(Some(std::time::Duration::from_secs(8)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(25)))
            .timeout_per_read(Some(std::time::Duration::from_secs(20)))
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .http_status_as_error(false);
        #[cfg(target_os = "android")]
        let builder = {
            let (android_certs, loaded_system_root) = load_android_system_and_user_certs();
            if loaded_system_root && !android_certs.is_empty() {
                let tls = ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::new_with_certs(&android_certs))
                    .build();
                builder.tls_config(tls)
            } else {
                builder
            }
        };
        ureq::Agent::new_with_config(builder.build())
    })
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubsonicCredentials {
    pub base_url: String,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for SubsonicCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubsonicCredentials")
            .field("base_url", &self.base_url)
            .field("username", &self.username)
            .field(
                "password",
                &if self.password.is_empty() {
                    ""
                } else {
                    "[REDACTED]"
                },
            )
            .finish()
    }
}

/// Pure-Rust RFC 1321 MD5 hex digest used strictly for Subsonic v1.13+ salted token authentication.
fn md5_hex(input: &[u8]) -> String {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut msg = input.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    let (blocks, _) = msg.as_chunks::<64>();
    for block in blocks {
        let mut m = [0u32; 16];
        let (words, _) = block.as_chunks::<4>();
        for (slot, &w) in m.iter_mut().zip(words.iter()) {
            *slot = u32::from_le_bytes(w);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut digest = [0u8; 16];
    digest[0..4].copy_from_slice(&a0.to_le_bytes());
    digest[4..8].copy_from_slice(&b0.to_le_bytes());
    digest[8..12].copy_from_slice(&c0.to_le_bytes());
    digest[12..16].copy_from_slice(&d0.to_le_bytes());

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for b in digest {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

/// Encode UTF-8 password bytes as Subsonic `enc:<hex>` parameter (retained for compatibility tests).
pub fn encode_subsonic_password_hex(password: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(4 + password.len() * 2);
    out.push_str("enc:");
    for &b in password.as_bytes() {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

fn is_blocked_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let oct = v4.octets();
            let is_cgnat = oct[0] == 100 && (oct[1] & 0b1100_0000) == 0b0100_0000;
            v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || is_cgnat
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ip(std::net::IpAddr::V4(mapped));
            }
            v6.is_unspecified() || v6.is_multicast() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback() || mapped.is_private();
            }
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_shorthand_numeric_host(h: &str) -> bool {
    if h.contains(':') {
        return false;
    }
    let labels_numeric = h.split('.').all(|lbl| {
        !lbl.is_empty() && (lbl.starts_with("0x") || lbl.bytes().all(|b| b.is_ascii_digit()))
    });
    labels_numeric && h.parse::<std::net::Ipv4Addr>().is_err()
}

fn is_private_or_local_host(host: &str) -> bool {
    let h = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if is_shorthand_numeric_host(&h) {
        return false;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }
    if h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".lan")
        || h.ends_with(".internal")
    {
        return true;
    }
    // Single-label LAN hostnames (e.g. `navidrome`) must contain at least one ASCII letter.
    !h.contains('.') && h.bytes().any(|b| b.is_ascii_alphabetic())
}

fn is_blocked_metadata_host(host: &str) -> bool {
    let h = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if is_shorthand_numeric_host(&h) {
        return true;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return is_blocked_ip(ip);
    }
    false
}

fn parse_validated_host_port(raw: &str) -> ProviderResult<(String, bool, String, u16)> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(ProviderError::Http(
            "Subsonic server URL cannot be empty".to_string(),
        ));
    }
    if trimmed
        .bytes()
        .any(|b| b.is_ascii_control() || b == b' ' || b == b'\\')
    {
        return Err(ProviderError::Http(
            "Subsonic server URL contains invalid characters".to_string(),
        ));
    }
    let (is_https, rest) = if let Some(r) = trimmed.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = trimmed.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(ProviderError::Http(
            "Subsonic server URL must start with http:// or https://".to_string(),
        ));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(ProviderError::Http(
            "Subsonic server URL has an invalid host".to_string(),
        ));
    }
    let default_port: u16 = if is_https { 443 } else { 80 };
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let Some((v6_host, tail)) = inner.split_once(']') else {
            return Err(ProviderError::Http(
                "Subsonic server URL has a malformed IPv6 host".to_string(),
            ));
        };
        let port = if tail.is_empty() {
            default_port
        } else if let Some(p_str) = tail.strip_prefix(':') {
            p_str
                .parse::<u16>()
                .map_err(|_| ProviderError::Http("Invalid Subsonic server port".to_string()))?
        } else {
            return Err(ProviderError::Http(
                "Subsonic server URL has an invalid host".to_string(),
            ));
        };
        (v6_host, port)
    } else if let Some((h, p_str)) = authority.split_once(':') {
        let port = p_str
            .parse::<u16>()
            .map_err(|_| ProviderError::Http("Invalid Subsonic server port".to_string()))?;
        (h, port)
    } else {
        (authority, default_port)
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
        || is_blocked_metadata_host(host)
    {
        return Err(ProviderError::Http(
            "Refused to connect to link-local, unspecified, or malformed Subsonic host".to_string(),
        ));
    }
    if !is_https && !is_private_or_local_host(host) {
        return Err(ProviderError::Http(
            "Public Subsonic servers must use https:// (http:// is allowed only for local/LAN hosts)"
                .to_string(),
        ));
    }
    Ok((trimmed.to_string(), is_https, host.to_string(), port))
}

fn normalized_base_url(raw: &str) -> ProviderResult<String> {
    parse_validated_host_port(raw).map(|(url, _, _, _)| url)
}

fn verify_resolved_subsonic_target(host: &str, port: u16, is_https: bool) -> ProviderResult<()> {
    use std::net::ToSocketAddrs;
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| ProviderError::Http(format!("Could not resolve Subsonic host: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(ProviderError::Http(
            "Subsonic host resolved to no addresses".to_string(),
        ));
    }
    for sock in addrs {
        let ip = sock.ip();
        if is_blocked_ip(ip) {
            return Err(ProviderError::Http(
                "Refused to connect to blocked link-local, metadata, or reserved Subsonic IP"
                    .to_string(),
            ));
        }
        if !is_https && !is_private_ip(ip) {
            return Err(ProviderError::Http(
                "Resolved public Subsonic host requires https://".to_string(),
            ));
        }
    }
    Ok(())
}

fn build_auth_query_with_salt(creds: &SubsonicCredentials, salt: &str) -> String {
    let token_input = format!("{}{}", creds.password, salt);
    let token = md5_hex(token_input.as_bytes());
    format!(
        "f=json&v={SUBSONIC_API_VERSION}&c={SUBSONIC_CLIENT_NAME}&u={}&t={token}&s={}",
        url_encode(creds.username.trim()),
        url_encode(salt)
    )
}

fn build_auth_query(creds: &SubsonicCredentials) -> String {
    let salt = ferric_core::secrets::csprng_hex(8);
    build_auth_query_with_salt(creds, &salt)
}

/// Construct direct OpenSubsonic/Navidrome audio stream URL for `song_id`.
pub fn subsonic_stream_url(creds: &SubsonicCredentials, song_id: &str) -> ProviderResult<String> {
    let base = normalized_base_url(&creds.base_url)?;
    let raw_id = song_id.strip_prefix("subsonic:").unwrap_or(song_id);
    Ok(format!(
        "{base}/rest/stream.view?id={}&{}",
        url_encode(raw_id),
        build_auth_query(creds)
    ))
}

fn deserialize_string_or_int<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrIntVisitor;
    impl serde::de::Visitor<'_> for StringOrIntVisitor {
        type Value = String;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a string or integer")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.to_owned())
        }

        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(v.to_string())
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(String::new())
        }
    }
    deserializer.deserialize_any(StringOrIntVisitor)
}

fn deserialize_vec_or_single<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct VecOrSingleVisitor<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for VecOrSingleVisitor<T> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a sequence, single object, or null")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
            while let Some(item) = seq.next_element()? {
                out.push(item);
            }
            out.shrink_to_fit();
            Ok(out)
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
            let single = T::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![single])
        }
    }
    deserializer.deserialize_any(VecOrSingleVisitor(std::marker::PhantomData))
}

fn deserialize_first_isrc<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(val) = val else {
        return Ok(None);
    };
    match val {
        serde_json::Value::String(s) => Ok(Some(s).filter(|v| !v.trim().is_empty())),
        serde_json::Value::Array(arr) => Ok(arr.into_iter().find_map(|v| {
            v.as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        })),
        _ => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
struct SubsonicEnvelope {
    #[serde(rename = "subsonic-response")]
    response: SubsonicResponseBody,
}

#[derive(Debug, Deserialize)]
struct SubsonicErrorBody {
    code: Option<i32>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubsonicResponseBody {
    status: String,
    error: Option<SubsonicErrorBody>,
    #[serde(rename = "searchResult3")]
    search_result3: Option<SubsonicSearchResult3>,
    playlists: Option<SubsonicPlaylistsContainer>,
    playlist: Option<SubsonicPlaylistDetail>,
}

#[derive(Debug, Deserialize)]
struct SubsonicSearchResult3 {
    #[serde(default, deserialize_with = "deserialize_vec_or_single")]
    song: Vec<SubsonicSong>,
}

#[derive(Debug, Deserialize)]
struct SubsonicPlaylistsContainer {
    #[serde(default, deserialize_with = "deserialize_vec_or_single")]
    playlist: Vec<SubsonicPlaylistSummary>,
}

#[derive(Debug, Deserialize)]
struct SubsonicPlaylistSummary {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    id: String,
    name: Option<String>,
    changed: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubsonicPlaylistDetail {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    id: String,
    name: Option<String>,
    comment: Option<String>,
    changed: Option<String>,
    #[serde(default, deserialize_with = "deserialize_vec_or_single")]
    entry: Vec<SubsonicSong>,
}

#[derive(Debug, Deserialize)]
struct SubsonicSong {
    #[serde(deserialize_with = "deserialize_string_or_int")]
    id: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<u64>,
    suffix: Option<String>,
    #[serde(rename = "transcodedSuffix")]
    transcoded_suffix: Option<String>,
    #[serde(rename = "bitRate")]
    bit_rate: Option<u32>,
    #[serde(rename = "samplingRate")]
    sampling_rate: Option<u32>,
    #[serde(rename = "bitDepth")]
    bit_depth: Option<u16>,
    starred: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "deserialize_first_isrc")]
    isrc: Option<String>,
}

fn map_subsonic_song(song: SubsonicSong) -> Option<Track> {
    let raw_id = song.id.trim();
    if raw_id.is_empty() {
        return None;
    }
    let title = song
        .title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Untitled Track".to_string());
    let artist = song
        .artist
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = song
        .album
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Subsonic Library".to_string());

    let codec = song
        .suffix
        .or(song.transcoded_suffix)
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "FLAC".to_string());

    let sample_rate = song.sampling_rate.filter(|&sr| sr > 0).unwrap_or(44_100);
    let bit_depth = song.bit_depth.filter(|&bd| bd > 0).unwrap_or(16);
    let bitrate_kbps = song.bit_rate.unwrap_or(0);

    Some(Track {
        id: format!("subsonic:{raw_id}"),
        title,
        artist,
        album,
        duration_ms: song.duration.unwrap_or(0).saturating_mul(1000),
        path: None,
        source: TrackSource::Subsonic,
        codec,
        sample_rate,
        bit_depth,
        bitrate_kbps,
        isrc: song.isrc,
        spotify_id: None,
        youtube_video_id: None,
        thumbnail_url: None,
        replay_gain_db: 0,
        liked: song.starred.is_some(),
    })
}

fn fetch_subsonic_response(agent: &ureq::Agent, url: &str) -> ProviderResult<SubsonicResponseBody> {
    let resp = agent
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", concat!("Ferric/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| ProviderError::Http(format!("Subsonic request failed: {e}")))?;

    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .into_with_config()
        .limit(MAX_SUBSONIC_JSON_BYTES)
        .read_to_vec()
        .map_err(|e| ProviderError::Http(format!("Could not read Subsonic body: {e}")))?;

    if status != 200 {
        return Err(ProviderError::Http(format!(
            "Subsonic server returned HTTP {status}"
        )));
    }

    let env: SubsonicEnvelope = serde_json::from_slice(&bytes)
        .map_err(|e| ProviderError::Http(format!("Invalid OpenSubsonic JSON: {e}")))?;

    if !env.response.status.eq_ignore_ascii_case("ok") {
        let msg = env
            .response
            .error
            .and_then(|err| {
                err.message.map(|m| match err.code {
                    Some(c) => format!("{m} (code {c})"),
                    None => m,
                })
            })
            .unwrap_or_else(|| "Subsonic API returned status failed".to_string());
        return Err(ProviderError::Http(msg));
    }

    Ok(env.response)
}

/// Synchronize up to 200 catalog songs and all playlists from an OpenSubsonic / Navidrome server.
pub fn sync_subsonic_catalog(
    creds: &SubsonicCredentials,
) -> ProviderResult<(Vec<Track>, Vec<SyncedPlaylistBundle>)> {
    let (base, is_https, host, port) = parse_validated_host_port(&creds.base_url)?;
    verify_resolved_subsonic_target(&host, port, is_https)?;

    let agent = subsonic_http_agent();

    let search_url = format!(
        "{base}/rest/search3.view?query=&songCount=200&artistCount=0&albumCount=0&{}",
        build_auth_query(creds)
    );
    let search_body = fetch_subsonic_response(agent, &search_url)?;
    let mut catalog_tracks = Vec::new();
    if let Some(sr) = search_body.search_result3 {
        for song in sr.song {
            if let Some(track) = map_subsonic_song(song) {
                catalog_tracks.push(track);
            }
        }
    }

    let playlists_url = format!("{base}/rest/getPlaylists.view?{}", build_auth_query(creds));
    let playlists_body = fetch_subsonic_response(agent, &playlists_url)?;
    let summaries = playlists_body
        .playlists
        .map(|p| p.playlist)
        .unwrap_or_default();

    let mut bundles = Vec::with_capacity(summaries.len().min(MAX_SUBSONIC_PLAYLISTS));
    for summary in summaries.into_iter().take(MAX_SUBSONIC_PLAYLISTS) {
        if summary.id.trim().is_empty() {
            continue;
        }
        let detail_url = format!(
            "{base}/rest/getPlaylist.view?id={}&{}",
            url_encode(&summary.id),
            build_auth_query(creds)
        );
        let detail_body = fetch_subsonic_response(agent, &detail_url)?;
        let Some(detail) = detail_body.playlist else {
            continue;
        };

        let mut pl_tracks =
            Vec::with_capacity(detail.entry.len().min(MAX_SUBSONIC_PLAYLIST_TRACKS));
        for entry in detail.entry.into_iter().take(MAX_SUBSONIC_PLAYLIST_TRACKS) {
            if let Some(track) = map_subsonic_song(entry) {
                pl_tracks.push(track);
            }
        }

        let name = detail
            .name
            .or(summary.name)
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Subsonic Playlist".to_string());

        let playlist = Playlist {
            id: format!("subsonic:playlist:{}", detail.id),
            name,
            source: TrackSource::Subsonic,
            etag: detail.changed.or(summary.changed),
            track_count: pl_tracks.len(),
            description: detail
                .comment
                .map(|c| c.trim().to_string())
                .unwrap_or_default(),
            thumbnail_url: None,
        };
        bundles.push(SyncedPlaylistBundle {
            playlist,
            tracks: pl_tracks,
        });
    }

    Ok((catalog_tracks, bundles))
}

fn sanitize_subsonic_ext(suffix_hint: &str, content_type: Option<&str>) -> &'static str {
    let hint = suffix_hint.trim().trim_start_matches('.');
    if hint.eq_ignore_ascii_case("flac") {
        return "flac";
    }
    if hint.eq_ignore_ascii_case("mp3") {
        return "mp3";
    }
    if hint.eq_ignore_ascii_case("m4a")
        || hint.eq_ignore_ascii_case("aac")
        || hint.eq_ignore_ascii_case("alac")
    {
        return "m4a";
    }
    if hint.eq_ignore_ascii_case("opus") {
        return "opus";
    }
    if hint.eq_ignore_ascii_case("ogg") {
        return "ogg";
    }
    if hint.eq_ignore_ascii_case("wav") {
        return "wav";
    }
    if hint.eq_ignore_ascii_case("aiff") || hint.eq_ignore_ascii_case("aif") {
        return "aiff";
    }
    if let Some(ct) = content_type {
        let low = ct.to_ascii_lowercase();
        if low.contains("flac") {
            return "flac";
        }
        if low.contains("mpeg") || low.contains("mp3") {
            return "mp3";
        }
        if low.contains("mp4") || low.contains("aac") {
            return "m4a";
        }
        if low.contains("ogg") || low.contains("opus") {
            return "ogg";
        }
        if low.contains("wav") {
            return "wav";
        }
    }
    "flac"
}

fn subsonic_cache_key(base_url: &str, raw_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(base_url.as_bytes());
    hasher.update(b"|");
    hasher.update(raw_id.as_bytes());
    let digest = hasher.finalize();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(24);
    for &b in &digest[..12] {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

/// Download and cache an OpenSubsonic / Navidrome audio stream to `cache_dir` atomically.
pub fn download_and_cache_subsonic_stream(
    creds: &SubsonicCredentials,
    song_id: &str,
    suffix_hint: &str,
    cache_dir: &std::path::Path,
) -> ProviderResult<CachedSubsonicStream> {
    use std::io::{Read, Write};

    let (base, is_https, host, port) = parse_validated_host_port(&creds.base_url)?;
    verify_resolved_subsonic_target(&host, port, is_https)?;
    let raw_id = song_id.strip_prefix("subsonic:").unwrap_or(song_id).trim();
    if raw_id.is_empty() {
        return Err(ProviderError::Http("Empty Subsonic song ID".to_string()));
    }

    std::fs::create_dir_all(cache_dir)?;
    let key = subsonic_cache_key(&base, raw_id);
    let hint_ext = sanitize_subsonic_ext(suffix_hint, None);
    let candidate = cache_dir.join(format!("subsonic_{key}.{hint_ext}"));
    if let Ok(meta) = std::fs::metadata(&candidate)
        && meta.is_file()
        && meta.len() >= 1024
    {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&candidate) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
        return Ok(CachedSubsonicStream {
            path: candidate,
            codec: hint_ext.to_ascii_uppercase(),
        });
    }

    let url = format!(
        "{base}/rest/stream.view?id={}&{}",
        url_encode(raw_id),
        build_auth_query(creds)
    );
    let resp = subsonic_http_agent()
        .get(&url)
        .header("User-Agent", concat!("Ferric/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| ProviderError::Http(format!("Subsonic stream request failed: {e}")))?;

    let status = resp.status().as_u16();
    if status != 200 {
        return Err(ProviderError::Http(format!(
            "Subsonic stream returned HTTP {status}"
        )));
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    if let Some(ct) = content_type.as_deref() {
        let low = ct.to_ascii_lowercase();
        if low.starts_with("application/json")
            || low.starts_with("text/")
            || low.starts_with("application/xml")
        {
            return Err(ProviderError::Http(format!(
                "Subsonic stream returned non-audio content-type: {ct}"
            )));
        }
    }

    let final_ext = sanitize_subsonic_ext(suffix_hint, content_type.as_deref());
    let dest_path = cache_dir.join(format!("subsonic_{key}.{final_ext}"));
    let part_path = cache_dir.join(format!("subsonic_{key}_{}.part", fastrand::u64(..)));

    let mut reader = resp.into_body().into_reader();
    let mut out = std::fs::File::create(&part_path)?;
    let mut buf = vec![0u8; 32 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > MAX_SUBSONIC_STREAM_BYTES {
            drop(out);
            let _ = std::fs::remove_file(&part_path);
            return Err(ProviderError::Http(
                "Subsonic audio stream exceeded 128 MiB safety cap".to_string(),
            ));
        }
        out.write_all(&buf[..n])?;
    }
    out.flush()?;
    drop(out);

    if total < 256 {
        let _ = std::fs::remove_file(&part_path);
        return Err(ProviderError::Http(
            "Subsonic stream returned empty/truncated audio".to_string(),
        ));
    }

    if let Err(err) = std::fs::rename(&part_path, &dest_path) {
        let _ = std::fs::remove_file(&dest_path);
        std::fs::rename(&part_path, &dest_path).map_err(|_| err)?;
    }

    Ok(CachedSubsonicStream {
        path: dest_path,
        codec: final_ext.to_ascii_uppercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_subsonic_salted_token_and_validates_urls() {
        assert_eq!(encode_subsonic_password_hex("sesame"), "enc:736573616d65");
        // RFC 1321 MD5 vectors + Subsonic v1.13+ salted token md5("sesame" + "c19b2d")
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(md5_hex(b"sesamec19b2d"), "26719a1196d2a940705a59634eb18eab");
        let creds = SubsonicCredentials {
            base_url: "https://music.example.lan/".to_string(),
            username: "audiophile".to_string(),
            password: "secret".to_string(),
        };
        let url = subsonic_stream_url(&creds, "subsonic:song_42").expect("valid stream url");
        assert!(url.starts_with(
            "https://music.example.lan/rest/stream.view?id=song_42&f=json&v=1.16.1&c=Ferric&u=audiophile&t="
        ));
        assert!(url.contains("&s="));
        assert!(!url.contains("enc:"));
        assert!(!url.contains("secret"));

        assert!(normalized_base_url("http://169.254.169.254").is_err());
        assert!(normalized_base_url("http://2852039166").is_err());
        assert!(normalized_base_url("http://0xa9fea9fe").is_err());
        assert!(normalized_base_url("http://[::ffff:169.254.169.254]").is_err());
        assert!(normalized_base_url("http://public.example.com").is_err());
        assert!(normalized_base_url("https://public.example.com").is_ok());
        assert!(normalized_base_url("http://192.168.1.50:4533").is_ok());
    }

    #[test]
    fn parses_opensubsonic_lossless_song_fields() {
        let sample = r#"{
            "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "searchResult3": {
                    "song": [{
                        "id": "trk_99",
                        "title": "Giorgio by Moroder",
                        "artist": "Daft Punk",
                        "album": "Random Access Memories",
                        "duration": 544,
                        "suffix": "flac",
                        "bitRate": 1580,
                        "samplingRate": 88200,
                        "bitDepth": 24,
                        "starred": "2026-01-12T00:00:00Z",
                        "isrc": ["USQX91300103"]
                    }]
                }
            }
        }"#;
        let env: SubsonicEnvelope = serde_json::from_str(sample).expect("parse subsonic json");
        let songs = env.response.search_result3.expect("searchResult3").song;
        assert_eq!(songs.len(), 1);
        let track = map_subsonic_song(songs.into_iter().next().expect("song")).expect("map track");
        assert_eq!(track.id, "subsonic:trk_99");
        assert_eq!(track.source, TrackSource::Subsonic);
        assert_eq!(track.codec, "FLAC");
        assert_eq!(track.sample_rate, 88_200);
        assert_eq!(track.bit_depth, 24);
        assert_eq!(track.bitrate_kbps, 1580);
        assert_eq!(track.duration_ms, 544_000);
        assert_eq!(track.isrc.as_deref(), Some("USQX91300103"));
        assert!(track.liked);
    }

    #[test]
    fn subsonic_stream_download_caches_and_rejects_json_errors() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let payload = [0x66u8, 0x4c, 0x61, 0x43].repeat(400); // 1600 bytes fake FLAC
        let payload_server = payload.clone();

        let handle = std::thread::spawn(move || {
            for idx in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf);
                if idx == 0 {
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload_server.len()
                    );
                    stream.write_all(hdr.as_bytes()).expect("hdr");
                    stream.write_all(&payload_server).expect("body");
                } else {
                    let body = br#"{"subsonic-response":{"status":"failed"}}"#;
                    let hdr = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(hdr.as_bytes()).expect("hdr2");
                    stream.write_all(body).expect("body2");
                }
            }
        });

        let creds = SubsonicCredentials {
            base_url: format!("http://127.0.0.1:{port}"),
            username: "tester".to_string(),
            password: "pw".to_string(),
        };
        let dir = std::env::temp_dir().join(format!(
            "ferric_subsonic_dl_{}_{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        let first = download_and_cache_subsonic_stream(&creds, "subsonic:s1", "FLAC", &dir)
            .expect("download audio");
        assert!(first.path.is_file());
        assert_eq!(std::fs::read(&first.path).expect("read"), payload);

        // Second call hits local cache without contacting server.
        let cached = download_and_cache_subsonic_stream(&creds, "subsonic:s1", "FLAC", &dir)
            .expect("cache hit");
        assert_eq!(cached.path, first.path);

        // JSON error body on a new song ID must be rejected fail-closed.
        assert!(download_and_cache_subsonic_stream(&creds, "subsonic:err", "FLAC", &dir).is_err());

        handle.join().expect("server thread");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
