mod snowflake_support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::{BlockAllocator, BlockAllocatorImpl, SchemaBuilder, Value};
use athena_federation_sdk::domain::predicate::{Constraints, Range, SortedRangeSet, ValueSet};
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    HandlerContext, MetadataContext, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::metadata::{GetSplitsRequest, GetTableLayoutRequest, GetTableRequest};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::{FederatedIdentity, FederationResponse};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, JdbcDriver, JdbcProperties,
};
use athena_jdbc::sql::{JdbcConnection, RowSource, SqlException, SqlValue, VecRowSource};
use athena_snowflake::connection::SnowflakeConnectionFactory;
use athena_snowflake::resolver::SnowflakeJdbcCaseResolver;
use athena_snowflake::sql::{
    SnowflakeColumn, SnowflakeColumnType, SnowflakeConnection, SnowflakeDriver, SnowflakeExecutor,
    SnowflakeQueryResult,
};
use athena_snowflake::utils::SnowflakePrivateKey;
use athena_snowflake::{
    S3ParquetExportReader, SnowflakeConstants, SnowflakeMetadataDialect, SnowflakeMetadataHandler,
    SnowflakeRecordDialect, SnowflakeRecordHandler,
};
use indexmap::IndexMap;
use snowflake_support::Fakes;

const DATABASE: &str = "TESTDB";
const SCHEMA: &str = "CMRN";

#[derive(Clone)]
struct FakeTable {
    columns: Vec<(String, SnowflakeColumnType, String)>,
    rows: Vec<Vec<SqlValue>>,
    view: bool,
    row_count: i64,
    primary_key: Option<String>,
}

impl FakeTable {
    fn new(columns: &[(&str, SnowflakeColumnType, &str)], rows: Vec<Vec<SqlValue>>) -> FakeTable {
        let row_count = i64::try_from(rows.len()).unwrap();
        FakeTable {
            columns: columns
                .iter()
                .map(|(name, kind, json)| ((*name).to_string(), kind.clone(), (*json).to_string()))
                .collect(),
            rows,
            view: false,
            row_count,
            primary_key: None,
        }
    }

    fn result_columns(&self, wanted: Option<&[String]>) -> Vec<SnowflakeColumn> {
        self.columns
            .iter()
            .filter(|(name, _, _)| wanted.is_none_or(|w| w.contains(name)))
            .map(|(name, kind, _)| SnowflakeColumn::new(name.clone(), kind.clone(), true))
            .collect()
    }

    fn result_rows(&self, wanted: Option<&[String]>) -> Vec<Vec<SqlValue>> {
        let indexes: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, (name, _, _))| wanted.is_none_or(|w| w.contains(name)))
            .map(|(i, _)| i)
            .collect();
        self.rows
            .iter()
            .map(|row| indexes.iter().map(|i| row[*i].clone()).collect())
            .collect()
    }
}

#[derive(Clone, Default)]
struct FakeSnowflake {
    tables: Arc<Mutex<HashMap<String, FakeTable>>>,
    queries: QueryLog,
}

type QueryLog = Arc<Mutex<Vec<(String, Vec<SqlValue>)>>>;

fn text(value: &str) -> SqlValue {
    SqlValue::String(value.to_string())
}

fn result(columns: Vec<SnowflakeColumn>, rows: Vec<Vec<SqlValue>>) -> SnowflakeQueryResult {
    let source: Box<dyn RowSource> = Box::new(VecRowSource::new(rows));
    SnowflakeQueryResult {
        columns,
        rows: source,
    }
}

fn text_columns(names: &[&str]) -> Vec<SnowflakeColumn> {
    names
        .iter()
        .map(|name| SnowflakeColumn::new(*name, SnowflakeColumnType::Text { length: 256 }, true))
        .collect()
}

fn quoted_columns(sql: &str) -> Vec<String> {
    let select = sql.split(" FROM ").next().unwrap_or_default();
    select
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, name)| name.to_string())
        .collect()
}

impl FakeSnowflake {
    fn add(&self, name: &str, table: FakeTable) {
        self.tables.lock().unwrap().insert(name.to_string(), table);
    }

