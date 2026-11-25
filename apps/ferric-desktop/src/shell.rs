use std::path::Path;

/// Allowed external HTTPS origins that Ferric may open in the system browser (OAuth & GitHub Releases only).
const ALLOWED_HTTPS_PREFIXES: &[&str] = &[
    "https://github.com/Raflyf/Ferric",
    "https://accounts.spotify.com/",
    "https://developer.spotify.com/",
    "https://accounts.google.com/",
    "https://console.cloud.google.com/",
];

/// Events emitted by the Win32 window subclass & System Tray icon back to Ferric's UI loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    ShowWindow,
    HiddenToTray,
    TogglePlayPause,
    NextTrack,
    PrevTrack,
    QuitApp,
    WindowMovedOrResized,
}

/// Physical screen-pixel rectangle of Ferric's client area plus monitor DPI scale.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WindowClientScreenRect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    pub dpi_scale: f32,
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
    open_allowed_url(url)
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

#[cfg(target_os = "windows")]
mod win_impl {
    use super::{TrayEvent, WindowClientScreenRect};
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetClientRect, GetCursorPos, IDI_APPLICATION,
        LoadIconW, MF_SEPARATOR, MF_STRING, MINMAXINFO, NCCALCSIZE_PARAMS, PostMessageW, SW_HIDE,
        SW_RESTORE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetForegroundWindow, SetWindowPos, ShowWindow, TPM_BOTTOMALIGN,
        TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE,
        WM_DESTROY, WM_GETMINMAXINFO, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_MOVE, WM_NCCALCSIZE,
        WM_RBUTTONUP, WM_SHOWWINDOW, WM_SIZE, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED, WS_CAPTION,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };
    use windows::core::{PCWSTR, s, w};

    const SUBCLASS_ID: usize = 0xFE11_C001;
    const TRAY_ICON_ID: u32 = 1;
    const WM_TRAYICON: u32 = WM_APP + 42;
    const ERROR_ALREADY_EXISTS: u32 = 183;

    const CMD_SHOW: u32 = 1001;
    const CMD_PLAY_PAUSE: u32 = 1002;
    const CMD_NEXT: u32 = 1003;
    const CMD_PREV: u32 = 1004;
    const CMD_QUIT: u32 = 1005;

    const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMWCP_DONOTROUND: i32 = 1;
    const DWMWCP_ROUND: i32 = 2;
    const DWMSBT_NONE: i32 = 1;
    const DWMSBT_TRANSIENTWINDOW: i32 = 3; // Native Windows 11 Acrylic
    const GWL_STYLE: i32 = -16;

    static SINGLE_INSTANCE_MUTEX: AtomicIsize = AtomicIsize::new(0);
    static ACTIVATE_MSG: AtomicU32 = AtomicU32::new(0);
    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
    static ACRYLIC_ENABLED: AtomicBool = AtomicBool::new(false);
    static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
    static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
    static FORCE_QUIT: AtomicBool = AtomicBool::new(false);
    static TRAY_ADDED: AtomicBool = AtomicBool::new(false);
    static MINI_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);
    static WINDOW_HIDDEN_OR_MIN: AtomicBool = AtomicBool::new(false);
    static TRAY_EVENTS: OnceLock<Mutex<Vec<TrayEvent>>> = OnceLock::new();

    #[repr(C)]
    struct DwmMargins {
        cx_left_width: i32,
        cx_right_width: i32,
        cy_top_height: i32,
        cy_bottom_height: i32,
    }

    #[repr(C)]
    struct AccentPolicy {
        accent_state: u32,
        accent_flags: u32,
        gradient_color: u32,
        animation_id: u32,
    }

    #[repr(C)]
    struct WindowCompositionAttribData {
        attrib: u32,
        pv_data: *mut c_void,
        cb_data: usize,
    }

    type SubclassProc =
        unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT;

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: HWND,
            dw_attribute: u32,
            pv_attribute: *const c_void,
            cb_attribute: u32,
        ) -> i32;
        fn DwmExtendFrameIntoClientArea(hwnd: HWND, p_mar_inset: *const DwmMargins) -> i32;
    }

    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn SetWindowSubclass(
            hwnd: HWND,
            pfn_subclass: SubclassProc,
            uid_subclass: usize,
            dw_ref_data: usize,
        ) -> i32;
        fn DefSubclassProc(hwnd: HWND, umsg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
        fn RemoveWindowSubclass(hwnd: HWND, pfn_subclass: SubclassProc, uid_subclass: usize)
        -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetActiveWindow() -> *mut c_void;
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, lpdw_process_id: *mut u32) -> u32;
        fn EnumWindows(
            lp_enum_func: unsafe extern "system" fn(*mut c_void, isize) -> i32,
            l_param: isize,
        ) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn IsIconic(hwnd: *mut c_void) -> i32;
        fn IsZoomed(hwnd: *mut c_void) -> i32;
        fn GetDpiForWindow(hwnd: *mut c_void) -> u32;
        fn GetClassNameW(hwnd: *mut c_void, lp_class_name: *mut u16, n_max_count: i32) -> i32;
        fn GetWindowLongW(hwnd: HWND, n_index: i32) -> i32;
        fn SetWindowLongW(hwnd: HWND, n_index: i32, dw_new_long: i32) -> i32;
        fn RegisterWindowMessageW(lp_string: *const u16) -> u32;
        fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16) -> *mut c_void;
        fn AllowSetForegroundWindow(dw_process_id: u32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            lp_mutex_attributes: *mut c_void,
            b_initial_owner: i32,
            lp_name: *const u16,
        ) -> *mut c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
    }

    fn activate_message_id() -> u32 {
        let cached = ACTIVATE_MSG.load(Ordering::Acquire);
        if cached != 0 {
            return cached;
        }
        let name: Vec<u16> = "Ferric.MusicPlayer.ActivateSingleInstance.v1\0"
            .encode_utf16()
            .collect();
        // SAFETY: Null-terminated UTF-16 message name passed to RegisterWindowMessageW.
        let id = unsafe { RegisterWindowMessageW(name.as_ptr()) };
        if id != 0 {
            ACTIVATE_MSG.store(id, Ordering::Release);
        }
        id
    }

    unsafe extern "system" fn enum_proc_find_other_ferric_hwnd(
        hwnd: *mut c_void,
        l_param: isize,
    ) -> i32 {
        // SAFETY: `l_param` points to stack-owned `*mut c_void`.
        unsafe {
            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, &mut pid);
            if pid != 0 && pid != std::process::id() && is_makepad_window(hwnd) {
                let out_slot = l_param as *mut *mut c_void;
                *out_slot = hwnd;
                return 0;
            }
            1
        }
    }

    /// Ensure only one Ferric desktop instance runs per user session.
    /// If Ferric is already running (even minimized or hidden in System Tray),
    /// signal the existing instance to restore and focus itself, then return `false`.
    pub fn acquire_single_instance_or_activate() -> bool {
        let msg_id = activate_message_id();
        let mutex_name: Vec<u16> = "Local\\FerricMusicPlayerSingleInstance_v1\0"
            .encode_utf16()
            .collect();
        // SAFETY: Null-terminated UTF-16 named mutex scoped to current desktop session.
        unsafe {
            let handle = CreateMutexW(ptr::null_mut(), 0, mutex_name.as_ptr());
            let last_err = GetLastError();
            if handle.is_null() {
                return true;
            }
            if last_err == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                let cls: Vec<u16> = "MakepadWindow\0".encode_utf16().collect();
                let title: Vec<u16> = "Ferric\0".encode_utf16().collect();
                let mut target = FindWindowW(cls.as_ptr(), title.as_ptr());
                if target.is_null() {
                    let slot_ptr = (&raw mut target) as isize;
                    let _ = EnumWindows(enum_proc_find_other_ferric_hwnd, slot_ptr);
                }
                if !target.is_null() {
                    let mut existing_pid = 0u32;
                    let _ = GetWindowThreadProcessId(target, &mut existing_pid);
                    if existing_pid != 0 {
                        let _ = AllowSetForegroundWindow(existing_pid);
                    }
                    if msg_id != 0 {
                        let _ = PostMessageW(Some(HWND(target)), msg_id, WPARAM(0), LPARAM(0));
                    } else {
                        let _ = ShowWindow(HWND(target), SW_SHOW);
                        if IsIconic(target) != 0 {
                            let _ = ShowWindow(HWND(target), SW_RESTORE);
                        }
                        let _ = SetForegroundWindow(HWND(target));
                    }
                }
                return false;
            }
            SINGLE_INSTANCE_MUTEX.store(handle as isize, Ordering::Release);
            true
        }
    }

    unsafe fn apply_dwm_corner_pref(hwnd: HWND, maximized: bool) {
        let pref = if maximized {
            DWMWCP_DONOTROUND
        } else {
            DWMWCP_ROUND
        };
        // SAFETY: Valid HWND and 4-byte i32 attribute value.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const pref).cast(),
                std::mem::size_of::<i32>() as u32,
            );
        }
    }

    unsafe fn apply_wca_acrylic_fallback(hwnd: HWND, enabled: bool) {
        // SAFETY: Dynamically resolves SetWindowCompositionAttribute from user32.dll for Windows 10 fallback.
        unsafe {
            if let Ok(user32) = GetModuleHandleW(w!("user32.dll"))
                && let Some(proc_fn) = GetProcAddress(user32, s!("SetWindowCompositionAttribute"))
            {
                type SetWcaFn =
                    unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32;
                let set_wca: SetWcaFn = std::mem::transmute(proc_fn);
                let mut policy = AccentPolicy {
                    accent_state: if enabled { 4 } else { 0 },
                    accent_flags: if enabled { 2 } else { 0 },
                    gradient_color: if enabled { 0x0110_0E16 } else { 0 },
                    animation_id: 0,
                };
                let mut data = WindowCompositionAttribData {
                    attrib: 19,
                    pv_data: (&raw mut policy).cast(),
                    cb_data: std::mem::size_of::<AccentPolicy>(),
                };
                let _ = set_wca(hwnd, &raw mut data);
            }
        }
    }

    pub fn set_native_acrylic(enabled: bool, _opacity: f32) {
        ACRYLIC_ENABLED.store(enabled, Ordering::Release);
        let Some(hwnd) = active_process_hwnd() else {
            return;
        };
        // SAFETY: Enables true per-pixel Windows 11 DWM Acrylic directly behind Ferric's
        // premultiplied DirectComposition swapchain — zero WS_EX_LAYERED text dimming.
        unsafe {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                std::mem::size_of::<i32>() as u32,
            );
            apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);

            let margins = DwmMargins {
                cx_left_width: -1,
                cx_right_width: -1,
                cy_top_height: -1,
                cy_bottom_height: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);

            let backdrop: i32 = if enabled {
                DWMSBT_TRANSIENTWINDOW
            } else {
                DWMSBT_NONE
            };
            let hr = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const backdrop).cast(),
                std::mem::size_of::<i32>() as u32,
            );
            if hr != 0 {
                // Windows 10 / older builds fallback
                apply_wca_acrylic_fallback(hwnd, enabled);
            }
        }
    }

    unsafe fn is_makepad_window(hwnd: *mut c_void) -> bool {
        if hwnd.is_null() {
            return false;
        }
        let mut buf = [0u16; 64];
        // SAFETY: Valid stack buffer passed to `GetClassNameW`.
        let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if len <= 0 {
            return false;
        }
        String::from_utf16_lossy(&buf[..len as usize]) == "MakepadWindow"
    }

    fn push_event(ev: TrayEvent) {
        let q = TRAY_EVENTS.get_or_init(|| Mutex::new(Vec::new()));
        if let Ok(mut guard) = q.lock() {
            // Coalesce repeated move/resize events
            if ev == TrayEvent::WindowMovedOrResized
                && guard.last() == Some(&TrayEvent::WindowMovedOrResized)
            {
                return;
            }
            guard.push(ev);
        }
    }

    pub fn poll_tray_events() -> Vec<TrayEvent> {
        let Some(q) = TRAY_EVENTS.get() else {
            return Vec::new();
        };
        q.lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default()
    }

    unsafe extern "system" fn enum_proc_find_own_hwnd(hwnd: *mut c_void, l_param: isize) -> i32 {
        // SAFETY: `l_param` points to stack-owned `*mut c_void`.
        unsafe {
            let mut pid = 0u32;
            let _ = GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == std::process::id() && is_makepad_window(hwnd) {
                let out_slot = l_param as *mut *mut c_void;
                *out_slot = hwnd;
                return 0;
            }
            1
        }
    }

    pub fn active_process_hwnd() -> Option<HWND> {
        let cached = MAIN_HWND.load(Ordering::Acquire);
        if cached != 0 {
            return Some(HWND(cached as *mut c_void));
        }
        // SAFETY: Win32 thread/process queries restricted strictly to our `MakepadWindow` HWND
        // so Windows IME / `UAC_InputIndicatorOverlayWnd` helper windows are never hooked.
        unsafe {
            let active = GetActiveWindow();
            if !active.is_null() && is_makepad_window(active) {
                return Some(HWND(active));
            }
            let fg = GetForegroundWindow();
            if !fg.is_null() && is_makepad_window(fg) {
                let mut pid = 0u32;
                let _ = GetWindowThreadProcessId(fg, &mut pid);
                if pid == std::process::id() {
                    return Some(HWND(fg));
                }
            }
            let mut found: *mut c_void = ptr::null_mut();
            let slot_ptr = (&raw mut found) as isize;
            let _ = EnumWindows(enum_proc_find_own_hwnd, slot_ptr);
            if !found.is_null() {
                Some(HWND(found))
            } else {
                None
            }
        }
    }

    pub fn main_hwnd_raw() -> isize {
        active_process_hwnd().map_or(0, |h| h.0 as isize)
    }

    /// Install the Win32 window subclass once so:
    /// 1. Maximizing stops cleanly above the Windows Taskbar (`rcWork`, zero bottom bar clip).
    /// 2. Optional Close-to-Tray intercepts `WM_CLOSE` cleanly while audio plays in background.
    /// 3. Window moves/resizes notify the docked native Video Clip host immediately.
    pub fn install_window_hooks() {
        if HOOK_INSTALLED.load(Ordering::Acquire) {
            return;
        }
        let Some(hwnd) = active_process_hwnd() else {
            return;
        };
        MAIN_HWND.store(hwnd.0 as isize, Ordering::Release);
        // SAFETY: `hwnd` is our own Makepad top-level window on the UI thread.
        let ok = unsafe { SetWindowSubclass(hwnd, ferric_subclass_proc, SUBCLASS_ID, 0) };
        if ok != 0 {
            HOOK_INSTALLED.store(true, Ordering::Release);
            // Upgrade WS_POPUP -> WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU
            // while WM_NCCALCSIZE keeps the window 100% frameless, unlocking native Windows 11 DWM
            // maximize / restore / minimize fly-zoom animations and Aero Snap!
            unsafe {
                let cur_style = GetWindowLongW(hwnd, GWL_STYLE);
                let next_style = (cur_style & !(WS_POPUP.0 as i32))
                    | (WS_CAPTION.0
                        | WS_THICKFRAME.0
                        | WS_MINIMIZEBOX.0
                        | WS_MAXIMIZEBOX.0
                        | WS_SYSMENU.0) as i32;
                if next_style != cur_style {
                    let _ = SetWindowLongW(hwnd, GWL_STYLE, next_style);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                let disable_trans: i32 = 0;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_TRANSITIONS_FORCEDISABLED,
                    (&raw const disable_trans).cast(),
                    std::mem::size_of::<i32>() as u32,
                );
            }
            if CLOSE_TO_TRAY.load(Ordering::Acquire) {
                ensure_tray_icon(hwnd, "Ferric — Music Player");
            }
            let acr_on = ACRYLIC_ENABLED.load(Ordering::Acquire);
            set_native_acrylic(acr_on, 0.72);
        }
    }

    pub fn set_close_to_tray(enabled: bool) {
        CLOSE_TO_TRAY.store(enabled, Ordering::Release);
        if let Some(hwnd) = active_process_hwnd() {
            if enabled {
                ensure_tray_icon(hwnd, "Ferric — Music Player");
            } else {
                remove_tray_icon(hwnd);
            }
        }
    }

    pub fn update_tray_tooltip(tip: &str) {
        if !TRAY_ADDED.load(Ordering::Acquire) {
            return;
        }
        if let Some(hwnd) = active_process_hwnd() {
            ensure_tray_icon(hwnd, tip);
        }
    }

    pub fn set_mini_mode_flag(mini: bool) {
        MINI_MODE_ACTIVE.store(mini, Ordering::Release);
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetProcessWorkingSetSizeEx(
            h_process: *mut c_void,
            dw_minimum_working_set_size: usize,
            dw_maximum_working_set_size: usize,
            flags: u32,
        ) -> i32;
    }

    pub fn trim_working_set() {
        // SAFETY: Official Win32 APIs that trim idle/scratch pages cleanly back to the OS
        // without affecting any live allocations or audio buffers.
        unsafe {
            let proc_handle = GetCurrentProcess();
            let _ = SetProcessWorkingSetSizeEx(proc_handle.0, usize::MAX, usize::MAX, 0);
            let _ = K32EmptyWorkingSet(proc_handle);
        }
    }

    pub fn client_rect_screen() -> Option<WindowClientScreenRect> {
        let hwnd = active_process_hwnd()?;
        // SAFETY: Standard Win32 client rect and DPI queries on our main HWND.
        unsafe {
            let mut rc = RECT::default();
            if GetClientRect(hwnd, &mut rc).is_err() {
                return None;
            }
            let mut tl = POINT {
                x: rc.left,
                y: rc.top,
            };
            let _ = ClientToScreen(hwnd, &mut tl);
            let dpi = GetDpiForWindow(hwnd.0);
            let dpi_scale = if dpi >= 96 { dpi as f32 / 96.0 } else { 1.0 };
            Some(WindowClientScreenRect {
                left: tl.x,
                top: tl.y,
                width: (rc.right - rc.left).max(1),
                height: (rc.bottom - rc.top).max(1),
                dpi_scale,
            })
        }
    }

    fn ensure_tray_icon(hwnd: HWND, tooltip: &str) {
        // SAFETY: Populate standard Win32 `NOTIFYICONDATAW` structure for our window.
        unsafe {
            let hinst = GetModuleHandleW(None).ok();
            let icon = hinst
                .and_then(|m| {
                    LoadIconW(Some(m.into()), PCWSTR(std::ptr::without_provenance(1))).ok()
                })
                .or_else(|| LoadIconW(None, IDI_APPLICATION).ok())
                .unwrap_or_default();

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ICON_ID,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAYICON,
                hIcon: icon,
                ..Default::default()
            };
            let wide: Vec<u16> = tooltip.encode_utf16().take(126).collect();
            for (i, ch) in wide.into_iter().enumerate() {
                nid.szTip[i] = ch;
            }
            if TRAY_ADDED.load(Ordering::Acquire) {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
            } else if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                TRAY_ADDED.store(true, Ordering::Release);
            }
        }
    }

    fn remove_tray_icon(hwnd: HWND) {
        if !TRAY_ADDED.swap(false, Ordering::AcqRel) {
            return;
        }
        // SAFETY: Standard `NIM_DELETE` for previously registered tray icon.
        unsafe {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ICON_ID,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    fn show_tray_context_menu(hwnd: HWND) {
        // SAFETY: Standard synchronous Win32 popup menu owned by our window on UI thread.
        unsafe {
            let Ok(hmenu) = CreatePopupMenu() else {
                return;
            };
            let _ = AppendMenuW(hmenu, MF_STRING, CMD_SHOW as usize, w!("Show Ferric"));
            let _ = AppendMenuW(
                hmenu,
                MF_STRING,
                CMD_PLAY_PAUSE as usize,
                w!("Play / Pause"),
            );
            let _ = AppendMenuW(hmenu, MF_STRING, CMD_NEXT as usize, w!("Next Track"));
            let _ = AppendMenuW(hmenu, MF_STRING, CMD_PREV as usize, w!("Previous Track"));
            let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(hmenu, MF_STRING, CMD_QUIT as usize, w!("Quit Ferric"));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(hwnd);
            let cmd = TrackPopupMenu(
                hmenu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                Some(0),
                hwnd,
                None,
            );
            let _ = DestroyMenu(hmenu);

            match cmd.0 as u32 {
                CMD_SHOW => {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                    push_event(TrayEvent::ShowWindow);
                }
                CMD_PLAY_PAUSE => push_event(TrayEvent::TogglePlayPause),
                CMD_NEXT => push_event(TrayEvent::NextTrack),
                CMD_PREV => push_event(TrayEvent::PrevTrack),
                CMD_QUIT => {
                    FORCE_QUIT.store(true, Ordering::Release);
                    remove_tray_icon(hwnd);
                    push_event(TrayEvent::QuitApp);
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                _ => {}
            }
        }
    }

    unsafe extern "system" fn ferric_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        uid: usize,
        _ref_data: usize,
    ) -> LRESULT {
        // SAFETY: Valid Win32 subclass callback executing on Ferric's UI thread.
        unsafe {
            let act_msg = ACTIVATE_MSG.load(Ordering::Acquire);
            if act_msg != 0 && msg == act_msg {
                let _ = ShowWindow(hwnd, SW_SHOW);
                if IsIconic(hwnd.0) != 0 {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
                WINDOW_HIDDEN_OR_MIN.store(false, Ordering::Release);
                apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);
                push_event(TrayEvent::ShowWindow);
                return LRESULT(0);
            }
            match msg {
                WM_GETMINMAXINFO => {
                    let _ = DefSubclassProc(hwnd, msg, wparam, lparam);
                    let mmi = lparam.0 as *mut MINMAXINFO;
                    if !mmi.is_null() {
                        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                        let mut mi = MONITORINFO {
                            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                            ..Default::default()
                        };
                        if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                            (*mmi).ptMaxPosition.x = mi.rcWork.left - mi.rcMonitor.left;
                            (*mmi).ptMaxPosition.y = mi.rcWork.top - mi.rcMonitor.top;
                            (*mmi).ptMaxSize.x = mi.rcWork.right - mi.rcWork.left;
                            (*mmi).ptMaxSize.y = mi.rcWork.bottom - mi.rcWork.top;
                            (*mmi).ptMaxTrackSize.x =
                                (mi.rcMonitor.right - mi.rcMonitor.left).max(1920);
                            (*mmi).ptMaxTrackSize.y =
                                (mi.rcMonitor.bottom - mi.rcMonitor.top).max(1080);
                        }
                        if MINI_MODE_ACTIVE.load(Ordering::Relaxed) {
                            (*mmi).ptMinTrackSize.x = 460;
                            (*mmi).ptMinTrackSize.y = 110;
                        } else {
                            (*mmi).ptMinTrackSize.x = 920;
                            (*mmi).ptMinTrackSize.y = 580;
                        }
                        return LRESULT(0);
                    }
                }
                WM_NCCALCSIZE => {
                    // When maximized, clamp client area strictly to monitor rcWork (stops above taskbar,
                    // eliminates the WS_THICKFRAME ~9px border overshoot on all 4 sides).
                    if wparam.0 != 0 {
                        let params = lparam.0 as *mut NCCALCSIZE_PARAMS;
                        if !params.is_null() {
                            let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                            let mut mi = MONITORINFO {
                                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                                ..Default::default()
                            };
                            if GetMonitorInfoW(hmon, &mut mi).as_bool() {
                                let r = (*params).rgrc[0];
                                let overshoots_work = r.left < mi.rcWork.left
                                    && r.right > mi.rcWork.right
                                    && r.top < mi.rcWork.top;
                                if IsZoomed(hwnd.0) != 0 || overshoots_work {
                                    (*params).rgrc[0] = mi.rcWork;
                                    return LRESULT(0);
                                }
                            }
                        }
                    }
                }
                WM_SIZE => {
                    apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);
                    if wparam.0 == 1 {
                        // SIZE_MINIMIZED -> release GPU/UI artwork buffers & trim working set
                        if !WINDOW_HIDDEN_OR_MIN.swap(true, Ordering::AcqRel) {
                            trim_working_set();
                            push_event(TrayEvent::HiddenToTray);
                        }
                    } else if WINDOW_HIDDEN_OR_MIN.swap(false, Ordering::AcqRel) {
                        push_event(TrayEvent::ShowWindow);
                    } else {
                        push_event(TrayEvent::WindowMovedOrResized);
                    }
                }
                WM_MOVE | WM_WINDOWPOSCHANGED | WM_SHOWWINDOW => {
                    push_event(TrayEvent::WindowMovedOrResized);
                }
                WM_CLOSE => {
                    if CLOSE_TO_TRAY.load(Ordering::Acquire) && !FORCE_QUIT.load(Ordering::Acquire)
                    {
                        ensure_tray_icon(hwnd, "Ferric — Playing in background");
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        WINDOW_HIDDEN_OR_MIN.store(true, Ordering::Release);
                        trim_working_set();
                        push_event(TrayEvent::HiddenToTray);
                        return LRESULT(0);
                    }
                    remove_tray_icon(hwnd);
                }
                WM_TRAYICON => {
                    let ev = (lparam.0 & 0xFFFF) as u32;
                    if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                        let _ = ShowWindow(hwnd, SW_SHOW);
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                        let _ = SetForegroundWindow(hwnd);
                        WINDOW_HIDDEN_OR_MIN.store(false, Ordering::Release);
                        apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);
                        push_event(TrayEvent::ShowWindow);
                        return LRESULT(0);
                    }
                    if ev == WM_RBUTTONUP {
                        show_tray_context_menu(hwnd);
                        return LRESULT(0);
                    }
                }
                WM_DESTROY => {
                    let mh = SINGLE_INSTANCE_MUTEX.swap(0, Ordering::AcqRel);
                    if mh != 0 {
                        let _ = CloseHandle(mh as *mut c_void);
                    }
                    remove_tray_icon(hwnd);
                    let _ = RemoveWindowSubclass(hwnd, ferric_subclass_proc, uid);
                }
                _ => {}
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
    }

    pub fn minimize() {
        const SC_MINIMIZE: usize = 0xF020;
        if let Some(hwnd) = active_process_hwnd() {
            // SAFETY: Valid HWND owned by current process.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(SC_MINIMIZE), LPARAM(0));
            }
        }
    }

    pub fn toggle_maximize() -> bool {
        const SC_MAXIMIZE: usize = 0xF030;
        const SC_RESTORE: usize = 0xF120;
        if let Some(hwnd) = active_process_hwnd() {
            // SAFETY: Valid HWND owned by current process.
            unsafe {
                let currently_max = IsZoomed(hwnd.0) != 0;
                let cmd = if currently_max {
                    SC_RESTORE
                } else {
                    SC_MAXIMIZE
                };
                let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(cmd), LPARAM(0));
                return !currently_max;
            }
        }
        false
    }

    pub fn is_maximized() -> bool {
        active_process_hwnd().is_some_and(|hwnd| {
            // SAFETY: Valid HWND owned by current process.
            unsafe { IsZoomed(hwnd.0) != 0 }
        })
    }

    pub fn is_window_hidden_or_minimized() -> bool {
        active_process_hwnd().is_some_and(|hwnd| {
            // SAFETY: Valid HWND owned by current process.
            unsafe { IsWindowVisible(hwnd.0) == 0 || IsIconic(hwnd.0) != 0 }
        })
    }

    pub fn close_or_hide_to_tray() {
        const SC_CLOSE: usize = 0xF060;
        if let Some(hwnd) = active_process_hwnd() {
            if CLOSE_TO_TRAY.load(Ordering::Acquire) && !FORCE_QUIT.load(Ordering::Acquire) {
                // SAFETY: Valid HWND owned by current process; subclass handles WM_CLOSE -> Hide to Tray.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            } else {
                // SAFETY: Valid HWND owned by current process.
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(SC_CLOSE), LPARAM(0));
                }
            }
        }
    }

    pub fn force_quit() {
        FORCE_QUIT.store(true, Ordering::Release);
        if let Some(hwnd) = active_process_hwnd() {
            remove_tray_icon(hwnd);
            // SAFETY: Force real shutdown bypassing Close-to-Tray.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
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

        // SAFETY: Standard Win32 CF_UNICODETEXT clipboard transfer with checked pointers.
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
}

pub fn win32_acquire_single_instance_or_activate() -> bool {
    #[cfg(target_os = "windows")]
    {
        win_impl::acquire_single_instance_or_activate()
    }
    #[cfg(not(target_os = "windows"))]
    {
        true
    }
}

pub fn win32_install_window_hooks() {
    #[cfg(target_os = "windows")]
    win_impl::install_window_hooks();
}

pub fn win32_set_native_acrylic(enabled: bool, opacity: f32) {
    #[cfg(target_os = "windows")]
    win_impl::set_native_acrylic(enabled, opacity);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enabled, opacity);
    }
}

