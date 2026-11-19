use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use indexmap::IndexMap;
use tokio::sync::Mutex;

use super::aws_clients::AwsClients;
use super::handler_context::{ConfigOptions, HandlerContext};
use crate::connection::environment_constants;
use crate::data::block_utils;
use crate::data::types::java_field_to_string;
use crate::data::{
    BlockAllocator, S3HeadBucketClient, SchemaBuilder, SimpleBlockWriter, SupportedTypes,
};
use crate::domain::predicate::ConstraintEvaluator;
use crate::domain::spill::{S3SpillLocation, SpillLocation, SpillLocationVerifier};
use crate::domain::split::{Split, SplitBuilder};
use crate::exceptions::{FederationError, Result};
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse, MetadataHeader,
};
use crate::request::{FederationRequest, FederationResponse};
use crate::security::{
    AwsRequestOverrideConfiguration, EncryptionKey, FederatedIdentity, KeySource,
};
use crate::serde::ObjectMapper;
use crate::QueryStatusChecker;

pub const PARTITION_ID_COL: &str = "partitionId";
const DISABLE_ENCRYPTION: &str = "true";
pub const DEFAULT_SPILL_PREFIX: &str = "athena-federation-spill";
pub const SPILL_BUCKET_ENV: &str = "spill_bucket";
pub const SPILL_PREFIX_ENV: &str = "spill_prefix";
pub const KMS_KEY_ID_ENV: &str = "kms_key_id";
pub const DISABLE_SPILL_ENCRYPTION: &str = "disable_spill_encryption";

pub struct MetadataContext {
    handler: HandlerContext,
    encryption_key_factory: Option<KeySource>,
    spill_bucket: Option<String>,
    spill_prefix: Option<String>,
    verifier: Mutex<SpillLocationVerifier<S3HeadBucketClient>>,
}

pub struct MetadataContextBuilder {
    source_type: String,
    clients: AwsClients,
    config_options: HashMap<String, String>,
    encryption_key_factory: Setting<Option<KeySource>>,
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

impl MetadataContextBuilder {
    #[must_use]
    pub fn config_options(mut self, config_options: HashMap<String, String>) -> Self {
        self.config_options = config_options;
        self
    }

