use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{AwsClients, CompositeHandler};
use athena_federation_sdk::lambda::{LambdaContext, RequestStreamHandler};
use athena_federation_sdk::Result;
use athena_jdbc::JdbcEnvironmentProperties;

use crate::environment_properties::PostGreSqlEnvironmentProperties;
use crate::metadata_handler::PostGreSqlMetadataHandler;
use crate::record_handler::PostGreSqlRecordHandler;

pub struct PostGreSqlCompositeHandler {
    composite_handler: CompositeHandler,
}

impl PostGreSqlCompositeHandler {
    pub async fn from_environment() -> Result<PostGreSqlCompositeHandler> {
        let environment_properties = PostGreSqlEnvironmentProperties::new();
        let metadata_environment = environment_properties.create_environment().await?;
        let record_environment = environment_properties.create_environment().await?;
        let clients = AwsClients::from_environment().await?;
        PostGreSqlCompositeHandler::new(clients, metadata_environment, record_environment)
    }

    pub fn new(
        clients: AwsClients,
        metadata_config_options: HashMap<String, String>,
        record_config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlCompositeHandler> {
        let composite_handler = CompositeHandler::new(
            Arc::new(PostGreSqlMetadataHandler::from_config_options(
                clients.clone(),
                metadata_config_options,
            )?),
            Arc::new(PostGreSqlRecordHandler::from_config_options(
                clients,
                record_config_options,
            )?),
        );
        Ok(PostGreSqlCompositeHandler { composite_handler })
    }

    pub fn composite_handler(&self) -> &CompositeHandler {
        &self.composite_handler
    }
}

#[async_trait]
impl RequestStreamHandler for PostGreSqlCompositeHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        self.composite_handler.handle_request(input, context).await
    }
}
