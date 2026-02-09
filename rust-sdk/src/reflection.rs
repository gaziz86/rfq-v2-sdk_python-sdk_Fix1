//! gRPC Server Reflection client for discovering services and methods at runtime

use crate::error::{MarketMakerError, Result};
use prost::Message;
use prost_types::{
    DescriptorProto, FileDescriptorProto, MethodDescriptorProto, ServiceDescriptorProto,
};
use std::time::Duration;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic_reflection::pb::v1::{
    server_reflection_client::ServerReflectionClient as TonicReflectionClient,
    server_reflection_request::MessageRequest, server_reflection_response::MessageResponse,
    ServerReflectionRequest,
};
use tracing::{debug, error, info};

/// High-level client for querying gRPC server reflection.
pub struct ReflectionClient {
    channel: Channel,
}

/// Cached endpoint string used for deferred connections
#[derive(Clone)]
pub struct ReflectionHandle {
    endpoint: String,
}

impl ReflectionHandle {
    /// Create a new reflection handle from an endpoint URL
    pub fn new(endpoint: String) -> Self {
        Self { endpoint }
    }

    /// Connect and return a live [`ReflectionClient`]
    pub async fn connect(&self) -> Result<ReflectionClient> {
        ReflectionClient::connect(&self.endpoint).await
    }

    /// Convenience: list services in one shot
    pub async fn list_services(&self) -> Result<Vec<String>> {
        self.connect().await?.list_services().await
    }

    /// Convenience: verify the MarketMaker service in one shot
    pub async fn verify_market_maker_service(
        &self,
    ) -> Result<ServiceInfo> {
        self.connect().await?.verify_market_maker_service().await
    }
}

/// Information about a gRPC service discovered via reflection
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Fully qualified service name (e.g. `market_maker.MarketMakerIngestionService`)
    pub name: String,
    /// Methods exposed by the service
    pub methods: Vec<MethodInfo>,
}

/// Information about a gRPC method discovered via reflection
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// Method name (e.g. `StreamQuotes`)
    pub name: String,
    /// Fully qualified input type (e.g. `.market_maker.MarketMakerQuote`)
    pub input_type: String,
    /// Fully qualified output type (e.g. `.market_maker.QuoteUpdate`)
    pub output_type: String,
    /// Whether the client sends a stream
    pub client_streaming: bool,
    /// Whether the server returns a stream
    pub server_streaming: bool,
}

/// Information about a protobuf message type discovered via reflection
#[derive(Debug, Clone)]
pub struct MessageInfo {
    /// Message name (e.g. `MarketMakerQuote`)
    pub name: String,
    /// Fields in the message
    pub fields: Vec<FieldInfo>,
}

/// Information about a protobuf message field
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Field name
    pub name: String,
    /// Field number
    pub number: i32,
    /// Field type name (e.g. `string`, `uint64`, or a message type name)
    pub type_name: String,
    /// Whether the field is repeated
    pub is_repeated: bool,
    /// Whether the field is required (proto2)
    pub is_required: bool,
    /// Whether the field is optional
    pub is_optional: bool,
}

impl ReflectionClient {
    /// Connect to a gRPC server's reflection service
    pub async fn connect<S: Into<String>>(endpoint: S) -> Result<Self> {
        let endpoint_str: String = endpoint.into();
        info!(
            "Connecting to gRPC reflection service at {}",
            endpoint_str
        );
        let _ = rustls::crypto::ring::default_provider().install_default();

        let mut ep = Endpoint::try_from(endpoint_str.clone())
            .map_err(|e| MarketMakerError::configuration(format!("Invalid endpoint: {}", e)))?
            .timeout(Duration::from_secs(crate::DEFAULT_TIMEOUT_SECS));

        if endpoint_str.starts_with("https://") {
            let tls_config = ClientTlsConfig::new().with_native_roots();
            ep = ep.tls_config(tls_config).map_err(|e| {
                MarketMakerError::configuration(format!("TLS configuration failed: {}", e))
            })?;
        }

        let channel = ep.connect().await.map_err(|e| {
            error!("Reflection connection error: {}", e);
            MarketMakerError::Connection(e)
        })?;

        debug!("Connected to reflection service");
        Ok(Self { channel })
    }