    fn table_in(&self, sql: &str) -> Option<FakeTable> {
        self.tables
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| sql.contains(&format!("\"{SCHEMA}\".\"{name}\"")))
            .map(|(_, table)| table.clone())
    }

    fn table_named(&self, name: Option<&SqlValue>) -> Option<FakeTable> {
        let Some(SqlValue::String(name)) = name else {
            return None;
        };
        self.tables.lock().unwrap().get(name).cloned()
    }

    fn queries(&self) -> Vec<(String, Vec<SqlValue>)> {
        self.queries.lock().unwrap().clone()
    }

    fn data_queries(&self) -> Vec<(String, Vec<SqlValue>)> {
        self.queries()
            .into_iter()
            .filter(|(sql, _)| {
                sql.starts_with("SELECT ")
                    && sql.contains(&format!("FROM \"{SCHEMA}\""))
                    && !sql.contains("COUNTS")
            })
            .collect()
    }

    fn show_columns(&self, sql: &str) -> SnowflakeQueryResult {
        let table = self.table_in(sql.replace(&format!("\"{DATABASE}\"."), "").as_str());
        let rows = table
            .map(|t| {
                let table_name = sql.rsplit('"').nth(1).unwrap_or_default().to_string();
                t.columns
                    .iter()
                    .map(|(name, _, json)| {
                        vec![
                            text(&table_name),
                            text(SCHEMA),
                            text(name),
                            text(json),
                            text("true"),
                            text(""),
                            text("COLUMN"),
                            text(""),
                            text(""),
                            text(DATABASE),
                            text(""),
                        ]
                    })
                    .collect()
            })
            .unwrap_or_default();
        result(
            text_columns(&[
                "table_name",
                "schema_name",
                "column_name",
                "data_type",
                "null?",
                "default",
                "kind",
                "expression",
                "comment",
                "database_name",
                "autoincrement",
            ]),
            rows,
        )
    }

    fn answer(&self, sql: &str, binds: &[SqlValue]) -> SnowflakeQueryResult {
        if sql == "select 1" {
            return result(text_columns(&["1"]), vec![vec![text("1")]]);
        }
        if sql.starts_with("select current_database()") {
            return result(
                text_columns(&["CURRENT_DATABASE()", "CURRENT_SCHEMA()"]),
                vec![vec![text(DATABASE), text("PUBLIC")]],
            );
        }
        if sql.starts_with("show /* JDBC:DatabaseMetaData.getColumns() */") {
            return self.show_columns(sql);
        }
        if sql == SnowflakeConstants::VIEW_CHECK_QUERY {
            let view = self.table_named(binds.get(1)).is_some_and(|t| t.view);
            let rows = if view {
                vec![vec![binds[0].clone(), binds[1].clone()]]
            } else {
                Vec::new()
            };
            return result(text_columns(&["TABLE_SCHEMA", "TABLE_NAME"]), rows);
        }
        if sql == SnowflakeConstants::COUNT_RECORDS_QUERY {
            let count = self.table_named(binds.get(1)).map_or(0, |t| t.row_count);
            return result(
                vec![SnowflakeColumn::new(
                    "ROW_COUNT",
                    SnowflakeColumnType::Fixed {
                        precision: 38,
                        scale: 0,
                    },
                    true,
                )],
                vec![vec![SqlValue::BigInt(count)]],
            );
        }
        if sql.starts_with(SnowflakeConstants::SHOW_PRIMARY_KEYS_QUERY) {
            let rows = self
                .table_in(sql)
                .and_then(|t| t.primary_key)
                .map(|key| vec![vec![text(&key)]])
                .unwrap_or_default();
            return result(text_columns(&["column_name"]), rows);
        }
        if sql.contains("COUNTS") {
            return result(
                vec![SnowflakeColumn::new(
                    "COUNTS",
                    SnowflakeColumnType::Fixed {
                        precision: 18,
                        scale: 0,
                    },
                    true,
                )],
                vec![vec![SqlValue::BigInt(1)]],
            );
        }
        if let Some(inner) = sql
            .strip_prefix("select * from (")
            .and_then(|rest| rest.strip_suffix(") limit 0"))
        {
            let table = self.table_in(inner);
            return result(
                table.map(|t| t.result_columns(None)).unwrap_or_default(),
                Vec::new(),
            );
        }
        if let Some(table) = self.table_in(sql) {
            let wanted = quoted_columns(sql);
            let wanted = (!wanted.is_empty()).then_some(wanted);
            return result(
                table.result_columns(wanted.as_deref()),
                table.result_rows(wanted.as_deref()),
            );
        }
        result(Vec::new(), Vec::new())
    }
}

