"""Production-ready streaming example with quote sending and swap transaction signing."""

import asyncio
import base64
import logging
import os
import sys
from datetime import datetime
from typing import Optional

# Add parent directory to path for imports
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'src'))

from rfq_sdk import (
    MarketMakerClient,
    ClientConfig,
    StreamConfig,
    TokenPairHelper,
    get_maker_id_from_env,
    get_auth_token_from_env,
    current_timestamp_micros,
    swap_helpers,
)
from protos.market_maker_pb2 import (
    MarketMakerQuote,
    MarketMakerSwap,
    PriceLevel,
    Cluster,
    SwapMessageType,
)

# Solana transaction signing
from solders.keypair import Keypair  # type: ignore
from solders.transaction import VersionedTransaction  # type: ignore
from solders.signature import Signature  # type: ignore
import base58


# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


# Constants for pricing
PRICE_DECIMALS = 6  # USDC has 6 decimals
SOL_DECIMALS = 9    # SOL has 9 decimals
PRICE_SCALE = 10 ** PRICE_DECIMALS
SOL_SCALE = 10 ** SOL_DECIMALS

# Volume tiers: (volume_in_lamports, markup_basis_points)
VOLUME_TIERS = [
    (1 * SOL_SCALE, 0),       # 1 SOL - no markup
    (10 * SOL_SCALE, 30),     # 10 SOL - 0.3% markup
    (100 * SOL_SCALE, 80),    # 100 SOL - 0.8% markup
    (1000 * SOL_SCALE, 150),  # 1000 SOL - 1.5% markup
    (5000 * SOL_SCALE, 250),  # 5000 SOL - 2.5% markup
]


def load_or_generate_keypair() -> Optional['Keypair']:
    """
    Load or generate a keypair for signing transactions.
    
    Returns:
        Keypair instance or None if solders is not available
    """
    
    # Check if a private key string is provided via environment variable
    private_key_str = os.getenv("SOLANA_PRIVATE_KEY")
    
    if private_key_str:
        logger.info("Loading keypair from SOLANA_PRIVATE_KEY environment variable")
        try:
            # Decode the base58 private key string
            private_key_bytes = base58.b58decode(private_key_str.strip())
            keypair = Keypair.from_bytes(private_key_bytes)
            logger.info(f"Loaded keypair with public key: {keypair.pubkey()}")
            return keypair
        except Exception as e:
            logger.error(f"Failed to load keypair from SOLANA_PRIVATE_KEY: {e}")
        
    # Generate a new keypair
    keypair = Keypair()
    logger.info(f"Generated temporary keypair: {keypair.pubkey()}")
    return keypair


def process_and_sign_transaction(
    swap_uuid: str,
    unsigned_tx_base64: str,
    keypair: 'Keypair'
) -> Optional[str]:
    """
    Process and sign an unsigned transaction (supports both legacy and V0 transactions).
    
    Args:
        swap_uuid: UUID of the swap
        unsigned_tx_base64: Base64 encoded unsigned transaction
        keypair: Keypair for signing
        
    Returns:
        Base64 encoded signed transaction or None on error
    """   
    logger.info(f"Processing transaction for swap UUID: {swap_uuid}")
    
    try:
        # Decode the base64 unsigned transaction
        tx_bytes = base64.b64decode(unsigned_tx_base64)
        logger.info(f"Decoded transaction: {len(tx_bytes)} bytes")
        
        # Deserialize as VersionedTransaction (supports both legacy and V0)
        transaction = VersionedTransaction.from_bytes(tx_bytes)
        logger.info("Transaction deserialized successfully")
        
        # Validate the transaction before signing
        if not validate_transaction(transaction):
            logger.error("Transaction validation failed")
            return None
        
        # Sign the transaction
        # Sign at index 1 (index 0 is the fee payer)
        signed_tx = transaction.sign([keypair])
        
        # Serialize the signed transaction
        signed_tx_bytes = bytes(signed_tx)
        signed_tx_base64 = base64.b64encode(signed_tx_bytes).decode('utf-8')
        
        logger.info("Transaction signed and encoded successfully")
        return signed_tx_base64
        
    except Exception as e:
        logger.error(f"Failed to sign transaction: {e}", exc_info=True)
        return None


