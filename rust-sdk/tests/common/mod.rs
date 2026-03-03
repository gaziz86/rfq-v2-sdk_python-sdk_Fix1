//! Shared configuration and helpers for the integration test suite.

use base64::prelude::*;
use bs58;
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use std::env;

pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const SPL_TOKEN_MINT: &str = "A3QAoKnf3jFcCfTGvEpE7KVBMZqXQJwvwt6Uc4UExkDp";

pub const DEFAULT_ULTRA_API_BASE: &str = "https://preprod.ultra-api.jup.ag";

#[derive(Debug)]
pub struct TestConfig {
    /// Base URL for the preprod Ultra API
    pub ultra_api_base: String,
    /// Input mint address (default: USDC)
    pub input_mint: String,
    /// Output mint address (default: SPL_TOKEN_MINT)
    pub output_mint: String,
    /// Taker public key (Solana address)
    pub taker: String,
    /// Taker keypair for signing (only when `SOLANA_PRIVATE_KEY` is set)
    pub keypair: Option<Keypair>,
}

impl TestConfig {
    /// Build configuration from environment variables.
    pub fn from_env() -> Self {
        let keypair = env::var("SOLANA_PRIVATE_KEY").ok().map(|pk| {
            let bytes = bs58::decode(pk.trim())
                .into_vec()
                .expect("SOLANA_PRIVATE_KEY is not valid base58");
            Keypair::try_from(&bytes[..]).expect("SOLANA_PRIVATE_KEY is not a valid keypair")
        });

        let taker = env::var("TAKER").unwrap_or_else(|_| {
            keypair
                .as_ref()
                .expect("Either TAKER or SOLANA_PRIVATE_KEY must be set")
                .pubkey()
                .to_string()
        });

        Self {
            ultra_api_base: env::var("ULTRA_API_BASE")
                .unwrap_or_else(|_| DEFAULT_ULTRA_API_BASE.to_string()),
            input_mint: env::var("INPUT_MINT").unwrap_or_else(|_| USDC_MINT.to_string()),
            output_mint: env::var("OUTPUT_MINT").unwrap_or_else(|_| SPL_TOKEN_MINT.to_string()),
            taker,
            keypair,
        }
    }

    /// Return the keypair, panicking with a clear message if not available.
    pub fn keypair(&self) -> &Keypair {
        self.keypair
            .as_ref()
            .expect("SOLANA_PRIVATE_KEY is required for this test")
    }
}

/// Decode a base64-encoded unsigned transaction, sign it with the given
/// keypair, and return the base64-encoded signed transaction.
pub fn sign_transaction(
    unsigned_tx_base64: &str,
    keypair: &Keypair,
) -> Result<String, Box<dyn std::error::Error>> {
    let tx_bytes = BASE64_STANDARD.decode(unsigned_tx_base64)?;
    let mut tx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;

    let account_keys = tx.message.static_account_keys();
    let taker_pubkey = keypair.pubkey();
    let signer_index = account_keys
        .iter()
        .position(|key| *key == taker_pubkey)
        .ok_or_else(|| {
            format!(
                "Taker pubkey {} not found in transaction account keys",
                taker_pubkey
            )
        })?;

    let signature = keypair.sign_message(&tx.message.serialize());
    tx.signatures[signer_index] = signature;

    Ok(BASE64_STANDARD.encode(bincode::serialize(&tx)?))
}
