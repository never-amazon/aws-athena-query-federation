use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::{ArrowType, SupportedTypes};
use athena_federation_sdk::handlers::{AwsClients, HandlerContext};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{
    get_arrow_type_from_jdbc_type, ConnectionSource, JdbcArrowTypeConverter, JdbcRecordHandler,
    JdbcSplitQueryBuilder, JdbcUtil, RecordDialect, SplitQuery,
};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement, ResultSetMetaData, SqlException, Types};

use crate::constants::PostGreSqlConstants;
use crate::federation_expression_parser::PostgreSqlFederationExpressionParser;
use crate::metadata_handler::PostGreSqlMetadataDialect;
use crate::query_string_builder::PostGreSqlQueryStringBuilder;

const FETCH_SIZE: i32 = 1000;

pub struct PostGreSqlRecordDialect {
    query_builder: Arc<dyn JdbcSplitQueryBuilder>,
}

pub type PostGreSqlRecordHandler = JdbcRecordHandler<PostGreSqlRecordDialect>;

fn internal_service_error(error: FederationError) -> FederationError {
    match SqlException::from_federation_error(&error) {
        Some(sql) => FederationError::internal(sql.message()),
        None => error,
    }
}

pub fn default_query_builder() -> Arc<dyn JdbcSplitQueryBuilder> {
    Arc::new(PostGreSqlQueryStringBuilder::new(
        PostGreSqlConstants::POSTGRES_QUOTE_CHARACTER,
        Arc::new(PostgreSqlFederationExpressionParser::new(
            PostGreSqlConstants::POSTGRES_QUOTE_CHARACTER,
        )),
    ))
}

impl PostGreSqlRecordDialect {
    pub fn handler(
        context: HandlerContext,
        config: DatabaseConnectionConfig,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
        query_builder: Arc<dyn JdbcSplitQueryBuilder>,
    ) -> PostGreSqlRecordHandler {
        JdbcRecordHandler::new(
            context,
            ConnectionSource::Direct {
                config,
                factory: connection_factory,
            },
            PostGreSqlRecordDialect { query_builder },
        )
    }

    pub fn from_config(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlRecordHandler> {
        let factory = Arc::new(PostGreSqlMetadataDialect::connection_factory(
            &config,
            &config_options,
        )?);
        let context = HandlerContext::new(config.engine(), clients, config_options)?;
        Ok(PostGreSqlRecordDialect::handler(
            context,
            config,
            factory,
            default_query_builder(),
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
        PostGreSqlRecordDialect::from_config(clients, config, config_options)
    }
}

fn export_arrow_type(
    config_options: &HashMap<String, String>,
    metadata: &ResultSetMetaData,
    column: usize,
) -> Result<ArrowType> {
    let column_type = metadata.column_type(column)?;
    let precision = metadata.precision(column)?;
    let scale = metadata.scale(column)?;
    if column_type == Types::NUMERIC && precision == 0 && scale <= 0 {
        return Ok(
            JdbcArrowTypeConverter::to_arrow_type(Types::NUMERIC, 0, 0, config_options)?
                .unwrap_or(DataType::Utf8),
        );
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

#[async_trait]
impl RecordDialect for PostGreSqlRecordDialect {
    async fn build_split_sql(
        &self,
        handler: &PostGreSqlRecordHandler,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        let result = async {
            let mut prepared_statement = if query.constraints.is_query_pass_through() {
                handler
                    .build_query_passthrough_sql(connection, query.constraints)
                    .await?
            } else {
                self.query_builder
                    .build_sql(
                        connection,
                        "",
                        query.table_name.schema_name(),
                        query.table_name.table_name(),
                        query.schema,
                        query.constraints,
                        query.split,
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

    fn derive_export_schema(
        &self,
        handler: &PostGreSqlRecordHandler,
        metadata: &ResultSetMetaData,
    ) -> Result<Schema> {
        let mut fields = Vec::with_capacity(metadata.column_count());
        for column in 1..=metadata.column_count() {
            fields.push(Field::new(
                metadata.column_label(column)?,
                export_arrow_type(handler.config_options(), metadata, column)?,
                true,
            ));
        }
        Ok(Schema::new(fields))
    }
}
