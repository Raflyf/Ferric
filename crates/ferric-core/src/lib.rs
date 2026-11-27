pub mod collections;
pub mod db;
pub mod eq;
pub mod models;
pub mod queue;
pub mod secrets;
pub mod text;
pub mod theme;

use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use thiserror::Error;

static APP_DATA_DIR_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

fn app_data_dir_lock() -> &'static RwLock<Option<PathBuf>> {
    APP_DATA_DIR_OVERRIDE.get_or_init(|| RwLock::new(None))
}

/// Override the default platform data directory (required on Android where `ProjectDirs` is `None`).
pub fn set_app_data_dir_override(dir: PathBuf) {
    let mut guard = app_data_dir_lock()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(dir);
}

/// Clear any explicit data directory override (primarily for tests).
pub fn clear_app_data_dir_override() {
    let mut guard = app_data_dir_lock()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None;
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid color hex: {0}")]
    InvalidHex(String),
    #[error("invalid equalizer apo config: {0}")]
    InvalidEqApo(String),
    #[error("secret protection error: {0}")]
    SecretProtection(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

/// Resolve the canonical per-user data directory (`%APPDATA%\Ferric` on Windows, or explicit override on Android).
pub fn app_data_dir() -> PathBuf {
    let override_dir = app_data_dir_lock()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(dir) = override_dir {
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    if let Some(dirs) = directories::ProjectDirs::from("dev", "LivelyDesk", "Ferric") {
        let dir = dirs.data_local_dir().to_path_buf();
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    let fallback = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("ferric_data");
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_data_dir_respects_explicit_override() {
        let custom = std::env::temp_dir().join(format!("ferric_data_override_{}", std::process::id()));
        set_app_data_dir_override(custom.clone());
        assert_eq!(app_data_dir(), custom);
        clear_app_data_dir_override();
        let _ = std::fs::remove_dir_all(&custom);
    }
}
