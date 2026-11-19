mod composite_handler;
pub mod constants;
mod driver;
mod metadata_handler;
mod mux_handler;
mod query_string_builder;
mod record_handler;
mod resolver;
mod sql_dialect;

pub use composite_handler::{
    composite_handler_from_environment, connection_properties_to_environment,
    POSTGRESQL_CONNECTION_STRING,
};
pub use driver::{PgConnection, PgDatabaseMetaData, PgDriver, POSTGRESQL_DRIVER_CLASS};
pub use metadata_handler::{
    char_columns, escape_identifier, is_valid_split_column, split_clauses,
    wrap_name_with_escaped_character, PostGreSqlMetadataDialect, PostGreSqlMetadataHandler,
    ALL_PARTITIONS, BLOCK_PARTITION_COLUMN_NAME, BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
    DEFAULT_NUM_SPLITS, GET_PARTITIONS_QUERY, LIST_PAGINATED_TABLES_QUERY, NON_DEFAULT_COLLATE,
};
pub use mux_handler::{
    mux_composite_handler_from_environment, PostGreSqlMuxMetadataHandler,
    PostGreSqlMuxRecordHandler,
};
pub use query_string_builder::{
    build_sql, query_string_builder, PostGreSqlQueryStringBuilder, PostGreSqlSplitDialect,
};
pub use record_handler::{PostGreSqlRecordDialect, PostGreSqlRecordHandler};
pub use resolver::{postgresql_case_resolver, POSTGRESQL_CASE_LOOKUP};
pub use sql_dialect::PostgresqlSqlDialect;
