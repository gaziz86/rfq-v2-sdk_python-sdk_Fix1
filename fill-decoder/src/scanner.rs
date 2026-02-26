//! Scan raw instruction data for an embedded `fill_exact_in` argument pattern.

use crate::analysis::analyze_fill;
use crate::types::{FillAnalysis, FillExactInInstruction, FillExactInParams, Level, Side};

/// Scan raw instruction data for an embedded `fill_exact_in` argument pattern.
pub fn scan_for_embedded_fill(data: &[u8]) -> Option<(FillExactInInstruction, FillAnalysis)> {
    // Minimum useful payload: lot_size(8) + tick_size(8) + len(4) + 1 level(16) = 36
    if data.len() < 36 {
        return None;
    }

    // Best-effort: amount_in_atoms lives at offset 8 (after the 8-byte discriminator) in the Jupiter instruction data.
    let amount_in = if data.len() >= 16 {
        u64::from_le_bytes(data[8..16].try_into().unwrap())
    } else {
        0
    };

    // Scan every possible offset for the u32 num_levels header.
    for pos in 16..data.len().saturating_sub(20) {
        let num = u32::from_le_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
        if num == 0 || num > 20 {
            continue;
        }

        let levels_start = pos + 4;
        let levels_end = levels_start + num * 16;
        if levels_end > data.len() {
            continue;
        }

        // Decode candidate levels.
        let mut levels = Vec::with_capacity(num);
        let mut ok = true;
        for j in 0..num {
            let off = levels_start + j * 16;
            let px = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
            let qty = u64::from_le_bytes(data[off + 8..off + 16].try_into().unwrap());
            if px == 0 || qty == 0 {
                ok = false;
                break;
            }
            levels.push(Level {
                px_ticks: px,
                qty_lots: qty,
            });
        }
        if !ok {
            continue;
        }

        // Validate: prices must be within 10× of each other.
        let min_px = levels.iter().map(|l| l.px_ticks).min().unwrap();
        let max_px = levels.iter().map(|l| l.px_ticks).max().unwrap();
        if max_px > min_px.saturating_mul(10) {
            continue;
        }
        // Reject absurdly large tick values (> 10^15).
        if max_px > 1_000_000_000_000_000 {
            continue;
        }

        // Prices should be non-decreasing (ask book) or non-increasing (bid).
        let monotonic = levels.windows(2).all(|w| w[0].px_ticks <= w[1].px_ticks)
            || levels.windows(2).all(|w| w[0].px_ticks >= w[1].px_ticks);
        if !monotonic {
            continue;
        }

        // Two u64s immediately before num_levels.
        // On-chain layout: tick_size_qpb | lot_size_base | num_levels | levels…
        // We try both orderings of (tick_size, lot_size) and both sides to be
        // robust against any serialisation differences.
        if pos < 16 {
            continue;
        }
        let val_a = u64::from_le_bytes(data[pos - 16..pos - 8].try_into().unwrap());
        let val_b = u64::from_le_bytes(data[pos - 8..pos].try_into().unwrap());
        if val_a == 0 || val_b == 0 {
            continue;
        }
        if val_a > 1_000_000_000_000 || val_b > 1_000_000_000_000 {
            continue;
        }

        // Try to recover expire_at (u64 before the two params, must be a timestamp).
        let expire_at = if pos >= 24 {
            let v = u64::from_le_bytes(data[pos - 24..pos - 16].try_into().unwrap());
            if v >= 1_600_000_000 && v <= 2_000_000_000 {
                v
            } else {
                0
            }
        } else {
            0
        };

        // Try both orderings of (tick_size_qpb, lot_size_base) and both sides.
        // The on-chain layout is tick_size_qpb first, lot_size_base second, but
        // we try both to handle any edge cases.  The analysis validation rejects
        // combinations that don't produce a meaningful fill.
        let orderings: [(u64, u64); 2] = [(val_a, val_b), (val_b, val_a)];
        let sides = [Side::Bid, Side::Ask];

        let mut best: Option<(FillExactInInstruction, FillAnalysis)> = None;
        for &(tick_size_qpb, lot_size_base) in &orderings {
            for &side in &sides {
                let candidate = FillExactInInstruction {
                    taker_side: side,
                    amount_in_atoms: amount_in,
                    params: FillExactInParams {
                        expire_at,
                        tick_size_qpb,
                        lot_size_base,
                        levels: levels.clone(),
                    },
                };

                if let Ok(analysis) = analyze_fill(&candidate) {
                    if analysis.levels_consumed > 0 {
                        // Prefer the combination where amount_spent == amount_in
                        // (fully consumed input) since that matches typical fills.
                        let is_exact = analysis.amount_spent_atoms == amount_in;
                        if is_exact {
                            return Some((candidate, analysis));
                        }
                        if best.is_none() {
                            best = Some((candidate, analysis));
                        }
                    }
                }
            }
        }
        if best.is_some() {
            return best;
        }
        // Not a valid fill — keep scanning.
    }
    None
}
