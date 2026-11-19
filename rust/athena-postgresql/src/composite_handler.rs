use std::sync::Arc;

use athena_federation_sdk::handlers::{AwsClients, CompositeHandler};
use athena_federation_sdk::Result;
use athena_jdbc::JdbcEnvironmentProperties;

use crate::environment_properties::PostGreSqlEnvironmentProperties;
use crate::metadata_handler::PostGreSqlMetadataDialect;
use crate::record_handler::PostGreSqlRecordDialect;

pub async fn composite_handler_from_environment() -> Result<CompositeHandler> {
    let environment_properties = PostGreSqlEnvironmentProperties::new();
    let metadata_environment = environment_properties.create_environment().await?;
    let record_environment = environment_properties.create_environment().await?;
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
