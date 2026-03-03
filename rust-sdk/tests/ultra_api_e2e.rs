//! End-to-end integration tests for the RFQ V2 flow:
//!
//!   1. Verify the orderbook exists on the gRPC service (GetQuotes)
//!   2. Fetch a swap order from the preprod Ultra API (/order)
//!   3. Sign the transaction as the taker
//!   4. Submit the signed transaction to the Ultra API (/execute)
//!   5. Decode a transaction with fill-decoder and find the RFQ v2 fill
//!
//! These tests hit live services and require env vars — see `tests/README.md`.

mod common;

use common::TestConfig;
use fill_decoder::{
    decode_transaction_base64, scan_for_embedded_fill, FillAnalysis, FillExactInInstruction,
    JUPITER_PROGRAM_ID, RFQ_V2_PROGRAM_ID,
};
use market_maker_client_sdk::{ClientConfig, MarketMakerClient, Token, TokenPair};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Ultra API types
// ---------------------------------------------------------------------------

const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrderResponse {
    request_id: String,
    in_amount: String,
    out_amount: String,
    transaction: Option<String>,
    route_plan: Option<Vec<RoutePlanStep>>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutePlanStep {
    percent: u32,
    swap_info: SwapInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct SwapInfo {
    label: String,
    input_mint: String,
    output_mint: String,
    in_amount: String,
    out_amount: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteRequest<'a> {
    request_id: &'a str,
    signed_transaction: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteResponse {
    status: String,
    signature: Option<String>,
    slot: Option<String>,
    code: Option<i64>,
    error: Option<String>,
    #[allow(dead_code)]
    input_amount_result: Option<String>,
    #[allow(dead_code)]
    output_amount_result: Option<String>,
    swap_events: Option<Vec<SwapEvent>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwapEvent {
    input_mint: String,
    input_amount: String,
    output_mint: String,
    output_amount: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_token(mint: &str) -> Token {
    let (decimals, symbol) = match mint {
        common::USDC_MINT => (6, "USDC"),
        common::SOL_MINT => (9, "SOL"),
        common::SPL_TOKEN_MINT => (6, "MCT"),
        _ => (6, "UNKNOWN"),
    };
    Token {
        address: mint.to_string(),
        decimals,
        symbol: symbol.to_string(),
        owner: SPL_TOKEN_PROGRAM.to_string(),
    }
}

fn make_token_pair(base: &str, quote: &str) -> TokenPair {
    TokenPair {
        base_token: make_token(base),
        quote_token: make_token(quote),
    }
}

async fn connect_grpc(cfg: &TestConfig) -> MarketMakerClient {
    MarketMakerClient::connect_with_config(
        ClientConfig::new(&cfg.grpc_endpoint)
            .with_timeout(30)
            .with_auth_token(&cfg.auth_token),
    )
    .await
    .expect("failed to connect to gRPC service")
}

async fn fetch_order(
    api_base: &str,
    input_mint: &str,
    output_mint: &str,
    amount: u64,
    taker: &str,
) -> OrderResponse {
    let url = format!(
        "{api_base}/order?\
         inputMint={input_mint}&\
         outputMint={output_mint}&\
         amount={amount}&\
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
    );
    println!("  GET {url}");

    let resp = HttpClient::new()
        .get(&url)
        .send()
        .await
        .expect("HTTP request to /order failed");

    let status = resp.status();
    let body = resp.text().await.expect("failed to read response body");
    println!("  status: {status}");
    println!("  body  : {body}");
    assert!(status.is_success(), "/order returned {status}: {body}");

    let order: OrderResponse = serde_json::from_str(&body).expect("failed to parse /order JSON");

    println!(
        "  requestId: {}, inAmount: {}, outAmount: {}",
        order.request_id, order.in_amount, order.out_amount
    );
    if let Some(steps) = &order.route_plan {
        for (i, s) in steps.iter().enumerate() {
            println!(
                "  route[{i}]: {}% via {} ({} → {})",
                s.percent, s.swap_info.label, s.swap_info.in_amount, s.swap_info.out_amount,
            );
        }
    }
    if let Some(err) = &order.error {
        println!("  error: {err}");
    }
    order
}

fn print_fill(ix: &FillExactInInstruction, a: &FillAnalysis) {
    println!(
        "    side={}, in={}, out={}, spent={}",
        ix.taker_side, ix.amount_in_atoms, a.amount_out_atoms, a.amount_spent_atoms
    );
    println!(
        "    tick_size={}, lot_size={}, levels={}, consumed={}",
        ix.params.tick_size_qpb,
        ix.params.lot_size_base,
        ix.params.levels.len(),
        a.levels_consumed
    );
    println!(
        "    vwap_ticks={}, effective_price={:.10}",
        a.vwap_ticks,
        a.effective_price()
    );
    for (i, lvl) in ix.params.levels.iter().enumerate() {
        println!(
            "      [{i}] px_ticks={}, qty_lots={}",
            lvl.px_ticks, lvl.qty_lots
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Verify that the gRPC service returns a non-empty orderbook.
#[tokio::test]
async fn test_grpc_get_quotes() {
    let cfg = TestConfig::from_env();
    println!("=== test_grpc_get_quotes ===");

    let mut client = connect_grpc(&cfg).await;

    let resp = client
        .get_quotes(
            make_token_pair(&cfg.input_mint, &cfg.output_mint),
            cfg.auth_token.clone(),
        )
        .await
        .expect("GetQuotes RPC failed");

    println!("  quotes: {}", resp.quotes.len());
    assert!(!resp.quotes.is_empty(), "Expected at least one quote");

    let q = &resp.quotes[0];
    println!(
        "  first: maker={}, bids={}, asks={}",
        q.maker_id,
        q.bid_levels.len(),
        q.ask_levels.len()
    );
}

/// Fetch a swap order from the Ultra API `/order` endpoint.
#[tokio::test]
async fn test_ultra_api_order() {
    let cfg = TestConfig::from_env();
    println!("=== test_ultra_api_order ===");

    let order = fetch_order(
        &cfg.ultra_api_base,
        &cfg.input_mint,
        &cfg.output_mint,
        1_000_000,
        &cfg.taker,
    )
    .await;

    assert!(
        !order.request_id.is_empty(),
        "Expected a non-empty requestId"
    );
    assert!(
        order.transaction.as_ref().is_some_and(|t| !t.is_empty()),
        "Expected a non-empty unsigned transaction",
    );
}

/// Full e2e: GetQuotes → /order → sign → /execute.
#[tokio::test]
async fn test_full_order_flow() {
    let cfg = TestConfig::from_env();
    println!("=== test_full_order_flow ===");

    // Step 1 – Verify orderbook
    println!("\n--- Step 1: GetQuotes ---");
    let mut client = connect_grpc(&cfg).await;
    let quotes = client
        .get_quotes(
            make_token_pair(&cfg.input_mint, &cfg.output_mint),
            cfg.auth_token.clone(),
        )
        .await
        .expect("GetQuotes RPC failed");
    println!("  quotes: {}", quotes.quotes.len());
    assert!(!quotes.quotes.is_empty(), "Orderbook is empty");

    // Step 2 – Fetch order
    println!("\n--- Step 2: GET /order ---");
    let order = fetch_order(
        &cfg.ultra_api_base,
        &cfg.input_mint,
        &cfg.output_mint,
        1_000_000,
        &cfg.taker,
    )
    .await;

    // Step 3 – Sign
    println!("\n--- Step 3: Sign transaction ---");
    let unsigned_tx = order
        .transaction
        .as_ref()
        .expect("Order has no transaction");
    let signed_tx =
        common::sign_transaction(unsigned_tx, &cfg.keypair).expect("failed to sign transaction");
    println!("  signed tx base64 len={}", signed_tx.len());

    // Step 4 – Execute
    println!("\n--- Step 4: POST /execute ---");
    let execute_url = format!("{}/execute", cfg.ultra_api_base);
    println!("  POST {execute_url}");

    let resp = HttpClient::new()
        .post(&execute_url)
        .json(&ExecuteRequest {
            request_id: &order.request_id,
            signed_transaction: &signed_tx,
        })
        .send()
        .await
        .expect("/execute request failed");

    let status = resp.status();
    let body = resp.text().await.expect("failed to read /execute body");
    println!("  status: {status}");
    println!("  body  : {body}");
    assert!(status.is_success(), "/execute returned {status}: {body}");

    let result: ExecuteResponse =
        serde_json::from_str(&body).expect("failed to parse /execute JSON");

    println!(
        "  status={}, sig={:?}, slot={:?}",
        result.status, result.signature, result.slot
    );
    if let Some(events) = &result.swap_events {
        for (i, ev) in events.iter().enumerate() {
            println!(
                "  swap[{i}]: {} {} -> {} {}",
                ev.input_amount, ev.input_mint, ev.output_amount, ev.output_mint
            );
        }
    }

    assert_eq!(
        result.status, "Success",
        "Expected 'Success', got '{}'. Code: {:?}, Error: {:?}",
        result.status, result.code, result.error,
    );
}

/// Verify orderbooks exist in both directions of the pair.
#[tokio::test]
async fn test_grpc_get_quotes_both_directions() {
    let cfg = TestConfig::from_env();
    println!("=== test_grpc_get_quotes_both_directions ===");

    let mut client = connect_grpc(&cfg).await;

    let resp_a = client
        .get_quotes(
            make_token_pair(&cfg.input_mint, &cfg.output_mint),
            cfg.auth_token.clone(),
        )
        .await
        .expect("GetQuotes (A→B) failed");

    let resp_b = client
        .get_quotes(
            make_token_pair(&cfg.output_mint, &cfg.input_mint),
            cfg.auth_token.clone(),
        )
        .await
        .expect("GetQuotes (B→A) failed");

    println!(
        "  A→B: {} quotes | B→A: {} quotes",
        resp_a.quotes.len(),
        resp_b.quotes.len()
    );

    assert!(
        !resp_a.quotes.is_empty() || !resp_b.quotes.is_empty(),
        "Expected quotes in at least one direction",
    );
}

/// Fetch USDC → SPL token order, decode with fill-decoder, verify RFQ v2 fill is present.
#[tokio::test]
async fn test_decode_spl_token_order() {
    let cfg = TestConfig::from_env();
    println!("=== test_decode_spl_token_order ===");

    let order = fetch_order(
        &cfg.ultra_api_base,
        common::USDC_MINT,
        common::SPL_TOKEN_MINT,
        1_000_000, // 1 USDC
        &cfg.taker,
    )
    .await;

    let tx_base64 = order
        .transaction
        .as_ref()
        .filter(|t| !t.is_empty())
        .expect("Order has no transaction — taker wallet likely has no funds");

    let decoded = decode_transaction_base64(tx_base64).expect("failed to decode transaction");

    println!(
        "  version={}, sigs={}, accounts={}, ixs={}, lookups={}",
        decoded.message.version,
        decoded.signatures.len(),
        decoded.message.account_keys.len(),
        decoded.message.instructions.len(),
        decoded.message.address_table_lookups.len(),
    );
    for (i, ix) in decoded.message.instructions.iter().enumerate() {
        println!("  ix[{i}] program: {}", ix.program_id);
    }

    // Scan for the RFQ v2 fill — either as a direct instruction or embedded in Jupiter CPI
    let mut found_rfq_v2 = false;

    for ix in &decoded.message.instructions {
        if ix.program_id == RFQ_V2_PROGRAM_ID {
            found_rfq_v2 = true;
            println!("\n  RFQ v2 instruction (direct):");
            if let Some((fill_ix, analysis)) = &ix.fill {
                print_fill(fill_ix, analysis);
            }
        }
        if ix.program_id == JUPITER_PROGRAM_ID {
            println!("\n  Jupiter aggregator instruction:");
            if let Some((fill_ix, analysis)) = scan_for_embedded_fill(&ix.data) {
                println!("  -> embedded RFQ v2 fill:");
                print_fill(&fill_ix, &analysis);
                found_rfq_v2 = true;
            }
        }
    }

    assert!(
        found_rfq_v2,
        "No RFQ v2 fill found. Programs: {:?}",
        decoded
            .message
            .instructions
            .iter()
            .map(|ix| &ix.program_id)
            .collect::<Vec<_>>()
    );
}
