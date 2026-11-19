use super::sql_error::SqlError;
use super::types::JdbcType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Nullability {
    NoNulls,
    Nullable,
    #[default]
    Unknown,
}

impl Nullability {
    pub fn jdbc_code(self) -> i32 {
        match self {
            Nullability::NoNulls => 0,
            Nullability::Nullable => 1,
            Nullability::Unknown => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnMetadata {
    name: String,
    label: String,
    jdbc_type: JdbcType,
    type_name: String,
    precision: i32,
    scale: i32,
    display_size: i32,
    nullability: Nullability,
    table_name: String,
    schema_name: String,
    catalog_name: String,
}

impl ColumnMetadata {
    pub fn new(name: impl Into<String>, jdbc_type: JdbcType) -> ColumnMetadata {
        let name = name.into();
        ColumnMetadata {
            label: name.clone(),
            name,
            jdbc_type,
            ..ColumnMetadata::default()
        }
    }

    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> ColumnMetadata {
        self.label = label.into();
        self
    }

    #[must_use]
    pub fn with_type_name(mut self, type_name: impl Into<String>) -> ColumnMetadata {
        self.type_name = type_name.into();
        self
    }

    #[must_use]
    pub fn with_precision(mut self, precision: i32) -> ColumnMetadata {
        self.precision = precision;
        self
    }

    #[must_use]
    pub fn with_scale(mut self, scale: i32) -> ColumnMetadata {
        self.scale = scale;
        self
    }

    #[must_use]
    pub fn with_display_size(mut self, display_size: i32) -> ColumnMetadata {
        self.display_size = display_size;
        self
    }

    #[must_use]
    pub fn with_nullability(mut self, nullability: Nullability) -> ColumnMetadata {
        self.nullability = nullability;
        self
    }

    #[must_use]
    pub fn with_table_name(mut self, table_name: impl Into<String>) -> ColumnMetadata {
        self.table_name = table_name.into();
        self
    }

    #[must_use]
    pub fn with_schema_name(mut self, schema_name: impl Into<String>) -> ColumnMetadata {
        self.schema_name = schema_name.into();
        self
    }

    #[must_use]
    pub fn with_catalog_name(mut self, catalog_name: impl Into<String>) -> ColumnMetadata {
        self.catalog_name = catalog_name.into();
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn jdbc_type(&self) -> JdbcType {
        self.jdbc_type
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    pub fn precision(&self) -> i32 {
        self.precision
    }

    pub fn scale(&self) -> i32 {
        self.scale
    }

    pub fn display_size(&self) -> i32 {
        self.display_size
    }

    pub fn nullability(&self) -> Nullability {
        self.nullability
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResultSetMetaData {
    columns: Vec<ColumnMetadata>,
}

impl ResultSetMetaData {
    pub fn new(columns: Vec<ColumnMetadata>) -> ResultSetMetaData {
        ResultSetMetaData { columns }
    }

    pub fn of_labels<I, S>(labels: I) -> ResultSetMetaData
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ResultSetMetaData::new(
            labels
                .into_iter()
                .map(|label| ColumnMetadata::new(label, JdbcType::Varchar))
                .collect(),
        )
    }

    pub fn columns(&self) -> &[ColumnMetadata] {
        &self.columns
    }

    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    pub fn column(&self, index: usize) -> Result<&ColumnMetadata, SqlError> {
        self.columns.get(index).ok_or_else(|| {
            SqlError::new(format!(
                "The column index is out of range: {}, number of columns: {}.",
                index + 1,
                self.columns.len()
            ))
        })
    }

    pub fn find_column(&self, label: &str) -> Result<usize, SqlError> {
        self.columns
            .iter()
            .position(|c| c.label == label)
            .or_else(|| {
                self.columns
                    .iter()
                    .position(|c| c.label.eq_ignore_ascii_case(label))
            })
            .ok_or_else(|| {
                SqlError::new(format!(
                    "The column name {label} was not found in this ResultSet."
                ))
            })
    }
}
