use std::sync::Arc;

use async_trait::async_trait;

use super::federation_capabilities::CAPABILITIES;
use super::metadata_handler::{self, MetadataHandler};
use super::record_handler::{self, RecordHandler};
use super::user_defined_function_handler::{self, UserDefinedFunctionHandler};
use crate::data::{BlockAllocator, BlockAllocatorImpl};
use crate::exceptions::{
    AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode,
    JavaThrowable, Result,
};
use crate::lambda::{LambdaContext, RequestStreamHandler};
use crate::request::{FederationRequest, PingRequest, PingResponse};
use crate::serde::{ObjectMapper, SerdeVersion, SERDE_VERSION};

#[async_trait]
pub(crate) trait Route: Sync {
    const VERSION_FALLBACK: bool = false;

    fn source_type(&self) -> &str;

    async fn dispatch(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: FederationRequest,
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>>;

    fn map_error(&self, error: FederationError) -> FederationError {
        tracing::warn!("handleRequest: Completed with an exception. {}", error);
        error.into_runtime_exception()
    }
}

pub(crate) async fn serve<R: Route + ?Sized>(
    route: &R,
    input: &[u8],
    context: Option<&LambdaContext>,
) -> Result<Vec<u8>> {
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let result = async {
        let (object_mapper, request) = if R::VERSION_FALLBACK {
            ObjectMapper::read_request_with_version_fallback(&allocator, input)?
        } else {
            let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&allocator));
            let request = object_mapper.read_request(input)?;
            (object_mapper, request)
        };
        if let FederationRequest::Ping(ping) = &request {
            return object_mapper
                .write_response(&ping_response(route.source_type(), ping).into())
                .map(String::into_bytes);
        }
        route
            .dispatch(&allocator, &object_mapper, request, context)
            .await
    }
    .await;
    result.map_err(|e| route.map_error(e))
}

fn ping_response(source_type: &str, request: &PingRequest) -> PingResponse {
    PingResponse::new(
        request.catalog_name(),
        request.query_id(),
        Some(source_type.to_string()),
        CAPABILITIES,
        SERDE_VERSION,
    )
}

pub(crate) fn java_class_of_request(request: &FederationRequest) -> String {
    let package = match request {
        FederationRequest::Ping(_) => "request",
        FederationRequest::ReadRecords(_) => "records",
        FederationRequest::UserDefinedFunction(_) => "udf",
        _ => "metadata",
    };
    format!(
        "class com.amazonaws.athena.connector.lambda.{package}.{}",
        request.type_name()
    )
}

pub struct MetadataStreamHandler<H: ?Sized = dyn MetadataHandler>(Arc<H>);

impl<H: MetadataHandler + ?Sized> MetadataStreamHandler<H> {
    pub fn new(handler: Arc<H>) -> Self {
        MetadataStreamHandler(handler)
    }
}

#[async_trait]
impl<H: MetadataHandler + ?Sized> Route for MetadataStreamHandler<H> {
    fn source_type(&self) -> &str {
        self.0.context().handler().source_type()
    }

    async fn dispatch(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: FederationRequest,
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        if !request.is_metadata_request() {
            return Err(FederationError::invalid_input(format!(
                "Expected a MetadataRequest but found {}",
                java_class_of_request(&request)
            )));
        }
        let request = request.with_context(context.cloned());
        metadata_handler::dispatch(self.0.as_ref(), allocator.as_ref(), object_mapper, &request)
            .await
    }

    fn map_error(&self, error: FederationError) -> FederationError {
        tracing::warn!("handleRequest: Completed with an exception. {}", error);
        if error.is_runtime_exception() {
            return error;
        }
        let message = error.to_string();
        FederationError::AthenaConnector(AthenaConnectorException::with_response(
            Arc::new(error),
            message,
            ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
        ))
    }
}

pub struct RecordStreamHandler<H: ?Sized = dyn RecordHandler>(Arc<H>);

impl<H: RecordHandler + ?Sized> RecordStreamHandler<H> {
    pub fn new(handler: Arc<H>) -> Self {
        RecordStreamHandler(handler)
    }
}

#[async_trait]
impl<H: RecordHandler + ?Sized> Route for RecordStreamHandler<H> {
    fn source_type(&self) -> &str {
        self.0.context().source_type()
    }

    async fn dispatch(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: FederationRequest,
        _context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        if !request.is_record_request() {
            return Err(FederationError::invalid_input(format!(
                "Expected a RecordRequest but found {}",
                java_class_of_request(&request)
            )));
        }
        record_handler::dispatch(self.0.as_ref(), allocator, object_mapper, &request).await
    }
}

pub struct UserDefinedFunctionStreamHandler<H: ?Sized = dyn UserDefinedFunctionHandler>(Arc<H>);

impl<H: UserDefinedFunctionHandler + ?Sized> UserDefinedFunctionStreamHandler<H> {
    pub fn new(handler: Arc<H>) -> Self {
        UserDefinedFunctionStreamHandler(handler)
    }
}

#[async_trait]
impl<H: UserDefinedFunctionHandler + ?Sized> Route for UserDefinedFunctionStreamHandler<H> {
    fn source_type(&self) -> &str {
        self.0.source_type()
    }

    async fn dispatch(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: FederationRequest,
        _context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        match &request {
            FederationRequest::UserDefinedFunction(request) => {
                user_defined_function_handler::dispatch(
                    self.0.as_ref(),
                    allocator.as_ref(),
                    object_mapper,
                    request,
                )
            }
            other => Err(FederationError::invalid_input(format!(
                "Expected a UserDefinedFunctionRequest but found {}",
                java_class_of_request(other)
            ))),
        }
    }

    fn map_error(&self, error: FederationError) -> FederationError {
        error.into_runtime_exception()
    }
}

macro_rules! stream_handler {
    ($name:ident, $handler:ident) => {
        #[async_trait]
        impl<H: $handler + ?Sized> RequestStreamHandler for $name<H> {
            async fn handle_request(
                &self,
                input: &[u8],
                context: Option<&LambdaContext>,
            ) -> Result<Vec<u8>> {
                serve(self, input, context).await
            }
        }
    };
}

stream_handler!(MetadataStreamHandler, MetadataHandler);
stream_handler!(RecordStreamHandler, RecordHandler);
stream_handler!(UserDefinedFunctionStreamHandler, UserDefinedFunctionHandler);
