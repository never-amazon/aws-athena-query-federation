use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use indexmap::IndexMap;
use tokio::sync::Mutex;

use super::athena_exception_filter::ATHENA_EXCEPTION_FILTER;
use super::aws_clients::AwsClients;
use super::federation_capabilities::CAPABILITIES;
use super::federation_request_handler::{ConfigOptions, FederationRequestHandler};
use crate::connection::EnvironmentConstants;
use crate::data::types::java_field_to_string;
use crate::data::{
    BlockAllocator, BlockAllocatorImpl, BlockUtils, BlockWriter, S3Client, S3HeadBucketClient,
    SchemaBuilder, SimpleBlockWriter, SupportedTypes,
};
use crate::domain::predicate::ConstraintEvaluator;
use crate::domain::spill::{S3SpillLocation, SpillLocation, SpillLocationVerifier};
use crate::exceptions::{
    AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode, Result,
};
use crate::lambda::LambdaContext;
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse, MetadataRequest,
};
use crate::request::{FederationRequest, PingRequest, PingResponse};
use crate::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, CachableSecretsManager, EncryptionKey,
    EncryptionKeyFactory, FederatedIdentity, KmsEncryptionProvider, KmsKeyFactory, LocalKeyFactory,
};
use crate::serde::{ObjectMapper, SerdeVersion, SERDE_VERSION};
use crate::{AthenaClient, QueryStatusChecker, ThrottlingInvoker};

pub const PARTITION_ID_COL: &str = "partitionId";
const DISABLE_ENCRYPTION: &str = "true";
pub const DEFAULT_SPILL_PREFIX: &str = "athena-federation-spill";
pub const SPILL_BUCKET_ENV: &str = "spill_bucket";
pub const SPILL_PREFIX_ENV: &str = "spill_prefix";
pub const KMS_KEY_ID_ENV: &str = "kms_key_id";
pub const DISABLE_SPILL_ENCRYPTION: &str = "disable_spill_encryption";

pub struct MetadataHandlerBase {
    config_options: HashMap<String, String>,
    secrets_manager: CachableSecretsManager,
    athena: Arc<dyn AthenaClient>,
    s3_client: Arc<dyn S3Client>,
    athena_invoker: Arc<ThrottlingInvoker<AwsServiceError>>,
    encryption_key_factory: Option<Arc<dyn EncryptionKeyFactory>>,
    spill_bucket: Option<String>,
    spill_prefix: Option<String>,
    source_type: String,
    verifier: Mutex<SpillLocationVerifier<S3HeadBucketClient>>,
    kms_encryption_provider: Arc<KmsEncryptionProvider>,
}

pub struct MetadataHandlerBaseBuilder {
    source_type: String,
    clients: AwsClients,
    config_options: HashMap<String, String>,
    encryption_key_factory: Setting<Option<Arc<dyn EncryptionKeyFactory>>>,
    spill_bucket: Setting<Option<String>>,
    spill_prefix: Setting<Option<String>>,
}

enum Setting<T> {
    FromConfig,
    Explicit(T),
}

impl<T> Setting<T> {
    fn unwrap_or_else(self, from_config: impl FnOnce() -> T) -> T {
        match self {
            Setting::FromConfig => from_config(),
            Setting::Explicit(value) => value,
        }
    }
}

impl MetadataHandlerBaseBuilder {
    #[must_use]
    pub fn config_options(mut self, config_options: HashMap<String, String>) -> Self {
        self.config_options = config_options;
        self
    }

    #[must_use]
    pub fn encryption_key_factory(
        mut self,
        encryption_key_factory: Option<Arc<dyn EncryptionKeyFactory>>,
    ) -> Self {
        self.encryption_key_factory = Setting::Explicit(encryption_key_factory);
        self
    }

    #[must_use]
    pub fn spill_bucket(mut self, spill_bucket: Option<String>) -> Self {
        self.spill_bucket = Setting::Explicit(spill_bucket);
        self
    }

    #[must_use]
    pub fn spill_prefix(mut self, spill_prefix: Option<String>) -> Self {
        self.spill_prefix = Setting::Explicit(spill_prefix);
        self
    }

