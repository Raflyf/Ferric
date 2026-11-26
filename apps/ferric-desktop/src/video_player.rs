//! Native in-app hardware-accelerated Music Video player docked seamlessly inside Ferric.
//! Spawns an owned borderless WebView2 surface positioned precisely inside Ferric's video viewport,
//! plays direct MP4/WebM streams (360p/480p/720p/1080p) muted and time-synced to Ferric's Rust
//! WASAPI audio engine (so EQ, volume, seek, and crossfade stay 100% native), and tears down
//! completely when closed so idle RAM remains under 100 MB.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VideoViewportRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[cfg(target_os = "windows")]
mod imp {
    use super::VideoViewportRect;
    use ferric_providers::ytmusic::is_allowed_stream_url;
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::path::{Component, Path, PathBuf, Prefix};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_STATE_DENY, CreateCoreWebView2EnvironmentWithOptions,
        ICoreWebView2, ICoreWebView2Controller, ICoreWebView2EnvironmentOptions,
        ICoreWebView2Settings3, ICoreWebView2Settings4, ICoreWebView2Settings6,
    };
    use webview2_com::{
        CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler, NavigationStartingEventHandler,
        NewWindowRequestedEventHandler, PermissionRequestedEventHandler, take_pwstr,
    };
    use windows::Win32::Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::UpdateWindow;
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
        GetClientRect, GetMessageW, GetWindowLongPtrW, HWND_TOP, KillTimer, MSG, PostMessageW,
        PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER,
        SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE,
        WM_DESTROY, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{Interface, PCWSTR, PWSTR, w};

    const WM_WAKE_VIDEO_CMD: u32 = WM_APP + 77;
    const CMD_POLL_TIMER: usize = 2001;

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> *mut c_void;
        fn DeleteObject(ho: *mut c_void) -> i32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowRgn(hwnd: *mut c_void, hrgn: *mut c_void, redraw: i32) -> i32;
        fn GetDpiForWindow(hwnd: *mut c_void) -> u32;
    }

    /// Clip the docked WebView2 surface with a single DPI-exact 16px rounded region (matching
    /// `#vignette`'s `border-radius: 16px`) and zero competing OS DWM corner frames.
    fn apply_rounded_video_corners(hwnd: HWND, width: i32, height: i32) {
        let w = width.max(2);
        let h = height.max(2);
        // SAFETY: Valid owned popup HWND on the video STA thread; Windows owns HRGN on success.
        unsafe {
            let dpi = GetDpiForWindow(hwnd.0).max(96) as f32;
            let radius = (16.0 * dpi / 96.0).round().clamp(12.0, 36.0) as i32;
            let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, radius * 2, radius * 2);
            if !rgn.is_null() && SetWindowRgn(hwnd.0, rgn, 1) == 0 {
                let _ = DeleteObject(rgn);
            }
        }
    }

    fn is_allowed_local_video_uri(raw: &str) -> bool {
        let Some(rest) = raw.strip_prefix("file:///") else {
            return false;
        };
        if raw.bytes().any(|b| b.is_ascii_control()) || rest.starts_with('/') {
            return false;
        }
        let path = Path::new(rest);
        for comp in path.components() {
            match comp {
                Component::Prefix(pref) => match pref.kind() {
                    Prefix::Disk(_) | Prefix::VerbatimDisk(_) => {}
                    _ => return false,
                },
                Component::ParentDir => return false,
                _ => {}
            }
        }
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "webm" | "mkv"));
        ext_ok && path.is_file()
    }

    fn is_allowed_video_stream_url(raw: &str) -> bool {
        is_allowed_stream_url(raw) || is_allowed_local_video_uri(raw)
    }

    fn safe_pos_secs(pos: f64) -> f64 {
        if pos.is_finite() && pos >= 0.0 {
            pos.min(86_400.0)
        } else {
            0.0
        }
    }

    /// JSON-encode string and escape HTML/JS breakout characters (`</script>`, U+2028/U+2029).
    fn js_quote_safe(s: &str) -> String {
        serde_json::to_string(s)
            .unwrap_or_else(|_| "\"\"".to_string())
            .replace('<', "\\u003c")
            .replace('>', "\\u003e")
            .replace('&', "\\u0026")
            .replace('/', "\\/")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    }

    enum VideoCmd {
        LoadStream {
            url: String,
            pos_sec: f64,
            playing: bool,
        },
        SyncPlayback {
            pos_sec: f64,
            playing: bool,
        },
        SetBounds(VideoViewportRect),
        SetVisible(bool),
        Close,
    }

    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            // SAFETY: Matches successful `CoInitializeEx` on this STA thread.
            unsafe { CoUninitialize() };
        }
    }

    struct VideoHostState {
        controller: ICoreWebView2Controller,
        webview: ICoreWebView2,
        cmd_rx: RefCell<Receiver<VideoCmd>>,
        shared_hwnd: Arc<AtomicIsize>,
        is_visible: Cell<bool>,
        last_bounds: Cell<VideoViewportRect>,
    }

    pub struct NativeVideoPlayer {
        cmd_tx: Option<Sender<VideoCmd>>,
        host_hwnd_raw: Arc<AtomicIsize>,
        profile_dir: PathBuf,
    }

    impl NativeVideoPlayer {
        pub fn new(profile_dir: PathBuf) -> Self {
            Self {
                cmd_tx: None,
                host_hwnd_raw: Arc::new(AtomicIsize::new(0)),
                profile_dir,
            }
        }

        pub fn is_open(&self) -> bool {
            self.cmd_tx.is_some()
        }

        pub fn open_or_load(
            &mut self,
            owner_hwnd_raw: isize,
            rect: VideoViewportRect,
            stream_url: String,
            pos_sec: f64,
            playing: bool,
            initially_visible: bool,
        ) {
            if !is_allowed_video_stream_url(&stream_url) {
                return;
            }
            let pos_sec = safe_pos_secs(pos_sec);
            if let Some(tx) = &self.cmd_tx {
                let sent = tx.send(VideoCmd::SetBounds(rect)).is_ok()
                    && tx.send(VideoCmd::SetVisible(initially_visible)).is_ok()
                    && tx
                        .send(VideoCmd::LoadStream {
                            url: stream_url.clone(),
                            pos_sec,
                            playing,
                        })
                        .is_ok();
                if sent {
                    self.wake_thread();
                    return;
                }
                // Worker exited (e.g. WebView2 init error or closed window); clear dead handle and respawn.
                self.cmd_tx = None;
                self.host_hwnd_raw = Arc::new(AtomicIsize::new(0));
            }

            let (tx, rx) = mpsc::channel::<VideoCmd>();
            let (hwnd_tx, _hwnd_rx) = mpsc::channel::<isize>();
            let profile_dir = self.profile_dir.clone();
            // Fresh per-session AtomicIsize prevents a closing previous worker from clobbering
            // a newly spawned worker's active HWND.
            let session_hwnd = Arc::new(AtomicIsize::new(0));
            let shared_hwnd = Arc::clone(&session_hwnd);
            if thread::Builder::new()
                .name("ferric-video-stage".to_string())
                .spawn(move || {
                    let _ = run_video_sta(
                        owner_hwnd_raw,
                        rect,
                        profile_dir,
                        stream_url,
                        pos_sec,
                        playing,
                        initially_visible,
                        rx,
                        hwnd_tx,
                        shared_hwnd,
                    );
                })
                .is_ok()
            {
                self.host_hwnd_raw = session_hwnd;
                self.cmd_tx = Some(tx);
            }
        }

        pub fn set_bounds(&self, rect: VideoViewportRect) {
            if let Some(tx) = &self.cmd_tx {
                let _ = tx.send(VideoCmd::SetBounds(rect));
                self.wake_thread();
            }
        }

        pub fn set_visible(&self, visible: bool) {
            if let Some(tx) = &self.cmd_tx {
                let _ = tx.send(VideoCmd::SetVisible(visible));
                self.wake_thread();
            }
        }

        pub fn sync_playback(&self, pos_sec: f64, playing: bool) {
            if let Some(tx) = &self.cmd_tx {
                let _ = tx.send(VideoCmd::SyncPlayback {
                    pos_sec: safe_pos_secs(pos_sec),
                    playing,
                });
                self.wake_thread();
            }
        }

        pub fn close(&mut self) {
            if let Some(tx) = self.cmd_tx.take() {
                let _ = tx.send(VideoCmd::Close);
                self.wake_thread();
            }
            self.host_hwnd_raw.store(0, Ordering::Release);
        }

        fn wake_thread(&self) {
            let raw = self.host_hwnd_raw.load(Ordering::Acquire);
            if raw != 0 {
                // SAFETY: Non-blocking wake message posted to our STA video window; cleared on WM_DESTROY.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(raw as *mut c_void)),
                        WM_WAKE_VIDEO_CMD,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    }

    impl Drop for NativeVideoPlayer {
        fn drop(&mut self) {
            self.close();
        }
    }

    fn player_shell_html(
        initial_url: &str,
        initial_pos: f64,
        initial_playing: bool,
        initially_visible: bool,
    ) -> String {
        let quoted_url = js_quote_safe(initial_url);
        let safe_pos = safe_pos_secs(initial_pos);
        let init_play = if initial_playing { "true" } else { "false" };
        let init_visible_js = if initially_visible { "true" } else { "false" };
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="referrer" content="no-referrer">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; media-src https://*.googlevideo.com file:; style-src 'unsafe-inline'; script-src 'unsafe-inline';">
<style>
  html, body {{
    margin: 0; padding: 0; width: 100%; height: 100%;
    background: #0A0C12; overflow: hidden;
    user-select: none; position: relative;
  }}
  #stage {{
    width: 100%; height: 100%;
    display: flex; align-items: center; justify-content: center;
    position: relative; opacity: 0;
    transition: opacity 220ms cubic-bezier(0.22, 1, 0.36, 1);
  }}
  #stage.shown {{ opacity: 1; }}
  video {{
    width: 100%; height: 100%; object-fit: cover;
    background: #0A0C12; opacity: 0;
    transition: opacity 260ms ease-out;
  }}
  video.contain {{ object-fit: contain; }}
  video.ready {{ opacity: 1; }}
  #vignette {{
    position: absolute; inset: 0; pointer-events: none;
    box-shadow: inset 0 0 22px rgba(0, 0, 0, 0.34), inset 0 0 0 1px rgba(255, 255, 255, 0.14);
    border-radius: 16px;
  }}
  #msg {{
    position: absolute; color: #B4B8C8; font: 500 13px/1.4 "Segoe UI", sans-serif;
    letter-spacing: 0.2px; pointer-events: none;
  }}
