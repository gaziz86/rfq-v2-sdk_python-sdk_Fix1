//! Main client implementation for the Market Maker SDK

use crate::error::{MarketMakerError, Result};
use crate::market_maker::market_maker_ingestion_service_client::MarketMakerIngestionServiceClient;
use crate::streaming::{QuoteStreamHandle, StreamConfig};
use crate::types::*;
use std::error::Error;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::Request;
use tracing::{debug, error, info, instrument, warn};

/// Main client for interacting with the Market Maker Ingestion Service
#[derive(Clone)]
pub struct MarketMakerClient {
    inner: MarketMakerIngestionServiceClient<Channel>,
    config: ClientConfig,
}

impl MarketMakerClient {
    /// Helper to add authentication token to a request
    fn add_auth_token<T>(&self, mut request: Request<T>) -> Result<Request<T>> {
        if let Some(auth_token) = &self.config.auth_token {
            request.metadata_mut().insert(
                "x-api-key",
                auth_token.parse().map_err(|_| {
                    MarketMakerError::configuration("Invalid auth token format".to_string())
                })?,
            );
            debug!("Added authentication token to request metadata");
        }
        Ok(request)
    }

    /// Connect to the Market Maker service with default configuration
    #[instrument(skip(endpoint))]
    pub async fn connect<S: Into<String>>(endpoint: S) -> Result<Self> {
        let config = ClientConfig::new(endpoint.into());
        Self::connect_with_config(config).await
    }

    /// Connect to the Market Maker service with custom configuration
    #[instrument(skip(config))]
    pub async fn connect_with_config(config: ClientConfig) -> Result<Self> {
        info!("Connecting to Market Maker service at {}", config.endpoint);

        let mut endpoint = Endpoint::try_from(config.endpoint.clone())
            .map_err(|e| MarketMakerError::configuration(format!("Invalid endpoint: {}", e)))?
            .timeout(Duration::from_secs(config.timeout_secs));

        if config.endpoint.starts_with("https://") {
            debug!("Configuring HTTPS connection with HTTP/2 over TLS and ALPN");
            let tls_config = ClientTlsConfig::new().with_native_roots();

            endpoint = endpoint.tls_config(tls_config).map_err(|e| {
                MarketMakerError::configuration(format!("TLS configuration failed: {}", e))
            })?;

            debug!("TLS configuration with HTTP/2 and ALPN enabled");
        } else {
            debug!("Using HTTP/2 connection (plain text for development)");
        }

        let channel = endpoint.connect().await.map_err(|e| {
            error!("Connection error: {}", e);
            MarketMakerError::Connection(e)
        })?;

        let inner = MarketMakerIngestionServiceClient::new(channel);

        debug!("Successfully connected to Market Maker service with HTTP/2 protocol");

        Ok(Self { inner, config })
    }

    /// Start a bidirectional gRPC streaming connection for real-time quote updates
    #[instrument(skip(self))]
    pub async fn start_streaming(&mut self) -> Result<QuoteStreamHandle> {
        self.start_streaming_with_config(&StreamConfig::default())
            .await
    }

    /// Start gRPC streaming with custom configuration
    #[instrument(skip(self))]
    pub async fn start_streaming_with_config(
        &mut self,
        _config: &StreamConfig,
    ) -> Result<QuoteStreamHandle> {
        info!("Starting bidirectional gRPC streaming connection");

        // Create an unbounded channel for quote sending
        let (quote_tx, quote_rx) = mpsc::unbounded_channel();

        // Convert the receiver to a gRPC-compatible stream
        let quote_stream = UnboundedReceiverStream::new(quote_rx);

        // Establish the bidirectional gRPC stream with the remote server
        let request = Request::new(quote_stream);
        let request = self.add_auth_token(request)?;

        let response = self
            .inner
            .stream_quotes(request)
            .await
            .map_err(MarketMakerError::Grpc)?;

        // Get the inbound stream of updates from the server
        let update_stream = response.into_inner();

        debug!("gRPC streaming connection established successfully");

        Ok(QuoteStreamHandle::new(quote_tx, update_stream))
    }

    /// Start a bidirectional gRPC streaming connection for swap updates
    #[instrument(skip(self))]
    pub async fn start_swap_streaming(&mut self) -> Result<crate::streaming::SwapStreamHandle> {
        info!("Starting bidirectional gRPC swap streaming connection");

        // Create an unbounded channel for swap sending
        let (swap_tx, swap_rx) = mpsc::unbounded_channel();
        let swap_stream = UnboundedReceiverStream::new(swap_rx);
        let request = Request::new(swap_stream);
        let request = self.add_auth_token(request)?;

        let response = self
            .inner
            .stream_swap(request)
            .await
            .map_err(MarketMakerError::Grpc)?;

        // Get the inbound stream of updates from the server
        let update_stream = response.into_inner();

        debug!("gRPC swap streaming connection established successfully");

        Ok(crate::streaming::SwapStreamHandle::new(
            swap_tx,
            update_stream,
        ))
    }

