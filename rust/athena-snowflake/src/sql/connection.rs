use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use async_trait::async_trait;
use athena_jdbc::sql::{
    ColumnMetadata, DatabaseMetaData, JdbcConnection, PreparedStatement, ResultSet,
    ResultSetMetaData, SqlException, SqlValue, Types,
};
use regex::Regex;
use tokio::sync::OnceCell;

use super::executor::{snowflake_exception, SnowflakeExecutor, SnowflakeQueryResult};
use super::type_map::{result_column_metadata, show_column_type};
use super::wildcard::{
    escape_single_quote_for_like_command, escape_sql_quotes, is_wildcard_pattern_str, matches,
    to_regex_pattern, unescape_chars,
};

const NO_DATA: &str = "02000";
const BASE_TABLE_OR_VIEW_NOT_FOUND: &str = "42S02";
const READER_ACCOUNT_MESSAGE: &str = "Operation is not supported in reader account";
const COLUMN_NULLABLE: i32 = 1;
const COLUMN_NO_NULLS: i32 = 0;
const SUPPORTED_TABLE_TYPES: [&str; 2] = ["TABLE", "VIEW"];

static DESCRIBABLE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used, reason = "the pattern is a fixed literal")]
    Regex::new(r"(?is)^\s*(select|with)\b").expect("valid literal pattern")
});

pub(crate) fn positive_seconds(seconds: i32) -> Option<Duration> {
    u64::try_from(seconds)
        .ok()
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
}

fn int_column(name: &str) -> ColumnMetadata {
    ColumnMetadata::new(name, Types::INTEGER).with_type_name("INTEGER")
}

fn text_column(name: &str) -> ColumnMetadata {
    ColumnMetadata::new(name, Types::VARCHAR).with_type_name("TEXT")
}

fn short_column(name: &str) -> ColumnMetadata {
    ColumnMetadata::new(name, Types::SMALLINT).with_type_name("SHORT")
}

fn schemas_metadata() -> ResultSetMetaData {
    ResultSetMetaData::new(vec![
        text_column("TABLE_SCHEM"),
        text_column("TABLE_CATALOG"),
    ])
}

fn tables_metadata() -> ResultSetMetaData {
    ResultSetMetaData::new(
        [
            "TABLE_CAT",
            "TABLE_SCHEM",
            "TABLE_NAME",
            "TABLE_TYPE",
            "REMARKS",
            "TYPE_CAT",
            "TYPE_SCHEM",
            "TYPE_NAME",
            "SELF_REFERENCING_COL_NAME",
            "REF_GENERATION",
        ]
        .into_iter()
        .map(text_column)
        .collect(),
    )
}

fn columns_metadata() -> ResultSetMetaData {
    ResultSetMetaData::new(vec![
        text_column("TABLE_CAT"),
        text_column("TABLE_SCHEM"),
        text_column("TABLE_NAME"),
        text_column("COLUMN_NAME"),
        int_column("DATA_TYPE"),
        text_column("TYPE_NAME"),
        int_column("COLUMN_SIZE"),
        int_column("BUFFER_LENGTH"),
        int_column("DECIMAL_DIGITS"),
        int_column("NUM_PREC_RADIX"),
        int_column("NULLABLE"),
        text_column("REMARKS"),
        text_column("COLUMN_DEF"),
        int_column("SQL_DATA_TYPE"),
        int_column("SQL_DATETIME_SUB"),
        int_column("CHAR_OCTET_LENGTH"),
        int_column("ORDINAL_POSITION"),
        text_column("IS_NULLABLE"),
        text_column("SCOPE_CATALOG"),
        text_column("SCOPE_SCHEMA"),
        text_column("SCOPE_TABLE"),
        short_column("SOURCE_DATA_TYPE"),
        text_column("IS_AUTOINCREMENT"),
        text_column("IS_GENERATEDCOLUMN"),
    ])
}

fn primary_keys_metadata() -> ResultSetMetaData {
    ResultSetMetaData::new(vec![
        text_column("TABLE_CAT"),
        text_column("TABLE_SCHEM"),
        text_column("TABLE_NAME"),
        text_column("COLUMN_NAME"),
        short_column("KEY_SEQ"),
        text_column("PK_NAME"),
    ])
}

