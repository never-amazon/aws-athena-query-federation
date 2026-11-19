use athena_jdbc::handler::MultiplexingJdbcRecordHandler;

use crate::snowflake_record_handler::SnowflakeRecordHandler;

pub type SnowflakeMuxRecordHandler = MultiplexingJdbcRecordHandler<SnowflakeRecordHandler>;