def validate_transaction(transaction: 'VersionedTransaction') -> bool:
    """
    Validate a versioned transaction before signing.
    
    Args:
        transaction: The transaction to validate
        
    Returns:
        True if valid, False otherwise
    """
    logger.info("Validating versioned transaction...")
    
    try:
        message = transaction.message
        
        # Check that the transaction has instructions
        if not message.instructions or len(message.instructions) == 0:
            logger.error("Transaction has no instructions")
            return False
        
        # Check that the transaction has account keys
        account_keys = message.account_keys
        if not account_keys or len(account_keys) == 0:
            logger.error("Transaction has no account keys")
            return False
        
        logger.info("Transaction validation passed")
        
        return True
        
    except Exception as e:
        logger.error(f"Transaction validation error: {e}")
        return False


async def handle_swap_stream(
    swap_stream,
    keypair: Optional['Keypair'],
    stream_config: StreamConfig
):
    """
    Handle swap streaming in a dedicated task.
    
    Args:
        swap_stream: SwapStreamHandle instance
        keypair: Keypair for signing transactions
        stream_config: Stream configuration
    """
    swap_count = 0
    ping_interval = 10  # seconds
    last_ping_time = asyncio.get_event_loop().time()
    
    logger.info("Swap stream handler started")
    
    while True:
        try:
            # Send periodic pings to keep connection alive
            current_time = asyncio.get_event_loop().time()
            if current_time - last_ping_time >= ping_interval:
                ping_message = MarketMakerSwap(
                    message_type=SwapMessageType.SWAP_MESSAGE_TYPE_PING,
                    swap_uuid="",
                    signed_transaction=""
                )
                
                try:
                    await swap_stream.send_swap(ping_message)
                    logger.debug("Sent ping to server")
                    last_ping_time = current_time
                except Exception as e:
                    logger.error(f"Failed to send ping: {e}")
            
            # Receive updates with timeout
            try:
                swap_update = await swap_stream.receive_update_timeout(1)
                
                if swap_update is None:
                    logger.info("Swap stream ended")
                    break
                
                # Handle different message types
                if swap_helpers.is_pong(swap_update):
                    logger.debug("Received pong from server")
                    continue
                
                if swap_helpers.is_connection_ready(swap_update):
                    status_msg = swap_helpers.get_status_message(swap_update) or "Ready"
                    logger.info(f"Swap stream connection established: {status_msg}")
                    continue
                
                if swap_helpers.is_error(swap_update):
                    error_msg = swap_helpers.get_status_message(swap_update) or "Unknown error"
                    logger.error(f"Swap stream error: {error_msg}")
                    continue
                
                if swap_helpers.is_transaction_confirmed(swap_update):
                    details = swap_helpers.extract_confirmation_details(swap_update)
                    if details:
                        uuid, signature = details
                        logger.info(f"Transaction confirmed - UUID: {uuid}, Signature: {signature}")
                    continue
                
                if swap_helpers.is_swap_available(swap_update):
                    details = swap_helpers.extract_swap_details(swap_update)
                    if details:
                        swap_uuid, unsigned_transaction = details
                        swap_count += 1
                        logger.info(f"Swap #{swap_count}: {swap_uuid}")
                        
                        # Process and sign the transaction
                        signed_tx = process_and_sign_transaction(
                            swap_uuid,
                            unsigned_transaction,
                            keypair
                        )
                        
                        if signed_tx:
                            # Send the signed transaction back
                            market_maker_swap = MarketMakerSwap(
                                message_type=SwapMessageType.SWAP_MESSAGE_TYPE_SWAP_SUBMIT,
                                swap_uuid=swap_uuid,
                                signed_transaction=signed_tx
                            )
                            
                            try:
                                await swap_stream.send_swap(market_maker_swap)
                                logger.info(f"Sent signed transaction for swap {swap_uuid}")
                            except Exception as e:
                                logger.error(f"Failed to send signed transaction: {e}")
                        else:
                            logger.error(f"Failed to sign transaction for swap {swap_uuid}")
                    else:
                        logger.warning("Received swap available message but missing swap details")
                else:
                    logger.info(f"Received other swap update type: {swap_helpers.update_type_description(swap_update)}")
                    
            except asyncio.TimeoutError:
                await asyncio.sleep(0.5)
                continue
            except asyncio.CancelledError:
                logger.info("Swap handler cancelled (shutdown)")
                break
                
        except asyncio.CancelledError:
            logger.info("Swap handler task cancelled")
            break
        except Exception as e:
            logger.error(f"Error in swap handler: {e}", exc_info=True)
            break
    
    logger.info(f"Swap handler completed: {swap_count} swaps processed")


