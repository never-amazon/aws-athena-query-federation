use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{AwsClients, RecordHandlerBase};
use athena_federation_sdk::Result;
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{JdbcRecordHandler, JdbcRecordHandlerBase, JdbcRecordHandlerFactory};
use athena_jdbc::MultiplexingJdbcRecordHandler;
use indexmap::IndexMap;

use crate::constants::PostGreSqlConstants;
use crate::record_handler::PostGreSqlRecordHandler;

#[derive(Clone)]
pub struct PostGreSqlRecordHandlerFactory {
    clients: AwsClients,
}

impl PostGreSqlRecordHandlerFactory {
    pub fn new(clients: AwsClients) -> PostGreSqlRecordHandlerFactory {
        PostGreSqlRecordHandlerFactory { clients }
    }
}

#[async_trait]
impl JdbcRecordHandlerFactory for PostGreSqlRecordHandlerFactory {
    fn engine(&self) -> &str {
        PostGreSqlConstants::POSTGRES_NAME
    }

    async fn create_jdbc_record_handler(
        &self,
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn JdbcRecordHandler>> {
        Ok(Arc::new(PostGreSqlRecordHandler::new(
            self.clients.clone(),
            config.clone(),
            config_options.clone(),
        )?))
    }
}

pub struct PostGreSqlMuxRecordHandler;

impl PostGreSqlMuxRecordHandler {
    pub async fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<MultiplexingJdbcRecordHandler> {
        let factory = PostGreSqlRecordHandlerFactory::new(clients.clone());
        let record_base = RecordHandlerBase::new(
            PostGreSqlConstants::POSTGRES_NAME,
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