fn text(value: Option<&SqlValue>) -> Option<String> {
    match value? {
        SqlValue::Null => None,
        SqlValue::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn opt_text(value: Option<String>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::String)
}

fn opt_int(value: Option<i32>) -> SqlValue {
    value.map_or(SqlValue::Null, SqlValue::Integer)
}

fn show_like(pattern: Option<&str>) -> String {
    match pattern {
        Some(p) if !p.is_empty() && p.trim() != "%" && p.trim() != ".*" => {
            format!(" like '{}'", escape_single_quote_for_like_command(p))
        }
        _ => String::new(),
    }
}

enum Location {
    Account,
    Database(String),
    Schema(String, String),
    Table(String, String, String),
}

impl Location {
    fn render(&self) -> String {
        match self {
            Location::Account => "account".to_string(),
            Location::Database(db) => format!("database \"{db}\""),
            Location::Schema(db, schema) => format!("schema \"{db}\".\"{schema}\""),
            Location::Table(db, schema, table) => {
                format!("table \"{db}\".\"{schema}\".\"{table}\"")
            }
        }
    }
}

#[derive(Clone, Copy)]
enum TableScope {
    None,
    WildcardToSchema,
    Always,
}

fn location(
    catalog: Option<&str>,
    schema: Option<&str>,
    table: Option<&str>,
    table_scope: TableScope,
) -> Option<Location> {
    let Some(catalog) = catalog else {
        return Some(Location::Account);
    };
    if catalog.is_empty() {
        return None;
    }
    let catalog_escaped = escape_sql_quotes(catalog);
    let schema = match schema {
        None => return Some(Location::Database(catalog_escaped)),
        Some(s) if is_wildcard_pattern_str(Some(s)) => {
            return Some(Location::Database(catalog_escaped))
        }
        Some("") => return None,
        Some(s) => unescape_chars(s),
    };
    let table = match (table_scope, table) {
        (TableScope::None, _) | (_, None) => {
            return Some(Location::Schema(catalog_escaped, schema))
        }
        (TableScope::WildcardToSchema, Some(t)) if is_wildcard_pattern_str(Some(t)) => {
            return Some(Location::Schema(catalog_escaped, schema))
        }
        (_, Some("")) => return None,
        (_, Some(t)) => unescape_chars(t),
    };
    Some(Location::Table(catalog_escaped, schema, table))
}

fn column_row(
    row: &[SqlValue],
    schema_name: String,
    table_name: String,
    column_name: String,
    ordinal_position: i32,
) -> Result<Vec<SqlValue>, SqlException> {
    let data_type_text = text(row.get(3)).unwrap_or_default();
    let data_type: serde_json::Value = serde_json::from_str(&data_type_text)
        .map_err(|_| snowflake_exception(format!("error parsing data type: {data_type_text}")))?;
    let column_type = show_column_type(&data_type);
    let default_value = text(row.get(5)).unwrap_or_default();
    let default_value = if default_value.is_empty() {
        None
    } else if default_value.len() >= 2
        && default_value.starts_with('\'')
        && default_value.ends_with('\'')
    {
        Some(default_value[1..default_value.len() - 1].replace("''", "'"))
    } else {
        Some(default_value)
    };
    let comment = text(row.get(8));
    let catalog_name = text(row.get(9));
    let auto_increment = text(row.get(10)).unwrap_or_default();
    let nullable = column_type.nullable;
    Ok(vec![
        opt_text(catalog_name),
        SqlValue::String(schema_name),
        SqlValue::String(table_name),
        SqlValue::String(column_name),
        SqlValue::Integer(column_type.jdbc_type),
        SqlValue::String(column_type.type_name.clone()),
        opt_int(column_type.column_size),
        SqlValue::Null,
        SqlValue::Integer(column_type.scale),
        SqlValue::Null,
        SqlValue::Integer(if nullable {
            COLUMN_NULLABLE
        } else {
            COLUMN_NO_NULLS
        }),
        opt_text(comment),
        opt_text(default_value),
        SqlValue::Integer(column_type.jdbc_type),
        SqlValue::Null,
        opt_int(column_type.char_octet_length),
        SqlValue::Integer(ordinal_position),
        SqlValue::String(if nullable { "YES" } else { "NO" }.to_string()),
        SqlValue::Null,
        SqlValue::Null,
        SqlValue::Null,
        SqlValue::Null,
        SqlValue::String(
            if auto_increment.is_empty() {
                "NO"
            } else {
                "YES"
            }
            .to_string(),
        ),
        SqlValue::String("NO".to_string()),
    ])
}

pub struct SnowflakeConnection {
    executor: Arc<dyn SnowflakeExecutor>,
    user: Option<String>,
    session_context: OnceCell<(Option<String>, Option<String>)>,
    auto_commit: AtomicBool,
    closed: AtomicBool,
    query_timeout: Option<Duration>,
}

impl SnowflakeConnection {
    pub fn new(executor: Arc<dyn SnowflakeExecutor>, user: Option<String>) -> SnowflakeConnection {
        SnowflakeConnection {
            executor,
            user,
            session_context: OnceCell::new(),
            auto_commit: AtomicBool::new(true),
            closed: AtomicBool::new(false),
            query_timeout: None,
        }
    }

    #[must_use]
    pub fn with_query_timeout(mut self, query_timeout: Option<Duration>) -> SnowflakeConnection {
        self.query_timeout = query_timeout;
        self
    }

    fn check_open(&self) -> Result<(), SqlException> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(snowflake_exception("Connection is closed"));
        }
        Ok(())
    }

    async fn run(
        &self,
        sql: &str,
        binds: &[SqlValue],
        timeout: Option<Duration>,
    ) -> Result<SnowflakeQueryResult, SqlException> {
        self.check_open()?;
        tracing::debug!("Snowflake query: {sql}");
        self.executor
            .query(sql, binds, timeout.or(self.query_timeout))
            .await
    }

    async fn collect_rows(&self, sql: &str) -> Result<Vec<Vec<SqlValue>>, SqlException> {
        let mut result = self.run(sql, &[], None).await?;
        let mut rows = Vec::new();
        while let Some(row) = result.rows.next_row().await? {
            rows.push(row);
        }
        Ok(rows)
    }

    async fn show_rows_or_empty(&self, sql: &str) -> Result<Vec<Vec<SqlValue>>, SqlException> {
        match self.collect_rows(sql).await {
            Ok(rows) => Ok(rows),
            Err(e)
                if matches!(e.sql_state(), Some(NO_DATA | BASE_TABLE_OR_VIEW_NOT_FOUND))
                    || e.message().contains(READER_ACCOUNT_MESSAGE) =>
            {
                Ok(Vec::new())
            }
            Err(e) => Err(e),
        }
    }

    async fn session_context(&self) -> Result<&(Option<String>, Option<String>), SqlException> {
        self.session_context
            .get_or_try_init(|| async {
                let rows = self
                    .collect_rows("select current_database(), current_schema()")
                    .await?;
                let row = rows.into_iter().next().unwrap_or_default();
                Ok((text(row.first()), text(row.get(1))))
            })
            .await
    }

    async fn describe(&self, sql: &str) -> Result<Option<ResultSetMetaData>, SqlException> {
        if !DESCRIBABLE.is_match(sql) || sql.contains('?') {
            return Ok(None);
        }
        let trimmed = sql.trim().trim_end_matches(';');
        let probe = format!("select * from ({trimmed}) limit 0");
        match self.run(&probe, &[], None).await {
            Ok(result) => Ok(Some(ResultSetMetaData::new(
                result.columns.iter().map(result_column_metadata).collect(),
            ))),
            Err(e) => {
                tracing::debug!("Describe of prepared statement failed: {e}");
                Ok(None)
            }
        }
    }
}

