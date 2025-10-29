# Jupiter RFQ Python SDK

Python SDK for Jupiter RFQ integration via gRPC streaming.

## Installation

```bash
cd python-sdk
python -m venv ./venv
source venv/bin/activate 
pip install .
```

Run the example:
```bash
python examples/production_streaming.py
```

## Requirements

- Python 3.8+
- asyncio
- grpcio, protobuf, solders, base58

## License

MIT
