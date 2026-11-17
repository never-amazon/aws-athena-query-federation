use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{AwsClients, RecordHandlerBase};
use athena_federation_sdk::Result;
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{JdbcRecordHandler, JdbcRecordHandlerBase, JdbcRecordHandlerFactory};
use athena_jdbc::MultiplexingJdbcRecordHandler;
use indexmap::IndexMap;

use crate::snowflake_constants::SnowflakeConstants;
use crate::snowflake_record_handler::SnowflakeRecordHandler;

#[derive(Clone)]
pub struct SnowflakeMuxRecordHandlerFactory {
    clients: AwsClients,
}

impl SnowflakeMuxRecordHandlerFactory {
    pub fn new(clients: AwsClients) -> SnowflakeMuxRecordHandlerFactory {
        SnowflakeMuxRecordHandlerFactory { clients }
    }
}

#[async_trait]
impl JdbcRecordHandlerFactory for SnowflakeMuxRecordHandlerFactory {
    fn engine(&self) -> &str {
        SnowflakeConstants::SNOWFLAKE_NAME
    }

    async fn create_jdbc_record_handler(
        &self,
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn JdbcRecordHandler>> {
        Ok(Arc::new(SnowflakeRecordHandler::new(
            self.clients.clone(),
            config.clone(),
            config_options.clone(),
        )?))
    }
}

pub struct SnowflakeMuxRecordHandler;

impl SnowflakeMuxRecordHandler {
    pub async fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<MultiplexingJdbcRecordHandler> {
        let factory = SnowflakeMuxRecordHandlerFactory::new(clients.clone());
        let record_base = RecordHandlerBase::new(
            SnowflakeConstants::SNOWFLAKE_NAME,
            clients,
            config_options.clone(),
        )?;
        MultiplexingJdbcRecordHandler::new_with_factory(record_base, &factory, &config_options)
            .await
    }

    pub fn with_handler_map(
        clients: AwsClients,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        config: DatabaseConnectionConfig,
        record_handler_map: IndexMap<String, Arc<dyn JdbcRecordHandler>>,
        config_options: HashMap<String, String>,
    ) -> Result<MultiplexingJdbcRecordHandler> {
        let record_base = RecordHandlerBase::new(config.engine(), clients, config_options)?;
        MultiplexingJdbcRecordHandler::new(
            JdbcRecordHandlerBase::new(record_base, config, jdbc_connection_factory),
            record_handler_map,
        )
    }
}
