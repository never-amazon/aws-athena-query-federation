use athena_jdbc::MultiplexingJdbcRecordHandler;

use crate::record_handler::PostGreSqlRecordHandler;

pub type PostGreSqlMuxRecordHandler = MultiplexingJdbcRecordHandler<PostGreSqlRecordHandler>;
