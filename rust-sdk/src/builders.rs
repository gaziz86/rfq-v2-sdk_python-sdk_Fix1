//! Builder patterns for creating Market Maker quotes and requests

use crate::error::{MarketMakerError, Result};
use crate::types::*;
use chrono::Utc;

/// Builder for creating MarketMakerQuote instances
#[derive(Debug, Clone)]
pub struct MarketMakerQuoteBuilder {
    maker_id: Option<String>,
    cluster: Cluster,
    token_pair: Option<TokenPair>,
    bid_levels: Vec<PriceLevel>,
    ask_levels: Vec<PriceLevel>,
    quote_expiry_time: u64,
    timestamp: Option<u64>,
    sequence_number: Option<u64>,
    maker_address: Option<String>,
    lot_size_base: Option<u64>,
}

impl Default for MarketMakerQuoteBuilder {
    fn default() -> Self {
        Self {
            maker_id: None,
            cluster: Cluster::Mainnet,
            token_pair: None,
            bid_levels: Vec::new(),
            ask_levels: Vec::new(),
            quote_expiry_time: 30_000_000, // 30 seconds in microseconds
            timestamp: None,
            sequence_number: None,
            maker_address: None,
            lot_size_base: None,
        }
    }
}

impl MarketMakerQuoteBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maker ID
    pub fn maker_id<S: Into<String>>(mut self, maker_id: S) -> Self {
        self.maker_id = Some(maker_id.into());
        self
    }

    /// Set the cluster (mainnet/devnet)
    pub fn cluster(mut self, cluster: Cluster) -> Self {
        self.cluster = cluster;
        self
    }

    /// Set the token pair
    pub fn token_pair(mut self, token_pair: TokenPair) -> Self {
        self.token_pair = Some(token_pair);
        self
    }

    /// Set SOL/USDC token pair
    pub fn sol_usdc_pair(mut self) -> Self {
        self.token_pair = Some(TokenPair::sol_usdc());
        self
    }

    /// Set ETH/USDC token pair
    pub fn eth_usdc_pair(mut self) -> Self {
        self.token_pair = Some(TokenPair::eth_usdc());
        self
    }

    /// Add a bid level
    pub fn bid_level(mut self, volume: u64, price: u64) -> Self {
        self.bid_levels.push(PriceLevel::new(volume, price));
        self
    }

    /// Add multiple bid levels
    pub fn bid_levels(mut self, levels: Vec<PriceLevel>) -> Self {
        self.bid_levels.extend(levels);
        self
    }

    /// Add an ask level
    pub fn ask_level(mut self, volume: u64, price: u64) -> Self {
        self.ask_levels.push(PriceLevel::new(volume, price));
        self
    }

    /// Add multiple ask levels
    pub fn ask_levels(mut self, levels: Vec<PriceLevel>) -> Self {
        self.ask_levels.extend(levels);
        self
    }

    /// Set quote expiry time in microseconds
    pub fn expiry_time_micros(mut self, micros: u64) -> Self {
        self.quote_expiry_time = micros;
        self
    }

    /// Set quote expiry time in seconds
    pub fn expiry_time_secs(mut self, secs: u64) -> Self {
        self.quote_expiry_time = secs * 1_000_000;
        self
    }

    /// Set the maker's Solana address
    pub fn maker_address(mut self, address: String) -> Self {
        self.maker_address = Some(address);
        self
    }

    /// Set a custom timestamp (defaults to current time)
    pub fn timestamp(mut self, timestamp_micros: u64) -> Self {
        self.timestamp = Some(timestamp_micros);
        self
    }

    /// Set sequence number
    pub fn sequence_number(mut self, seq: u64) -> Self {
        self.sequence_number = Some(seq);
        self
    }

    /// Set lot size base
    pub fn lot_size_base(mut self, lot_size: u64) -> Self {
        self.lot_size_base = Some(lot_size);
        self
    }

    /// Build the MarketMakerQuote
    pub fn build(self) -> Result<MarketMakerQuote> {
        let maker_id = self
            .maker_id
            .ok_or_else(|| MarketMakerError::validation("maker_id is required"))?;

        let token_pair = self
            .token_pair
            .ok_or_else(|| MarketMakerError::validation("token_pair is required"))?;

        if self.bid_levels.is_empty() && self.ask_levels.is_empty() {
            return Err(MarketMakerError::validation(
                "at least one bid or ask level is required",
            ));
        }

        // Validate that prices and volumes are non-zero for both bids and asks
        for level in self.bid_levels.iter().chain(self.ask_levels.iter()) {
            if level.price() == 0 {
                return Err(MarketMakerError::validation("price cannot be zero"));
            }
            if level.volume() == 0 {
                return Err(MarketMakerError::validation("volume cannot be zero"));
            }
        }

        let maker_address = self
            .maker_address
            .ok_or_else(|| MarketMakerError::validation("maker_address is required"))?;

        let lot_size_base = self
            .lot_size_base
            .ok_or_else(|| MarketMakerError::validation("lot_size_base is required"))?;

        Ok(MarketMakerQuote {
            timestamp: self
                .timestamp
                .unwrap_or_else(|| Utc::now().timestamp_micros() as u64),
            sequence_number: self.sequence_number.unwrap_or(1),
            quote_expiry_time: self.quote_expiry_time,
            maker_id,
            cluster: self.cluster as i32,
            token_pair,
            bid_levels: self.bid_levels,
            ask_levels: self.ask_levels,
            maker_address,
            lot_size_base,
        })
    }
}

/// Extension trait to add builder methods to MarketMakerQuote
pub trait MarketMakerQuoteBuilderExt {
    /// Create a builder from an existing quote
    fn to_builder(&self) -> MarketMakerQuoteBuilder;
}

impl MarketMakerQuoteBuilderExt for MarketMakerQuote {
    fn to_builder(&self) -> MarketMakerQuoteBuilder {
        MarketMakerQuoteBuilder {
            maker_id: Some(self.maker_id.clone()),
            cluster: Cluster::try_from(self.cluster).unwrap_or(Cluster::Mainnet),
            token_pair: Some(self.token_pair.clone()),
            bid_levels: self.bid_levels.clone(),
            ask_levels: self.ask_levels.clone(),
            quote_expiry_time: self.quote_expiry_time,
            timestamp: Some(self.timestamp),
            sequence_number: Some(self.sequence_number),
            maker_address: Some(self.maker_address.clone()),
            lot_size_base: Some(self.lot_size_base),
        }
    }
}

/// Convenience methods for MarketMakerQuote
impl MarketMakerQuote {
    /// Create a new builder
    pub fn builder() -> MarketMakerQuoteBuilder {
        MarketMakerQuoteBuilder::new()
    }
}
