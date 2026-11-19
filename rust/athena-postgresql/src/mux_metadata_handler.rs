use athena_jdbc::MultiplexingJdbcMetadataHandler;

use crate::metadata_handler::PostGreSqlMetadataHandler;

pub type PostGreSqlMuxMetadataHandler = MultiplexingJdbcMetadataHandler<PostGreSqlMetadataHandler>;