    /// Get a copy of the client configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Get the last sequence number for a maker (for synchronization before streaming)
    #[instrument(skip(self), fields(maker_id = %maker_id))]
    pub async fn get_last_sequence_number(
        &mut self,
        maker_id: String,
        auth_token: String,
    ) -> Result<u64> {
        debug!("Getting last sequence number for maker: {}", maker_id);
        use crate::market_maker::SequenceNumberRequest;
        let request = Request::new(SequenceNumberRequest {
            maker_id: maker_id.clone(),
            auth_token,
        });

        let response = self
            .inner
            .get_last_sequence_number(request)
            .await
            .map_err(MarketMakerError::Grpc)?;

        let sequence_response = response.into_inner();

        if sequence_response.success {
            debug!(
                "Retrieved last sequence number for maker {}: {}",
                maker_id, sequence_response.last_sequence_number
            );
            Ok(sequence_response.last_sequence_number)
        } else {
            warn!(
                "Failed to get sequence number for maker {}: {}",
                maker_id, sequence_response.message
            );
            Ok(0)
        }
    }

    /// Receive an update containing all orderbooks for a specific cluster or all clusters
    #[instrument(skip(self))]
    pub async fn receive_update(
        &mut self,
        cluster: Option<Cluster>,
    ) -> Result<GetAllOrderbooksResponse> {
        debug!("Receiving update for all orderbooks");

        use crate::market_maker::GetAllOrderbooksRequest;

        let request = Request::new(GetAllOrderbooksRequest {
            cluster: cluster.map(|c| c as i32),
        });

        let response = self
            .inner
            .get_all_orderbooks(request)
            .await
            .map_err(MarketMakerError::Grpc)?;

        let orderbooks_response = response.into_inner();

        info!(
            "Retrieved {} orderbooks at timestamp {}",
            orderbooks_response.orderbooks.len(),
            orderbooks_response.timestamp
        );

        Ok(orderbooks_response)
    }
}

/// Convenience methods for common operations
impl MarketMakerClient {
    /// Start streaming with automatic sequence number synchronization
    #[instrument(skip(self), fields(maker_id = %maker_id))]
    pub async fn start_streaming_with_sync(
        &mut self,
        maker_id: String,
        auth_token: String,
    ) -> Result<(QuoteStreamHandle, u64)> {
        self.start_streaming_with_sync_and_config(maker_id, auth_token, &StreamConfig::default())
            .await
    }

    /// Start streaming with automatic sequence number synchronization and custom config
    ///
    /// The returned stream handle supports graceful shutdown and proper cleanup.
    #[instrument(skip(self, stream_config), fields(maker_id = %maker_id))]
    pub async fn start_streaming_with_sync_and_config(
        &mut self,
        maker_id: String,
        auth_token: String,
        stream_config: &StreamConfig,
    ) -> Result<(QuoteStreamHandle, u64)> {
        debug!(
            "Starting streaming with sequence number synchronization for maker: {}",
            maker_id
        );

        let last_sequence = self
            .get_last_sequence_number(maker_id.clone(), auth_token.clone())
            .await?;
        let stream_handle = self.start_streaming_with_config(stream_config).await?;
        let next_sequence = last_sequence + 1;

        debug!(
            "Sequence sync complete for maker {}: last={}, next={}",
            maker_id, last_sequence, next_sequence
        );

        Ok((stream_handle, next_sequence))
    }

    /// Properly shutdown a streaming connection with timeout
    pub async fn shutdown_stream_with_timeout(
        stream: &mut QuoteStreamHandle,
        timeout: std::time::Duration,
    ) -> Result<()> {
        info!(
            "Shutting down streaming connection with timeout: {:?}",
            timeout
        );

        match stream.close_with_timeout(timeout).await {
            Ok(_) => {
                info!("Stream shutdown completed successfully");
                Ok(())
            }
            Err(e) => {
                warn!("Stream shutdown encountered issues: {}", e);
                Err(e)
            }
        }
    }

    /// Shutdown a stream with statistics reporting
    pub async fn shutdown_stream_with_stats(
        stream: &mut QuoteStreamHandle,
        timeout: std::time::Duration,
    ) -> Result<()> {
        info!("Collecting final statistics before shutdown");

        let stats = stream.get_stats().await;
        info!("Final Stream Statistics:");
        info!("Messages sent: {}", stats.messages_sent);
        info!("Updates received: {}", stats.updates_received);
        info!("Errors encountered: {}", stats.errors_encountered);
        info!("Connected for: {:?}", stats.connected_at.elapsed());

        Self::shutdown_stream_with_timeout(stream, timeout).await
    }
}
