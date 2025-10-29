//! Production-ready streaming example with Birdeye price feeds and volume-based pricing
//! Also demonstrates swap streaming with transaction signing
//! Uses integer arithmetic for precise financial calculations
mod helpers;
use crate::helpers::birdeye::BirdeyeClient;
use base64::prelude::*;
use bs58;
use market_maker_client_sdk::{
    streaming::{swap_update_helpers, update_helpers},
    ClientConfig, MarketMakerClient, MarketMakerQuote, MarketMakerSwap, StreamConfig,
};
use rand::Rng;
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn, Level};

#[derive(Debug)]
struct SolanaTokens;

impl SolanaTokens {
    const SOL: &'static str = "So11111111111111111111111111111111111111112"; // Wrapped SOL
}

// Precision constants
const PRICE_DECIMALS: u32 = 6; // 6 decimal places for USDC price (1 USDC = 1_000_000 units)
const SOL_DECIMALS: u32 = 9; // 9 decimal places for SOL (1 SOL = 1_000_000_000 lamports)
const PRICE_SCALE: u64 = 10_u64.pow(PRICE_DECIMALS);
const SOL_SCALE: u64 = 10_u64.pow(SOL_DECIMALS);
const BASIS_POINTS_SCALE: u64 = 10_000; // 10,000 basis points = 100%

/// Volume tiers for SOL trading with corresponding price adjustments in basis points
/// Format: (volume_in_lamports, markup_basis_points)
const VOLUME_TIERS: &[(u64, u64)] = &[
    (1 * SOL_SCALE, 0),      // 1 SOL - no markup (0 basis points)
    (10 * SOL_SCALE, 30),    // 10 SOL - 0.3% markup (30 basis points)
    (100 * SOL_SCALE, 80),   // 100 SOL - 0.8% markup (80 basis points)
    (1000 * SOL_SCALE, 150), // 1000 SOL - 1.5% markup (150 basis points)
    (5000 * SOL_SCALE, 250), // 5000 SOL - 2.5% markup (250 basis points)
];

async fn fetch_sol_price(
    birdeye_client: &BirdeyeClient,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let response = birdeye_client
        .fetch_current_price(SolanaTokens::SOL)
        .await?;

    if !response.success {
        return Err("Birdeye API returned success=false".into());
    }

    // Convert float price to integer (multiply by PRICE_SCALE for 6 decimal precision)
    let price_integer = (response.data.value * PRICE_SCALE as f64).round() as u64;
    Ok(price_integer)
}

fn calculate_volume_adjusted_price(base_price: u64, volume_lamports: u64, is_ask: bool) -> u64 {
    let markup_bp = VOLUME_TIERS
        .iter()
        .rev()
        .find(|(tier_volume, _)| volume_lamports >= *tier_volume)
        .map(|(_, markup)| *markup)
        .unwrap_or(0);

    let adjustment_bp = if is_ask { markup_bp } else { markup_bp / 2 };
    let adjustment = base_price.saturating_mul(adjustment_bp) / BASIS_POINTS_SCALE;

    if is_ask {
        base_price.saturating_add(adjustment)
    } else {
        base_price.saturating_sub(adjustment)
    }
}

/// Convert USDC amount (in integer format) to SOL volume in lamports
fn usdc_to_sol_volume(usdc_amount: u64, sol_price: u64) -> u64 {
    // usdc_amount is in scaled format, sol_price is in scaled format
    // Result should be in lamports (SOL_SCALE)
    // Use checked operations to prevent overflow
    if sol_price == 0 {
        return 0; // Prevent division by zero
    }

    // Check if multiplication would overflow
    match usdc_amount.checked_mul(SOL_SCALE) {
        Some(product) => product / sol_price,
        None => {
            // Overflow occurred, use alternative calculation
            // (usdc_amount / sol_price) * SOL_SCALE
            // This gives less precision but prevents overflow
            (usdc_amount / sol_price).saturating_mul(SOL_SCALE)
        }
    }
}

