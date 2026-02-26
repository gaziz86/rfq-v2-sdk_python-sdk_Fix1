# fill-decoder

Decoder and analysis utilities for RFQ v2 `fill_exact_in` transactions on Solana. Helps market makers decode and verify fills — both standalone instructions and CPI-embedded fills (e.g. inside Jupiter route transactions).

## CLI

The crate ships a `decode-tx` binary (behind the `cli` feature).

```sh
# Decode a base-64 transaction (message hash) directly
cargo run --features=cli --bin decode-tx -- --message-hash <BASE64>

# Fetch by tx signature from RPC and decode
cargo run --features=cli --bin decode-tx -- --tx <SIGNATURE> --rpc-url <RPC_URL>

# Or use the RPC_URL env var
RPC_URL=https://api.mainnet-beta.solana.com \
  cargo run --features=cli --bin decode-tx -- --tx <SIGNATURE>
```

## Crate structure

| Module | Purpose |
|--------|---------|
| `types` | Core types (`Side`, `Level`, `FillExactInParams`, `FillAnalysis`, …) |
| `decode` | Constants, discriminator check, instruction & account decoding |
| `analysis` | Off-chain sweep simulation (`analyze_fill`) |
| `scanner` | Embedded fill detection for CPI calls (Jupiter) |
| `transaction` | Solana transaction/message parsing (Legacy & V0) |
| `error` | `FillDecoderError` error type |