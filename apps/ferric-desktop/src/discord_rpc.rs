//! Native zero-dependency Discord Desktop Rich Presence IPC client.
//!
//! Communicates directly with Discord Desktop over the local Windows named pipe
//! (`\\.\pipe\discord-ipc-0` .. `discord-ipc-9`) using Discord's binary JSON framing
//! (`[opcode: u32 LE][len: u32 LE][utf8 json]`) and Activity Type `2` (`Listening`).
//! Runs on a single sleeping background worker thread so UI and audio threads never block.

use ferric_core::models::{DiscordRpcSettings, DiscordStatusFormat};
use serde_json::{Value, json};
#[cfg(target_os = "windows")]
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

/// Official built-in Ferric Discord Application ID (public numeric client identifier).
/// Enables zero-config 1-toggle Rich Presence out of the box without manual setup.
pub const BUILTIN_DISCORD_APP_ID: &str = "1575176866413940806";

/// Public HTTPS URL for Ferric's brand icon used as the Rich Presence badge (`small_image`)
/// and local-track cover fallback (`large_image`).
pub const FERRIC_BRAND_ICON_URL: &str =
    "https://raw.githubusercontent.com/Raflyf/Ferric/main/apps/ferric-desktop/resources/ferric.png";

pub fn effective_client_id(cfg: &DiscordRpcSettings) -> &str {
    let custom = cfg.client_id.trim();
    if is_valid_client_id(custom) {
        custom
    } else if is_valid_client_id(BUILTIN_DISCORD_APP_ID) {
        BUILTIN_DISCORD_APP_ID
    } else {
        ""
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordTrackSnapshot {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_url: Option<String>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub is_playing: bool,
}

enum RpcCommand {
    Configure(DiscordRpcSettings),
    UpdatePlayback(Option<DiscordTrackSnapshot>),
    Shutdown,
}

pub struct DiscordRpcBridge {
    tx: Sender<RpcCommand>,
    status: Arc<Mutex<String>>,
    last_track_key: Option<(String, String, bool, u64)>,
}

impl DiscordRpcBridge {
    pub fn start(initial: DiscordRpcSettings) -> Self {
        let (tx, rx) = mpsc::channel();
        let init_status = initial_status_label(&initial).to_string();
        let status = Arc::new(Mutex::new(init_status));
        let status_bg = Arc::clone(&status);

        thread::Builder::new()
            .name("ferric-discord-rpc".to_string())
            .spawn(move || worker_loop(rx, initial, status_bg))
            .ok();

        Self {
            tx,
            status,
            last_track_key: None,
        }
    }

    pub fn configure(&mut self, cfg: DiscordRpcSettings) {
        self.last_track_key = None;
        let _ = self.tx.send(RpcCommand::Configure(cfg));
    }

    pub fn update_playback(&mut self, snap: Option<DiscordTrackSnapshot>) {
        // Deduplicate high-frequency ticks: only emit on track/state change or seek drift >= 3s.
        let next_key = snap.as_ref().map(|s| {
            let now_s = unix_now_secs();
            let start_s = now_s.saturating_sub(s.position_ms / 1000);
            (s.title.clone(), s.artist.clone(), s.is_playing, start_s / 3)
        });
        if next_key == self.last_track_key {
            return;
        }
        self.last_track_key = next_key;
        let _ = self.tx.send(RpcCommand::UpdatePlayback(snap));
    }

    pub fn status_text(&self) -> String {
        self.status
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "Disabled".to_string())
    }
}

impl Drop for DiscordRpcBridge {
    fn drop(&mut self) {
        let _ = self.tx.send(RpcCommand::Shutdown);
    }
}

fn initial_status_label(cfg: &DiscordRpcSettings) -> &'static str {
    if !cfg.enabled {
        "Disabled"
    } else if effective_client_id(cfg).is_empty() {
        "Enter your Discord Application ID below"
    } else {
        "Connecting to Discord desktop..."
    }
}

