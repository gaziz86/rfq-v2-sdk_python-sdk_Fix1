//! Streaming functionality for real-time quote and swap updates
//!
//! This module provides functionality for bidirectional gRPC streaming between
//! the market maker client and the ingestion service. The client can send quotes
//! and swaps to the server and receive real-time updates.

use crate::error::{MarketMakerError, Result};
use crate::types::*;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tonic::Streaming;

/// Handle for managing a bidirectional gRPC quote stream
pub struct QuoteStreamHandle {
    /// Sender for sending quotes to the gRPC stream
    quote_sender: tokio::sync::mpsc::UnboundedSender<MarketMakerQuote>,
    /// Direct gRPC stream receiver for incoming quote updates from the server
    pub update_receiver: Streaming<QuoteUpdate>,
    /// Shutdown notifier
    shutdown: Arc<Notify>,
    /// Connection statistics
    stats: Arc<Mutex<ConnectionStats>>,
    /// Flag to track if the stream has been gracefully closed
    is_closed: Arc<Mutex<bool>>,
}

impl QuoteStreamHandle {
    /// Create a new stream handle with direct gRPC streaming
    pub(crate) fn new(
        quote_sender: tokio::sync::mpsc::UnboundedSender<MarketMakerQuote>,
        update_receiver: Streaming<QuoteUpdate>,
    ) -> Self {
        let shutdown = Arc::new(Notify::new());
        let stats = Arc::new(Mutex::new(ConnectionStats::new()));
        let is_closed = Arc::new(Mutex::new(false));

        Self {
            quote_sender,
            update_receiver,
            shutdown,
            stats,
            is_closed,
        }
    }

    /// Send a quote directly to the gRPC server
    pub async fn send_quote(&self, quote: MarketMakerQuote) -> Result<()> {
        // Check if the stream has been closed
        if *self.is_closed.lock().await {
            return Err(MarketMakerError::streaming("Stream has been closed"));
        }

        self.quote_sender.send(quote).map_err(|_| {
            MarketMakerError::streaming("Failed to send quote - gRPC stream closed")
        })?;

        // Update statistics
        {
            let mut stats = self.stats.lock().await;
            stats.message_sent();
        }

        Ok(())
    }

    /// Receive the next quote update from the gRPC server
    pub async fn receive_update(&mut self) -> Result<Option<QuoteUpdate>> {
        // Check if the stream has been closed
        if *self.is_closed.lock().await {
            return Ok(None);
        }

        match self.update_receiver.message().await {
            Ok(Some(update)) => {
                // Update statistics
                {
                    let mut stats = self.stats.lock().await;
                    stats.update_received();
                }
                Ok(Some(update))
            }
            Ok(None) => {
                // Stream ended normally - mark as closed
                *self.is_closed.lock().await = true;
                Ok(None)
            }
            Err(e) => {
                // Update error statistics
                {
                    let mut stats = self.stats.lock().await;
                    stats.error_encountered();
                }
                Err(MarketMakerError::Grpc(e))
            }
        }
    }

    /// Receive updates with a timeout
    pub async fn receive_update_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<Option<QuoteUpdate>> {
        match tokio::time::timeout(timeout, self.receive_update()).await {
            Ok(result) => result,
            Err(_) => Err(MarketMakerError::timeout("Timed out waiting for update")),
        }
    }

    /// Close the gRPC stream gracefully
    pub async fn close(&mut self) {
        tracing::info!("Initiating graceful stream shutdown");

        // Mark as closed first to prevent new operations
        *self.is_closed.lock().await = true;

        // Close the outbound stream by dropping the sender
        drop(std::mem::replace(
            &mut self.quote_sender,
            tokio::sync::mpsc::unbounded_channel().0,
        ));

        // Notify any background tasks to shutdown
        self.shutdown.notify_waiters();

        // Give a brief moment for cleanup
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        tracing::info!("Stream shutdown completed");
    }

    /// Close the stream with a custom timeout
    pub async fn close_with_timeout(&mut self, timeout: std::time::Duration) -> Result<()> {
        match tokio::time::timeout(timeout, self.close()).await {
            Ok(_) => Ok(()),
            Err(_) => {
                tracing::warn!("Stream close timed out after {:?}", timeout);
                // Force close by marking as closed
                *self.is_closed.lock().await = true;
                Err(MarketMakerError::timeout(
                    "Stream close operation timed out",
                ))
            }
        }
    }

    /// Check if the stream is closed
    pub async fn is_closed(&self) -> bool {
        *self.is_closed.lock().await || self.quote_sender.is_closed()
    }

