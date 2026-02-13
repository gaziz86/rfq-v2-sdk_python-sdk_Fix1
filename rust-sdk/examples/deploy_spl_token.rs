//! Example: Deploy a new SPL Token on Solana
//!
//! This example demonstrates how to create and deploy a new SPL token (mint),
//! create an associated token account, and mint an initial supply.
//!
//! # Prerequisites
//! - A funded Solana keypair (set via `SOLANA_KEYPAIR` env var as base58 or path to JSON file)
//! - An RPC endpoint (set via `SOLANA_RPC_URL` env var, defaults to devnet)
//!
//! # Usage
//! ```bash
//! SOLANA_KEYPAIR=<base58_private_key_or_path> \
//! SOLANA_RPC_URL=https://api.devnet.solana.com \
//! cargo run --example deploy_spl_token
//! ```

use base64::Engine;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use std::str::FromStr;
use tracing::{error, info, Level};

// --- SPL Token program constants ---

/// SPL Token program ID
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// SPL Associated Token Account program ID
const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// System program ID
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

/// SPL Token Mint account size in bytes
const MINT_SIZE: u64 = 82;

// --- Default token configuration ---

/// Token name (for display purposes only — not stored on-chain for basic SPL tokens)
const TOKEN_NAME: &str = "My Custom Token";

/// Token symbol
const TOKEN_SYMBOL: &str = "MCT";

/// Number of decimals for the token (6 is common for USDC-like tokens, 9 for SOL-like)
const TOKEN_DECIMALS: u8 = 6;

/// Initial supply to mint (in the smallest unit). 1_000_000 with 6 decimals = 1.0 token
const INITIAL_SUPPLY: u64 = 1_000_000_000_000; // 1,000,000 tokens

// --- SPL Token instruction builders ---

/// Build an `InitializeMint2` instruction (SPL Token instruction index = 20).
fn build_initialize_mint2_ix(
    mint: &Pubkey,
    mint_authority: &Pubkey,
    freeze_authority: Option<&Pubkey>,
    decimals: u8,
) -> Instruction {
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();

    // InitializeMint2 data layout:
    // [0]    = 20 (instruction index)
    // [1]    = decimals
    // [2..34] = mint_authority (32 bytes)
    // [34]   = option flag for freeze_authority (1 = Some, 0 = None)
    // [35..67] = freeze_authority (32 bytes, or zeros if None)
    let mut data = vec![20u8]; // InitializeMint2
    data.push(decimals);
    data.extend_from_slice(mint_authority.as_ref());
    match freeze_authority {
        Some(fa) => {
            data.push(1);
            data.extend_from_slice(fa.as_ref());
        }
        None => {
            data.push(0);
            data.extend_from_slice(&[0u8; 32]);
        }
    }

    Instruction {
        program_id: token_program,
        accounts: vec![solana_sdk::instruction::AccountMeta::new(*mint, false)],
        data,
    }
}

/// Build a `MintTo` instruction (SPL Token instruction index = 7).
fn build_mint_to_ix(
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
) -> Instruction {
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();

    // MintTo data layout:
    // [0]    = 7 (instruction index)
    // [1..9] = amount (u64 LE)
    let mut data = vec![7u8];
    data.extend_from_slice(&amount.to_le_bytes());

    Instruction {
        program_id: token_program,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(*mint, false),
            solana_sdk::instruction::AccountMeta::new(*destination, false),
            solana_sdk::instruction::AccountMeta::new_readonly(*authority, true),
        ],
        data,
    }
}

/// Derive the Associated Token Account address for a given wallet and mint.
fn get_associated_token_address(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    let ata_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();

    let seeds = &[wallet.as_ref(), token_program.as_ref(), mint.as_ref()];

    Pubkey::find_program_address(seeds, &ata_program).0
}

/// Build a `CreateAssociatedTokenAccount` instruction.
fn build_create_ata_ix(payer: &Pubkey, wallet: &Pubkey, mint: &Pubkey) -> Instruction {
    let ata_program = Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM_ID).unwrap();
    let token_program = Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap();
    let system_program = Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap();

    let ata = get_associated_token_address(wallet, mint);

    Instruction {
        program_id: ata_program,
        accounts: vec![
            solana_sdk::instruction::AccountMeta::new(*payer, true), // funding account
            solana_sdk::instruction::AccountMeta::new(ata, false),   // ATA to create
            solana_sdk::instruction::AccountMeta::new_readonly(*wallet, false), // wallet
            solana_sdk::instruction::AccountMeta::new_readonly(*mint, false), // mint
            solana_sdk::instruction::AccountMeta::new_readonly(system_program, false),
            solana_sdk::instruction::AccountMeta::new_readonly(token_program, false),
        ],
        data: vec![], // CreateAssociatedTokenAccount has no data
    }
}

