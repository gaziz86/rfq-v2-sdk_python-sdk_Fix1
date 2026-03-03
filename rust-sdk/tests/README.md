# RFQ V2 Integration Tests

End-to-end integration tests that interact with the live V2 gRPC service and the preprod Ultra API.

## Prerequisites

| Variable | Description | Required |
|---|---|---|
| `SOLANA_PRIVATE_KEY` | Base58-encoded private key of the **taker** wallet | **Yes** |
| `INPUT_MINT` | SPL token mint for the input side (default: USDC) | No |
| `OUTPUT_MINT` | SPL token mint for the output side (default: SOL) | No |
| `TAKER` | Taker public key – derived from `SOLANA_PRIVATE_KEY` when omitted | No |
| `ULTRA_API_BASE` | Ultra API base URL (default: `https://preprod.ultra-api.jup.ag`) | No |

## Running

```bash
# Run all integration tests (ignored by default `cargo test`)
cargo test --test ultra_api_e2e -- --ignored --nocapture

# Run a single test
cargo test --test ultra_api_e2e test_full_order_flow -- --ignored --nocapture
```
