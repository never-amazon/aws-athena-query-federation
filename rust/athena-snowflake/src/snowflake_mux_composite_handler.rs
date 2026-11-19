use std::sync::Arc;

use athena_federation_sdk::handlers::{
    AwsClients, CompositeHandler, HandlerContext, MetadataContext, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::Result;
use athena_jdbc::handler::{
    multiplexing_composite_handler, MultiplexingJdbcMetadataHandler, RecordMultiplexer,
};

use crate::snowflake_constants;
use crate::snowflake_metadata_handler::SnowflakeMetadataDialect;
use crate::snowflake_record_handler::SnowflakeRecordDialect;

pub async fn mux_composite_handler_from_environment() -> Result<CompositeHandler> {
    let clients = AwsClients::from_environment().await?;
    let record_clients = clients.clone();
    multiplexing_composite_handler(
        std::env::vars().collect(),
        async move |multiplexed, config_options| {
            let handler: Arc<dyn MetadataHandler> = if multiplexed {
                let context =
                    MetadataContext::builder(snowflake_constants::SNOWFLAKE_NAME, clients.clone())
                        .config_options(config_options.clone())
                        .build()?;
                Arc::new(MultiplexingJdbcMetadataHandler::from_config(
                    context,
                    snowflake_constants::SNOWFLAKE_NAME,
                    &config_options,
                    |config| {
                        Ok(Arc::new(SnowflakeMetadataDialect::from_config(
                            clients.clone(),
                            config.clone(),
                            config_options.clone(),
                            None,
                        )?))
                    },
                )?)
            } else {
                Arc::new(
                    SnowflakeMetadataDialect::from_config_options(clients, config_options).await?,
                )
            };
            Ok(handler)
        },
        async move |multiplexed, config_options| {
            let handler: Arc<dyn RecordHandler> = if multiplexed {
                let context = HandlerContext::new(
                    snowflake_constants::SNOWFLAKE_NAME,
                    record_clients.clone(),
                    config_options.clone(),
                )?;
                Arc::new(RecordMultiplexer::handler_from_config(
                    context,
                    snowflake_constants::SNOWFLAKE_NAME,
                    &config_options,
                    |config| {
                        Ok(Arc::new(SnowflakeRecordDialect::from_config(
                            record_clients.clone(),
                            config.clone(),
                            config_options.clone(),
                        )?))
                    },
                )?)
            } else {
                Arc::new(SnowflakeRecordDialect::from_config_options(
                    record_clients,
                    config_options,
                )?)
            };
            Ok(handler)
        },
    )
    .await
}
