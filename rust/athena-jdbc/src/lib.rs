pub mod connection;
pub mod java_null;
pub mod manager;
pub mod qpt;
pub mod resolver;
pub mod splits;
pub mod sql;

mod jdbc_environment_properties;
mod multiplexing_jdbc_composite_handler;
mod multiplexing_jdbc_metadata_handler;
mod multiplexing_jdbc_record_handler;
mod validate;

pub use jdbc_environment_properties::ConnectionStringFormat;
pub use multiplexing_jdbc_composite_handler::{
    has_catalog_connections, multiplexing_composite_handler,
};
pub use multiplexing_jdbc_metadata_handler::{
    MultiplexingJdbcMetadataHandler, CATALOG_NOT_REGISTERED_ERROR_TEMPLATE,
};
pub use multiplexing_jdbc_record_handler::{MultiplexingJdbcRecordHandler, RecordMultiplexer};
pub use validate::{close_after, internal_service_error};
