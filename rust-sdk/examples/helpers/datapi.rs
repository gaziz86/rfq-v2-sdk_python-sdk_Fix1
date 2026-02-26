use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenPriceData {
    pub usd_price: f64,
    pub block_id: u64,
    pub decimals: u8,
    pub price_change24h: f64,
}

pub type DatapiResponse = HashMap<String, TokenPriceData>;

pub struct DatapiClient {
    client: reqwest::Client,
    host: String,
}

impl DatapiClient {
    pub fn new(url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            host: url.to_string(),
        }
    }

    pub async fn fetch_prices(
        &self,
        token_ids: &[String],
    ) -> Result<DatapiResponse, Box<dyn Error + Send + Sync>> {
        let ids = token_ids.join(",");
        let url = format!("{}/v1/prices?ids={}", self.host, ids);

        tracing::debug!("fetching prices for tokens {:?}", token_ids);

        let response = self
            .client
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let price_response: DatapiResponse = response.json().await?;
        Ok(price_response)
    }

    #[allow(dead_code)]
    pub async fn fetch_price(
        &self,
        token_id: &String,
    ) -> Result<DatapiResponse, Box<dyn Error + Send + Sync>> {
        self.fetch_prices(&[token_id.clone()]).await
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_fetch_price() {
        let token_id = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN".to_string();

        let mut response = DatapiResponse::new();
        response.insert(
            token_id.clone(),
            TokenPriceData {
                usd_price: 0.235262424439037,
                block_id: 383966743,
                decimals: 6,
                price_change24h: 4.90644413730616,
            },
        );

        let url = format!("/v1/prices?ids={}", token_id);

        let mut datapi_client_mock = mockito::Server::new_async().await;
        let price_mock = datapi_client_mock
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("accept", "application/json")
            .with_body(serde_json::to_string(&response).unwrap())
            .create_async()
            .await;

        let client = DatapiClient::new(datapi_client_mock.url().as_str());

        let result = client.fetch_price(&token_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.contains_key(&token_id));

        let price_data = response.get(&token_id).unwrap();
        assert_eq!(price_data.usd_price, 0.235262424439037);
        assert_eq!(price_data.block_id, 383966743);
        assert_eq!(price_data.decimals, 6);
        assert_eq!(price_data.price_change24h, 4.90644413730616);

        price_mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fetch_prices_multiple() {
        let token_ids = vec![
            "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN".to_string(),
            "So11111111111111111111111111111111111111112".to_string(),
        ];

        let mut response = DatapiResponse::new();
        response.insert(
            token_ids[0].clone(),
            TokenPriceData {
                usd_price: 0.235262424439037,
                block_id: 383966743,
                decimals: 6,
                price_change24h: 4.90644413730616,
            },
        );
        response.insert(
            token_ids[1].clone(),
            TokenPriceData {
                usd_price: 128.217824872236,
                block_id: 383966743,
                decimals: 9,
                price_change24h: 0.895752915330662,
            },
        );

        let url = format!("/v1/prices?ids={}", token_ids.join(","));

        let mut datapi_client_mock = mockito::Server::new_async().await;
        let prices_mock = datapi_client_mock
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("accept", "application/json")
            .with_body(serde_json::to_string(&response).unwrap())
            .create_async()
            .await;

        let client = DatapiClient::new(datapi_client_mock.url().as_str());

        let result = client.fetch_prices(&token_ids).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.len(), 2);
        assert!(response.contains_key(&token_ids[0]));
        assert!(response.contains_key(&token_ids[1]));

        let jup_price = response.get(&token_ids[0]).unwrap();
        assert_eq!(jup_price.usd_price, 0.235262424439037);
        assert_eq!(jup_price.decimals, 6);

        let sol_price = response.get(&token_ids[1]).unwrap();
        assert_eq!(sol_price.usd_price, 128.217824872236);
        assert_eq!(sol_price.decimals, 9);

        prices_mock.assert_async().await;
    }
}
