use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use athena_jdbc::sql::{
    DatabaseMetaData, JdbcConnection, JdbcType, PreparedStatement, RowSource, SqlError, SqlValue,
    VecRowSource,
};
use athena_snowflake::sql::{
    parse_connection_settings, url_decode, SnowflakeColumn, SnowflakeColumnType,
    SnowflakeConnection, SnowflakeExecutor, SnowflakeQueryResult, EXTRA_TYPES_TIMESTAMP_LTZ,
};

type Answer =
    Box<dyn Fn(&str) -> Result<(Vec<SnowflakeColumn>, Vec<Vec<SqlValue>>), SqlError> + Send + Sync>;

type RecordedQuery = (String, Vec<SqlValue>, Option<Duration>);

struct RecordingExecutor {
    answer: Answer,
    queries: Mutex<Vec<RecordedQuery>>,
}

impl RecordingExecutor {
    fn new(answer: Answer) -> Arc<RecordingExecutor> {
        Arc::new(RecordingExecutor {
            answer,
            queries: Mutex::new(Vec::new()),
        })
    }

    fn sql(&self) -> Vec<String> {
        self.queries
            .lock()
            .unwrap()
            .iter()
            .map(|(sql, _, _)| sql.clone())
            .collect()
    }
}

#[async_trait]
impl SnowflakeExecutor for RecordingExecutor {
    async fn query(
        &self,
        sql: &str,
        binds: &[SqlValue],
        timeout: Option<Duration>,
    ) -> Result<SnowflakeQueryResult, SqlError> {
        self.queries
            .lock()
            .unwrap()
            .push((sql.to_string(), binds.to_vec(), timeout));
        let (columns, rows) = (self.answer)(sql)?;
        let source: Box<dyn RowSource> = Box::new(VecRowSource::new(rows));
        Ok(SnowflakeQueryResult {
            columns,
            rows: source,
        })
    }
}

fn text(value: &str) -> SqlValue {
    SqlValue::String(value.to_string())
}

fn texts(values: &[&str]) -> Vec<SqlValue> {
    values.iter().map(|v| text(v)).collect()
}

fn empty() -> Answer {
    Box::new(|_| Ok((Vec::new(), Vec::new())))
}

fn connection(executor: &Arc<RecordingExecutor>) -> SnowflakeConnection {
    let executor: Arc<dyn SnowflakeExecutor> = executor.clone();
    SnowflakeConnection::new(executor)
}

#[tokio::test]
async fn get_tables_builds_show_objects_and_maps_rows() {
    let executor = RecordingExecutor::new(Box::new(|_| {
        Ok((
            Vec::new(),
            vec![
                texts(&[
                    "2024-01-01",
                    "ORDERS",
                    "DB",
                    "PUBLIC",
                    "TABLE",
                    "order rows",
                ]),
                texts(&["2024-01-01", "OTHER", "DB", "SALES", "TABLE", ""]),
            ],
        ))
    }));
    let connection = connection(&executor);
    let result = connection
        .tables(
            Some("DB"),
            Some("PUBLIC"),
            Some("ORD%"),
            Some(&["TABLE", "VIEW"]),
        )
        .await
        .unwrap();
    assert_eq!(
        vec!["show /* JDBC:DatabaseMetaData.getTables() */ objects like 'ORD%' in schema \"DB\".\"PUBLIC\"".to_string()],
        executor.sql()
    );
    assert_eq!(1, result.len());
    assert_eq!(Some("DB".to_string()), result[0].catalog);
    assert_eq!(Some("PUBLIC".to_string()), result[0].schema);
    assert_eq!("ORDERS", result[0].name);
    assert_eq!(Some("TABLE".to_string()), result[0].kind);
}

#[tokio::test]
async fn get_tables_for_views_reads_view_columns() {
    let executor = RecordingExecutor::new(Box::new(|_| {
        Ok((
            Vec::new(),
            vec![texts(&[
                "2024-01-01",
                "V1",
                "",
                "DB",
                "PUBLIC",
                "owner",
                "a view",
            ])],
        ))
    }));
    let connection = connection(&executor);
    let result = connection
        .tables(Some("DB"), Some("PUBLIC"), None, Some(&["VIEW"]))
        .await
        .unwrap();
    assert_eq!(
        vec![
            "show /* JDBC:DatabaseMetaData.getTables() */ views in schema \"DB\".\"PUBLIC\""
                .to_string()
        ],
        executor.sql()
    );
    assert_eq!(Some("VIEW".to_string()), result[0].kind);
    assert_eq!(Some("a view".to_string()), result[0].remarks);
}

