"""Main client implementation for the RFQv2 SDK."""

import asyncio
import logging
from datetime import datetime
from typing import Optional, Tuple
import grpc

from protos.market_maker_pb2 import (
    SequenceNumberRequest,
    GetAllOrderbooksRequest,
)
from protos.market_maker_pb2_grpc import MarketMakerIngestionServiceStub
from .stream_manager import QuoteStreamHandle, SwapStreamHandle, StreamConfig
from .models import ClientConfig

logger = logging.getLogger(__name__)


class MarketMakerClient:
    """Main client for interacting with the RFQv2."""

    def __init__(self, config: ClientConfig):
        """
        Initialize the RFQv2 client.

        Args:
            config: Client configuration including endpoint and auth settings
        """
        self.config = config
        self._channel: Optional[grpc.aio.Channel] = None
        self._stub: Optional[MarketMakerIngestionServiceStub] = None

    async def __aenter__(self):
        """Async context manager entry."""
        if self._channel is None:
            await self._establish_connection()
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb):
        """Async context manager exit."""
        await self.close()

    @classmethod
    async def connect(cls, endpoint: str, auth_token: Optional[str] = None) -> "MarketMakerClient":
        """
        Connect to the RFQv2 service with default configuration.

        Args:
            endpoint: gRPC service endpoint URL
            auth_token: Optional authentication token

        Returns:
            Connected MarketMakerClient instance
        """
        config = ClientConfig(endpoint=endpoint, auth_token=auth_token)
        client = await cls.connect_with_config(config)
        return client

    @classmethod
    async def connect_with_config(cls, config: ClientConfig) -> "MarketMakerClient":
        """
        Connect to the RFQv2 service with custom configuration.

        Args:
            config: Client configuration

        Returns:
            Connected MarketMakerClient instance
        """
        logger.info(f"Connecting to RFQv2 service at {config.endpoint}")
        client = cls(config)
        await client._establish_connection()
        return client

    async def _establish_connection(self):
        """Establish the gRPC connection."""
        if self.config.endpoint.startswith("https://"):
            logger.debug("Configuring HTTPS connection with TLS")
            credentials = grpc.ssl_channel_credentials()
            self._channel = grpc.aio.secure_channel(
                self.config.endpoint.replace("https://", ""),
                credentials,
                options=[
                    ("grpc.max_send_message_length", -1),
                    ("grpc.max_receive_message_length", -1),
                ]
            )
        else:
            logger.debug("Using insecure connection (development mode)")
            self._channel = grpc.aio.insecure_channel(
                self.config.endpoint.replace("http://", ""),
                options=[
                    ("grpc.max_send_message_length", -1),
                    ("grpc.max_receive_message_length", -1),
                ]
            )

        self._stub = MarketMakerIngestionServiceStub(self._channel)
        logger.debug("Successfully connected to RFQv2 service")

    def _get_metadata(self) -> list:
        """Get metadata with authentication token if available."""
        metadata = []
        if self.config.auth_token:
            metadata.append(("x-api-key", self.config.auth_token))
            logger.debug("Added authentication token to request metadata")
        return metadata

    async def start_quote_streaming(
        self, config: Optional[StreamConfig] = None
    ) -> QuoteStreamHandle:
        """
        Start a bidirectional gRPC streaming connection for real-time quote updates.

        Args:
            config: Optional stream configuration

        Returns:
            QuoteStreamHandle for managing the stream
        """
        stream_config = config or StreamConfig()
        logger.info("Starting bidirectional gRPC streaming connection")

        # Create queue for outgoing quotes
        quote_queue = asyncio.Queue(maxsize=stream_config.send_buffer_size)

        async def quote_iterator():
            """Async generator for outgoing quotes."""
            while True:
                try:
                    quote = await asyncio.wait_for(
                        quote_queue.get(),
                        timeout=stream_config.operation_timeout.total_seconds()
                    )
                    if quote is None:  # Shutdown signal
                        break
                    yield quote
                except asyncio.TimeoutError:
                    continue
                except Exception as e:
                    logger.error(f"Error in quote iterator: {e}")
                    break

        # Establish bidirectional stream
        update_stream = self._stub.StreamQuotes(
            quote_iterator(),
            metadata=self._get_metadata()
        )

        logger.debug("gRPC streaming connection established successfully")
        return QuoteStreamHandle(quote_queue, update_stream, stream_config)

    async def start_swap_streaming(
        self, config: Optional[StreamConfig] = None
    ) -> SwapStreamHandle:
        """
        Start a bidirectional gRPC streaming connection for swap updates.

        Args:
            config: Optional stream configuration

        Returns:
            SwapStreamHandle for managing the stream
        """
        stream_config = config or StreamConfig()
        logger.info("Starting bidirectional gRPC swap streaming connection")

        # Create queue for outgoing swaps
        swap_queue = asyncio.Queue(maxsize=stream_config.send_buffer_size)

        async def swap_iterator():
            """Async generator for outgoing swaps."""
            while True:
                try:
                    swap = await asyncio.wait_for(
                        swap_queue.get(),
                        timeout=stream_config.operation_timeout.total_seconds()
                    )
                    if swap is None:  # Shutdown signal
                        break
                    yield swap
                except asyncio.TimeoutError:
                    continue
                except Exception as e:
                    logger.error(f"Error in swap iterator: {e}")
                    break

        # Establish bidirectional stream
        update_stream = self._stub.StreamSwap(
            swap_iterator(),
            metadata=self._get_metadata()
        )

        logger.debug("gRPC swap streaming connection established successfully")
        return SwapStreamHandle(swap_queue, update_stream, stream_config)

    async def get_last_sequence_number(
        self, maker_id: str, auth_token: str
    ) -> int:
        """
        Get the last sequence number for a maker (for synchronization before streaming).

        Args:
            maker_id: RFQv2 identifier
            auth_token: Authentication token

        Returns:
            Last sequence number for the maker
        """
        logger.debug(f"Getting last sequence number for maker: {maker_id}")

        request = SequenceNumberRequest(
            maker_id=maker_id,
            auth_token=auth_token
        )

        try:
            response = await self._stub.GetLastSequenceNumber(request)

            if response.success:
                logger.debug(
                    f"Retrieved last sequence number for maker {maker_id}: "
                    f"{response.last_sequence_number}"
                )
                return response.last_sequence_number
            else:
                logger.warning(
                    f"Failed to get sequence number for maker {maker_id}: "
                    f"{response.message}"
                )
                return 0
        except grpc.RpcError as e:
            logger.error(f"gRPC error getting sequence number: {e}")
            raise

    async def get_all_orderbooks(
        self, cluster: Optional[int] = None
    ):
        """
        Get all orderbooks for a specific cluster or all clusters.

        Args:
            cluster: Optional cluster filter (mainnet/devnet)

        Returns:
            GetAllOrderbooksResponse containing orderbooks
        """
        logger.debug("Receiving update for all orderbooks")

        request = GetAllOrderbooksRequest()
        if cluster is not None:
            request.cluster = cluster

        try:
            response = await self._stub.GetAllOrderbooks(request)

            logger.info(
                f"Retrieved {len(response.orderbooks)} orderbooks at "
                f"timestamp {response.timestamp}"
            )
            return response
        except grpc.RpcError as e:
            logger.error(f"gRPC error getting orderbooks: {e}")
            raise

    async def start_quote_streaming_with_sync(
        self,
        maker_id: str,
        auth_token: str,
        stream_config: Optional[StreamConfig] = None
    ) -> Tuple[QuoteStreamHandle, int]:
        """
        Start streaming with automatic sequence number synchronization.

        Args:
            maker_id: RFQv2 identifier
            auth_token: Authentication token
            stream_config: Optional stream configuration

        Returns:
            Tuple of (QuoteStreamHandle, next_sequence_number)
        """
        logger.debug(
            f"Starting streaming with sequence number synchronization for maker: {maker_id}"
        )

        last_sequence = await self.get_last_sequence_number(maker_id, auth_token)
        stream_handle = await self.start_quote_streaming(stream_config)
        next_sequence = last_sequence + 1

        logger.debug(
            f"Sequence sync complete for maker {maker_id}: "
            f"last={last_sequence}, next={next_sequence}"
        )

        return stream_handle, next_sequence

    async def close(self):
        """Close the gRPC connection."""
        if self._channel:
            logger.info("Closing gRPC connection")
            await self._channel.close()
            self._channel = None
            self._stub = None

    @staticmethod
    async def shutdown_stream_with_timeout(
        stream: QuoteStreamHandle,
        timeout: float
    ):
        """
        Properly shutdown a streaming connection with timeout.

        Args:
            stream: The stream to shutdown
            timeout: Timeout in seconds
        """
        logger.info(f"Shutting down streaming connection with timeout: {timeout}s")
        try:
            await asyncio.wait_for(stream.close(), timeout=timeout)
            logger.info("Stream shutdown completed successfully")
        except asyncio.TimeoutError:
            logger.warning(f"Stream shutdown timed out after {timeout}s")
            raise

    @staticmethod
    async def shutdown_stream_with_stats(
        stream: QuoteStreamHandle,
        timeout: float
    ):
        """
        Shutdown a stream with statistics reporting.

        Args:
            stream: The stream to shutdown
            timeout: Timeout in seconds
        """
        logger.info("Collecting final statistics before shutdown")
        stats = stream.get_stats()

        logger.info("Final Stream Statistics:")
        logger.info(f"  Messages sent: {stats['messages_sent']}")
        logger.info(f"  Updates received: {stats['updates_received']}")
        logger.info(f"  Errors encountered: {stats['errors_encountered']}")

        connected_duration = datetime.now() - stats['connected_at']
        logger.info(f"  Connected for: {connected_duration}")

        await MarketMakerClient.shutdown_stream_with_timeout(stream, timeout)
