use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use athena_federation_sdk::Result;

use super::jdbc_record_handler::JdbcRecordHandler;
use crate::connection::DatabaseConnectionConfig;

#[async_trait]
pub trait JdbcRecordHandlerFactory: Send + Sync {
    fn engine(&self) -> &str;

    async fn create_jdbc_record_handler(
        &self,
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn JdbcRecordHandler>>;
}
