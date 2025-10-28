"""Helper functions for processing swap updates."""

from typing import Optional, Tuple
from protos.market_maker_pb2 import SwapUpdate, SwapMessageType


def is_pong(swap_update: SwapUpdate) -> bool:
    """Check if the swap update is a pong message."""
    return swap_update.message_type == SwapMessageType.SWAP_MESSAGE_TYPE_PONG


def is_connection_ready(swap_update: SwapUpdate) -> bool:
    """Check if the swap update indicates connection is ready."""
    return swap_update.message_type == SwapMessageType.SWAP_MESSAGE_TYPE_CONNECTION_READY


def is_error(swap_update: SwapUpdate) -> bool:
    """Check if the swap update is an error message."""
    return swap_update.message_type == SwapMessageType.SWAP_MESSAGE_TYPE_ERROR


def is_transaction_confirmed(swap_update: SwapUpdate) -> bool:
    """Check if the swap update indicates a confirmed transaction."""
    return swap_update.message_type == SwapMessageType.SWAP_MESSAGE_TYPE_TRANSACTION_CONFIRMED


def is_swap_available(swap_update: SwapUpdate) -> bool:
    """Check if the swap update indicates a swap is available."""
    return swap_update.message_type == SwapMessageType.SWAP_MESSAGE_TYPE_SWAP_AVAILABLE


def get_status_message(swap_update: SwapUpdate) -> Optional[str]:
    """Extract the status message from a swap update."""
    if hasattr(swap_update, 'status_message') and swap_update.status_message:
        return swap_update.status_message
    return None


def extract_confirmation_details(swap_update: SwapUpdate) -> Optional[Tuple[str, str]]:
    """
    Extract confirmation details from a transaction confirmed update.
    
    Returns:
        Tuple of (swap_uuid, transaction_signature) or None
    """
    if not is_transaction_confirmed(swap_update):
        return None
    
    swap_uuid = swap_update.swap_uuid if hasattr(swap_update, 'swap_uuid') else ""
    signature = swap_update.transaction_signature if hasattr(swap_update, 'transaction_signature') else ""
    
    if swap_uuid and signature:
        return (swap_uuid, signature)
    return None


def extract_swap_details(swap_update: SwapUpdate) -> Optional[Tuple[str, str]]:
    """
    Extract swap details from a swap available update.
    
    Returns:
        Tuple of (swap_uuid, unsigned_transaction) or None
    """
    if not is_swap_available(swap_update):
        return None
    
    swap_uuid = swap_update.swap_uuid if hasattr(swap_update, 'swap_uuid') else ""
    unsigned_tx = swap_update.unsigned_transaction if hasattr(swap_update, 'unsigned_transaction') else ""
    
    if swap_uuid and unsigned_tx:
        return (swap_uuid, unsigned_tx)
    return None


def update_type_description(swap_update: SwapUpdate) -> str:
    """Get a human-readable description of the swap update type."""
    message_type = swap_update.message_type
    
    type_map = {
        SwapMessageType.SWAP_MESSAGE_TYPE_PING: "Ping",
        SwapMessageType.SWAP_MESSAGE_TYPE_PONG: "Pong",
        SwapMessageType.SWAP_MESSAGE_TYPE_CONNECTION_READY: "Connection Ready",
        SwapMessageType.SWAP_MESSAGE_TYPE_SWAP_AVAILABLE: "Swap Available",
        SwapMessageType.SWAP_MESSAGE_TYPE_SWAP_SUBMIT: "Swap Submit",
        SwapMessageType.SWAP_MESSAGE_TYPE_TRANSACTION_CONFIRMED: "Transaction Confirmed",
        SwapMessageType.SWAP_MESSAGE_TYPE_ERROR: "Error",
    }
    
    return type_map.get(message_type, f"Unknown ({message_type})")
