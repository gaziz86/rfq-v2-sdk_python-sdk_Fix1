//! Birdeye API client for fetching real-time cryptocurrency prices
//!
//! This module provides a client for interacting with the Birdeye API to fetch
//! current token prices, particularly useful for market making applications.

use serde::{Deserialize, Serialize};
use std::error::Error;

/// Birdeye API client for fetching token prices
#[derive(Debug, Clone)]
pub struct BirdeyeClient {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

/// Response structure for Birdeye price API
#[derive(Debug, Deserialize, Serialize)]
pub struct BirdeyePriceResponse {
    pub success: bool,
    pub data: BirdeyePriceData,
}

/// Price data structure from Birdeye API
#[derive(Debug, Deserialize, Serialize)]
pub struct BirdeyePriceData {
    pub value: f64,
    #[serde(rename = "updateUnixTime")]
    pub update_unix_time: i64,
    #[serde(rename = "updateHumanTime")]
    pub update_human_time: String,
}

impl BirdeyeClient {
    /// Create a new Birdeye client with the given base URL and API key
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Fetch the current price for a given token address
    ///
    /// # Arguments
    /// * `token_address` - The token address (e.g., Solana token mint address)
    ///
    /// # Returns
    /// Returns a `BirdeyePriceResponse` containing the current price data
    pub async fn fetch_current_price(
        &self,
        token_address: &str,
    ) -> Result<BirdeyePriceResponse, Box<dyn Error + Send + Sync>> {
        let url = format!("{}/defi/price?address={}", self.base_url, token_address);

        let response = self
            .client
            .get(&url)
            .header("X-API-KEY", &self.api_key)
            .header("accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let price_response: BirdeyePriceResponse = response.json().await?;
        Ok(price_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_birdeye_client_creation() {
        let client = BirdeyeClient::new("https://public-api.birdeye.so", "test_key");
        assert_eq!(client.base_url, "https://public-api.birdeye.so");
        assert_eq!(client.api_key, "test_key");
    }

    // Note: Integration tests would require a real API key and network access
    // These would typically be run separately or with environment variables
}
