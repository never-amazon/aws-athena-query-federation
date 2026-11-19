use std::sync::Arc;
use std::time::Duration;

use super::result_set_metadata::ResultSetMetaData;
use super::value::SqlValue;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PreparedStatement {
    sql: String,
    parameters: Vec<SqlValue>,
    fetch_size: u32,
    query_timeout: Option<Duration>,
    parameter_count: Option<usize>,
    metadata: Option<Arc<ResultSetMetaData>>,
}

impl PreparedStatement {
    pub fn new(sql: impl Into<String>) -> PreparedStatement {
        PreparedStatement {
            sql: sql.into(),
            ..PreparedStatement::default()
        }
    }

    #[must_use]
    pub fn with_parameter_count(mut self, parameter_count: usize) -> PreparedStatement {
        self.parameter_count = Some(parameter_count);
        self
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: ResultSetMetaData) -> PreparedStatement {
        self.metadata = Some(Arc::new(metadata));
        self
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn parameters(&self) -> &[SqlValue] {
        &self.parameters
    }

    pub fn parameter_count(&self) -> Option<usize> {
        self.parameter_count
    }

    pub fn metadata(&self) -> Option<&Arc<ResultSetMetaData>> {
        self.metadata.as_ref()
    }

    pub fn fetch_size(&self) -> u32 {
        self.fetch_size
    }

    pub fn set_fetch_size(&mut self, rows: u32) {
        self.fetch_size = rows;
    }

    pub fn query_timeout(&self) -> Option<Duration> {
        self.query_timeout
    }

    pub fn set_query_timeout(&mut self, timeout: Duration) {
        self.query_timeout = Some(timeout).filter(|t| !t.is_zero());
    }

    pub fn bind(&mut self, value: impl Into<SqlValue>) {
        self.parameters.push(value.into());
    }
}
