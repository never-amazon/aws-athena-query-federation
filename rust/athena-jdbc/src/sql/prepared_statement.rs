use std::any::Any;
use std::fmt;
use std::sync::Arc;

use bigdecimal::BigDecimal;
use chrono::{NaiveDate, NaiveDateTime};

use super::result_set_metadata::ResultSetMetaData;
use super::sql_exception::SqlException;
use super::sql_value::SqlValue;

#[derive(Clone)]
pub struct StatementHandle(pub Arc<dyn Any + Send + Sync>);

impl StatementHandle {
    pub fn new<T: Any + Send + Sync>(handle: T) -> StatementHandle {
        StatementHandle(Arc::new(handle))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl fmt::Debug for StatementHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StatementHandle")
    }
}

impl PartialEq for StatementHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PreparedStatement {
    sql: String,
    parameters: Vec<SqlValue>,
    fetch_size: i32,
    query_timeout: i32,
    parameter_count: Option<usize>,
    metadata: Option<Arc<ResultSetMetaData>>,
    handle: Option<StatementHandle>,
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

    #[must_use]
    pub fn with_handle(mut self, handle: StatementHandle) -> PreparedStatement {
        self.handle = Some(handle);
        self
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn parameters(&self) -> &[SqlValue] {
        &self.parameters
    }

    pub fn parameter(&self, index: usize) -> Option<&SqlValue> {
        index.checked_sub(1).and_then(|i| self.parameters.get(i))
    }

    pub fn parameter_count(&self) -> Option<usize> {
        self.parameter_count
    }

    pub fn metadata(&self) -> Option<&Arc<ResultSetMetaData>> {
        self.metadata.as_ref()
    }

    pub fn handle(&self) -> Option<&StatementHandle> {
        self.handle.as_ref()
    }

    pub fn fetch_size(&self) -> i32 {
        self.fetch_size
    }

    pub fn set_fetch_size(&mut self, rows: i32) -> Result<(), SqlException> {
        if rows < 0 {
            return Err(SqlException::new(
                "Fetch size must be a value greater to or equal to 0.",
            ));
        }
        self.fetch_size = rows;
        Ok(())
    }

    pub fn query_timeout(&self) -> i32 {
        self.query_timeout
    }

    pub fn set_query_timeout(&mut self, seconds: i32) -> Result<(), SqlException> {
        if seconds < 0 {
            return Err(SqlException::new(
                "Query timeout must be a value greater than or equals to 0.",
            ));
        }
        self.query_timeout = seconds;
        Ok(())
    }

    pub fn set_object(&mut self, index: usize, value: SqlValue) -> Result<(), SqlException> {
        let position = index.checked_sub(1).ok_or_else(|| {
            SqlException::new(format!("The column index is out of range: {index}"))
        })?;
        if self.parameters.len() <= position {
            self.parameters.resize(position + 1, SqlValue::Null);
        }
        self.parameters[position] = value;
        Ok(())
    }

    pub fn set_null(&mut self, index: usize) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Null)
    }

    pub fn set_boolean(&mut self, index: usize, value: bool) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Boolean(value))
    }

    pub fn set_byte(&mut self, index: usize, value: i8) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::TinyInt(value))
    }

    pub fn set_short(&mut self, index: usize, value: i16) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::SmallInt(value))
    }

    pub fn set_int(&mut self, index: usize, value: i32) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Integer(value))
    }

    pub fn set_long(&mut self, index: usize, value: i64) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::BigInt(value))
    }

    pub fn set_float(&mut self, index: usize, value: f32) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Real(value))
    }

    pub fn set_double(&mut self, index: usize, value: f64) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Double(value))
    }

    pub fn set_big_decimal(&mut self, index: usize, value: BigDecimal) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Decimal(value))
    }

    pub fn set_string(
        &mut self,
        index: usize,
        value: impl Into<String>,
    ) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::String(value.into()))
    }

    pub fn set_bytes(&mut self, index: usize, value: Vec<u8>) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Bytes(value))
    }

    pub fn set_date(&mut self, index: usize, value: NaiveDate) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Date(value))
    }

    pub fn set_timestamp(
        &mut self,
        index: usize,
        value: NaiveDateTime,
    ) -> Result<(), SqlException> {
        self.set_object(index, SqlValue::Timestamp(value))
    }
}
