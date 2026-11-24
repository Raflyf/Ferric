use std::path::Path;

/// Allowed external HTTPS origins that Ferric may open in the system browser.
const ALLOWED_HTTPS_PREFIXES: &[&str] = &[
    "https://github.com/Raflyf/Ferric",
    "https://accounts.spotify.com/",
    "https://developer.spotify.com/",
    "https://accounts.google.com/",
    "https://console.cloud.google.com/",
    "https://www.youtube.com/watch?v=",
    "https://www.youtube.com/results?search_query=",
];

/// Launch a local sidecar music video (`.mp4`, `.webm`, `.mkv`) if present next to the audio file,
/// or open the official YouTube video / music video search in the system browser/PiP player.
pub fn open_track_video_clip(
    local_audio_path: Option<&Path>,
    youtube_video_id: Option<&str>,
    artist: &str,
    title: &str,
) -> Result<&'static str, String> {
    if let Some(audio_path) = local_audio_path {
        for ext in ["mp4", "webm", "mkv"] {
            let candidate = audio_path.with_extension(ext);
            if candidate.is_file() {
                let target = candidate.to_string_lossy();
                let dir = candidate.parent().map(|p| p.to_string_lossy().into_owned());
                shell_open(&target, dir.as_deref())?;
                return Ok("Opened local sidecar video clip");
            }
        }
    }
    if let Some(vid) = youtube_video_id
        && !vid.trim().is_empty()
        && vid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        let url = format!("https://www.youtube.com/watch?v={vid}");
        open_allowed_url(&url)?;
        return Ok("Opened YouTube video clip");
    }
    let query = simple_url_encode(&format!("{artist} {title} official music video"));
    let url = format!("https://www.youtube.com/results?search_query={query}");
    open_allowed_url(&url)?;
    Ok("Opened official music video search")
}

fn simple_url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Open an allowlisted HTTPS URL (GitHub Releases or official Spotify/Google OAuth pages) fail-closed.
pub fn open_allowed_url(url: &str) -> Result<(), String> {
    if !ALLOWED_HTTPS_PREFIXES.iter().any(|p| url.starts_with(p)) {
        return Err("Refused to open untrusted external URL".to_string());
    }
    shell_open(url, None)
}

/// Backwards-compatible helper for GitHub release links.
pub fn open_github_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://github.com/Raflyf/Ferric") {
        return Err("Refused to open non-Ferric GitHub URL".to_string());
    }
    shell_open(url, None)
}

/// Launch a SHA-256-verified `.msi` installer file using Windows ShellExecuteW.
pub fn launch_verified_installer(msi_path: &Path) -> Result<(), String> {
    if !msi_path.is_file() {
        return Err("Verified installer file does not exist".to_string());
    }
    let ext_ok = msi_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"));
    if !ext_ok {
        return Err("Refused to launch non-MSI installer file".to_string());
    }
    let target = msi_path.to_string_lossy();
    let dir = msi_path.parent().map(|p| p.to_string_lossy().into_owned());
    shell_open(&target, dir.as_deref())
}

