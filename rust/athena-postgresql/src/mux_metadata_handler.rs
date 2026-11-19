use athena_jdbc::handler::MultiplexingJdbcMetadataHandler;

use crate::metadata_handler::PostGreSqlMetadataHandler;

pub type PostGreSqlMuxMetadataHandler = MultiplexingJdbcMetadataHandler<PostGreSqlMetadataHandler>;
