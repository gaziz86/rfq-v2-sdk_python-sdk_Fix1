//! Type definitions and helpers for the RFQv2 SDK

use chrono::Utc;

// Re-export the generated types for convenience
pub use crate::market_maker::{
    Cluster, GetAllOrderbooksRequest, GetAllOrderbooksResponse, GetQuotesRequest,
    GetQuotesResponse, MarketMakerQuote, MarketMakerSwap, Orderbook, PriceLevel, QuoteResponse,
    QuoteUpdate, SequenceNumberRequest, SequenceNumberResponse, SwapMessageType, SwapUpdate, Token,
    TokenPair, UpdateType,
};

/// Configuration for connecting to the RFQv2 service
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Service endpoint URL
    pub endpoint: String,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Buffer size for streaming channels
    pub stream_buffer_size: usize,
    /// Authentication token for API access
    pub auth_token: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:2408".to_string(),
            timeout_secs: crate::DEFAULT_TIMEOUT_SECS,
            max_retries: 3,
            stream_buffer_size: crate::DEFAULT_CHANNEL_BUFFER_SIZE,
            auth_token: None,
        }
    }
}

impl ClientConfig {
    /// Create a new configuration with the specified endpoint
    pub fn new<S: Into<String>>(endpoint: S) -> Self {
        Self {
            endpoint: endpoint.into(),
            ..Default::default()
        }
    }

    /// Set the connection timeout
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set the maximum retry attempts
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set the authentication token for API access
    pub fn with_auth_token<S: Into<String>>(mut self, auth_token: S) -> Self {
        self.auth_token = Some(auth_token.into());
        self
    }
}

/// Common token pairs for convenience
impl TokenPair {
    /// SOL/USDC token pair on mainnet
    pub fn sol_usdc() -> Self {
        Self {
            base_token: Token {
                address: "So11111111111111111111111111111111111111112".to_string(),
                decimals: 9,
                symbol: "SOL".to_string(),
                owner: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
            },
            quote_token: Token {
                address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                decimals: 6,
                symbol: "USDC".to_string(),
                owner: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
            },
        }
    }

    /// ETH/USDC token pair on mainnet
    pub fn eth_usdc() -> Self {
        Self {
            base_token: Token {
                address: "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs".to_string(),
                decimals: 8,
                symbol: "ETH".to_string(),
                owner: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
            },
            quote_token: Token {
                address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                decimals: 6,
                symbol: "USDC".to_string(),
                owner: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string(),
            },
        }
    }

    /// Create a custom token pair
    pub fn new(base_token: Token, quote_token: Token) -> Self {
        Self {
            base_token,
            quote_token,
        }
    }

    /// Get a string representation of the token pair (e.g., "SOL/USDC")
    pub fn pair_name(&self) -> String {
        format!("{}/{}", self.base_token.symbol, self.quote_token.symbol)
    }
}

impl Token {
    /// Create a new token
    pub fn new<S1: Into<String>, S2: Into<String>, S3: Into<String>>(
        address: S1,
        decimals: u32,
        symbol: S2,
        owner: S3,
    ) -> Self {
        Self {
            address: address.into(),
            decimals,
            symbol: symbol.into(),
            owner: owner.into(),
        }
    }
}

impl PriceLevel {
    /// Create a new price level
    pub fn new(volume: u64, price: u64) -> Self {
        Self { volume, price }
    }

    /// Get volume as u64
    pub fn volume(&self) -> u64 {
        self.volume
    }

    /// Get price as u64
    pub fn price(&self) -> u64 {
        self.price
    }
}

/// Extension trait for MarketMakerQuote
pub trait MarketMakerQuoteExt {
    /// Check if the quote has expired
    fn is_expired(&self) -> bool;

    /// Get the best bid price
    fn best_bid(&self) -> Option<&PriceLevel>;

    /// Get the best ask price
    fn best_ask(&self) -> Option<&PriceLevel>;

    /// Calculate the spread
    fn spread(&self) -> Option<u64>;
}

impl MarketMakerQuoteExt for MarketMakerQuote {
    fn is_expired(&self) -> bool {
        let now = Utc::now().timestamp_micros() as u64;
        now > self.timestamp + self.quote_expiry_time
    }

    fn best_bid(&self) -> Option<&PriceLevel> {
        self.bid_levels
            .iter()
            .max_by(|a, b| a.price().cmp(&b.price()))
    }

    fn best_ask(&self) -> Option<&PriceLevel> {
        self.ask_levels
            .iter()
            .min_by(|a, b| a.price().cmp(&b.price()))
    }

    fn spread(&self) -> Option<u64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => {
                // Spread in raw units (same decimals as price)
                ask.price().checked_sub(bid.price())
            }
            _ => None,
        }
    }
}
