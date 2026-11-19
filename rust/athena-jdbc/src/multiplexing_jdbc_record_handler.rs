use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;

use athena_federation_sdk::data::BlockSpiller;
use athena_federation_sdk::handlers::{HandlerContext, RecordHandler};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::{QueryStatusChecker, Result};

use crate::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use crate::manager::{
    ConnectionSource, JdbcRecordHandler, JdbcUtil, RecordDelegate, RecordDialect, SplitQuery,
};
use crate::multiplexing_jdbc_metadata_handler::{
    catalog_not_registered, close_all, validate_catalog_count, validate_handler_map,
};
use crate::sql::{JdbcConnection, PreparedStatement};
use crate::validate::null_pointer;

pub type MultiplexingJdbcRecordHandler<H> = JdbcRecordHandler<RecordMultiplexer<H>>;

pub struct RecordMultiplexer<H> {
    delegates: IndexMap<String, Arc<H>>,
}

impl<H: RecordHandler + RecordDelegate + 'static> RecordMultiplexer<H> {
    pub fn handler(
        context: HandlerContext,
        connection_config: DatabaseConnectionConfig,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
        delegates: IndexMap<String, Arc<H>>,
    ) -> Result<MultiplexingJdbcRecordHandler<H>> {
        validate_handler_map(&delegates, "recordHandlerMap must not be empty")?;
        validate_catalog_count(&delegates)?;
        Ok(JdbcRecordHandler::new(
            context,
            ConnectionSource::Direct {
                config: connection_config,
                factory: connection_factory,
            },
            RecordMultiplexer { delegates },
        ))
    }

    pub fn handler_from_config(
        context: HandlerContext,
        engine: &str,
        config_options: &HashMap<String, String>,
        make_delegate: impl Fn(&DatabaseConnectionConfig) -> Result<Arc<H>>,
    ) -> Result<MultiplexingJdbcRecordHandler<H>> {
        let delegates = JdbcUtil::handler_map(engine, config_options, make_delegate)?;
        validate_handler_map(&delegates, "Could not find any delegatee.")?;
        Ok(JdbcRecordHandler::new(
            context,
            ConnectionSource::Delegating,
            RecordMultiplexer { delegates },
        ))
    }

    pub fn delegates(&self) -> &IndexMap<String, Arc<H>> {
        &self.delegates
    }
}

#[async_trait]
impl<H: RecordHandler + RecordDelegate + 'static> RecordDialect for RecordMultiplexer<H> {
    async fn build_split_sql(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        let delegate = self.delegates.get(query.catalog_name).ok_or_else(|| {
            null_pointer("Cannot invoke \"com.amazonaws.athena.connectors.jdbc.manager.JdbcRecordHandler.buildSplitSql(java.sql.Connection, String, com.amazonaws.athena.connector.lambda.domain.TableName, org.apache.arrow.vector.types.pojo.Schema, com.amazonaws.athena.connector.lambda.domain.predicate.Constraints, com.amazonaws.athena.connector.lambda.domain.Split)\" because the return value of \"java.util.Map.get(Object)\" is null")
        })?;
        RecordDelegate::build_split_sql(delegate.as_ref(), connection, query).await
    }

    async fn read_with_constraint(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.delegates
            .get(request.catalog_name())
            .ok_or_else(|| catalog_not_registered(request.catalog_name()))?
            .read_with_constraint(spiller, request, query_status_checker)
            .await
    }

    async fn close(&self) -> Result<()> {
        let closes: Vec<_> = self
            .delegates
            .iter()
            .map(|(c, d)| (c, RecordDelegate::close(d.as_ref())))
            .collect();
        close_all("record", closes).await.map_or(Ok(()), Err)
    }
}
