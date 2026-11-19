use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;

use super::result_set_metadata::ResultSetMetaData;
use super::sql_error::SqlError;
use super::sql_value::SqlValue;

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

    pub fn value(&self, index: usize) -> Result<&SqlValue, SqlError> {
        self.metadata.column(index)?;
        Ok(self.values.get(index).unwrap_or(&SqlValue::Null))
    }

    pub fn by_label(&self, label: &str) -> Result<&SqlValue, SqlError> {
        self.value(self.metadata.find_column(label)?)
    }
}

#[async_trait]
pub trait RowSource: Send {
    async fn next_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlError>;
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
    async fn next_row(&mut self) -> Result<Option<Vec<SqlValue>>, SqlError> {
        Ok(self.rows.pop_front())
    }
}

pub struct ResultSet {
    metadata: Arc<ResultSetMetaData>,
    source: Box<dyn RowSource>,
}

impl fmt::Debug for ResultSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResultSet")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl ResultSet {
    pub fn new(metadata: Arc<ResultSetMetaData>, source: Box<dyn RowSource>) -> ResultSet {
        ResultSet { metadata, source }
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

    pub async fn next_row(&mut self) -> Result<Option<Row>, SqlError> {
        Ok(self
            .source
            .next_row()
            .await?
            .map(|values| Row::new(Arc::clone(&self.metadata), values)))
    }
}
