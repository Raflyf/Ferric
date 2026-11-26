//! Native 1-Click Google / YouTube Music Sign-In window using Windows Edge WebView2.
//! Zero-Developer-Console sign-in flow: opens Google's official YouTube Music
//! login page, automatically captures `SAPISID` / `__Secure-3PAPISID` session cookies once
//! redirected to `music.youtube.com`, and closes the window cleanly.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum GoogleLoginOutcome {
    SignedIn { cookie: String, sapisid: String },
    Cancelled,
    Failed(String),
}

/// Result of the Spotify web-session sign-in window.
#[derive(Debug, Clone)]
pub enum SpotifyLoginOutcome {
    /// `sp_dc` session cookie plus the access-token JSON fetched inside the signed-in page.
    SignedIn {
        sp_dc: String,
        token_json: String,
    },
    Cancelled,
    Failed(String),
}

/// Which service the shared WebView2 login window is driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoginTarget {
    GoogleYtm,
    Spotify,
}

/// Internal payload handed from the login window thread back to the caller.
pub(crate) enum LoginPayload {
    Cookie(String),
    /// Spotify only: captured `sp_dc` cookie plus the token JSON fetched from inside the
    /// signed-in page context.
    SpotifySession {
        cookie: String,
        token_json: String,
    },
    Cancelled,
    Failed(String),
}

/// Launch the interactive Google sign-in window on a dedicated STA thread so the main UI stays fluid.
pub fn launch_google_ytm_login_window<F>(
    profile_dir: PathBuf,
    clear_existing_cookies: bool,
    on_done: F,
) where
    F: FnOnce(GoogleLoginOutcome) + Send + 'static,
{
    imp::launch(profile_dir, clear_existing_cookies, on_done);
}

/// Launch the Spotify web-player sign-in window and capture the signed-in session token.
///
/// This path needs no Client ID and no developer app: the user signs in to Spotify's own web
/// player, and the access token that the player fetches for itself is captured from the page.
pub fn launch_spotify_login_window<F>(
    profile_dir: PathBuf,
    clear_existing_cookies: bool,
    on_done: F,
) where
    F: FnOnce(SpotifyLoginOutcome) + Send + 'static,
{
    imp::launch_spotify(profile_dir, clear_existing_cookies, on_done);
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{LoginPayload, LoginTarget, SpotifyLoginOutcome};
    use ferric_providers::spotify_web::extract_sp_dc;
    use ferric_providers::ytmusic::extract_sapisid_from_cookie;
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::thread;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_STATE_DENY, CreateCoreWebView2EnvironmentWithOptions,
        ICoreWebView2, ICoreWebView2_2, ICoreWebView2Controller, ICoreWebView2EnvironmentOptions,
        ICoreWebView2Settings3, ICoreWebView2Settings4,
    };
    use webview2_com::{
        AddScriptToExecuteOnDocumentCreatedCompletedHandler, CoTaskMemPWSTR,
        CreateCoreWebView2ControllerCompletedHandler,
        CreateCoreWebView2EnvironmentCompletedHandler, ExecuteScriptCompletedHandler,
        GetCookiesCompletedHandler, NavigationCompletedEventHandler,
        NavigationStartingEventHandler, NewWindowRequestedEventHandler,
        PermissionRequestedEventHandler, SourceChangedEventHandler, take_pwstr,
    };
    use windows::Win32::Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::UpdateWindow;
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
        GetClientRect, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, KillTimer, MSG,
        PostMessageW, PostQuitMessage, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SetTimer,
        SetWindowLongPtrW, ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_SIZE, WM_TIMER,
        WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::{Interface, PCWSTR, PWSTR, w};

    const GOOGLE_YTM_LOGIN_URL: &str = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin%3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F";
    const YTM_COOKIE_URI: &str = "https://music.youtube.com";
    const YTM_COOKIE_HOST: &str = "music.youtube.com";
    const SPOTIFY_LOGIN_URL: &str =
        "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F";
    const SPOTIFY_COOKIE_URI: &str = "https://open.spotify.com";
    const SPOTIFY_COOKIE_HOST: &str = "open.spotify.com";
    const COOKIE_POLL_TIMER_ID: usize = 1001;

    /// Installed on every Spotify document before the first navigation. The web player computes
    /// its own per-session token challenge inside obfuscated JS; re-issuing the endpoint call from
    /// outside that code path returns HTML. Hooking the player's own `fetch`/XHR responses keeps
    /// the challenge where Spotify expects it in the request and only records the resulting JSON.
    const SPOTIFY_TOKEN_HOOK_JS: &str = r#"