#[async_trait]
impl DatabaseMetaData for SnowflakeConnection {
    async fn get_schemas(&self) -> Result<ResultSet, SqlException> {
        self.get_schemas_with_catalog(None, None).await
    }

    async fn get_schemas_with_catalog(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlException> {
        self.check_open()?;
        let compiled = to_regex_pattern(schema_pattern);
        let location = match catalog {
            None => " in account".to_string(),
            Some("") => return Ok(ResultSet::empty(schemas_metadata())),
            Some(catalog) => format!(" in database \"{}\"", escape_sql_quotes(catalog)),
        };
        let sql = format!(
            "show /* JDBC:DatabaseMetaData.getSchemas() */ schemas{}{location}",
            show_like(schema_pattern)
        );
        tracing::debug!("Sql command to get schemas metadata: {sql}");
        let rows = self
            .show_rows_or_empty(&sql)
            .await?
            .into_iter()
            .filter_map(|row| {
                let schema_name = text(row.get(1)).unwrap_or_default();
                let db_name = text(row.get(4));
                matches(compiled.as_ref(), &schema_name)
                    .then(|| vec![SqlValue::String(schema_name), opt_text(db_name)])
            })
            .collect();
        Ok(ResultSet::from_rows(schemas_metadata(), rows))
    }

    async fn get_tables(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        types: Option<&[&str]>,
    ) -> Result<ResultSet, SqlException> {
        self.check_open()?;
        let valid_types: Vec<&str> = match types {
            Some(types) => types
                .iter()
                .copied()
                .filter(|t| SUPPORTED_TABLE_TYPES.contains(t))
                .collect(),
            None => SUPPORTED_TABLE_TYPES.to_vec(),
        };
        if valid_types.is_empty() {
            return Ok(ResultSet::empty(tables_metadata()));
        }
        let compiled_schema = to_regex_pattern(schema_pattern);
        let compiled_table = to_regex_pattern(table_name_pattern);
        let view_only = valid_types.len() == 1 && valid_types[0].eq_ignore_ascii_case("VIEW");
        let table_only = valid_types.len() == 1 && valid_types[0].eq_ignore_ascii_case("TABLE");
        let command = if view_only {
            "views"
        } else if table_only {
            "tables"
        } else {
            "objects"
        };
        let Some(location) = location(catalog, schema_pattern, None, TableScope::None) else {
            return Ok(ResultSet::empty(tables_metadata()));
        };
        let sql = format!(
            "show /* JDBC:DatabaseMetaData.getTables() */ {command}{} in {}",
            show_like(table_name_pattern),
            location.render()
        );
        tracing::debug!("Sql command to get table metadata: {sql}");
        let rows = self
            .show_rows_or_empty(&sql)
            .await?
            .into_iter()
            .filter_map(|row| {
                let table_name = text(row.get(1)).unwrap_or_default();
                let (db_name, schema_name, kind, comment) = if view_only {
                    (
                        text(row.get(3)),
                        text(row.get(4)),
                        Some("VIEW".to_string()),
                        text(row.get(6)),
                    )
                } else {
                    (
                        text(row.get(2)),
                        text(row.get(3)),
                        text(row.get(4)),
                        text(row.get(5)),
                    )
                };
                let schema_text = schema_name.clone().unwrap_or_default();
                (matches(compiled_table.as_ref(), &table_name)
                    && matches(compiled_schema.as_ref(), &schema_text))
                .then(|| {
                    vec![
                        opt_text(db_name),
                        opt_text(schema_name),
                        SqlValue::String(table_name),
                        opt_text(kind),
                        opt_text(comment),
                        SqlValue::Null,
                        SqlValue::Null,
                        SqlValue::Null,
                        SqlValue::Null,
                        SqlValue::Null,
                    ]
                })
            })
            .collect();
        Ok(ResultSet::from_rows(tables_metadata(), rows))
    }

    async fn get_columns(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        column_name_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlException> {
        self.check_open()?;
        let compiled_schema = to_regex_pattern(schema_pattern);
        let compiled_table = to_regex_pattern(table_name_pattern);
        let compiled_column = to_regex_pattern(column_name_pattern);
        let Some(location) = location(
            catalog,
            schema_pattern,
            table_name_pattern,
            TableScope::WildcardToSchema,
        ) else {
            return Ok(ResultSet::empty(columns_metadata()));
        };
        let sql = format!(
            "show /* JDBC:DatabaseMetaData.getColumns() */ columns{} in {}",
            show_like(column_name_pattern),
            location.render()
        );
        tracing::debug!("Sql command to get column metadata: {sql}");
        let mut rows = Vec::new();
        let mut ordinal_position = 0;
        let mut current_table: Option<String> = None;
        for row in self.show_rows_or_empty(&sql).await? {
            let table_name = text(row.first()).unwrap_or_default();
            let schema_name = text(row.get(1)).unwrap_or_default();
            let column_name = text(row.get(2)).unwrap_or_default();
            if !(matches(compiled_table.as_ref(), &table_name)
                && matches(compiled_schema.as_ref(), &schema_name)
                && matches(compiled_column.as_ref(), &column_name))
            {
                continue;
            }
            if current_table.as_deref() == Some(table_name.as_str()) {
                ordinal_position += 1;
            } else {
                ordinal_position = 1;
                current_table = Some(table_name.clone());
            }
            rows.push(column_row(
                &row,
                schema_name,
                table_name,
                column_name,
                ordinal_position,
            )?);
        }
        Ok(ResultSet::from_rows(columns_metadata(), rows))
    }

    async fn get_primary_keys(
        &self,
        catalog: Option<&str>,
        schema: Option<&str>,
        table: &str,
    ) -> Result<ResultSet, SqlException> {
        self.check_open()?;
        let compiled_schema = to_regex_pattern(schema);
        let compiled_table = to_regex_pattern(Some(table));
        let Some(location) = location(catalog, schema, Some(table), TableScope::Always) else {
            return Ok(ResultSet::empty(primary_keys_metadata()));
        };
        let sql = format!(
            "show /* JDBC:DatabaseMetaData.getPrimaryKeys() */ primary keys in {}",
            location.render()
        );
        tracing::debug!("Sql command to get primary key metadata: {sql}");
        let rows = self
            .show_rows_or_empty(&sql)
            .await?
            .into_iter()
            .filter_map(|row| {
                let table_cat = text(row.get(1));
                let table_schem = text(row.get(2)).unwrap_or_default();
                let table_name = text(row.get(3)).unwrap_or_default();
                let column_name = text(row.get(4));
                let key_seq = text(row.get(5))
                    .and_then(|k| k.trim().parse::<i16>().ok())
                    .unwrap_or_default();
                let pk_name = text(row.get(6));
                let catalog_matches = catalog.is_none_or(|c| table_cat.as_deref() == Some(c));
                (catalog_matches
                    && matches(compiled_schema.as_ref(), &table_schem)
                    && matches(compiled_table.as_ref(), &table_name))
                .then(|| {
                    vec![
                        opt_text(table_cat),
                        SqlValue::String(table_schem),
                        SqlValue::String(table_name),
                        opt_text(column_name),
                        SqlValue::SmallInt(key_seq),
                        opt_text(pk_name),
                    ]
                })
            })
            .collect();
        Ok(ResultSet::from_rows(primary_keys_metadata(), rows))
    }

    fn get_search_string_escape(&self) -> Result<Option<String>, SqlException> {
        Ok(Some("\\".to_string()))
    }

    fn get_identifier_quote_string(&self) -> Result<String, SqlException> {
        Ok("\"".to_string())
    }

    fn get_database_product_name(&self) -> Result<String, SqlException> {
        Ok("Snowflake".to_string())
    }

    async fn get_user_name(&self) -> Result<Option<String>, SqlException> {
        Ok(self.user.clone())
    }
}

#[async_trait]
impl JdbcConnection for SnowflakeConnection {
    fn metadata(&self) -> &dyn DatabaseMetaData {
        self
    }

    async fn get_catalog(&self) -> Result<Option<String>, SqlException> {
        self.check_open()?;
        Ok(self.session_context().await?.0.clone())
    }

    async fn get_schema(&self) -> Result<Option<String>, SqlException> {
        self.check_open()?;
        Ok(self.session_context().await?.1.clone())
    }

    async fn prepare_statement(&self, sql: &str) -> Result<PreparedStatement, SqlException> {
        self.check_open()?;
        let statement = PreparedStatement::new(sql);
        Ok(match self.describe(sql).await? {
            Some(metadata) => statement.with_metadata(metadata),
            None => statement,
        })
    }

    async fn execute_query(
        &self,
        statement: &PreparedStatement,
    ) -> Result<ResultSet, SqlException> {
        let timeout = positive_seconds(statement.query_timeout());
        let result = self
            .run(statement.sql(), statement.parameters(), timeout)
            .await?;
        let metadata =
            ResultSetMetaData::new(result.columns.iter().map(result_column_metadata).collect());
        Ok(ResultSet::new(Arc::new(metadata), result.rows))
    }

    async fn execute(&self, statement: &PreparedStatement) -> Result<bool, SqlException> {
        let mut result_set = self.execute_query(statement).await?;
        let has_columns = result_set.metadata().column_count() > 0;
        while result_set.next().await? {}
        Ok(has_columns)
    }

    async fn set_auto_commit(&self, auto_commit: bool) -> Result<(), SqlException> {
        self.check_open()?;
        if self.auto_commit.load(Ordering::SeqCst) != auto_commit {
            self.auto_commit.store(auto_commit, Ordering::SeqCst);
            self.collect_rows(&format!(
                "alter session /* JDBC:SnowflakeConnectionImpl.setAutoCommit*/ set autocommit={auto_commit}"
            ))
            .await?;
        }
        Ok(())
    }

    async fn get_auto_commit(&self) -> Result<bool, SqlException> {
        self.check_open()?;
        Ok(self.auto_commit.load(Ordering::SeqCst))
    }

    async fn commit(&self) -> Result<(), SqlException> {
        self.collect_rows("commit").await.map(|_| ())
    }

    async fn rollback(&self) -> Result<(), SqlException> {
        self.collect_rows("rollback").await.map(|_| ())
    }

    async fn is_valid(&self, timeout_seconds: i32) -> Result<bool, SqlException> {
        if timeout_seconds < 0 {
            return Err(SqlException::new("timeout is less than 0"));
        }
        if self.closed.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let timeout = positive_seconds(timeout_seconds);
        Ok(self.run("select 1", &[], timeout).await.is_ok())
    }

    async fn close(&self) -> Result<(), SqlException> {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.executor.close().await?;
        }
        Ok(())
    }
}
