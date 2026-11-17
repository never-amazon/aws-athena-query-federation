use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use indexmap::IndexMap;

use athena_federation_sdk::data::{BlockAllocator, BlockSpiller};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{FederationRequestHandler, RecordHandler, RecordHandlerBase};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{QueryStatusChecker, Result};

use crate::manager::{
    JdbcRecordHandler, JdbcRecordHandlerBase, JdbcRecordHandlerExt, JdbcRecordHandlerFactory,
    JdbcUtil,
};
use crate::multiplexing_jdbc_metadata_handler::{
    catalog_not_registered, validate_catalog_count, validate_handler_map,
};
use crate::sql::{JdbcConnection, PreparedStatement};
use crate::validate::null_pointer;

pub struct MultiplexingJdbcRecordHandler {
    base: JdbcRecordHandlerBase,
    record_handler_map: IndexMap<String, Arc<dyn JdbcRecordHandler>>,
}

impl MultiplexingJdbcRecordHandler {
    pub async fn new_with_factory(
        record_base: RecordHandlerBase,
        jdbc_record_handler_factory: &dyn JdbcRecordHandlerFactory,
        config_options: &HashMap<String, String>,
    ) -> Result<MultiplexingJdbcRecordHandler> {
        let record_handler_map =
            JdbcUtil::create_jdbc_record_handler_map(config_options, jdbc_record_handler_factory)
                .await?;
        validate_handler_map(&record_handler_map, "Could not find any delegatee.")?;
        Ok(MultiplexingJdbcRecordHandler {
            base: JdbcRecordHandlerBase::new_without_connection(record_base),
            record_handler_map,
        })
    }

    pub fn new(
        base: JdbcRecordHandlerBase,
        record_handler_map: IndexMap<String, Arc<dyn JdbcRecordHandler>>,
    ) -> Result<MultiplexingJdbcRecordHandler> {
        validate_handler_map(&record_handler_map, "recordHandlerMap must not be empty")?;
        validate_catalog_count(&record_handler_map)?;
        Ok(MultiplexingJdbcRecordHandler {
            base,
            record_handler_map,
        })
    }

    pub fn record_handler_map(&self) -> &IndexMap<String, Arc<dyn JdbcRecordHandler>> {
        &self.record_handler_map
    }

    fn validate_multiplexer(&self, catalog_name: &str) -> Result<&Arc<dyn JdbcRecordHandler>> {
        self.record_handler_map
            .get(catalog_name)
            .ok_or_else(|| catalog_not_registered(catalog_name))
    }
}

impl FederationRequestHandler for MultiplexingJdbcRecordHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.record_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.record_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

#[async_trait]
impl RecordHandler for MultiplexingJdbcRecordHandler {
    fn record_base(&self) -> &RecordHandlerBase {
        self.base.record_base()
    }

    async fn do_read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        self.jdbc_do_read_records(allocator, request).await
    }

    async fn read_with_constraint(
        &self,
        block_spiller: &dyn BlockSpiller,
        read_records_request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.validate_multiplexer(read_records_request.catalog_name())?
            .read_with_constraint(block_spiller, read_records_request, query_status_checker)
            .await
    }
}

#[async_trait]
impl JdbcRecordHandler for MultiplexingJdbcRecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase {
        &self.base
    }

    async fn build_split_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        catalog_name: &str,
        table_name: &TableName,
        schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> Result<PreparedStatement> {
        let delegate = self.record_handler_map.get(catalog_name).ok_or_else(|| {
            null_pointer("Cannot invoke \"com.amazonaws.athena.connectors.jdbc.manager.JdbcRecordHandler.buildSplitSql(java.sql.Connection, String, com.amazonaws.athena.connector.lambda.domain.TableName, org.apache.arrow.vector.types.pojo.Schema, com.amazonaws.athena.connector.lambda.domain.predicate.Constraints, com.amazonaws.athena.connector.lambda.domain.Split)\" because the return value of \"java.util.Map.get(Object)\" is null")
        })?;
        delegate
            .build_split_sql(
                jdbc_connection,
                catalog_name,
                table_name,
                schema,
                constraints,
                split,
            )
            .await
    }

    async fn close(&self) -> Result<()> {
        let mut first_failure = None;
        for (catalog, delegate) in &self.record_handler_map {
            if let Err(e) = delegate.close().await {
                tracing::warn!(
                    "Failed to close record handler for catalog {} {}",
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