#[async_trait]
impl SnowflakeExecutor for FakeSnowflake {
    async fn query(
        &self,
        sql: &str,
        binds: &[SqlValue],
        _timeout: Option<Duration>,
    ) -> Result<SnowflakeQueryResult, SqlException> {
        self.queries
            .lock()
            .unwrap()
            .push((sql.to_string(), binds.to_vec()));
        Ok(self.answer(sql, binds))
    }
}

struct FakeDriver {
    database: FakeSnowflake,
}

#[async_trait]
impl JdbcDriver for FakeDriver {
    fn driver_class_name(&self) -> &str {
        SnowflakeConstants::SNOWFLAKE_DRIVER_CLASS
    }

    async fn connect(
        &self,
        _url: &str,
        properties: &JdbcProperties,
    ) -> Result<Box<dyn JdbcConnection>, SqlException> {
        Ok(Box::new(SnowflakeConnection::new(
            Arc::new(self.database.clone()),
            properties.get("user").cloned(),
        )))
    }
}

#[async_trait]
impl SnowflakeDriver for FakeDriver {
    async fn connect_with_private_key(
        &self,
        url: &str,
        properties: &JdbcProperties,
        _private_key: &SnowflakePrivateKey,
    ) -> Result<Box<dyn JdbcConnection>, SqlException> {
        self.connect(url, properties).await
    }
}

struct Lambda {
    database: FakeSnowflake,
    metadata: SnowflakeMetadataHandler,
    record: SnowflakeRecordHandler,
    allocator: Arc<dyn BlockAllocator>,
}