/// Calculate the exact price deviation for a given USDC input amount
fn calculate_price_deviation_for_usdc(usdc_amount: u64, sol_price: u64) -> (u64, u64, u64) {
    let volume_lamports = usdc_to_sol_volume(usdc_amount, sol_price);
    let spread_bp = get_spread_bp(volume_lamports);

    let bid_base = calculate_volume_adjusted_price(sol_price, volume_lamports, false);
    let ask_base = calculate_volume_adjusted_price(sol_price, volume_lamports, true);

    let bid_spread = bid_base.saturating_mul(spread_bp) / BASIS_POINTS_SCALE;
    let ask_spread = ask_base.saturating_mul(spread_bp) / BASIS_POINTS_SCALE;

    let final_bid = bid_base.saturating_sub(bid_spread);
    let final_ask = ask_base.saturating_add(ask_spread);

    (final_bid, final_ask, volume_lamports)
}

/// Get spread in basis points based on volume
fn get_spread_bp(volume_lamports: u64) -> u64 {
    if volume_lamports < 10 * SOL_SCALE {
        10
    } else if volume_lamports < 1000 * SOL_SCALE {
        20
    } else {
        30
    }
}

/// Convert integer price to display format
fn price_to_display(price: u64) -> String {
    let whole = price / PRICE_SCALE;
    let fractional = price % PRICE_SCALE;
    format!("{}.{:06}", whole, fractional)
}

/// Convert lamports to SOL display format
fn lamports_to_display(lamports: u64) -> String {
    let whole = lamports / SOL_SCALE;
    let fractional = lamports % SOL_SCALE;
    format!("{}.{:09}", whole, fractional)
}

/// Convert basis points to percentage display
fn basis_points_to_percentage(bp: u64) -> f64 {
    (bp as f64 / BASIS_POINTS_SCALE as f64) * 100.0
}

/// Helper to load environment variable with warning if not set
fn load_env_or_default(key: &str, default: &str, warn_msg: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| {
        warn!("{}", warn_msg);
        default.to_string()
    })
}

/// Load or generate a keypair for signing transactions
fn load_or_generate_keypair() -> Result<Keypair, Box<dyn std::error::Error>> {
    // Check if a private key string is provided via environment variable
    if let Ok(private_key_str) = std::env::var("SOLANA_PRIVATE_KEY") {
        info!("Loading keypair from SOLANA_PRIVATE_KEY environment variable");

        // Decode the base58 private key string
        let bytes = bs58::decode(private_key_str.trim()).into_vec()?;
        let keypair = Keypair::try_from(&bytes[..])?;

        info!("Loaded keypair with public key: {}", keypair.pubkey());
        Ok(keypair)
    } else {
        warn!("No keypair provided - generating a temporary keypair");
        warn!("Set SOLANA_PRIVATE_KEY (base58 string) or SOLANA_KEYPAIR_PATH (file) environment variable");
        let keypair = Keypair::new();
        info!("Generated temporary keypair: {}", keypair.pubkey());
        Ok(keypair)
    }
}

/// Process and sign an unsigned transaction (supports both legacy and V0 transactions)
fn process_and_sign_transaction(
    swap_uuid: &str,
    unsigned_tx_base64: &str,
    keypair: &Keypair,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    info!("Processing transaction for swap UUID: {}", swap_uuid);

    // Decode the base64 unsigned transaction
    let tx_bytes = BASE64_STANDARD.decode(unsigned_tx_base64)?;
    info!("Decoded transaction: {} bytes", tx_bytes.len());

    // Try to deserialize as VersionedTransaction (supports both legacy and V0)
    let mut transaction: VersionedTransaction = bincode::deserialize(&tx_bytes)?;
    info!(
        "Transaction deserialized successfully (version: {:?})",
        if matches!(
            transaction.message,
            solana_sdk::message::VersionedMessage::V0(_)
        ) {
            "V0"
        } else {
            "Legacy"
        }
    );

    // Validate the transaction before signing
    validate_versioned_transaction(&transaction)?;

    let message_data = transaction.message.serialize();
    let signature = keypair.sign_message(&message_data);
    transaction.signatures[1] = signature;

    // Serialize the signed transaction
    let signed_tx_bytes = bincode::serialize(&transaction)?;
    let signed_tx_base64 = BASE64_STANDARD.encode(&signed_tx_bytes);

    info!("Transaction signed and encoded successfully");

    Ok(signed_tx_base64)
}