def calculate_volume_adjusted_price(
    base_price: int,
    volume_lamports: int,
    is_ask: bool
) -> int:
    """Calculate price with volume-based markup."""
    markup_bp = 0
    for tier_volume, tier_markup in reversed(VOLUME_TIERS):
        if volume_lamports >= tier_volume:
            markup_bp = tier_markup
            break
    
    adjustment_bp = markup_bp if is_ask else markup_bp // 2
    adjustment = (base_price * adjustment_bp) // 10000
    
    if is_ask:
        return base_price + adjustment
    else:
        return base_price - adjustment


def create_sample_quote(
    maker_id: str,
    maker_address: str,
    sequence_number: int,
    base_price: int = 50 * PRICE_SCALE  # Default $50 SOL
) -> MarketMakerQuote:
    """Create a sample market maker quote."""
    timestamp = current_timestamp_micros()
    
    # Create bid and ask levels with volume-based pricing
    bid_levels = []
    ask_levels = []
    
    for volume_lamports, _ in VOLUME_TIERS:
        bid_price = calculate_volume_adjusted_price(base_price, volume_lamports, is_ask=False)
        ask_price = calculate_volume_adjusted_price(base_price, volume_lamports, is_ask=True)
        
        bid_levels.append(PriceLevel(volume=volume_lamports, price=bid_price))
        ask_levels.append(PriceLevel(volume=volume_lamports, price=ask_price))
    
    quote = MarketMakerQuote(
        timestamp=timestamp,
        sequence_number=sequence_number,
        quote_expiry_time=10_000_000,  # 10 seconds in microseconds
        maker_id=maker_id,
        maker_address=maker_address,
        lot_size_base=1000, # base_decimals - quote_decimals
        cluster=Cluster.CLUSTER_MAINNET,
        token_pair=TokenPairHelper.sol_usdc(),
        bid_levels=bid_levels,
        ask_levels=ask_levels,
    )
    
    return quote


async def quote_sender_task(stream, maker_id: str, maker_address: str, start_seq: int):
    """Background task to send quotes periodically."""
    sequence = start_seq
    
    while True:
        try:
            quote = create_sample_quote(maker_id, maker_address, sequence)
            await stream.send_quote(quote)
            
            logger.info(f"Sent quote #{sequence} with {len(quote.bid_levels)} bid levels")
            
            sequence += 1
            await asyncio.sleep(15)  # Send quote every 15 seconds
            
        except asyncio.CancelledError:
            logger.info("Quote sender cancelled (shutdown)")
            break
        except RuntimeError as e:
            logger.warning(f"Stream closed: {e}")
            break
        except Exception as e:
            logger.error(f"Error sending quote: {e}", exc_info=True)
            await asyncio.sleep(1)


async def update_listener_task(stream):
    """Background task to listen for updates."""
    try:
        async for update in stream.updates():
            logger.info(f"Received update: {update.update_type}")
            
            # We can process different update types here
            # UpdateType.UPDATE_TYPE_NEW
            # UpdateType.UPDATE_TYPE_UPDATED
            # UpdateType.UPDATE_TYPE_EXPIRED
    
    except asyncio.CancelledError:
        logger.info("Update listener cancelled (shutdown)")
    except Exception as e:
        logger.error(f"Error receiving updates: {e}", exc_info=True)


