//! Core types for RFQ v2 `fill_exact_in` decoding and analysis.

/// Which side the *taker* is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Taker buys base (consumes ask levels).
    Bid = 0,
    /// Taker sells base (consumes bid levels).
    Ask = 1,
}

impl std::fmt::Display for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Side::Bid => write!(f, "Bid"),
            Side::Ask => write!(f, "Ask"),
        }
    }
}

/// A single price level in the orderbook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Level {
    /// Price in ticks – actual quote-atoms per lot = `px_ticks * tick_size_qpb`.
    pub px_ticks: u64,
    /// Quantity in lots – actual base-atoms = `qty_lots * lot_size_base`.
    pub qty_lots: u64,
}

/// Parameters embedded in the `fill_exact_in` instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillExactInParams {
    /// Unix timestamp (seconds) after which the quote is stale.
    pub expire_at: u64,
    /// Quote-atoms per base-lot per price tick.
    pub tick_size_qpb: u64,
    /// Base-atoms per lot.
    pub lot_size_base: u64,
    /// L2 levels ordered best → worst for the consuming side.
    pub levels: Vec<Level>,
}

/// Fully decoded `fill_exact_in` instruction data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillExactInInstruction {
    pub taker_side: Side,
    pub amount_in_atoms: u64,
    pub params: FillExactInParams,
}

/// Named account keys extracted from the instruction's account list.
///
/// Keys are stored as raw 32-byte arrays so callers can convert to whatever
/// pubkey type they prefer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillAccounts {
    pub user: [u8; 32],
    pub fill_authority: [u8; 32],
    pub user_base_token_account: [u8; 32],
    pub user_quote_token_account: [u8; 32],
    pub maker_base_token_account: [u8; 32],
    pub maker_quote_token_account: [u8; 32],
    pub base_mint: [u8; 32],
    pub quote_mint: [u8; 32],
    pub base_token_program: [u8; 32],
    pub quote_token_program: [u8; 32],
    pub instructions_sysvar: [u8; 32],
}

/// Result of running the sweep simulation against decoded fill data.
#[derive(Debug, Clone)]
pub struct FillAnalysis {
    /// Side the taker is on.
    pub taker_side: Side,
    /// Exact input amount (atoms) the taker supplied.
    pub amount_in_atoms: u64,
    /// Computed output amount (atoms) the taker would receive.
    pub amount_out_atoms: u64,
    /// Volume-weighted average price in ticks.
    pub vwap_ticks: u64,
    /// How many of the provided levels were (partially) consumed.
    pub levels_consumed: usize,
    /// Total lots filled across all levels.
    pub total_lots_filled: u64,
    /// Input amount actually spent (may be less than `amount_in_atoms`
    /// if the last level could not be fully consumed).
    pub amount_spent_atoms: u64,
    /// Params echoed back for convenience.
    pub lot_size_base: u64,
    pub tick_size_qpb: u64,
}

impl FillAnalysis {
    /// The effective price the taker paid / received expressed as a ratio of
    /// quote-atoms per base-atom (as `f64` for display purposes).
    pub fn effective_price(&self) -> f64 {
        if self.amount_out_atoms == 0 {
            return 0.0;
        }
        match self.taker_side {
            // Bid: taker spent quote to get base
            Side::Bid => self.amount_spent_atoms as f64 / self.amount_out_atoms as f64,
            // Ask: taker spent base to get quote
            Side::Ask => self.amount_out_atoms as f64 / self.amount_spent_atoms as f64,
        }
    }
}

impl std::fmt::Display for FillAnalysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FillAnalysis {{ side: {}, in: {}, spent: {}, out: {}, vwap_ticks: {}, \
             levels: {}, lots: {}, eff_price: {:.6} }}",
            self.taker_side,
            self.amount_in_atoms,
            self.amount_spent_atoms,
            self.amount_out_atoms,
            self.vwap_ticks,
            self.levels_consumed,
            self.total_lots_filled,
            self.effective_price(),
        )
    }
}
