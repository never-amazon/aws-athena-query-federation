use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{AwsClients, CompositeHandler};
use athena_federation_sdk::lambda::{LambdaContext, RequestStreamHandler};
use athena_federation_sdk::Result;
use athena_jdbc::JdbcEnvironmentProperties;

use crate::snowflake_environment_properties::SnowflakeEnvironmentProperties;
use crate::snowflake_metadata_handler::SnowflakeMetadataHandler;
use crate::snowflake_record_handler::SnowflakeRecordHandler;

pub struct SnowflakeCompositeHandler {
    composite_handler: CompositeHandler,
}

impl SnowflakeCompositeHandler {
    pub async fn from_environment() -> Result<SnowflakeCompositeHandler> {
        let environment_properties = SnowflakeEnvironmentProperties::new();
        let metadata_environment = environment_properties.create_environment().await?;
        let record_environment = environment_properties.create_environment().await?;
        let clients = AwsClients::from_environment().await?;
        SnowflakeCompositeHandler::new(clients, metadata_environment, record_environment).await
    }

    pub async fn new(
        clients: AwsClients,
        metadata_config_options: HashMap<String, String>,
        record_config_options: HashMap<String, String>,
    ) -> Result<SnowflakeCompositeHandler> {
        let composite_handler = CompositeHandler::new(
            Arc::new(
                SnowflakeMetadataHandler::from_config_options(
                    clients.clone(),
                    metadata_config_options,
                )
                .await?,
            ),
            Arc::new(SnowflakeRecordHandler::from_config_options(
                clients,
                record_config_options,
            )?),
        );
        Ok(SnowflakeCompositeHandler { composite_handler })
    }

    pub fn composite_handler(&self) -> &CompositeHandler {
        &self.composite_handler
    }
}

#[async_trait]
impl RequestStreamHandler for SnowflakeCompositeHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        self.composite_handler.handle_request(input, context).await
    }
}
