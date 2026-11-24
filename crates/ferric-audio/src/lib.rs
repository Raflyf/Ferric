pub mod dsp;
pub mod engine;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no audio output device available")]
    NoOutputDevice,
    #[error("audio device error: {0}")]
    Device(String),
    #[error("decoder error: {0}")]
    Decode(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

pub type AudioResult<T> = Result<T, AudioError>;
