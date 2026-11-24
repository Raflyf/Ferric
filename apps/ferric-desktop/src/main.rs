#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod shell;
mod smtc;
mod theme;
mod track_list;
mod widgets;

use std::sync::OnceLock;

static STARTUP_PAGE_OVERRIDE: OnceLock<Option<String>> = OnceLock::new();

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

fn main() {
    let _ = startup_page_override();
    // Anchor working directory to the executable folder so portable Makepad
    // font/icon resources under `makepad/` resolve even from shortcuts.
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let _ = std::env::set_current_dir(exe_dir);
    }
    app::app_main();
}
