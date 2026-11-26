use ferric_core::app_data_dir;
use ferric_core::models::Track;
use ferric_providers::local_scanner::read_embedded_cover_bytes;
use ferric_providers::ytmusic::{cached_artwork_file, normalize_ytm_thumbnail_url};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub enum SmtcCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    SetPositionMs(u64),
    SeekRelativeMs(i64),
}

/// Windows System Media Transport Controls (SMTC) bridge for hardware media keys, Windows 11
/// Quick Settings flyout, and third-party SMTC Dynamic Island overlays (Nusa Pro, NotchPixel, etc.).
pub struct SmtcBridge {
    controls: Option<MediaControls>,
    rx: Receiver<SmtcCommand>,
    last_title: String,
    last_artist: String,
    last_album: String,
    last_duration_ms: u64,
    last_cover_url: Option<String>,
    cover_is_local_file: bool,
    last_playing: Option<bool>,
    last_pos_ms: u64,
    last_sync_at: Option<Instant>,
}

impl SmtcBridge {
    pub fn try_init() -> Self {
        let (tx, rx) = mpsc::channel();
        let controls = create_platform_controls(tx);
        Self {
            controls,
            rx,
            last_title: String::new(),
            last_artist: String::new(),
            last_album: String::new(),
            last_duration_ms: 0,
            last_cover_url: None,
            cover_is_local_file: false,
            last_playing: None,
            last_pos_ms: 0,
            last_sync_at: None,
        }
    }

    pub fn try_attach_late(&mut self) {
        if self.controls.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        if let Some(ctrl) = create_platform_controls(tx) {
            self.controls = Some(ctrl);
            self.rx = rx;
            if !self.last_title.is_empty() {
                self.push_cached_metadata();
            }
        }
    }

    pub fn poll_commands(&self) -> Vec<SmtcCommand> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.rx.try_recv() {
            out.push(cmd);
        }
        out
    }

    /// Update SMTC display metadata (title, artist, album, duration, and local-first cover thumbnail).
    pub fn set_metadata_for_track(&mut self, track: &Track) {
        self.try_attach_late();
        let (cover_url, is_local) = resolve_smtc_cover_url(track);
        self.last_title = track.title.clone();
        self.last_artist = track.artist.clone();
        self.last_album = track.album.clone();
        self.last_duration_ms = track.duration_ms;
        self.last_cover_url = cover_url;
        self.cover_is_local_file = is_local;
        self.last_pos_ms = 0;
        self.last_sync_at = None;
        self.push_cached_metadata();
    }

    /// Upgrade SMTC cover thumbnail from remote HTTPS URL to a verified local `file://` stream
    /// once the artwork worker finishes caching the active track cover on disk.
    pub fn refresh_cover_if_cached(&mut self, track: &Track) {
        if self.cover_is_local_file || self.last_title != track.title {
            return;
        }
        let (cover_url, is_local) = resolve_smtc_cover_url(track);
        if is_local && cover_url.is_some() {
            self.last_cover_url = cover_url;
            self.cover_is_local_file = true;
            self.push_cached_metadata();
        }
    }

    /// Synchronize SMTC playback status and real-time timeline position without ever resetting
    /// `Position` back to `00:00`. Throttled to state changes, seeks, duration discovery, or 1 Hz.
    pub fn sync_playback(&mut self, playing: bool, position_ms: u64, duration_ms: u64) {
        self.try_attach_late();
        if self.controls.is_none() {
            return;
        }

        // If Symphonia resolved exact stream duration after initial metadata push, update EndTime.
        if duration_ms > 0
            && (duration_ms as i64 - self.last_duration_ms as i64).abs() > 1_000
            && !self.last_title.is_empty()
        {
            self.last_duration_ms = duration_ms;
            self.push_cached_metadata();
        }

        let state_changed = self.last_playing != Some(playing);
        let elapsed_ms = self
            .last_sync_at
            .map_or(u64::MAX, |t| t.elapsed().as_millis() as u64);
        let expected_pos = if self.last_playing == Some(true) {
            self.last_pos_ms.saturating_add(elapsed_ms.min(60_000))
        } else {
            self.last_pos_ms
        };
        let jumped = (position_ms as i64 - expected_pos as i64).abs() > 1_200;
        let periodic_due = playing && elapsed_ms >= 1_000;

        if !state_changed && !jumped && !periodic_due {
            return;
        }

        if let Some(ctrl) = &mut self.controls {
            let pos = Some(MediaPosition(Duration::from_millis(position_ms)));
            let state = if playing {
                MediaPlayback::Playing { progress: pos }
            } else {
                MediaPlayback::Paused { progress: pos }
            };
            if ctrl.set_playback(state).is_ok() {
                self.last_playing = Some(playing);
                self.last_pos_ms = position_ms;
                self.last_sync_at = Some(Instant::now());
            }
        }
    }

    pub fn set_stopped(&mut self) {
        self.last_playing = None;
        self.last_pos_ms = 0;
        self.last_sync_at = None;
        if let Some(ctrl) = &mut self.controls {
            let _ = ctrl.set_playback(MediaPlayback::Stopped);
        }
    }

    fn push_cached_metadata(&mut self) {
        let Some(ctrl) = &mut self.controls else {
            return;
        };
        let duration =
            (self.last_duration_ms > 0).then(|| Duration::from_millis(self.last_duration_ms));
        let res = ctrl.set_metadata(MediaMetadata {
            title: Some(self.last_title.as_str()),
            artist: Some(self.last_artist.as_str()),
            album: Some(self.last_album.as_str()),
            duration,
            cover_url: self.last_cover_url.as_deref(),
        });
        if res.is_err() && self.last_cover_url.is_some() {
            // Fallback without thumbnail URI so title/artist/duration always publish cleanly.
            let _ = ctrl.set_metadata(MediaMetadata {
                title: Some(self.last_title.as_str()),
                artist: Some(self.last_artist.as_str()),
                album: Some(self.last_album.as_str()),
                duration,
                cover_url: None,
            });
        }
    }
}

