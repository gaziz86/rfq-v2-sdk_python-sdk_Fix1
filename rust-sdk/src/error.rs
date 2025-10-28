use thiserror::Error;

/// Result type for Market Maker SDK operations
pub type Result<T> = std::result::Result<T, MarketMakerError>;

/// Comprehensive error types for the Market Maker SDK
#[derive(Error, Debug)]
pub enum MarketMakerError {
    /// Connection-related errors
    #[error("Connection error: {0}")]
    Connection(#[from] tonic::transport::Error),

    /// gRPC status errors
    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),

    /// Validation errors for quote data
    #[error("Validation error: {0}")]
    Validation(String),

    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Streaming-related errors
    #[error("Streaming error: {0}")]
    Streaming(String),

    /// Timeout errors
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Generic errors
    #[error("Error: {0}")]
    Other(String),
}

impl MarketMakerError {
    /// Create a validation error
    pub fn validation<S: Into<String>>(msg: S) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a streaming error
    pub fn streaming<S: Into<String>>(msg: S) -> Self {
        Self::Streaming(msg.into())
    }

    /// Create a timeout error
    pub fn timeout<S: Into<String>>(msg: S) -> Self {
        Self::Timeout(msg.into())
    }

    /// Create a configuration error
    pub fn configuration<S: Into<String>>(msg: S) -> Self {
        Self::Configuration(msg.into())
    }

    /// Create a generic error
    pub fn other<S: Into<String>>(msg: S) -> Self {
        Self::Other(msg.into())
    }

    /// Check if this is a connection-related error
    pub fn is_connection_error(&self) -> bool {
        matches!(self, Self::Connection(_))
    }

    /// Check if this is a gRPC error
    pub fn is_grpc_error(&self) -> bool {
        matches!(self, Self::Grpc(_))
    }

    /// Check if this is a validation error
    pub fn is_validation_error(&self) -> bool {
        matches!(self, Self::Validation(_))
    }

    /// Check if this is a streaming error
    pub fn is_streaming_error(&self) -> bool {
        matches!(self, Self::Streaming(_))
    }

    /// Check if this is a timeout error
    pub fn is_timeout_error(&self) -> bool {
        matches!(self, Self::Timeout(_))
    }
}
