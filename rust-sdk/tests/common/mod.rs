//! Shared configuration and helpers for the integration test suite.

use base64::prelude::*;
use bs58;
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use std::env;

// ---------------------------------------------------------------------------
// Well-known Solana mints
// ---------------------------------------------------------------------------

pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub const DEFAULT_GRPC_ENDPOINT: &str = "https://rfq-mm-edge-grpc.raccoons.dev";
pub const DEFAULT_ULTRA_API_BASE: &str = "https://preprod.ultra-api.jup.ag";

// ---------------------------------------------------------------------------
// Test configuration loaded from environment variables
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct TestConfig {
    /// gRPC service endpoint
    pub grpc_endpoint: String,
    /// Authentication token for gRPC calls
    pub auth_token: String,
    /// Base URL for the preprod Ultra API
    pub ultra_api_base: String,
    /// Input mint address (e.g. USDC)
    pub input_mint: String,
    /// Output mint address (e.g. SOL)
    pub output_mint: String,
    /// Taker public key (Solana address)
    pub taker: String,
    /// Taker keypair used for signing transactions
    pub keypair: Keypair,
}

impl TestConfig {
    /// Build configuration from environment variables.
    ///
    /// # Panics
    /// Panics when mandatory variables (`SOLANA_PRIVATE_KEY`, `MM_AUTH_TOKEN`) are missing.
    pub fn from_env() -> Self {
        let private_key_str = env::var("SOLANA_PRIVATE_KEY")
            .expect("SOLANA_PRIVATE_KEY env var is required (base58-encoded)");
        let bytes = bs58::decode(private_key_str.trim())
            .into_vec()
            .expect("SOLANA_PRIVATE_KEY is not valid base58");
        let keypair =
            Keypair::try_from(&bytes[..]).expect("SOLANA_PRIVATE_KEY is not a valid keypair");

        let taker = env::var("TAKER").unwrap_or_else(|_| keypair.pubkey().to_string());

        let auth_token =
            env::var("MM_AUTH_TOKEN").expect("MM_AUTH_TOKEN env var is required");

        Self {
            grpc_endpoint: env::var("GRPC_ENDPOINT")
                .unwrap_or_else(|_| DEFAULT_GRPC_ENDPOINT.to_string()),
            auth_token,
            ultra_api_base: env::var("ULTRA_API_BASE")
                .unwrap_or_else(|_| DEFAULT_ULTRA_API_BASE.to_string()),
            input_mint: env::var("INPUT_MINT").unwrap_or_else(|_| USDC_MINT.to_string()),
            output_mint: env::var("OUTPUT_MINT").unwrap_or_else(|_| SOL_MINT.to_string()),
            taker,
            keypair,
        }
    }
}

// ---------------------------------------------------------------------------
// Transaction signing helper
// ---------------------------------------------------------------------------

/// Decode a base64-encoded unsigned transaction, sign it with the given
/// keypair at the correct signature index, and return the base64-encoded
/// signed transaction.
pub fn sign_transaction(
    unsigned_tx_base64: &str,
    keypair: &Keypair,
) -> Result<String, Box<dyn std::error::Error>> {
    let tx_bytes = BASE64_STANDARD.decode(unsigned_tx_base64)?;

    let mut transaction: VersionedTransaction = bincode::deserialize(&tx_bytes)?;

    // Log some basic info
    let version = match &transaction.message {
        solana_sdk::message::VersionedMessage::V0(_) => "V0",
        solana_sdk::message::VersionedMessage::Legacy(_) => "Legacy",
    };
    println!("  Transaction version: {version}");
    println!("  Signatures count   : {}", transaction.signatures.len());

    // Find the signature index that corresponds to our pubkey.
    let account_keys = transaction.message.static_account_keys();
    let taker_pubkey = keypair.pubkey();
    let signer_index = account_keys
        .iter()
        .position(|key| *key == taker_pubkey)
        .ok_or_else(|| {
            format!(
                "Taker pubkey {} not found in transaction account keys: {:?}",
                taker_pubkey,
                account_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>()
            )
        })?;

    println!("  Taker pubkey       : {taker_pubkey}");
    println!("  Signing at index   : {signer_index}");

    let message_data = transaction.message.serialize();
    let signature = keypair.sign_message(&message_data);
    transaction.signatures[signer_index] = signature;

    let signed_tx_bytes = bincode::serialize(&transaction)?;
    let signed_tx_base64 = BASE64_STANDARD.encode(&signed_tx_bytes);

    println!("  Signed tx length   : {} bytes", signed_tx_bytes.len());

    Ok(signed_tx_base64)
}
