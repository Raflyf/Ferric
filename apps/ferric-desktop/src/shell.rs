use std::path::Path;

/// Allowed external HTTPS origins that Ferric may open in the system browser (OAuth & GitHub Releases only).
const ALLOWED_HTTPS_PREFIXES: &[&str] = &[
    "https://github.com/Raflyf/Ferric/",
    "https://accounts.spotify.com/",
    "https://developer.spotify.com/",
    "https://accounts.google.com/",
    "https://console.cloud.google.com/",
    "https://discord.com/developers/applications/",
];

const ALLOWED_HTTPS_EXACT: &[&str] = &[
    "https://github.com/Raflyf/Ferric",
    "https://discord.com/developers/applications",
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

fn is_safe_external_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > 2048
        || url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\\' | '<' | '>'))
        || url.contains("..")
    {
        return false;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    ALLOWED_HTTPS_EXACT.contains(&url) || ALLOWED_HTTPS_PREFIXES.iter().any(|p| url.starts_with(p))
}

/// Open an allowlisted HTTPS URL (GitHub Releases or official Spotify/Google OAuth pages) fail-closed.
pub fn open_allowed_url(url: &str) -> Result<(), String> {
    if !is_safe_external_url(url) {
        return Err("Refused to open untrusted external URL".to_string());
    }
    shell_open(url, None)
}

/// Backwards-compatible helper for GitHub release links.
pub fn open_github_url(url: &str) -> Result<(), String> {
    let is_ferric_repo = url == "https://github.com/Raflyf/Ferric"
        || url.starts_with("https://github.com/Raflyf/Ferric/");
    if !is_ferric_repo || !is_safe_external_url(url) {
        return Err("Refused to open non-Ferric GitHub URL".to_string());
    }
    open_allowed_url(url)
}

/// Launch a SHA-256-verified `.msi` installer file directly via `%SystemRoot%\System32\msiexec.exe`
/// after re-verifying containment, regular-file type, and cryptographic SHA-256 digest right before launch.
pub fn launch_verified_installer(
    msi_path: &Path,
    expected_dir: &Path,
    expected_sha256: &[u8; 32],
) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(msi_path)
        .map_err(|_| "Verified installer file does not exist".to_string())?;
    if !meta.file_type().is_file() {
        return Err("Refused to launch non-regular or symlinked installer file".to_string());
    }
    let ext_ok = msi_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"));
    if !ext_ok {
        return Err("Refused to launch non-MSI installer file".to_string());
    }
    let canonical_dir = std::fs::canonicalize(expected_dir)
        .map_err(|_| "Updates directory could not be verified".to_string())?;
    let canonical_msi = std::fs::canonicalize(msi_path)
        .map_err(|_| "Verified installer path could not be resolved".to_string())?;
    if !canonical_msi.starts_with(&canonical_dir) {
        return Err("Refused to launch installer outside verified updates directory".to_string());
    }

    // Re-hash immediately before execution to eliminate TOCTOU tampering on disk.
    let actual_sha = ferric_providers::updater::sha256_file(&canonical_msi)?;
    if actual_sha != *expected_sha256 {
        return Err("Installer integrity check failed before launch".to_string());
    }

    let raw_target = canonical_msi.to_string_lossy();
    if raw_target.starts_with(r"\\?\UNC\") || raw_target.starts_with(r"\\.\") {
        return Err("Refused to launch installer from network or device path".to_string());
    }
    let clean_target = raw_target.strip_prefix(r"\\?\").unwrap_or(&raw_target);
    if clean_target.is_empty()
        || clean_target
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\0'))
    {
        return Err("Installer path contains unsafe characters".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        win_impl::launch_msiexec(clean_target)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = clean_target;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod win_impl {
    use super::{TrayEvent, WindowClientScreenRect};
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, ClientToScreen, CreateBitmap, CreateDIBSection,
        DIB_RGB_COLORS, DeleteObject, GetMonitorInfoW, HGDIOBJ, MONITOR_DEFAULTTONEAREST,
        MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows::Win32::System::Threading::GetCurrentProcess;
    use windows::Win32::UI::Shell::{
        ITaskbarList3, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW, Shell_NotifyIconW, THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_ENABLED,
        THBN_CLICKED, THUMBBUTTON, TaskbarList,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetClientRect,
        GetCursorPos, HICON, ICONINFO, IDI_APPLICATION, LoadIconW, MF_SEPARATOR, MF_STRING,
        MINMAXINFO, NCCALCSIZE_PARAMS, PostMessageW, SW_HIDE, SW_RESTORE, SW_SHOW,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, TPM_BOTTOMALIGN,
        TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE,
        WM_COMMAND, WM_DESTROY, WM_GETMINMAXINFO, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_MOVE,
        WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_RBUTTONUP, WM_SHOWWINDOW,
        WM_SIZE, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED, WS_CAPTION, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };
    use windows::core::{PCWSTR, s, w};

    const SUBCLASS_ID: usize = 0xFE11_C001;
    const TRAY_ICON_ID: u32 = 1;
    const WM_TRAYICON: u32 = WM_APP + 42;
    const ERROR_ALREADY_EXISTS: u32 = 183;
    const HTCLIENT: isize = 1;
    const HTSYSMENU: isize = 3;
    const HTMINBUTTON: isize = 8;
    const HTMAXBUTTON: isize = 9;
    const HTCLOSE: isize = 20;

    const CMD_SHOW: u32 = 1001;
    const CMD_PLAY_PAUSE: u32 = 1002;
    const CMD_NEXT: u32 = 1003;
    const CMD_PREV: u32 = 1004;
    const CMD_QUIT: u32 = 1005;

    const TB_BTN_PREV: u32 = 2001;
    const TB_BTN_PLAY_PAUSE: u32 = 2002;
    const TB_BTN_NEXT: u32 = 2003;

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
    static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);
    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

    #[derive(Clone, Copy)]
    enum ThumbIconKind {
        Prev,
        Play,
        Pause,
        Next,
    }

    struct TaskbarThumbState {
        tb: ITaskbarList3,
        ico_prev: HICON,
        ico_play: HICON,
        ico_pause: HICON,
        ico_next: HICON,
        buttons_added: bool,
        add_retries: u8,
        last_playing: Option<bool>,
        last_title: String,
    }

    thread_local! {
        static TASKBAR_STATE: RefCell<Option<TaskbarThumbState>> = const { RefCell::new(None) };
    }
    static ACRYLIC_ENABLED: AtomicBool = AtomicBool::new(false);
    static ACRYLIC_TINT_RGB: AtomicU32 = AtomicU32::new(0x000B_0D14);
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

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        lp_reserved: *mut u16,
        lp_desktop: *mut u16,
        lp_title: *mut u16,
        dw_x: u32,
        dw_y: u32,
        dw_x_size: u32,
        dw_y_size: u32,
        dw_x_count_chars: u32,
        dw_y_count_chars: u32,
        dw_fill_attribute: u32,
        dw_flags: u32,
        w_show_window: u16,
        cb_reserved2: u16,
        lp_reserved2: *mut u8,
        h_std_input: *mut c_void,
        h_std_output: *mut c_void,
        h_std_error: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        h_process: *mut c_void,
        h_thread: *mut c_void,
        dw_process_id: u32,
        dw_thread_id: u32,
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
        fn SetDllDirectoryW(lp_path_name: *const u16) -> i32;
        fn SetDefaultDllDirectories(directory_flags: u32) -> i32;
        fn HeapSetInformation(
            heap_handle: *mut c_void,
            heap_information_class: u32,
            heap_information: *const c_void,
            heap_information_length: usize,
        ) -> i32;
        fn SetProcessMitigationPolicy(
            mitigation_policy: i32,
            lp_buffer: *const c_void,
            dw_length: usize,
        ) -> i32;
        fn GetSystemDirectoryW(lp_buffer: *mut u16, u_size: u32) -> u32;
        fn CreateProcessW(
            lp_application_name: *const u16,
            lp_command_line: *mut u16,
            lp_process_attributes: *mut c_void,
            lp_thread_attributes: *mut c_void,
            b_inherit_handles: i32,
            dw_creation_flags: u32,
            lp_environment: *mut c_void,
            lp_current_directory: *const u16,
            lp_startup_info: *const StartupInfoW,
            lp_process_information: *mut ProcessInformation,
        ) -> i32;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }

    /// Enable OS-level Win32 anti-injection and DLL-hijacking mitigations immediately at process startup.
    pub fn harden_process() {
        const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x0000_0800;
        const HEAP_ENABLE_TERMINATION_ON_CORRUPTION: u32 = 1;
        const PROCESS_EXTENSION_POINT_DISABLE_POLICY: i32 = 6;
        const PROCESS_IMAGE_LOAD_POLICY: i32 = 10;
        // NoRemoteImages (0x1) | NoLowMandatoryLabelImages (0x2) | PreferSystem32Images (0x4)
        let image_load_flags: u32 = 0x0000_0007;
        // DisableExtensionPoints (0x1): blocks legacy AppInit_DLLs & global WinEventHook injection
        let ext_point_flags: u32 = 0x0000_0001;
        let empty_dir: [u16; 1] = [0];
        // SAFETY: Standard Win32 process-hardening APIs with valid pointer/size arguments.
        // Older OS builds ignore unsupported policies cleanly via non-zero/zero return codes.
        unsafe {
            let _ = SetDllDirectoryW(empty_dir.as_ptr());
            let _ = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
            let _ = HeapSetInformation(
                ptr::null_mut(),
                HEAP_ENABLE_TERMINATION_ON_CORRUPTION,
                ptr::null(),
                0,
            );
            let _ = SetProcessMitigationPolicy(
                PROCESS_IMAGE_LOAD_POLICY,
                (&raw const image_load_flags).cast(),
                std::mem::size_of::<u32>(),
            );
            let _ = SetProcessMitigationPolicy(
                PROCESS_EXTENSION_POINT_DISABLE_POLICY,
                (&raw const ext_point_flags).cast(),
                std::mem::size_of::<u32>(),
            );
        }
    }

    /// Set a deterministic AppUserModelID so Windows Taskbar, SMTC, and Task Manager bind consistently.
    pub fn set_app_user_model_id() {
        let app_id: Vec<u16> = "Raflyf.Ferric\0".encode_utf16().collect();
        // SAFETY: Null-terminated UTF-16 AppUserModelID passed once at process startup.
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
        }
    }

    pub fn launch_msiexec(msi_path: &str) -> Result<(), String> {
        let mut sys_buf = [0u16; 512];
        // SAFETY: Queries `%SystemRoot%\System32` into a bounded UTF-16 stack buffer.
        let sys_len = unsafe { GetSystemDirectoryW(sys_buf.as_mut_ptr(), sys_buf.len() as u32) };
        if sys_len == 0 || (sys_len as usize) >= sys_buf.len() {
            return Err("Could not locate Windows System32 directory".to_string());
        }
        let sys_dir = String::from_utf16_lossy(&sys_buf[..sys_len as usize]);
        let msiexec = format!("{sys_dir}\\msiexec.exe");
        let app_w: Vec<u16> = msiexec.encode_utf16().chain(Some(0)).collect();
        let dir_w: Vec<u16> = sys_dir.encode_utf16().chain(Some(0)).collect();
        let mut cmd_w: Vec<u16> = format!("\"{msiexec}\" /i \"{msi_path}\"")
            .encode_utf16()
            .chain(Some(0))
            .collect();

        // SAFETY: Zero-initialized STARTUPINFOW with exact struct size and PROCESS_INFORMATION.
        let mut si: StartupInfoW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<StartupInfoW>() as u32;
        let mut pi: ProcessInformation = unsafe { std::mem::zeroed() };

        // SAFETY: Direct CreateProcessW invocation of System32 msiexec.exe with mutable UTF-16 cmdline.
        let ok = unsafe {
            CreateProcessW(
                app_w.as_ptr(),
                cmd_w.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
                0,
                ptr::null_mut(),
                dir_w.as_ptr(),
                &raw const si,
                &raw mut pi,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            return Err(format!("Failed to launch Windows Installer (error {code})"));
        }
        unsafe {
            if !pi.h_thread.is_null() {
                let _ = CloseHandle(pi.h_thread);
            }
            if !pi.h_process.is_null() {
                let _ = CloseHandle(pi.h_process);
            }
        }
        Ok(())
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
                let cls: Vec<u16> = "FerricMainWindow\0".encode_utf16().collect();
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
                let rgb = ACRYLIC_TINT_RGB.load(Ordering::Acquire);
                let r = (rgb >> 16) & 0xFF;
                let g = (rgb >> 8) & 0xFF;
                let b = rgb & 0xFF;
                let abgr = 0x0100_0000 | (b << 16) | (g << 8) | r;
                let mut policy = AccentPolicy {
                    accent_state: if enabled { 4 } else { 0 },
                    accent_flags: if enabled { 2 } else { 0 },
                    gradient_color: if enabled { abgr } else { 0 },
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

    pub fn set_native_acrylic(enabled: bool, _opacity: f32, tint_rgb: (u8, u8, u8)) {
        ACRYLIC_ENABLED.store(enabled, Ordering::Release);
        let packed = ((tint_rgb.0 as u32) << 16) | ((tint_rgb.1 as u32) << 8) | (tint_rgb.2 as u32);
        ACRYLIC_TINT_RGB.store(packed, Ordering::Release);
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
        String::from_utf16_lossy(&buf[..len as usize]) == "FerricMainWindow"
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
            // Use overlapped top-level style (WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX |
            // WS_MAXIMIZEBOX | WS_SYSMENU, clearing WS_POPUP) so Windows 11 DWM delivers full
            // native minimize / maximize / restore animations, rounded corners, and Aero Snap.
            // Zero OS caption buttons appear because WM_NCCALCSIZE removes the entire NC frame,
            // WM_NCACTIVATE/WM_NCPAINT suppress classic non-client drawing, WM_NCHITTEST keeps
            // caption hitboxes as HTCLIENT, and WS_EX_NOREDIRECTIONBITMAP removes GDI surface.
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
                }
                let margins = DwmMargins {
                    cx_left_width: -1,
                    cx_right_width: -1,
                    cy_top_height: -1,
                    cy_bottom_height: -1,
                };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
                // Always force a fresh WM_NCCALCSIZE pass after subclassing so DWM clears any
                // initial non-client caption frame even when style flags already matched.
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
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
            apply_window_icon(hwnd);
            init_taskbar_thumbbar(hwnd);
            let acr_on = ACRYLIC_ENABLED.load(Ordering::Acquire);
            let packed = ACRYLIC_TINT_RGB.load(Ordering::Acquire);
            let tint = (
                ((packed >> 16) & 0xFF) as u8,
                ((packed >> 8) & 0xFF) as u8,
                (packed & 0xFF) as u8,
            );
            set_native_acrylic(acr_on, 0.72, tint);
        }
    }

    fn taskbar_created_message_id() -> u32 {
        let cached = TASKBAR_CREATED_MSG.load(Ordering::Acquire);
        if cached != 0 {
            return cached;
        }
        let name: Vec<u16> = "TaskbarButtonCreated\0".encode_utf16().collect();
        // SAFETY: Null-terminated UTF-16 message name for Windows Explorer taskbar button creation.
        let id = unsafe { RegisterWindowMessageW(name.as_ptr()) };
        if id != 0 {
            TASKBAR_CREATED_MSG.store(id, Ordering::Release);
        }
        id
    }

    fn fill_tip_buf(tip: &str) -> [u16; 260] {
        let mut buf = [0u16; 260];
        for (i, ch) in tip.encode_utf16().take(259).enumerate() {
            buf[i] = ch;
        }
        buf
    }

    fn hit_media_glyph(kind: ThumbIconKind, x: f32, y: f32) -> bool {
        match kind {
            ThumbIconKind::Prev => {
                let bar = (6.5..=9.5).contains(&x) && (7.0..=25.0).contains(&y);
                let tri = (11.5..=24.5).contains(&x) && (y - 16.0).abs() <= 8.5 * (x - 11.5) / 13.0;
                bar || tri
            }
            ThumbIconKind::Play => {
                (10.0..=24.5).contains(&x) && (y - 16.0).abs() <= 9.0 * (24.5 - x) / 14.5
            }
            ThumbIconKind::Pause => {
                let l = (8.5..=13.5).contains(&x) && (7.0..=25.0).contains(&y);
                let r = (18.5..=23.5).contains(&x) && (7.0..=25.0).contains(&y);
                l || r
            }
            ThumbIconKind::Next => {
                let tri = (7.5..=20.5).contains(&x) && (y - 16.0).abs() <= 8.5 * (20.5 - x) / 13.0;
                let bar = (22.5..=25.5).contains(&x) && (7.0..=25.0).contains(&y);
                tri || bar
            }
        }
    }

    fn create_media_hicon(kind: ThumbIconKind) -> HICON {
        const SZ: i32 = 32;
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SZ,
                biHeight: -SZ, // Top-down 32-bit BGRA
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: Allocates a 32x32 top-down DIB section and zeroed 1bpp mask bitmap, fills 4x4
        // supersampled white media glyph pixels, builds an HICON, and releases temp bitmaps.
        unsafe {
            let mut bits_ptr: *mut c_void = ptr::null_mut();
            let Ok(hbm_color) =
                CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
            else {
                return HICON::default();
            };
            if bits_ptr.is_null() {
                let _ = DeleteObject(HGDIOBJ(hbm_color.0));
                return HICON::default();
            }
            let pixels = std::slice::from_raw_parts_mut(bits_ptr as *mut u32, (SZ * SZ) as usize);
            for py in 0..SZ {
                for px in 0..SZ {
                    let mut hits = 0u32;
                    for sy in 0..4 {
                        for sx in 0..4 {
                            let fx = px as f32 + (sx as f32 + 0.5) * 0.25;
                            let fy = py as f32 + (sy as f32 + 0.5) * 0.25;
                            if hit_media_glyph(kind, fx, fy) {
                                hits += 1;
                            }
                        }
                    }
                    let a = ((hits * 255) / 16).min(255);
                    // Straight white BGRA for CreateIconIndirect (Windows premultiplies on draw)
                    pixels[(py * SZ + px) as usize] =
                        if a == 0 { 0 } else { (a << 24) | 0x00FF_FFFF };
                }
            }
            let mask_bits = [0u8; (SZ * SZ / 8) as usize];
            let hbm_mask = CreateBitmap(SZ, SZ, 1, 1, Some(mask_bits.as_ptr().cast()));
            let ii = ICONINFO {
                fIcon: true.into(),
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: hbm_mask,
                hbmColor: hbm_color,
            };
            let hicon = CreateIconIndirect(&ii).unwrap_or_default();
            let _ = DeleteObject(HGDIOBJ(hbm_color.0));
            if !hbm_mask.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(hbm_mask.0));
            }
            hicon
        }
    }

    fn build_thumb_buttons(
        ico_prev: HICON,
        ico_play: HICON,
        ico_pause: HICON,
        ico_next: HICON,
        is_playing: bool,
    ) -> [THUMBBUTTON; 3] {
        let mask = THB_ICON | THB_TOOLTIP | THB_FLAGS;
        [
            THUMBBUTTON {
                dwMask: mask,
                iId: TB_BTN_PREV,
                iBitmap: 0,
                hIcon: ico_prev,
                szTip: fill_tip_buf("Previous"),
                dwFlags: THBF_ENABLED,
            },
            THUMBBUTTON {
                dwMask: mask,
                iId: TB_BTN_PLAY_PAUSE,
                iBitmap: 0,
                hIcon: if is_playing { ico_pause } else { ico_play },
                szTip: fill_tip_buf(if is_playing { "Pause" } else { "Play" }),
                dwFlags: THBF_ENABLED,
            },
            THUMBBUTTON {
                dwMask: mask,
                iId: TB_BTN_NEXT,
                iBitmap: 0,
                hIcon: ico_next,
                szTip: fill_tip_buf("Next"),
                dwFlags: THBF_ENABLED,
            },
        ]
    }

    fn init_taskbar_thumbbar(hwnd: HWND) {
        let _ = taskbar_created_message_id();
        // SAFETY: STA COM creation of Shell ITaskbarList3 on Ferric's UI thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(tb) =
                CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
            else {
                return;
            };
            if tb.HrInit().is_err() {
                return;
            }
            let (old_state, prev_playing, prev_title) = TASKBAR_STATE.with(|cell| {
                let Ok(mut guard) = cell.try_borrow_mut() else {
                    return (None, false, String::new());
                };
                if let Some(old) = guard.take() {
                    let p = old.last_playing.unwrap_or(false);
                    let t = old.last_title.clone();
                    (Some(old), p, t)
                } else {
                    (None, false, String::new())
                }
            });
            if let Some(old) = old_state {
                for ic in [old.ico_prev, old.ico_play, old.ico_pause, old.ico_next] {
                    if !ic.is_invalid() {
                        let _ = DestroyIcon(ic);
                    }
                }
            }
            let ico_prev = create_media_hicon(ThumbIconKind::Prev);
            let ico_play = create_media_hicon(ThumbIconKind::Play);
            let ico_pause = create_media_hicon(ThumbIconKind::Pause);
            let ico_next = create_media_hicon(ThumbIconKind::Next);
            let buttons =
                build_thumb_buttons(ico_prev, ico_play, ico_pause, ico_next, prev_playing);
            // Execute STA COM outside RefCell borrows to prevent re-entrancy panics.
            let added = tb.ThumbBarAddButtons(hwnd, &buttons).is_ok();
            let stored = TASKBAR_STATE.with(|cell| {
                if let Ok(mut guard) = cell.try_borrow_mut() {
                    *guard = Some(TaskbarThumbState {
                        tb,
                        ico_prev,
                        ico_play,
                        ico_pause,
                        ico_next,
                        buttons_added: added,
                        add_retries: 0,
                        last_playing: if added { Some(prev_playing) } else { None },
                        last_title: prev_title,
                    });
                    true
                } else {
                    false
                }
            });
            if !stored {
                for ic in [ico_prev, ico_play, ico_pause, ico_next] {
                    if !ic.is_invalid() {
                        let _ = DestroyIcon(ic);
                    }
                }
            }
        }
    }

    pub fn update_taskbar_playback(title: &str, artist: &str, is_playing: bool) {
        let Some(hwnd) = active_process_hwnd() else {
            return;
        };
        let win_title = if title.trim().is_empty() {
            "Ferric".to_string()
        } else if artist.trim().is_empty() {
            format!("{} - Ferric", title.trim())
        } else {
            format!("{} - {} - Ferric", title.trim(), artist.trim())
        };

        enum PendingAction {
            None,
            Add(ITaskbarList3, Box<[THUMBBUTTON; 3]>),
            Update(ITaskbarList3, Box<THUMBBUTTON>),
        }

        let (need_set_title, action) = TASKBAR_STATE.with(|cell| {
            let Ok(mut guard) = cell.try_borrow_mut() else {
                return (false, PendingAction::None);
            };
            let Some(st) = guard.as_mut() else {
                return (true, PendingAction::None);
            };
            let set_title = if st.last_title != win_title {
                st.last_title = win_title.clone();
                true
            } else {
                false
            };
            let act = if !st.buttons_added && st.add_retries < 8 {
                st.add_retries += 1;
                let buttons = build_thumb_buttons(
                    st.ico_prev,
                    st.ico_play,
                    st.ico_pause,
                    st.ico_next,
                    is_playing,
                );
                PendingAction::Add(st.tb.clone(), Box::new(buttons))
            } else if st.buttons_added && st.last_playing != Some(is_playing) {
                st.last_playing = Some(is_playing);
                let btn = THUMBBUTTON {
                    dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
                    iId: TB_BTN_PLAY_PAUSE,
                    iBitmap: 0,
                    hIcon: if is_playing {
                        st.ico_pause
                    } else {
                        st.ico_play
                    },
                    szTip: fill_tip_buf(if is_playing { "Pause" } else { "Play" }),
                    dwFlags: THBF_ENABLED,
                };
                PendingAction::Update(st.tb.clone(), Box::new(btn))
            } else {
                PendingAction::None
            };
            (set_title, act)
        });

        // Execute Win32 & STA COM calls outside RefCell borrows so re-entrant messages never panic.
        if need_set_title {
            let wide: Vec<u16> = win_title.encode_utf16().chain(Some(0)).collect();
            // SAFETY: Valid HWND and null-terminated UTF-16 title string.
            unsafe {
                let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            }
        }
        match action {
            PendingAction::None => {}
            PendingAction::Add(tb, buttons) => {
                // SAFETY: UI-thread COM retry when Explorer taskbar button finishes registering.
                if unsafe { tb.ThumbBarAddButtons(hwnd, buttons.as_slice()) }.is_ok() {
                    TASKBAR_STATE.with(|cell| {
                        if let Ok(mut guard) = cell.try_borrow_mut()
                            && let Some(st) = guard.as_mut()
                        {
                            st.buttons_added = true;
                            st.last_playing = Some(is_playing);
                        }
                    });
                }
            }
            PendingAction::Update(tb, btn) => {
                // SAFETY: UI-thread COM call updating center Play/Pause thumbnail toolbar button.
                unsafe {
                    let _ = tb.ThumbBarUpdateButtons(hwnd, std::slice::from_ref(btn.as_ref()));
                }
            }
        }
    }

    fn destroy_taskbar_thumbbar() {
        let taken =
            TASKBAR_STATE.with(|cell| cell.try_borrow_mut().ok().and_then(|mut g| g.take()));
        if let Some(st) = taken {
            // SAFETY: Releases HICONs created via CreateIconIndirect on shutdown.
            unsafe {
                for ic in [st.ico_prev, st.ico_play, st.ico_pause, st.ico_next] {
                    if !ic.is_invalid() {
                        let _ = DestroyIcon(ic);
                    }
                }
            }
        }
    }

    fn apply_window_icon(hwnd: HWND) {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetSystemMetrics(n_index: i32) -> i32;
            fn LoadImageW(
                h_inst: *mut c_void,
                name: *const u16,
                img_type: u32,
                cx: i32,
                cy: i32,
                fu_load: u32,
            ) -> *mut c_void;
            fn SendMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
            fn SetClassLongPtrW(hwnd: *mut c_void, n_index: i32, dw_new_long: isize) -> isize;
        }
        const IMAGE_ICON: u32 = 1;
        const LR_DEFAULTCOLOR: u32 = 0x0000;
        const LR_SHARED: u32 = 0x8000;
        const SM_CXICON: i32 = 11;
        const SM_CYICON: i32 = 12;
        const SM_CXSMICON: i32 = 49;
        const SM_CYSMICON: i32 = 50;
        const WM_SETICON: u32 = 0x0080;
        const ICON_SMALL: usize = 0;
        const ICON_BIG: usize = 1;
        const GCLP_HICON: i32 = -14;
        const GCLP_HICONSM: i32 = -34;

        // SAFETY: Loads embedded resource icon #1 (`ferric.ico` via `winresource`) at exact OS
        // small/big icon dimensions and binds it to both HWND and MakepadWindow class so Windows 11
        // Taskbar Hover Thumbnail Preview & Alt+Tab display Ferric's cassette icon instead of a blank icon.
        unsafe {
            let Ok(hmod) = GetModuleHandleW(None) else {
                return;
            };
            let res_id = std::ptr::without_provenance::<u16>(1);
            let dpi = GetDpiForWindow(hwnd.0).max(96) as i32;
            let big_w = ((32 * dpi + 48) / 96)
                .max(GetSystemMetrics(SM_CXICON))
                .max(32);
            let big_h = ((32 * dpi + 48) / 96)
                .max(GetSystemMetrics(SM_CYICON))
                .max(32);
            let sm_w = ((16 * dpi + 48) / 96)
                .max(GetSystemMetrics(SM_CXSMICON))
                .max(16);
            let sm_h = ((16 * dpi + 48) / 96)
                .max(GetSystemMetrics(SM_CYSMICON))
                .max(16);
            let h_big = LoadImageW(
                hmod.0,
                res_id,
                IMAGE_ICON,
                big_w,
                big_h,
                LR_DEFAULTCOLOR | LR_SHARED,
            );
            let h_small = LoadImageW(
                hmod.0,
                res_id,
                IMAGE_ICON,
                sm_w,
                sm_h,
                LR_DEFAULTCOLOR | LR_SHARED,
            );
            if !h_big.is_null() {
                let _ = SendMessageW(hwnd.0, WM_SETICON, ICON_BIG, h_big as isize);
                let _ = SetClassLongPtrW(hwnd.0, GCLP_HICON, h_big as isize);
            }
            if !h_small.is_null() {
                let _ = SendMessageW(hwnd.0, WM_SETICON, ICON_SMALL, h_small as isize);
                let _ = SetClassLongPtrW(hwnd.0, GCLP_HICONSM, h_small as isize);
            }
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
            let tb_msg = TASKBAR_CREATED_MSG.load(Ordering::Acquire);
            if tb_msg != 0 && msg == tb_msg {
                init_taskbar_thumbbar(hwnd);
                return LRESULT(0);
            }
            match msg {
                WM_COMMAND => {
                    let hi = ((wparam.0 >> 16) & 0xFFFF) as u32;
                    let lo = (wparam.0 & 0xFFFF) as u32;
                    if hi == THBN_CLICKED {
                        match lo {
                            TB_BTN_PREV => {
                                push_event(TrayEvent::PrevTrack);
                                return LRESULT(0);
                            }
                            TB_BTN_PLAY_PAUSE => {
                                push_event(TrayEvent::TogglePlayPause);
                                return LRESULT(0);
                            }
                            TB_BTN_NEXT => {
                                push_event(TrayEvent::NextTrack);
                                return LRESULT(0);
                            }
                            _ => {}
                        }
                    }
                }
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
                WM_NCACTIVATE => {
                    // Never allow DefWindowProcW to paint classic titlebar/caption frames on focus/blur.
                    if IsIconic(hwnd.0) == 0 {
                        return LRESULT(1);
                    }
                }
                WM_NCPAINT => {
                    return LRESULT(0);
                }
                WM_NCCALCSIZE => {
                    // Keep the entire window 100% client area (zero OS non-client titlebar/caption
                    // frame) on both normal and maximized states; when maximized, clamp strictly to
                    // monitor rcWork so the window stops cleanly above the Windows Taskbar.
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
                                }
                            }
                        }
                        let margins = DwmMargins {
                            cx_left_width: -1,
                            cx_right_width: -1,
                            cy_top_height: -1,
                            cy_bottom_height: -1,
                        };
                        let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
                        return LRESULT(0);
                    }
                }
                WM_NCHITTEST => {
                    let hit = DefSubclassProc(hwnd, msg, wparam, lparam);
                    if matches!(hit.0, HTSYSMENU | HTMINBUTTON | HTMAXBUTTON | HTCLOSE) {
                        return LRESULT(HTCLIENT);
                    }
                    return hit;
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
                    destroy_taskbar_thumbbar();
                    MAIN_HWND.store(0, Ordering::Release);
                    HOOK_INSTALLED.store(false, Ordering::Release);
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
            fn GlobalFree(h_mem: *mut c_void) -> *mut c_void;
        }

        const CF_UNICODETEXT: u32 = 13;
        const GMEM_MOVEABLE: u32 = 0x0002;

        let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let byte_len = wide.len() * std::mem::size_of::<u16>();

        // SAFETY: Standard Win32 CF_UNICODETEXT clipboard transfer with checked pointers and cleanup.
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
                let _ = GlobalFree(h_mem);
                let _ = CloseClipboard();
                return Err("Clipboard GlobalLock failed".to_string());
            }
            ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            let _ = GlobalUnlock(h_mem);
            if SetClipboardData(CF_UNICODETEXT, h_mem).is_null() {
                let _ = GlobalFree(h_mem);
                let _ = CloseClipboard();
                return Err("SetClipboardData failed".to_string());
            }
            let _ = CloseClipboard();
        }
        Ok(())
    }
}

pub fn win32_harden_process() {
    #[cfg(target_os = "windows")]
    win_impl::harden_process();
}

pub fn win32_set_app_user_model_id() {
    #[cfg(target_os = "windows")]
    win_impl::set_app_user_model_id();
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

pub fn win32_set_native_acrylic(enabled: bool, opacity: f32, tint_rgb: (u8, u8, u8)) {
    #[cfg(target_os = "windows")]
    win_impl::set_native_acrylic(enabled, opacity, tint_rgb);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enabled, opacity, tint_rgb);
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

pub fn win32_update_taskbar_playback(title: &str, artist: &str, is_playing: bool) {
    #[cfg(target_os = "windows")]
    win_impl::update_taskbar_playback(title, artist, is_playing);
    #[cfg(not(target_os = "windows"))]
    let _ = (title, artist, is_playing);
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