fn write_smtc_cover_bytes(bytes: &[u8], cache_dir: &std::path::Path) -> Option<PathBuf> {
    let ext = if bytes.len() >= 8 && bytes[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    {
        "png"
    } else if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        "jpg"
    } else {
        return None;
    };
    let _ = std::fs::create_dir_all(cache_dir);
    let dest = cache_dir.join(format!("smtc_now_playing.{ext}"));
    std::fs::write(&dest, bytes).ok()?;
    Some(dest)
}

/// Resolve the most reliable SMTC thumbnail URI for a track:
/// 1. Local `file://` copy from disk `artwork_cache` or embedded audio tags (100% reliable in
///    sandboxed Microsoft Store Dynamic Island apps like Nusa Pro & NotchPixel).
/// 2. Normalized HTTPS cover URL fallback until disk artwork finishes downloading.
fn resolve_smtc_cover_url(track: &Track) -> (Option<String>, bool) {
    let cache_dir = app_data_dir().join("artwork_cache");

    let remote_url = track
        .thumbnail_url
        .as_deref()
        .map(normalize_ytm_thumbnail_url)
        .filter(|s| s.starts_with("https://"))
        .or_else(|| {
            track
                .youtube_video_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .or_else(|| track.id.strip_prefix("yt:video:").filter(|s| !s.is_empty()))
                .map(|vid| format!("https://i.ytimg.com/vi/{vid}/mqdefault.jpg"))
        });

    if let Some(ref url) = remote_url
        && let Some(cached_path) = cached_artwork_file(url, &cache_dir)
        && let Ok(bytes) = std::fs::read(&cached_path)
        && let Some(typed_path) = write_smtc_cover_bytes(&bytes, &cache_dir)
    {
        return (Some(format!("file://{}", typed_path.display())), true);
    }

    if let Some(path) = track.path.as_ref().filter(|p| p.is_file())
        && let Some(bytes) = read_embedded_cover_bytes(path)
        && let Some(typed_path) = write_smtc_cover_bytes(&bytes, &cache_dir)
    {
        return (Some(format!("file://{}", typed_path.display())), true);
    }

    (remote_url, false)
}

#[cfg(target_os = "windows")]
fn find_current_process_hwnd() -> Option<*mut std::ffi::c_void> {
    let raw = crate::shell::win32_main_hwnd_raw();
    (raw != 0).then_some(raw as *mut std::ffi::c_void)
}

#[cfg(not(target_os = "windows"))]
fn find_current_process_hwnd() -> Option<*mut std::ffi::c_void> {
    None
}

fn create_platform_controls(tx: mpsc::Sender<SmtcCommand>) -> Option<MediaControls> {
    #[cfg(target_os = "windows")]
    let hwnd = find_current_process_hwnd()?;
    #[cfg(not(target_os = "windows"))]
    let hwnd: Option<*mut std::ffi::c_void> = None;

    #[cfg(target_os = "windows")]
    let cfg = PlatformConfig {
        dbus_name: "ferric_music_player",
        display_name: "Ferric",
        hwnd: Some(hwnd),
    };
    #[cfg(not(target_os = "windows"))]
    let cfg = PlatformConfig {
        dbus_name: "ferric_music_player",
        display_name: "Ferric",
        hwnd,
    };

    let mut controls = MediaControls::new(cfg).ok()?;
    controls
        .attach(move |event| {
            let cmd = match event {
                MediaControlEvent::Play => Some(SmtcCommand::Play),
                MediaControlEvent::Pause => Some(SmtcCommand::Pause),
                MediaControlEvent::Toggle => Some(SmtcCommand::Toggle),
                MediaControlEvent::Next => Some(SmtcCommand::Next),
                MediaControlEvent::Previous => Some(SmtcCommand::Previous),
                MediaControlEvent::Stop => Some(SmtcCommand::Stop),
                MediaControlEvent::SetPosition(pos) => {
                    Some(SmtcCommand::SetPositionMs(pos.0.as_millis() as u64))
                }
                MediaControlEvent::Seek(SeekDirection::Forward) => {
                    Some(SmtcCommand::SeekRelativeMs(10_000))
                }
                MediaControlEvent::Seek(SeekDirection::Backward) => {
                    Some(SmtcCommand::SeekRelativeMs(-10_000))
                }
                MediaControlEvent::SeekBy(SeekDirection::Forward, d) => {
                    Some(SmtcCommand::SeekRelativeMs(d.as_millis() as i64))
                }
                MediaControlEvent::SeekBy(SeekDirection::Backward, d) => {
                    Some(SmtcCommand::SeekRelativeMs(-(d.as_millis() as i64)))
                }
                _ => None,
            };
            if let Some(c) = cmd {
                let _ = tx.send(c);
            }
        })
        .ok()?;
    Some(controls)
}