    pub fn build(self) -> Result<MetadataHandlerBase> {
        let config_options = self.config_options;
        let spill_bucket = self
            .spill_bucket
            .unwrap_or_else(|| config_options.get(SPILL_BUCKET_ENV).cloned());
        let spill_prefix = self.spill_prefix.unwrap_or_else(|| {
            Some(
                config_options
                    .get(SPILL_PREFIX_ENV)
                    .cloned()
                    .unwrap_or_else(|| DEFAULT_SPILL_PREFIX.to_string()),
            )
        });
        let encryption_key_factory = self
            .encryption_key_factory
            .unwrap_or_else(|| default_encryption_key_factory(&config_options, &self.clients));
        let athena_invoker = ThrottlingInvoker::new_default_builder(
            Arc::new(ATHENA_EXCEPTION_FILTER),
            &config_options,
        )?
        .build()?;
        Ok(MetadataHandlerBase {
            secrets_manager: CachableSecretsManager::new(self.clients.secrets_manager),
            athena: self.clients.athena,
            verifier: Mutex::new(SpillLocationVerifier::new(S3HeadBucketClient::new(
                Arc::clone(&self.clients.s3),
            ))),
            s3_client: self.clients.s3,
            athena_invoker: Arc::new(athena_invoker),
            encryption_key_factory,
            spill_bucket,
            spill_prefix,
            source_type: self.source_type,
            kms_encryption_provider: self.clients.kms_encryption_provider,
            config_options,
        })
    }
}

fn default_encryption_key_factory(
    config_options: &HashMap<String, String>,
    clients: &AwsClients,
) -> Option<Arc<dyn EncryptionKeyFactory>> {
    let disable = config_options
        .get(DISABLE_SPILL_ENCRYPTION)
        .map_or("false", String::as_str);
    if DISABLE_ENCRYPTION.eq_ignore_ascii_case(disable) {
        tracing::debug!("DISABLE_SPILL_ENCRYPTION");
        return None;
    }
    let factory: Arc<dyn EncryptionKeyFactory> =
        if let Some(key_id) = config_options.get(KMS_KEY_ID_ENV) {
            tracing::debug!("ENABLE_SPILL_ENCRYPTION with encryption factory: KmsKeyFactory");
            Arc::new(KmsKeyFactory::new(Arc::clone(&clients.kms), key_id.clone()))
        } else {
            tracing::debug!("ENABLE_SPILL_ENCRYPTION with encryption factory: LocalKeyFactory");
            Arc::new(LocalKeyFactory::new())
        };
    Some(factory)
}

impl MetadataHandlerBase {
    pub fn builder(
        source_type: impl Into<String>,
        clients: AwsClients,
    ) -> MetadataHandlerBaseBuilder {
        MetadataHandlerBaseBuilder {
            source_type: source_type.into(),
            clients,
            config_options: HashMap::new(),
            encryption_key_factory: Setting::FromConfig,
            spill_bucket: Setting::FromConfig,
            spill_prefix: Setting::FromConfig,
        }
    }

    pub async fn from_environment(
        source_type: impl Into<String>,
        config_options: HashMap<String, String>,
    ) -> Result<MetadataHandlerBase> {
        MetadataHandlerBase::builder(source_type, AwsClients::from_environment().await?)
            .config_options(config_options)
            .build()
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        &self.config_options
    }

    pub fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        &self.secrets_manager
    }

    pub fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        &self.kms_encryption_provider
    }

    pub fn athena(&self) -> &Arc<dyn AthenaClient> {
        &self.athena
    }

    pub fn s3_client(&self) -> &Arc<dyn S3Client> {
        &self.s3_client
    }

    pub fn athena_invoker(&self) -> &Arc<ThrottlingInvoker<AwsServiceError>> {
        &self.athena_invoker
    }

    pub fn encryption_key_factory(&self) -> Option<&Arc<dyn EncryptionKeyFactory>> {
        self.encryption_key_factory.as_ref()
    }

    pub fn spill_bucket(&self) -> Option<&str> {
        self.spill_bucket.as_deref()
    }

    pub fn spill_prefix(&self) -> Option<&str> {
        self.spill_prefix.as_deref()
    }

    pub fn source_type(&self) -> &str {
        &self.source_type
    }

    pub async fn set_verifier(&self, verifier: SpillLocationVerifier<S3HeadBucketClient>) {
        *self.verifier.lock().await = verifier;
    }
}

pub(crate) fn require_identity(identity: Option<&FederatedIdentity>) -> Result<&FederatedIdentity> {
    identity.ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"com.amazonaws.athena.connector.lambda.security.FederatedIdentity.getConfigOptions()\" because \"federatedIdentity\" is null")
    })
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

fn find_field<'a>(schema: &'a Schema, name: &str) -> Result<&'a Field> {
    schema.field_with_name(name).map_err(|_| {
        let fields: Vec<String> = schema
            .fields()
            .iter()
            .map(|f| java_field_to_string(f))
            .collect();
        FederationError::IllegalArgument(format!(
            "field {name} not found in [{}]",
            fields.join(", ")
        ))
    })
}

fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty())
}

