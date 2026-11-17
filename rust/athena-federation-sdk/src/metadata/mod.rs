mod get_data_source_capabilities_request;
mod get_data_source_capabilities_response;
mod get_splits_request;
mod get_splits_response;
mod get_table_layout_request;
mod get_table_layout_response;
mod get_table_request;
mod get_table_response;
pub mod glue;
mod list_schemas_request;
mod list_schemas_response;
mod list_tables_request;
mod list_tables_response;
mod metadata_request;
mod metadata_request_type;
mod metadata_response;
pub mod optimizations;

pub use get_data_source_capabilities_request::GetDataSourceCapabilitiesRequest;
pub use get_data_source_capabilities_response::GetDataSourceCapabilitiesResponse;
pub use get_splits_request::GetSplitsRequest;
pub use get_splits_response::GetSplitsResponse;
pub use get_table_layout_request::GetTableLayoutRequest;
pub use get_table_layout_response::GetTableLayoutResponse;
pub use get_table_request::GetTableRequest;
pub use get_table_response::GetTableResponse;
pub use list_schemas_request::ListSchemasRequest;
pub use list_schemas_response::ListSchemasResponse;
pub use list_tables_request::{ListTablesRequest, UNLIMITED_PAGE_SIZE_VALUE};
pub use list_tables_response::ListTablesResponse;
pub use metadata_request::MetadataRequest;
pub use metadata_request_type::MetadataRequestType;
pub use metadata_response::MetadataResponse;

pub(crate) fn collections_equals<T: PartialEq>(lhs: &[T], rhs: &[T]) -> bool {
    lhs.len() == rhs.len() && rhs.iter().all(|r| lhs.contains(r))
}

pub(crate) fn opt_str(value: Option<&str>) -> &str {
    value.unwrap_or("null")
}