async def main():
    """Main production streaming example with swap signing."""
    # Get configuration from environment
    maker_id = get_maker_id_from_env()
    auth_token = get_auth_token_from_env()
    
    if not maker_id or not auth_token:
        logger.error("Missing required environment variables:")
        logger.error("  MM_MAKER_ID - Your maker identifier")
        logger.error("  MM_AUTH_TOKEN - JWT authentication token")
        return 1
    
    # Load or generate keypair for transaction signing
    keypair = load_or_generate_keypair()
    if not keypair:
        logger.warning("Running without transaction signing capability")
    
    # Get endpoint from environment or use default
    endpoint = os.getenv("RFQ_ENDPOINT", "https://rfq-mm-edge-grpc.raccoons.dev")
    maker_address = str(keypair.pubkey()) if keypair else os.getenv("MAKER_ADDRESS", "917Yp1mesMs14d32kDwH4uNocdhuB67QzzaYKezkjy4B")
    
    logger.info(f"Starting production streaming for maker: {maker_id}")
    logger.info(f"Endpoint: {endpoint}")
    logger.info(f"Maker address: {maker_address}")
    
    # Configure client
    client_config = ClientConfig(
        endpoint=endpoint,
        timeout_secs=60,
        auth_token=auth_token
    )
    
    stream_config = StreamConfig()
    
    try:
        # Connect to the service
        async with await MarketMakerClient.connect_with_config(client_config) as client:
            logger.info("Connected to Market Maker service")
            
            # Start quote streaming with sequence synchronization
            stream, next_sequence = await client.start_streaming_with_sync(
                maker_id=maker_id,
                auth_token=auth_token,
                stream_config=stream_config
            )
            
            logger.info(f"Quote stream established. Starting sequence: {next_sequence}")
            
            # Start swap streaming in background task
            swap_task = None
            if keypair:
                try:
                    swap_stream = await client.start_swap_streaming(stream_config)
                    logger.info("Swap streaming started with keep-alive monitoring")
                    
                    swap_task = asyncio.create_task(
                        handle_swap_stream(swap_stream, keypair, stream_config)
                    )
                except Exception as e:
                    logger.warning(f"Swap streaming failed: {e}. Continuing with quotes only")
            else:
                logger.info("Skipping swap streaming (no keypair available)")
            
            # Start background tasks for quote streaming
            sender_task = asyncio.create_task(
                quote_sender_task(stream, maker_id, maker_address, next_sequence)
            )
            
            listener_task = asyncio.create_task(
                update_listener_task(stream)
            )
            
            # Wait for tasks (they run until interrupted)
            try:
                tasks = [sender_task, listener_task]
                if swap_task:
                    tasks.append(swap_task)
                await asyncio.gather(*tasks)
            except KeyboardInterrupt:
                logger.info("Received shutdown signal")
                sender_task.cancel()
                listener_task.cancel()
                if swap_task:
                    swap_task.cancel()
            
            # Shutdown with statistics
            await MarketMakerClient.shutdown_stream_with_stats(stream, timeout=5.0)
            
            # Get swap stream stats if available
            if swap_task and not swap_task.cancelled():
                try:
                    await asyncio.wait_for(swap_task, timeout=5.0)
                except asyncio.TimeoutError:
                    logger.warning("Swap stream shutdown timeout")
                except Exception as e:
                    logger.error(f"Swap stream error during shutdown: {e}")
            
    except Exception as e:
        logger.error(f"Error in main loop: {e}", exc_info=True)
        return 1
    
    logger.info("Streaming example completed successfully")
    return 0


if __name__ == "__main__":
    try:
        exit_code = asyncio.run(main())
        sys.exit(exit_code)
    except KeyboardInterrupt:
        logger.info("Interrupted by user")
        sys.exit(0)
