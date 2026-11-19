use std::sync::Arc;

use athena_federation_sdk::handlers::{
    AwsClients, CompositeHandler, HandlerContext, MetadataContext, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::Result;
use athena_jdbc::{
    multiplexing_composite_handler, MultiplexingJdbcMetadataHandler, RecordMultiplexer,
};

use crate::constants::PostGreSqlConstants;
use crate::metadata_handler::PostGreSqlMetadataDialect;
use crate::record_handler::PostGreSqlRecordDialect;

pub async fn mux_composite_handler_from_environment() -> Result<CompositeHandler> {
    let clients = AwsClients::from_environment().await?;
    let record_clients = clients.clone();
    multiplexing_composite_handler(
        std::env::vars().collect(),
        async move |multiplexed, config_options| {
            let handler: Arc<dyn MetadataHandler> = if multiplexed {
                let context =
                    MetadataContext::builder(PostGreSqlConstants::POSTGRES_NAME, clients.clone())
                        .config_options(config_options.clone())
                        .build()?;
                Arc::new(MultiplexingJdbcMetadataHandler::from_config(
                    context,
                    PostGreSqlConstants::POSTGRES_NAME,
                    &config_options,
                    |config| {
                        Ok(Arc::new(PostGreSqlMetadataDialect::from_config(
                            clients.clone(),
                            config.clone(),
                            config_options.clone(),
                        )?))
                    },
                )?)
            } else {
                Arc::new(PostGreSqlMetadataDialect::from_config_options(
                    clients,
                    config_options,
                )?)
            };
            Ok(handler)
        },
        async move |multiplexed, config_options| {
            let handler: Arc<dyn RecordHandler> = if multiplexed {
                let context = HandlerContext::new(
                    PostGreSqlConstants::POSTGRES_NAME,
                    record_clients.clone(),
                    config_options.clone(),
                )?;
                Arc::new(RecordMultiplexer::handler_from_config(
                    context,
                    PostGreSqlConstants::POSTGRES_NAME,
                    &config_options,
                    |config| {
                        Ok(Arc::new(PostGreSqlRecordDialect::from_config(
                            record_clients.clone(),
                            config.clone(),
                            config_options.clone(),
                        )?))
                    },
                )?)
            } else {
                Arc::new(PostGreSqlRecordDialect::from_config_options(
                    record_clients,
                    config_options,
                )?)
            };
            Ok(handler)
        },
    )
    .await
}
