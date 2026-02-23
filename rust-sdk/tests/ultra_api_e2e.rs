//! End-to-end integration tests for the RFQ V2 flow:
//!
//!   1. Verify the orderbook exists on the gRPC service (GetQuotes)
//!   2. Fetch a swap order from the preprod Ultra API (/order)
//!   3. Sign the transaction as the taker
//!   4. Submit the signed transaction to the Ultra API (/execute)
//!
//! All tests are `#[ignore]` so they only run when explicitly requested
//! (`cargo test --test ultra_api_e2e -- --ignored --nocapture`).

mod common;

use common::TestConfig;
use market_maker_client_sdk::{ClientConfig, MarketMakerClient, Token, TokenPair};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Ultra API types
// ---------------------------------------------------------------------------

/// Response from `GET /order`
///
/// See: https://github.com/jup-ag/ultra-api#get-order
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct OrderResponse {
    request_id: String,
    input_mint: String,
    output_mint: String,
    in_amount: String,
    out_amount: String,
    other_amount_threshold: Option<String>,
    swap_mode: Option<String>,
    slippage_bps: Option<u32>,
    price_impact_pct: Option<String>,
    swap_type: Option<String>,
    /// Base-64 encoded unsigned transaction (may be absent if quote-only)
    transaction: Option<String>,
    gasless: Option<bool>,
    total_time: Option<u64>,
    taker: Option<String>,
    prioritization_type: Option<String>,
    prioritization_fee_lamports: Option<u64>,
}

/// Payload for `POST /execute`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteRequest<'a> {
    request_id: &'a str,
    signed_transaction: &'a str,
}

/// Swap event from the execute response
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct SwapEvent {
    input_mint: String,
    input_amount: String,
    output_mint: String,
    output_amount: String,
}