(() => {
  if (window.__ferricTokenHook) return;
  window.__ferricTokenHook = true;
  window.__ferricToken = null;
  const isTokenUrl = (u) => {
    try { return /(\/api\/token|get_access_token)/.test(String(u)); } catch (e) { return false; }
  };
  const remember = (text) => {
    if (typeof text !== 'string' || text.length > 65536) return;
    if (text.indexOf('accessToken') === -1) return;
    try {
      const parsed = JSON.parse(text);
      if (!parsed || !parsed.accessToken || parsed.isAnonymous === true) return;
      window.__ferricToken = text;
    } catch (e) {}
  };
  const originalFetch = window.fetch;
  if (originalFetch) {
    window.fetch = function (input, init) {
      const url = (input && input.url) || input;
      const pending = originalFetch.apply(this, arguments);
      if (isTokenUrl(url)) {
        pending.then((res) => res.clone().text()).then(remember).catch(() => {});
      }
      return pending;
    };
  }
  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url) {
    if (isTokenUrl(url)) {
      this.addEventListener('load', () => {
        try { remember(this.responseText); } catch (e) {}
      });
    }
    return originalOpen.apply(this, arguments);
  };
})();
"#;

    /// Reads the token the player fetched for itself. The direct `get_access_token` call is kept
    /// only as a fallback for builds where the player has not issued the request within 10 s.
    const SPOTIFY_TOKEN_READ_JS: &str = r#"
