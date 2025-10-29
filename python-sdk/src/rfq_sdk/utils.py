"""Utility functions for the SDK."""

import time
from typing import Optional


def current_timestamp_micros() -> int:
    """Get current timestamp in microseconds."""
    return int(time.time() * 1_000_000)


def format_price(price: int, decimals: int) -> float:
    """
    Format a raw price value to human-readable decimal.

    Args:
        price: Raw price value
        decimals: Number of decimal places

    Returns:
        Formatted price as float
    """
    return price / (10 ** decimals)


def format_volume(volume: int, decimals: int) -> float:
    """
    Format a raw volume value to human-readable decimal.

    Args:
        volume: Raw volume value
        decimals: Number of decimal places

    Returns:
        Formatted volume as float
    """
    return volume / (10 ** decimals)


def to_raw_price(price: float, decimals: int) -> int:
    """
    Convert a decimal price to raw integer value.

    Args:
        price: Decimal price
        decimals: Number of decimal places

    Returns:
        Raw price value
    """
    return int(price * (10 ** decimals))


def to_raw_volume(volume: float, decimals: int) -> int:
    """
    Convert a decimal volume to raw integer value.

    Args:
        volume: Decimal volume
        decimals: Number of decimal places

    Returns:
        Raw volume value
    """
    return int(volume * (10 ** decimals))


def validate_keypair(private_key: Optional[str]) -> bool:
    """
    Validate a Solana private key format.

    Args:
        private_key: Base58 encoded private key

    Returns:
        True if valid, False otherwise
    """
    if not private_key:
        return False

    try:
        import base58
        decoded = base58.b58decode(private_key)
        return len(decoded) == 64  # Solana keypairs are 64 bytes
    except Exception:
        return False
