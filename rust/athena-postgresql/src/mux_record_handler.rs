use athena_jdbc::handler::MultiplexingJdbcRecordHandler;

use crate::record_handler::PostGreSqlRecordHandler;

pub type PostGreSqlMuxRecordHandler = MultiplexingJdbcRecordHandler<PostGreSqlRecordHandler>;
