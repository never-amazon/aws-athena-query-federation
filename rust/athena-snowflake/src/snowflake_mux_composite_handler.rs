use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{
    AwsClients, CompositeHandler, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::lambda::{LambdaContext, RequestStreamHandler};
use athena_federation_sdk::Result;
use athena_jdbc::MultiplexingJdbcCompositeHandler;

use crate::snowflake_metadata_handler::SnowflakeMetadataHandler;
use crate::snowflake_mux_metadata_handler::SnowflakeMuxMetadataHandler;
use crate::snowflake_mux_record_handler::SnowflakeMuxRecordHandler;
use crate::snowflake_record_handler::SnowflakeRecordHandler;

pub struct SnowflakeMuxCompositeHandler {
    composite_handler: CompositeHandler,
}

impl SnowflakeMuxCompositeHandler {
    pub async fn from_environment() -> Result<SnowflakeMuxCompositeHandler> {
        let clients = AwsClients::from_environment().await?;
        SnowflakeMuxCompositeHandler::new(clients, std::env::vars().collect()).await
    }

    pub async fn new(
        clients: AwsClients,
        environment: HashMap<String, String>,
    ) -> Result<SnowflakeMuxCompositeHandler> {
        let mux_metadata_clients = clients.clone();
        let mux_record_clients = clients.clone();
        let metadata_clients = clients.clone();
        let record_clients = clients;
        let handler = MultiplexingJdbcCompositeHandler::new_with_environment(
            environment,
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn MetadataHandler> = Arc::new(
                    SnowflakeMuxMetadataHandler::from_config_options(
                        mux_metadata_clients,
                        config_options,
                    )
                    .await?,
                );
                Ok(handler)
            },
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn RecordHandler> = Arc::new(
                    SnowflakeMuxRecordHandler::from_config_options(
                        mux_record_clients,
                        config_options,
                    )
                    .await?,
                );
                Ok(handler)
            },
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn MetadataHandler> = Arc::new(
                    SnowflakeMetadataHandler::from_config_options(metadata_clients, config_options)
                        .await?,
                );
                Ok(handler)
            },
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn RecordHandler> = Arc::new(
                    SnowflakeRecordHandler::from_config_options(record_clients, config_options)?,
                );
                Ok(handler)
            },
        )
        .await?;
        Ok(SnowflakeMuxCompositeHandler {
            composite_handler: handler.into_composite_handler(),
        })
    }

    pub fn composite_handler(&self) -> &CompositeHandler {
        &self.composite_handler
    }
}

#[async_trait]
impl RequestStreamHandler for SnowflakeMuxCompositeHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        self.composite_handler.handle_request(input, context).await
    }
}
