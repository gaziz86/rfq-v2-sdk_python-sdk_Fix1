//! # Market Maker Client SDK
//!
//! This SDK provides a Rust client for interacting with the Market Maker Ingestion Service.
//! It supports both regular quote submission and real-time bidirectional streaming.

pub mod builders;
pub mod client;
pub mod error;
pub mod streaming;
pub mod types;

pub mod market_maker {
    tonic::include_proto!("market_maker");
}

// Re-export main types for convenience
pub use builders::*;
pub use client::MarketMakerClient;
pub use error::{MarketMakerError, Result};
pub use streaming::*;
pub use types::*;

/// SDK version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default connection timeout in seconds
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default channel buffer size for streaming
pub const DEFAULT_CHANNEL_BUFFER_SIZE: usize = 1000;