</style>
</head>
<body>
  <div id="stage">
    <div id="msg">Loading video stream...</div>
    <video id="v" muted playsinline autoplay preload="auto" disablePictureInPicture controlsList="nodownload noplaybackrate"></video>
    <div id="vignette"></div>
  </div>
  <script>
    document.addEventListener('contextmenu', e => e.preventDefault());
    document.addEventListener('dragstart', e => e.preventDefault());
    const stage = document.getElementById('stage');
    const v = document.getElementById('v');
    const msg = document.getElementById('msg');
    let desiredPos = {safe_pos:.3};
    let desiredPlaying = {init_play};
    window.__ferricStageVisible = (vis) => {{
      if (vis) {{
        stage.classList.remove('shown');
        requestAnimationFrame(() => requestAnimationFrame(() => stage.classList.add('shown')));
      }} else {{
        stage.classList.remove('shown');
      }}
    }};
    if ({init_visible_js}) {{
      requestAnimationFrame(() => requestAnimationFrame(() => stage.classList.add('shown')));
    }}
    const reveal = () => {{
      if (v.videoWidth > 0 && v.videoHeight > 0) {{
        const ar = v.videoWidth / v.videoHeight;
        v.classList.toggle('contain', ar < 1.25);
      }}
      msg.style.display = 'none';
      v.classList.add('ready');
      try {{
        if (Number.isFinite(desiredPos) && desiredPos > 0.25 && v.duration && desiredPos < v.duration) {{
          if (Math.abs((v.currentTime || 0) - desiredPos) > 0.6) {{
            v.currentTime = desiredPos;
          }}
        }}
        if (desiredPlaying) {{ v.play().catch(() => {{}}); }} else {{ v.pause(); }}
      }} catch (_) {{}}
    }};
    v.addEventListener('loadeddata', reveal);
    v.addEventListener('canplay', reveal);
    v.addEventListener('error', () => {{
      msg.style.display = 'block';
      msg.textContent = 'Video stream unavailable for this track';
    }});
    window.__ferricLoad = (url, pos, playing) => {{
      desiredPos = Number.isFinite(pos) ? pos : 0;
      desiredPlaying = !!playing;
      v.classList.remove('ready');
      msg.style.display = 'block';
      msg.textContent = 'Switching stream...';
      v.src = url;
      v.load();
      if (desiredPlaying) {{ v.play().catch(() => {{}}); }}
    }};
    window.__ferricSync = (pos, playing) => {{
      desiredPos = Number.isFinite(pos) ? pos : 0;
      desiredPlaying = !!playing;
      if (v.readyState >= 2) {{
        if (Math.abs((v.currentTime || 0) - desiredPos) > 0.55) {{
          try {{ v.currentTime = desiredPos; }} catch (_) {{}}
        }}
        if (desiredPlaying && v.paused) {{
          v.play().catch(() => {{}});
        }} else if (!desiredPlaying && !v.paused) {{
          v.pause();
        }}
      }}
    }};
    window.__ferricLoad({quoted_url}, {safe_pos:.3}, {init_play});
  </script>
