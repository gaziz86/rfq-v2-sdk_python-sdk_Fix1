"""Authentication helpers for the SDK."""

import os
from typing import Optional


def get_auth_token_from_env() -> Optional[str]:
    """
    Get authentication token from environment variables.

    Checks MM_AUTH_TOKEN environment variable.

    Returns:
        Authentication token or None if not found
    """
    return os.getenv("MM_AUTH_TOKEN")


def get_maker_id_from_env() -> Optional[str]:
    """
    Get maker ID from environment variables.

    Checks MM_MAKER_ID environment variable.

    Returns:
        Maker ID or None if not found
    """
    return os.getenv("MM_MAKER_ID")


def get_solana_private_key_from_env() -> Optional[str]:
    """
    Get Solana private key from environment variables.

    Checks SOLANA_PRIVATE_KEY environment variable.

    Returns:
        Base58 encoded private key or None if not found
    """
    return os.getenv("SOLANA_PRIVATE_KEY")


def validate_environment() -> dict:
    """
    Validate that all required environment variables are set.

    Returns:
        Dictionary with validation results
    """
    return {
        "auth_token": get_auth_token_from_env() is not None,
        "maker_id": get_maker_id_from_env() is not None,
        "solana_private_key": get_solana_private_key_from_env() is not None,
    }