pub fn is_valid_client_id(raw: &str) -> bool {
    let s = raw.trim();
    (17..=22).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn set_status(slot: &Arc<Mutex<String>>, text: impl Into<String>) {
    if let Ok(mut g) = slot.lock() {
        *g = text.into();
    }
}

fn normalize_cover_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return None;
    }
    if trimmed.contains("googleusercontent.com")
        && let Some(eq_idx) = trimmed.rfind('=')
    {
        return Some(format!("{}=w512-h512-l90-rj", &trimmed[..eq_idx]));
    }
    Some(trimmed.to_string())
}

fn truncate_discord_field(s: &str, max_chars: usize) -> String {
    let clean = s.trim();
    if clean.chars().count() <= max_chars {
        return clean.to_string();
    }
    let mut out: String = clean.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn build_set_activity_payload(
    cfg: &DiscordRpcSettings,
    snap: Option<&DiscordTrackSnapshot>,
    pid: u32,
    nonce: u64,
    now_unix_secs: u64,
) -> Value {
    let Some(track) = snap else {
        return json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": pid, "activity": Value::Null },
            "nonce": format!("ferric-{nonce}")
        });
    };

    if !track.is_playing && !cfg.show_when_paused {
        return json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": pid, "activity": Value::Null },
            "nonce": format!("ferric-{nonce}")
        });
    }

    if cfg.hide_details {
        return json!({
            "cmd": "SET_ACTIVITY",
            "args": {
                "pid": pid,
                "activity": {
                    "type": 2,
                    "status_display_type": 0,
                    "details": if track.is_playing { "Listening to music" } else { "Paused" },
                    "state": "Ferric Hi-Fi Player",
                    "assets": {
                        "large_image": FERRIC_BRAND_ICON_URL,
                        "large_text": "Ferric Hi-Fi",
                        "small_image": FERRIC_BRAND_ICON_URL,
                        "small_text": if track.is_playing { "Ferric Hi-Fi" } else { "Paused · Ferric" }
                    }
                }
            },
            "nonce": format!("ferric-{nonce}")
        });
    }

    let title = truncate_discord_field(&track.title, 120);
    let artist = truncate_discord_field(&track.artist, 120);
    let album = truncate_discord_field(&track.album, 120);

    let (status_display_type, state_text) = match cfg.status_format {
        DiscordStatusFormat::ArtistTitle => {
            let combined = if artist.is_empty() {
                title.clone()
            } else {
                truncate_discord_field(&format!("{artist} - {title}"), 120)
            };
            (1_u8, combined)
        }
        DiscordStatusFormat::TrackTitle => (2_u8, artist),
        DiscordStatusFormat::Artist => (1_u8, artist),
        DiscordStatusFormat::AppName => (0_u8, artist),
    };

    let state_final = if !track.is_playing {
        if state_text.is_empty() {
            "Paused".to_string()
        } else {
            truncate_discord_field(&format!("{state_text} (Paused)"), 124)
        }
    } else if state_text.is_empty() {
        "Ferric".to_string()
    } else {
        state_text
    };

    let mut activity = serde_json::Map::new();
    activity.insert("type".to_string(), json!(2)); // 2 = Listening
    activity.insert(
        "status_display_type".to_string(),
        json!(status_display_type),
    );
    activity.insert(
        "details".to_string(),
        json!(if title.len() < 2 {
            format!("{title} ")
        } else {
            title
        }),
    );
    activity.insert(
        "state".to_string(),
        json!(if state_final.len() < 2 {
            format!("{state_final} ")
        } else {
            state_final
        }),
    );

    if track.is_playing && track.duration_ms > 0 {
        let elapsed_s = track.position_ms / 1000;
        let dur_s = track.duration_ms / 1000;
        let start = now_unix_secs.saturating_sub(elapsed_s);
        let end = start.saturating_add(dur_s);
        activity.insert(
            "timestamps".to_string(),
            json!({ "start": start, "end": end }),
        );
    }

    let mut assets = serde_json::Map::new();
    let cover_img = if cfg.show_cover_art
        && let Some(raw_url) = track.cover_url.as_deref()
        && let Some(clean_url) = normalize_cover_url(raw_url)
    {
        clean_url
    } else {
        FERRIC_BRAND_ICON_URL.to_string()
    };
    assets.insert("large_image".to_string(), json!(cover_img));
    if !album.is_empty() && album.len() >= 2 && !album.eq_ignore_ascii_case(track.title.trim()) {
        assets.insert("large_text".to_string(), json!(album));
    }
    assets.insert("small_image".to_string(), json!(FERRIC_BRAND_ICON_URL));
    assets.insert(
        "small_text".to_string(),
        json!(if track.is_playing { "Ferric" } else { "Paused" }),
    );
    activity.insert("assets".to_string(), Value::Object(assets));

    json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": pid,
            "activity": Value::Object(activity)
        },
        "nonce": format!("ferric-{nonce}")
    })
}

