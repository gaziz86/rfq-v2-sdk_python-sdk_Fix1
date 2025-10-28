# Jupiter RFQ v2 Market Maker SDK

SDKs for Jupiter's RFQ v2 Market Maker service via gRPC streaming. Available in **Rust** and **Python**.

## Features

- Bidirectional streaming for quotes and swaps
- Real-time quote submission with custom pricing
- Solana transaction signing
- Type-safe APIs with builder patterns
- Production ready with TLS and health monitoring

## SDKs

### Rust SDK

High-performance async SDK built on Tokio. See [`rust-sdk/README.md`](rust-sdk/README.md)

```bash
cd rust-sdk
cargo run --example production_streaming
```

**Requirements:** Rust 1.70+, Tokio runtime

---

### Python SDK

Python SDK with asyncio support. See [`python-sdk/README.md`](python-sdk/README.md)

```bash
cd python-sdk
pip install -e .
python examples/production_streaming.py
```

**Requirements:** Python 3.8+, grpcio, protobuf, solders

## Environment Variables

- `MM_MAKER_ID` - Your maker identifier
- `MM_AUTH_TOKEN` - JWT authentication token
- `RFQ_ENDPOINT` - RFQ service endpoint URL
- `SOLANA_PRIVATE_KEY` - Base58 encoded private key (for signing)

## License

MIT