/// Copy UTF-8 text into the native OS clipboard (used for OAuth Redirect URI, Pairing URI, and APO export).
#[cfg(target_os = "windows")]
pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    use std::ffi::c_void;
    use std::ptr;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(hwnd_new_owner: *mut c_void) -> i32;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(u_format: u32, h_mem: *mut c_void) -> *mut c_void;
        fn CloseClipboard() -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(u_flags: u32, dw_bytes: usize) -> *mut c_void;
        fn GlobalLock(h_mem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(h_mem: *mut c_void) -> i32;
    }

    const CF_UNICODETEXT: u32 = 13;
    const GMEM_MOVEABLE: u32 = 0x0002;

    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let byte_len = wide.len() * std::mem::size_of::<u16>();

    // SAFETY: Standard Win32 CF_UNICODETEXT clipboard transfer with checked GlobalAlloc/GlobalLock pointers.
    unsafe {
        if OpenClipboard(ptr::null_mut()) == 0 {
            return Err("Could not open Windows clipboard".to_string());
        }
        let _ = EmptyClipboard();
        let h_mem = GlobalAlloc(GMEM_MOVEABLE, byte_len);
        if h_mem.is_null() {
            let _ = CloseClipboard();
            return Err("Clipboard GlobalAlloc failed".to_string());
        }
        let dst = GlobalLock(h_mem) as *mut u16;
        if dst.is_null() {
            let _ = CloseClipboard();
            return Err("Clipboard GlobalLock failed".to_string());
        }
        ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
        let _ = GlobalUnlock(h_mem);
        if SetClipboardData(CF_UNICODETEXT, h_mem).is_null() {
            let _ = CloseClipboard();
            return Err("SetClipboardData failed".to_string());
        }
        let _ = CloseClipboard();
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn copy_text_to_clipboard(_text: &str) -> Result<(), String> {
    Ok(())
}

/// Direct Win32 window state helpers ensuring custom frameless Minimize / Maximize / Restore
/// buttons operate reliably on the active Ferric window at any DPI scale.
#[cfg(target_os = "windows")]
pub fn win32_minimize_active_window() {
    if let Some(hwnd) = active_process_hwnd() {
        // SAFETY: Valid HWND owned by the current process.
        unsafe {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn win32_minimize_active_window() {}

#[cfg(target_os = "windows")]
pub fn win32_toggle_maximize_active_window() -> bool {
    if let Some(hwnd) = active_process_hwnd() {
        // SAFETY: Valid HWND owned by the current process.
        unsafe {
            let currently_max = IsZoomed(hwnd) != 0;
            let _ = ShowWindow(
                hwnd,
                if currently_max {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                },
            );
            return !currently_max;
        }
    }
    false
}

#[cfg(not(target_os = "windows"))]
pub fn win32_toggle_maximize_active_window() -> bool {
    false
}

#[cfg(target_os = "windows")]
pub fn win32_is_active_window_maximized() -> bool {
    if let Some(hwnd) = active_process_hwnd() {
        // SAFETY: Valid HWND owned by the current process.
        unsafe { IsZoomed(hwnd) != 0 }
    } else {
        false
    }
}

#[cfg(not(target_os = "windows"))]
pub fn win32_is_active_window_maximized() -> bool {
    false
}

#[cfg(target_os = "windows")]
const SW_SHOWNORMAL: i32 = 1;
#[cfg(target_os = "windows")]
const SW_MAXIMIZE: i32 = 3;
#[cfg(target_os = "windows")]
const SW_MINIMIZE: i32 = 6;
#[cfg(target_os = "windows")]
const SW_RESTORE: i32 = 9;

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetActiveWindow() -> *mut std::ffi::c_void;
    fn GetForegroundWindow() -> *mut std::ffi::c_void;
    fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, lpdw_process_id: *mut u32) -> u32;
    fn ShowWindow(hwnd: *mut std::ffi::c_void, n_cmd_show: i32) -> i32;
    fn IsZoomed(hwnd: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "windows")]
fn active_process_hwnd() -> Option<*mut std::ffi::c_void> {
    // SAFETY: Win32 thread/process queries on opaque window handles.
    unsafe {
        let active = GetActiveWindow();
        if !active.is_null() {
            return Some(active);
        }
        let fg = GetForegroundWindow();
        if !fg.is_null() {
            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(fg, &mut pid);
            if pid == std::process::id() {
                return Some(fg);
            }
        }
        None
    }
}

#[cfg(target_os = "windows")]
fn shell_open(target: &str, working_dir: Option<&str>) -> Result<(), String> {
    use std::ffi::c_void;
    use std::ptr;

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: *mut c_void,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }

    let op: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    let dir_buf: Option<Vec<u16>> = working_dir.map(|d| d.encode_utf16().chain(Some(0)).collect());
    let dir_ptr = dir_buf.as_ref().map_or(ptr::null(), Vec::as_ptr);

    // SAFETY: NUL-terminated UTF-16 buffers live for the duration of ShellExecuteW.
    let res = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            dir_ptr,
            SW_SHOWNORMAL,
        )
    };
    if res <= 32 {
        Err(format!("Windows ShellExecuteW failed (code {res})"))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn shell_open(_target: &str, _working_dir: Option<&str>) -> Result<(), String> {
    Err("Shell launcher is implemented for Windows desktop builds".to_string())
}