    /// Get connection statistics
    pub async fn get_stats(&self) -> ConnectionStats {
        self.stats.lock().await.clone()
    }

    /// Reset connection statistics
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.lock().await;
        *stats = ConnectionStats::new();
    }

    /// Stream updates using an async iterator
    pub fn updates(&mut self) -> QuoteUpdateStream<'_> {
        QuoteUpdateStream::new(self)
    }

    /// Wait for shutdown notification
    pub async fn wait_for_shutdown(&self) {
        self.shutdown.notified().await;
    }
}

/// Async iterator for quote updates
pub struct QuoteUpdateStream<'a> {
    handle: &'a mut QuoteStreamHandle,
}

impl<'a> QuoteUpdateStream<'a> {
    fn new(handle: &'a mut QuoteStreamHandle) -> Self {
        Self { handle }
    }

    /// Get the next update with graceful shutdown support
    pub async fn next(&mut self) -> Result<Option<QuoteUpdate>> {
        // Check if closed first
        if self.handle.is_closed().await {
            return Ok(None);
        }

        // Receive the next update
        self.handle.receive_update().await
    }

    /// Get the next update with timeout and shutdown support
    pub async fn next_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<Option<QuoteUpdate>> {
        // Check if closed first
        if self.handle.is_closed().await {
            return Ok(None);
        }

        // Receive the next update with timeout
        self.handle.receive_update_timeout(timeout).await
    }
}

/// Configuration for streaming behavior
#[derive(Debug, Clone)]
pub struct StreamConfig {
    /// Buffer size for the quote sending channel
    pub send_buffer_size: usize,
    /// Timeout for individual operations
    pub operation_timeout: std::time::Duration,
    /// Whether to automatically reconnect on errors
    pub auto_reconnect: bool,
    /// Maximum number of reconnection attempts
    pub max_reconnect_attempts: u32,
    /// Maximum duration of inactivity before considering connection unhealthy
    pub inactivity_timeout: std::time::Duration,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            send_buffer_size: 1000,
            operation_timeout: std::time::Duration::from_secs(30),
            auto_reconnect: false,
            max_reconnect_attempts: 3,
            inactivity_timeout: std::time::Duration::from_secs(120),
        }
    }
}

impl StreamConfig {
    /// Create a new stream configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the send buffer size
    pub fn with_send_buffer_size(mut self, size: usize) -> Self {
        self.send_buffer_size = size;
        self
    }

    /// Set the operation timeout
    pub fn with_operation_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.operation_timeout = timeout;
        self
    }

    /// Enable auto-reconnection
    pub fn with_auto_reconnect(mut self, max_attempts: u32) -> Self {
        self.auto_reconnect = true;
        self.max_reconnect_attempts = max_attempts;
        self
    }

    /// Set the inactivity timeout
    pub fn with_inactivity_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.inactivity_timeout = timeout;
        self
    }
}

/// Generic connection statistics for monitoring stream health
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub messages_sent: u64,
    pub updates_received: u64,
    pub errors_encountered: u64,
    pub reconnections: u64,
    pub connected_at: std::time::Instant,
    pub last_activity: Option<std::time::Instant>,
}

impl ConnectionStats {
    /// Create new connection statistics
    pub fn new() -> Self {
        Self {
            messages_sent: 0,
            updates_received: 0,
            errors_encountered: 0,
            reconnections: 0,
            connected_at: std::time::Instant::now(),
            last_activity: None,
        }
    }

    /// Record activity
    pub fn activity(&mut self) {
        self.last_activity = Some(std::time::Instant::now());
    }

    /// Record a sent message
    pub fn message_sent(&mut self) {
        self.messages_sent += 1;
        self.activity();
    }

    /// Record a received update
    pub fn update_received(&mut self) {
        self.updates_received += 1;
        self.activity();
    }

    /// Record an error
    pub fn error_encountered(&mut self) {
        self.errors_encountered += 1;
    }

    /// Record a reconnection
    pub fn reconnection(&mut self) {
        self.reconnections += 1;
    }

    /// Get time since last activity
    pub fn time_since_last_activity(&self) -> Option<std::time::Duration> {
        self.last_activity.map(|instant| instant.elapsed())
    }
}

/// Helper functions for working with quote updates
pub mod update_helpers {
    use super::*;

    /// Check if an update is a heartbeat/system message
    pub fn is_heartbeat(update: &QuoteUpdate) -> bool {
        // With proto2 required fields, we check for UNSPECIFIED type
        update.update_type == UpdateType::Unspecified as i32
    }