pub fn encode_ipc_frame(opcode: u32, payload: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(payload).unwrap_or_default();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&opcode.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

#[cfg(target_os = "windows")]
struct DiscordPipe {
    file: std::fs::File,
}

#[cfg(target_os = "windows")]
impl DiscordPipe {
    fn connect(client_id: &str) -> Result<Self, String> {
        let mut last_err = String::from("Discord desktop app not detected");
        for idx in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{idx}");
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let hs = json!({ "v": 1, "client_id": client_id.trim() });
                    let frame = encode_ipc_frame(OP_HANDSHAKE, &hs);
                    if let Err(e) = file.write_all(&frame).and_then(|_| file.flush()) {
                        last_err = format!("IPC write failed: {e}");
                        continue;
                    }
                    let mut pipe = Self { file };
                    match pipe.read_frame() {
                        Ok((OP_FRAME, val)) => {
                            if val.get("evt").and_then(Value::as_str) == Some("ERROR") {
                                let msg = val
                                    .pointer("/data/message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("Invalid Discord Application ID");
                                return Err(msg.to_string());
                            }
                            return Ok(pipe);
                        }
                        Ok((OP_CLOSE, val)) => {
                            let msg = val
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("Invalid Discord Application ID");
                            return Err(msg.to_string());
                        }
                        Ok(_) => return Ok(pipe),
                        Err(e) => {
                            last_err = e;
                        }
                    }
                }
                Err(_) => continue,
            }
        }
        Err(last_err)
    }

    fn send_frame(&mut self, opcode: u32, payload: &Value) -> Result<(), String> {
        let bytes = encode_ipc_frame(opcode, payload);
        self.file
            .write_all(&bytes)
            .and_then(|_| self.file.flush())
            .map_err(|e| format!("IPC send error: {e}"))
    }

    fn read_frame(&mut self) -> Result<(u32, Value), String> {
        let mut hdr = [0u8; 8];
        self.file
            .read_exact(&mut hdr)
            .map_err(|e| format!("IPC read header error: {e}"))?;
        let op = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
        if len > 256 * 1024 {
            return Err("IPC frame exceeded safety limit".to_string());
        }
        let mut buf = vec![0u8; len];
        if len > 0 {
            self.file
                .read_exact(&mut buf)
                .map_err(|e| format!("IPC read body error: {e}"))?;
        }
        let val = serde_json::from_slice(&buf).unwrap_or(Value::Null);
        Ok((op, val))
    }

    fn close_gracefully(&mut self) {
        let _ = self.send_frame(OP_CLOSE, &json!({}));
    }
}

#[cfg(not(target_os = "windows"))]
struct DiscordPipe;

#[cfg(not(target_os = "windows"))]
impl DiscordPipe {
    fn connect(_client_id: &str) -> Result<Self, String> {
        Err("Discord desktop IPC is only active on Windows".to_string())
    }
    fn send_frame(&mut self, _opcode: u32, _payload: &Value) -> Result<(), String> {
        Ok(())
    }
    fn read_frame(&mut self) -> Result<(u32, Value), String> {
        Ok((OP_FRAME, Value::Null))
    }
    fn close_gracefully(&mut self) {}
}

