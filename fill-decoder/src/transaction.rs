//! Transaction and message decoding for Solana V0 / Legacy transactions.

use crate::analysis::analyze_fill;
use crate::decode::{
    decode_fill_instruction, is_fill_exact_in, read_u8, FILL_ACCOUNT_LABELS, RFQ_V2_PROGRAM_ID,
};
use crate::error::FillDecoderError;
use crate::scanner::scan_for_embedded_fill;
use crate::types::{FillAnalysis, FillExactInInstruction};

/// Solana message version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageVersion {
    Legacy,
    V0,
}

impl std::fmt::Display for MessageVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy => write!(f, "Legacy"),
            Self::V0 => write!(f, "V0"),
        }
    }
}

/// Solana message header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    pub num_required_signatures: u8,
    pub num_readonly_signed_accounts: u8,
    pub num_readonly_unsigned_accounts: u8,
}

/// An account resolved from the transaction's account-key list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAccount {
    /// Index in the message's full account list.
    pub index: u8,
    /// Base-58 public key, or `"LookupWritable[N]"` / `"LookupReadonly[N]"`
    /// for addresses that live in an on-chain lookup table.
    pub pubkey: String,
    /// Whether this account signed the transaction.
    pub is_signer: bool,
    /// Whether this account is writable.
    pub is_writable: bool,
    /// Human-readable label (only set for `fill_exact_in` accounts).
    pub label: Option<String>,
}

/// A decoded compiled instruction.
#[derive(Debug, Clone)]
pub struct DecodedInstruction {
    /// Position of this instruction in the message.
    pub instruction_index: usize,
    /// Index of the program account in the message key list.
    pub program_id_index: u8,
    /// Base-58 program ID.
    pub program_id: String,
    /// Raw account indices from the compiled instruction.
    pub account_indices: Vec<u8>,
    /// Resolved accounts with signer/writable/label metadata.
    pub accounts: Vec<ResolvedAccount>,
    /// Raw instruction data.
    pub data: Vec<u8>,
    /// Decoded fill + sweep analysis (present only for `fill_exact_in`).
    pub fill: Option<(FillExactInInstruction, FillAnalysis)>,
}

/// An address-table lookup entry (V0 messages only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressTableLookup {
    /// Base-58 lookup-table address.
    pub account_key: String,
    /// Indices into the table for writable accounts.
    pub writable_indexes: Vec<u8>,
    /// Indices into the table for read-only accounts.
    pub readonly_indexes: Vec<u8>,
}

/// A fully decoded Solana message.
#[derive(Debug, Clone)]
pub struct DecodedMessage {
    pub version: MessageVersion,
    pub header: MessageHeader,
    /// Base-58 static account keys.
    pub account_keys: Vec<String>,
    /// Base-58 recent blockhash.
    pub recent_blockhash: String,
    /// All instructions in order.
    pub instructions: Vec<DecodedInstruction>,
    /// V0 address-table lookups (empty for Legacy).
    pub address_table_lookups: Vec<AddressTableLookup>,
}

/// A fully decoded Solana transaction (signatures + message).
#[derive(Debug, Clone)]
pub struct DecodedTransaction {
    /// Base-58 signatures.
    pub signatures: Vec<String>,
    /// The decoded message.
    pub message: DecodedMessage,
}

/// Read Solana's compact-u16 variable-length encoding.
fn read_compact_u16(data: &[u8], offset: &mut usize) -> crate::Result<u16> {
    let mut val: u16 = 0;
    for i in 0..3u32 {
        if *offset >= data.len() {
            return Err(FillDecoderError::other(
                "unexpected end of data reading compact-u16",
            ));
        }
        let byte = data[*offset];
        *offset += 1;
        val |= ((byte & 0x7f) as u16) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(val);
        }
    }
    Err(FillDecoderError::other("compact-u16 exceeded 3 bytes"))
}

/// Read exactly `N` bytes into a fixed-size array.
fn read_bytes_fixed<const N: usize>(data: &[u8], offset: &mut usize) -> crate::Result<[u8; N]> {
    if *offset + N > data.len() {
        return Err(FillDecoderError::other(format!(
            "unexpected end of data: need {} bytes at offset {}, have {}",
            N,
            *offset,
            data.len()
        )));
    }
    let mut buf = [0u8; N];
    buf.copy_from_slice(&data[*offset..*offset + N]);
    *offset += N;
    Ok(buf)
}