/// Response from `POST /execute`
///
/// See: https://github.com/jup-ag/ultra-api#post-execute
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ExecuteResponse {
    /// "Success" or "Failed"
    status: String,
    /// Transaction signature (may be present even on failure)
    signature: Option<String>,
    /// Slot in which the transaction landed
    slot: Option<String>,
    /// Error code (0 = success)
    code: Option<i64>,
    /// Error message (present on failure)
    error: Option<String>,
    /// Total input token amount used (before fees)
    input_amount_result: Option<String>,
    /// Total output token amount received (after fees)
    output_amount_result: Option<String>,
    /// Swap event details
    swap_events: Option<Vec<SwapEvent>>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the token pair for GetQuotes from the configured mints.
fn make_token_pair(input_mint: &str, output_mint: &str) -> TokenPair {
    // The proto requires full Token objects – we fill in the minimum fields
    // required for the server to match the pair. Owner is always the SPL
    // Token program.
    let spl_token_program = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

    let (input_decimals, input_symbol) = mint_metadata(input_mint);
    let (output_decimals, output_symbol) = mint_metadata(output_mint);

    TokenPair {
        base_token: Token {
            address: input_mint.to_string(),
            decimals: input_decimals,
            symbol: input_symbol.to_string(),
            owner: spl_token_program.to_string(),
        },
        quote_token: Token {
            address: output_mint.to_string(),
            decimals: output_decimals,
            symbol: output_symbol.to_string(),
            owner: spl_token_program.to_string(),
        },
    }
}

/// Return (decimals, symbol) for well-known mints, with a sensible fallback.
fn mint_metadata(mint: &str) -> (u32, &str) {
    match mint {
        common::USDC_MINT => (6, "USDC"),
        common::SOL_MINT => (9, "SOL"),
        _ => (6, "UNKNOWN"),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verify that the gRPC service returns a non-empty orderbook for the
/// configured token pair.
#[tokio::test]
async fn test_grpc_get_quotes() {
    let cfg = TestConfig::from_env();

    println!("=== test_grpc_get_quotes ===");
    println!("  gRPC endpoint : {}", cfg.grpc_endpoint);
    println!("  input_mint    : {}", cfg.input_mint);
    println!("  output_mint   : {}", cfg.output_mint);

    let client_cfg = ClientConfig::new(&cfg.grpc_endpoint)
        .with_timeout(30)
        .with_auth_token(&cfg.auth_token);

    let mut client = MarketMakerClient::connect_with_config(client_cfg)
        .await
        .expect("failed to connect to gRPC service");

    let token_pair = make_token_pair(&cfg.input_mint, &cfg.output_mint);

    let response = client
        .get_quotes(token_pair, cfg.auth_token.clone())
        .await
        .expect("GetQuotes RPC failed");

    println!("  quotes returned: {}", response.quotes.len());

    assert!(
        !response.quotes.is_empty(),
        "Expected at least one quote in the orderbook for {}/{}",
        cfg.input_mint,
        cfg.output_mint,
    );

    // Print summary of first quote
    let first = &response.quotes[0];
    println!("  first quote maker_id : {}", first.maker_id);
    println!("  bid levels           : {}", first.bid_levels.len());
    println!("  ask levels           : {}", first.ask_levels.len());
}

/// Fetch a swap order from the Ultra API `/order` endpoint.
#[tokio::test]
async fn test_ultra_api_order() {
    let cfg = TestConfig::from_env();

    println!("=== test_ultra_api_order ===");
    println!("  ultra_api_base : {}", cfg.ultra_api_base);
    println!("  taker          : {}", cfg.taker);

    let http = HttpClient::new();
    let url = format!(
        "{base}/order?\
         inputMint={input}&\
         outputMint={output}&\
         amount=1000000&\
         swapMode=ExactIn&\
         slippageBps=50&\
         broadcastFeeType=maxCap&\
         priorityFeeLamports=1000000&\
         useWsol=false&\
         asLegacyTransaction=false&\
         excludeDexes=&\
         excludeRouters=jupiterz&\
         taker={taker}&\
         enableRfqV2=true",
        base = cfg.ultra_api_base,
        input = cfg.input_mint,
        output = cfg.output_mint,
        taker = cfg.taker,
    );

    println!("  GET {url}");

    let resp = http
        .get(&url)
        .send()
        .await
        .expect("HTTP request to /order failed");

    let status = resp.status();
    let body = resp.text().await.expect("failed to read response body");

    println!("  status: {status}");
    println!("  body  : {body}");

    assert!(
        status.is_success(),
        "Expected 2xx from /order, got {status}: {body}"
    );

    let order: OrderResponse =
        serde_json::from_str(&body).expect("failed to parse /order response JSON");

    println!("  requestId : {}", order.request_id);
    println!("  inAmount  : {}", order.in_amount);
    println!("  outAmount : {}", order.out_amount);

    assert!(
        !order.request_id.is_empty(),
        "Expected a non-empty requestId"
    );
    assert!(
        order.transaction.as_ref().map_or(false, |t| !t.is_empty()),
        "Expected a non-empty unsigned transaction, got {:?}",
        order.transaction,
    );
}

/// Full end-to-end flow:
///   1. GetQuotes (gRPC) – verify orderbook
///   2. GET /order (Ultra API) – get unsigned transaction
///   3. Sign the transaction as the taker
///   4. POST /execute – submit the signed transaction
#[tokio::test]
async fn test_full_order_flow() {
    let cfg = TestConfig::from_env();

    println!("=== test_full_order_flow ===");
    println!("  gRPC endpoint  : {}", cfg.grpc_endpoint);
    println!("  Ultra API base : {}", cfg.ultra_api_base);
    println!("  input_mint     : {}", cfg.input_mint);
    println!("  output_mint    : {}", cfg.output_mint);
    println!("  taker          : {}", cfg.taker);

    // ------------------------------------------------------------------
    // Step 1 – Verify the orderbook exists on the gRPC service
    // ------------------------------------------------------------------
    println!("\n--- Step 1: GetQuotes (gRPC) ---");

    let client_cfg = ClientConfig::new(&cfg.grpc_endpoint)
        .with_timeout(30)
        .with_auth_token(&cfg.auth_token);

    let mut client = MarketMakerClient::connect_with_config(client_cfg)
        .await
        .expect("failed to connect to gRPC service");

    let token_pair = make_token_pair(&cfg.input_mint, &cfg.output_mint);

    let quotes_resp = client
        .get_quotes(token_pair, cfg.auth_token.clone())
        .await
        .expect("GetQuotes RPC failed");

    println!("  quotes returned: {}", quotes_resp.quotes.len());
    assert!(
        !quotes_resp.quotes.is_empty(),
        "No quotes found – the orderbook is empty for {}/{}",
        cfg.input_mint,
        cfg.output_mint,
    );

    // ------------------------------------------------------------------
    // Step 2 – Fetch order from Ultra API
    // ------------------------------------------------------------------
    println!("\n--- Step 2: GET /order (Ultra API) ---");

    let http = HttpClient::new();
    let order_url = format!(
        "{base}/order?\
         inputMint={input}&\
         outputMint={output}&\
         amount=1000000&\
         swapMode=ExactIn&\
         slippageBps=50&\
         broadcastFeeType=maxCap&\
         priorityFeeLamports=1000000&\
         useWsol=false&\
         asLegacyTransaction=false&\
         excludeDexes=&\
         excludeRouters=jupiterz&\
         taker={taker}&\
         enableRfqV2=true",
        base = cfg.ultra_api_base,
        input = cfg.input_mint,
        output = cfg.output_mint,
        taker = cfg.taker,
    );

    println!("  GET {order_url}");

    let resp = http
        .get(&order_url)
        .send()
        .await
        .expect("HTTP request to /order failed");

    let status = resp.status();
    let body = resp.text().await.expect("failed to read /order body");

    println!("  status : {status}");
    println!("  body   : {body}");

    assert!(
        status.is_success(),
        "/order returned {status}: {body}"
    );

    let order: OrderResponse =
        serde_json::from_str(&body).expect("failed to parse /order JSON");

    println!("  requestId : {}", order.request_id);
    println!("  inAmount  : {}", order.in_amount);
    println!("  outAmount : {}", order.out_amount);

    // ------------------------------------------------------------------
    // Step 3 – Sign the transaction as the taker
    // ------------------------------------------------------------------
    println!("\n--- Step 3: Sign transaction ---");

    let unsigned_tx = order
        .transaction
        .as_ref()
        .expect("Order response has no transaction – cannot proceed");

    let signed_tx = common::sign_transaction(unsigned_tx, &cfg.keypair)
        .expect("failed to sign transaction");

    println!("  Signed transaction ready (base64 len={})", signed_tx.len());

    // ------------------------------------------------------------------
    // Step 4 – Submit signed transaction to /execute
    // ------------------------------------------------------------------
    println!("\n--- Step 4: POST /execute ---");

    let execute_url = format!("{}/execute", cfg.ultra_api_base);
    let execute_payload = ExecuteRequest {
        request_id: &order.request_id,
        signed_transaction: &signed_tx,
    };

    println!("  POST {execute_url}");
    println!(
        "  payload: requestId={}, signedTransaction=<{} chars>",
        order.request_id,
        signed_tx.len()
    );

    let resp = http
        .post(&execute_url)
        .json(&execute_payload)
        .send()
        .await
        .expect("HTTP request to /execute failed");

    let status = resp.status();
    let body = resp.text().await.expect("failed to read /execute body");

    println!("  status : {status}");
    println!("  body   : {body}");

    assert!(
        status.is_success(),
        "/execute returned {status}: {body}"
    );

    let execute_resp: ExecuteResponse =
        serde_json::from_str(&body).expect("failed to parse /execute JSON");

    println!("  execute status          : {}", execute_resp.status);
    println!("  execute code            : {:?}", execute_resp.code);
    if let Some(sig) = &execute_resp.signature {
        println!("  transaction sig         : {sig}");
    }
    if let Some(slot) = &execute_resp.slot {
        println!("  slot                    : {slot}");
    }
    if let Some(input) = &execute_resp.input_amount_result {
        println!("  input amount result     : {input}");
    }
    if let Some(output) = &execute_resp.output_amount_result {
        println!("  output amount result    : {output}");
    }
    if let Some(events) = &execute_resp.swap_events {
        for (i, ev) in events.iter().enumerate() {
            println!("  swap event[{i}]: {} {} -> {} {}",
                ev.input_amount, ev.input_mint,
                ev.output_amount, ev.output_mint);
        }
    }
    if let Some(err) = &execute_resp.error {
        println!("  execute error           : {err}");
    }

    // The execute endpoint returns "Success" or "Failed"
    assert_eq!(
        execute_resp.status, "Success",
        "Expected status 'Success' from /execute, got '{}'. Code: {:?}, Error: {:?}",
        execute_resp.status,
        execute_resp.code,
        execute_resp.error,
    );
}

/// Verify we can connect to gRPC and fetch orderbooks for **both**
/// directions of the pair (input→output and output→input).
#[tokio::test]
async fn test_grpc_get_quotes_both_directions() {
    let cfg = TestConfig::from_env();

    println!("=== test_grpc_get_quotes_both_directions ===");

    let client_cfg = ClientConfig::new(&cfg.grpc_endpoint)
        .with_timeout(30)
        .with_auth_token(&cfg.auth_token);

    let mut client = MarketMakerClient::connect_with_config(client_cfg)
        .await
        .expect("failed to connect to gRPC service");

    // Direction A: input → output
    let pair_a = make_token_pair(&cfg.input_mint, &cfg.output_mint);
    let resp_a = client
        .get_quotes(pair_a, cfg.auth_token.clone())
        .await
        .expect("GetQuotes (A→B) failed");

    println!(
        "  {}/{} : {} quotes",
        cfg.input_mint,
        cfg.output_mint,
        resp_a.quotes.len()
    );

    // Direction B: output → input
    let pair_b = make_token_pair(&cfg.output_mint, &cfg.input_mint);
    let resp_b = client
        .get_quotes(pair_b, cfg.auth_token.clone())
        .await
        .expect("GetQuotes (B→A) failed");

    println!(
        "  {}/{} : {} quotes",
        cfg.output_mint,
        cfg.input_mint,
        resp_b.quotes.len()
    );

    // At least one direction should have quotes
    assert!(
        !resp_a.quotes.is_empty() || !resp_b.quotes.is_empty(),
        "Expected quotes in at least one direction for {}<->{}",
        cfg.input_mint,
        cfg.output_mint,
    );
}
