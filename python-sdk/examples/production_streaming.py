"""Production-ready streaming example with quote sending and update listening."""

import asyncio
import logging
import os
import sys
from datetime import datetime

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
)
from protos.market_maker_pb2 import MarketMakerQuote, PriceLevel, Cluster

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
            
    except Exception as e:
        logger.error(f"Error receiving updates: {e}", exc_info=True)


async def main():
    """Main production streaming example."""
    # Get configuration from environment
    maker_id = get_maker_id_from_env()
    auth_token = get_auth_token_from_env()
    
    if not maker_id or not auth_token:
        logger.error("Missing required environment variables:")
        logger.error("  MM_MAKER_ID - Your maker identifier")
        logger.error("  MM_AUTH_TOKEN - JWT authentication token")
        return
    
    # Get endpoint from environment or use default
    endpoint = os.getenv("RFQ_ENDPOINT", "https://rfq-mm-edge-grpc.raccoons.dev")
    maker_address = os.getenv("MAKER_ADDRESS", "your_maker_wallet_address")
    
    logger.info(f"Starting production streaming for maker: {maker_id}")
    logger.info(f"Endpoint: {endpoint}")
    
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
            
            # Start streaming with sequence synchronization
            stream, next_sequence = await client.start_streaming_with_sync(
                maker_id=maker_id,
                auth_token=auth_token,
                stream_config=stream_config
            )
            
            logger.info(f"Stream established. Starting sequence: {next_sequence}")
            
            # Start background tasks
            sender_task = asyncio.create_task(
                quote_sender_task(stream, maker_id, maker_address, next_sequence)
            )
            
            listener_task = asyncio.create_task(
                update_listener_task(stream)
            )
            
            # Wait for tasks (they run until interrupted)
            try:
                await asyncio.gather(sender_task, listener_task)
            except KeyboardInterrupt:
                logger.info("Received shutdown signal")
                sender_task.cancel()
                listener_task.cancel()
            
            # Shutdown with statistics
            await MarketMakerClient.shutdown_stream_with_stats(stream, timeout=5.0)
            
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
