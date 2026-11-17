use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::handlers::{
    AwsClients, CompositeHandler, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::lambda::{LambdaContext, RequestStreamHandler};
use athena_federation_sdk::Result;
use athena_jdbc::MultiplexingJdbcCompositeHandler;

use crate::metadata_handler::PostGreSqlMetadataHandler;
use crate::mux_metadata_handler::PostGreSqlMuxMetadataHandler;
use crate::mux_record_handler::PostGreSqlMuxRecordHandler;
use crate::record_handler::PostGreSqlRecordHandler;

pub struct PostGreSqlMuxCompositeHandler {
    composite_handler: CompositeHandler,
}

impl PostGreSqlMuxCompositeHandler {
    pub async fn from_environment() -> Result<PostGreSqlMuxCompositeHandler> {
        let clients = AwsClients::from_environment().await?;
        PostGreSqlMuxCompositeHandler::new(clients, std::env::vars().collect()).await
    }

    pub async fn new(
        clients: AwsClients,
        environment: HashMap<String, String>,
    ) -> Result<PostGreSqlMuxCompositeHandler> {
        let mux_metadata_clients = clients.clone();
        let mux_record_clients = clients.clone();
        let metadata_clients = clients.clone();
        let record_clients = clients;
        let handler = MultiplexingJdbcCompositeHandler::new_with_environment(
            environment,
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn MetadataHandler> = Arc::new(
                    PostGreSqlMuxMetadataHandler::from_config_options(
                        mux_metadata_clients,
                        config_options,
                    )
                    .await?,
                );
                Ok(handler)
            },
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn RecordHandler> = Arc::new(
                    PostGreSqlMuxRecordHandler::from_config_options(
                        mux_record_clients,
                        config_options,
                    )
                    .await?,
                );
                Ok(handler)
            },
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn MetadataHandler> =
                    Arc::new(PostGreSqlMetadataHandler::from_config_options(
                        metadata_clients,
                        config_options,
                    )?);
                Ok(handler)
            },
            async move |config_options: HashMap<String, String>| {
                let handler: Arc<dyn RecordHandler> = Arc::new(
                    PostGreSqlRecordHandler::from_config_options(record_clients, config_options)?,
                );
                Ok(handler)
            },
        )
        .await?;
        Ok(PostGreSqlMuxCompositeHandler {
            composite_handler: handler.into_composite_handler(),
        })
    }

    pub fn composite_handler(&self) -> &CompositeHandler {
        &self.composite_handler
    }
}

#[async_trait]
impl RequestStreamHandler for PostGreSqlMuxCompositeHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        self.composite_handler.handle_request(input, context).await
    }
}
