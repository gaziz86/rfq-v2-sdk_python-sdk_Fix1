//! Off-chain replay of the on-chain sweep logic for `fill_exact_in`.

use crate::error::FillDecoderError;
use crate::types::{FillAnalysis, FillExactInInstruction, Side};

/// Simulate the on-chain sweep and compute the expected fill outcome.
///
/// This mirrors the `sweep_quote_in_for_base` / `sweep_base_in_for_quote` logic from the RFQ v2 program so MMs can verify fills locally.
pub fn analyze_fill(ix: &FillExactInInstruction) -> crate::Result<FillAnalysis> {
    let p = &ix.params;
    let is_bid = ix.taker_side == Side::Bid;
    let mut remaining = ix.amount_in_atoms;
    let mut bid_lots: u64 = 0;
    let mut ask_quote: u64 = 0;
    let mut weighted_px: u128 = 0;
    let mut levels_consumed: usize = 0;

    for lvl in &p.levels {
        if remaining == 0 {
            break;
        }

        let price_per_lot: u64 = (lvl.px_ticks as u128)
            .checked_mul(p.tick_size_qpb as u128)
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| err("overflow: price_per_lot"))?;

        // How many lots the taker can take from this level.
        let lots = if is_bid {
            if price_per_lot == 0 {
                continue;
            }
            (remaining / price_per_lot).min(lvl.qty_lots)
        } else {
            let avail = remaining / p.lot_size_base;
            if avail == 0 {
                break;
            }
            avail.min(lvl.qty_lots)
        };
        if lots == 0 {
            continue;
        }

        // Spend quote-atoms (Bid) or base-atoms (Ask).
        let unit = if is_bid {
            price_per_lot
        } else {
            p.lot_size_base
        };
        let spend = lots
            .checked_mul(unit)
            .ok_or_else(|| err("overflow: spend"))?;
        remaining = remaining
            .checked_sub(spend)
            .ok_or_else(|| err("underflow: remaining"))?;

        // Accumulate output.
        if is_bid {
            bid_lots = bid_lots
                .checked_add(lots)
                .ok_or_else(|| err("overflow: lots"))?;
        } else {
            let q = lots
                .checked_mul(price_per_lot)
                .ok_or_else(|| err("overflow: quote"))?;
            ask_quote = ask_quote
                .checked_add(q)
                .ok_or_else(|| err("overflow: quote"))?;
        }

        weighted_px = weighted_px
            .checked_add(
                (lvl.px_ticks as u128)
                    .checked_mul(lots as u128)
                    .ok_or_else(|| err("overflow: weighted_px"))?,
            )
            .ok_or_else(|| err("overflow: weighted_px"))?;

        levels_consumed += 1;
    }

    let amount_spent = ix.amount_in_atoms - remaining;

    let (amount_out, total_lots) = if is_bid {
        let out = bid_lots
            .checked_mul(p.lot_size_base)
            .ok_or_else(|| err("overflow: out"))?;
        (out, bid_lots)
    } else {
        (ask_quote, amount_spent / p.lot_size_base)
    };

    let vwap_ticks = if total_lots > 0 {
        (weighted_px / total_lots as u128) as u64
    } else {
        0
    };

    Ok(FillAnalysis {
        taker_side: ix.taker_side,
        amount_in_atoms: ix.amount_in_atoms,
        amount_out_atoms: amount_out,
        vwap_ticks,
        levels_consumed,
        total_lots_filled: total_lots,
        amount_spent_atoms: amount_spent,
        lot_size_base: p.lot_size_base,
        tick_size_qpb: p.tick_size_qpb,
    })
}

fn err(msg: &str) -> FillDecoderError {
    FillDecoderError::other(msg)
}
