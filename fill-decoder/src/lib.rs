//! # fill-decoder
//!
//! Decoder and analysis utilities for RFQ v2 `fill_exact_in` transactions on Solana.

pub mod analysis;
pub mod decode;
pub mod error;
pub mod scanner;
pub mod transaction;
pub mod types;
pub use error::{FillDecoderError, Result};

pub use types::{
    FillAccounts, FillAnalysis, FillExactInInstruction, FillExactInParams, Level, Side,
};

pub use decode::{
    decode_fill_accounts, decode_fill_instruction, is_fill_exact_in, FILL_ACCOUNT_LABELS,
    FILL_EXACT_IN_ACCOUNT_COUNT, FILL_EXACT_IN_DISCRIMINATOR, RFQ_V2_PROGRAM_ID,
};

pub use analysis::analyze_fill;

pub use scanner::scan_for_embedded_fill;

pub use transaction::{
    decode_message_base64, decode_transaction_base64, decode_transaction_bytes, AddressTableLookup,
    DecodedInstruction, DecodedMessage, DecodedTransaction, MessageHeader, MessageVersion,
    ResolvedAccount,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Build minimal fill_exact_in instruction data for testing.
    fn build_instruction_data(
        side: Side,
        amount_in: u64,
        expire_at: u64,
        tick_size_qpb: u64,
        lot_size_base: u64,
        levels: &[(u64, u64)],
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&FILL_EXACT_IN_DISCRIMINATOR);
        data.push(side as u8);
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&expire_at.to_le_bytes());
        data.extend_from_slice(&tick_size_qpb.to_le_bytes());
        data.extend_from_slice(&lot_size_base.to_le_bytes());
        data.extend_from_slice(&(levels.len() as u32).to_le_bytes());
        for (px, qty) in levels {
            data.extend_from_slice(&px.to_le_bytes());
            data.extend_from_slice(&qty.to_le_bytes());
        }
        data
    }

    #[test]
    fn test_discriminator_check() {
        let data = build_instruction_data(Side::Bid, 0, 0, 1, 1, &[]);
        assert!(is_fill_exact_in(&data));
        assert!(!is_fill_exact_in(&[0u8; 8]));
        assert!(!is_fill_exact_in(&[0u8; 4]));
    }

    #[test]
    fn test_decode_roundtrip() {
        let levels = vec![(100, 50), (105, 30)];
        let data = build_instruction_data(Side::Ask, 1_000_000, 999, 1_000, 1_000_000, &levels);

        let ix = decode_fill_instruction(&data).unwrap();
        assert_eq!(ix.taker_side, Side::Ask);
        assert_eq!(ix.amount_in_atoms, 1_000_000);
        assert_eq!(ix.params.expire_at, 999);
        assert_eq!(ix.params.tick_size_qpb, 1_000);
        assert_eq!(ix.params.lot_size_base, 1_000_000);
        assert_eq!(ix.params.levels.len(), 2);
        assert_eq!(ix.params.levels[0].px_ticks, 100);
        assert_eq!(ix.params.levels[0].qty_lots, 50);
        assert_eq!(ix.params.levels[1].px_ticks, 105);
        assert_eq!(ix.params.levels[1].qty_lots, 30);
    }

    #[test]
    fn test_decode_accounts() {
        let keys: Vec<[u8; 32]> = (0..11).map(|i| [i as u8; 32]).collect();
        let accs = decode_fill_accounts(&keys).unwrap();
        assert_eq!(accs.user, [0u8; 32]);
        assert_eq!(accs.fill_authority, [1u8; 32]);
        assert_eq!(accs.maker_base_token_account, [4u8; 32]);
        assert_eq!(accs.quote_mint, [7u8; 32]);
    }

    #[test]
    fn test_decode_accounts_too_few() {
        let keys: Vec<[u8; 32]> = (0..5).map(|i| [i as u8; 32]).collect();
        assert!(decode_fill_accounts(&keys).is_err());
    }

    // SOL/USDC example: lot_size_base = 1 (raw), tick_size_qpb = 1
    // Taker buys SOL with 500 USDC atoms worth, best ask px_ticks = 100
    // price_per_lot = 100 * 1 = 100 quote-atoms per lot
    // affordable lots = 500 / 100 = 5
    // base out = 5 * 1 = 5 base-atoms
    #[test]
    fn test_analyze_bid_single_level() {
        let data = build_instruction_data(
            Side::Bid,
            500,  // 500 quote-atoms in
            9999, // expire_at
            1,    // tick_size_qpb
            1,    // lot_size_base (raw)
            &[(100, 10)],
        );
        let ix = decode_fill_instruction(&data).unwrap();
        let analysis = analyze_fill(&ix).unwrap();

        assert_eq!(analysis.taker_side, Side::Bid);
        assert_eq!(analysis.amount_spent_atoms, 500);
        assert_eq!(analysis.amount_out_atoms, 5);
        assert_eq!(analysis.total_lots_filled, 5);
        assert_eq!(analysis.vwap_ticks, 100);
        assert_eq!(analysis.levels_consumed, 1);
    }

    // Multi-level bid: 2 ask levels at different prices
    #[test]
    fn test_analyze_bid_multi_level() {
        // 1000 quote-atoms, two levels:
        //   level 0: px=100, qty=5  → spend 500, get 5 lots
        //   level 1: px=200, qty=5  → spend 400 (afford 2), get 2 lots
        // total spent = 900, remaining = 100, out = 7 lots = 7 atoms
        let data = build_instruction_data(
            Side::Bid,
            1000,
            9999,
            1, // tick_size_qpb
            1, // lot_size_base
            &[(100, 5), (200, 5)],
        );
        let ix = decode_fill_instruction(&data).unwrap();
        let analysis = analyze_fill(&ix).unwrap();

        assert_eq!(analysis.amount_spent_atoms, 900);
        assert_eq!(analysis.amount_out_atoms, 7);
        assert_eq!(analysis.total_lots_filled, 7);
        assert_eq!(analysis.levels_consumed, 2);
        // VWAP = (100*5 + 200*2) / 7 = 900/7 = 128 (integer division)
        assert_eq!(analysis.vwap_ticks, 128);
    }

    // Ask side: taker sells base, receives quote
    #[test]
    fn test_analyze_ask_single_level() {
        // Taker sells 10 base-atoms, lot_size = 2, so 5 lots available
        // Bid level: px=50, qty=10 → take 5 lots
        // quote out = 5 * (50 * 1) = 250
        let data = build_instruction_data(
            Side::Ask,
            10,   // 10 base-atoms in
            9999, // expire_at
            1,    // tick_size_qpb
            2,    // lot_size_base
            &[(50, 10)],
        );
        let ix = decode_fill_instruction(&data).unwrap();
        let analysis = analyze_fill(&ix).unwrap();

        assert_eq!(analysis.taker_side, Side::Ask);
        assert_eq!(analysis.amount_spent_atoms, 10);
        assert_eq!(analysis.amount_out_atoms, 250);
        assert_eq!(analysis.total_lots_filled, 5);
        assert_eq!(analysis.vwap_ticks, 50);
    }

    #[test]
    fn test_effective_price_bid() {
        let data = build_instruction_data(Side::Bid, 500, 9999, 1, 1, &[(100, 10)]);
        let ix = decode_fill_instruction(&data).unwrap();
        let analysis = analyze_fill(&ix).unwrap();

        // Paid 500 quote-atoms for 5 base-atoms → price = 100.0
        assert!((analysis.effective_price() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_display() {
        let data = build_instruction_data(Side::Bid, 500, 9999, 1, 1, &[(100, 10)]);
        let ix = decode_fill_instruction(&data).unwrap();
        let analysis = analyze_fill(&ix).unwrap();
        let s = format!("{}", analysis);
        assert!(s.contains("Bid"));
        assert!(s.contains("vwap_ticks: 100"));
    }

    // ---- Transaction / Message decoding tests ----

    /// Real transaction from Solana mainnet containing a fill_exact_in instruction.
    const REAL_TX_BASE64: &str = "AsPqw9SAB7rMKDuWgFVxTnfagAj/mSIwuKrYVM3csciSD2HOcJfht8nYL9sARghcVsJlxtTT0uaudrmCDEV1PwkAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAIBBg7KTUteDZUF9eqHCzJvdWHUARq8NQU4DIdyCSngydvcb3bk5le5izsp4223iXZnE4grlusUmzXQL4KHUihpkIa4ZNs5dmgwxMyL/IfP1O5Ac9iAQLnbqGpcsdM0PwusuV3sGjl7eLbP6JfIktH4I1aEAyciR2HwKu4QXwEGaOhOz2Hiyz7h80+tj7g8An6p3AGnu96N6DCinehLp7TnorlL9sMrLW9QqpZr3Vb5mV0mnsAM1mxk/2i/SD2e+0t5s5TXDN1sPYOMmaxV5QajzizK3Ud8JdMKkPML/GYipWTOIZRVvlKNhT2MVZBGmyT2HRRgChORTrPsbIfA4a0nvrlLAwZGb+UhFzL/7K26csOb57yM5bvF9xJrLEObOkAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAR51VvyMcBu7nTFbs5oFQf9sbLeo/SOUQKxzaJWvBOPBt324ddloZPZy+FGzut5rBy0he1fWzeROoz1hX7/AKmMlyWPTiSJ8bs9ECkUjg2DC1oTmdr/EIQEjnvY2+n4WQnk1I8BnjipjWjfEIVY5ELgMj6qznalm0dryLHTf7MuPFxhbWYWCgjj9eAH/Sz/1aYw9GQui88WwYxH3hd9dvYHCAAFAtmOAwAIAAkDzPEfAAAAAAAJAgACDAIAAADwSlABAAAAAAoFAgAZCwkJk/F7ZPSErnb/DAYAAwAdCQsBAQo0AAIDGR0LCwoaCg4NAAECBAUGGRsLCyAhACIWBwQXGBcYGAsgCh4AHw8HAxAREgsTFBUKHGO7ZPrMMcSvFAAtMQEAAAAAYHrfdQAAAABkAAoAAAADAAAAeAEsAAAAqG+UaQAAAAABAAAAAAAAAOgDAAAAAAAAAQAAAGMAAAAAAAAAQEIPAAAAAAAQJwABaAAQJwECGhAnAgMLAwIAAAEJAym/lQcqT78E33F1k+c4vMwhJygVwkcagNn59VWw1IQlASQEFwAoAV0T1DATNHXpX/sFnj+G3qAzHNFlFcd3JmJW5UXLSEmBB7Swr6yxXbIDrlmz4k8+KpZXHoZ2stouSFQQDE0nTzzoyEvg3OKGj8kaqe0DAQYEAwMCAA==";

    #[test]
    fn test_decode_real_transaction() {
        let tx = decode_transaction_base64(REAL_TX_BASE64).unwrap();

        // 2 signatures (one real, one placeholder)
        assert_eq!(tx.signatures.len(), 2);
        assert!(tx.signatures[0].starts_with("4vBpXi9zG"));

        let msg = &tx.message;
        assert_eq!(msg.version, MessageVersion::V0);
        assert_eq!(msg.header.num_required_signatures, 2);
        assert_eq!(msg.header.num_readonly_signed_accounts, 1);
        assert_eq!(msg.header.num_readonly_unsigned_accounts, 6);
        assert_eq!(msg.account_keys.len(), 14);

        // Should have multiple instructions
        assert!(msg.instructions.len() >= 2);

        // Find the instruction containing embedded fill_exact_in params
        // (called via CPI from Jupiter, not a standalone instruction)
        let fill_ix = msg
            .instructions
            .iter()
            .find(|ix| ix.fill.is_some())
            .expect("should find embedded fill_exact_in params in a Jupiter instruction");

        let (fill, analysis) = fill_ix.fill.as_ref().unwrap();

        // Verify decoded fill parameters
        assert!(fill.amount_in_atoms > 0);
        assert!(fill.params.tick_size_qpb > 0);
        assert!(fill.params.lot_size_base > 0);
        assert!(!fill.params.levels.is_empty());

        // Verify analysis ran successfully
        assert!(analysis.amount_out_atoms > 0);
        assert!(analysis.vwap_ticks > 0);
        assert!(analysis.levels_consumed > 0);

        // Print the full decoded output for manual inspection
        println!("{}", tx);
    }

    #[test]
    fn test_decode_real_message_only() {
        // Extract the message portion from the same real transaction.
        // Wire format: compact-u16(num_sigs) + num_sigs × 64-byte sigs + message.
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let tx_bytes = STANDARD.decode(REAL_TX_BASE64).unwrap();
        // num_sigs = 2 → compact-u16 encodes as single byte 0x02
        // skip: 1 (compact header) + 2 * 64 (signatures) = 129 bytes
        let msg_bytes = &tx_bytes[129..];
        let msg_b64 = STANDARD.encode(msg_bytes);

        let msg = decode_message_base64(&msg_b64).unwrap();

        assert_eq!(msg.version, MessageVersion::V0);
        assert_eq!(msg.account_keys.len(), 14);

        // Same fill should be found
        let fill_ix = msg
            .instructions
            .iter()
            .find(|ix| ix.fill.is_some())
            .expect("should find embedded fill_exact_in params");

        let (fill, analysis) = fill_ix.fill.as_ref().unwrap();
        assert!(fill.amount_in_atoms > 0);
        assert!(fill.params.tick_size_qpb > 0);
        assert!(analysis.levels_consumed > 0);

        println!("{}", msg);
    }

    // ---- Second real transaction: A3QA token → USDC fill ----

    /// Real transaction (message hash) from Solana mainnet: taker sells A3QA token for USDC.
    /// Tx sig: 2gHWdMvw1bYLq63P9FQ3GfhvVBQjibdZtkPRCiQ9r2wS4fkJphsMzN5K9gTohhcFxtyynytqWrcLXtPxZrjbZq3q
    /// On-chain log: side=Ask, amount_in=4000000, amount_out=799960, vwap_ticks=199990
    /// This is the pre-signing form (second signature is zeroed out).
    const REAL_TX2_BASE64: &str = "AlPlBOM0/PJtMdGe0Umk2ZL+l83VLIZlnto+clZr77+LZpMHjFOMhyn8d9paVwW2MUB5yfiVF9rQoqDX+HdVZgQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgAIBBQuWmpufqjVYZfnyuFyDIYJfGWS2mAfjxKCUSfgP6ocw/3bk5le5izsp4223iXZnE4grlusUmzXQL4KHUihpkIa4FRxK4rsywvjpGrtIXvcwlTqIfhWBjuSSKYumfXogybHMphZItU08AEZ6ahGHYgLn6pzxnUhwhDZJYTR1tfeVfdtjecj/osn4Wsppt6OzLN+qBRRBwfnZU+16g1g0LNZ01wzdbD2DjJmsVeUGo84syt1HfCXTCpDzC/xmIqVkziGGVoqJoXHQv3NrqnA8ud4Fpk5exDk5GSvBfPxu23BzewMGRm/lIRcy/+ytunLDm+e8jOW7xfcSayxDmzpAAAAACeTUjwGeOKmNaN8QhVjkQuAyPqrOdqWbR2vIsdN/sy4EedVb8jHAbu50xW7OaBUH/bGy3qP0jlECsc2iVrwTjwan1RcYe9FmNdrUBFX9wsDBJMaPIVZ1pdu6y18IAAAASNIEZ4DY6P0pwzwN89l81A/PZUIvzSKvmhMEoq6GJccDBwAFAjrVAAAHAAkDQCsAAAAAAAAJFwACAwYMDg4JCwkIAAECAwQFBgwODgoNWLtk+swxxK8UAAk9AAAAAADYNAwAAAAAADIAAAAAAAEAAAB4ASwAAACVbp1pAAAAAAEAAAAAAAAAQEIPAAAAAAABAAAANg0DAAAAAADoAwAAAAAAABAnAAEBKb+VBypPvwTfcXWT5zi8zCEnKBXCRxqA2fn1VbDUhCUABAAoAhQ=";

    #[test]
    fn test_decode_real_transaction_2() {
        let tx = decode_transaction_base64(REAL_TX2_BASE64).unwrap();

        // 2 signatures (first real, second zeroed placeholder)
        assert_eq!(tx.signatures.len(), 2);

        let msg = &tx.message;
        assert_eq!(msg.version, MessageVersion::V0);
        assert_eq!(msg.header.num_required_signatures, 2);
        assert_eq!(msg.header.num_readonly_signed_accounts, 1);
        assert_eq!(msg.header.num_readonly_unsigned_accounts, 5);
        assert_eq!(msg.account_keys.len(), 11);

        // 3 instructions: ComputeBudget × 2 + Jupiter route
        assert_eq!(msg.instructions.len(), 3);

        // Find the instruction containing embedded fill_exact_in params
        let fill_ix = msg
            .instructions
            .iter()
            .find(|ix| ix.fill.is_some())
            .expect("should find embedded fill_exact_in in Jupiter route instruction");

        let (fill, analysis) = fill_ix.fill.as_ref().unwrap();

        // On-chain log says: side=Ask, amount_in=4000000, amount_out=799960, vwap_ticks=199990
        assert_eq!(fill.taker_side, Side::Ask);
        assert_eq!(fill.amount_in_atoms, 4_000_000);
        assert_eq!(fill.params.tick_size_qpb, 1);
        assert_eq!(fill.params.lot_size_base, 1_000_000);
        assert_eq!(fill.params.levels.len(), 1);
        assert_eq!(fill.params.levels[0].px_ticks, 199_990);
        assert_eq!(fill.params.levels[0].qty_lots, 1_000);

        assert_eq!(analysis.taker_side, Side::Ask);
        assert_eq!(analysis.amount_in_atoms, 4_000_000);
        assert_eq!(analysis.amount_spent_atoms, 4_000_000);
        assert_eq!(analysis.amount_out_atoms, 799_960);
        assert_eq!(analysis.vwap_ticks, 199_990);
        assert_eq!(analysis.levels_consumed, 1);
        assert_eq!(analysis.total_lots_filled, 4);

        // Print full decoded output for inspection
        println!("{}", tx);
    }
}