    /// Create a reflection client from an existing channel (shares the connection)
    pub fn from_channel(channel: Channel) -> Self {
        Self { channel }
    }

    /// List all services advertised by the server
    pub async fn list_services(&self) -> Result<Vec<String>> {
        let response = self
            .make_reflection_request(MessageRequest::ListServices(String::new()))
            .await?;

        match response {
            MessageResponse::ListServicesResponse(list) => {
                let services: Vec<String> =
                    list.service.into_iter().map(|s| s.name).collect();
                debug!("Discovered {} services", services.len());
                Ok(services)
            }
            _ => Err(MarketMakerError::other(
                "Unexpected reflection response for list_services",
            )),
        }
    }

    /// Get the file descriptor for the file containing the given symbol (service or message)
    pub async fn file_descriptor_by_symbol(
        &self,
        symbol: &str,
    ) -> Result<Vec<FileDescriptorProto>> {
        let response = self
            .make_reflection_request(MessageRequest::FileContainingSymbol(symbol.to_string()))
            .await?;

        Self::parse_file_descriptor_response(response)
    }

    /// Get the file descriptor for a given proto filename
    pub async fn file_descriptor_by_filename(
        &self,
        filename: &str,
    ) -> Result<Vec<FileDescriptorProto>> {
        let response = self
            .make_reflection_request(MessageRequest::FileByFilename(filename.to_string()))
            .await?;

        Self::parse_file_descriptor_response(response)
    }

    /// List all methods for a given fully-qualified service name
    pub async fn list_methods(&self, service_name: &str) -> Result<Vec<String>> {
        let info = self.get_service_info(service_name).await?;
        Ok(info.methods.into_iter().map(|m| m.name).collect())
    }

    /// Get detailed information about a service, including its methods and their types
    pub async fn get_service_info(&self, service_name: &str) -> Result<ServiceInfo> {
        let file_descriptors = self.file_descriptor_by_symbol(service_name).await?;
        let short_name = service_name
            .rsplit('.')
            .next()
            .unwrap_or(service_name);

        for fd in &file_descriptors {
            for service in &fd.service {
                if service.name.as_deref() == Some(short_name) {
                    return Ok(Self::build_service_info(service_name, service));
                }
            }
        }

        Err(MarketMakerError::other(format!(
            "Service '{}' not found in file descriptors",
            service_name
        )))
    }

    /// Get detailed information about a protobuf message type
    pub async fn get_message_info(&self, message_name: &str) -> Result<MessageInfo> {
        let file_descriptors = self.file_descriptor_by_symbol(message_name).await?;

        let short_name = message_name
            .rsplit('.')
            .next()
            .unwrap_or(message_name);

        for fd in &file_descriptors {
            if let Some(info) = Self::find_message_in_file(short_name, fd) {
                return Ok(info);
            }
        }

        Err(MarketMakerError::other(format!(
            "Message '{}' not found in file descriptors",
            message_name
        )))
    }

    /// Get full service information for all services advertised by the server
    pub async fn get_all_service_info(&self) -> Result<Vec<ServiceInfo>> {
        let service_names = self.list_services().await?;
        let mut services = Vec::with_capacity(service_names.len());

        for name in service_names {
            match self.get_service_info(&name).await {
                Ok(info) => services.push(info),
                Err(e) => {
                    debug!(
                        "Skipping service '{}' (reflection metadata unavailable): {}",
                        name, e
                    );
                }
            }
        }

        Ok(services)
    }

    /// Verify that the expected `MarketMakerIngestionService` is available on the server
    pub async fn verify_market_maker_service(&self) -> Result<ServiceInfo> {
        let services = self.list_services().await?;

        let service_name = services
            .iter()
            .find(|s| s.contains("MarketMakerIngestionService"))
            .cloned()
            .ok_or_else(|| {
                MarketMakerError::other(format!(
                    "MarketMakerIngestionService not found. Available services: {:?}",
                    services
                ))
            })?;

        self.get_service_info(&service_name).await
    }