/// Read `len` bytes into a `Vec`.
fn read_bytes_vec(data: &[u8], offset: &mut usize, len: usize) -> crate::Result<Vec<u8>> {
    if *offset + len > data.len() {
        return Err(FillDecoderError::other(format!(
            "unexpected end of data: need {} bytes at offset {}, have {}",
            len,
            *offset,
            data.len()
        )));
    }
    let bytes = data[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(bytes)
}

/// Simple hex encoder (avoids adding a hex crate).
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decode a base-64 encoded Solana transaction (signatures + message).
pub fn decode_transaction_base64(input: &str) -> crate::Result<DecodedTransaction> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bytes = STANDARD
        .decode(input)
        .map_err(|e| FillDecoderError::other(format!("invalid base64: {e}")))?;
    decode_transaction_bytes(&bytes)
}

/// Decode a Solana transaction from raw bytes (signatures + message).
pub fn decode_transaction_bytes(data: &[u8]) -> crate::Result<DecodedTransaction> {
    let mut offset = 0;

    let num_sigs = read_compact_u16(data, &mut offset)? as usize;
    let mut signatures = Vec::with_capacity(num_sigs);
    for _ in 0..num_sigs {
        let sig: [u8; 64] = read_bytes_fixed(data, &mut offset)?;
        signatures.push(bs58::encode(&sig).into_string());
    }

    let message = parse_message(data, &mut offset)?;

    Ok(DecodedTransaction {
        signatures,
        message,
    })
}

/// Decode a base-64 encoded Solana message (**without** signatures).
pub fn decode_message_base64(input: &str) -> crate::Result<DecodedMessage> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bytes = STANDARD
        .decode(input)
        .map_err(|e| FillDecoderError::other(format!("invalid base64: {e}")))?;
    let mut offset = 0;
    parse_message(&bytes, &mut offset)
}

/// Raw intermediate instruction before account resolution.
struct RawInstruction {
    program_id_index: u8,
    account_indices: Vec<u8>,
    data: Vec<u8>,
}

/// Raw intermediate address-table lookup.
struct RawLookup {
    account_key: [u8; 32],
    writable_indexes: Vec<u8>,
    readonly_indexes: Vec<u8>,
}