#[tokio::test]
async fn get_columns_parses_show_columns_output() {
    let executor = RecordingExecutor::new(Box::new(|_| {
        let row = |name: &str, json: &str| {
            texts(&[
                "MY_TABLE", "PUBLIC", name, json, "true", "", "COLUMN", "", "", "DB", "",
            ])
        };
        Ok((
            Vec::new(),
            vec![
                row(
                    "ID",
                    r#"{"type":"FIXED","precision":38,"scale":0,"nullable":false}"#,
                ),
                row(
                    "PRICE",
                    r#"{"type":"FIXED","precision":10,"scale":2,"nullable":true}"#,
                ),
                row(
                    "NAME",
                    r#"{"type":"TEXT","length":100,"byteLength":400,"nullable":true,"fixed":false}"#,
                ),
                row(
                    "CREATED",
                    r#"{"type":"TIMESTAMP_TZ","precision":0,"scale":9,"nullable":true}"#,
                ),
            ],
        ))
    }));
    let connection = connection(&executor);
    let result = connection
        .columns(Some("DB"), Some("PUBLIC"), Some("MY\\_TABLE"), None)
        .await
        .unwrap();
    assert_eq!(
        vec!["show /* JDBC:DatabaseMetaData.getColumns() */ columns in table \"DB\".\"PUBLIC\".\"MY_TABLE\"".to_string()],
        executor.sql()
    );
    let columns: Vec<_> = result
        .into_iter()
        .map(|column| {
            (
                column.name.unwrap(),
                column.data_type,
                column.type_name.unwrap(),
                column.column_size,
                column.decimal_digits,
                Some(column.ordinal_position),
            )
        })
        .collect();
    assert_eq!(
        vec![
            (
                "ID".to_string(),
                JdbcType::BigInt,
                "NUMBER".to_string(),
                Some(38),
                Some(0),
                Some(1)
            ),
            (
                "PRICE".to_string(),
                JdbcType::Decimal,
                "NUMBER".to_string(),
                Some(10),
                Some(2),
                Some(2)
            ),
            (
                "NAME".to_string(),
                JdbcType::Varchar,
                "VARCHAR".to_string(),
                Some(100),
                Some(0),
                Some(3)
            ),
            (
                "CREATED".to_string(),
                JdbcType::TimestampWithTimezone,
                "TIMESTAMPTZ".to_string(),
                Some(0),
                Some(9),
                Some(4),
            ),
        ],
        columns
    );
}

#[tokio::test]
async fn get_primary_keys_and_schemas_build_show_sql() {
    let executor = RecordingExecutor::new(empty());
    let connection = connection(&executor);
    connection
        .primary_key_columns(Some("DB"), Some("PUBLIC"), "T")
        .await
        .unwrap();
    connection.schemas(Some("DB"), None).await.unwrap();
    connection.schemas(None, None).await.unwrap();
    assert_eq!(
        vec![
            "show /* JDBC:DatabaseMetaData.getPrimaryKeys() */ primary keys in table \"DB\".\"PUBLIC\".\"T\"".to_string(),
            "show /* JDBC:DatabaseMetaData.getSchemas() */ schemas in database \"DB\"".to_string(),
            "show /* JDBC:DatabaseMetaData.getSchemas() */ schemas in account".to_string(),
        ],
        executor.sql()
    );
}

#[tokio::test]
async fn show_commands_return_empty_for_missing_objects() {
    let executor = RecordingExecutor::new(Box::new(|_| {
        Err(SqlError::new("Object does not exist").with_sql_state("02000"))
    }));
    let connection = connection(&executor);
    let result = connection
        .tables(Some("DB"), Some("MISSING"), None, None)
        .await
        .unwrap();
    assert!(result.is_empty());
    let error = connection
        .execute_query(&PreparedStatement::new("select 2"))
        .await
        .unwrap_err();
    assert_eq!("Object does not exist", error.message());
}

#[tokio::test]
async fn result_metadata_follows_the_jdbc_driver_type_map() {
    let executor = RecordingExecutor::new(Box::new(|_| {
        Ok((
            vec![
                SnowflakeColumn::new(
                    "ID",
                    SnowflakeColumnType::Fixed {
                        precision: 18,
                        scale: 0,
                    },
                    false,
                ),
                SnowflakeColumn::new(
                    "PRICE",
                    SnowflakeColumnType::Fixed {
                        precision: 10,
                        scale: 2,
                    },
                    true,
                ),
                SnowflakeColumn::new("RATE", SnowflakeColumnType::Real, true),
                SnowflakeColumn::new("NAME", SnowflakeColumnType::Text { length: 50 }, true),
                SnowflakeColumn::new("LTZ", SnowflakeColumnType::TimestampLtz { scale: 9 }, true),
                SnowflakeColumn::new("DOC", SnowflakeColumnType::Variant, true),
            ],
            Vec::new(),
        ))
    }));
    let connection = connection(&executor);
    let result = connection
        .execute_query(&PreparedStatement::new("select * from t"))
        .await
        .unwrap();
    let metadata = result.metadata();
    assert_eq!(JdbcType::BigInt, metadata.column(0).unwrap().jdbc_type());
    assert_eq!(18, metadata.column(0).unwrap().precision());
    assert_eq!(19, metadata.column(0).unwrap().display_size());
    assert_eq!(JdbcType::Decimal, metadata.column(1).unwrap().jdbc_type());
    assert_eq!(2, metadata.column(1).unwrap().scale());
    assert_eq!(12, metadata.column(1).unwrap().display_size());
    assert_eq!(JdbcType::Double, metadata.column(2).unwrap().jdbc_type());
    assert_eq!(24, metadata.column(2).unwrap().display_size());
    assert_eq!(JdbcType::Varchar, metadata.column(3).unwrap().jdbc_type());
    assert_eq!(50, metadata.column(3).unwrap().precision());
    assert_eq!(JdbcType::Timestamp, metadata.column(4).unwrap().jdbc_type());
    assert_ne!(
        EXTRA_TYPES_TIMESTAMP_LTZ,
        metadata.column(4).unwrap().jdbc_type()
    );
    assert_eq!(JdbcType::Varchar, metadata.column(5).unwrap().jdbc_type());
}