    /// Send a single reflection request and return the message response
    async fn make_reflection_request(
        &self,
        message_request: MessageRequest,
    ) -> Result<MessageResponse> {
        let mut client = TonicReflectionClient::new(self.channel.clone());

        let request = ServerReflectionRequest {
            host: String::new(),
            message_request: Some(message_request),
        };

        let response_stream = client
            .server_reflection_info(tokio_stream::once(request))
            .await
            .map_err(MarketMakerError::Grpc)?;

        let mut inbound = response_stream.into_inner();

        use tokio_stream::StreamExt;
        let response = inbound
            .next()
            .await
            .ok_or_else(|| MarketMakerError::other("Empty reflection response stream"))?
            .map_err(MarketMakerError::Grpc)?;

        response
            .message_response
            .ok_or_else(|| MarketMakerError::other("Reflection response missing message_response"))
    }

    fn parse_file_descriptor_response(
        response: MessageResponse,
    ) -> Result<Vec<FileDescriptorProto>> {
        match response {
            MessageResponse::FileDescriptorResponse(fd_response) => {
                let mut descriptors = Vec::new();
                for encoded in fd_response.file_descriptor_proto {
                    let fd = FileDescriptorProto::decode(encoded.as_slice()).map_err(|e| {
                        MarketMakerError::other(format!(
                            "Failed to decode FileDescriptorProto: {}",
                            e
                        ))
                    })?;
                    descriptors.push(fd);
                }
                Ok(descriptors)
            }
            MessageResponse::ErrorResponse(err) => Err(MarketMakerError::other(format!(
                "Reflection error (code {}): {}",
                err.error_code, err.error_message
            ))),
            _ => Err(MarketMakerError::other(
                "Unexpected reflection response type",
            )),
        }
    }

    fn build_service_info(
        fully_qualified_name: &str,
        service: &ServiceDescriptorProto,
    ) -> ServiceInfo {
        let methods = service
            .method
            .iter()
            .map(|m| Self::build_method_info(m))
            .collect();

        ServiceInfo {
            name: fully_qualified_name.to_string(),
            methods,
        }
    }

    fn build_method_info(method: &MethodDescriptorProto) -> MethodInfo {
        MethodInfo {
            name: method.name.clone().unwrap_or_default(),
            input_type: method.input_type.clone().unwrap_or_default(),
            output_type: method.output_type.clone().unwrap_or_default(),
            client_streaming: method.client_streaming.unwrap_or(false),
            server_streaming: method.server_streaming.unwrap_or(false),
        }
    }

    /// Recursively search for a message type in a file descriptor
    fn find_message_in_file(name: &str, fd: &FileDescriptorProto) -> Option<MessageInfo> {
        for msg in &fd.message_type {
            if let Some(info) = Self::find_message_recursive(name, msg) {
                return Some(info);
            }
        }
        None
    }

    /// Recursively search nested message types
    fn find_message_recursive(name: &str, msg: &DescriptorProto) -> Option<MessageInfo> {
        if msg.name.as_deref() == Some(name) {
            return Some(Self::build_message_info(msg));
        }

        // Search nested types
        for nested in &msg.nested_type {
            if let Some(info) = Self::find_message_recursive(name, nested) {
                return Some(info);
            }
        }
        None
    }

