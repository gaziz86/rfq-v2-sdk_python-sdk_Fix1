"""Stream management for bidirectional gRPC streaming."""

import asyncio
import logging
from datetime import datetime
from typing import Optional, AsyncIterator
import grpc

from protos.market_maker_pb2 import (
    MarketMakerQuote,
    MarketMakerSwap,
    QuoteUpdate,
    SwapUpdate,
)
from .models import StreamConfig

logger = logging.getLogger(__name__)


class QuoteStreamHandle:
    """Handle for managing a bidirectional gRPC quote stream."""

    def __init__(
        self,
        quote_queue: asyncio.Queue,
        update_stream: grpc.aio.UnaryStreamCall,
        config: StreamConfig
    ):
        """
        Initialize the quote stream handle.

        Args:
            quote_queue: Queue for sending quotes
            update_stream: gRPC stream for receiving updates
            config: Stream configuration
        """
        self._quote_queue = quote_queue
        self._update_stream = update_stream
        self._config = config
        self._is_closed = False
        self._stats = {
            "messages_sent": 0,
            "updates_received": 0,
            "errors_encountered": 0,
            "connected_at": datetime.now(),
            "last_activity": None,
        }
        self._lock = asyncio.Lock()

    async def send_quote(self, quote: MarketMakerQuote):
        """
        Send a quote directly to the gRPC server.

        Args:
            quote: The MarketMakerQuote to send

        Raises:
            RuntimeError: If the stream has been closed
        """
        async with self._lock:
            if self._is_closed:
                raise RuntimeError("Stream has been closed")

            try:
                await self._quote_queue.put(quote)
                self._stats["messages_sent"] += 1
                self._stats["last_activity"] = datetime.now()
                logger.debug("Quote sent to stream")
            except Exception as e:
                logger.error(f"Failed to send quote: {e}")
                self._stats["errors_encountered"] += 1
                raise

    async def receive_update(self) -> Optional[QuoteUpdate]:
        """
        Receive the next quote update from the gRPC server.

        Returns:
            QuoteUpdate or None if stream ended
        """
        if self._is_closed:
            return None

        try:
            update = await self._update_stream.read()
            if update is None:
                logger.info("Stream ended normally")
                self._is_closed = True
                return None

            self._stats["updates_received"] += 1
            self._stats["last_activity"] = datetime.now()
            return update

        except asyncio.CancelledError:
            logger.debug("Stream read cancelled (normal during shutdown)")
            self._is_closed = True
            return None
        except grpc.RpcError as e:
            logger.error(f"gRPC error receiving update: {e}")
            self._stats["errors_encountered"] += 1
            raise
        except Exception as e:
            logger.error(f"Error receiving update: {e}")
            self._stats["errors_encountered"] += 1
            raise

    async def receive_update_timeout(
        self, timeout: float
    ) -> Optional[QuoteUpdate]:
        """
        Receive updates with a timeout.

        Args:
            timeout: Timeout in seconds

        Returns:
            QuoteUpdate or None if stream ended

        Raises:
            asyncio.TimeoutError: If timeout is exceeded
        """
        return await asyncio.wait_for(self.receive_update(), timeout=timeout)

    async def close(self):
        """Close the gRPC stream gracefully."""
        logger.info("Initiating graceful stream shutdown")

        async with self._lock:
            if self._is_closed:
                return

            self._is_closed = True

            # Signal the iterator to stop by putting None
            try:
                await self._quote_queue.put(None)
            except Exception as e:
                logger.warning(f"Error signaling stream closure: {e}")

            # Cancel the update stream
            try:
                self._update_stream.cancel()
            except Exception as e:
                logger.warning(f"Error cancelling stream: {e}")

        # Brief pause for cleanup
        await asyncio.sleep(0.1)
        logger.info("Stream shutdown completed")

    async def is_closed(self) -> bool:
        """Check if the stream is closed."""
        return self._is_closed

    def get_stats(self) -> dict:
        """Get connection statistics."""
        return self._stats.copy()

    def reset_stats(self):
        """Reset connection statistics."""
        self._stats = {
            "messages_sent": 0,
            "updates_received": 0,
            "errors_encountered": 0,
            "connected_at": datetime.now(),
            "last_activity": None,
        }

    async def updates(self) -> AsyncIterator[QuoteUpdate]:
        """
        Stream updates using an async iterator.

        Yields:
            QuoteUpdate objects
        """
        while not self._is_closed:
            try:
                update = await self.receive_update()
                if update is None:
                    break
                yield update
            except Exception as e:
                logger.error(f"Error in update iterator: {e}")
                break