#[async_trait]
pub trait MetadataHandler: FederationRequestHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase;

    async fn make_encryption_key(&self) -> Result<Option<EncryptionKey>> {
        match self.metadata_base().encryption_key_factory() {
            Some(factory) => factory.create().await.map(Some),
            None => Ok(None),
        }
    }

    async fn make_encryption_key_with_aws_request_override_configuration(
        &self,
        aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<EncryptionKey>> {
        match self.metadata_base().encryption_key_factory() {
            Some(factory) => factory
                .create_with_override_configuration(aws_request_override_configuration)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    fn make_spill_location(&self, request: &dyn MetadataRequest) -> Result<SpillLocation> {
        let base = self.metadata_base();
        let identity_options = require_identity(request.identity())?.config_options();
        let config_options: HashMap<String, String> = if identity_options.is_empty() {
            tracing::warn!("configOptions is empty from federation. Use default configOptions.");
            base.config_options().clone()
        } else {
            identity_options
                .iter()
                .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
                .collect()
        };
        let prefix = if is_blank(config_options.get(SPILL_PREFIX_ENV).map(String::as_str)) {
            base.spill_prefix().map(str::to_string)
        } else {
            config_options.get(SPILL_PREFIX_ENV).cloned()
        };
        let bucket = if is_blank(config_options.get(SPILL_BUCKET_ENV).map(String::as_str)) {
            base.spill_bucket().map(str::to_string)
        } else {
            config_options.get(SPILL_BUCKET_ENV).cloned()
        };
        let request_query_id = request.query_id().ok_or_else(|| {
            FederationError::null_pointer(
                "Cannot invoke \"java.lang.CharSequence.toString()\" because \"s\" is null",
            )
        })?;
        let mut query_id = request_query_id.to_string();
        if prefix
            .as_deref()
            .is_some_and(|p| p.contains(request_query_id))
        {
            query_id = String::new();
        }
        let mut builder = S3SpillLocation::new_builder()
            .with_query_id(query_id)
            .with_split_id(uuid::Uuid::new_v4().to_string());
        if let Some(bucket) = bucket {
            builder = builder.with_bucket(bucket);
        }
        if let Some(prefix) = prefix {
            builder = builder.with_prefix(prefix);
        }
        Ok(SpillLocation::S3(builder.build()))
    }

    async fn do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse>;

    async fn do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse>;

    async fn do_get_query_passthrough_schema(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        Err(FederationError::not_supported("Not implemented"))
    }

    async fn do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse>;

    async fn do_get_table_layout(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        let mut constraint_schema = SchemaBuilder::new_builder();
        let mut partition_schema_builder = SchemaBuilder::new_builder();
        for next_part_col in request.partition_cols() {
            let partition_col = find_field(request.schema(), next_part_col)?;
            partition_schema_builder
                .add_field_with_type(next_part_col, partition_col.data_type().clone());
            constraint_schema.add_field_with_type(next_part_col, partition_col.data_type().clone());
        }
        self.enhance_partition_schema(&mut partition_schema_builder, request);
        let partition_schema = partition_schema_builder.build();
        if partition_schema.fields().is_empty() && partition_schema.metadata().is_empty() {
            let partitions = BlockUtils::new_block(
                allocator.as_ref(),
                PARTITION_ID_COL,
                &DataType::Int32,
                [1i32],
            )?;
            return Ok(GetTableLayoutResponse::new(
                request.catalog_name(),
                request.table_name().clone(),
                partitions,
            ));
        }
        let identity = require_identity(request.identity())?;
        let override_config = self
            .get_request_override_config_with_config_options(Some(identity.config_options()))
            .await?;
        let base = self.metadata_base();
        let constraint_evaluator = Arc::new(ConstraintEvaluator::new(
            Some(allocator.as_ref()),
            &constraint_schema.build(),
            request.constraints().clone(),
        ));
        let query_status_checker = QueryStatusChecker::new(
            self.get_athena_client(override_config.as_ref(), base.athena()),
            Arc::clone(base.athena_invoker()),
            request.query_id().unwrap_or_default(),
        );
        let mut partitions = allocator.create_block(Arc::new(partition_schema_builder.build()));
        partitions.constrain(Some(constraint_evaluator));
        let mut block_writer = SimpleBlockWriter::new(&mut partitions);
        let result = self
            .get_partitions(&mut block_writer, request, &query_status_checker)
            .await;
        query_status_checker.close();
        result?;
        Ok(GetTableLayoutResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            partitions,
        ))
    }

    fn enhance_partition_schema(
        &self,
        _partition_schema_builder: &mut SchemaBuilder,
        _request: &GetTableLayoutRequest,
    ) {
    }

    async fn get_partitions(
        &self,
        block_writer: &mut (dyn BlockWriter + Send),
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()>;

    async fn do_get_splits(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse>;

    async fn do_get_data_source_capabilities(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            IndexMap::new(),
        ))
    }

    async fn do_ping(&self, request: &PingRequest) -> PingResponse {
        let response = PingResponse::new(
            request.catalog_name(),
            request.query_id(),
            Some(self.metadata_base().source_type().to_string()),
            CAPABILITIES,
            SERDE_VERSION,
        );
        if let Err(ex) = self.on_ping(request).await {
            tracing::warn!(
                "doPing: encountered an exception while delegating onPing. {}",
                ex
            );
        }
        response
    }

    async fn on_ping(&self, _request: &PingRequest) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
pub trait MetadataHandlerExt: MetadataHandler {
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

#[async_trait]
impl<T: MetadataHandler + ?Sized> MetadataHandlerExt for T {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        let block_allocator = Arc::new(BlockAllocatorImpl::new());
        let allocator: Arc<dyn BlockAllocator> = block_allocator.clone();
        let result = async {
            let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&allocator));
            let mut raw_req = object_mapper.read_request(input)?;
            if let FederationRequest::Ping(ping) = &raw_req {
                let response = self.do_ping(ping).await;
                return object_mapper.write_value_as_bytes(&response);
            }
            if !raw_req.is_metadata_request() {
                return Err(FederationError::invalid_input(format!(
                    "Expected a MetadataRequest but found {}",
                    java_class_of_request(&raw_req)
                )));
            }
            raw_req.set_context(context.cloned());
            self.do_handle_request(&allocator, &object_mapper, &raw_req)
                .await
        }
        .await;
        block_allocator.close();
        result.map_err(|ex| {
            tracing::warn!("handleRequest: Completed with an exception. {}", ex);
            if ex.is_runtime_exception() {
                ex
            } else {
                let message = ex.to_string();
                FederationError::AthenaConnector(AthenaConnectorException::with_response(
                    Arc::new(ex),
                    message,
                    ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
                ))
            }
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
            FederationRequest::ListSchemas(req) => {
                let response = self.do_list_schema_names(allocator, req).await?;
                tracing::info!("doHandleRequest: response[{}]", response);
                object_mapper.write_value_as_bytes(&response)
            }
            FederationRequest::ListTables(req) => {
                let response = self.do_list_tables(allocator, req).await?;
                tracing::info!("doHandleRequest: response[{}]", response);
                object_mapper.write_value_as_bytes(&response)
            }
            FederationRequest::GetTable(req) => {
                tracing::info!(
                    "resolveDoGetTableImplementation: resolving implementation - isQueryPassthrough[{}]",
                    req.is_query_passthrough()
                );
                let response = if req.is_query_passthrough() {
                    self.do_get_query_passthrough_schema(allocator, req).await?
                } else {
                    self.do_get_table(allocator, req).await?
                };
                tracing::info!("doHandleRequest: response[{}]", response);
                assert_types(response.schema())?;
                object_mapper.write_value_as_bytes(&response)
            }
            FederationRequest::GetTableLayout(req) => {
                let response = self.do_get_table_layout(allocator, req).await?;
                tracing::info!("doHandleRequest: response[{}]", response);
                object_mapper.write_value_as_bytes(&response)
            }
            FederationRequest::GetSplits(req) => {
                let base = self.metadata_base();
                let identity = require_identity(req.identity())?;
                let options: &ConfigOptions = identity.config_options();
                if options
                    .get(EnvironmentConstants::FAS_TOKEN)
                    .is_some_and(Option::is_some)
                {
                    let override_config = self
                        .get_request_override_config_with_config_options(Some(options))
                        .await?;
                    let s3 = self.get_s3_client(override_config.as_ref(), base.s3_client());
                    base.set_verifier(SpillLocationVerifier::new(S3HeadBucketClient::new(s3)))
                        .await;
                }
                base.verifier
                    .lock()
                    .await
                    .check_bucket_auth_z(base.spill_bucket())
                    .await?;
                let response = self.do_get_splits(allocator, req).await?;
                tracing::info!("doHandleRequest: response[{}]", response);
                object_mapper.write_value_as_bytes(&response)
            }
            FederationRequest::GetDataSourceCapabilities(req) => {
                let response = self.do_get_data_source_capabilities(allocator, req).await?;
                tracing::info!("doHandleRequest: response[{}]", response);
                object_mapper.write_value_as_bytes(&response)
            }
            other => Err(FederationError::invalid_input(format!(
                "Unknown request type {}",
                other.type_name()
            ))),
        }
    }
}

fn assert_types(schema: &Schema) -> Result<()> {
    for next in schema.fields() {
        SupportedTypes::assert_supported(next)?;
    }
    Ok(())
}
