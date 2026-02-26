//! Error types for the fill-decoder crate.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, FillDecoderError>;

/// Errors produced by the fill-decoder crate.
#[derive(Error, Debug)]
pub enum FillDecoderError {
    /// Validation / format errors (e.g. wrong discriminator, missing accounts).
    #[error("Validation error: {0}")]
    Validation(String),

    /// Catch-all for unexpected failures (overflow, truncated data, etc.).
    #[error("Error: {0}")]
    Other(String),
}

impl FillDecoderError {
    /// Create a validation error.
    pub fn validation<S: Into<String>>(msg: S) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a generic error.
    pub fn other<S: Into<String>>(msg: S) -> Self {
        Self::Other(msg.into())
    }
}
