use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{AwsClients, MetadataHandlerBase};
use athena_federation_sdk::Result;
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{
    JdbcMetadataHandler, JdbcMetadataHandlerBase, JdbcMetadataHandlerFactory,
};
use athena_jdbc::MultiplexingJdbcMetadataHandler;
use indexmap::IndexMap;

use crate::resolver::SnowflakeJdbcCaseResolver;
use crate::snowflake_constants::SnowflakeConstants;
use crate::snowflake_metadata_handler::SnowflakeMetadataHandler;

#[derive(Clone)]
pub struct SnowflakeMetadataHandlerFactory {
    clients: AwsClients,
}

impl SnowflakeMetadataHandlerFactory {
    pub fn new(clients: AwsClients) -> SnowflakeMetadataHandlerFactory {
        SnowflakeMetadataHandlerFactory { clients }
    }
}

#[async_trait]
impl JdbcMetadataHandlerFactory for SnowflakeMetadataHandlerFactory {
    fn engine(&self) -> &str {
        SnowflakeConstants::SNOWFLAKE_NAME
    }

    async fn create_jdbc_metadata_handler(
        &self,
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn JdbcMetadataHandler>> {
        Ok(Arc::new(SnowflakeMetadataHandler::new(
            self.clients.clone(),
            config.clone(),
            config_options.clone(),
        )?))
    }
}

pub struct SnowflakeMuxMetadataHandler;

impl SnowflakeMuxMetadataHandler {
    pub async fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<MultiplexingJdbcMetadataHandler> {
        let factory = SnowflakeMetadataHandlerFactory::new(clients.clone());
        let metadata_base =
            MetadataHandlerBase::builder(SnowflakeConstants::SNOWFLAKE_NAME, clients)
                .config_options(config_options.clone())
                .build()?;
        MultiplexingJdbcMetadataHandler::new_with_factory(metadata_base, &factory, &config_options)
            .await
    }

    pub fn with_handler_map(
        clients: AwsClients,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        metadata_handler_map: IndexMap<String, Arc<dyn JdbcMetadataHandler>>,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<MultiplexingJdbcMetadataHandler> {
        let metadata_base = MetadataHandlerBase::builder(config.engine(), clients)
            .config_options(config_options)
            .encryption_key_factory(None)
            .spill_bucket(None)
            .spill_prefix(None)
            .build()?;
        let base = JdbcMetadataHandlerBase::new_with_case_resolver(
            metadata_base,
            config,
            jdbc_connection_factory,
            Arc::new(SnowflakeJdbcCaseResolver::new(
                SnowflakeConstants::SNOWFLAKE_NAME,
            )),
        );
        MultiplexingJdbcMetadataHandler::new(base, metadata_handler_map)
    }
}