fn lambda() -> Lambda {
    let database = FakeSnowflake::default();
    let fakes = Fakes::new();
    let config = DatabaseConnectionConfig::new(
        "snowflake",
        SnowflakeConstants::SNOWFLAKE_NAME,
        "snowflake://jdbc:snowflake://account.snowflakecomputing.com/?warehouse=WH&db=TESTDB&schema=CMRN&user=athena&password=secret",
    )
    .unwrap();
    let options = HashMap::from([
        ("spill_bucket".to_string(), "spill".to_string()),
        ("disable_spill_encryption".to_string(), "true".to_string()),
    ]);
    let factory = Arc::new(
        SnowflakeConnectionFactory::new_with_driver(
            config.clone(),
            Some(SnowflakeConstants::jdbc_properties()),
            DatabaseConnectionInfo::new(
                SnowflakeConstants::SNOWFLAKE_DRIVER_CLASS,
                SnowflakeConstants::SNOWFLAKE_DEFAULT_PORT,
            )
            .unwrap(),
            Some(&options),
            Arc::new(FakeDriver {
                database: database.clone(),
            }),
        )
        .unwrap(),
    );
    let metadata_context = MetadataContext::builder(config.engine(), fakes.clients())
        .config_options(options.clone())
        .build()
        .unwrap();
    let metadata = SnowflakeMetadataDialect::handler(
        metadata_context,
        config.clone(),
        factory.clone(),
        Arc::new(SnowflakeJdbcCaseResolver::new(
            SnowflakeConstants::SNOWFLAKE_NAME,
        )),
        None,
    );
    let record_clients = fakes.clients();
    let export_reader = Arc::new(S3ParquetExportReader::new(Arc::clone(&record_clients.s3)));
    let record_context = HandlerContext::new(config.engine(), record_clients, options).unwrap();
    let record = SnowflakeRecordDialect::handler(record_context, config, factory, export_reader);
    Lambda {
        database,
        metadata,
        record,
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

fn constraints(summary: IndexMap<String, ValueSet>, limit: i64) -> Constraints {
    Constraints::new(summary, Vec::new(), Vec::new(), limit, HashMap::new(), None)
}

fn equal(
    allocator: &Arc<dyn BlockAllocator>,
    data_type: &DataType,
    value: impl Into<Value>,
) -> ValueSet {
    ValueSet::from(
        SortedRangeSet::of_ranges(
            Range::equal(allocator.as_ref(), data_type, value).unwrap(),
            Vec::new(),
        )
        .unwrap(),
    )
}

fn at_least(
    allocator: &Arc<dyn BlockAllocator>,
    data_type: &DataType,
    value: impl Into<Value>,
) -> ValueSet {
    ValueSet::from(
        SortedRangeSet::of_ranges(
            Range::greater_than_or_equal(allocator.as_ref(), data_type, value).unwrap(),
            Vec::new(),
        )
        .unwrap(),
    )
}

struct QueryRun {
    schema: Schema,
    splits: usize,
    rows: usize,
    values: Vec<Vec<Value>>,
}

async fn read_splits(
    lambda: &Lambda,
    table_name: &TableName,
    read_schema: &Schema,
    splits: &[athena_federation_sdk::domain::split::Split],
    constraints: &Constraints,
) -> (usize, Vec<Vec<Value>>) {
    let mut rows = 0;
    let mut values = Vec::new();
    for split in splits {
        let response = lambda
            .record
            .read_records(
                &lambda.allocator,
                &ReadRecordsRequest::new(
                    Some(identity()),
                    "snowflake",
                    "query",
                    table_name.clone(),
                    read_schema.clone(),
                    split.clone(),
                    constraints.clone(),
                    100_000_000,
                    100_000_000,
                ),
            )
            .await
            .unwrap();
        let FederationResponse::ReadRecords(response) = response else {
            panic!("expected inline records");
        };
        let block = response.records();
        rows += block.row_count();
        for row in 0..block.row_count() {
            values.push(
                block
                    .field_vectors()
                    .iter()
                    .map(|vector| vector.get_object(row))
                    .collect(),
            );
        }
    }
    (rows, values)
}

async fn run_query(
    lambda: &Lambda,
    table: &str,
    projection: Option<&[&str]>,
    constraints: Constraints,
) -> QueryRun {
    let table_name = TableName::new(SCHEMA, table);
    let table_response = lambda
        .metadata
        .get_table(&GetTableRequest::new(
            Some(identity()),
            Some("query".to_string()),
            "snowflake",
            table_name.clone(),
            HashMap::new(),
        ))
        .await
        .unwrap();
    let schema = table_response.schema().clone();
    let layout = lambda
        .metadata
        .get_table_layout(
            lambda.allocator.as_ref(),
            &GetTableLayoutRequest::new(
                Some(identity()),
                Some("query".to_string()),
                "snowflake",
                table_name.clone(),
                constraints.clone(),
                schema.clone(),
                vec!["partition".to_string()],
            ),
        )
        .await
        .unwrap();
    let splits = lambda
        .metadata
        .get_splits(&GetSplitsRequest::new(
            Some(identity()),
            Some("query".to_string()),
            "snowflake",
            table_name.clone(),
            layout.partitions().clone(),
            vec!["partition".to_string()],
            constraints.clone(),
            None,
        ))
        .await
        .unwrap();
    let mut builder = SchemaBuilder::new_builder();
    for field in schema.fields() {
        let keep = field.name() == "partition"
            || projection.is_none_or(|names| names.contains(&field.name().as_str()));
        if keep {
            builder.add_field(Arc::clone(field));
        }
    }
    let read_schema = builder.build();
    let (rows, values) = read_splits(
        lambda,
        &table_name,
        &read_schema,
        splits.splits(),
        &constraints,
    )
    .await;
    QueryRun {
        schema,
        splits: splits.splits().len(),
        rows,
        values,
    }
}

fn issuance_table() -> FakeTable {
    FakeTable::new(
        &[
            (
                "FREQUENCY",
                SnowflakeColumnType::Text { length: 64 },
                r#"{"type":"TEXT","length":64,"byteLength":256,"nullable":true,"fixed":false}"#,
            ),
            (
                "AMOUNT",
                SnowflakeColumnType::Fixed {
                    precision: 10,
                    scale: 2,
                },
                r#"{"type":"FIXED","precision":10,"scale":2,"nullable":true}"#,
            ),
        ],
        vec![
            vec![
                text("Monthly Issuance"),
                SqlValue::Decimal("12.50".parse().unwrap()),
            ],
            vec![
                text("Weekly Issuance"),
                SqlValue::Decimal("3.25".parse().unwrap()),
            ],
            vec![
                text("Monthly Issuance"),
                SqlValue::Decimal("8.00".parse().unwrap()),
            ],
        ],
    )
}

fn types_table() -> FakeTable {
    FakeTable::new(
        &[
            (
                "ID",
                SnowflakeColumnType::Fixed {
                    precision: 38,
                    scale: 0,
                },
                r#"{"type":"FIXED","precision":38,"scale":0,"nullable":false}"#,
            ),
            (
                "NAME",
                SnowflakeColumnType::Text { length: 100 },
                r#"{"type":"TEXT","length":100,"byteLength":400,"nullable":true,"fixed":false}"#,
            ),
            (
                "FLOATCOL",
                SnowflakeColumnType::Real,
                r#"{"type":"REAL","nullable":true}"#,
            ),
            (
                "PARSEDDATE",
                SnowflakeColumnType::Date,
                r#"{"type":"DATE","nullable":true}"#,
            ),
            (
                "TS_NTZ",
                SnowflakeColumnType::TimestampNtz { scale: 9 },
                r#"{"type":"TIMESTAMP_NTZ","precision":0,"scale":9,"nullable":true}"#,
            ),
            (
                "FLAG",
                SnowflakeColumnType::Boolean,
                r#"{"type":"BOOLEAN","nullable":true}"#,
            ),
        ],
        vec![vec![
            SqlValue::BigInt(58),
            text("credit"),
            SqlValue::Double(4.0),
            SqlValue::Date(chrono::NaiveDate::from_ymd_opt(2013, 1, 1).unwrap()),
            SqlValue::Timestamp(
                chrono::NaiveDate::from_ymd_opt(2016, 10, 19)
                    .unwrap()
                    .and_hms_opt(15, 39, 47)
                    .unwrap(),
            ),
            SqlValue::Boolean(true),
        ]],
    )
}

fn last_data_query(lambda: &Lambda) -> (String, Vec<SqlValue>) {
    lambda.database.data_queries().last().cloned().unwrap()
}

#[tokio::test]
async fn fetch_data_types() {
    let lambda = lambda();
    lambda.database.add("EMPLOYEE", types_table());
    let run = run_query(&lambda, "EMPLOYEE", None, constraints(IndexMap::new(), -1)).await;
    let type_of = |name: &str| {
        run.schema
            .field_with_name(name)
            .unwrap()
            .data_type()
            .clone()
    };
    assert_eq!(DataType::Decimal128(38, 0), type_of("ID"));
    assert_eq!(DataType::Utf8, type_of("NAME"));
    assert_eq!(DataType::Float64, type_of("FLOATCOL"));
    assert_eq!(DataType::Date32, type_of("PARSEDDATE"));
    assert_eq!(DataType::Date64, type_of("TS_NTZ"));
    assert_eq!(DataType::Boolean, type_of("FLAG"));
    assert_eq!(1, run.rows);
    let predicates: Vec<(&str, ValueSet)> = vec![
        (
            "ID",
            equal(
                &lambda.allocator,
                &DataType::Decimal128(38, 0),
                Value::Decimal(58.into()),
            ),
        ),
        ("NAME", equal(&lambda.allocator, &DataType::Utf8, "credit")),
        (
            "FLOATCOL",
            equal(&lambda.allocator, &DataType::Float64, 4.0_f64),
        ),
    ];
    for (column, value_set) in predicates {
        let summary = IndexMap::from([(column.to_string(), value_set)]);
        let run = run_query(&lambda, "EMPLOYEE", None, constraints(summary, -1)).await;
        assert_eq!(1, run.rows);
        let (sql, binds) = last_data_query(&lambda);
        assert!(sql.contains(&format!("(\"{column}\" = ?)")), "{sql}");
        assert_eq!(1, binds.len(), "{sql}");
    }
    let summary = IndexMap::from([(
        "PARSEDDATE".to_string(),
        equal(&lambda.allocator, &DataType::Date32, Value::Int(15706)),
    )]);
    run_query(&lambda, "EMPLOYEE", None, constraints(summary, -1)).await;
    let (sql, binds) = last_data_query(&lambda);
    assert!(sql.contains("(\"PARSEDDATE\" = ?)"), "{sql}");
    assert_eq!(
        vec![SqlValue::Date(
            chrono::NaiveDate::from_ymd_opt(2013, 1, 1).unwrap()
        )],
        binds
    );
    let run = run_query(&lambda, "EMPLOYEE", None, constraints(IndexMap::new(), 2)).await;
    assert_eq!(1, run.rows);
    let (sql, _) = last_data_query(&lambda);
    assert!(sql.contains("LIMIT"), "{sql}");
}

#[tokio::test]
async fn fetch_data_select_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let run = run_query(&lambda, "ISSUANCE", None, constraints(IndexMap::new(), -1)).await;
    assert_eq!(1, run.splits);
    assert_eq!(3, run.rows);
    let (sql, binds) = last_data_query(&lambda);
    assert_eq!(
        "SELECT \"FREQUENCY\", \"AMOUNT\" FROM \"CMRN\".\"ISSUANCE\"",
        sql
    );
    assert!(binds.is_empty());
}

#[tokio::test]
async fn fetch_data_select_count_all_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let run = run_query(
        &lambda,
        "ISSUANCE",
        Some(&["FREQUENCY"]),
        constraints(IndexMap::new(), -1),
    )
    .await;
    assert_eq!(3, run.rows);
}

