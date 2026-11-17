use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use bigdecimal::BigDecimal;
use chrono::{NaiveDate, NaiveDateTime};

use super::result_set_metadata::ResultSetMetaData;
use super::sql_exception::SqlException;
use super::sql_value::SqlValue;

pub trait ColumnIndex {
    fn column_position(self, metadata: &ResultSetMetaData) -> Result<usize, SqlException>;
}

impl ColumnIndex for usize {
    fn column_position(self, metadata: &ResultSetMetaData) -> Result<usize, SqlException> {
        metadata.column(self).map(|_| self)
    }
}

impl ColumnIndex for &str {
    fn column_position(self, metadata: &ResultSetMetaData) -> Result<usize, SqlException> {
        metadata.find_column(self)
    }
}

impl ColumnIndex for &String {
    fn column_position(self, metadata: &ResultSetMetaData) -> Result<usize, SqlException> {
        metadata.find_column(self)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    metadata: Arc<ResultSetMetaData>,
    values: Vec<SqlValue>,
}

impl Row {
    pub fn new(metadata: Arc<ResultSetMetaData>, values: Vec<SqlValue>) -> Row {
        Row { metadata, values }
    }

    pub fn metadata(&self) -> &Arc<ResultSetMetaData> {
        &self.metadata
    }

    pub fn values(&self) -> &[SqlValue] {
        &self.values
    }

    pub fn get_object<I: ColumnIndex>(&self, column: I) -> Result<&SqlValue, SqlException> {
        let position = column.column_position(&self.metadata)?;
        Ok(self.values.get(position - 1).unwrap_or(&SqlValue::Null))
    }

    pub fn get_string<I: ColumnIndex>(&self, column: I) -> Result<Option<String>, SqlException> {
        Ok(self.get_object(column)?.to_java_string())
    }

    pub fn get_boolean<I: ColumnIndex>(&self, column: I) -> Result<Option<bool>, SqlException> {
        self.get_object(column)?.to_boolean()
    }

    pub fn get_byte<I: ColumnIndex>(&self, column: I) -> Result<Option<i8>, SqlException> {
        self.get_object(column)?.to_byte()
    }

    pub fn get_short<I: ColumnIndex>(&self, column: I) -> Result<Option<i16>, SqlException> {
        self.get_object(column)?.to_short()
    }

    pub fn get_int<I: ColumnIndex>(&self, column: I) -> Result<Option<i32>, SqlException> {
        self.get_object(column)?.to_int()
    }

    pub fn get_long<I: ColumnIndex>(&self, column: I) -> Result<Option<i64>, SqlException> {
        self.get_object(column)?.to_long()
    }

    pub fn get_float<I: ColumnIndex>(&self, column: I) -> Result<Option<f32>, SqlException> {
        self.get_object(column)?.to_float()
    }

    pub fn get_double<I: ColumnIndex>(&self, column: I) -> Result<Option<f64>, SqlException> {
        self.get_object(column)?.to_double()
    }

    pub fn get_big_decimal<I: ColumnIndex>(
        &self,
        column: I,
    ) -> Result<Option<BigDecimal>, SqlException> {
        self.get_object(column)?.to_big_decimal()
    }

    pub fn get_bytes<I: ColumnIndex>(&self, column: I) -> Result<Option<Vec<u8>>, SqlException> {
        self.get_object(column)?.to_bytes()
    }

    pub fn get_date<I: ColumnIndex>(&self, column: I) -> Result<Option<NaiveDate>, SqlException> {
        self.get_object(column)?.to_date()
    }

    pub fn get_timestamp<I: ColumnIndex>(
        &self,
        column: I,
    ) -> Result<Option<NaiveDateTime>, SqlException> {
        self.get_object(column)?.to_timestamp()
    }

    pub fn get_array<I: ColumnIndex>(
        &self,
        column: I,
    ) -> Result<Option<Vec<SqlValue>>, SqlException> {
        self.get_object(column)?.to_array()
    }
}

#[async_trait]
pub trait RowSource: Send {
    async fn next_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlException>;
}

#[derive(Debug, Default)]
pub struct VecRowSource {
    rows: VecDeque<Vec<SqlValue>>,
}

impl VecRowSource {
    pub fn new(rows: Vec<Vec<SqlValue>>) -> VecRowSource {
        VecRowSource { rows: rows.into() }
    }
}

#[async_trait]
impl RowSource for VecRowSource {
    async fn next_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlException> {
        Ok(self.rows.pop_front())
    }
}

pub struct ResultSet {
    metadata: Arc<ResultSetMetaData>,
    source: Box<dyn RowSource>,
    current: Option<Row>,
}

impl fmt::Debug for ResultSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResultSet")
            .field("metadata", &self.metadata)
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl ResultSet {
    pub fn new(metadata: Arc<ResultSetMetaData>, source: Box<dyn RowSource>) -> ResultSet {
        ResultSet {
            metadata,
            source,
            current: None,
        }
    }

    pub fn from_rows(metadata: ResultSetMetaData, rows: Vec<Vec<SqlValue>>) -> ResultSet {
        ResultSet::new(Arc::new(metadata), Box::new(VecRowSource::new(rows)))
    }

    pub fn empty(metadata: ResultSetMetaData) -> ResultSet {
        ResultSet::from_rows(metadata, Vec::new())
    }

    pub fn metadata(&self) -> &Arc<ResultSetMetaData> {
        &self.metadata
    }

    pub async fn next(&mut self) -> Result<bool, SqlException> {
        self.current = self
            .source
            .next_row()
            .await?
            .map(|values| Row::new(Arc::clone(&self.metadata), values));
        Ok(self.current.is_some())
    }

    pub fn current_row(&self) -> Result<&Row, SqlException> {
        self.current.as_ref().ok_or_else(|| {
            SqlException::new("ResultSet not positioned properly, perhaps you need to call next.")
        })
    }

    pub fn take_current_row(&mut self) -> Result<Row, SqlException> {
        self.current.take().ok_or_else(|| {
            SqlException::new("ResultSet not positioned properly, perhaps you need to call next.")
        })
    }

    pub fn find_column(&self, label: &str) -> Result<usize, SqlException> {
        self.metadata.find_column(label)
    }

    pub fn get_object<I: ColumnIndex>(&self, column: I) -> Result<&SqlValue, SqlException> {
        self.current_row()?.get_object(column)
    }

    pub fn get_string<I: ColumnIndex>(&self, column: I) -> Result<Option<String>, SqlException> {
        self.current_row()?.get_string(column)
    }

    pub fn get_boolean<I: ColumnIndex>(&self, column: I) -> Result<Option<bool>, SqlException> {
        self.current_row()?.get_boolean(column)
    }

    pub fn get_int<I: ColumnIndex>(&self, column: I) -> Result<Option<i32>, SqlException> {
        self.current_row()?.get_int(column)
    }

    pub fn get_long<I: ColumnIndex>(&self, column: I) -> Result<Option<i64>, SqlException> {
        self.current_row()?.get_long(column)
    }

    pub fn get_double<I: ColumnIndex>(&self, column: I) -> Result<Option<f64>, SqlException> {
        self.current_row()?.get_double(column)
    }

    pub fn get_big_decimal<I: ColumnIndex>(
        &self,
        column: I,
    ) -> Result<Option<BigDecimal>, SqlException> {
        self.current_row()?.get_big_decimal(column)
    }
}
