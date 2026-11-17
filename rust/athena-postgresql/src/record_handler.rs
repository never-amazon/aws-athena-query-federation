use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::{ArrowType, BlockAllocator, BlockSpiller, SupportedTypes};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    AwsClients, FederationRequestHandler, RecordHandler, RecordHandlerBase,
};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{
    get_arrow_type_from_jdbc_type, JdbcArrowTypeConverter, JdbcRecordHandler,
    JdbcRecordHandlerBase, JdbcRecordHandlerExt, JdbcSplitQueryBuilder, JdbcUtil,
};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement, ResultSetMetaData, SqlException, Types};

use crate::constants::PostGreSqlConstants;
use crate::federation_expression_parser::PostgreSqlFederationExpressionParser;
use crate::metadata_handler::PostGreSqlMetadataHandler;
use crate::query_string_builder::PostGreSqlQueryStringBuilder;

const FETCH_SIZE: i32 = 1000;

pub struct PostGreSqlRecordHandler {
    base: JdbcRecordHandlerBase,
    jdbc_split_query_builder: Arc<dyn JdbcSplitQueryBuilder>,
}

fn internal_service_error(error: FederationError) -> FederationError {
    match SqlException::from_federation_error(&error) {
        Some(sql) => FederationError::internal(sql.message()),
        None => error,
    }
}

impl PostGreSqlRecordHandler {
    pub fn default_query_builder() -> Arc<dyn JdbcSplitQueryBuilder> {
        Arc::new(PostGreSqlQueryStringBuilder::new(
            PostGreSqlConstants::POSTGRES_QUOTE_CHARACTER,
            Arc::new(PostgreSqlFederationExpressionParser::new(
                PostGreSqlConstants::POSTGRES_QUOTE_CHARACTER,
            )),
        ))
    }

    pub fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlRecordHandler> {
        let config = JdbcUtil::get_single_database_config_from_env(
            PostGreSqlConstants::POSTGRES_NAME,
            &config_options,
        )?;
        PostGreSqlRecordHandler::new(clients, config, config_options)
    }

    pub fn new(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlRecordHandler> {
        let factory = Arc::new(PostGreSqlMetadataHandler::connection_factory(
            &config,
            &config_options,
        )?);
        PostGreSqlRecordHandler::with_query_builder(
            clients,
            config,
            factory,
            PostGreSqlRecordHandler::default_query_builder(),
            config_options,
        )
    }

    pub fn with_query_builder(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        jdbc_split_query_builder: Arc<dyn JdbcSplitQueryBuilder>,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlRecordHandler> {
        let record_base = RecordHandlerBase::new(config.engine(), clients, config_options)?;
        Ok(PostGreSqlRecordHandler {
            base: JdbcRecordHandlerBase::new(record_base, config, jdbc_connection_factory),
            jdbc_split_query_builder,
        })
    }

    fn export_arrow_type(&self, metadata: &ResultSetMetaData, column: usize) -> Result<ArrowType> {
        let column_type = metadata.column_type(column)?;
        let precision = metadata.precision(column)?;
        let scale = metadata.scale(column)?;
        if column_type == Types::NUMERIC && precision == 0 && scale <= 0 {
            return Ok(JdbcArrowTypeConverter::to_arrow_type(
                Types::NUMERIC,
                0,
                0,
                self.base.config_options(),
            )?
            .unwrap_or(DataType::Utf8));
        }
        match get_arrow_type_from_jdbc_type(column_type, precision, scale, Some("UTC")) {
            Ok(arrow_type)
                if SupportedTypes::is_supported_arrow_type(&arrow_type)
                    && !matches!(arrow_type, DataType::List(_)) =>
            {
                Ok(arrow_type)
            }
            Ok(_) | Err(FederationError::UnsupportedOperation(_)) => Ok(DataType::Utf8),
            Err(other) => Err(other),
        }
    }
}

impl FederationRequestHandler for PostGreSqlRecordHandler {
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
impl RecordHandler for PostGreSqlRecordHandler {
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
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.jdbc_read_with_constraint(spiller, request, query_status_checker)
            .await
    }
}

#[async_trait]
impl JdbcRecordHandler for PostGreSqlRecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase {
        &self.base
    }

    async fn build_split_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        _catalog_name: &str,
        table_name: &TableName,
        schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> Result<PreparedStatement> {
        let result = async {
            let mut prepared_statement = if constraints.is_query_pass_through() {
                self.build_query_passthrough_sql(jdbc_connection, constraints)
                    .await?
            } else {
                self.jdbc_split_query_builder
                    .build_sql(
                        jdbc_connection,
                        "",
                        table_name.schema_name(),
                        table_name.table_name(),
                        schema,
                        constraints,
                        split,
                    )
                    .await?
            };
            prepared_statement.set_fetch_size(FETCH_SIZE)?;
            Ok(prepared_statement)
        }
        .await;
        result.map_err(internal_service_error)
    }

    async fn prepare_export_statement(
        &self,
        connection: &dyn JdbcConnection,
        query: &str,
    ) -> Result<PreparedStatement> {
        let mut prepared_statement = connection.prepare_statement(query).await?;
        prepared_statement.set_fetch_size(FETCH_SIZE)?;
        Ok(prepared_statement)
    }

    fn derive_export_schema(&self, metadata: &ResultSetMetaData) -> Result<Schema> {
        let mut fields = Vec::with_capacity(metadata.column_count());
        for column in 1..=metadata.column_count() {
            fields.push(Field::new(
                metadata.column_label(column)?,
                self.export_arrow_type(metadata, column)?,
                true,
            ));
        }
        Ok(Schema::new(fields))
    }
}
