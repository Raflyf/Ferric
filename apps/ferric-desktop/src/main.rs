#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod artwork;
mod discord_rpc;
mod google_login;
mod icons;
mod shell;
mod smtc;
#[cfg(target_os = "windows")]
mod spotify_browser_login;
mod state;
mod theme;
mod track_list;
mod video_player;
mod widgets;

use std::sync::OnceLock;

#[cfg(debug_assertions)]
static STARTUP_PAGE_OVERRIDE: OnceLock<Option<String>> = OnceLock::new();
static STARTUP_LOAD_DEMO: OnceLock<bool> = OnceLock::new();
#[cfg(debug_assertions)]
static STARTUP_PLAY_FIRST: OnceLock<bool> = OnceLock::new();

#[cfg(debug_assertions)]
pub fn startup_page_override() -> Option<&'static str> {
    STARTUP_PAGE_OVERRIDE
        .get_or_init(|| {
            let mut args = std::env::args().skip(1);
            while let Some(arg) = args.next() {
                if arg == "--page" {
                    return args.next();
                }
                if let Some(val) = arg.strip_prefix("--page=") {
                    return Some(val.to_string());
                }
            }
            None
        })
        .as_deref()
}

#[cfg(not(debug_assertions))]
pub fn startup_page_override() -> Option<&'static str> {
    None
}

pub fn startup_load_demo() -> bool {
    *STARTUP_LOAD_DEMO.get_or_init(|| std::env::args().skip(1).any(|a| a == "--demo"))
}

#[cfg(debug_assertions)]
pub fn startup_play_first() -> bool {
    *STARTUP_PLAY_FIRST.get_or_init(|| std::env::args().skip(1).any(|a| a == "--play-first"))
}

#[cfg(not(debug_assertions))]
pub fn startup_play_first() -> bool {
    false
}

fn main() {
    shell::win32_harden_process();
    shell::win32_set_app_user_model_id();
    if !shell::win32_acquire_single_instance_or_activate() {
        return;
    }
    let _ = startup_page_override();
    let _ = startup_load_demo();
    let _ = startup_play_first();
    // Anchor working directory to the executable folder so portable Makepad
    // font/icon resources under `makepad/` resolve even from shortcuts.
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let _ = std::env::set_current_dir(exe_dir);
    }
    app::app_main();
}