#[tokio::test]
async fn fetch_data_where_clause_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let summary = IndexMap::from([(
        "FREQUENCY".to_string(),
        equal(&lambda.allocator, &DataType::Utf8, "Monthly Issuance"),
    )]);
    run_query(&lambda, "ISSUANCE", None, constraints(summary, -1)).await;
    let (sql, binds) = last_data_query(&lambda);
    assert!(sql.contains(" WHERE (\"FREQUENCY\" = ?)"), "{sql}");
    assert_eq!(vec![text("Monthly Issuance")], binds);
}

#[tokio::test]
async fn fetch_data_where_clause_like_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let run = run_query(&lambda, "ISSUANCE", None, constraints(IndexMap::new(), -1)).await;
    let (sql, _) = last_data_query(&lambda);
    assert!(!sql.contains("WHERE"), "{sql}");
    let matching = run
        .values
        .iter()
        .filter(|row| matches!(&row[0], Value::String(s) if s.contains("Issuance")))
        .count();
    assert_eq!(3, matching);
}

#[tokio::test]
async fn fetch_data_group_by_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let run = run_query(
        &lambda,
        "ISSUANCE",
        Some(&["FREQUENCY"]),
        constraints(IndexMap::new(), -1),
    )
    .await;
    let (sql, _) = last_data_query(&lambda);
    assert!(
        sql.starts_with("SELECT \"FREQUENCY\" FROM \"CMRN\".\"ISSUANCE\""),
        "{sql}"
    );
    let mut groups: Vec<String> = run
        .values
        .iter()
        .map(|row| format!("{:?}", row[0]))
        .collect();
    groups.sort();
    groups.dedup();
    assert_eq!(2, groups.len());
}

