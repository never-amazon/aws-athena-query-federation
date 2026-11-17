use async_trait::async_trait;

use super::prepared_statement::PreparedStatement;
use super::result_set::ResultSet;
use super::sql_exception::SqlException;

#[async_trait]
pub trait DatabaseMetaData: Send + Sync {
    async fn get_schemas(&self) -> Result<ResultSet, SqlException>;

    async fn get_schemas_with_catalog(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlException>;

    async fn get_tables(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        types: Option<&[&str]>,
    ) -> Result<ResultSet, SqlException>;

    async fn get_columns(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        column_name_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlException>;

    async fn get_primary_keys(
        &self,
        catalog: Option<&str>,
        schema: Option<&str>,
        table: &str,
    ) -> Result<ResultSet, SqlException>;

    fn get_search_string_escape(&self) -> Result<Option<String>, SqlException>;

    fn get_identifier_quote_string(&self) -> Result<String, SqlException>;

    fn get_database_product_name(&self) -> Result<String, SqlException>;

    async fn get_user_name(&self) -> Result<Option<String>, SqlException>;
}

#[async_trait]
pub trait JdbcConnection: Send + Sync {
    fn metadata(&self) -> &dyn DatabaseMetaData;

    async fn get_catalog(&self) -> Result<Option<String>, SqlException>;

    async fn get_schema(&self) -> Result<Option<String>, SqlException>;

    async fn prepare_statement(&self, sql: &str) -> Result<PreparedStatement, SqlException>;

    async fn execute_query(&self, statement: &PreparedStatement)
        -> Result<ResultSet, SqlException>;

    async fn execute(&self, statement: &PreparedStatement) -> Result<bool, SqlException>;

    async fn execute_query_with_sql(&self, sql: &str) -> Result<ResultSet, SqlException> {
        let statement = self.prepare_statement(sql).await?;
        self.execute_query(&statement).await
    }

    async fn set_auto_commit(&self, auto_commit: bool) -> Result<(), SqlException>;

    async fn get_auto_commit(&self) -> Result<bool, SqlException>;

    async fn commit(&self) -> Result<(), SqlException>;

    async fn rollback(&self) -> Result<(), SqlException>;

    async fn is_valid(&self, timeout_seconds: i32) -> Result<bool, SqlException>;

    async fn close(&self) -> Result<(), SqlException>;
}
