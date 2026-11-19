use std::sync::Arc;

use async_trait::async_trait;

use super::prepared_statement::PreparedStatement;
use super::result_set::ResultSet;
use super::result_set_metadata::ResultSetMetaData;
use super::sql_error::SqlError;
use super::types::JdbcType;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaEntry {
    pub schema: Option<String>,
    pub catalog: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableEntry {
    pub catalog: Option<String>,
    pub schema: Option<String>,
    pub name: String,
    pub kind: Option<String>,
    pub remarks: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ColumnInfo {
    pub name: Option<String>,
    pub data_type: JdbcType,
    pub type_name: Option<String>,
    pub column_size: Option<i32>,
    pub decimal_digits: Option<i32>,
    pub ordinal_position: i32,
}

#[async_trait]
pub trait DatabaseMetaData: Send + Sync {
    async fn schemas(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
    ) -> Result<Vec<SchemaEntry>, SqlError>;

    async fn tables(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        types: Option<&[&str]>,
    ) -> Result<Vec<TableEntry>, SqlError>;

    async fn columns(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        column_name_pattern: Option<&str>,
    ) -> Result<Vec<ColumnInfo>, SqlError>;

    async fn primary_key_columns(
        &self,
        catalog: Option<&str>,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<Option<String>>, SqlError>;

    fn search_string_escape(&self) -> Option<String>;

    fn database_product_name(&self) -> String;
}

#[async_trait]
pub trait JdbcConnection: Send + Sync {
    fn metadata(&self) -> &dyn DatabaseMetaData;

    async fn get_catalog(&self) -> Result<Option<String>, SqlError>;

    async fn get_schema(&self) -> Result<Option<String>, SqlError>;

    async fn prepare_statement(&self, sql: &str) -> Result<PreparedStatement, SqlError>;

    async fn execute_query(&self, statement: &PreparedStatement) -> Result<ResultSet, SqlError>;

    async fn execute(&self, statement: &PreparedStatement) -> Result<bool, SqlError>;

    async fn statement_metadata(
        &self,
        statement: &PreparedStatement,
    ) -> Result<Option<Arc<ResultSetMetaData>>, SqlError> {
        Ok(statement.metadata().cloned())
    }

    async fn parameter_count(
        &self,
        statement: &PreparedStatement,
    ) -> Result<Option<usize>, SqlError> {
        Ok(statement.parameter_count())
    }

    async fn execute_query_with_sql(&self, sql: &str) -> Result<ResultSet, SqlError> {
        let statement = self.prepare_statement(sql).await?;
        self.execute_query(&statement).await
    }

    async fn set_auto_commit(&self, auto_commit: bool) -> Result<(), SqlError>;

    async fn get_auto_commit(&self) -> Result<bool, SqlError>;

    async fn commit(&self) -> Result<(), SqlError>;

    async fn rollback(&self) -> Result<(), SqlError>;

    async fn is_valid(&self, timeout_seconds: i32) -> Result<bool, SqlError>;

    async fn close(&self) -> Result<(), SqlError>;
}