fn parse_message(data: &[u8], offset: &mut usize) -> crate::Result<DecodedMessage> {
    if *offset >= data.len() {
        return Err(FillDecoderError::other("empty message data"));
    }

    // Version prefix
    let version = if data[*offset] & 0x80 != 0 {
        *offset += 1;
        MessageVersion::V0
    } else {
        MessageVersion::Legacy
    };

    // Header
    let num_required_signatures = read_u8(data, offset)?;
    let num_readonly_signed = read_u8(data, offset)?;
    let num_readonly_unsigned = read_u8(data, offset)?;

    let header = MessageHeader {
        num_required_signatures,
        num_readonly_signed_accounts: num_readonly_signed,
        num_readonly_unsigned_accounts: num_readonly_unsigned,
    };

    // Static account keys
    let num_static = read_compact_u16(data, offset)? as usize;
    let mut static_keys: Vec<[u8; 32]> = Vec::with_capacity(num_static);
    for _ in 0..num_static {
        static_keys.push(read_bytes_fixed(data, offset)?);
    }

    // Recent blockhash
    let blockhash: [u8; 32] = read_bytes_fixed(data, offset)?;

    // Instructions (raw)
    let num_ixs = read_compact_u16(data, offset)? as usize;
    let mut raw_ixs: Vec<RawInstruction> = Vec::with_capacity(num_ixs);
    for _ in 0..num_ixs {
        let program_id_index = read_u8(data, offset)?;
        let num_accounts = read_compact_u16(data, offset)? as usize;
        let mut account_indices = Vec::with_capacity(num_accounts);
        for _ in 0..num_accounts {
            account_indices.push(read_u8(data, offset)?);
        }
        let data_len = read_compact_u16(data, offset)? as usize;
        let ix_data = read_bytes_vec(data, offset, data_len)?;
        raw_ixs.push(RawInstruction {
            program_id_index,
            account_indices,
            data: ix_data,
        });
    }

    // Address table lookups (V0 only)
    let mut raw_lookups: Vec<RawLookup> = Vec::new();
    if version == MessageVersion::V0 && *offset < data.len() {
        let num_lookups = read_compact_u16(data, offset)? as usize;
        for _ in 0..num_lookups {
            let account_key: [u8; 32] = read_bytes_fixed(data, offset)?;
            let nw = read_compact_u16(data, offset)? as usize;
            let mut writable_indexes = Vec::with_capacity(nw);
            for _ in 0..nw {
                writable_indexes.push(read_u8(data, offset)?);
            }
            let nr = read_compact_u16(data, offset)? as usize;
            let mut readonly_indexes = Vec::with_capacity(nr);
            for _ in 0..nr {
                readonly_indexes.push(read_u8(data, offset)?);
            }
            raw_lookups.push(RawLookup {
                account_key,
                writable_indexes,
                readonly_indexes,
            });
        }
    }

    let total_writable_lookups: usize = raw_lookups.iter().map(|l| l.writable_indexes.len()).sum();
    let writable_lookup_end = num_static + total_writable_lookups;

    let account_keys: Vec<String> = static_keys
        .iter()
        .map(|k| bs58::encode(k).into_string())
        .collect();

    let recent_blockhash = bs58::encode(&blockhash).into_string();

    // Helper closure: resolve one account index
    let resolve = |idx: u8| -> ResolvedAccount {
        let i = idx as usize;
        if i < num_static {
            let is_signer = i < num_required_signatures as usize;
            let is_writable = if is_signer {
                i < (num_required_signatures.saturating_sub(num_readonly_signed)) as usize
            } else {
                i < num_static.saturating_sub(num_readonly_unsigned as usize)
            };
            ResolvedAccount {
                index: idx,
                pubkey: account_keys[i].clone(),
                is_signer,
                is_writable,
                label: None,
            }
        } else if i < writable_lookup_end {
            ResolvedAccount {
                index: idx,
                pubkey: format!("LookupWritable[{}]", i - num_static),
                is_signer: false,
                is_writable: true,
                label: None,
            }
        } else {
            ResolvedAccount {
                index: idx,
                pubkey: format!("LookupReadonly[{}]", i - writable_lookup_end),
                is_signer: false,
                is_writable: false,
                label: None,
            }
        }
    };

    // Resolve instructions
    let instructions: Vec<DecodedInstruction> = raw_ixs
        .into_iter()
        .enumerate()
        .map(|(ix_idx, raw)| {
            let program_id = if (raw.program_id_index as usize) < account_keys.len() {
                account_keys[raw.program_id_index as usize].clone()
            } else {
                format!("Unknown(index={})", raw.program_id_index)
            };

            let is_rfq_program = program_id == RFQ_V2_PROGRAM_ID;
            let is_direct_fill = is_fill_exact_in(&raw.data);

            // Auto-detect fill: either a direct fill_exact_in instruction,
            // or an embedded fill within a CPI caller (e.g. Jupiter route).
            let fill = if is_direct_fill {
                decode_fill_instruction(&raw.data)
                    .ok()
                    .and_then(|ix| analyze_fill(&ix).ok().map(|a| (ix, a)))
            } else {
                scan_for_embedded_fill(&raw.data)
            };

            let accounts: Vec<ResolvedAccount> = raw
                .account_indices
                .iter()
                .enumerate()
                .map(|(pos, &idx)| {
                    let mut acct = resolve(idx);
                    // Label accounts for direct fill instructions
                    if is_direct_fill && is_rfq_program && pos < FILL_ACCOUNT_LABELS.len() {
                        acct.label = Some(FILL_ACCOUNT_LABELS[pos].to_string());
                    }
                    acct
                })
                .collect();

            DecodedInstruction {
                instruction_index: ix_idx,
                program_id_index: raw.program_id_index,
                program_id,
                account_indices: raw.account_indices,
                accounts,
                data: raw.data,
                fill,
            }
        })
        .collect();

    let address_table_lookups: Vec<AddressTableLookup> = raw_lookups
        .into_iter()
        .map(|l| AddressTableLookup {
            account_key: bs58::encode(&l.account_key).into_string(),
            writable_indexes: l.writable_indexes,
            readonly_indexes: l.readonly_indexes,
        })
        .collect();

    Ok(DecodedMessage {
        version,
        header,
        account_keys,
        recent_blockhash,
        instructions,
        address_table_lookups,
    })
}

impl std::fmt::Display for DecodedTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Decoded Transaction ===")?;
        writeln!(f, "Signatures ({}):", self.signatures.len())?;
        for (i, sig) in self.signatures.iter().enumerate() {
            writeln!(f, "  [{}] {}", i, sig)?;
        }
        writeln!(f)?;
        write!(f, "{}", self.message)
    }
}

