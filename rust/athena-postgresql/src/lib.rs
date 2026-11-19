mod composite_handler;
pub mod constants;
mod driver;
mod environment_properties;
mod metadata_handler;
mod mux_composite_handler;
mod mux_metadata_handler;
mod mux_record_handler;
mod query_string_builder;
mod record_handler;
mod resolver;
mod sql_dialect;

pub use composite_handler::composite_handler_from_environment;
pub use driver::{PgConnection, PgDatabaseMetaData, PgDriver, POSTGRESQL_DRIVER_CLASS};
pub use environment_properties::{
    connection_properties_to_environment, POSTGRESQL_CONNECTION_STRING,
};
pub use metadata_handler::{
    escape_identifier, get_char_columns, get_split_clauses, is_valid_split_column, jdbc_properties,
    wrap_name_with_escaped_character, PostGreSqlMetadataDialect, PostGreSqlMetadataHandler,
    ALL_PARTITIONS, BLOCK_PARTITION_COLUMN_NAME, BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
    DEFAULT_NUM_SPLITS, GET_PARTITIONS_QUERY, JDBC_PROPERTIES, LIST_PAGINATED_TABLES_QUERY,
    NON_DEFAULT_COLLATE, SQL_SPLITS_STRING,
};
pub use mux_composite_handler::mux_composite_handler_from_environment;
pub use mux_metadata_handler::PostGreSqlMuxMetadataHandler;
pub use mux_record_handler::PostGreSqlMuxRecordHandler;
pub use query_string_builder::{
    build_sql, query_string_builder, PostGreSqlQueryStringBuilder, PostGreSqlSplitDialect,
};
pub use record_handler::{PostGreSqlRecordDialect, PostGreSqlRecordHandler};
pub use resolver::PostGreSqlJdbcCaseResolver;
pub use sql_dialect::PostgresqlSqlDialect;