// --- RPC helpers ---

/// Minimal JSON-RPC client for Solana
struct SolanaRpcClient {
    client: reqwest::Client,
    url: String,
}

impl SolanaRpcClient {
    fn new(url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: url.to_string(),
        }
    }

    async fn get_latest_blockhash(
        &self,
    ) -> Result<solana_sdk::hash::Hash, Box<dyn std::error::Error>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [{ "commitment": "finalized" }]
        });

        let resp: serde_json::Value = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let blockhash_str = resp["result"]["value"]["blockhash"]
            .as_str()
            .ok_or("Failed to get blockhash from response")?;

        Ok(solana_sdk::hash::Hash::from_str(blockhash_str)?)
    }

    async fn get_minimum_balance_for_rent_exemption(
        &self,
        data_len: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getMinimumBalanceForRentExemption",
            "params": [data_len]
        });

        let resp: serde_json::Value = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let lamports = resp["result"]
            .as_u64()
            .ok_or("Failed to get rent exemption from response")?;

        Ok(lamports)
    }

    async fn send_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let serialized = bincode::serialize(tx)?;
        let encoded = base64::prelude::BASE64_STANDARD.encode(&serialized);

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                encoded,
                {
                    "encoding": "base64",
                    "skipPreflight": false,
                    "preflightCommitment": "confirmed"
                }
            ]
        });

        let resp: serde_json::Value = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.get("error") {
            return Err(format!("RPC error: {}", err).into());
        }

        let signature = resp["result"]
            .as_str()
            .ok_or("Failed to get signature from response")?
            .to_string();

        Ok(signature)
    }

    async fn confirm_transaction(
        &self,
        signature: &str,
        max_retries: u32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        for attempt in 1..=max_retries {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getSignatureStatuses",
                "params": [[signature]]
            });

            let resp: serde_json::Value = self
                .client
                .post(&self.url)
                .json(&body)
                .send()
                .await?
                .json()
                .await?;

            if let Some(status) = resp["result"]["value"][0].as_object() {
                if status.get("err").map_or(false, |e| !e.is_null()) {
                    error!("Transaction failed with error: {:?}", status["err"]);
                    return Ok(false);
                }

                let confirmation = status
                    .get("confirmationStatus")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if confirmation == "confirmed" || confirmation == "finalized" {
                    info!(
                        "Transaction confirmed (attempt {}/{}): status = {}",
                        attempt, max_retries, confirmation
                    );
                    return Ok(true);
                }
            }

            info!(
                "Waiting for confirmation (attempt {}/{})...",
                attempt, max_retries
            );
        }

        Ok(false)
    }
}

// --- Keypair loading ---

/// Load a Solana keypair from either a base58 private key or a JSON file path.
fn load_keypair(value: &str) -> Result<Keypair, Box<dyn std::error::Error>> {
    // Try loading as a JSON file path first
    if let Ok(contents) = std::fs::read_to_string(value) {
        let bytes: Vec<u8> = serde_json::from_str(&contents)?;
        return Ok(Keypair::try_from(&bytes[..])?);
    }

    // Fall back to base58 decoding
    let bytes = bs58::decode(value).into_vec()?;
    Ok(Keypair::try_from(&bytes[..])?)
}

