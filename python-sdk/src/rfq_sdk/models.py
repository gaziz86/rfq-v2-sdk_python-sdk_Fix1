"""Data models and configuration for the RFQv2 SDK."""

from dataclasses import dataclass, field
from datetime import timedelta
from typing import Optional
from protos.market_maker_pb2 import (
    Token,
    TokenPair,
    PriceLevel,
    MarketMakerQuote,
)


@dataclass
class ClientConfig:
    """Configuration for connecting to the RFQv2 service."""

    endpoint: str
    timeout_secs: int = 30
    max_retries: int = 3
    stream_buffer_size: int = 1000
    auth_token: Optional[str] = None

    @classmethod
    def default(cls) -> "ClientConfig":
        """Create default configuration."""
        return cls(endpoint="http://localhost:2408")

    def with_timeout(self, timeout_secs: int) -> "ClientConfig":
        """Set the connection timeout."""
        self.timeout_secs = timeout_secs
        return self

    def with_max_retries(self, max_retries: int) -> "ClientConfig":
        """Set the maximum retry attempts."""
        self.max_retries = max_retries
        return self

    def with_auth_token(self, auth_token: str) -> "ClientConfig":
        """Set the authentication token for API access."""
        self.auth_token = auth_token
        return self


@dataclass
class StreamConfig:
    """Configuration for streaming behavior."""

    send_buffer_size: int = 1000
    operation_timeout: timedelta = field(default_factory=lambda: timedelta(seconds=30))
    auto_reconnect: bool = False
    max_reconnect_attempts: int = 3
    inactivity_timeout: timedelta = field(default_factory=lambda: timedelta(seconds=120))

    def with_send_buffer_size(self, size: int) -> "StreamConfig":
        """Set the send buffer size."""
        self.send_buffer_size = size
        return self

    def with_operation_timeout(self, timeout: timedelta) -> "StreamConfig":
        """Set the operation timeout."""
        self.operation_timeout = timeout
        return self

    def with_auto_reconnect(self, max_attempts: int) -> "StreamConfig":
        """Enable auto-reconnection."""
        self.auto_reconnect = True
        self.max_reconnect_attempts = max_attempts
        return self

    def with_inactivity_timeout(self, timeout: timedelta) -> "StreamConfig":
        """Set the inactivity timeout."""
        self.inactivity_timeout = timeout
        return self


@dataclass
class ConnectionStats:
    """Generic connection statistics for monitoring stream health."""

    messages_sent: int = 0
    updates_received: int = 0
    errors_encountered: int = 0
    reconnections: int = 0
    connected_at: Optional[object] = None
    last_activity: Optional[object] = None


class TokenPairHelper:
    """Helper class for common token pairs."""

    @staticmethod
    def sol_usdc() -> TokenPair:
        """SOL/USDC token pair on mainnet."""
        return TokenPair(
            base_token=Token(
                address="So11111111111111111111111111111111111111112",
                decimals=9,
                symbol="SOL",
                owner="TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            ),
            quote_token=Token(
                address="EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                decimals=6,
                symbol="USDC",
                owner="TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            )
        )

    @staticmethod
    def eth_usdc() -> TokenPair:
        """ETH/USDC token pair on mainnet."""
        return TokenPair(
            base_token=Token(
                address="7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs",
                decimals=8,
                symbol="ETH",
                owner="TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            ),
            quote_token=Token(
                address="EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                decimals=6,
                symbol="USDC",
                owner="TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            )
        )

    @staticmethod
    def pair_name(token_pair: TokenPair) -> str:
        """Get a string representation of the token pair (e.g., 'SOL/USDC')."""
        return f"{token_pair.base_token.symbol}/{token_pair.quote_token.symbol}"


class QuoteHelper:
    """Helper methods for MarketMakerQuote."""

    @staticmethod
    def is_expired(quote: MarketMakerQuote) -> bool:
        """Check if the quote has expired."""
        from datetime import datetime
        now_micros = int(datetime.utcnow().timestamp() * 1_000_000)
        return now_micros > quote.timestamp + quote.quote_expiry_time

    @staticmethod
    def best_bid(quote: MarketMakerQuote) -> Optional[PriceLevel]:
        """Get the best bid price."""
        if not quote.bid_levels:
            return None
        return max(quote.bid_levels, key=lambda x: x.price)

    @staticmethod
    def best_ask(quote: MarketMakerQuote) -> Optional[PriceLevel]:
        """Get the best ask price."""
        if not quote.ask_levels:
            return None
        return min(quote.ask_levels, key=lambda x: x.price)

    @staticmethod
    def spread(quote: MarketMakerQuote) -> Optional[int]:
        """Calculate the spread."""
        best_bid = QuoteHelper.best_bid(quote)
        best_ask = QuoteHelper.best_ask(quote)

        if best_bid and best_ask:
            return best_ask.price - best_bid.price
        return None