</body>
</html>"#
        )
    }

    unsafe fn harden_video_webview(wv: &ICoreWebView2) {
        // SAFETY: STA-thread COM settings and event handler registration on our WebView2 instance.
        unsafe {
            if let Ok(settings) = wv.Settings() {
                let _ = settings.SetAreDevToolsEnabled(false);
                let _ = settings.SetAreDefaultContextMenusEnabled(false);
                let _ = settings.SetIsStatusBarEnabled(false);
                let _ = settings.SetAreHostObjectsAllowed(false);
                let _ = settings.SetIsWebMessageEnabled(false);
                let _ = settings.SetAreDefaultScriptDialogsEnabled(false);
                let _ = settings.SetIsZoomControlEnabled(false);
                let _ = settings.SetIsBuiltInErrorPageEnabled(false);
                if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                    let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                }
                if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                    let _ = s4.SetIsPasswordAutosaveEnabled(false);
                    let _ = s4.SetIsGeneralAutofillEnabled(false);
                }
                if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
                    let _ = s6.SetIsSwipeNavigationEnabled(false);
                }
            }

            let new_win_handler = NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(a) = args {
                    let _ = a.SetHandled(true);
                }
                Ok(())
            }));
            let mut nw_tok = 0_i64;
            let _ = wv.add_NewWindowRequested(&new_win_handler, &mut nw_tok);

            let perm_handler = PermissionRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(a) = args {
                    let _ = a.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY);
                }
                Ok(())
            }));
            let mut perm_tok = 0_i64;
            let _ = wv.add_PermissionRequested(&perm_handler, &mut perm_tok);

            let nav_handler = NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(a) = args {
                    let mut uri_raw = PWSTR(std::ptr::null_mut());
                    let allowed = if a.Uri(&mut uri_raw).is_ok() {
                        let uri = take_pwstr(uri_raw);
                        uri.eq_ignore_ascii_case("about:blank") || uri.starts_with("data:text/html")
                    } else {
                        false
                    };
                    if !allowed {
                        let _ = a.SetCancel(true);
                    }
                }
                Ok(())
            }));
            let mut nav_tok = 0_i64;
            let _ = wv.add_NavigationStarting(&nav_handler, &mut nav_tok);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_video_sta(
        owner_hwnd_raw: isize,
        rect: VideoViewportRect,
        profile_dir: PathBuf,
        initial_url: String,
        initial_pos: f64,
        initial_playing: bool,
        initially_visible: bool,
        cmd_rx: Receiver<VideoCmd>,
        hwnd_tx: Sender<isize>,
        shared_hwnd: Arc<AtomicIsize>,
    ) -> Result<(), String> {
        let _ = std::fs::create_dir_all(&profile_dir);

        // SAFETY: Initialize STA COM apartment dedicated to this WebView2 video surface.
        let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        let _com_guard = com_ok.then_some(ComGuard);

        let hinstance =
            // SAFETY: Current process module handle.
            unsafe { GetModuleHandleW(None).ok().map(|h| HINSTANCE(h.0)) };

        let class_name = w!("FerricNativeVideoStageWnd");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(video_wnd_proc),
            hInstance: hinstance.unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        // SAFETY: Registering window class (duplicate calls safely ignored).
        unsafe {
            let _ = RegisterClassW(&wc);
        }

        let owner = if owner_hwnd_raw != 0 {
            Some(HWND(owner_hwnd_raw as *mut c_void))
        } else {
            None
        };

        // SAFETY: Borderless owned tool popup docked inside Ferric's viewport (no extra taskbar entry).
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("Ferric Video"),
                WS_POPUP | WS_CLIPSIBLINGS,
                rect.x,
                rect.y,
                rect.width.max(160),
                rect.height.max(90),
                owner,
                None,
                hinstance,
                None,
            )
        }
        .map_err(|e| format!("Video window create failed: {e}"))?;

        shared_hwnd.store(hwnd.0 as isize, Ordering::Release);
        let _ = hwnd_tx.send(hwnd.0 as isize);

        // SAFETY: Position docked stage cleanly; only show immediately when `initially_visible` is true
        // so Cover -> Video morph animations can pre-buffer silently without popping at an intermediate rect.
        let init_w = rect.width.max(160);
        let init_h = rect.height.max(90);
        let init_rect = VideoViewportRect {
            x: rect.x,
            y: rect.y,
            width: init_w,
            height: init_h,
        };
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                init_rect.x,
                init_rect.y,
                init_w,
                init_h,
                SWP_NOACTIVATE,
            );
            apply_rounded_video_corners(hwnd, init_w, init_h);
            if initially_visible {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = UpdateWindow(hwnd);
            }
        }

        let cleanup_hwnd = |h: HWND, slot: &Arc<AtomicIsize>| {
            slot.store(0, Ordering::Release);
            // SAFETY: Destroy window if WebView2 init fails before state is attached.
            unsafe {
                let _ = DestroyWindow(h);
            }
        };

        let environment = {
            let (tx, rx) = mpsc::channel();
            let ud = profile_dir.to_string_lossy().into_owned();
            if let Err(e) = CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
                Box::new(move |handler| {
                    let ud_w = CoTaskMemPWSTR::from(ud.as_str());
                    // SAFETY: `ud_w` stays valid for the async WebView2 environment creation call.
                    unsafe {
                        CreateCoreWebView2EnvironmentWithOptions(
                            PCWSTR::null(),
                            *ud_w.as_ref().as_pcwstr(),
                            None::<&ICoreWebView2EnvironmentOptions>,
                            &handler,
                        )
                        .map_err(webview2_com::Error::WindowsError)
                    }
                }),
                Box::new(move |err, env| {
                    err?;
                    let _ = tx.send(env.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                    Ok(())
                }),
            ) {
                cleanup_hwnd(hwnd, &shared_hwnd);
                return Err(format!("WebView2 video init failed: {e}"));
            }
            match rx.recv() {
                Ok(Ok(env)) => env,
                Ok(Err(e)) => {
                    cleanup_hwnd(hwnd, &shared_hwnd);
                    return Err(format!("WebView2 env error: {e}"));
                }
                Err(_) => {
                    cleanup_hwnd(hwnd, &shared_hwnd);
                    return Err("WebView2 env channel closed".to_string());
                }
            }
        };

        let controller = {
            let (tx, rx) = mpsc::channel();
            if let Err(e) = CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
                Box::new(move |handler| {
                    // SAFETY: Attach controller to STA-owned borderless video HWND.
                    unsafe {
                        environment
                            .CreateCoreWebView2Controller(hwnd, &handler)
                            .map_err(webview2_com::Error::WindowsError)
                    }
                }),
                Box::new(move |err, ctrl| {
                    err?;
                    let _ = tx.send(ctrl.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                    Ok(())
                }),
            ) {
                cleanup_hwnd(hwnd, &shared_hwnd);
                return Err(format!("WebView2 video controller failed: {e}"));
            }
            match rx.recv() {
                Ok(Ok(ctrl)) => ctrl,
                Ok(Err(e)) => {
                    cleanup_hwnd(hwnd, &shared_hwnd);
                    return Err(format!("WebView2 controller error: {e}"));
                }
                Err(_) => {
                    cleanup_hwnd(hwnd, &shared_hwnd);
                    return Err("WebView2 controller channel closed".to_string());
                }
            }
        };

        // SAFETY: Fit WebView2 surface to video viewport, harden settings/handlers, and load shell HTML.
        let webview = unsafe {
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let _ = controller.SetBounds(client);
            let _ = controller.SetIsVisible(initially_visible);
            let wv = match controller.CoreWebView2() {
                Ok(wv) => wv,
                Err(e) => {
                    let _ = controller.Close();
                    cleanup_hwnd(hwnd, &shared_hwnd);
                    return Err(format!("CoreWebView2 unavailable: {e}"));
                }
            };
            harden_video_webview(&wv);
            let html = player_shell_html(
                &initial_url,
                initial_pos,
                initial_playing,
                initially_visible,
            );
            let html_w = CoTaskMemPWSTR::from(html.as_str());
            let _ = wv.NavigateToString(*html_w.as_ref().as_pcwstr());
            wv
        };

        let state = Rc::new(VideoHostState {
            controller,
            webview,
            cmd_rx: RefCell::new(cmd_rx),
            shared_hwnd,
            is_visible: Cell::new(initially_visible),
            last_bounds: Cell::new(init_rect),
        });

        // Guard against the host HWND being closed during async WebView2 initialization.
        if !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(hwnd)).as_bool() } {
            unsafe {
                let _ = state.controller.Close();
            }
            return Ok(());
        }

        // SAFETY: Attach Rc pointer (`Rc::into_raw`) released cleanly once in `WM_DESTROY`.
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                Rc::into_raw(Rc::clone(&state)) as isize,
            );
            let _ = SetTimer(Some(hwnd), CMD_POLL_TIMER, 120, None);
        }
        // Flush any bounds/visibility updates queued while WebView2 was initializing with zero timer lag.
        drain_video_commands(hwnd, &state);
        drop(state);

        let mut msg = MSG::default();
        loop {
            // SAFETY: STA message loop until WM_DESTROY posts WM_QUIT.
            let res = unsafe { GetMessageW(&mut msg, None, 0, 0).0 };
            if res <= 0 {
                break;
            }
            // SAFETY: Dispatch Win32 & COM messages.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        Ok(())
    }

    fn drain_video_commands(hwnd: HWND, st: &Rc<VideoHostState>) {
        // Drain pending commands into a local Vec and drop the RefCell borrow BEFORE any
        // Win32/COM call so re-entrant WM_SIZE / WM_DESTROY can never alias or panic.
        let pending: Vec<VideoCmd> = {
            let Ok(rx) = st.cmd_rx.try_borrow() else {
                return;
            };
            let mut batch = Vec::new();
            while let Ok(cmd) = rx.try_recv() {
                batch.push(cmd);
            }
            batch
        };

        for cmd in pending {
            match cmd {
                VideoCmd::LoadStream {
                    url,
                    pos_sec,
                    playing,
                } => {
                    if !is_allowed_video_stream_url(&url) {
                        continue;
                    }
                    let q = js_quote_safe(&url);
                    let pos = safe_pos_secs(pos_sec);
                    let p_flag = if playing { "true" } else { "false" };
                    let script = format!(
                        "(function r(n){{if(window.__ferricLoad){{window.__ferricLoad({q},{pos:.3},{p_flag});}}else if(n>0){{setTimeout(()=>r(n-1),120);}}}})(30);"
                    );
                    let w_script = CoTaskMemPWSTR::from(script.as_str());
                    // SAFETY: STA thread WebView2 script execution; `st: &Rc` keeps allocation alive.
                    unsafe {
                        let _ = st
                            .webview
                            .ExecuteScript(*w_script.as_ref().as_pcwstr(), None);
                    }
                }
                VideoCmd::SyncPlayback { pos_sec, playing } => {
                    let pos = safe_pos_secs(pos_sec);
                    let script = format!(
                        "window.__ferricSync && window.__ferricSync({pos:.3}, {});",
                        if playing { "true" } else { "false" }
                    );
                    let w_script = CoTaskMemPWSTR::from(script.as_str());
                    // SAFETY: STA thread WebView2 script execution; `st: &Rc` keeps allocation alive.
                    unsafe {
                        let _ = st
                            .webview
                            .ExecuteScript(*w_script.as_ref().as_pcwstr(), None);
                    }
                }
                VideoCmd::SetBounds(r) => {
                    let clamped = VideoViewportRect {
                        x: r.x,
                        y: r.y,
                        width: r.width.max(120),
                        height: r.height.max(80),
                    };
                    if st.last_bounds.get() == clamped {
                        continue;
                    }
                    st.last_bounds.set(clamped);
                    // SAFETY: Keep docked video surface locked to Ferric's viewport rect with smooth rounded corners.
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            clamped.x,
                            clamped.y,
                            clamped.width,
                            clamped.height,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        apply_rounded_video_corners(hwnd, clamped.width, clamped.height);
                        let mut client = RECT::default();
                        let _ = GetClientRect(hwnd, &mut client);
                        let _ = st.controller.SetBounds(client);
                    }
                }
                VideoCmd::SetVisible(vis) => {
                    if st.is_visible.get() == vis {
                        continue;
                    }
                    st.is_visible.set(vis);
                    let script = if vis {
                        "(function r(n){if(window.__ferricStageVisible){window.__ferricStageVisible(true);}else if(n>0){setTimeout(()=>r(n-1),40);}})(25);"
                    } else {
                        "window.__ferricStageVisible && window.__ferricStageVisible(false);"
                    };
                    let w_script = CoTaskMemPWSTR::from(script);
                    // SAFETY: Show/hide docked video surface cleanly with smooth opacity reveal on entry.
                    unsafe {
                        let _ = st.controller.SetIsVisible(vis);
                        let _ = ShowWindow(hwnd, if vis { SW_SHOWNOACTIVATE } else { SW_HIDE });
                        let _ = st
                            .webview
                            .ExecuteScript(*w_script.as_ref().as_pcwstr(), None);
                    }
                }
                VideoCmd::Close => {
                    // SAFETY: Post WM_CLOSE asynchronously so teardown never re-enters mid-loop.
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return;
                }
            }
        }
    }

    unsafe fn clone_host_state(hwnd: HWND) -> Option<Rc<VideoHostState>> {
        // SAFETY: `GWLP_USERDATA` stores `Rc::into_raw` on this STA thread until `WM_DESTROY` clears it.
        unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const VideoHostState;
            if ptr.is_null() {
                return None;
            }
            Rc::increment_strong_count(ptr);
            Some(Rc::from_raw(ptr))
        }
    }

    unsafe extern "system" fn video_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: Valid Win32 message dispatch on video STA window.
        unsafe {
            match msg {
                WM_SIZE => {
                    if let Some(st) = clone_host_state(hwnd) {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        apply_rounded_video_corners(hwnd, rc.right - rc.left, rc.bottom - rc.top);
                        let _ = st.controller.SetBounds(rc);
                    }
                    LRESULT(0)
                }
                WM_WAKE_VIDEO_CMD | WM_TIMER => {
                    if let Some(st) = clone_host_state(hwnd) {
                        drain_video_commands(hwnd, &st);
                    }
                    LRESULT(0)
                }
                WM_CLOSE => {
                    if let Some(st) = clone_host_state(hwnd) {
                        let _ = st.controller.Close();
                    }
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    let _ = KillTimer(Some(hwnd), CMD_POLL_TIMER);
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const VideoHostState;
                    if !ptr.is_null() {
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                        let st = Rc::from_raw(ptr);
                        st.shared_hwnd.store(0, Ordering::Release);
                        let _ = st.controller.Close();
                        drop(st);
                    }
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::VideoViewportRect;
    use std::path::PathBuf;

    pub struct NativeVideoPlayer;
    impl NativeVideoPlayer {
        pub fn new(_profile_dir: PathBuf) -> Self {
            Self
        }
        pub fn is_open(&self) -> bool {
            false
        }
        pub fn open_or_load(
            &mut self,
            _owner_hwnd_raw: isize,
            _rect: VideoViewportRect,
            _stream_url: String,
            _pos_sec: f64,
            _playing: bool,
            _initially_visible: bool,
        ) {
        }
        pub fn set_bounds(&self, _rect: VideoViewportRect) {}
        pub fn set_visible(&self, _visible: bool) {}
        pub fn sync_playback(&self, _pos_sec: f64, _playing: bool) {}
        pub fn close(&mut self) {}
    }
}

pub use imp::NativeVideoPlayer;
