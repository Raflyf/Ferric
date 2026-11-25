pub mod db;
pub mod eq;
pub mod models;
pub mod queue;
pub mod secrets;
pub mod theme;

use std::path::PathBuf;
use thiserror::Error;

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
}

pub type CoreResult<T> = Result<T, CoreError>;

/// Resolve the canonical per-user data directory (`%APPDATA%\Ferric` on Windows).
pub fn app_data_dir() -> PathBuf {
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
