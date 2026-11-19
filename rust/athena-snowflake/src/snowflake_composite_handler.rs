use std::sync::Arc;

use athena_federation_sdk::handlers::{AwsClients, CompositeHandler};
use athena_federation_sdk::Result;
use athena_jdbc::JdbcEnvironmentProperties;

use crate::snowflake_environment_properties::SnowflakeEnvironmentProperties;
use crate::snowflake_metadata_handler::SnowflakeMetadataDialect;
use crate::snowflake_record_handler::SnowflakeRecordDialect;

pub async fn composite_handler_from_environment() -> Result<CompositeHandler> {
    let environment_properties = SnowflakeEnvironmentProperties::new();
    let metadata_environment = environment_properties.create_environment().await?;
    let record_environment = environment_properties.create_environment().await?;
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
