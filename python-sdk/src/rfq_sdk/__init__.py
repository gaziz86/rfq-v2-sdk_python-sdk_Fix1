"""Jupiter RFQv2 SDK for Python.

This SDK provides a Python interface for Jupiter RFQv2 integration
via gRPC streaming.
"""

__version__ = "0.1.0"

from .client import MarketMakerClient
from .models import (
    ClientConfig,
    StreamConfig,
    ConnectionStats,
    TokenPairHelper,
    QuoteHelper,
)
from .stream_manager import QuoteStreamHandle, SwapStreamHandle
from .builders import QuoteBuilder
from .auth import (
    get_auth_token_from_env,
    get_maker_id_from_env,
    get_solana_private_key_from_env,
    validate_environment,
)
from .utils import (
    current_timestamp_micros,
    format_price,
    format_volume,
    to_raw_price,
    to_raw_volume,
    validate_keypair,
)
from . import swap_helpers

# Re-export protobuf types for convenience
from protos.market_maker_pb2 import (
    Token,
    TokenPair,
    PriceLevel,
    Cluster,
    MarketMakerQuote,
    MarketMakerSwap,
    SequenceNumberRequest,
    SequenceNumberResponse,
    QuoteUpdate,
    SwapUpdate,
    SwapMessageType,
    UpdateType,
    Orderbook,
    GetAllOrderbooksRequest,
    GetAllOrderbooksResponse,
)

__all__ = [
    # Client
    "MarketMakerClient",
    # Models
    "ClientConfig",
    "StreamConfig",
    "ConnectionStats",
    "TokenPairHelper",
    "QuoteHelper",
    # Streams
    "QuoteStreamHandle",
    "SwapStreamHandle",
    # Builders
    "QuoteBuilder",
    # Auth
    "get_auth_token_from_env",
    "get_maker_id_from_env",
    "get_solana_private_key_from_env",
    "validate_environment",
    # Utils
    "current_timestamp_micros",
    "format_price",
    "format_volume",
    "to_raw_price",
    "to_raw_volume",
    "validate_keypair",
    # Helpers
    "swap_helpers",
    # Protobuf types
    "Token",
    "TokenPair",
    "PriceLevel",
    "Cluster",
    "MarketMakerQuote",
    "MarketMakerSwap",
    "SequenceNumberRequest",
    "SequenceNumberResponse",
    "QuoteUpdate",
    "SwapUpdate",
    "SwapMessageType",
    "UpdateType",
    "Orderbook",
    "GetAllOrderbooksRequest",
    "GetAllOrderbooksResponse",
]
