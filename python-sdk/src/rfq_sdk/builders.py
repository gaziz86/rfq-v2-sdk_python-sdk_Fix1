"""Builder pattern for constructing MarketMakerQuote objects."""

from typing import List, Optional
from protos.market_maker_pb2 import (
    MarketMakerQuote,
    TokenPair,
    PriceLevel,
    Cluster,
)
from .utils import current_timestamp_micros


class QuoteBuilder:
    """Builder for creating MarketMakerQuote objects with validation."""

    def __init__(self):
        """Initialize a new quote builder."""
        self._timestamp: Optional[int] = None
        self._sequence_number: Optional[int] = None
        self._quote_expiry_time: int = 10_000_000  # 10 seconds default
        self._maker_id: Optional[str] = None
        self._maker_address: Optional[str] = None
        self._lot_size_base: int = 1000000  # 0.001 SOL default
        self._cluster: Cluster = Cluster.CLUSTER_MAINNET
        self._token_pair: Optional[TokenPair] = None
        self._bid_levels: List[PriceLevel] = []
        self._ask_levels: List[PriceLevel] = []

    def timestamp(self, timestamp: int) -> "QuoteBuilder":
        """Set the timestamp (in microseconds)."""
        self._timestamp = timestamp
        return self

    def current_timestamp(self) -> "QuoteBuilder":
        """Set the timestamp to current time."""
        self._timestamp = current_timestamp_micros()
        return self

    def sequence_number(self, seq: int) -> "QuoteBuilder":
        """Set the sequence number."""
        self._sequence_number = seq
        return self

    def quote_expiry_time(self, expiry_micros: int) -> "QuoteBuilder":
        """Set the quote expiry time in microseconds."""
        self._quote_expiry_time = expiry_micros
        return self

    def quote_expiry_seconds(self, expiry_secs: float) -> "QuoteBuilder":
        """Set the quote expiry time in seconds (converted to microseconds)."""
        self._quote_expiry_time = int(expiry_secs * 1_000_000)
        return self

    def maker_id(self, maker_id: str) -> "QuoteBuilder":
        """Set the maker ID."""
        self._maker_id = maker_id
        return self

    def maker_address(self, address: str) -> "QuoteBuilder":
        """Set the maker address."""
        self._maker_address = address
        return self

    def lot_size_base(self, lot_size: int) -> "QuoteBuilder":
        """Set the minimum lot size for the base token."""
        self._lot_size_base = lot_size
        return self

    def cluster(self, cluster: Cluster) -> "QuoteBuilder":
        """Set the cluster (mainnet/devnet)."""
        self._cluster = cluster
        return self

    def token_pair(self, token_pair: TokenPair) -> "QuoteBuilder":
        """Set the token pair."""
        self._token_pair = token_pair
        return self

    def add_bid_level(self, volume: int, price: int) -> "QuoteBuilder":
        """Add a bid level."""
        self._bid_levels.append(PriceLevel(volume=volume, price=price))
        return self

    def add_ask_level(self, volume: int, price: int) -> "QuoteBuilder":
        """Add an ask level."""
        self._ask_levels.append(PriceLevel(volume=volume, price=price))
        return self

    def bid_levels(self, levels: List[PriceLevel]) -> "QuoteBuilder":
        """Set all bid levels at once."""
        self._bid_levels = levels
        return self

    def ask_levels(self, levels: List[PriceLevel]) -> "QuoteBuilder":
        """Set all ask levels at once."""
        self._ask_levels = levels
        return self

    def clear_bids(self) -> "QuoteBuilder":
        """Clear all bid levels."""
        self._bid_levels = []
        return self

    def clear_asks(self) -> "QuoteBuilder":
        """Clear all ask levels."""
        self._ask_levels = []
        return self

    def validate(self) -> List[str]:
        """
        Validate the quote configuration.

        Returns:
            List of validation error messages (empty if valid)
        """
        errors = []

        if self._timestamp is None:
            errors.append("Timestamp is required")
        if self._sequence_number is None:
            errors.append("Sequence number is required")
        if not self._maker_id:
            errors.append("Maker ID is required")
        if not self._maker_address:
            errors.append("Maker address is required")
        if self._token_pair is None:
            errors.append("Token pair is required")
        if not self._bid_levels and not self._ask_levels:
            errors.append("At least one bid or ask level is required")

        # Validate bid levels are sorted descending by price
        if self._bid_levels:
            prices = [level.price for level in self._bid_levels]
            if prices != sorted(prices, reverse=True):
                errors.append("Bid levels should be sorted by price (highest first)")

        # Validate ask levels are sorted ascending by price
        if self._ask_levels:
            prices = [level.price for level in self._ask_levels]
            if prices != sorted(prices):
                errors.append("Ask levels should be sorted by price (lowest first)")

        # Validate no negative prices or volumes
        for level in self._bid_levels + self._ask_levels:
            if level.price <= 0:
                errors.append(f"Invalid price: {level.price}")
            if level.volume <= 0:
                errors.append(f"Invalid volume: {level.volume}")

        return errors

    def build(self) -> MarketMakerQuote:
        """
        Build the MarketMakerQuote.

        Returns:
            Constructed MarketMakerQuote

        Raises:
            ValueError: If validation fails
        """
        errors = self.validate()
        if errors:
            raise ValueError(f"Quote validation failed: {', '.join(errors)}")

        return MarketMakerQuote(
            timestamp=self._timestamp,
            sequence_number=self._sequence_number,
            quote_expiry_time=self._quote_expiry_time,
            maker_id=self._maker_id,
            maker_address=self._maker_address,
            lot_size_base=self._lot_size_base,
            cluster=self._cluster,
            token_pair=self._token_pair,
            bid_levels=self._bid_levels,
            ask_levels=self._ask_levels,
        )

    @classmethod
    def new(cls) -> "QuoteBuilder":
        """Create a new quote builder."""
        return cls()
