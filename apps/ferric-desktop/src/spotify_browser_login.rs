//! External-browser Spotify sign-in through the Chrome DevTools Protocol.
//!
//! Chrome is preferred and Edge is the fallback that ships with Windows; Firefox is not
//! supported because it does not expose CDP. The browser always runs with Ferric's dedicated
//! profile, so the user's daily browsing profile is untouched and the Spotify session can be
//! reused on the next run.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::{Message, connect};

use crate::google_login::{SPOTIFY_TOKEN_HOOK_JS, SpotifyLoginOutcome};

static LOGIN_OPEN: AtomicBool = AtomicBool::new(false);

const LOGIN_URL: &str =
    "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F";
const START_TIMEOUT: Duration = Duration::from_secs(25);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

pub fn browser_available() -> bool {
    !find_browsers().is_empty()
}

pub fn launch<F>(profile_dir: PathBuf, clear_existing_cookies: bool, on_done: F)
where
    F: FnOnce(SpotifyLoginOutcome) + Send + 'static,
{
    if LOGIN_OPEN
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        on_done(SpotifyLoginOutcome::Failed(
            "A sign-in window is already open.".to_string(),
        ));
        return;
    }

    let spawned = thread::Builder::new()
        .name("ferric-spotify-browser-login".to_string())
        .spawn(move || {
            struct Guard;
            impl Drop for Guard {
                fn drop(&mut self) {
                    LOGIN_OPEN.store(false, Ordering::SeqCst);
                }
            }
            let _guard = Guard;
            let outcome = run(profile_dir, clear_existing_cookies);
            on_done(outcome);
        });

    if spawned.is_err() {
        LOGIN_OPEN.store(false, Ordering::SeqCst);
    }
}

fn find_browsers() -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(5);
    // Prefer admin-protected system installation paths ahead of user-writable %LOCALAPPDATA%.
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        let Ok(base) = std::env::var(key) else {
            continue;
        };
        candidates.push(
            Path::new(&base)
                .join("Google")
                .join("Chrome")
                .join("Application")
                .join("chrome.exe"),
        );
    }
    for key in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Ok(base) = std::env::var(key) else {
            continue;
        };
        candidates.push(
            Path::new(&base)
                .join("Microsoft")
                .join("Edge")
                .join("Application")
                .join("msedge.exe"),
        );
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(
            Path::new(&local)
                .join("Google")
                .join("Chrome")
                .join("Application")
                .join("chrome.exe"),
        );
    }
    candidates.retain(|path| path.is_file());
    candidates.dedup();
    candidates
}

fn run(profile_dir: PathBuf, clear_existing_cookies: bool) -> SpotifyLoginOutcome {
    let browsers = find_browsers();
    if browsers.is_empty() {
        return SpotifyLoginOutcome::Failed(
            "Chrome or Edge is required for the external Spotify sign-in.".to_string(),
        );
    }

    let user_data = profile_dir.join("browser");
    if let Err(err) = std::fs::create_dir_all(&user_data) {
        return SpotifyLoginOutcome::Failed(format!("Could not create the browser profile: {err}"));
    }

    // A previous Ferric browser may still hold the profile. Chrome forwards a second launch to
    // that instance, and the fresh debug port never appears, so reuse the live endpoint instead.
    let existing = active_debug_port(&user_data);
    let mut child = None;
    let port = if let Some(port) = existing {
        port
    } else {
        let port = match free_port() {
            Ok(port) => port,
            Err(err) => {
                return SpotifyLoginOutcome::Failed(format!(
                    "Could not reserve a local debug port: {err}"
                ));
            }
        };

        let mut last_error = None;
        for exe in &browsers {
            match spawn_browser(exe, &user_data, port) {
                Ok(mut spawned) => {
                    thread::sleep(Duration::from_millis(500));
                    match spawned.try_wait() {
                        Ok(None) => {
                            child = Some(spawned);
                            last_error = None;
                            break;
                        }
                        Ok(Some(_)) => {
                            last_error = Some(format!(
                                "{} exited before the debug endpoint opened",
                                exe.display()
                            ));
                        }
                        Err(err) => {
                            last_error = Some(format!("Could not poll the browser process: {err}"));
                        }
                    }
                }
                Err(err) => {
                    last_error = Some(format!("Could not start {}: {err}", exe.display()));
                }
            }
        }

        if child.is_none() {
            return SpotifyLoginOutcome::Failed(format!(
                "Could not start Chrome or Edge: {}",
                last_error.unwrap_or_else(|| "no usable browser was found".to_string())
            ));
        }
        port
    };

    let port_marker = user_data.join("ferric-debug-port");
    struct PortMarkerCleanup(PathBuf);
    impl Drop for PortMarkerCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _ = std::fs::write(&port_marker, port.to_string());
    let _marker_guard = PortMarkerCleanup(port_marker);

    let outcome = drive(&mut child, port, clear_existing_cookies);
    if let Some(mut spawned) = child {
        let _ = spawned.kill();
        let _ = spawned.wait();
    }
    outcome
}

