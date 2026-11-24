#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod smtc;
mod track_list;

fn main() {
    // Anchor working directory to the executable folder so portable Makepad
    // font/icon resources under `makepad/` resolve even from shortcuts.
    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        let _ = std::env::set_current_dir(exe_dir);
    }
    app::app_main();
}
