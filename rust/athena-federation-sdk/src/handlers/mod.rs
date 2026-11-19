mod athena_exception_filter;
mod aws_clients;
mod composite_handler;
mod federation_capabilities;
mod glue_metadata_handler;
mod handler_context;
mod metadata_handler;
mod record_handler;
mod serve;
mod user_defined_function_handler;

pub use athena_exception_filter::is_athena_throttle;
pub use aws_clients::AwsClients;
pub use composite_handler::CompositeHandler;
pub use federation_capabilities::CAPABILITIES;
pub use glue_metadata_handler::{
    get_column_name_mapping, get_source_table_name, glue_client_from_config, is_glue_disabled,
    populate_source_table_name_if_available, GlueMetadataHandler, COLUMN_NAME_MAPPING_PROPERTY,
    DATETIME_FORMAT_MAPPING_PROPERTY, DATETIME_FORMAT_MAPPING_PROPERTY_NORMALIZED,
    GET_TABLES_REQUEST_MAX_RESULTS, GLUE_TABLE_CONTAINS_PREVIOUSLY_UNSUPPORTED_TYPE,
    SOURCE_TABLE_PROPERTY, VIEW_METADATA_FIELD,
};
pub use handler_context::{ConfigOptions, HandlerContext};
pub use metadata_handler::{
    table_layout, MetadataContext, MetadataContextBuilder, MetadataHandler, DEFAULT_SPILL_PREFIX,
    DISABLE_SPILL_ENCRYPTION, KMS_KEY_ID_ENV, PARTITION_ID_COL, SPILL_BUCKET_ENV, SPILL_PREFIX_ENV,
};
pub use record_handler::{read_records, RecordHandler};
pub use serve::{MetadataStreamHandler, RecordStreamHandler, UserDefinedFunctionStreamHandler};
pub use user_defined_function_handler::{
    process_function, JavaType, UdfFunction, UdfMethod, UdfMethods, UserDefinedFunctionHandler,
};
