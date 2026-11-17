use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use indexmap::IndexMap;

use athena_federation_sdk::data::{BlockAllocator, BlockWriter};
use athena_federation_sdk::handlers::{
    FederationRequestHandler, MetadataHandler, MetadataHandlerBase,
};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};

use crate::manager::{
    JdbcMetadataHandler, JdbcMetadataHandlerBase, JdbcMetadataHandlerExt,
    JdbcMetadataHandlerFactory, JdbcUtil,
};

const MAX_CATALOGS_TO_MULTIPLEX: usize = 100;

pub const CATALOG_NOT_REGISTERED_ERROR_TEMPLATE: &str = "Catalog is not supported in multiplexer. After registering the catalog in Athena, must set '%s_connection_string' environment variable in Lambda. See JDBC connector README for further details.";

pub(crate) fn catalog_not_registered(catalog_name: &str) -> FederationError {
    FederationError::invalid_input(
        CATALOG_NOT_REGISTERED_ERROR_TEMPLATE.replace("%s", catalog_name),
    )
}

pub(crate) fn validate_handler_map<V>(map: &IndexMap<String, V>, message: &str) -> Result<()> {
    if map.is_empty() {
        return Err(FederationError::IllegalArgument(message.to_string()));
    }
    Ok(())
}

pub(crate) fn validate_catalog_count<V>(map: &IndexMap<String, V>) -> Result<()> {
    if map.len() > MAX_CATALOGS_TO_MULTIPLEX {
        return Err(FederationError::invalid_input(
            "Max 100 catalogs supported in multiplexer.",
        ));
    }
    Ok(())
}

pub struct MultiplexingJdbcMetadataHandler {
    base: JdbcMetadataHandlerBase,
    metadata_handler_map: IndexMap<String, Arc<dyn JdbcMetadataHandler>>,
}

impl MultiplexingJdbcMetadataHandler {
    pub fn new(
        base: JdbcMetadataHandlerBase,
        metadata_handler_map: IndexMap<String, Arc<dyn JdbcMetadataHandler>>,
    ) -> Result<MultiplexingJdbcMetadataHandler> {
        validate_handler_map(
            &metadata_handler_map,
            "metadataHandlerMap must not be empty",
        )?;
        validate_catalog_count(&metadata_handler_map)?;
        Ok(MultiplexingJdbcMetadataHandler {
            base,
            metadata_handler_map,
        })
    }

    pub async fn new_with_factory(
        metadata_base: MetadataHandlerBase,
        jdbc_metadata_handler_factory: &dyn JdbcMetadataHandlerFactory,
        config_options: &HashMap<String, String>,
    ) -> Result<MultiplexingJdbcMetadataHandler> {
        let metadata_handler_map = JdbcUtil::create_jdbc_metadata_handler_map(
            config_options,
            jdbc_metadata_handler_factory,
        )
        .await?;
        validate_handler_map(&metadata_handler_map, "Could not find any delegatee.")?;
        Ok(MultiplexingJdbcMetadataHandler {
            base: JdbcMetadataHandlerBase::new_without_connection(metadata_base),
            metadata_handler_map,
        })
    }

    pub fn metadata_handler_map(&self) -> &IndexMap<String, Arc<dyn JdbcMetadataHandler>> {
        &self.metadata_handler_map
    }

    fn validate_multiplexer(&self, catalog_name: &str) -> Result<&Arc<dyn JdbcMetadataHandler>> {
        self.metadata_handler_map
            .get(catalog_name)
            .ok_or_else(|| catalog_not_registered(catalog_name))
    }
}

impl FederationRequestHandler for MultiplexingJdbcMetadataHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.metadata_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.metadata_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

#[async_trait]
impl MetadataHandler for MultiplexingJdbcMetadataHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        self.base.metadata_base()
    }

    async fn do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_list_schema_names(allocator, request)
            .await
    }

    async fn do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_list_tables(allocator, request)
            .await
    }

    async fn do_get_query_passthrough_schema(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_get_query_passthrough_schema(allocator, request)
            .await
    }

    async fn do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_get_table(allocator, request)
            .await
    }

    async fn do_get_table_layout(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_get_table_layout(allocator, request)
            .await
    }

    async fn get_partitions(
        &self,
        block_writer: &mut (dyn BlockWriter + Send),
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.validate_multiplexer(request.catalog_name())?
            .get_partitions(block_writer, request, query_status_checker)
            .await
    }

    async fn do_get_splits(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_get_splits(allocator, request)
            .await
    }

    async fn do_get_data_source_capabilities(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        self.validate_multiplexer(request.catalog_name())?
            .do_get_data_source_capabilities(allocator, request)
            .await
    }
}

#[async_trait]
impl JdbcMetadataHandler for MultiplexingJdbcMetadataHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase {
        &self.base
    }

    fn get_partition_schema(&self, catalog_name: &str) -> Result<Schema> {
        self.validate_multiplexer(catalog_name)?
            .get_partition_schema(catalog_name)
    }

    async fn close(&self) -> Result<()> {
        let mut first_failure = None;
        for (catalog, delegate) in &self.metadata_handler_map {
            if let Err(e) = delegate.close().await {
                tracing::warn!(
                    "Failed to close metadata handler for catalog {} {}",
                    catalog,
                    e
                );
                first_failure.get_or_insert(e);
            }
        }
        if let Some(factory) = self.base.jdbc_connection_factory() {
            if let Err(e) = factory.close().await {
                first_failure.get_or_insert(e);
            }
        }
        first_failure.map_or(Ok(()), Err)
    }
}