pub fn win32_set_close_to_tray(enabled: bool) {
    #[cfg(target_os = "windows")]
    win_impl::set_close_to_tray(enabled);
    #[cfg(not(target_os = "windows"))]
    let _ = enabled;
}

pub fn win32_update_tray_tooltip(tip: &str) {
    #[cfg(target_os = "windows")]
    win_impl::update_tray_tooltip(tip);
    #[cfg(not(target_os = "windows"))]
    let _ = tip;
}

#[allow(dead_code)]
pub fn win32_set_mini_mode_flag(mini: bool) {
    #[cfg(target_os = "windows")]
    win_impl::set_mini_mode_flag(mini);
    #[cfg(not(target_os = "windows"))]
    let _ = mini;
}

pub fn win32_poll_tray_events() -> Vec<TrayEvent> {
    #[cfg(target_os = "windows")]
    {
        win_impl::poll_tray_events()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

pub fn win32_trim_working_set() {
    #[cfg(target_os = "windows")]
    win_impl::trim_working_set();
}

pub fn win32_main_hwnd_raw() -> isize {
    #[cfg(target_os = "windows")]
    {
        win_impl::main_hwnd_raw()
    }
    #[cfg(not(target_os = "windows"))]
    {
        0
    }
}

pub fn win32_client_rect_screen() -> Option<WindowClientScreenRect> {
    #[cfg(target_os = "windows")]
    {
        win_impl::client_rect_screen()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

pub fn win32_minimize_active_window() {
    #[cfg(target_os = "windows")]
    win_impl::minimize();
}

pub fn win32_toggle_maximize_active_window() -> bool {
    #[cfg(target_os = "windows")]
    {
        win_impl::toggle_maximize()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn win32_is_active_window_maximized() -> bool {
    #[cfg(target_os = "windows")]
    {
        win_impl::is_maximized()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn win32_is_window_hidden_or_minimized() -> bool {
    #[cfg(target_os = "windows")]
    {
        win_impl::is_window_hidden_or_minimized()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn win32_close_active_window() {
    #[cfg(target_os = "windows")]
    win_impl::close_or_hide_to_tray();
}

pub fn win32_force_quit() {
    #[cfg(target_os = "windows")]
    win_impl::force_quit();
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win_impl::copy_to_clipboard(text)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
        Ok(())
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

    const SW_SHOWNORMAL: i32 = 1;
    let op: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    let dir: Option<Vec<u16>> = working_dir.map(|d| d.encode_utf16().chain(Some(0)).collect());

    // SAFETY: Null-terminated UTF-16 buffers passed to ShellExecuteW.
    let res = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            ptr::null(),
            dir.as_ref().map_or(ptr::null(), Vec::as_ptr),
            SW_SHOWNORMAL,
        )
    };
    if res <= 32 {
        Err(format!("Windows shell launch failed (code {res})"))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn shell_open(_target: &str, _working_dir: Option<&str>) -> Result<(), String> {
    Ok(())
}
