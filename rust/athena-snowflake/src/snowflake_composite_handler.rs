use std::sync::Arc;

use athena_federation_sdk::connection::EnvironmentProperties;
use athena_federation_sdk::handlers::{AwsClients, CompositeHandler};
use athena_federation_sdk::Result;

use crate::snowflake_environment_properties::connection_properties_to_environment;
use crate::snowflake_metadata_handler::SnowflakeMetadataDialect;
use crate::snowflake_record_handler::SnowflakeRecordDialect;

pub async fn composite_handler_from_environment() -> Result<CompositeHandler> {
    let environment_properties = EnvironmentProperties::new();
    let metadata_environment = environment_properties
        .create_environment_with(|properties| connection_properties_to_environment(&properties))
        .await?;
    let record_environment = environment_properties
        .create_environment_with(|properties| connection_properties_to_environment(&properties))
        .await?;
    let clients = AwsClients::from_environment().await?;
    Ok(CompositeHandler::new(
        Arc::new(
            SnowflakeMetadataDialect::from_config_options(clients.clone(), metadata_environment)
                .await?,
        ),
        Arc::new(SnowflakeRecordDialect::from_config_options(
            clients,
            record_environment,
        )?),
        None,
    ))
}
