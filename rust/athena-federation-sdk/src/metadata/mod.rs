mod get_data_source_capabilities;
mod get_splits;
mod get_table;
mod get_table_layout;
pub mod glue;
mod header;
mod list_schemas;
mod list_tables;
mod optimizations;
mod query_passthrough;

pub use get_data_source_capabilities::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse,
};
pub use get_splits::{GetSplitsRequest, GetSplitsResponse};
pub use get_table::{GetTableRequest, GetTableResponse};
pub use get_table_layout::{GetTableLayoutRequest, GetTableLayoutResponse};
pub(crate) use header::{impl_metadata_request, impl_metadata_response};
pub use header::{MetadataHeader, MetadataRequestType};
pub use list_schemas::{ListSchemasRequest, ListSchemasResponse};
pub use list_tables::{ListTablesRequest, ListTablesResponse, UNLIMITED_PAGE_SIZE_VALUE};
pub use optimizations::{
    Capability, ComplexExpressionPushdownSubType, DataSourceOptimizations, FilterPushdownSubType,
    HintsSubtype, LimitPushdownSubType, OptimizationSubType, SubTypeProperties,
    TopNPushdownSubType,
};
pub use query_passthrough::{
    QueryPassthrough, QueryPassthroughSignature, DEFAULT_ENABLE_QUERY_PASSTHROUGH_STATE,
    ENABLE_QUERY_PASSTHROUGH, SCHEMA_FUNCTION_NAME,
};

pub(crate) fn collections_equals<T: PartialEq>(lhs: &[T], rhs: &[T]) -> bool {
    lhs.len() == rhs.len() && rhs.iter().all(|r| lhs.contains(r))
}

pub(crate) fn opt_str(value: Option<&str>) -> &str {
    value.unwrap_or("null")
}
