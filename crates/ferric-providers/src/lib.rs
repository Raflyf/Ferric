pub mod local_scanner;
pub mod lyrics;
pub mod resolver;
pub mod yt_cleaner;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network request failed: {0}")]
    Http(String),
    #[error("metadata read failed: {0}")]
    Metadata(String),
}

pub type ProviderResult<T> = Result<T, ProviderError>;
