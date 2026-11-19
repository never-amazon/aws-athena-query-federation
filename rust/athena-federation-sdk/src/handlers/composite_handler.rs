use std::sync::Arc;

use async_trait::async_trait;

use super::metadata_handler::{self, MetadataHandler};
use super::record_handler::{self, RecordHandler};
use super::serve::{java_class_of_request, serve, Route};
use super::user_defined_function_handler::{self, UserDefinedFunctionHandler};
use crate::data::BlockAllocator;
use crate::exceptions::{FederationError, Result};
use crate::lambda::{LambdaContext, RequestStreamHandler};
use crate::request::FederationRequest;
use crate::security::AwsService;
use crate::serde::ObjectMapper;

#[derive(Clone)]
pub struct CompositeHandler {
    metadata: Arc<dyn MetadataHandler>,
    record: Arc<dyn RecordHandler>,
    udf: Option<Arc<dyn UserDefinedFunctionHandler>>,
}

impl CompositeHandler {
    pub fn new(
        metadata: Arc<dyn MetadataHandler>,
        record: Arc<dyn RecordHandler>,
        udf: Option<Arc<dyn UserDefinedFunctionHandler>>,
    ) -> CompositeHandler {
        CompositeHandler {
            metadata,
            record,
            udf,
        }
    }

    pub fn metadata_handler(&self) -> &dyn MetadataHandler {
        self.metadata.as_ref()
    }

    pub fn record_handler(&self) -> &dyn RecordHandler {
        self.record.as_ref()
    }

    pub async fn route(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: FederationRequest,
        object_mapper: &ObjectMapper,
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        self.route_request(allocator, request, object_mapper, context)
            .await
            .map_err(CompositeHandler::handle_exception)
    }

    async fn route_request(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: FederationRequest,
        object_mapper: &ObjectMapper,
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        match (request, &self.udf) {
            (request, _) if request.is_metadata_request() => {
                let request = request.with_context(context.cloned());
                metadata_handler::dispatch(
                    self.metadata.as_ref(),
                    allocator.as_ref(),
                    object_mapper,
                    &request,
                )
                .await
            }
            (request, _) if request.is_record_request() => {
                record_handler::dispatch(self.record.as_ref(), allocator, object_mapper, &request)
                    .await
            }
            (FederationRequest::UserDefinedFunction(request), Some(udf)) => {
                user_defined_function_handler::dispatch(
                    udf.as_ref(),
                    allocator.as_ref(),
                    object_mapper,
                    &request,
                )
            }
            (request, _) => Err(FederationError::invalid_input(format!(
                "Unknown request class {}",
                java_class_of_request(&request)
            ))),
        }
    }

    pub fn handle_exception(e: FederationError) -> FederationError {
        match &e {
            FederationError::AwsService(aws) if aws.is_service_exception_of(AwsService::Kms) => {
                FederationError::invalid_input(e.to_string())
            }
            _ => e,
        }
    }
}

#[async_trait]
impl Route for CompositeHandler {
    const VERSION_FALLBACK: bool = true;

    fn source_type(&self) -> &str {
        self.metadata.context().handler().source_type()
    }

    async fn dispatch(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: FederationRequest,
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        self.route(allocator, request, object_mapper, context).await
    }
}

#[async_trait]
impl RequestStreamHandler for CompositeHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        serve(self, input, context).await
    }
}
