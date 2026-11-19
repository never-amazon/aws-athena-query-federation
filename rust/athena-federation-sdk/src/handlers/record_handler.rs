use std::sync::Arc;

use async_trait::async_trait;

use super::handler_context::HandlerContext;
use super::metadata_handler::require_identity;
use crate::data::{BlockAllocator, BlockSpiller, S3BlockSpiller};
use crate::domain::predicate::ConstraintEvaluator;
use crate::exceptions::{FederationError, Result};
use crate::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::request::{FederationRequest, FederationResponse};
use crate::serde::ObjectMapper;
use crate::QueryStatusChecker;

#[async_trait]
pub trait RecordHandler: Send + Sync {
    fn context(&self) -> &HandlerContext;

    async fn read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        read_records(self, allocator, request).await
    }

    async fn read_with_constraint(
        &self,
        spiller: &dyn BlockSpiller,
        records_request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()>;
}

pub async fn read_records<H: RecordHandler + ?Sized>(
    handler: &H,
    allocator: &Arc<dyn BlockAllocator>,
    request: &ReadRecordsRequest,
) -> Result<FederationResponse> {
    tracing::info!(
        "doReadRecords: {:?}:{}",
        request.schema(),
        request
            .split()
            .spill_location()
            .map_or_else(|| "null".to_string(), ToString::to_string)
    );
    let cx = handler.context();
    let identity = require_identity(request.identity())?;
    let override_config = cx
        .request_override_config(Some(identity.config_options()))
        .await?;
    let spill_config = cx.spill_config(request)?;
    let s3_client = cx.s3_client(override_config.as_ref());
    let schema = Arc::new(request.schema().clone());
    let evaluator = Arc::new(ConstraintEvaluator::new(
        request.schema(),
        request.constraints().clone(),
    ));
    let spiller = S3BlockSpiller::new(
        s3_client,
        spill_config.clone(),
        Arc::clone(allocator),
        Arc::clone(&schema),
        evaluator,
        cx.config_options().clone(),
    )?;
    let query_status_checker =
        cx.query_status_checker(override_config.as_ref(), request.query_id());
    let result = async {
        handler
            .read_with_constraint(&spiller, request, &query_status_checker)
            .await?;
        if spiller.spilled().await? {
            Ok(FederationResponse::from(RemoteReadRecordsResponse::new(
                request.catalog_name(),
                request.schema().clone(),
                spiller.get_spill_locations().await?,
                spill_config.encryption_key.clone(),
            )))
        } else {
            Ok(FederationResponse::from(ReadRecordsResponse::new(
                request.catalog_name(),
                spiller.get_block().await?,
            )))
        }
    }
    .await;
    spiller.close().await;
    drop(query_status_checker);
    result
}

pub(crate) async fn dispatch<H: RecordHandler + ?Sized>(
    handler: &H,
    allocator: &Arc<dyn BlockAllocator>,
    object_mapper: &ObjectMapper,
    request: &FederationRequest,
) -> Result<Vec<u8>> {
    tracing::info!("doHandleRequest: request[{}]", request);
    match request {
        FederationRequest::ReadRecords(req) => {
            let response = handler.read_records(allocator, req).await?;
            tracing::info!("doHandleRequest: response[{}]", response);
            object_mapper
                .write_response(&response)
                .map(String::into_bytes)
        }
        other => Err(FederationError::invalid_input(format!(
            "Unknown request type {}",
            other.type_name()
        ))),
    }
}