(async () => {
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  for (let i = 0; i < 20; i += 1) {
    if (window.__ferricToken) return window.__ferricToken;
    await wait(500);
  }
  try {
    const res = await fetch('/get_access_token?reason=transport&productType=web_player', {
      credentials: 'include',
      headers: { 'app-platform': 'WebPlayer' },
    });
    const text = await res.text();
    if (text && text.indexOf('accessToken') !== -1) return text;
  } catch (e) {
    if (window.__ferricToken) return window.__ferricToken;
  }
  for (let i = 0; i < 20; i += 1) {
    if (window.__ferricToken) return window.__ferricToken;
    await wait(500);
  }
  return window.__ferricToken || 'ERR:no token captured from the web player';
})()
"#;

    impl LoginTarget {
        const fn login_url(self) -> &'static str {
            match self {
                Self::GoogleYtm => GOOGLE_YTM_LOGIN_URL,
                Self::Spotify => SPOTIFY_LOGIN_URL,
            }
        }

        const fn cookie_uri(self) -> &'static str {
            match self {
                Self::GoogleYtm => YTM_COOKIE_URI,
                Self::Spotify => SPOTIFY_COOKIE_URI,
            }
        }

        const fn cookie_host(self) -> &'static str {
            match self {
                Self::GoogleYtm => YTM_COOKIE_HOST,
                Self::Spotify => SPOTIFY_COOKIE_HOST,
            }
        }

        const fn thread_name(self) -> &'static str {
            match self {
                Self::GoogleYtm => "ferric-google-login",
                Self::Spotify => "ferric-spotify-login",
            }
        }
    }

    static LOGIN_WINDOW_OPEN: AtomicBool = AtomicBool::new(false);

    type DoneSlot = Rc<RefCell<Option<Box<dyn FnOnce(LoginPayload) + Send>>>>;

    struct OpenFlagGuard;
    impl Drop for OpenFlagGuard {
        fn drop(&mut self) {
            LOGIN_WINDOW_OPEN.store(false, Ordering::SeqCst);
        }
    }

    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            // SAFETY: Matches successful `CoInitializeEx` on this thread.
            unsafe { CoUninitialize() };
        }
    }

    struct LoginWindowState {
        controller: ICoreWebView2Controller,
        webview: ICoreWebView2,
        done_cb: DoneSlot,
        capture_in_flight: Rc<Cell<bool>>,
        destroyed: Rc<Cell<bool>>,
        target: LoginTarget,
    }

    fn emit_once(slot: &DoneSlot, outcome: LoginPayload) {
        if let Some(cb) = slot.borrow_mut().take() {
            cb(outcome);
        }
    }

    fn is_exact_https_host(url: &str, expected_host: &str) -> bool {
        if url.bytes().any(|b| b.is_ascii_control() || b == b' ') {
            return false;
        }
        let Some(rest) = url.strip_prefix("https://") else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        authority.eq_ignore_ascii_case(expected_host)
    }

    fn is_safe_login_navigation_url(url: &str) -> bool {
        if url.eq_ignore_ascii_case("about:blank") {
            return true;
        }
        if url.bytes().any(|b| b.is_ascii_control() || b == b' ') {
            return false;
        }
        let Some(rest) = url.strip_prefix("https://") else {
            return false;
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') {
            return false;
        }
        let host = authority
            .split(':')
            .next()
            .unwrap_or("")
            .trim_matches(|c| c == '[' || c == ']')
            .to_ascii_lowercase();
        if host.is_empty() || host == "localhost" || host.ends_with(".local") {
            return false;
        }
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            return match ip {
                std::net::IpAddr::V4(v4) => {
                    !v4.is_loopback()
                        && !v4.is_private()
                        && !v4.is_link_local()
                        && !v4.is_unspecified()
                }
                std::net::IpAddr::V6(v6) => {
                    !v6.is_loopback()
                        && !v6.is_unspecified()
                        && (v6.segments()[0] & 0xffc0) != 0xfe80
                        && (v6.segments()[0] & 0xfe00) != 0xfc00
                }
            };
        }
        true
    }

    pub fn launch<F>(profile_dir: PathBuf, clear_existing_cookies: bool, on_done: F)
    where
        F: FnOnce(super::GoogleLoginOutcome) + Send + 'static,
    {
        launch_for(
            LoginTarget::GoogleYtm,
            profile_dir,
            clear_existing_cookies,
            Box::new(move |payload| {
                let outcome = match payload {
                    LoginPayload::Cookie(cookie) => match extract_sapisid_from_cookie(&cookie) {
                        Some(sapisid) => super::GoogleLoginOutcome::SignedIn { cookie, sapisid },
                        None => super::GoogleLoginOutcome::Failed(
                            "Google cookies captured but SAPISID was missing".to_string(),
                        ),
                    },
                    LoginPayload::Cancelled => super::GoogleLoginOutcome::Cancelled,
                    LoginPayload::Failed(err) => super::GoogleLoginOutcome::Failed(err),
                    LoginPayload::SpotifySession { .. } => super::GoogleLoginOutcome::Failed(
                        "Unexpected Spotify session payload in the Google flow".to_string(),
                    ),
                };
                on_done(outcome);
            }),
        );
    }

    pub fn launch_spotify<F>(profile_dir: PathBuf, clear_existing_cookies: bool, on_done: F)
    where
        F: FnOnce(SpotifyLoginOutcome) + Send + 'static,
    {
        launch_for(
            LoginTarget::Spotify,
            profile_dir,
            clear_existing_cookies,
            Box::new(move |payload| {
                let outcome = match payload {
                    LoginPayload::SpotifySession { cookie, token_json } => {
                        match extract_sp_dc(&cookie) {
                            Some(sp_dc) => SpotifyLoginOutcome::SignedIn { sp_dc, token_json },
                            None => SpotifyLoginOutcome::Failed(
                                "Spotify cookies captured but sp_dc was missing".to_string(),
                            ),
                        }
                    }
                    LoginPayload::Cookie(_) => SpotifyLoginOutcome::Failed(
                        "Spotify session did not return an access token".to_string(),
                    ),
                    LoginPayload::Cancelled => SpotifyLoginOutcome::Cancelled,
                    LoginPayload::Failed(err) => SpotifyLoginOutcome::Failed(err),
                };
                on_done(outcome);
            }),
        );
    }

    fn launch_for(
        target: LoginTarget,
        profile_dir: PathBuf,
        clear_existing_cookies: bool,
        on_done: Box<dyn FnOnce(LoginPayload) + Send>,
    ) {
        if LOGIN_WINDOW_OPEN
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            on_done(LoginPayload::Failed(
                "A sign-in window is already open.".to_string(),
            ));
            return;
        }

        let spawn_res = thread::Builder::new()
            .name(target.thread_name().to_string())
            .spawn(move || {
                let _open_guard = OpenFlagGuard;
                let done_cb: DoneSlot = Rc::new(RefCell::new(Some(on_done)));
                if let Err(err) = run_login_window(
                    &profile_dir,
                    clear_existing_cookies,
                    target,
                    Rc::clone(&done_cb),
                ) {
                    emit_once(&done_cb, LoginPayload::Failed(err));
                } else {
                    emit_once(&done_cb, LoginPayload::Cancelled);
                }
            });

        if let Err(err) = spawn_res {
            LOGIN_WINDOW_OPEN.store(false, Ordering::SeqCst);
            let _ = err;
        }
    }

    unsafe fn harden_login_webview(wv: &ICoreWebView2) {
        // SAFETY: STA-thread COM settings & handlers on our login WebView2 instance.
        unsafe {
            if let Ok(settings) = wv.Settings() {
                let _ = settings.SetAreDevToolsEnabled(false);
                let _ = settings.SetAreDefaultContextMenusEnabled(false);
                let _ = settings.SetIsStatusBarEnabled(false);
                let _ = settings.SetAreHostObjectsAllowed(false);
                let _ = settings.SetIsWebMessageEnabled(false);
                if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                    let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                }
                if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                    let _ = s4.SetIsPasswordAutosaveEnabled(false);
                    let _ = s4.SetIsGeneralAutofillEnabled(false);
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

            let nav_start_handler = NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(a) = args {
                    let mut uri_raw = PWSTR(ptr::null_mut());
                    let allowed = if a.Uri(&mut uri_raw).is_ok() {
                        let uri = take_pwstr(uri_raw);
                        is_safe_login_navigation_url(&uri)
                    } else {
                        false
                    };
                    if !allowed {
                        let _ = a.SetCancel(true);
                    }
                }
                Ok(())
            }));
            let mut ns_tok = 0_i64;
            let _ = wv.add_NavigationStarting(&nav_start_handler, &mut ns_tok);
        }
    }

    fn run_login_window(
        profile_dir: &Path,
        clear_existing_cookies: bool,
        target: LoginTarget,
        done_cb: DoneSlot,
    ) -> Result<(), String> {
        let _ = std::fs::create_dir_all(profile_dir);

        // SAFETY: Initialize single-threaded COM apartment dedicated to this WebView2 UI thread.
        let com_ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        let _com_guard = com_ok.then_some(ComGuard);

        let hinstance =
            // SAFETY: Query current process module handle for Win32 window registration.
            unsafe { GetModuleHandleW(None).ok().map(|h| HINSTANCE(h.0)) };

        let class_name = w!("FerricGoogleSignInWnd");
        let wnd_class = WNDCLASSW {
            lpfnWndProc: Some(login_wnd_proc),
            hInstance: hinstance.unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        // SAFETY: Registering window class (subsequent registrations safely return 0).
        unsafe {
            let _ = RegisterClassW(&wnd_class);
        }

        let win_w = 540_i32;
        let win_h = 740_i32;
        // SAFETY: Read primary screen dimensions to center the sign-in dialog.
        let (x, y) = unsafe {
            let sw = GetSystemMetrics(SM_CXSCREEN).max(win_w);
            let sh = GetSystemMetrics(SM_CYSCREEN).max(win_h);
            ((sw - win_w) / 2, (sh - win_h) / 2)
        };

        // SAFETY: Standard top-level overlapped window owned by this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                class_name,
                w!("Sign in to YouTube Music — Ferric"),
                WS_OVERLAPPEDWINDOW,
                x,
                y,
                win_w,
                win_h,
                None,
                None,
                hinstance,
                None,
            )
        }
        .map_err(|e| format!("Failed to create sign-in window: {e}"))?;

        // SAFETY: Present sign-in window immediately before WebView2 finishes loading.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = UpdateWindow(hwnd);
        }

        let cleanup_hwnd = |h: HWND| {
            // SAFETY: Destroy empty sign-in window on init failure before state is attached.
            unsafe {
                let _ = DestroyWindow(h);
            }
        };

        let environment = {
            let (tx, rx) = mpsc::channel();
            let user_data_str = profile_dir.to_string_lossy().into_owned();
            if let Err(e) = CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
                Box::new(move |handler| {
                    let ud_w = CoTaskMemPWSTR::from(user_data_str.as_str());
                    // SAFETY: `ud_w` lives across the COM call; Edge WebView2 Runtime creates environment async.
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
                Box::new(move |error_code, env| {
                    error_code?;
                    let _ = tx.send(env.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                    Ok(())
                }),
            ) {
                cleanup_hwnd(hwnd);
                return Err(format!("Microsoft Edge WebView2 failed to start: {e}"));
            }

            match rx.recv() {
                Ok(Ok(env)) => env,
                Ok(Err(e)) => {
                    cleanup_hwnd(hwnd);
                    return Err(format!("WebView2 environment error: {e}"));
                }
                Err(_) => {
                    cleanup_hwnd(hwnd);
                    return Err("WebView2 environment callback closed".to_string());
                }
            }
        };

        let controller = {
            let (tx, rx) = mpsc::channel();
            if let Err(e) = CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
                Box::new(move |handler| {
                    // SAFETY: Attach WebView2 controller to our STA-owned top-level `hwnd`.
                    unsafe {
                        environment
                            .CreateCoreWebView2Controller(hwnd, &handler)
                            .map_err(webview2_com::Error::WindowsError)
                    }
                }),
                Box::new(move |error_code, ctrl| {
                    error_code?;
                    let _ = tx.send(ctrl.ok_or_else(|| windows::core::Error::from(E_POINTER)));
                    Ok(())
                }),
            ) {
                cleanup_hwnd(hwnd);
                return Err(format!("WebView2 controller creation failed: {e}"));
            }

            match rx.recv() {
                Ok(Ok(ctrl)) => ctrl,
                Ok(Err(e)) => {
                    cleanup_hwnd(hwnd);
                    return Err(format!("WebView2 controller error: {e}"));
                }
                Err(_) => {
                    cleanup_hwnd(hwnd);
                    return Err("WebView2 controller callback closed".to_string());
                }
            }
        };

        // SAFETY: Resize WebView2 surface to fill window client rect, harden settings, and obtain core interface.
        let webview = unsafe {
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let _ = controller.SetBounds(rect);
            let _ = controller.SetIsVisible(true);
            let wv = match controller.CoreWebView2() {
                Ok(wv) => wv,
                Err(e) => {
                    let _ = controller.Close();
                    cleanup_hwnd(hwnd);
                    return Err(format!("CoreWebView2 unavailable: {e}"));
                }
            };
            harden_login_webview(&wv);
            wv
        };

        if target == LoginTarget::Spotify {
            // Register before the first navigation so every document (login, redirect,
            // web player) gets the hook ahead of the player's own token request.
            let hook_wv = webview.clone();
            let hook_js = SPOTIFY_TOKEN_HOOK_JS;
            let _ = AddScriptToExecuteOnDocumentCreatedCompletedHandler::wait_for_async_operation(
                Box::new(move |handler| {
                    // SAFETY: `hook_wv` is on this STA thread; the script outlives the COM call.
                    unsafe {
                        let js = CoTaskMemPWSTR::from(hook_js);
                        hook_wv
                            .AddScriptToExecuteOnDocumentCreated(
                                *js.as_ref().as_pcwstr(),
                                &handler,
                            )
                            .map_err(webview2_com::Error::WindowsError)
                    }
                }),
                Box::new(|error_code, _id| error_code),
            );
        }

        if clear_existing_cookies && let Ok(wv2) = webview.cast::<ICoreWebView2_2>() {
            // SAFETY: Clear cached Google cookies on explicit re-login after Sign Out.
            unsafe {
                if let Ok(cm) = wv2.CookieManager() {
                    let _ = cm.DeleteAllCookies();
                }
            }
        }

        let capture_in_flight = Rc::new(Cell::new(false));
        let destroyed = Rc::new(Cell::new(false));

        let nav_done = Rc::clone(&done_cb);
        let nav_flag = Rc::clone(&capture_in_flight);
        let nav_dead = Rc::clone(&destroyed);
        let nav_handler =
            NavigationCompletedEventHandler::create(Box::new(move |sender, _args| {
                if !nav_dead.get()
                    && let Some(wv) = sender
                {
                    try_capture_ytm_cookies(hwnd, &wv, &nav_done, &nav_flag, &nav_dead, target);
                }
                Ok(())
            }));

        let src_done = Rc::clone(&done_cb);
        let src_flag = Rc::clone(&capture_in_flight);
        let src_dead = Rc::clone(&destroyed);
        let src_handler = SourceChangedEventHandler::create(Box::new(move |sender, _args| {
            if !src_dead.get()
                && let Some(wv) = sender
            {
                try_capture_ytm_cookies(hwnd, &wv, &src_done, &src_flag, &src_dead, target);
            }
            Ok(())
        }));

        // SAFETY: Register event listeners, store Rc state pointer, start fallback timer, navigate.
        unsafe {
            let mut nav_token = 0_i64;
            let _ = webview.add_NavigationCompleted(&nav_handler, &mut nav_token);
            let mut src_token = 0_i64;
            let _ = webview.add_SourceChanged(&src_handler, &mut src_token);

            let state = Rc::new(LoginWindowState {
                controller,
                webview: webview.clone(),
                done_cb,
                capture_in_flight,
                destroyed,
                target,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::into_raw(state) as isize);
            let _ = SetTimer(Some(hwnd), COOKIE_POLL_TIMER_ID, 900, None);

            let login_url = CoTaskMemPWSTR::from(target.login_url());
            let _ = webview.Navigate(*login_url.as_ref().as_pcwstr());
        }

        // Standard Win32 STA message pump until WM_DESTROY posts WM_QUIT.
        let mut msg = MSG::default();
        loop {
            // SAFETY: Thread message loop for this STA window.
            let res = unsafe { GetMessageW(&mut msg, None, 0, 0).0 };
            if res <= 0 {
                break;
            }
            // SAFETY: Dispatch window and WebView2 COM messages.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        Ok(())
    }

    fn try_capture_ytm_cookies(
        hwnd: HWND,
        webview: &ICoreWebView2,
        done_cb: &DoneSlot,
        capture_in_flight: &Rc<Cell<bool>>,
        destroyed: &Rc<Cell<bool>>,
        target: LoginTarget,
    ) {
        if destroyed.get() || capture_in_flight.get() || done_cb.borrow().is_none() {
            return;
        }
        let mut src_pwstr = PWSTR(ptr::null_mut());
        // SAFETY: Query current top-level WebView URL into COM-allocated PWSTR.
        let current_url = unsafe {
            if webview.Source(&mut src_pwstr).is_err() {
                return;
            }
            take_pwstr(src_pwstr)
        };
        if !is_exact_https_host(&current_url, target.cookie_host()) {
            return;
        }

        let Ok(wv2) = webview.cast::<ICoreWebView2_2>() else {
            return;
        };
        // SAFETY: Obtain CookieManager on STA thread.
        let Ok(cookie_manager) = (unsafe { wv2.CookieManager() }) else {
            return;
        };

        capture_in_flight.set(true);
        let done_slot = Rc::clone(done_cb);
        let flag_slot = Rc::clone(capture_in_flight);
        let dead_slot = Rc::clone(destroyed);
        let wv_for_capture = webview.clone();

        let handler = GetCookiesCompletedHandler::create(Box::new(move |status, list_opt| {
            flag_slot.set(false);
            if status.is_err() || dead_slot.get() {
                return Ok(());
            }
            let Some(list) = list_opt else {
                return Ok(());
            };

            let mut count = 0_u32;
            // SAFETY: Enumerate cookies returned by WebView2 CookieManager.
            unsafe {
                if list.Count(&mut count).is_err() {
                    return Ok(());
                }
            }

            let mut pairs = Vec::with_capacity(count as usize);
            for i in 0..count {
                // SAFETY: Release each COM PWSTR immediately after its call succeeds (zero leak on partial failure).
                unsafe {
                    if let Ok(cookie) = list.GetValueAtIndex(i) {
                        let mut name_raw = PWSTR(ptr::null_mut());
                        if cookie.Name(&mut name_raw).is_err() {
                            continue;
                        }
                        let name = take_pwstr(name_raw);
                        let mut val_raw = PWSTR(ptr::null_mut());
                        if cookie.Value(&mut val_raw).is_err() {
                            continue;
                        }
                        let val = take_pwstr(val_raw);
                        if !name.is_empty() {
                            pairs.push(format!("{name}={val}"));
                        }
                    }
                }
            }

            let cookie_header = pairs.join("; ");
            let captured = match target {
                LoginTarget::GoogleYtm => extract_sapisid_from_cookie(&cookie_header).is_some(),
                LoginTarget::Spotify => extract_sp_dc(&cookie_header).is_some(),
            };
            if captured {
                match target {
                    LoginTarget::Spotify => {
                        // The token endpoint rejects non-browser clients with a 403 edge block,
                        // so the token is fetched from inside the signed-in page context where
                        // the request carries the real session cookies and TLS fingerprint.
                        let done_inner = Rc::clone(&done_slot);
                        let dead_inner = Rc::clone(&dead_slot);
                        let wv_inner = wv_for_capture.clone();
                        let exec = ExecuteScriptCompletedHandler::create(Box::new(
                            move |status, result| {
                                if status.is_err() || dead_inner.get() {
                                    return Ok(());
                                }
                                emit_once(
                                    &done_inner,
                                    LoginPayload::SpotifySession {
                                        cookie: cookie_header.clone(),
                                        token_json: result,
                                    },
                                );
                                // SAFETY: Close the login window once the session payload is out.
                                unsafe {
                                    let _ =
                                        PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                                }
                                Ok(())
                            },
                        ));
                        let script = CoTaskMemPWSTR::from(SPOTIFY_TOKEN_READ_JS);
                        // SAFETY: Execute inside the signed-in page on the STA thread.
                        unsafe {
                            let _ = wv_inner.ExecuteScript(*script.as_ref().as_pcwstr(), &exec);
                        }
                    }
                    LoginTarget::GoogleYtm => {
                        emit_once(&done_slot, LoginPayload::Cookie(cookie_header));
                        if !dead_slot.get() {
                            // SAFETY: Auto-close once authenticated cookies are captured.
                            unsafe {
                                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                            }
                        }
                    }
                }
            }
            Ok(())
        }));

        let uri = CoTaskMemPWSTR::from(target.cookie_uri());
        // SAFETY: Async cookie query for the target host on STA thread.
        unsafe {
            if cookie_manager
                .GetCookies(*uri.as_ref().as_pcwstr(), &handler)
                .is_err()
            {
                capture_in_flight.set(false);
            }
        }
    }

    unsafe fn clone_login_state(hwnd: HWND) -> Option<Rc<LoginWindowState>> {
        // SAFETY: `GWLP_USERDATA` holds `Rc::into_raw` on this STA thread until `WM_DESTROY` clears it.
        unsafe {
            let raw_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const LoginWindowState;
            if raw_ptr.is_null() {
                return None;
            }
            Rc::increment_strong_count(raw_ptr);
            Some(Rc::from_raw(raw_ptr))
        }
    }

    unsafe extern "system" fn login_wnd_proc(
        hwnd: HWND,
        msg: u32,
        w_param: WPARAM,
        l_param: LPARAM,
    ) -> LRESULT {
        // SAFETY: All Win32 API calls inside this system window procedure execute on the owning STA thread.
        unsafe {
            match msg {
                WM_SIZE => {
                    if let Some(state) = clone_login_state(hwnd) {
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        let _ = state.controller.SetBounds(rect);
                    }
                    LRESULT(0)
                }
                WM_TIMER => {
                    if w_param.0 == COOKIE_POLL_TIMER_ID
                        && let Some(state) = clone_login_state(hwnd)
                    {
                        try_capture_ytm_cookies(
                            hwnd,
                            &state.webview,
                            &state.done_cb,
                            &state.capture_in_flight,
                            &state.destroyed,
                            state.target,
                        );
                    }
                    LRESULT(0)
                }
                WM_CLOSE => {
                    let _ = DestroyWindow(hwnd);
                    LRESULT(0)
                }
                WM_DESTROY => {
                    let _ = KillTimer(Some(hwnd), COOKIE_POLL_TIMER_ID);
                    let raw_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const LoginWindowState;
                    if !raw_ptr.is_null() {
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                        let state = Rc::from_raw(raw_ptr);
                        state.destroyed.set(true);
                        let _ = state.controller.Close();
                        drop(state);
                    }
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, w_param, l_param),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::{GoogleLoginOutcome, SpotifyLoginOutcome};
    use std::path::PathBuf;

    pub fn launch<F>(_profile_dir: PathBuf, _clear_existing_cookies: bool, on_done: F)
    where
        F: FnOnce(GoogleLoginOutcome) + Send + 'static,
    {
        on_done(GoogleLoginOutcome::Failed(
            "Native Google WebView2 sign-in is only supported on Windows.".to_string(),
        ));
    }

    pub fn launch_spotify<F>(_profile_dir: PathBuf, _clear_existing_cookies: bool, on_done: F)
    where
        F: FnOnce(SpotifyLoginOutcome) + Send + 'static,
    {
        on_done(SpotifyLoginOutcome::Failed(
            "Native Spotify WebView2 sign-in is only supported on Windows.".to_string(),
        ));
    }
}