#[tokio::test]
async fn fetch_data_group_by_having_clause_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let summary = IndexMap::from([(
        "FREQUENCY".to_string(),
        equal(&lambda.allocator, &DataType::Utf8, "Weekly Issuance"),
    )]);
    run_query(
        &lambda,
        "ISSUANCE",
        Some(&["FREQUENCY"]),
        constraints(summary, -1),
    )
    .await;
    let (sql, binds) = last_data_query(&lambda);
    assert!(
        sql.starts_with(
            "SELECT \"FREQUENCY\" FROM \"CMRN\".\"ISSUANCE\" WHERE (\"FREQUENCY\" = ?)"
        ),
        "{sql}"
    );
    assert_eq!(vec![text("Weekly Issuance")], binds);
}

#[tokio::test]
async fn fetch_data_union_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let first = run_query(
        &lambda,
        "ISSUANCE",
        Some(&["FREQUENCY"]),
        constraints(IndexMap::new(), -1),
    )
    .await;
    let second = run_query(
        &lambda,
        "ISSUANCE",
        Some(&["FREQUENCY"]),
        constraints(IndexMap::new(), -1),
    )
    .await;
    assert_eq!(6, first.rows + second.rows);
    assert_eq!(2, lambda.database.data_queries().len());
}

#[tokio::test]
async fn fetch_data_where_clause_distinct_integ_test() {
    let lambda = lambda();
    lambda.database.add("ISSUANCE", issuance_table());
    let summary = IndexMap::from([(
        "FREQUENCY".to_string(),
        equal(&lambda.allocator, &DataType::Utf8, "Issuance"),
    )]);
    run_query(
        &lambda,
        "ISSUANCE",
        Some(&["FREQUENCY"]),
        constraints(summary, -1),
    )
    .await;
    let (sql, binds) = last_data_query(&lambda);
    assert!(sql.contains("(\"FREQUENCY\" = ?)"), "{sql}");
    assert_eq!(vec![text("Issuance")], binds);
}