    #[must_use]
    pub fn encryption_key_factory(mut self, encryption_key_factory: Option<KeySource>) -> Self {
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

    pub fn build(self) -> Result<MetadataContext> {
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
        let verifier =
            SpillLocationVerifier::new(S3HeadBucketClient::new(Arc::clone(&self.clients.s3)));
        Ok(MetadataContext {
            handler: HandlerContext::new(self.source_type, self.clients, config_options)?,
            encryption_key_factory,
            spill_bucket,
            spill_prefix,
            verifier: Mutex::new(verifier),
        })
    }
}

fn default_encryption_key_factory(
    config_options: &HashMap<String, String>,
    clients: &AwsClients,
) -> Option<KeySource> {
    let disable = config_options
        .get(DISABLE_SPILL_ENCRYPTION)
        .map_or("false", String::as_str);
    if DISABLE_ENCRYPTION.eq_ignore_ascii_case(disable) {
        tracing::debug!("DISABLE_SPILL_ENCRYPTION");
        return None;
    }
    let factory = if let Some(key_id) = config_options.get(KMS_KEY_ID_ENV) {
        tracing::debug!("ENABLE_SPILL_ENCRYPTION with encryption factory: KmsKeyFactory");
        KeySource::kms(Arc::clone(&clients.kms), key_id.clone())
    } else {
        tracing::debug!("ENABLE_SPILL_ENCRYPTION with encryption factory: LocalKeyFactory");
        KeySource::Local
    };
    Some(factory)
}

impl MetadataContext {
    pub fn builder(source_type: impl Into<String>, clients: AwsClients) -> MetadataContextBuilder {
        MetadataContextBuilder {
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
    ) -> Result<MetadataContext> {
        MetadataContext::builder(source_type, AwsClients::from_environment().await?)
            .config_options(config_options)
            .build()
    }

    pub fn handler(&self) -> &HandlerContext {
        &self.handler
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        self.handler.config_options()
    }

    pub fn encryption_key_factory(&self) -> Option<&KeySource> {
        self.encryption_key_factory.as_ref()
    }

    pub fn spill_bucket(&self) -> Option<&str> {
        self.spill_bucket.as_deref()
    }

    pub fn spill_prefix(&self) -> Option<&str> {
        self.spill_prefix.as_deref()
    }

    pub async fn make_encryption_key(
        &self,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<EncryptionKey>> {
        match &self.encryption_key_factory {
            Some(factory) => factory.create(override_config).await.map(Some),
            None => Ok(None),
        }
    }

    pub fn make_spill_location(&self, request: MetadataHeader<'_>) -> Result<SpillLocation> {
        let identity_options = require_identity(request.identity)?.config_options();
        let config_options: HashMap<String, String> = if identity_options.is_empty() {
            tracing::warn!("configOptions is empty from federation. Use default configOptions.");
            self.config_options().clone()
        } else {
            identity_options
                .iter()
                .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
                .collect()
        };
        let prefix = if is_blank(config_options.get(SPILL_PREFIX_ENV).map(String::as_str)) {
            self.spill_prefix.clone()
        } else {
            config_options.get(SPILL_PREFIX_ENV).cloned()
        };
        let bucket = if is_blank(config_options.get(SPILL_BUCKET_ENV).map(String::as_str)) {
            self.spill_bucket.clone()
        } else {
            config_options.get(SPILL_BUCKET_ENV).cloned()
        };
        let request_query_id = request.query_id.ok_or_else(|| {
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

    pub async fn split_builder(&self, request: &GetSplitsRequest) -> Result<SplitBuilder> {
        let spill_location = self.make_spill_location(request.header())?;
        let override_config = self
            .handler
            .identity_override_config(request.identity())
            .await?;
        let encryption_key = self.make_encryption_key(override_config.as_ref()).await?;
        Ok(Split::new_builder(Some(spill_location), encryption_key))
    }

    async fn check_spill_bucket(&self, identity: &FederatedIdentity) -> Result<()> {
        let options: &ConfigOptions = identity.config_options();
        let mut verifier = self.verifier.lock().await;
        if options
            .get(environment_constants::FAS_TOKEN)
            .is_some_and(Option::is_some)
        {
            let override_config = self.handler.request_override_config(Some(options)).await?;
            let s3 = self.handler.s3_client(override_config.as_ref());
            *verifier = SpillLocationVerifier::new(S3HeadBucketClient::new(s3));
        }
        verifier.check_bucket_auth_z(self.spill_bucket()).await
    }
}

pub(crate) fn require_identity(identity: Option<&FederatedIdentity>) -> Result<&FederatedIdentity> {
    identity.ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"com.amazonaws.athena.connector.lambda.security.FederatedIdentity.getConfigOptions()\" because \"federatedIdentity\" is null")
    })
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
pub trait MetadataHandler: Send + Sync {
    fn context(&self) -> &MetadataContext;

    async fn list_schema_names(&self, request: &ListSchemasRequest) -> Result<ListSchemasResponse>;

    async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse>;

    async fn get_query_passthrough_schema(
        &self,
        _request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        Err(FederationError::not_supported("Not implemented"))
    }

    async fn get_table(&self, request: &GetTableRequest) -> Result<GetTableResponse>;

    async fn get_table_layout(
        &self,
        allocator: &dyn BlockAllocator,
        request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        table_layout(self, allocator, request).await
    }

    fn enhance_partition_schema(
        &self,
        _partition_schema_builder: &mut SchemaBuilder,
        _request: &GetTableLayoutRequest,
    ) {
    }

    async fn get_partitions(
        &self,
        block_writer: &mut SimpleBlockWriter<'_>,
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()>;

    async fn get_splits(&self, request: &GetSplitsRequest) -> Result<GetSplitsResponse>;

    async fn get_data_source_capabilities(
        &self,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            IndexMap::new(),
        ))
    }
}

pub async fn table_layout<H: MetadataHandler + ?Sized>(
    handler: &H,
    allocator: &dyn BlockAllocator,
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
    handler.enhance_partition_schema(&mut partition_schema_builder, request);
    let partition_schema = partition_schema_builder.build();
    if partition_schema.fields().is_empty() && partition_schema.metadata().is_empty() {
        let partitions =
            block_utils::new_block(allocator, PARTITION_ID_COL, &DataType::Int32, [1i32])?;
        return Ok(GetTableLayoutResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            partitions,
        ));
    }
    let identity = require_identity(request.identity())?;
    let cx = handler.context().handler();
    let override_config = cx
        .request_override_config(Some(identity.config_options()))
        .await?;
    let constraint_evaluator = Arc::new(ConstraintEvaluator::new(
        &constraint_schema.build(),
        request.constraints().clone(),
    ));
    let query_status_checker = cx.query_status_checker(
        override_config.as_ref(),
        request.query_id().unwrap_or_default(),
    );
    let mut partitions = allocator.create_block(Arc::new(partition_schema_builder.build()));
    partitions.constrain(Some(constraint_evaluator));
    let mut block_writer = SimpleBlockWriter::new(&mut partitions);
    let result = handler
        .get_partitions(&mut block_writer, request, &query_status_checker)
        .await;
    drop(query_status_checker);
    result?;
    Ok(GetTableLayoutResponse::new(
        request.catalog_name(),
        request.table_name().clone(),
        partitions,
    ))
}

pub(crate) async fn dispatch<H: MetadataHandler + ?Sized>(
    handler: &H,
    allocator: &dyn BlockAllocator,
    object_mapper: &ObjectMapper,
    request: &FederationRequest,
) -> Result<Vec<u8>> {
    tracing::info!("doHandleRequest: request[{}]", request);
    let response: FederationResponse = match request {
        FederationRequest::ListSchemas(req) => handler.list_schema_names(req).await?.into(),
        FederationRequest::ListTables(req) => handler.list_tables(req).await?.into(),
        FederationRequest::GetTable(req) => {
            tracing::info!(
                "resolveDoGetTableImplementation: resolving implementation - isQueryPassthrough[{}]",
                req.is_query_passthrough()
            );
            let response = if req.is_query_passthrough() {
                handler.get_query_passthrough_schema(req).await?
            } else {
                handler.get_table(req).await?
            };
            tracing::info!("doHandleRequest: response[{}]", response);
            assert_types(response.schema())?;
            return object_mapper
                .write_response(&response.into())
                .map(String::into_bytes);
        }
        FederationRequest::GetTableLayout(req) => {
            handler.get_table_layout(allocator, req).await?.into()
        }
        FederationRequest::GetSplits(req) => {
            handler
                .context()
                .check_spill_bucket(require_identity(req.identity())?)
                .await?;
            handler.get_splits(req).await?.into()
        }
        FederationRequest::GetDataSourceCapabilities(req) => {
            handler.get_data_source_capabilities(req).await?.into()
        }
        other => {
            return Err(FederationError::invalid_input(format!(
                "Unknown request type {}",
                other.type_name()
            )))
        }
    };
    tracing::info!("doHandleRequest: response[{}]", response);
    object_mapper
        .write_response(&response)
        .map(String::into_bytes)
}

fn assert_types(schema: &Schema) -> Result<()> {
    for next in schema.fields() {
        SupportedTypes::assert_supported(next)?;
    }
    Ok(())
}