/// Validate a versioned transaction before signing
fn validate_versioned_transaction(
    transaction: &VersionedTransaction,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Validating versioned transaction...");

    match &transaction.message {
        solana_sdk::message::VersionedMessage::Legacy(message) => {
            if message.instructions.is_empty() {
                return Err("Transaction has no instructions".into());
            }
            if message.account_keys.is_empty() {
                return Err("Transaction has no account keys".into());
            }
            info!("Transaction validation passed (Legacy)");
            info!("Instructions: {}", message.instructions.len());
            info!("Account keys: {}", message.account_keys.len());
            info!("Recent blockhash: {}", message.recent_blockhash);
        }
        solana_sdk::message::VersionedMessage::V0(message) => {
            if message.instructions.is_empty() {
                return Err("Transaction has no instructions".into());
            }
            if message.account_keys.is_empty() {
                return Err("Transaction has no account keys".into());
            }
            info!("Transaction validation passed (V0)");
            info!("Instructions: {}", message.instructions.len());
            info!("Account keys: {}", message.account_keys.len());
            info!(
                "Address lookup tables: {}",
                message.address_table_lookups.len()
            );
            info!("Recent blockhash: {}", message.recent_blockhash);
        }
    }

    Ok(())
}

/// Run the swap streaming loop
async fn run_swap_stream(
    mut swap_stream: market_maker_client_sdk::streaming::SwapStreamHandle,
    keypair: Keypair,
    stream_config: &StreamConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut swap_count = 0;
    let mut health_check_counter = 0;
    let mut last_ping_time = tokio::time::Instant::now();
    let ping_interval = Duration::from_secs(10);

    info!("Swap stream started with keep-alive monitoring");

    loop {
        // Send periodic pings to keep connection alive
        if last_ping_time.elapsed() >= ping_interval {
            let ping_message = MarketMakerSwap {
                message_type: market_maker_client_sdk::types::SwapMessageType::Ping as i32,
                swap_uuid: String::default(),
                signed_transaction: String::default(),
            };

            match swap_stream.send_swap(ping_message).await {
                Ok(_) => {
                    info!("Sent ping to server");
                    last_ping_time = tokio::time::Instant::now();
                }
                Err(e) => {
                    error!("Failed to send ping: {}", e);
                    break;
                }
            }
        }

        // Receive updates with timeout
        match tokio::time::timeout(
            Duration::from_millis(100),
            swap_stream.receive_update(),
        )
        .await
        {
            Ok(Ok(Some(swap_update))) => {
                health_check_counter += 1;

                // Handle different message types
                if swap_update_helpers::is_pong(&swap_update) {
                    info!("Received pong from server");
                    continue;
                }

                if swap_update_helpers::is_connection_ready(&swap_update) {
                    info!(
                        "Swap stream connection established: {}",
                        swap_update_helpers::get_status_message(&swap_update)
                            .unwrap_or("Ready")
                    );
                    continue;
                }

                if swap_update_helpers::is_error(&swap_update) {
                    error!(
                        "Swap stream error: {}",
                        swap_update_helpers::get_status_message(&swap_update)
                            .unwrap_or("Unknown error")
                    );
                    continue;
                }

                if swap_update_helpers::is_transaction_confirmed(&swap_update) {
                    if let Some((uuid, signature)) =
                        swap_update_helpers::extract_confirmation_details(&swap_update)
                    {
                        info!(
                            "Transaction confirmed - UUID: {}, Signature: {}",
                            uuid, signature
                        );
                    }
                    continue;
                }

                if swap_update_helpers::is_swap_available(&swap_update) {
                    if let Some((swap_uuid, unsigned_transaction)) =
                        swap_update_helpers::extract_swap_details(&swap_update)
                    {
                        swap_count += 1;
                        info!("Swap #{}: {}", swap_count, swap_uuid);

                        match process_and_sign_transaction(
                            swap_uuid,
                            unsigned_transaction,
                            &keypair,
                        ) {
                            Ok(signed_tx) => {
                                let market_maker_swap = MarketMakerSwap {
                                    message_type:
                                        market_maker_client_sdk::types::SwapMessageType::SwapSubmit
                                            as i32,
                                    swap_uuid: swap_uuid.to_string(),
                                    signed_transaction: signed_tx,
                                };

                                if let Err(e) = swap_stream.send_swap(market_maker_swap).await {
                                    error!("Failed to send signed tx: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to sign transaction: {}", e);
                            }
                        }
                    } else {
                        warn!("Received swap available message but missing swap details");
                    }
                } else {
                    info!(
                        "Received other swap update type: {}",
                        swap_update_helpers::update_type_description(&swap_update)
                    );
                }
            }
            Ok(Ok(None)) => {
                info!("Swap stream closed by server");
                break;
            }
            Ok(Err(e)) => {
                error!("Swap stream error: {}", e);
                break;
            }
            Err(_) => {
                // Timeout occurred, continue loop
            }
        }

        // Periodic health check
        if health_check_counter >= 10 {
            if !swap_stream.is_healthy(stream_config).await {
                warn!("Swap stream health check failed - possible connection issue");
            }
            health_check_counter = 0;
        }

        sleep(Duration::from_millis(50)).await;
    }

    info!("Swap stream completed: {} swaps processed", swap_count);

    // Display final statistics
    let final_stats = swap_stream.get_stats().await;
    info!(
        "Swap stats: {} sent, {} received, {} errors, uptime {:?}",
        final_stats.messages_sent,
        final_stats.updates_received,
        final_stats.errors_encountered,
        final_stats.connected_at.elapsed()
    );

    // Close stream
    if let Err(e) = swap_stream
        .close_with_timeout(Duration::from_secs(5))
        .await
    {
        warn!("Swap stream close error: {}", e);
    }

    Ok(())
}

/// Run the quote streaming loop
async fn run_quote_stream(
    mut stream: market_maker_client_sdk::streaming::QuoteStreamHandle,
    mut next_sequence: u64,
    maker_id: &str,
    maker_address: &str,
    birdeye_client: &BirdeyeClient,
    mut sol_price: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut quote_counter = 0;
    let mut price_refresh_counter = 0;
    let price_refresh_interval = 5;

    loop {
        // Refresh SOL price periodically
        if price_refresh_counter >= price_refresh_interval {
            if let Ok(new_price) = fetch_sol_price(birdeye_client).await {
                let price_diff = new_price.abs_diff(sol_price);
                if price_diff > PRICE_SCALE / 100 {
                    info!(
                        "Updated SOL price: ${} -> ${}",
                        price_to_display(sol_price),
                        price_to_display(new_price)
                    );
                    sol_price = new_price;
                }
            }
            price_refresh_counter = 0;
        }

        // Build quote with all volume tiers
        let mut quote_builder = MarketMakerQuote::builder()
            .maker_id(maker_id)
            .sol_usdc_pair()
            .sequence_number(next_sequence)
            .expiry_time_secs(60)
            .maker_address(maker_address.to_string())
            .lot_size_base(10u64.pow(3)); // Hardcoded lot size base, adjust as needed

        // Add randomness to base price (±0.5%)
        let mut rng = rand::thread_rng();
        let variance_bp = rng.gen_range(-50i64..=50i64);
        let adjustment = sol_price.saturating_mul(variance_bp.unsigned_abs()) / BASIS_POINTS_SCALE;
        let adjusted_base_price = if variance_bp >= 0 {
            sol_price.saturating_add(adjustment)
        } else {
            sol_price.saturating_sub(adjustment)
        };

        // Track price range for logging
        let mut min_bid_price = u64::MAX;
        let mut max_ask_price = 0u64;

        // Add all volume tiers
        for (volume_lamports, _) in VOLUME_TIERS {
            let spread_bp = get_spread_bp(*volume_lamports);
            let bid_price =
                calculate_volume_adjusted_price(adjusted_base_price, *volume_lamports, false);
            let ask_price =
                calculate_volume_adjusted_price(adjusted_base_price, *volume_lamports, true);

            let bid_spread = bid_price.saturating_mul(spread_bp) / BASIS_POINTS_SCALE;
            let ask_spread = ask_price.saturating_mul(spread_bp) / BASIS_POINTS_SCALE;

            let final_bid = bid_price.saturating_sub(bid_spread);
            let final_ask = ask_price.saturating_add(ask_spread);

            if final_bid < min_bid_price && final_bid > 0 {
                min_bid_price = final_bid;
            }
            if final_ask > max_ask_price {
                max_ask_price = final_ask;
            }

            quote_builder = quote_builder
                .bid_level(*volume_lamports, final_bid)
                .ask_level(*volume_lamports, final_ask);
        }

        // Send quote
        let quote = quote_builder.build()?;
        match stream.send_quote(quote).await {
            Ok(_) => {
                info!(
                    "Quote #{} sent (seq: {}) - {} levels, ${}-${}",
                    quote_counter + 1,
                    next_sequence,
                    VOLUME_TIERS.len(),
                    price_to_display(min_bid_price),
                    price_to_display(max_ask_price)
                );
                next_sequence += 1;
                quote_counter += 1;
            }
            Err(e) => {
                error!("Failed to send quote: {}", e);
                break;
            }
        }

        price_refresh_counter += 1;
        sleep(Duration::from_secs(10)).await;

        // Handle incoming updates
        while let Ok(Some(update)) = stream
            .receive_update_timeout(Duration::from_millis(100))
            .await
        {
            info!("Update: {}", update.update_type);
        }
    }

    // Drain remaining updates
    info!("Draining quote stream...");
    let drain_start = tokio::time::Instant::now();
    while drain_start.elapsed() < Duration::from_secs(3) {
        match stream
            .receive_update_timeout(Duration::from_millis(100))
            .await
        {
            Ok(Some(update)) => {
                info!(
                    "Draining: {}",
                    update_helpers::update_type_description(&update)
                );
            }
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    // Display final statistics
    let final_stats = stream.get_stats().await;
    info!(
        "Quote stats: {} sent, {} received, {} errors, uptime {:?}",
        final_stats.messages_sent,
        final_stats.updates_received,
        final_stats.errors_encountered,
        final_stats.connected_at.elapsed()
    );

    // Close stream
    if let Err(e) = stream.close_with_timeout(Duration::from_secs(5)).await {
        warn!("Stream close error: {}", e);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the default crypto provider for rustls
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    info!("Production Streaming Example - RFQv2 SDK");

    // Load or generate a keypair for transaction signing
    let keypair = load_or_generate_keypair()?;

    // Initialize Birdeye client for price fetching
    let birdeye_api_key = load_env_or_default(
        "BIRDEYE_API_KEY",
        "your_api_key",
        "BIRDEYE_API_KEY not set - using placeholder (API requests may fail)",
    );
    let birdeye_client = BirdeyeClient::new("https://public-api.birdeye.so", &birdeye_api_key);

    // Fetch initial SOL price from Birdeye
    let sol_price = match fetch_sol_price(&birdeye_client).await {
        Ok(price) => {
            info!("SOL price: ${}", price_to_display(price));

            // Demonstrate price deviation calculation for 1,000,000 USDC
            let usdc_amount = 1_000_000 * PRICE_SCALE;
            let (bid, ask, volume_lamports) =
                calculate_price_deviation_for_usdc(usdc_amount, price);

            let price_safe = price.max(1);
            let bid_deviation =
                (price.saturating_sub(bid)).saturating_mul(BASIS_POINTS_SCALE) / price_safe;
            let ask_deviation =
                (ask.saturating_sub(price)).saturating_mul(BASIS_POINTS_SCALE) / price_safe;
            let spread_bp =
                (ask.saturating_sub(bid)).saturating_mul(BASIS_POINTS_SCALE) / price_safe;

            info!(
                "Example 1M USDC: {} SOL, bid ${} (-{:.3}%), ask ${} (+{:.3}%), spread {:.3}%",
                lamports_to_display(volume_lamports),
                price_to_display(bid),
                basis_points_to_percentage(bid_deviation),
                price_to_display(ask),
                basis_points_to_percentage(ask_deviation),
                basis_points_to_percentage(spread_bp)
            );

            price
        }
        Err(e) => {
            warn!("Failed to fetch SOL price: {}. Using fallback $100.00", e);
            100 * PRICE_SCALE
        }
    };

    // Configure the client with production settings for HTTPS with HTTP/2 and ALPN
    info!("Connecting to RFQv2 service...");

    // Get authentication token from environment or use default
    let auth_token = load_env_or_default(
        "MM_AUTH_TOKEN",
        "production_jwt_token",
        "MM_AUTH_TOKEN not set - using default 'production_jwt_token'. Set MM_AUTH_TOKEN environment variable for production use",
    );

    let config = ClientConfig::new("https://rfq-mm-edge-grpc.raccoons.dev")
        .with_timeout(30)
        .with_max_retries(5)
        .with_auth_token(auth_token);

    let mut client = match MarketMakerClient::connect_with_config(config).await {
        Ok(client) => {
            info!("Connected successfully");
            client
        }
        Err(e) => {
            error!("Connection failed: {}", e);
            return Err(e.into());
        }
    };

    // Configure streaming with production settings
    let stream_config = StreamConfig::new()
        .with_send_buffer_size(10000)
        .with_operation_timeout(Duration::from_secs(30));

    // Start streaming with sequence synchronization
    // Note: maker_id is still passed for sequence tracking
    // auth_token is now configured in ClientConfig
    let maker_id = load_env_or_default(
        "MM_MAKER_ID",
        "production_maker",
        "MM_MAKER_ID not set - using default 'production_maker'",
    );

    info!("Starting quote streaming for maker: {}...", maker_id);
    let (stream, next_sequence) = match client
        .start_streaming_with_sync_and_config(
            maker_id.clone(),
            client.config().auth_token.clone().unwrap_or_default(),
            &stream_config,
        )
        .await
    {
        Ok((stream, seq)) => {
            info!("Quote streaming started (sequence: {})", seq);
            (stream, seq)
        }
        Err(e) => {
            error!("Failed to start streaming: {}", e);
            return Ok(());
        }
    };

    // Start swap streaming in background task
    let swap_handle = match client.start_swap_streaming().await {
        Ok(swap_stream) => {
            let keypair_clone = Keypair::try_from(&keypair.to_bytes()[..])?;
            let stream_config_clone = stream_config.clone();
            Some(tokio::spawn(async move {
                run_swap_stream(swap_stream, keypair_clone, &stream_config_clone).await
            }))
        }
        Err(e) => {
            warn!("Swap streaming failed: {}. Continuing with quotes only", e);
            None
        }
    };

    // Run quote streaming loop
    run_quote_stream(
        stream,
        next_sequence,
        &maker_id,
        &keypair.pubkey().to_string(),
        &birdeye_client,
        sol_price,
    )
    .await?;

    // Graceful shutdown - wait for swap handler to complete
    if let Some(handle) = swap_handle {
        match tokio::time::timeout(Duration::from_secs(10), handle).await {
            Ok(Ok(_)) => {
                info!("Swap handler completed successfully");
            }
            Ok(Err(e)) => warn!("Swap handler task error: {}", e),
            Err(_) => warn!("Swap handler timeout"),
        }
    }

    info!("Shutdown complete");
    Ok(())
}
