pub(crate) mod paginated_request_iterator;
pub(crate) mod pagination_helper;

pub use crate::metadata::UNLIMITED_PAGE_SIZE_VALUE;
pub use paginated_request_iterator::PaginatedRequestIterator;
pub use pagination_helper::{
    calculate_next_token, manual_pagination, validate_and_parse_pagination_arguments,
    ListTablesPage,
};
