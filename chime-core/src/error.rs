//! Error types for the Chime audio core.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChimeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Codec error: {0}")]
    Codec(String),

    #[error("End of stream")]
    EndOfStream,

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}