impl std::fmt::Display for DecodedMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Message Version: {}", self.version)?;
        writeln!(
            f,
            "Header: {} required sigs, {} readonly signed, {} readonly unsigned",
            self.header.num_required_signatures,
            self.header.num_readonly_signed_accounts,
            self.header.num_readonly_unsigned_accounts,
        )?;
        writeln!(f)?;

        writeln!(f, "Account Keys ({}):", self.account_keys.len())?;
        for (i, key) in self.account_keys.iter().enumerate() {
            let signer = i < self.header.num_required_signatures as usize;
            let writable = if signer {
                i < (self
                    .header
                    .num_required_signatures
                    .saturating_sub(self.header.num_readonly_signed_accounts))
                    as usize
            } else {
                i < self
                    .account_keys
                    .len()
                    .saturating_sub(self.header.num_readonly_unsigned_accounts as usize)
            };
            let flags = match (signer, writable) {
                (true, true) => "SIGNER WRITABLE",
                (true, false) => "SIGNER READONLY",
                (false, true) => "WRITABLE",
                (false, false) => "READONLY",
            };
            writeln!(f, "  #{:<2} [{}] {}", i, flags, key)?;
        }
        writeln!(f)?;

        writeln!(f, "Recent Blockhash: {}", self.recent_blockhash)?;
        writeln!(f)?;

        writeln!(f, "Instructions ({}):", self.instructions.len())?;
        for ix in &self.instructions {
            write!(f, "{}", ix)?;
        }

        if !self.address_table_lookups.is_empty() {
            writeln!(f)?;
            writeln!(
                f,
                "Address Table Lookups ({}):",
                self.address_table_lookups.len()
            )?;
            for (i, lookup) in self.address_table_lookups.iter().enumerate() {
                writeln!(f, "  [{}] Table: {}", i, lookup.account_key)?;
                writeln!(f, "      Writable indices: {:?}", lookup.writable_indexes)?;
                writeln!(f, "      Readonly indices: {:?}", lookup.readonly_indexes)?;
            }
        }

        Ok(())
    }
}

impl std::fmt::Display for DecodedInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tag = if self.fill.is_some() {
            " [fill_exact_in]"
        } else {
            ""
        };
        writeln!(
            f,
            "  Instruction #{}{}: program={} (index={})",
            self.instruction_index, tag, self.program_id, self.program_id_index,
        )?;
        writeln!(
            f,
            "    Data ({} bytes): {}",
            self.data.len(),
            to_hex(&self.data),
        )?;
        writeln!(f, "    Accounts ({}):", self.accounts.len())?;
        for acct in &self.accounts {
            let flags = match (acct.is_signer, acct.is_writable) {
                (true, true) => "SIGNER WRITABLE",
                (true, false) => "SIGNER READONLY",
                (false, true) => "WRITABLE",
                (false, false) => "READONLY",
            };
            let label = acct
                .label
                .as_deref()
                .map(|l| format!(" ({})", l))
                .unwrap_or_default();
            writeln!(
                f,
                "      #{:<2} [{}] {}{}",
                acct.index, flags, acct.pubkey, label,
            )?;
        }

        if let Some((fill_ix, analysis)) = &self.fill {
            writeln!(f, "    --- Fill Details ---")?;
            writeln!(f, "    Taker side:      {}", fill_ix.taker_side)?;
            writeln!(f, "    Amount in:       {} atoms", fill_ix.amount_in_atoms)?;
            writeln!(f, "    Expire at:       {}", fill_ix.params.expire_at)?;
            writeln!(f, "    Tick size (qpb): {}", fill_ix.params.tick_size_qpb)?;
            writeln!(f, "    Lot size (base): {}", fill_ix.params.lot_size_base)?;
            writeln!(f, "    Levels ({}):", fill_ix.params.levels.len())?;
            for (i, lvl) in fill_ix.params.levels.iter().enumerate() {
                writeln!(
                    f,
                    "      #{}: px_ticks={}, qty_lots={}",
                    i, lvl.px_ticks, lvl.qty_lots,
                )?;
            }
            writeln!(f, "    --- Sweep Analysis ---")?;
            writeln!(
                f,
                "    Amount spent:    {} atoms",
                analysis.amount_spent_atoms
            )?;
            writeln!(
                f,
                "    Amount out:      {} atoms",
                analysis.amount_out_atoms
            )?;
            writeln!(f, "    VWAP (ticks):    {}", analysis.vwap_ticks)?;
            writeln!(f, "    Levels consumed: {}", analysis.levels_consumed)?;
            writeln!(f, "    Total lots:      {}", analysis.total_lots_filled)?;
            writeln!(
                f,
                "    Effective price: {:.6} quote-atoms/base-atom",
                analysis.effective_price()
            )?;
        }

        Ok(())
    }
}
