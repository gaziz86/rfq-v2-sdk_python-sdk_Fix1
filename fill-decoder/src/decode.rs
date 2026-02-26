//! Constants, detection, and low-level decoding for `fill_exact_in` instructions.

use crate::error::FillDecoderError;
use crate::types::{FillAccounts, FillExactInInstruction, FillExactInParams, Level, Side};

/// Base-58 encoded RFQ v2 program ID.
pub const RFQ_V2_PROGRAM_ID: &str = "fd3nMFYTQjX1yr5ER8u7tPdHJB7qt8RpDpNtLQX2Br5";

/// 8-byte Anchor discriminator for `fill_exact_in`.
pub const FILL_EXACT_IN_DISCRIMINATOR: [u8; 8] = [222, 208, 6, 209, 154, 163, 54, 94];

/// Number of accounts the `fill_exact_in` instruction expects.
pub const FILL_EXACT_IN_ACCOUNT_COUNT: usize = 11;

/// Labels for the 11 accounts in a `fill_exact_in` instruction.
pub const FILL_ACCOUNT_LABELS: [&str; FILL_EXACT_IN_ACCOUNT_COUNT] = [
    "user",
    "fill_authority",
    "user_base_token_account",
    "user_quote_token_account",
    "maker_base_token_account",
    "maker_quote_token_account",
    "base_mint",
    "quote_mint",
    "base_token_program",
    "quote_token_program",
    "instructions_sysvar",
];

/// Returns `true` if `instruction_data` begins with the `fill_exact_in` discriminator.
pub fn is_fill_exact_in(instruction_data: &[u8]) -> bool {
    instruction_data
        .get(..8)
        .is_some_and(|d| d == FILL_EXACT_IN_DISCRIMINATOR)
}

pub(crate) fn read_u8(data: &[u8], offset: &mut usize) -> crate::Result<u8> {
    if *offset + 1 > data.len() {
        return Err(FillDecoderError::other(
            "unexpected end of instruction data",
        ));
    }
    let val = data[*offset];
    *offset += 1;
    Ok(val)
}

pub(crate) fn read_u32(data: &[u8], offset: &mut usize) -> crate::Result<u32> {
    if *offset + 4 > data.len() {
        return Err(FillDecoderError::other(
            "unexpected end of instruction data",
        ));
    }
    let val = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
    *offset += 4;
    Ok(val)
}

pub(crate) fn read_u64(data: &[u8], offset: &mut usize) -> crate::Result<u64> {
    if *offset + 8 > data.len() {
        return Err(FillDecoderError::other(
            "unexpected end of instruction data",
        ));
    }
    let val = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    Ok(val)
}

fn read_pubkey(keys: &[[u8; 32]], index: usize) -> crate::Result<[u8; 32]> {
    keys.get(index)
        .copied()
        .ok_or_else(|| FillDecoderError::other(format!("missing account at index {index}")))
}

/// Decode the `fill_exact_in` instruction from raw instruction data bytes.
///
/// The expected layout (Anchor / Borsh):
/// ```text
/// [0..8]   discriminator
/// [8]      taker_side          (u8 enum)
/// [9..17]  amount_in_atoms     (u64 LE)
/// [17..25] expire_at           (u64 LE)
/// [25..33] tick_size_qpb       (u64 LE)
/// [33..41] lot_size_base       (u64 LE)
/// [41..45] levels.len()        (u32 LE)
/// [45..]   levels[]            (each: px_ticks u64 + qty_lots u64 = 16 bytes)
/// ```
pub fn decode_fill_instruction(data: &[u8]) -> crate::Result<FillExactInInstruction> {
    if !is_fill_exact_in(data) {
        return Err(FillDecoderError::validation(
            "instruction data does not match fill_exact_in discriminator",
        ));
    }

    let mut offset: usize = 8; // skip discriminator

    // taker_side
    let side_byte = read_u8(data, &mut offset)?;
    let taker_side = match side_byte {
        0 => Side::Bid,
        1 => Side::Ask,
        other => {
            return Err(FillDecoderError::validation(format!(
                "invalid Side variant: {other}"
            )))
        }
    };

    // amount_in_atoms
    let amount_in_atoms = read_u64(data, &mut offset)?;

    // FillExactInParams
    let expire_at = read_u64(data, &mut offset)?;
    let tick_size_qpb = read_u64(data, &mut offset)?;
    let lot_size_base = read_u64(data, &mut offset)?;

    let num_levels = read_u32(data, &mut offset)? as usize;
    let mut levels = Vec::with_capacity(num_levels);
    for _ in 0..num_levels {
        let px_ticks = read_u64(data, &mut offset)?;
        let qty_lots = read_u64(data, &mut offset)?;
        levels.push(Level { px_ticks, qty_lots });
    }

    Ok(FillExactInInstruction {
        taker_side,
        amount_in_atoms,
        params: FillExactInParams {
            expire_at,
            tick_size_qpb,
            lot_size_base,
            levels,
        },
    })
}

/// Map the instruction's account keys (in order) to named [`FillAccounts`].
///
/// `account_keys` must contain at least [`FILL_EXACT_IN_ACCOUNT_COUNT`] entries.
pub fn decode_fill_accounts(account_keys: &[[u8; 32]]) -> crate::Result<FillAccounts> {
    if account_keys.len() < FILL_EXACT_IN_ACCOUNT_COUNT {
        return Err(FillDecoderError::validation(format!(
            "expected at least {} account keys, got {}",
            FILL_EXACT_IN_ACCOUNT_COUNT,
            account_keys.len()
        )));
    }

    Ok(FillAccounts {
        user: read_pubkey(account_keys, 0)?,
        fill_authority: read_pubkey(account_keys, 1)?,
        user_base_token_account: read_pubkey(account_keys, 2)?,
        user_quote_token_account: read_pubkey(account_keys, 3)?,
        maker_base_token_account: read_pubkey(account_keys, 4)?,
        maker_quote_token_account: read_pubkey(account_keys, 5)?,
        base_mint: read_pubkey(account_keys, 6)?,
        quote_mint: read_pubkey(account_keys, 7)?,
        base_token_program: read_pubkey(account_keys, 8)?,
        quote_token_program: read_pubkey(account_keys, 9)?,
        instructions_sysvar: read_pubkey(account_keys, 10)?,
    })
}
