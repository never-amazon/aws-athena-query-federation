use std::sync::Arc;

use athena_federation_sdk::connection::EnvironmentProperties;
use athena_federation_sdk::handlers::{AwsClients, CompositeHandler};
use athena_federation_sdk::Result;

use crate::environment_properties::connection_properties_to_environment;
use crate::metadata_handler::PostGreSqlMetadataDialect;
use crate::record_handler::PostGreSqlRecordDialect;

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
        Arc::new(PostGreSqlMetadataDialect::from_config_options(
            clients.clone(),
            metadata_environment,
        )?),
        Arc::new(PostGreSqlRecordDialect::from_config_options(
            clients,
            record_environment,
        )?),
        None,
    ))
}
