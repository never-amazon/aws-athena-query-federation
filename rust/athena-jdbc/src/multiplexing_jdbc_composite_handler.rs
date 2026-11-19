use std::collections::HashMap;
use std::sync::Arc;

use athena_federation_sdk::handlers::{CompositeHandler, MetadataHandler, RecordHandler};
use athena_federation_sdk::Result;

use crate::connection::CONNECTION_STRING_PROPERTY_SUFFIX;

pub fn has_catalog_connections<S>(environment: &HashMap<String, String, S>) -> bool {
    environment
        .keys()
        .any(|k| k.ends_with(CONNECTION_STRING_PROPERTY_SUFFIX))
}

pub async fn multiplexing_composite_handler<S, M, R>(
    environment: HashMap<String, String, S>,
    metadata_handler: M,
    record_handler: R,
) -> Result<CompositeHandler>
where
    S: Clone,
    M: AsyncFnOnce(bool, HashMap<String, String, S>) -> Result<Arc<dyn MetadataHandler>>,
    R: AsyncFnOnce(bool, HashMap<String, String, S>) -> Result<Arc<dyn RecordHandler>>,
{
    let multiplexed = has_catalog_connections(&environment);
    let metadata = metadata_handler(multiplexed, environment.clone()).await?;
    let record = record_handler(multiplexed, environment).await?;
    Ok(CompositeHandler::new(metadata, record, None))
}