#[tokio::test]
async fn prepare_statement_describes_select_without_parameters() {
    let executor = RecordingExecutor::new(Box::new(|sql| {
        if sql.ends_with(") limit 0") {
            return Ok((
                vec![SnowflakeColumn::new(
                    "A",
                    SnowflakeColumnType::Boolean,
                    true,
                )],
                Vec::new(),
            ));
        }
        Ok((Vec::new(), Vec::new()))
    }));
    let connection = connection(&executor);
    let described = connection
        .prepare_statement("SELECT a FROM t;")
        .await
        .unwrap();
    assert_eq!(
        JdbcType::Boolean,
        described.metadata().unwrap().column(0).unwrap().jdbc_type()
    );
    let with_parameter = connection
        .prepare_statement("SELECT a FROM t WHERE b = ?")
        .await
        .unwrap();
    assert!(with_parameter.metadata().is_none());
    let show = connection
        .prepare_statement("SHOW PRIMARY KEYS IN t")
        .await
        .unwrap();
    assert!(show.metadata().is_none());
    assert_eq!(
        vec!["select * from (SELECT a FROM t) limit 0".to_string()],
        executor.sql()
    );
}

#[tokio::test]
async fn query_timeout_and_binds_reach_the_executor() {
    let executor = RecordingExecutor::new(empty());
    let connection = connection(&executor);
    let mut statement = PreparedStatement::new("select x from t where y = ?");
    statement.bind("v");
    statement.set_query_timeout(Duration::from_mins(2));
    connection.execute_query(&statement).await.unwrap();
    let (sql, binds, timeout) = executor.queries.lock().unwrap()[0].clone();
    assert_eq!("select x from t where y = ?", sql);
    assert_eq!(vec![text("v")], binds);
    assert_eq!(Some(Duration::from_mins(2)), timeout);
}

#[tokio::test]
async fn session_statements_match_the_jdbc_driver() {
    let executor = RecordingExecutor::new(Box::new(|sql| {
        if sql.starts_with("select current_database()") {
            return Ok((Vec::new(), vec![texts(&["DB", "PUBLIC"])]));
        }
        Ok((Vec::new(), Vec::new()))
    }));
    let connection = connection(&executor);
    connection.set_auto_commit(false).await.unwrap();
    assert!(!connection.get_auto_commit().await.unwrap());
    connection.commit().await.unwrap();
    connection.rollback().await.unwrap();
    assert!(connection.is_valid(5).await.unwrap());
    assert_eq!(
        Some("DB".to_string()),
        connection.get_catalog().await.unwrap()
    );
    assert_eq!(
        Some("PUBLIC".to_string()),
        connection.get_schema().await.unwrap()
    );
    assert_eq!(
        Some("PUBLIC".to_string()),
        connection.get_schema().await.unwrap()
    );
    connection.close().await.unwrap();
    assert!(connection.get_catalog().await.is_err());
    assert_eq!(
        vec![
            "alter session /* JDBC:SnowflakeConnectionImpl.setAutoCommit*/ set autocommit=false"
                .to_string(),
            "commit".to_string(),
            "rollback".to_string(),
            "select 1".to_string(),
            "select current_database(), current_schema()".to_string(),
        ],
        executor.sql()
    );
}

#[test]
fn connection_settings_parse_the_jdbc_url() {
    let properties = HashMap::from([
        ("user".to_string(), "athena".to_string()),
        ("warehouse".to_string(), "\"WH\"".to_string()),
    ]);
    let settings = parse_connection_settings(
        "jdbc:snowflake://myorg-acct.snowflakecomputing.com/?db=SALES&schema=PUBLIC&role=READER",
        &properties,
    )
    .unwrap();
    assert_eq!(Some("athena"), settings.user.as_deref());
    assert_eq!("myorg-acct", settings.account);
    assert_eq!(
        "https://myorg-acct.snowflakecomputing.com/",
        settings.base_url.as_str()
    );
    assert_eq!(Some("SALES"), settings.database.as_deref());
    assert_eq!(Some("READER"), settings.role.as_deref());
    let error = parse_connection_settings("jdbc:mysql://host/", &properties).unwrap_err();
    assert_eq!(
        "Invalid Connect String: jdbc:mysql://host/.",
        error.message()
    );
}

#[test]
fn url_decode_follows_java_url_decoder() {
    assert_eq!("a b/c", url_decode("a+b%2Fc").unwrap());
    assert!(url_decode("bad%zz").is_err());
}