// --- Main ---

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();

    // Load configuration from environment
    let rpc_url =
        std::env::var("SOLANA_RPC_URL").unwrap_or_else(|_| "https://api.devnet.solana.com".into());
    let keypair_str = std::env::var("SOLANA_KEYPAIR")
        .expect("SOLANA_KEYPAIR env var is required (base58 private key or path to JSON file)");

    let payer = load_keypair(&keypair_str)?;
    let rpc = SolanaRpcClient::new(&rpc_url);

    info!("=== SPL Token Deployment ===");
    info!("RPC endpoint : {}", rpc_url);
    info!("Payer wallet : {}", payer.pubkey());
    info!("Token name   : {} ({})", TOKEN_NAME, TOKEN_SYMBOL);
    info!("Decimals     : {}", TOKEN_DECIMALS);
    info!(
        "Initial supply: {} (raw: {} smallest units)",
        INITIAL_SUPPLY as f64 / 10_f64.powi(TOKEN_DECIMALS as i32),
        INITIAL_SUPPLY
    );

    // ---------------------------------------------------------------
    // Step 1: Create the Mint account
    // ---------------------------------------------------------------
    info!("\n--- Step 1: Creating mint account ---");

    let mint_keypair = Keypair::new();
    let mint_pubkey = mint_keypair.pubkey();
    info!("New mint address: {}", mint_pubkey);

    let rent_exemption = rpc
        .get_minimum_balance_for_rent_exemption(MINT_SIZE)
        .await?;
    info!(
        "Rent-exempt minimum: {} lamports ({:.6} SOL)",
        rent_exemption,
        rent_exemption as f64 / 1e9
    );

    let token_program_id = Pubkey::from_str(TOKEN_PROGRAM_ID)?;

    let create_mint_account_ix = system_instruction::create_account(
        &payer.pubkey(),
        &mint_pubkey,
        rent_exemption,
        MINT_SIZE,
        &token_program_id,
    );

    let init_mint_ix = build_initialize_mint2_ix(
        &mint_pubkey,
        &payer.pubkey(),       // mint authority
        Some(&payer.pubkey()), // freeze authority (optional)
        TOKEN_DECIMALS,
    );

    let blockhash = rpc.get_latest_blockhash().await?;

    let create_mint_tx = Transaction::new_signed_with_payer(
        &[create_mint_account_ix, init_mint_ix],
        Some(&payer.pubkey()),
        &[&payer, &mint_keypair],
        blockhash,
    );

    info!("Sending create-mint transaction...");
    let sig = rpc.send_transaction(&create_mint_tx).await?;
    info!("Transaction signature: {}", sig);

    let confirmed = rpc.confirm_transaction(&sig, 15).await?;
    if !confirmed {
        error!("Failed to confirm mint creation transaction");
        return Err("Mint creation not confirmed".into());
    }
    info!("✅ Mint account created successfully!");

    // ---------------------------------------------------------------
    // Step 2: Create an Associated Token Account (ATA)
    // ---------------------------------------------------------------
    info!("\n--- Step 2: Creating associated token account ---");

    let ata = get_associated_token_address(&payer.pubkey(), &mint_pubkey);
    info!("Associated token account: {}", ata);

    let create_ata_ix = build_create_ata_ix(&payer.pubkey(), &payer.pubkey(), &mint_pubkey);

    let blockhash = rpc.get_latest_blockhash().await?;

    let create_ata_tx = Transaction::new_signed_with_payer(
        &[create_ata_ix],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    info!("Sending create-ATA transaction...");
    let sig = rpc.send_transaction(&create_ata_tx).await?;
    info!("Transaction signature: {}", sig);

    let confirmed = rpc.confirm_transaction(&sig, 15).await?;
    if !confirmed {
        error!("Failed to confirm ATA creation transaction");
        return Err("ATA creation not confirmed".into());
    }
    info!("✅ Associated token account created!");

    // ---------------------------------------------------------------
    // Step 3: Mint initial supply
    // ---------------------------------------------------------------
    info!("\n--- Step 3: Minting initial supply ---");

    let mint_to_ix = build_mint_to_ix(&mint_pubkey, &ata, &payer.pubkey(), INITIAL_SUPPLY);

    let blockhash = rpc.get_latest_blockhash().await?;

    let mint_to_tx = Transaction::new_signed_with_payer(
        &[mint_to_ix],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    info!("Sending mint-to transaction...");
    let sig = rpc.send_transaction(&mint_to_tx).await?;
    info!("Transaction signature: {}", sig);

    let confirmed = rpc.confirm_transaction(&sig, 15).await?;
    if !confirmed {
        error!("Failed to confirm mint-to transaction");
        return Err("Mint-to not confirmed".into());
    }
    info!("✅ Initial supply minted!");

    // ---------------------------------------------------------------
    // Summary
    // ---------------------------------------------------------------
    info!("\n========================================");
    info!("  🎉 SPL Token Deployed Successfully!");
    info!("========================================");
    info!("  Mint address     : {}", mint_pubkey);
    info!("  Token account    : {}", ata);
    info!("  Mint authority   : {}", payer.pubkey());
    info!("  Freeze authority : {}", payer.pubkey());
    info!("  Decimals         : {}", TOKEN_DECIMALS);
    info!(
        "  Total supply     : {}",
        INITIAL_SUPPLY as f64 / 10_f64.powi(TOKEN_DECIMALS as i32)
    );
    info!("========================================");
    info!(
        "  Explorer: https://explorer.solana.com/address/{}?cluster=devnet",
        mint_pubkey
    );
    info!("========================================");

    Ok(())
}