    /// Check if an update represents a new quote
    pub fn is_new_quote(update: &QuoteUpdate) -> bool {
        update.update_type == UpdateType::New as i32
    }

    /// Check if an update represents an updated quote
    pub fn is_updated_quote(update: &QuoteUpdate) -> bool {
        update.update_type == UpdateType::Updated as i32
    }

    /// Check if an update represents an expired quote
    pub fn is_expired_quote(update: &QuoteUpdate) -> bool {
        update.update_type == UpdateType::Expired as i32
    }

    /// Check if an update represents a rejected quote (validation/storage failure)
    pub fn is_rejected_quote(update: &QuoteUpdate) -> bool {
        update.update_type == UpdateType::Rejected as i32
    }

    /// Get the status message from a quote update (present on REJECTED updates)
    pub fn get_status_message(update: &QuoteUpdate) -> Option<&str> {
        update.status_message.as_deref()
    }

    /// Get a human-readable description of the update type
    pub fn update_type_description(update: &QuoteUpdate) -> &'static str {
        match update.update_type {
            x if x == UpdateType::New as i32 => "New Quote",
            x if x == UpdateType::Updated as i32 => "Updated Quote",
            x if x == UpdateType::Expired as i32 => "Expired Quote",
            _ => "System Message",
        }
    }
}

/// Helper functions for working with swap updates
pub mod swap_update_helpers {
    use super::*;

    /// Check if a swap update indicates connection is ready
    pub fn is_connection_ready(update: &SwapUpdate) -> bool {
        update.message_type == SwapMessageType::ConnectionReady as i32
    }

    /// Check if a swap update contains an available swap transaction
    pub fn is_swap_available(update: &SwapUpdate) -> bool {
        update.message_type == SwapMessageType::SwapAvailable as i32
    }

    /// Check if a swap update confirms a transaction was submitted
    pub fn is_transaction_confirmed(update: &SwapUpdate) -> bool {
        update.message_type == SwapMessageType::TransactionConfirmed as i32
    }

    /// Check if a swap update indicates an error occurred
    pub fn is_error(update: &SwapUpdate) -> bool {
        update.message_type == SwapMessageType::Error as i32
    }

    /// Get the swap UUID from an update (if present)
    pub fn get_swap_uuid(update: &SwapUpdate) -> Option<&str> {
        update.swap_uuid.as_deref()
    }

    /// Get the unsigned transaction from an update (if present)
    pub fn get_unsigned_transaction(update: &SwapUpdate) -> Option<&str> {
        update.unsigned_transaction.as_deref()
    }

    /// Get the transaction signature from an update (if present)
    pub fn get_transaction_signature(update: &SwapUpdate) -> Option<&str> {
        update.transaction_signature.as_deref()
    }

    /// Get the status message from an update (if present)
    pub fn get_status_message(update: &SwapUpdate) -> Option<&str> {
        update.status_message.as_deref()
    }

    /// Get a human-readable description of the swap update type
    pub fn update_type_description(update: &SwapUpdate) -> &'static str {
        match update.message_type {
            x if x == SwapMessageType::Ping as i32 => "Ping",
            x if x == SwapMessageType::Pong as i32 => "Pong",
            x if x == SwapMessageType::ConnectionReady as i32 => "Connection Ready",
            x if x == SwapMessageType::SwapAvailable as i32 => "Swap Available",
            x if x == SwapMessageType::TransactionConfirmed as i32 => "Transaction Confirmed",
            x if x == SwapMessageType::Error as i32 => "Error",
            _ => "Unknown Message Type",
        }
    }

    pub fn is_pong(update: &SwapUpdate) -> bool {
        update.message_type == SwapMessageType::Pong as i32
    }

    /// Extract swap details for processing (returns UUID and transaction if this is a swap available message)
    pub fn extract_swap_details(update: &SwapUpdate) -> Option<(&str, &str)> {
        if is_swap_available(update) {
            if let (Some(uuid), Some(transaction)) =
                (get_swap_uuid(update), get_unsigned_transaction(update))
            {
                return Some((uuid, transaction));
            }
        }
        None
    }

    /// Extract confirmation details (returns UUID and transaction signature if this is a confirmation message)
    pub fn extract_confirmation_details(update: &SwapUpdate) -> Option<(&str, &str)> {
        if is_transaction_confirmed(update) {
            if let (Some(uuid), Some(signature)) =
                (get_swap_uuid(update), get_transaction_signature(update))
            {
                return Some((uuid, signature));
            }
        }
        None
    }
}

