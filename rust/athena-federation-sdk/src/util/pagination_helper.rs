use crate::domain::table_name::TableName;
use crate::exceptions::{FederationError, Result};
use crate::java_cast::len_to_long;

pub const UNLIMITED_PAGE_SIZE_VALUE: i32 = -1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListTablesPage {
    catalog_name: String,
    tables: Vec<TableName>,
    next_token: Option<String>,
}

impl ListTablesPage {
    pub fn new(
        catalog_name: impl Into<String>,
        tables: Vec<TableName>,
        next_token: Option<String>,
    ) -> ListTablesPage {
        ListTablesPage {
            catalog_name: catalog_name.into(),
            tables,
            next_token,
        }
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn tables(&self) -> &[TableName] {
        &self.tables
    }

    pub fn next_token(&self) -> Option<&str> {
        self.next_token.as_deref()
    }

    pub fn into_parts(self) -> (String, Vec<TableName>, Option<String>) {
        (self.catalog_name, self.tables, self.next_token)
    }
}

pub struct PaginationHelper;

impl PaginationHelper {
    pub fn validate_and_parse_pagination_arguments(
        next_token: Option<&str>,
        page_size: i32,
    ) -> Result<i32> {
        let start_token = match next_token {
            None => 0,
            Some(t) => t
                .parse::<i32>()
                .map_err(|_| FederationError::invalid_input(format!("Invalid next token: {t}")))?,
        };
        if start_token < 0 {
            return Err(FederationError::invalid_input(format!(
                "Invalid next token format. Token must be a valid integer, received: {start_token}"
            )));
        }
        if page_size < UNLIMITED_PAGE_SIZE_VALUE {
            return Err(FederationError::invalid_input(format!(
                "Page size must be either -1 for unlimited or a positive integer, received: {page_size}"
            )));
        }
        Ok(start_token)
    }

    pub fn manual_pagination(
        all_tables: &[TableName],
        token: Option<&str>,
        page_size: i32,
        catalog_name: &str,
    ) -> Result<ListTablesPage> {
        let start_token =
            PaginationHelper::validate_and_parse_pagination_arguments(token, page_size)?;
        let mut sorted = all_tables.to_vec();
        sorted.sort_by(|a, b| a.table_name().cmp(b.table_name()));
        let start = usize::try_from(start_token).unwrap_or(usize::MAX);
        if start >= all_tables.len() {
            return Ok(ListTablesPage::new(catalog_name, Vec::new(), None));
        }
        let mut end = usize::try_from((i64::from(start_token) + i64::from(page_size)).max(0))
            .unwrap_or(usize::MAX)
            .min(sorted.len());
        if page_size == UNLIMITED_PAGE_SIZE_VALUE {
            end = sorted.len();
        }
        let next_token = if page_size == UNLIMITED_PAGE_SIZE_VALUE || end == all_tables.len() {
            None
        } else {
            Some((i64::from(start_token) + i64::from(page_size)).to_string())
        };
        let page = if end > start {
            sorted[start..end].to_vec()
        } else {
            Vec::new()
        };
        Ok(ListTablesPage::new(catalog_name, page, next_token))
    }

    pub fn calculate_next_token(
        token: i32,
        page_size: i32,
        paginated_tables: &[TableName],
    ) -> Option<String> {
        if page_size == i32::MAX
            || paginated_tables.is_empty()
            || len_to_long(paginated_tables.len()) < i64::from(page_size)
        {
            None
        } else {
            Some(token.wrapping_add(page_size).to_string())
        }
    }
}
