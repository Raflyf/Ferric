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
    "https://livelydesk.vercel.app",
    "https://livelydesk.vercel.app/ferric",
    "https://yapp.ink/raflyf",
    "https://saweria.co/BuyMatcha",
];

/// Events emitted by the Win32 window subclass & System Tray icon back to Ferric's UI loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    ShowWindow,
    Minimized,
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

    // Open with FILE_SHARE_READ only (denying concurrent write/delete) BEFORE hashing, and keep
    // the read-locked handle open across CreateProcessW / msiexec execution to eliminate TOCTOU.
    #[cfg(target_os = "windows")]
    let mut locked_file = {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&canonical_msi)
            .map_err(|_| "Could not lock verified installer file for integrity check".to_string())?
    };
    #[cfg(not(target_os = "windows"))]
    let mut locked_file = std::fs::File::open(&canonical_msi)
        .map_err(|_| "Could not open verified installer file".to_string())?;

    let actual_sha = ferric_providers::updater::sha256_reader(&mut locked_file)?;
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
        win_impl::launch_msiexec(clean_target, locked_file)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (clean_target, locked_file);
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
        AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DestroyIcon,
        DestroyMenu, DestroyWindow, GetClientRect, GetCursorPos, HICON, ICONINFO, IDI_APPLICATION,
        LoadIconW, MF_SEPARATOR, MF_STRING, MINMAXINFO, NCCALCSIZE_PARAMS, PostMessageW, SW_HIDE,
        SW_RESTORE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow,
        TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP,
        WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_GETMINMAXINFO, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
        WM_MOVE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCPAINT, WM_NULL, WM_RBUTTONUP,
        WM_SHOWWINDOW, WM_SIZE, WM_SYSCOMMAND, WM_WINDOWPOSCHANGED, WS_CAPTION, WS_EX_TOOLWINDOW,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
    };
    use windows::core::{PCSTR, PCWSTR, s, w};

    const SUBCLASS_ID: usize = 0xFE11_C001;
    const TRAY_ICON_ID: u32 = 1;
    const WM_TRAYICON: u32 = WM_APP + 42;
    const WM_ACTIVATE_MSG: u32 = 0x0006;
    const WM_SETTINGCHANGE_MSG: u32 = 0x001A;
    const WM_THEMECHANGED_MSG: u32 = 0x031A;
    const WM_DWMCOMPOSITIONCHANGED_MSG: u32 = 0x031E;
    const LOAD_LIBRARY_SEARCH_SYSTEM32_FLAG: u32 = 0x0000_0800;
    const HKEY_CURRENT_USER_RAW: isize = -2147483647_isize; // 0x80000001
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
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

    impl Drop for TaskbarThumbState {
        fn drop(&mut self) {
            // SAFETY: Releases HICON handles created via `CreateIconIndirect` exactly once.
            unsafe {
                for ic in [self.ico_prev, self.ico_play, self.ico_pause, self.ico_next] {
                    if !ic.is_invalid() {
                        let _ = DestroyIcon(ic);
                    }
                }
            }
        }
    }

    thread_local! {
        static TASKBAR_STATE: RefCell<Option<TaskbarThumbState>> = const { RefCell::new(None) };
        static COM_INITIALIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    static ACRYLIC_ENABLED: AtomicBool = AtomicBool::new(false);
    static ACRYLIC_LIGHT: AtomicBool = AtomicBool::new(false);
    static ACRYLIC_TINT_RGB: AtomicU32 = AtomicU32::new(0x000B_0D14);
    static APPLIED_ACRYLIC_HWND: AtomicIsize = AtomicIsize::new(0);
    static APPLIED_ACRYLIC_ON: AtomicBool = AtomicBool::new(false);
    static APPLIED_ACRYLIC_LIGHT: AtomicBool = AtomicBool::new(false);
    static APPLIED_WCA_FALLBACK: AtomicBool = AtomicBool::new(false);
    static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
    static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(false);
    static FORCE_QUIT: AtomicBool = AtomicBool::new(false);
    static TRAY_ADDED: AtomicBool = AtomicBool::new(false);
    static MINI_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);
    static WINDOW_HIDDEN_OR_MIN: AtomicBool = AtomicBool::new(false);
    static HIDDEN_IN_TRAY: AtomicBool = AtomicBool::new(false);
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
        fn DwmFlush() -> i32;
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
        fn WaitForInputIdle(h_process: *mut c_void, dw_milliseconds: u32) -> u32;
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

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            hkey: isize,
            lp_sub_key: *const u16,
            lp_value: *const u16,
            dw_flags: u32,
            pdw_type: *mut u32,
            pv_data: *mut c_void,
            pcb_data: *mut u32,
        ) -> i32;
        fn RegCreateKeyExW(
            hkey: isize,
            lp_sub_key: *const u16,
            reserved: u32,
            lp_class: *const u16,
            dw_options: u32,
            sam_desired: u32,
            lp_security_attributes: *const c_void,
            phk_result: *mut isize,
            lpdw_disposition: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            hkey: isize,
            lp_value_name: *const u16,
            reserved: u32,
            dw_type: u32,
            lp_data: *const u8,
            cb_data: u32,
        ) -> i32;
        fn RegCloseKey(hkey: isize) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(
            lp_lib_file_name: *const u16,
            h_file: *mut c_void,
            dw_flags: u32,
        ) -> *mut c_void;
        fn CreateMutexW(
            lp_mutex_attributes: *mut c_void,
            b_initial_owner: i32,
            lp_name: *const u16,
        ) -> *mut c_void;
        fn GetLastError() -> u32;
        fn CloseHandle(h_object: *mut c_void) -> i32;
        fn WaitForSingleObject(h_handle: *mut c_void, dw_milliseconds: u32) -> u32;
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

    /// Set a deterministic AppUserModelID and ensure HKCU AUMID metadata (`DisplayName`, `IconUri`)
    /// is registered so Windows 11 Taskbar, Quick Settings, and third-party SMTC overlays (Nusa Pro,
    /// NotchPixel, ModernFlyouts) consistently identify Ferric with its icon.
    pub fn set_app_user_model_id() {
        const KEY_SET_VALUE: u32 = 0x0002;
        const REG_SZ: u32 = 1;
        let app_id: Vec<u16> = "Raflyf.Ferric\0".encode_utf16().collect();
        // SAFETY: Null-terminated UTF-16 AppUserModelID passed once at process startup.
        unsafe {
            let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
        }

        let subkey: Vec<u16> = "Software\\Classes\\AppUserModelId\\Raflyf.Ferric\0"
            .encode_utf16()
            .collect();
        let mut hkey: isize = 0;
        // SAFETY: Creates/opens current-user AUMID registry key and sets DisplayName & IconUri.
        unsafe {
            if RegCreateKeyExW(
                HKEY_CURRENT_USER_RAW,
                subkey.as_ptr(),
                0,
                ptr::null(),
                0,
                KEY_SET_VALUE,
                ptr::null(),
                &raw mut hkey,
                ptr::null_mut(),
            ) == 0
                && hkey != 0
            {
                let name_k: Vec<u16> = "DisplayName\0".encode_utf16().collect();
                let name_v: Vec<u16> = "Ferric\0".encode_utf16().collect();
                let _ = RegSetValueExW(
                    hkey,
                    name_k.as_ptr(),
                    0,
                    REG_SZ,
                    name_v.as_ptr().cast(),
                    (name_v.len() * 2) as u32,
                );
                if let Ok(exe_path) = std::env::current_exe() {
                    let ico_candidate = exe_path
                        .parent()
                        .map(|d| d.join("ferric.ico"))
                        .filter(|p| p.is_file())
                        .unwrap_or(exe_path);
                    let icon_k: Vec<u16> = "IconUri\0".encode_utf16().collect();
                    let icon_v: Vec<u16> = ico_candidate
                        .to_string_lossy()
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    let _ = RegSetValueExW(
                        hkey,
                        icon_k.as_ptr(),
                        0,
                        REG_SZ,
                        icon_v.as_ptr().cast(),
                        (icon_v.len() * 2) as u32,
                    );
                }
                let _ = RegCloseKey(hkey);
            }
        }
    }

    pub fn launch_msiexec(msi_path: &str, locked_file: std::fs::File) -> Result<(), String> {
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
        }
        let proc_raw = pi.h_process as usize;
        if proc_raw != 0 {
            // Wait up to 3 seconds for msiexec.exe to finish initializing and open the MSI package
            // while Ferric still holds the FILE_SHARE_READ lock prior to exiting.
            // SAFETY: `proc_raw` is a valid process handle from CreateProcessW.
            unsafe {
                let _ = WaitForInputIdle(proc_raw as *mut c_void, 3_000);
            }
        }
        // Keep the FILE_SHARE_READ handle open in a waiter thread until either msiexec exits
        // or Ferric completes graceful shutdown; per-user ACLs guard `%LOCALAPPDATA%\LivelyDesk\Ferric\updates`.
        let spawned = std::thread::Builder::new()
            .name("ferric-msi-lock".to_string())
            .spawn(move || {
                let _keep_locked = locked_file;
                if proc_raw != 0 {
                    // SAFETY: `proc_raw` is the owned process handle from CreateProcessW.
                    unsafe {
                        let _ = WaitForSingleObject(proc_raw as *mut c_void, 0xFFFF_FFFF);
                        let _ = CloseHandle(proc_raw as *mut c_void);
                    }
                }
            });
        if spawned.is_err() && proc_raw != 0 {
            // SAFETY: Clean up handle if waiter thread could not spawn.
            unsafe {
                let _ = CloseHandle(proc_raw as *mut c_void);
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

    /// Query Windows 10/11 system/taskbar theme (`SystemUsesLightTheme` -> `AppsUseLightTheme`).
    /// Returns `true` when device system theme is Light Mode, `false` when Dark Mode (default).
    fn windows_device_uses_light_theme() -> bool {
        let subkey: Vec<u16> =
            "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
                .encode_utf16()
                .collect();
        for val_name in ["SystemUsesLightTheme\0", "AppsUseLightTheme\0"] {
            let val_w: Vec<u16> = val_name.encode_utf16().collect();
            let mut dword: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            // SAFETY: Read-only registry query into stack `u32` buffer.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER_RAW,
                    subkey.as_ptr(),
                    val_w.as_ptr(),
                    RRF_RT_REG_DWORD,
                    ptr::null_mut(),
                    (&raw mut dword).cast(),
                    &raw mut size,
                )
            };
            if status == 0 {
                return dword != 0;
            }
        }
        false
    }

    /// Synchronize native Win32 popup/context menu styling (e.g. `TrackPopupMenu` on tray right-click)
    /// with the Windows device dark/light theme via `uxtheme.dll` ordinals #135 & #136.
    fn sync_win32_popup_menu_theme() {
        let mode: i32 = if windows_device_uses_light_theme() {
            3 // ForceLight — matches Windows Light Taskbar/System theme
        } else {
            2 // ForceDark — native Windows 11 Dark Mode menu matching dark taskbar
        };
        // SAFETY: Loads system `uxtheme.dll` strictly from System32 and invokes well-known
        // `SetPreferredAppMode` (#135) and `FlushMenuThemes` (#136) if exported.
        unsafe {
            let hmod = match GetModuleHandleW(w!("uxtheme.dll")) {
                Ok(m) => m,
                Err(_) => {
                    let lib_w: Vec<u16> = "uxtheme.dll\0".encode_utf16().collect();
                    let raw = LoadLibraryExW(
                        lib_w.as_ptr(),
                        ptr::null_mut(),
                        LOAD_LIBRARY_SEARCH_SYSTEM32_FLAG,
                    );
                    if raw.is_null() {
                        return;
                    }
                    windows::Win32::Foundation::HMODULE(raw)
                }
            };
            if let Some(p_set) = GetProcAddress(hmod, PCSTR(std::ptr::without_provenance(135usize)))
            {
                type SetPreferredAppModeFn = unsafe extern "system" fn(i32) -> i32;
                let set_mode: SetPreferredAppModeFn = std::mem::transmute(p_set);
                let _ = set_mode(mode);
            }
            if let Some(p_flush) =
                GetProcAddress(hmod, PCSTR(std::ptr::without_provenance(136usize)))
            {
                type FlushMenuThemesFn = unsafe extern "system" fn();
                let flush: FlushMenuThemesFn = std::mem::transmute(p_flush);
                flush();
            }
        }
    }

    /// Lightweight DWM extended-frame keepalive that preserves Native Acrylic across taskbar
    /// minimize/restore and focus changes WITHOUT resetting the backdrop brush or triggering
    /// `SWP_FRAMECHANGED` (eliminating the rare single-frame solid flash on restore).
    unsafe fn keep_acrylic_frame_alive(hwnd: HWND, flush_dwm: bool) {
        if !ACRYLIC_ENABLED.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: Valid HWND on UI thread with DWM frame-margin keepalive.
        unsafe {
            let margins = DwmMargins {
                cx_left_width: -1,
                cx_right_width: -1,
                cy_top_height: -1,
                cy_bottom_height: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
            if APPLIED_WCA_FALLBACK.load(Ordering::Acquire) {
                apply_wca_acrylic_fallback(hwnd, true);
            }
            if flush_dwm {
                let _ = DwmFlush();
            }
        }
    }

    /// Synchronously re-assert full DWM extended frame + Native Acrylic backdrop after
    /// `SW_HIDE` (System Tray restore) or OS theme/composition resets.
    unsafe fn reassert_acrylic_backdrop_now(hwnd: HWND, flush_frame: bool) {
        if !ACRYLIC_ENABLED.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: Valid HWND on UI thread with DWM/user32 calls.
        unsafe {
            let dark: i32 = if ACRYLIC_LIGHT.load(Ordering::Acquire) {
                0
            } else {
                1
            };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                std::mem::size_of::<i32>() as u32,
            );
            let margins = DwmMargins {
                cx_left_width: -1,
                cx_right_width: -1,
                cy_top_height: -1,
                cy_bottom_height: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &raw const margins);
            let backdrop: i32 = DWMSBT_TRANSIENTWINDOW;
            let hr = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                (&raw const backdrop).cast(),
                std::mem::size_of::<i32>() as u32,
            );
            if hr != 0 {
                apply_wca_acrylic_fallback(hwnd, true);
            }
            if flush_frame && IsWindowVisible(hwnd.0) != 0 {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = DwmFlush();
            }
        }
    }

    pub fn set_native_acrylic(
        enabled: bool,
        _opacity: f32,
        tint_rgb: (u8, u8, u8),
        is_light: bool,
    ) {
        ACRYLIC_ENABLED.store(enabled, Ordering::Release);
        ACRYLIC_LIGHT.store(is_light, Ordering::Release);
        let packed = ((tint_rgb.0 as u32) << 16) | ((tint_rgb.1 as u32) << 8) | (tint_rgb.2 as u32);
        ACRYLIC_TINT_RGB.store(packed, Ordering::Release);
        let Some(hwnd) = active_process_hwnd() else {
            return;
        };
        let hwnd_raw = hwnd.0 as isize;
        if !APPLIED_WCA_FALLBACK.load(Ordering::Acquire)
            && APPLIED_ACRYLIC_HWND.load(Ordering::Acquire) == hwnd_raw
            && APPLIED_ACRYLIC_ON.load(Ordering::Acquire) == enabled
            && APPLIED_ACRYLIC_LIGHT.load(Ordering::Acquire) == is_light
        {
            // Windows 11 Native Acrylic backdrop is already applied in this state on this HWND;
            // skip redundant DwmSetWindowAttribute + SWP_FRAMECHANGED calls so adaptive accent
            // updates or opacity drags never flash the DWM backdrop.
            return;
        }
        // SAFETY: Enables true per-pixel Windows 11 DWM Acrylic directly behind Ferric's
        // premultiplied DirectComposition swapchain — zero WS_EX_LAYERED text dimming.
        unsafe {
            let dark: i32 = if is_light { 0 } else { 1 };
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
                APPLIED_WCA_FALLBACK.store(true, Ordering::Release);
                apply_wca_acrylic_fallback(hwnd, enabled);
            } else {
                APPLIED_WCA_FALLBACK.store(false, Ordering::Release);
            }
            APPLIED_ACRYLIC_ON.store(enabled, Ordering::Release);
            APPLIED_ACRYLIC_LIGHT.store(is_light, Ordering::Release);
            APPLIED_ACRYLIC_HWND.store(hwnd_raw, Ordering::Release);
            if IsWindowVisible(hwnd.0) != 0 {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = DwmFlush();
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
        buf[..len as usize]
            .iter()
            .copied()
            .eq("FerricMainWindow".encode_utf16())
    }

    fn push_event(ev: TrayEvent) {
        let should_wake = ev != TrayEvent::WindowMovedOrResized;
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
        if should_wake {
            makepad_widgets::SignalToUI::set_ui_signal();
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
            sync_win32_popup_menu_theme();
            init_taskbar_thumbbar(hwnd);
            let acr_on = ACRYLIC_ENABLED.load(Ordering::Acquire);
            let is_light = ACRYLIC_LIGHT.load(Ordering::Acquire);
            let packed = ACRYLIC_TINT_RGB.load(Ordering::Acquire);
            let tint = (
                ((packed >> 16) & 0xFF) as u8,
                ((packed >> 8) & 0xFF) as u8,
                (packed & 0xFF) as u8,
            );
            set_native_acrylic(acr_on, 0.72, tint, is_light);
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
        // SAFETY: STA COM creation of Shell ITaskbarList3 on Ferric's UI thread (initialized once).
        unsafe {
            COM_INITIALIZED.with(|init| {
                if !init.get() {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    init.set(true);
                }
            });
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
            // Dropping `old_state` runs `TaskbarThumbState::drop` to release previous HICONs.
            drop(old_state);
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
        drop(taken);
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

    pub fn style_dialog_window(hwnd_raw: isize) {
        if hwnd_raw == 0 {
            return;
        }
        let hwnd = HWND(hwnd_raw as *mut c_void);
        apply_window_icon(hwnd);
        // SAFETY: Valid top-level dialog HWND owned by this process; apply Windows 11 dark titlebar.
        unsafe {
            let dark: i32 = 1;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&raw const dark).cast(),
                std::mem::size_of::<i32>() as u32,
            );
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
            for (i, ch) in tooltip.encode_utf16().take(126).enumerate() {
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
        // SAFETY: Standard synchronous Win32 popup menu owned by an invisible helper HWND so
        // right-clicking the tray icon dismisses cleanly on outside clicks WITHOUT raising or
        // stealing Z-order/focus for Ferric's main window.
        unsafe {
            sync_win32_popup_menu_theme();
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

            let hinst = GetModuleHandleW(None).ok().map(Into::into);
            let helper_hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                hinst,
                None,
            )
            .ok();
            let menu_owner = helper_hwnd.unwrap_or(hwnd);
            let _ = SetForegroundWindow(menu_owner);
            let cmd = TrackPopupMenu(
                hmenu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
                pt.x,
                pt.y,
                Some(0),
                menu_owner,
                None,
            );
            let _ = PostMessageW(Some(menu_owner), WM_NULL, WPARAM(0), LPARAM(0));
            if let Some(h) = helper_hwnd {
                let _ = DestroyWindow(h);
            }
            let _ = DestroyMenu(hmenu);

            match cmd.0 as u32 {
                CMD_SHOW => {
                    let from_tray = HIDDEN_IN_TRAY.swap(false, Ordering::AcqRel);
                    WINDOW_HIDDEN_OR_MIN.store(false, Ordering::Release);
                    if from_tray {
                        reassert_acrylic_backdrop_now(hwnd, false);
                    } else {
                        keep_acrylic_frame_alive(hwnd, false);
                    }
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    if IsIconic(hwnd.0) != 0 {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(hwnd);
                    apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);
                    if from_tray {
                        let _ = DwmFlush();
                    }
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
                let from_tray = HIDDEN_IN_TRAY.swap(false, Ordering::AcqRel);
                WINDOW_HIDDEN_OR_MIN.store(false, Ordering::Release);
                if from_tray {
                    reassert_acrylic_backdrop_now(hwnd, false);
                } else {
                    keep_acrylic_frame_alive(hwnd, false);
                }
                let _ = ShowWindow(hwnd, SW_SHOW);
                if IsIconic(hwnd.0) != 0 {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
                apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);
                if from_tray {
                    let _ = DwmFlush();
                }
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
                        let dpi = GetDpiForWindow(hwnd.0).max(96) as i32;
                        if MINI_MODE_ACTIVE.load(Ordering::Relaxed) {
                            (*mmi).ptMinTrackSize.x = (460 * dpi) / 96;
                            (*mmi).ptMinTrackSize.y = (110 * dpi) / 96;
                        } else {
                            (*mmi).ptMinTrackSize.x = (960 * dpi) / 96;
                            (*mmi).ptMinTrackSize.y = (620 * dpi) / 96;
                        }
                        return LRESULT(0);
                    }
                }
                WM_SYSCOMMAND => {
                    const SC_RESTORE_MASKED: usize = 0xF120;
                    if (wparam.0 & 0xFFF0) == SC_RESTORE_MASKED
                        && IsIconic(hwnd.0) != 0
                        && ACRYLIC_ENABLED.load(Ordering::Acquire)
                    {
                        // Pre-arm DWM extended frame + Native Acrylic attribute BEFORE Windows
                        // starts the taskbar unminimize animation (without SWP_FRAMECHANGED).
                        reassert_acrylic_backdrop_now(hwnd, false);
                    }
                }
                WM_NCACTIVATE => {
                    // Forward real activation `wparam` with `lParam = -1` so classic GDI caption
                    // chrome is suppressed while DWM receives authentic activate/deactivate state
                    // transitions without resetting the Acrylic backdrop brush.
                    if IsIconic(hwnd.0) == 0 {
                        let res = DefSubclassProc(hwnd, msg, wparam, LPARAM(-1));
                        if wparam.0 != 0 && ACRYLIC_ENABLED.load(Ordering::Acquire) {
                            keep_acrylic_frame_alive(hwnd, false);
                        }
                        return res;
                    }
                    return DefSubclassProc(hwnd, msg, wparam, lparam);
                }
                WM_ACTIVATE_MSG => {
                    let res = DefSubclassProc(hwnd, msg, wparam, lparam);
                    let active_state = wparam.0 & 0xFFFF;
                    if active_state != 0
                        && IsIconic(hwnd.0) == 0
                        && ACRYLIC_ENABLED.load(Ordering::Acquire)
                    {
                        keep_acrylic_frame_alive(hwnd, false);
                    }
                    return res;
                }
                WM_SETTINGCHANGE_MSG => {
                    sync_win32_popup_menu_theme();
                }
                WM_THEMECHANGED_MSG | WM_DWMCOMPOSITIONCHANGED_MSG => {
                    sync_win32_popup_menu_theme();
                    if ACRYLIC_ENABLED.load(Ordering::Acquire) {
                        reassert_acrylic_backdrop_now(hwnd, true);
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
                        // SIZE_MINIMIZED -> pause 60 FPS hidden animations without nuking VRAM cover cache
                        // or paging out DirectComposition so taskbar restore is instant with zero solid-blur flash.
                        if !WINDOW_HIDDEN_OR_MIN.swap(true, Ordering::AcqRel) {
                            push_event(TrayEvent::Minimized);
                        }
                    } else if WINDOW_HIDDEN_OR_MIN.swap(false, Ordering::AcqRel) {
                        let _from_tray = HIDDEN_IN_TRAY.swap(false, Ordering::AcqRel);
                        // Re-assert DWM Acrylic attribute without SWP_FRAMECHANGED and flush DWM
                        // so Windows 11 Build 26200 never lingers in solid fallback after unminimize.
                        reassert_acrylic_backdrop_now(hwnd, false);
                        if ACRYLIC_ENABLED.load(Ordering::Acquire) {
                            let _ = DwmFlush();
                        }
                        push_event(TrayEvent::ShowWindow);
                    } else {
                        push_event(TrayEvent::WindowMovedOrResized);
                    }
                }
                WM_SHOWWINDOW | WM_MOVE | WM_WINDOWPOSCHANGED => {
                    push_event(TrayEvent::WindowMovedOrResized);
                }
                0x0011 => {
                    // WM_QUERYENDSESSION: always consent to OS shutdown / MSI Restart Manager close.
                    return LRESULT(1);
                }
                0x0016 => {
                    // WM_ENDSESSION: exit immediately even when Close-to-Tray is enabled.
                    if wparam.0 != 0 {
                        FORCE_QUIT.store(true, Ordering::Release);
                        remove_tray_icon(hwnd);
                        push_event(TrayEvent::QuitApp);
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return LRESULT(0);
                }
                WM_CLOSE => {
                    if CLOSE_TO_TRAY.load(Ordering::Acquire) && !FORCE_QUIT.load(Ordering::Acquire)
                    {
                        ensure_tray_icon(hwnd, "Ferric — Playing in background");
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        WINDOW_HIDDEN_OR_MIN.store(true, Ordering::Release);
                        HIDDEN_IN_TRAY.store(true, Ordering::Release);
                        push_event(TrayEvent::HiddenToTray);
                        return LRESULT(0);
                    }
                    remove_tray_icon(hwnd);
                }
                WM_TRAYICON => {
                    let ev = (lparam.0 & 0xFFFF) as u32;
                    if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                        let from_tray = HIDDEN_IN_TRAY.swap(false, Ordering::AcqRel);
                        WINDOW_HIDDEN_OR_MIN.store(false, Ordering::Release);
                        if from_tray {
                            reassert_acrylic_backdrop_now(hwnd, false);
                        } else {
                            keep_acrylic_frame_alive(hwnd, false);
                        }
                        let _ = ShowWindow(hwnd, SW_SHOW);
                        if IsIconic(hwnd.0) != 0 {
                            let _ = ShowWindow(hwnd, SW_RESTORE);
                        }
                        let _ = SetForegroundWindow(hwnd);
                        apply_dwm_corner_pref(hwnd, IsZoomed(hwnd.0) != 0);
                        if from_tray {
                            let _ = DwmFlush();
                        }
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

pub fn win32_set_native_acrylic(
    enabled: bool,
    opacity: f32,
    tint_rgb: (u8, u8, u8),
    is_light: bool,
) {
    #[cfg(target_os = "windows")]
    win_impl::set_native_acrylic(enabled, opacity, tint_rgb, is_light);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enabled, opacity, tint_rgb, is_light);
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

pub fn win32_style_dialog_window(hwnd_raw: isize) {
    #[cfg(target_os = "windows")]
    {
        win_impl::style_dialog_window(hwnd_raw);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd_raw;
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

#[cfg(test)]
mod tests {
    use super::is_safe_external_url;

    #[test]
    fn allows_official_about_and_support_urls_fail_closed() {
        for ok_url in [
            "https://github.com/Raflyf/Ferric",
            "https://github.com/Raflyf/Ferric/releases",
            "https://github.com/Raflyf/Ferric/blob/main/CHANGELOG.md",
            "https://github.com/Raflyf/Ferric/issues",
            "https://github.com/Raflyf/Ferric/blob/main/LICENSE",
            "https://livelydesk.vercel.app/ferric",
            "https://yapp.ink/raflyf",
            "https://saweria.co/BuyMatcha",
        ] {
            assert!(is_safe_external_url(ok_url), "expected allowed: {ok_url}");
        }
        for bad_url in [
            "http://livelydesk.vercel.app/ferric",
            "https://livelydesk.vercel.app/other",
            "https://yapp.ink/other",
            "https://saweria.co/other",
            "https://github.com/Raflyf/Ferric/../../evil",
            "https://evil.example/https://github.com/Raflyf/Ferric",
        ] {
            assert!(
                !is_safe_external_url(bad_url),
                "expected rejected: {bad_url}"
            );
        }
    }
}
