use std::path::Path;

/// Open a canonical HTTPS GitHub release URL in the user's default browser.
/// Rejects non-GitHub URLs fail-closed.
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
