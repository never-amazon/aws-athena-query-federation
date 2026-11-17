use std::collections::HashMap;
use std::sync::Arc;

use athena_federation_sdk::handlers::{CompositeHandler, MetadataHandler, RecordHandler};
use athena_federation_sdk::Result;

use crate::connection::CONNECTION_STRING_PROPERTY_SUFFIX;

pub struct MultiplexingJdbcCompositeHandler {
    composite_handler: CompositeHandler,
}

impl MultiplexingJdbcCompositeHandler {
    pub fn has_catalog_connections(environment: &HashMap<String, String>) -> bool {
        environment
            .keys()
            .any(|k| k.ends_with(CONNECTION_STRING_PROPERTY_SUFFIX))
    }

    pub async fn new<MM, MR, M, R>(
        mux_metadata_handler: MM,
        mux_record_handler: MR,
        metadata_handler: M,
        record_handler: R,
    ) -> Result<MultiplexingJdbcCompositeHandler>
    where
        MM: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn MetadataHandler>>,
        MR: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn RecordHandler>>,
        M: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn MetadataHandler>>,
        R: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn RecordHandler>>,
    {
        MultiplexingJdbcCompositeHandler::new_with_environment(
            std::env::vars().collect(),
            mux_metadata_handler,
            mux_record_handler,
            metadata_handler,
            record_handler,
        )
        .await
    }

    pub async fn new_with_environment<MM, MR, M, R>(
        environment: HashMap<String, String>,
        mux_metadata_handler: MM,
        mux_record_handler: MR,
        metadata_handler: M,
        record_handler: R,
    ) -> Result<MultiplexingJdbcCompositeHandler>
    where
        MM: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn MetadataHandler>>,
        MR: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn RecordHandler>>,
        M: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn MetadataHandler>>,
        R: AsyncFnOnce(HashMap<String, String>) -> Result<Arc<dyn RecordHandler>>,
    {
        let (metadata, record) =
            if MultiplexingJdbcCompositeHandler::has_catalog_connections(&environment) {
                (
                    mux_metadata_handler(environment.clone()).await?,
                    mux_record_handler(environment).await?,
                )
            } else {
                (
                    metadata_handler(environment.clone()).await?,
                    record_handler(environment).await?,
                )
            };
        Ok(MultiplexingJdbcCompositeHandler {
            composite_handler: CompositeHandler::new(metadata, record),
        })
    }

    pub fn composite_handler(&self) -> &CompositeHandler {
        &self.composite_handler
    }

    pub fn into_composite_handler(self) -> CompositeHandler {
        self.composite_handler
    }
}
