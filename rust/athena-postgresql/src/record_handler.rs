use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::{minor_type_of, ArrowType};
use athena_federation_sdk::handlers::{AwsClients, HandlerContext};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::arrow_type::{get_arrow_type_from_jdbc_type, jdbc_to_arrow_type};
use athena_jdbc::connection::single_database_config_from_env;
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::handler::{ConnectionSource, JdbcRecordHandler, RecordDialect};
use athena_jdbc::internal_service_error;
use athena_jdbc::query::SplitQuery;
use athena_jdbc::sql::{
    ColumnMetadata, JdbcConnection, JdbcType, PreparedStatement, ResultSetMetaData,
};

use crate::constants;
use crate::metadata_handler::PostGreSqlMetadataDialect;
use crate::query_string_builder::{self, query_string_builder, PostGreSqlQueryStringBuilder};

const FETCH_SIZE: u32 = 1000;

pub struct PostGreSqlRecordDialect {
    query_builder: PostGreSqlQueryStringBuilder,
}

pub type PostGreSqlRecordHandler = JdbcRecordHandler<PostGreSqlRecordDialect>;

impl PostGreSqlRecordDialect {
    pub fn handler(
        context: HandlerContext,
        config: DatabaseConnectionConfig,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
    ) -> PostGreSqlRecordHandler {
        JdbcRecordHandler::new(
            context,
            ConnectionSource::Direct {
                config,
                factory: connection_factory,
            },
            PostGreSqlRecordDialect {
                query_builder: query_string_builder(),
            },
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
        Ok(PostGreSqlRecordDialect::handler(context, config, factory))
    }

    pub fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlRecordHandler> {
        let config = single_database_config_from_env(constants::POSTGRES_NAME, &config_options)?;
        PostGreSqlRecordDialect::from_config(clients, config, config_options)
    }
}

fn export_arrow_type(
    config_options: &HashMap<String, String>,
    column: &ColumnMetadata,
) -> Result<ArrowType> {
    let column_type = column.jdbc_type();
    let precision = column.precision();
    let scale = column.scale();
    if column_type == JdbcType::Numeric && precision == 0 && scale <= 0 {
        return Ok(
            jdbc_to_arrow_type(JdbcType::Numeric, 0, 0, config_options)?.unwrap_or(DataType::Utf8)
        );
    }
    match get_arrow_type_from_jdbc_type(column_type, precision, scale, Some("UTC")) {
        Ok(arrow_type)
            if minor_type_of(&arrow_type).is_supported()
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
                query_string_builder::build_sql(&self.query_builder, connection, query).await?
            };
            prepared_statement.set_fetch_size(FETCH_SIZE);
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
        prepared_statement.set_fetch_size(FETCH_SIZE);
        Ok(prepared_statement)
    }

    fn derive_export_schema(
        &self,
        handler: &PostGreSqlRecordHandler,
        metadata: &ResultSetMetaData,
    ) -> Result<Schema> {
        let mut fields = Vec::with_capacity(metadata.column_count());
        for column in metadata.columns() {
            fields.push(Field::new(
                column.label(),
                export_arrow_type(handler.config_options(), column)?,
                true,
            ));
        }
        Ok(Schema::new(fields))
    }
}
