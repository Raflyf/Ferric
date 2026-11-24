use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum SmtcCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
}

/// Optional Windows System Media Transport Controls (SMTC) bridge for hardware media keys & Win11 flyout.
pub struct SmtcBridge {
    controls: Option<MediaControls>,
    rx: Receiver<SmtcCommand>,
}

impl SmtcBridge {
    pub fn try_init() -> Self {
        let (tx, rx) = mpsc::channel();
        let controls = create_platform_controls(tx);
        Self { controls, rx }
    }

    pub fn try_attach_late(&mut self) {
        if self.controls.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        if let Some(ctrl) = create_platform_controls(tx) {
            self.controls = Some(ctrl);
            self.rx = rx;
        }
    }

    pub fn poll_commands(&self) -> Vec<SmtcCommand> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.rx.try_recv() {
            out.push(cmd);
        }
        out
    }

    pub fn set_metadata(&mut self, title: &str, artist: &str, album: &str, duration_ms: u64) {
        if let Some(ctrl) = &mut self.controls {
            let _ = ctrl.set_metadata(MediaMetadata {
                title: Some(title),
                artist: Some(artist),
                album: Some(album),
                duration: (duration_ms > 0).then(|| Duration::from_millis(duration_ms)),
                cover_url: None,
            });
        }
    }

    pub fn set_playing(&mut self, playing: bool) {
        if let Some(ctrl) = &mut self.controls {
            let state = if playing {
                MediaPlayback::Playing { progress: None }
            } else {
                MediaPlayback::Paused { progress: None }
            };
            let _ = ctrl.set_playback(state);
        }
    }
}

#[cfg(target_os = "windows")]
fn find_current_process_hwnd() -> Option<*mut std::ffi::c_void> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetActiveWindow() -> *mut std::ffi::c_void;
        fn GetForegroundWindow() -> *mut std::ffi::c_void;
        fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, lpdw_process_id: *mut u32) -> u32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcessId() -> u32;
    }

    // SAFETY: Standard Win32 queries with valid out-pointer and null checks.
    unsafe {
        let pid = GetCurrentProcessId();
        for hwnd in [GetActiveWindow(), GetForegroundWindow()] {
            if !hwnd.is_null() {
                let mut win_pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, &mut win_pid);
                if win_pid == pid {
                    return Some(hwnd);
                }
            }
        }
    }
    None
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
                _ => None,
            };
            if let Some(c) = cmd {
                let _ = tx.send(c);
            }
        })
        .ok()?;
    Some(controls)
}