    /// Build a `MessageInfo` from a protobuf `DescriptorProto`
    fn build_message_info(msg: &DescriptorProto) -> MessageInfo {
        use prost_types::field_descriptor_proto::{Label, Type};

        let fields = msg
            .field
            .iter()
            .map(|f| {
                let type_name = if let Some(ref tn) = f.type_name {
                    // Strip leading dot from fully qualified type names
                    tn.strip_prefix('.').unwrap_or(tn).to_string()
                } else {
                    // Use the scalar type name
                    match f.r#type() {
                        Type::Double => "double".to_string(),
                        Type::Float => "float".to_string(),
                        Type::Int64 => "int64".to_string(),
                        Type::Uint64 => "uint64".to_string(),
                        Type::Int32 => "int32".to_string(),
                        Type::Fixed64 => "fixed64".to_string(),
                        Type::Fixed32 => "fixed32".to_string(),
                        Type::Bool => "bool".to_string(),
                        Type::String => "string".to_string(),
                        Type::Bytes => "bytes".to_string(),
                        Type::Uint32 => "uint32".to_string(),
                        Type::Sfixed32 => "sfixed32".to_string(),
                        Type::Sfixed64 => "sfixed64".to_string(),
                        Type::Sint32 => "sint32".to_string(),
                        Type::Sint64 => "sint64".to_string(),
                        Type::Group => "group".to_string(),
                        Type::Message => "message".to_string(),
                        Type::Enum => "enum".to_string(),
                    }
                };

                FieldInfo {
                    name: f.name.clone().unwrap_or_default(),
                    number: f.number.unwrap_or(0),
                    type_name,
                    is_repeated: f.label() == Label::Repeated,
                    is_required: f.label() == Label::Required,
                    is_optional: f.label() == Label::Optional,
                }
            })
            .collect();

        MessageInfo {
            name: msg.name.clone().unwrap_or_default(),
            fields,
        }
    }
}

impl std::fmt::Display for ServiceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Service: {}", self.name)?;
        for method in &self.methods {
            write!(f, "  {}", method)?;
        }
        Ok(())
    }
}

impl std::fmt::Display for MethodInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let streaming = match (self.client_streaming, self.server_streaming) {
            (true, true) => " [bidirectional streaming]",
            (true, false) => " [client streaming]",
            (false, true) => " [server streaming]",
            (false, false) => "",
        };
        writeln!(
            f,
            "rpc {}({}) returns ({}){}",
            self.name, self.input_type, self.output_type, streaming
        )
    }
}

impl std::fmt::Display for MessageInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "message {} {{", self.name)?;
        for field in &self.fields {
            let label = if field.is_required {
                "required "
            } else if field.is_repeated {
                "repeated "
            } else {
                "optional "
            };
            writeln!(
                f,
                "  {}{} {} = {};",
                label, field.type_name, field.name, field.number
            )?;
        }
        writeln!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_info_display() {
        let method = MethodInfo {
            name: "StreamQuotes".to_string(),
            input_type: ".market_maker.MarketMakerQuote".to_string(),
            output_type: ".market_maker.QuoteUpdate".to_string(),
            client_streaming: true,
            server_streaming: true,
        };
        let display = format!("{}", method);
        assert!(display.contains("StreamQuotes"));
        assert!(display.contains("bidirectional streaming"));
    }

    #[test]
    fn test_service_info_display() {
        let service = ServiceInfo {
            name: "market_maker.MarketMakerIngestionService".to_string(),
            methods: vec![
                MethodInfo {
                    name: "GetLastSequenceNumber".to_string(),
                    input_type: ".market_maker.SequenceNumberRequest".to_string(),
                    output_type: ".market_maker.SequenceNumberResponse".to_string(),
                    client_streaming: false,
                    server_streaming: false,
                },
                MethodInfo {
                    name: "StreamQuotes".to_string(),
                    input_type: ".market_maker.MarketMakerQuote".to_string(),
                    output_type: ".market_maker.QuoteUpdate".to_string(),
                    client_streaming: true,
                    server_streaming: true,
                },
            ],
        };
        let display = format!("{}", service);
        assert!(display.contains("MarketMakerIngestionService"));
        assert!(display.contains("GetLastSequenceNumber"));
        assert!(display.contains("StreamQuotes"));
    }

    #[test]
    fn test_message_info_display() {
        let msg = MessageInfo {
            name: "Token".to_string(),
            fields: vec![
                FieldInfo {
                    name: "address".to_string(),
                    number: 1,
                    type_name: "string".to_string(),
                    is_repeated: false,
                    is_required: true,
                    is_optional: false,
                },
                FieldInfo {
                    name: "decimals".to_string(),
                    number: 2,
                    type_name: "uint32".to_string(),
                    is_repeated: false,
                    is_required: true,
                    is_optional: false,
                },
            ],
        };
        let display = format!("{}", msg);
        assert!(display.contains("message Token"));
        assert!(display.contains("required string address = 1"));
    }
}
