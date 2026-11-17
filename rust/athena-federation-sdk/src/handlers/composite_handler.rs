use std::sync::Arc;

use async_trait::async_trait;

use super::metadata_handler::{java_class_of_request, MetadataHandler, MetadataHandlerExt};
use super::record_handler::{RecordHandler, RecordHandlerExt};
use super::user_defined_function_handler::{
    UserDefinedFunctionHandler, UserDefinedFunctionHandlerExt,
};
use crate::data::{BlockAllocator, BlockAllocatorImpl};
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::lambda::{LambdaContext, RequestStreamHandler};
use crate::request::FederationRequest;
use crate::security::AwsService;
use crate::serde::{ObjectMapper, VersionedObjectMapperFactory};

#[derive(Clone)]
pub struct CompositeHandler {
    metadata: Arc<dyn MetadataHandler>,
    record: Arc<dyn RecordHandler>,
    udf: Option<Arc<dyn UserDefinedFunctionHandler>>,
}

impl CompositeHandler {
    pub fn new(
        metadata_handler: Arc<dyn MetadataHandler>,
        record_handler: Arc<dyn RecordHandler>,
    ) -> CompositeHandler {
        CompositeHandler {
            metadata: metadata_handler,
            record: record_handler,
            udf: None,
        }
    }

    pub fn new_with_udf_handler(
        metadata_handler: Arc<dyn MetadataHandler>,
        record_handler: Arc<dyn RecordHandler>,
        udf_handler: Arc<dyn UserDefinedFunctionHandler>,
    ) -> CompositeHandler {
        CompositeHandler {
            metadata: metadata_handler,
            record: record_handler,
            udf: Some(udf_handler),
        }
    }

    pub fn metadata_handler(&self) -> &Arc<dyn MetadataHandler> {
        &self.metadata
    }

    pub fn record_handler(&self) -> &Arc<dyn RecordHandler> {
        &self.record
    }

    pub fn udf_handler(&self) -> Option<&Arc<dyn UserDefinedFunctionHandler>> {
        self.udf.as_ref()
    }

    pub async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        let block_allocator = Arc::new(BlockAllocatorImpl::new());
        let allocator: Arc<dyn BlockAllocator> = block_allocator.clone();
        let result = async {
            let (object_mapper, mut raw_req) =
                VersionedObjectMapperFactory::read_request_with_version_fallback(
                    &allocator, input,
                )?;
            if raw_req.is_metadata_request() {
                raw_req.set_context(context.cloned());
            }
            self.handle_request_with_object_mapper(&allocator, &raw_req, &object_mapper)
                .await
        }
        .await;
        block_allocator.close();
        result.map_err(|ex| {
            tracing::warn!("handleRequest: Completed with an exception. {}", ex);
            ex.into_runtime_exception()
        })
    }

    pub async fn handle_request_with_object_mapper(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        raw_req: &FederationRequest,
        object_mapper: &ObjectMapper,
    ) -> Result<Vec<u8>> {
        self.route(allocator, raw_req, object_mapper)
            .await
            .map_err(|e| self.handle_exception(e))
    }

    async fn route(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        raw_req: &FederationRequest,
        object_mapper: &ObjectMapper,
    ) -> Result<Vec<u8>> {
        if let FederationRequest::Ping(ping) = raw_req {
            let response = self.metadata.do_ping(ping).await;
            return object_mapper.write_value_as_bytes(&response);
        }
        match (raw_req, &self.udf) {
            (request, _) if request.is_metadata_request() => {
                self.metadata
                    .do_handle_request(allocator, object_mapper, request)
                    .await
            }
            (request, _) if request.is_record_request() => {
                self.record
                    .do_handle_request(allocator, object_mapper, request)
                    .await
            }
            (FederationRequest::UserDefinedFunction(request), Some(udf_handler)) => {
                udf_handler
                    .do_handle_request(allocator, object_mapper, request)
                    .await
            }
            (request, _) => Err(FederationError::invalid_input(format!(
                "Unknown request class {}",
                java_class_of_request(request)
            ))),
        }
    }

    pub fn handle_exception(&self, e: FederationError) -> FederationError {
        match &e {
            FederationError::AwsService(aws) if aws.is_service_exception_of(AwsService::Kms) => {
                FederationError::invalid_input(e.to_string())
            }
            _ => e,
        }
    }

    pub fn map_http_error_code(error_code: i32) -> FederationSourceErrorCode {
        match error_code {
            400 | 409 => FederationSourceErrorCode::InvalidInputException,
            401 => FederationSourceErrorCode::InvalidCredentialsException,
            403 => FederationSourceErrorCode::AccessDeniedException,
            404 => FederationSourceErrorCode::EntityNotFoundException,
            408 | 504 => FederationSourceErrorCode::OperationTimeoutException,
            429 => FederationSourceErrorCode::ThrottlingException,
            500 | 502 | 503 => FederationSourceErrorCode::InternalServiceException,
            501 => FederationSourceErrorCode::OperationNotSupportedException,
            other => {
                tracing::warn!("Unknown error code: {}", other);
                FederationSourceErrorCode::InvalidInputException
            }
        }
    }
}

#[async_trait]
impl RequestStreamHandler for CompositeHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        CompositeHandler::handle_request(self, input, context).await
    }
}