class SwapStreamHandle:
    """Handle for managing a bidirectional gRPC swap stream."""

    def __init__(
        self,
        swap_queue: asyncio.Queue,
        update_stream: grpc.aio.UnaryStreamCall,
        config: StreamConfig
    ):
        """
        Initialize the swap stream handle.

        Args:
            swap_queue: Queue for sending swaps
            update_stream: gRPC stream for receiving updates
            config: Stream configuration
        """
        self._swap_queue = swap_queue
        self._update_stream = update_stream
        self._config = config
        self._is_closed = False
        self._stats = {
            "swaps_sent": 0,
            "updates_received": 0,
            "errors_encountered": 0,
            "connected_at": datetime.now(),
            "last_activity": None,
        }
        self._lock = asyncio.Lock()

    async def send_swap(self, swap: MarketMakerSwap):
        """
        Send a swap directly to the gRPC server.

        Args:
            swap: The MarketMakerSwap to send

        Raises:
            RuntimeError: If the stream has been closed
        """
        async with self._lock:
            if self._is_closed:
                raise RuntimeError("Stream has been closed")

            try:
                await self._swap_queue.put(swap)
                self._stats["swaps_sent"] += 1
                self._stats["last_activity"] = datetime.now()
                logger.debug("Swap sent to stream")
            except Exception as e:
                logger.error(f"Failed to send swap: {e}")
                self._stats["errors_encountered"] += 1
                raise

    async def receive_update(self) -> Optional[SwapUpdate]:
        """
        Receive the next swap update from the gRPC server.

        Returns:
            SwapUpdate or None if stream ended
        """
        if self._is_closed:
            return None

        try:
            update = await self._update_stream.read()
            if update is None:
                logger.info("Swap stream ended normally")
                self._is_closed = True
                return None

            self._stats["updates_received"] += 1
            self._stats["last_activity"] = datetime.now()
            return update

        except asyncio.CancelledError:
            logger.debug("Swap stream read cancelled (normal during shutdown)")
            self._is_closed = True
            return None
        except grpc.RpcError as e:
            logger.error(f"gRPC error receiving swap update: {e}")
            self._stats["errors_encountered"] += 1
            raise
        except Exception as e:
            logger.error(f"Error receiving swap update: {e}")
            self._stats["errors_encountered"] += 1
            raise

    async def receive_update_timeout(
        self, timeout: float
    ) -> Optional[SwapUpdate]:
        """
        Receive updates with a timeout.

        Args:
            timeout: Timeout in seconds

        Returns:
            SwapUpdate or None if stream ended

        Raises:
            asyncio.TimeoutError: If timeout is exceeded
        """
        return await asyncio.wait_for(self.receive_update(), timeout=timeout)

    async def close(self):
        """Close the gRPC stream gracefully."""
        logger.info("Initiating graceful swap stream shutdown")

        async with self._lock:
            if self._is_closed:
                return

            self._is_closed = True

            # Signal the iterator to stop
            try:
                await self._swap_queue.put(None)
            except Exception as e:
                logger.warning(f"Error signaling swap stream closure: {e}")

            # Cancel the update stream
            try:
                self._update_stream.cancel()
            except Exception as e:
                logger.warning(f"Error cancelling swap stream: {e}")

        await asyncio.sleep(0.1)
        logger.info("Swap stream shutdown completed")

    async def is_closed(self) -> bool:
        """Check if the stream is closed."""
        return self._is_closed

    def get_stats(self) -> dict:
        """Get connection statistics."""
        return self._stats.copy()

    def reset_stats(self):
        """Reset connection statistics."""
        self._stats = {
            "swaps_sent": 0,
            "updates_received": 0,
            "errors_encountered": 0,
            "connected_at": datetime.now(),
            "last_activity": None,
        }

    async def updates(self) -> AsyncIterator[SwapUpdate]:
        """
        Stream updates using an async iterator.

        Yields:
            SwapUpdate objects
        """
        while not self._is_closed:
            try:
                update = await self.receive_update()
                if update is None:
                    break
                yield update
            except Exception as e:
                logger.error(f"Error in swap update iterator: {e}")
                break
