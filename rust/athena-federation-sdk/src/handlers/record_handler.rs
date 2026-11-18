use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::athena_exception_filter::ATHENA_EXCEPTION_FILTER;
use super::aws_clients::AwsClients;
use super::federation_capabilities::CAPABILITIES;
use super::federation_request_handler::FederationRequestHandler;
use super::metadata_handler::{java_class_of_request, require_identity};
use crate::data::spill_config::SpillConfig;
use crate::data::{BlockAllocator, BlockAllocatorImpl, BlockSpiller, S3BlockSpiller, S3Client};
use crate::domain::predicate::ConstraintEvaluator;
use crate::exceptions::{FederationError, Result};
use crate::lambda::LambdaContext;
use crate::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::request::{FederationRequest, FederationResponse, PingRequest, PingResponse};
use crate::security::{AwsServiceError, CachableSecretsManager, KmsEncryptionProvider};
use crate::serde::{ObjectMapper, SerdeVersion, SERDE_VERSION};
use crate::{AthenaClient, QueryStatusChecker, ThrottlingInvoker};

const MAX_BLOCK_SIZE_BYTES: &str = "MAX_BLOCK_SIZE_BYTES";
const NUM_SPILL_THREADS: i32 = 2;

pub struct RecordHandlerBase {
    config_options: HashMap<String, String>,
    amazon_s3: Arc<dyn S3Client>,
    source_type: String,
    secrets_manager: CachableSecretsManager,
    athena: Arc<dyn AthenaClient>,
    athena_invoker: Arc<ThrottlingInvoker<AwsServiceError>>,
    kms_encryption_provider: Arc<KmsEncryptionProvider>,
}

impl RecordHandlerBase {
    pub fn new(
        source_type: impl Into<String>,
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<RecordHandlerBase> {
        let athena_invoker = ThrottlingInvoker::new_default_builder(
            Arc::new(ATHENA_EXCEPTION_FILTER),
            &config_options,
        )?
        .build()?;
        Ok(RecordHandlerBase {
            config_options,
            amazon_s3: clients.s3,
            source_type: source_type.into(),
            secrets_manager: CachableSecretsManager::new(clients.secrets_manager),
            athena: clients.athena,
            athena_invoker: Arc::new(athena_invoker),
            kms_encryption_provider: clients.kms_encryption_provider,
        })
    }

    pub async fn from_environment(
        source_type: impl Into<String>,
        config_options: HashMap<String, String>,
    ) -> Result<RecordHandlerBase> {
        RecordHandlerBase::new(
            source_type,
            AwsClients::from_environment().await?,
            config_options,
        )
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        &self.config_options
    }

    pub fn amazon_s3(&self) -> &Arc<dyn S3Client> {
        &self.amazon_s3
    }

    pub fn source_type(&self) -> &str {
        &self.source_type
    }

    pub fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        &self.secrets_manager
    }

    pub fn athena(&self) -> &Arc<dyn AthenaClient> {
        &self.athena
    }

    pub fn athena_invoker(&self) -> &Arc<ThrottlingInvoker<AwsServiceError>> {
        &self.athena_invoker
    }

    pub fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        &self.kms_encryption_provider
    }
}

#[async_trait]
pub trait RecordHandler: FederationRequestHandler {
    fn record_base(&self) -> &RecordHandlerBase;

    async fn do_read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        default_do_read_records(self, allocator, request).await
    }

    async fn read_with_constraint(
        &self,
        spiller: &dyn BlockSpiller,
        records_request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()>;

    fn get_spill_config(&self, request: &ReadRecordsRequest) -> Result<SpillConfig> {
        let mut max_block_size = request.max_block_size();
        if let Some(value) = self
            .record_base()
            .config_options()
            .get(MAX_BLOCK_SIZE_BYTES)
        {
            max_block_size = value.parse::<i64>().map_err(|_| {
                FederationError::NumberFormat(format!("For input string: \"{value}\""))
            })?;
        }
        let mut builder = SpillConfig::new_builder()
            .with_max_block_bytes(max_block_size)
            .with_max_inline_block_bytes(request.max_inline_block_size())
            .with_request_id(request.query_id())
            .with_encryption_key(request.split().encryption_key().cloned())
            .with_num_spill_threads(NUM_SPILL_THREADS);
        if let Some(location) = request.split().spill_location() {
            builder = builder.with_spill_location(location.clone());
        }
        builder.build()
    }

    async fn on_ping(&self, _request: &PingRequest) -> Result<()> {
        Ok(())
    }
}