fn spawn_browser(exe: &Path, user_data: &Path, port: u16) -> std::io::Result<Child> {
    Command::new(exe)
        .arg(format!("--user-data-dir={}", user_data.display()))
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--remote-allow-origins=http://127.0.0.1:{port}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-sync")
        .arg("--new-window")
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn active_debug_port(user_data: &Path) -> Option<u16> {
    for name in ["ferric-debug-port", "DevToolsActivePort"] {
        let Ok(raw) = std::fs::read_to_string(user_data.join(name)) else {
            continue;
        };
        let Some(first) = raw.lines().next() else {
            continue;
        };
        let Ok(port) = first.trim().parse::<u16>() else {
            continue;
        };
        if http_get(port, "/json/version").is_some() {
            return Some(port);
        }
    }
    None
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn drive(
    child: &mut Option<Child>,
    port: u16,
    clear_existing_cookies: bool,
) -> SpotifyLoginOutcome {
    let deadline = Instant::now() + START_TIMEOUT;
    let ws_url = match wait_for_page(port, deadline, child) {
        Ok(url) => url,
        Err(err) => return SpotifyLoginOutcome::Failed(err),
    };

    let mut cdp = match Cdp::connect(&ws_url, port) {
        Ok(cdp) => cdp,
        Err(err) => return SpotifyLoginOutcome::Failed(err),
    };

    if let Err(err) = cdp.call("Page.enable", json!({})) {
        return SpotifyLoginOutcome::Failed(err);
    }
    if let Err(err) = cdp.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": SPOTIFY_TOKEN_HOOK_JS }),
    ) {
        return SpotifyLoginOutcome::Failed(err);
    }
    if let Err(err) = cdp.call("Network.enable", json!({})) {
        return SpotifyLoginOutcome::Failed(err);
    }
    if clear_existing_cookies {
        // Wipe all cookies & origin storage in Ferric's dedicated browser profile so clicking
        // Sign Out -> Sign in via Web always opens a clean Spotify sign-in screen instead of
        // auto-logging back into the previous account.
        let _ = cdp.call("Network.clearBrowserCookies", json!({}));
        let _ = cdp.call("Network.clearBrowserCache", json!({}));
        for origin in [
            "https://open.spotify.com",
            "https://accounts.spotify.com",
            "https://accounts.google.com",
        ] {
            let _ = cdp.call(
                "Storage.clearDataForOrigin",
                json!({ "origin": origin, "storageTypes": "all" }),
            );
        }
        let _ = cdp.call(
            "Runtime.evaluate",
            json!({ "expression": "window.__ferricToken = null;", "returnByValue": true }),
        );
    }
    if let Err(err) = cdp.call("Page.navigate", json!({ "url": LOGIN_URL })) {
        return SpotifyLoginOutcome::Failed(err);
    }

    loop {
        if let Some(spawned) = child.as_mut() {
            match spawned.try_wait() {
                Ok(Some(_)) => return SpotifyLoginOutcome::Cancelled,
                Ok(None) => {}
                Err(_) => return SpotifyLoginOutcome::Cancelled,
            }
        } else if http_get(port, "/json/version").is_none() {
            return SpotifyLoginOutcome::Cancelled;
        }

        if let Ok(result) = cdp.call(
            "Runtime.evaluate",
            json!({
                "expression": "(location.origin === 'https://open.spotify.com' && window.__ferricToken) || null",
                "returnByValue": true,
            }),
        ) {
            let token_json = result
                .get("result")
                .and_then(|value| value.get("value"))
                .and_then(Value::as_str);
            if let Some(text) = token_json
                && ferric_providers::spotify_web::parse_web_token_json(text).is_ok()
            {
                let sp_dc = read_sp_dc(&mut cdp).unwrap_or_default();
                // Closing is best-effort: the window is Ferric's own dedicated instance.
                let _ = cdp.call("Browser.close", json!({}));
                return SpotifyLoginOutcome::SignedIn {
                    sp_dc,
                    token_json: text.to_string(),
                };
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn read_sp_dc(cdp: &mut Cdp) -> Option<String> {
    let result = cdp
        .call(
            "Network.getCookies",
            json!({ "urls": ["https://open.spotify.com"] }),
        )
        .ok()?;
    result
        .get("cookies")?
        .as_array()?
        .iter()
        .find_map(|cookie| {
            let name = cookie.get("name")?.as_str()?;
            let domain = cookie.get("domain")?.as_str()?.to_ascii_lowercase();
            let secure = cookie
                .get("secure")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let domain_ok =
                domain == ".spotify.com" || domain == "spotify.com" || domain == "open.spotify.com";
            (name == "sp_dc" && domain_ok && secure)
                .then(|| cookie.get("value")?.as_str().map(str::to_owned))
                .flatten()
        })
}

fn is_valid_loopback_cdp_ws_url(ws: &str, port: u16) -> bool {
    let prefix = format!("ws://127.0.0.1:{port}/devtools/page/");
    ws.starts_with(&prefix)
        && ws.len() <= 256
        && ws
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'/' | b'.' | b'-' | b'_'))
}

fn wait_for_page(
    port: u16,
    deadline: Instant,
    child: &mut Option<Child>,
) -> Result<String, String> {
    let mut last_http = String::from("no HTTP response");
    while Instant::now() < deadline {
        if let Some(spawned) = child.as_mut() {
            match spawned.try_wait() {
                Ok(Some(_)) => {
                    return Err(
                        "The browser exited before its DevTools page was ready. Close any Ferric Spotify browser window and try again."
                            .to_string(),
                    );
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }

        if let Some(body) = http_get(port, "/json/list") {
            last_http = body.chars().take(200).collect();
            if let Ok(list) = serde_json::from_str::<Value>(&body)
                && let Some(items) = list.as_array()
            {
                for item in items {
                    if item.get("type").and_then(Value::as_str) == Some("page")
                        && let Some(ws) = item.get("webSocketDebuggerUrl").and_then(Value::as_str)
                        && is_valid_loopback_cdp_ws_url(ws, port)
                    {
                        return Ok(ws.to_string());
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!(
        "The browser started but did not expose a DevTools page ({last_http}). Close any Ferric Spotify browser window and try again."
    ))
}

fn http_get(port: u16, path: &str) -> Option<String> {
    let url = format!("http://127.0.0.1:{port}{path}");
    let agent = ferric_providers::shared_http_agent();
    let response = agent.get(&url).call().ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let bytes = response
        .into_body()
        .into_with_config()
        .limit(512 * 1024)
        .read_to_vec()
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

struct Cdp {
    socket: tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Cdp {
    fn connect(url: &str, port: u16) -> Result<Self, String> {
        if !is_valid_loopback_cdp_ws_url(url, port) {
            return Err("Refused non-loopback DevTools WebSocket URL".to_string());
        }
        let uri: tungstenite::http::Uri = url
            .parse()
            .map_err(|err| format!("Invalid DevTools URI: {err}"))?;
        let req = tungstenite::ClientRequestBuilder::new(uri)
            .with_header("Origin", format!("http://127.0.0.1:{port}"));
        let (socket, _) = connect(req).map_err(|err| format!("DevTools connect failed: {err}"))?;
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = socket.get_ref() {
            let _ = stream.set_read_timeout(Some(COMMAND_TIMEOUT));
        }
        Ok(Self { socket, next_id: 1 })
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({ "id": id, "method": method, "params": params }).to_string();
        self.socket
            .send(Message::Text(payload.into()))
            .map_err(|err| format!("DevTools send failed: {err}"))?;

        loop {
            let message = self
                .socket
                .read()
                .map_err(|err| format!("DevTools read failed: {err}"))?;
            let text = match message {
                Message::Text(text) => text.as_str().to_string(),
                Message::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Message::Close(_) => {
                    return Err("The browser closed the DevTools connection.".to_string());
                }
                _ => continue,
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(format!("DevTools {method} failed: {error}"));
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "launches a real browser window"]
    fn cdp_roundtrip() {
        let Some(exe) = find_browsers().into_iter().next() else {
            eprintln!("no Chrome or Edge installed");
            return;
        };
        let profile =
            std::env::temp_dir().join(format!("ferric-cdp-selftest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile);
        std::fs::create_dir_all(&profile).unwrap();
        let port = free_port().unwrap();
        let spawned = spawn_browser(&exe, &profile, port).unwrap();
        let mut child = Some(spawned);
        let ws =
            wait_for_page(port, Instant::now() + START_TIMEOUT, &mut child).expect("devtools page");
        let mut cdp = Cdp::connect(&ws, port).expect("cdp connect");
        cdp.call("Page.enable", json!({})).unwrap();
        let result = cdp
            .call(
                "Runtime.evaluate",
                json!({ "expression": "1+1", "returnByValue": true }),
            )
            .unwrap();
        assert_eq!(result["result"]["value"].as_i64(), Some(2));
        if let Some(mut spawned) = child {
            let _ = spawned.kill();
            let _ = spawned.wait();
        }
        let _ = std::fs::remove_dir_all(&profile);
    }
}