fn worker_loop(rx: Receiver<RpcCommand>, mut cfg: DiscordRpcSettings, status: Arc<Mutex<String>>) {
    let mut pipe: Option<DiscordPipe> = None;
    let mut current_snap: Option<DiscordTrackSnapshot> = None;
    let mut nonce: u64 = 1;
    let pid = std::process::id();

    let send_now = |pipe_opt: &mut Option<DiscordPipe>,
                    cfg: &DiscordRpcSettings,
                    snap: Option<&DiscordTrackSnapshot>,
                    nonce: &mut u64,
                    status: &Arc<Mutex<String>>| {
        if !cfg.enabled {
            if let Some(mut p) = pipe_opt.take() {
                *nonce += 1;
                let clr = build_set_activity_payload(cfg, None, pid, *nonce, unix_now_secs());
                let _ = p.send_frame(OP_FRAME, &clr);
                p.close_gracefully();
            }
            set_status(status, "Disabled");
            return;
        }
        let cid = effective_client_id(cfg);
        if cid.is_empty() {
            if let Some(mut p) = pipe_opt.take() {
                p.close_gracefully();
            }
            set_status(status, "Enter your Discord Application ID below");
            return;
        }
        if pipe_opt.is_none() {
            match DiscordPipe::connect(cid) {
                Ok(p) => {
                    *pipe_opt = Some(p);
                    set_status(status, "Connected to Discord");
                }
                Err(err) => {
                    set_status(status, err);
                    return;
                }
            }
        }
        if let Some(p) = pipe_opt.as_mut() {
            *nonce += 1;
            let payload = build_set_activity_payload(cfg, snap, pid, *nonce, unix_now_secs());
            if p.send_frame(OP_FRAME, &payload).is_err() {
                *pipe_opt = None;
                set_status(status, "Waiting for Discord desktop app...");
                return;
            }
            match p.read_frame() {
                Ok((OP_PING, ping_val)) => {
                    let _ = p.send_frame(OP_PONG, &ping_val);
                }
                Ok((OP_CLOSE, val)) => {
                    *pipe_opt = None;
                    let msg = val
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Disconnected from Discord");
                    set_status(status, msg);
                }
                Ok((OP_FRAME, val)) => {
                    if val.get("evt").and_then(Value::as_str) == Some("ERROR") {
                        let msg = val
                            .pointer("/data/message")
                            .and_then(Value::as_str)
                            .unwrap_or("Discord rejected activity update");
                        set_status(status, msg);
                    } else if snap.is_some_and(|s| s.is_playing || cfg.show_when_paused) {
                        set_status(
                            status,
                            "Connected to Discord · Broadcasting Listening status",
                        );
                    } else {
                        set_status(status, "Connected to Discord · Idle");
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    *pipe_opt = None;
                    set_status(status, "Waiting for Discord desktop app...");
                }
            }
        }
    };

    if cfg.enabled && !effective_client_id(&cfg).is_empty() {
        send_now(&mut pipe, &cfg, current_snap.as_ref(), &mut nonce, &status);
    }

    loop {
        // Sleep completely (0% CPU) when disabled or already connected; retry every 8s when waiting for Discord.
        let wait = if cfg.enabled && !effective_client_id(&cfg).is_empty() && pipe.is_none() {
            Some(Duration::from_secs(8))
        } else {
            None
        };

        let next = match wait {
            Some(dur) => match rx.recv_timeout(dur) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => {
                    send_now(&mut pipe, &cfg, current_snap.as_ref(), &mut nonce, &status);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => rx.recv().ok(),
        };

        let Some(cmd) = next else {
            break;
        };

        match cmd {
            RpcCommand::Configure(new_cfg) => {
                let id_changed = new_cfg.client_id.trim() != cfg.client_id.trim();
                cfg = new_cfg;
                if id_changed && let Some(mut old) = pipe.take() {
                    old.close_gracefully();
                }
                send_now(&mut pipe, &cfg, current_snap.as_ref(), &mut nonce, &status);
            }
            RpcCommand::UpdatePlayback(snap) => {
                current_snap = snap;
                if cfg.enabled {
                    send_now(&mut pipe, &cfg, current_snap.as_ref(), &mut nonce, &status);
                }
            }
            RpcCommand::Shutdown => {
                if let Some(mut p) = pipe.take() {
                    nonce += 1;
                    let clr = build_set_activity_payload(&cfg, None, pid, nonce, unix_now_secs());
                    let _ = p.send_frame(OP_FRAME, &clr);
                    p.close_gracefully();
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_listening_payload_uses_type_2_and_unix_seconds() {
        let cfg = DiscordRpcSettings {
            enabled: true,
            client_id: "123456789012345678".to_string(),
            status_format: DiscordStatusFormat::ArtistTitle,
            show_when_paused: false,
            show_cover_art: true,
            hide_details: false,
        };
        let snap = DiscordTrackSnapshot {
            title: "I Love You So".to_string(),
            artist: "The Walters".to_string(),
            album: "Songs for Dads".to_string(),
            cover_url: Some("https://lh3.googleusercontent.com/abc=w60-h60-l90-rj".to_string()),
            position_ms: 30_000,
            duration_ms: 160_000,
            is_playing: true,
        };
        let payload = build_set_activity_payload(&cfg, Some(&snap), 999, 1, 1_750_000_000);
        assert_eq!(payload["args"]["activity"]["type"], 2);
        assert_eq!(payload["args"]["activity"]["status_display_type"], 1);
        assert_eq!(payload["args"]["activity"]["details"], "I Love You So");
        assert_eq!(
            payload["args"]["activity"]["state"],
            "The Walters - I Love You So"
        );
        assert_eq!(
            payload["args"]["activity"]["timestamps"]["start"],
            1_749_999_970
        );
        assert_eq!(
            payload["args"]["activity"]["timestamps"]["end"],
            1_750_000_130
        );
        assert_eq!(
            payload["args"]["activity"]["assets"]["large_image"],
            "https://lh3.googleusercontent.com/abc=w512-h512-l90-rj"
        );
        assert_eq!(
            payload["args"]["activity"]["assets"]["large_text"],
            "Songs for Dads"
        );
        assert_eq!(
            payload["args"]["activity"]["assets"]["small_image"],
            FERRIC_BRAND_ICON_URL
        );

        let single_snap = DiscordTrackSnapshot {
            album: "I Love You So".to_string(),
            ..snap
        };
        let single_payload =
            build_set_activity_payload(&cfg, Some(&single_snap), 999, 2, 1_750_000_000);
        assert!(
            single_payload["args"]["activity"]["assets"]
                .get("large_text")
                .is_none(),
            "single tracks whose album equals title must omit large_text so no extra 3rd line appears"
        );
    }

    #[test]
    fn discord_clears_activity_when_paused_and_disabled_on_pause() {
        let cfg = DiscordRpcSettings {
            enabled: true,
            show_when_paused: false,
            ..Default::default()
        };
        let snap = DiscordTrackSnapshot {
            title: "Track".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            cover_url: None,
            position_ms: 10_000,
            duration_ms: 100_000,
            is_playing: false,
        };
        let payload = build_set_activity_payload(&cfg, Some(&snap), 1, 2, 1_750_000_000);
        assert!(payload["args"]["activity"].is_null());
    }

    #[test]
    fn ipc_frame_header_encodes_little_endian_opcode_and_length() {
        let frame = encode_ipc_frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "123" }));
        let op = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
        let len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
        assert_eq!(op, OP_HANDSHAKE);
        assert_eq!(len, frame.len() - 8);
    }

    #[test]
    fn builtin_discord_app_id_is_used_automatically_and_overridable() {
        let default_cfg = DiscordRpcSettings::default();
        assert_eq!(effective_client_id(&default_cfg), BUILTIN_DISCORD_APP_ID);
        assert!(is_valid_client_id(effective_client_id(&default_cfg)));

        let custom_cfg = DiscordRpcSettings {
            client_id: "1234567890123456789".to_string(),
            ..Default::default()
        };
        assert_eq!(effective_client_id(&custom_cfg), "1234567890123456789");
    }
}
