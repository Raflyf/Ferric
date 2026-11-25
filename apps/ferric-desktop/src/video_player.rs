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
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
        ICoreWebView2EnvironmentOptions,
    };
    use webview2_com::{
        CoTaskMemPWSTR, CreateCoreWebView2ControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler,
    };
    use windows::Win32::Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::UpdateWindow;
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
        GetClientRect, GetMessageW, HWND_TOP, KillTimer, MSG, PostMessageW, PostQuitMessage,
        RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetTimer,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WM_APP, WM_CLOSE,
        WM_DESTROY, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };
    use windows::core::{PCWSTR, w};

    const WM_WAKE_VIDEO_CMD: u32 = WM_APP + 77;
    const CMD_POLL_TIMER: usize = 2001;

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
        cmd_rx: Receiver<VideoCmd>,
    }

    pub struct NativeVideoPlayer {
        cmd_tx: Option<Sender<VideoCmd>>,
        host_hwnd_raw: Rc<RefCell<isize>>,
        profile_dir: PathBuf,
    }

    impl NativeVideoPlayer {
        pub fn new(profile_dir: PathBuf) -> Self {
            Self {
                cmd_tx: None,
                host_hwnd_raw: Rc::new(RefCell::new(0)),
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
        ) {
            if let Some(tx) = &self.cmd_tx {
                let _ = tx.send(VideoCmd::SetBounds(rect));
                let _ = tx.send(VideoCmd::SetVisible(true));
                let _ = tx.send(VideoCmd::LoadStream {
                    url: stream_url,
                    pos_sec,
                    playing,
                });
                self.wake_thread();
                return;
            }

            let (tx, rx) = mpsc::channel::<VideoCmd>();
            self.cmd_tx = Some(tx);

            let (hwnd_tx, hwnd_rx) = mpsc::channel::<isize>();
            let profile_dir = self.profile_dir.clone();
            let _ = thread::Builder::new()
                .name("ferric-video-stage".to_string())
                .spawn(move || {
                    let _ = run_video_sta(
                        owner_hwnd_raw,
                        rect,
                        profile_dir,
                        stream_url,
                        pos_sec,
                        playing,
                        rx,
                        hwnd_tx,
                    );
                });
            if let Ok(raw) = hwnd_rx.recv_timeout(std::time::Duration::from_millis(600)) {
                *self.host_hwnd_raw.borrow_mut() = raw;
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
                let _ = tx.send(VideoCmd::SyncPlayback { pos_sec, playing });
                self.wake_thread();
            }
        }

        pub fn close(&mut self) {
            if let Some(tx) = self.cmd_tx.take() {
                let _ = tx.send(VideoCmd::Close);
                self.wake_thread();
            }
            *self.host_hwnd_raw.borrow_mut() = 0;
        }

        fn wake_thread(&self) {
            let raw = *self.host_hwnd_raw.borrow();
            if raw != 0 {
                // SAFETY: Non-blocking wake message posted to our STA video window.
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

    fn player_shell_html(initial_url: &str, initial_pos: f64, initial_playing: bool) -> String {
        let quoted_url = js_quote(initial_url);
        let init_play = if initial_playing { "true" } else { "false" };
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="referrer" content="no-referrer">
<style>
  html, body {{
    margin: 0; padding: 0; width: 100%; height: 100%;
    background: #000000; overflow: hidden;
    display: flex; align-items: center; justify-content: center;
    user-select: none;
  }}
  video {{
    width: 100%; height: 100%; object-fit: contain;
    background: #000000; opacity: 0;
    transition: opacity 220ms ease-out;
  }}
  video.ready {{ opacity: 1; }}
  #msg {{
    position: absolute; color: #9A9CA8; font: 500 13px/1.4 "Segoe UI", sans-serif;
    letter-spacing: 0.2px; pointer-events: none;
  }}
</style>
</head>
<body>
  <div id="msg">Loading video stream...</div>
  <video id="v" muted playsinline autoplay preload="auto"></video>
  <script>
    const v = document.getElementById('v');
    const msg = document.getElementById('msg');
    let desiredPos = {initial_pos:.3};
    let desiredPlaying = {init_play};
    const reveal = () => {{
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
      desiredPos = pos || 0;
      desiredPlaying = !!playing;
      v.classList.remove('ready');
      msg.style.display = 'block';
      msg.textContent = 'Switching stream...';
      v.src = url;
      v.load();
      if (desiredPlaying) {{ v.play().catch(() => {{}}); }}
    }};
    window.__ferricSync = (pos, playing) => {{
      desiredPos = pos || 0;
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
    window.__ferricLoad({quoted_url}, {initial_pos:.3}, {init_play});
  </script>
</body>
</html>"#
        )
    }

    fn js_quote(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    fn run_video_sta(
        owner_hwnd_raw: isize,
        rect: VideoViewportRect,
        profile_dir: PathBuf,
        initial_url: String,
        initial_pos: f64,
        initial_playing: bool,
        cmd_rx: Receiver<VideoCmd>,
        hwnd_tx: Sender<isize>,
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

        let _ = hwnd_tx.send(hwnd.0 as isize);

        // SAFETY: Show docked stage directly on top of Ferric's viewport without stealing keyboard focus.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                rect.x,
                rect.y,
                rect.width.max(160),
                rect.height.max(90),
                SWP_NOACTIVATE,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
        }

        let environment = {
            let (tx, rx) = mpsc::channel();
            let ud = profile_dir.to_string_lossy().into_owned();
            CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
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
            )
            .map_err(|e| {
                // SAFETY: Destroy window if WebView2 init fails.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                format!("WebView2 video init failed: {e}")
            })?;
            rx.recv()
                .map_err(|_| "WebView2 env channel closed".to_string())?
                .map_err(|e| format!("WebView2 env error: {e}"))?
        };

        let controller = {
            let (tx, rx) = mpsc::channel();
            CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
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
            )
            .map_err(|e| {
                // SAFETY: Cleanup window on failure.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                format!("WebView2 video controller failed: {e}")
            })?;
            rx.recv()
                .map_err(|_| "WebView2 controller channel closed".to_string())?
                .map_err(|e| format!("WebView2 controller error: {e}"))?
        };

        // SAFETY: Fit WebView2 surface to video viewport and load minimalist HTML5 video shell
        // with the initial stream baked directly in (zero NavigateToString/ExecuteScript race).
        let webview = unsafe {
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let _ = controller.SetBounds(client);
            let _ = controller.SetIsVisible(true);
            let wv = controller
                .CoreWebView2()
                .map_err(|e| format!("CoreWebView2 unavailable: {e}"))?;
            let html = player_shell_html(&initial_url, initial_pos, initial_playing);
            let html_w = CoTaskMemPWSTR::from(html.as_str());
            let _ = wv.NavigateToString(*html_w.as_ref().as_pcwstr());
            wv
        };

        let state = Box::new(VideoHostState {
            controller,
            webview,
            cmd_rx,
        });

        // SAFETY: Attach state pointer and start lightweight 120 ms command drain timer.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
            let _ = SetTimer(Some(hwnd), CMD_POLL_TIMER, 120, None);
        }

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

    fn drain_video_commands(hwnd: HWND, st: &mut VideoHostState) {
        while let Ok(cmd) = st.cmd_rx.try_recv() {
            match cmd {
                VideoCmd::LoadStream {
                    url,
                    pos_sec,
                    playing,
                } => {
                    let q = js_quote(&url);
                    let p_flag = if playing { "true" } else { "false" };
                    let script = format!(
                        "(function r(n){{if(window.__ferricLoad){{window.__ferricLoad({q},{pos_sec:.3},{p_flag});}}else if(n>0){{setTimeout(()=>r(n-1),120);}}}})(30);"
                    );
                    let w_script = CoTaskMemPWSTR::from(script.as_str());
                    // SAFETY: STA thread WebView2 script execution.
                    unsafe {
                        let _ = st
                            .webview
                            .ExecuteScript(*w_script.as_ref().as_pcwstr(), None);
                    }
                }
                VideoCmd::SyncPlayback { pos_sec, playing } => {
                    let script = format!(
                        "window.__ferricSync && window.__ferricSync({:.3}, {});",
                        pos_sec,
                        if playing { "true" } else { "false" }
                    );
                    let w_script = CoTaskMemPWSTR::from(script.as_str());
                    // SAFETY: STA thread WebView2 script execution.
                    unsafe {
                        let _ = st
                            .webview
                            .ExecuteScript(*w_script.as_ref().as_pcwstr(), None);
                    }
                }
                VideoCmd::SetBounds(r) => {
                    // SAFETY: Keep docked video surface locked to Ferric's viewport rect.
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            None,
                            r.x,
                            r.y,
                            r.width.max(120),
                            r.height.max(80),
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        let mut client = RECT::default();
                        let _ = GetClientRect(hwnd, &mut client);
                        let _ = st.controller.SetBounds(client);
                    }
                }
                VideoCmd::SetVisible(vis) => {
                    // SAFETY: Show/hide docked video surface cleanly.
                    unsafe {
                        let _ = st.controller.SetIsVisible(vis);
                        let _ = ShowWindow(hwnd, if vis { SW_SHOWNOACTIVATE } else { SW_HIDE });
                    }
                }
                VideoCmd::Close => {
                    // SAFETY: Tear down WebView2 controller and destroy window completely to reclaim 100% RAM.
                    unsafe {
                        let _ = st.controller.Close();
                        let _ = DestroyWindow(hwnd);
                    }
                    return;
                }
            }
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
            let ptr =
                windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
                    as *mut VideoHostState;
            match msg {
                WM_SIZE => {
                    if !ptr.is_null() {
                        let mut rc = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rc);
                        let _ = (*ptr).controller.SetBounds(rc);
                    }
                    LRESULT(0)
                }
                WM_WAKE_VIDEO_CMD | WM_TIMER => {
                    if !ptr.is_null() {
                        drain_video_commands(hwnd, &mut *ptr);
                    }
                    LRESULT(0)
                }
                WM_CLOSE => {
                    if !ptr.is_null() {
                        let _ = (*ptr).controller.Close();
                    }
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    let _ = KillTimer(Some(hwnd), CMD_POLL_TIMER);
                    if !ptr.is_null() {
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                        drop(Box::from_raw(ptr));
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
        ) {
        }
        pub fn set_bounds(&self, _rect: VideoViewportRect) {}
        pub fn set_visible(&self, _visible: bool) {}
        pub fn sync_playback(&self, _pos_sec: f64, _playing: bool) {}
        pub fn close(&mut self) {}
    }
}

pub use imp::NativeVideoPlayer;