/// Handle for managing a bidirectional gRPC swap stream
pub struct SwapStreamHandle {
    /// Sender for sending swaps to the gRPC stream
    swap_sender: tokio::sync::mpsc::UnboundedSender<MarketMakerSwap>,
    /// Direct gRPC stream receiver for incoming swap updates from the server
    pub update_receiver: Streaming<SwapUpdate>,
    /// Shutdown notifier
    shutdown: Arc<Notify>,
    /// Connection statistics
    stats: Arc<Mutex<SwapStats>>,
    /// Flag to track if the stream has been gracefully closed
    is_closed: Arc<Mutex<bool>>,
}

impl SwapStreamHandle {
    /// Create a new swap stream handle
    pub(crate) fn new(
        swap_sender: tokio::sync::mpsc::UnboundedSender<MarketMakerSwap>,
        update_receiver: Streaming<SwapUpdate>,
    ) -> Self {
        let shutdown = Arc::new(Notify::new());
        let stats = Arc::new(Mutex::new(SwapStats::new()));
        let is_closed = Arc::new(Mutex::new(false));

        Self {
            swap_sender,
            update_receiver,
            shutdown,
            stats,
            is_closed,
        }
    }

    /// Send a swap directly to the gRPC server
    pub async fn send_swap(&self, swap: MarketMakerSwap) -> Result<()> {
        if *self.is_closed.lock().await {
            return Err(MarketMakerError::streaming("Stream has been closed"));
        }

        self.swap_sender
            .send(swap)
            .map_err(|_| MarketMakerError::streaming("Failed to send swap - gRPC stream closed"))?;

        {
            let mut stats = self.stats.lock().await;
            stats.message_sent();
        }

        Ok(())
    }

    /// Receive the next swap update from the gRPC server
    pub async fn receive_update(&mut self) -> Result<Option<SwapUpdate>> {
        if *self.is_closed.lock().await {
            return Ok(None);
        }

        match self.update_receiver.message().await {
            Ok(Some(update)) => {
                {
                    let mut stats = self.stats.lock().await;
                    stats.update_received();
                }
                Ok(Some(update))
            }
            Ok(None) => {
                *self.is_closed.lock().await = true;
                Ok(None)
            }
            Err(e) => {
                {
                    let mut stats = self.stats.lock().await;
                    stats.error_encountered();
                }
                Err(MarketMakerError::Grpc(e))
            }
        }
    }

    /// Receive updates with a timeout
    pub async fn receive_update_timeout(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<Option<SwapUpdate>> {
        match tokio::time::timeout(timeout, self.receive_update()).await {
            Ok(result) => result,
            Err(_) => Err(MarketMakerError::timeout("Timed out waiting for update")),
        }
    }

    /// Check connection health based on activity
    pub async fn is_healthy(&self, config: &StreamConfig) -> bool {
        let stats = self.stats.lock().await;

        if let Some(duration) = stats.time_since_last_activity() {
            duration <= config.inactivity_timeout
        } else {
            // No activity yet, but connection is new
            stats.connected_at.elapsed() < config.inactivity_timeout
        }
    }

    /// Close the gRPC stream gracefully
    pub async fn close(&mut self) {
        tracing::info!("Initiating graceful swap stream shutdown");

        *self.is_closed.lock().await = true;

        drop(std::mem::replace(
            &mut self.swap_sender,
            tokio::sync::mpsc::unbounded_channel().0,
        ));

        self.shutdown.notify_waiters();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        tracing::info!("Swap stream shutdown completed");
    }

    /// Close the stream with a custom timeout
    pub async fn close_with_timeout(&mut self, timeout: std::time::Duration) -> Result<()> {
        match tokio::time::timeout(timeout, self.close()).await {
            Ok(_) => Ok(()),
            Err(_) => {
                tracing::warn!("Swap stream close timed out after {:?}", timeout);
                *self.is_closed.lock().await = true;
                Err(MarketMakerError::timeout(
                    "Swap stream close operation timed out",
                ))
            }
        }
    }

    /// Check if the stream is closed
    pub async fn is_closed(&self) -> bool {
        *self.is_closed.lock().await || self.swap_sender.is_closed()
    }

    /// Get connection statistics
    pub async fn get_stats(&self) -> SwapStats {
        self.stats.lock().await.clone()
    }

    /// Reset connection statistics
    pub async fn reset_stats(&self) {
        let mut stats = self.stats.lock().await;
        *stats = SwapStats::new();
    }
}

/// Type alias for SwapStats - uses the generic ConnectionStats
pub type SwapStats = ConnectionStats;
