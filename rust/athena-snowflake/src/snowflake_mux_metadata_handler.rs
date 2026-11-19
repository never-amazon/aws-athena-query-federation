use athena_jdbc::handler::MultiplexingJdbcMetadataHandler;

use crate::snowflake_metadata_handler::SnowflakeMetadataHandler;

pub type SnowflakeMuxMetadataHandler = MultiplexingJdbcMetadataHandler<SnowflakeMetadataHandler>;