#[tokio::test]
async fn fetch_data_select_integ_view_test() {
    let lambda = lambda();
    let mut view = issuance_table();
    view.view = true;
    lambda.database.add("ISSUANCE_VIEW", view);
    let run = run_query(
        &lambda,
        "ISSUANCE_VIEW",
        None,
        constraints(IndexMap::new(), -1),
    )
    .await;
    assert_eq!(1, run.splits);
    assert_eq!(3, run.rows);
    let (sql, _) = last_data_query(&lambda);
    assert_eq!(
        "SELECT \"FREQUENCY\", \"AMOUNT\" FROM \"CMRN\".\"ISSUANCE_VIEW\"",
        sql
    );
    assert!(!lambda
        .database
        .queries()
        .iter()
        .any(|(sql, _)| sql == SnowflakeConstants::COUNT_RECORDS_QUERY));
}

#[tokio::test]
async fn run_partion_test() {
    let lambda = lambda();
    let mut card = issuance_table();
    card.row_count = 20_000;
    card.primary_key = Some("FREQUENCY".to_string());
    lambda.database.add("AAB_COMPLETEDCARD", card);
    lambda.database.add("AAB_COMPLETEDACCT", issuance_table());
    let mut order = issuance_table();
    order.row_count = 20_000;
    lambda.database.add("AAB_COMPLETEDORDER", order);
    let card_run = run_query(
        &lambda,
        "AAB_COMPLETEDCARD",
        None,
        constraints(IndexMap::new(), -1),
    )
    .await;
    assert_eq!(50, card_run.splits);
    let card_queries: Vec<String> = lambda
        .database
        .data_queries()
        .into_iter()
        .map(|(sql, _)| sql)
        .collect();
    assert!(card_queries.contains(
        &"SELECT \"FREQUENCY\", \"AMOUNT\" FROM \"CMRN\".\"AAB_COMPLETEDCARD\" ORDER BY \"FREQUENCY\" LIMIT 400 OFFSET 19600".to_string()
    ));
    let acct_run = run_query(
        &lambda,
        "AAB_COMPLETEDACCT",
        None,
        constraints(IndexMap::new(), -1),
    )
    .await;
    assert_eq!(1, acct_run.splits);
    let order_run = run_query(
        &lambda,
        "AAB_COMPLETEDORDER",
        None,
        constraints(IndexMap::new(), -1),
    )
    .await;
    assert_eq!(1, order_run.splits);
}

#[tokio::test]
async fn get_large_table_with_limit() {
    let lambda = lambda();
    let mut large = issuance_table();
    large.row_count = 100_000_000;
    lambda.database.add("COVID19_DATA_100M", large);
    let summary = IndexMap::from([(
        "AMOUNT".to_string(),
        at_least(
            &lambda.allocator,
            &DataType::Decimal128(10, 2),
            Value::Decimal(0.into()),
        ),
    )]);
    let run = run_query(
        &lambda,
        "COVID19_DATA_100M",
        None,
        constraints(summary, 100_000),
    )
    .await;
    assert_eq!(1, run.splits);
    let (sql, _) = last_data_query(&lambda);
    assert!(sql.contains("LIMIT"), "{sql}");
    assert!(!lambda
        .database
        .queries()
        .iter()
        .any(|(sql, _)| sql.contains("COUNTS")));
}

#[tokio::test]
async fn get_throughput() {
    let lambda = lambda();
    lambda.database.add("COVID19_DATA", issuance_table());
    let count = run_query(
        &lambda,
        "COVID19_DATA",
        Some(&["FREQUENCY"]),
        constraints(IndexMap::new(), -1),
    )
    .await;
    let started = Instant::now();
    let run = run_query(
        &lambda,
        "COVID19_DATA",
        None,
        constraints(IndexMap::new(), -1),
    )
    .await;
    let elapsed = started.elapsed().as_secs_f64().max(f64::EPSILON);
    let throughput = f64::from(u32::try_from(count.rows).unwrap()) / elapsed;
    assert_eq!(count.rows, run.rows);
    assert!(throughput > 0.0);
}