pub async fn default_do_read_records<H: RecordHandler + ?Sized>(
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
    let base = handler.record_base();
    let identity = require_identity(request.identity())?;
    let override_config = handler
        .get_request_override_config_with_config_options(Some(identity.config_options()))
        .await?;
    let spill_config = handler.get_spill_config(request)?;
    let athena_client = handler.get_athena_client(override_config.as_ref(), base.athena());
    let s3_client = handler.get_s3_client(override_config.as_ref(), base.amazon_s3());
    let schema = Arc::new(request.schema().clone());
    let evaluator = Arc::new(ConstraintEvaluator::new(
        Some(allocator.as_ref()),
        request.schema(),
        request.constraints().clone(),
    ));
    let spiller = S3BlockSpiller::new(
        s3_client,
        spill_config.clone(),
        Arc::clone(allocator),
        Arc::clone(&schema),
        evaluator,
        base.config_options().clone(),
    )?;
    let query_status_checker = QueryStatusChecker::new(
        athena_client,
        Arc::clone(base.athena_invoker()),
        request.query_id(),
    );
    let result = async {
        handler
            .read_with_constraint(&spiller, request, &query_status_checker)
            .await?;
        if spiller.spilled().await? {
            Ok(FederationResponse::from(RemoteReadRecordsResponse::new(
                request.catalog_name(),
                request.schema().clone(),
                spiller.get_spill_locations().await?,
                spill_config.encryption_key().cloned(),
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

#[async_trait]
pub trait RecordHandlerExt: RecordHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>>;

    async fn do_handle_request(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: &FederationRequest,
    ) -> Result<Vec<u8>>;
}

async fn do_ping<H: RecordHandler + ?Sized>(handler: &H, request: &PingRequest) -> PingResponse {
    let response = PingResponse::new(
        request.catalog_name(),
        request.query_id(),
        Some(handler.record_base().source_type().to_string()),
        CAPABILITIES,
        SERDE_VERSION,
    );
    if let Err(ex) = handler.on_ping(request).await {
        tracing::warn!(
            "doPing: encountered an exception while delegating onPing. {}",
            ex
        );
    }
    response
}

#[async_trait]
impl<T: RecordHandler + ?Sized> RecordHandlerExt for T {
    async fn handle_request(
        &self,
        input: &[u8],
        _context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        let block_allocator = Arc::new(BlockAllocatorImpl::new());
        let allocator: Arc<dyn BlockAllocator> = block_allocator.clone();
        let result = async {
            let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&allocator));
            let raw_req = object_mapper.read_request(input)?;
            if let FederationRequest::Ping(ping) = &raw_req {
                let response = do_ping(self, ping).await;
                return object_mapper
                    .write_response(&response.into())
                    .map(String::into_bytes);
            }
            if !raw_req.is_record_request() {
                return Err(FederationError::invalid_input(format!(
                    "Expected a RecordRequest but found {}",
                    java_class_of_request(&raw_req)
                )));
            }
            self.do_handle_request(&allocator, &object_mapper, &raw_req)
                .await
        }
        .await;
        block_allocator.close();
        result.map_err(|ex| {
            tracing::warn!("handleRequest: Completed with an exception. {}", ex);
            ex.into_runtime_exception()
        })
    }

    async fn do_handle_request(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: &FederationRequest,
    ) -> Result<Vec<u8>> {
        tracing::info!("doHandleRequest: request[{}]", request);
        match request {
            FederationRequest::ReadRecords(req) => {
                let response = self.do_read_records(allocator, req).await?;
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
}
