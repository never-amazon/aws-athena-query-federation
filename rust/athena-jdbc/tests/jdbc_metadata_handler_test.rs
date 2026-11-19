mod jdbc_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, FieldBuilder, SchemaBuilder, SimpleBlockWriter,
};
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    FederationRequestHandler, MetadataHandler, MetadataHandlerBase,
};
use athena_federation_sdk::metadata::optimizations::querypassthrough::{
    ENABLE_QUERY_PASSTHROUGH, SCHEMA_FUNCTION_NAME,
};
use athena_federation_sdk::metadata::{
    GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::util::UNLIMITED_PAGE_SIZE_VALUE;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{JdbcMetadataHandler, JdbcMetadataHandlerBase, JdbcMetadataHandlerExt};
use athena_jdbc::qpt::{NAME, QUERY, SCHEMA_NAME};
use athena_jdbc::sql::{
    ColumnMetadata, PreparedStatement, ResultSetMetaData, SqlException, SqlValue, Types,
};
use jdbc_support::{
    FakeConnection, FakeConnectionFactory, FakeSecretsManager, Fakes, GetColumnsCall,
    GetTablesCall, ResultSetData,
};

const TABLE_TYPES: [&str; 4] = ["TABLE", "VIEW", "EXTERNAL TABLE", "MATERIALIZED VIEW"];

fn partition_schema() -> Schema {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_field(FieldBuilder::new_builder("testPartitionCol", DataType::Utf8).build());
    builder.build()
}

struct TestHandler {
    base: JdbcMetadataHandlerBase,
}

impl FederationRequestHandler for TestHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.metadata_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.metadata_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

#[async_trait]
impl MetadataHandler for TestHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        self.base.metadata_base()
    }

    async fn do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.jdbc_do_list_schema_names(allocator, request).await
    }

    async fn do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.jdbc_do_list_tables(allocator, request).await
    }

    async fn do_get_query_passthrough_schema(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.jdbc_do_get_query_passthrough_schema(allocator, request)
            .await
    }

    async fn do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.jdbc_do_get_table(allocator, request).await
    }

    async fn get_partitions(
        &self,
        _block_writer: &mut SimpleBlockWriter<'_>,
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        Ok(())
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        Ok(GetSplitsResponse::new(request.catalog_name(), [], None))
    }
}

#[async_trait]
impl JdbcMetadataHandler for TestHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase {
        &self.base
    }

    fn get_partition_schema(&self, _catalog_name: &str) -> Result<Schema> {
        Ok(partition_schema())
    }
}

struct Fixture {
    handler: TestHandler,
    factory: FakeConnectionFactory,
    factory_arc: Arc<FakeConnectionFactory>,
    connection: FakeConnection,
    allocator: Arc<dyn BlockAllocator>,
}

fn metadata_base() -> MetadataHandlerBase {
    let fakes = Fakes::with_secrets(FakeSecretsManager::with_secret(
        "testSecret",
        "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
    ));
    MetadataHandlerBase::builder("fakedatabase", fakes.clients())
        .config_options(HashMap::new())
        .build()
        .unwrap()
}

fn setup() -> Fixture {
    let connection = FakeConnection::new();
    connection.set_catalog(Some("testCatalog"));
    let factory = FakeConnectionFactory::new(connection.clone());
    let config = DatabaseConnectionConfig::new_with_secret(
        "testCatalog",
        "fakedatabase",
        "fakedatabase://jdbc:fakedatabase://hostname/${testSecret}",
        "testSecret",
    )
    .unwrap();
    let factory_arc = Arc::new(factory.clone());
    let base = JdbcMetadataHandlerBase::new(metadata_base(), config, factory_arc.clone());
    Fixture {
        handler: TestHandler { base },
        factory,
        factory_arc,
        connection,
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn federated_identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

fn list_tables_request(schema: &str, token: Option<&str>, page_size: i32) -> ListTablesRequest {
    ListTablesRequest::new(
        Some(federated_identity()),
        Some("testQueryId".to_string()),
        "testCatalog",
        Some(schema.to_string()),
        token.map(ToString::to_string),
        page_size,
    )
}

fn get_table_request(table: &TableName, qpt: HashMap<String, Option<String>>) -> GetTableRequest {
    GetTableRequest::new(
        Some(federated_identity()),
        Some("testQueryId".to_string()),
        "testCatalog",
        table.clone(),
        qpt,
    )
}

fn tables(rows: &[(&str, &str)]) -> ResultSetData {
    let rows: Vec<[&str; 2]> = rows.iter().map(|(s, t)| [*s, *t]).collect();
    let refs: Vec<&[&str]> = rows.iter().map(<[&str; 2]>::as_slice).collect();
    ResultSetData::of_strings(&["TABLE_SCHEM", "TABLE_NAME"], &refs)
}

fn five_tables() -> ResultSetData {
    tables(&[
        ("testSchema", "testTable"),
        ("testSchema", "testTable2"),
        ("testSchema", "testTable3"),
        ("testSchema", "testTable4"),
        ("testSchema", "testTable5"),
    ])
}

fn get_tables_call(schema_pattern: &str) -> GetTablesCall {
    GetTablesCall {
        catalog: Some("testCatalog".to_string()),
        schema_pattern: Some(schema_pattern.to_string()),
        table_name_pattern: None,
        types: Some(TABLE_TYPES.iter().map(ToString::to_string).collect()),
    }
}

fn assert_get_tables_calls(fixture: &Fixture, schema_pattern: &str) {
    let calls = fixture.connection.meta.get_tables_calls();
    assert!(!calls.is_empty());
    assert!(
        calls
            .iter()
            .all(|call| *call == get_tables_call(schema_pattern)),
        "{calls:?}"
    );
}

fn table_names(names: &[(&str, &str)]) -> Vec<TableName> {
    names.iter().map(|(s, t)| TableName::new(*s, *t)).collect()
}

fn assert_connector_exception(error: &FederationError) {
    assert!(error.as_connector_exception().is_some(), "{error:?}");
}

fn assert_sql_exception(error: &FederationError) {
    assert!(
        SqlException::from_federation_error(error).is_some(),
        "{error:?}"
    );
}

type ColumnRow<'a> = (i32, i32, &'a str, i32, i32, Option<&'a str>);

fn columns_result(rows: Vec<ColumnRow<'_>>) -> ResultSetData {
    let mut labels = vec![
        "DATA_TYPE",
        "COLUMN_SIZE",
        "COLUMN_NAME",
        "DECIMAL_DIGITS",
        "NUM_PREC_RADIX",
    ];
    let with_type_name = rows.iter().any(|r| r.5.is_some());
    if with_type_name {
        labels.push("TYPE_NAME");
    }
    let metadata = ResultSetMetaData::of_labels(labels.iter().copied());
    let rows = rows
        .into_iter()
        .map(|(data_type, size, name, digits, radix, type_name)| {
            let mut row = vec![
                SqlValue::Integer(data_type),
                SqlValue::Integer(size),
                SqlValue::String(name.to_string()),
                SqlValue::Integer(digits),
                SqlValue::Integer(radix),
            ];
            if with_type_name {
                row.push(SqlValue::String(type_name.unwrap_or_default().to_string()));
            }
            row
        })
        .collect();
    ResultSetData::new(metadata, rows)
}

#[tokio::test]
async fn get_jdbc_connection_factory() {
    let fixture = setup();
    let actual = fixture
        .handler
        .jdbc_metadata_base()
        .jdbc_connection_factory()
        .unwrap();
    assert_eq!(
        Arc::as_ptr(actual).cast::<()>(),
        Arc::as_ptr(&fixture.factory_arc).cast::<()>()
    );
}

#[tokio::test]
async fn close_delegates_to_jdbc_connection_factory() {
    let fixture = setup();
    fixture.handler.close().await.unwrap();
    assert_eq!(fixture.factory.close_calls(), 1);
}

#[tokio::test]
async fn close_with_null_factory_is_no_op() {
    let handler = TestHandler {
        base: JdbcMetadataHandlerBase::new_without_connection(metadata_base()),
    };
    assert!(handler
        .jdbc_metadata_base()
        .jdbc_connection_factory()
        .is_none());
    handler.close().await.unwrap();
}

#[tokio::test]
async fn do_list_schema_names() {
    let fixture = setup();
    fixture
        .connection
        .meta
        .set_schemas(Ok(ResultSetData::of_strings(
            &["TABLE_SCHEM"],
            &[&["testDB"], &["testdb2"], &["information_schema"]],
        )));
    let response = fixture
        .handler
        .do_list_schema_names(
            &fixture.allocator,
            &ListSchemasRequest::new(
                Some(federated_identity()),
                Some("testQueryId".to_string()),
                "testCatalog",
            ),
        )
        .await
        .unwrap();
    assert_eq!(response.schemas(), ["testDB", "testdb2"]);
}

#[tokio::test]
async fn do_list_tables() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(tables(&[
        ("testSchema", "testTable"),
        ("testSchema", "testtable2"),
    ])));
    let response = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", None, UNLIMITED_PAGE_SIZE_VALUE),
        )
        .await
        .unwrap();
    assert_eq!(
        response.tables(),
        table_names(&[("testSchema", "testTable"), ("testSchema", "testtable2")])
    );
    assert_get_tables_calls(&fixture, "testSchema");
}

#[tokio::test]
async fn do_list_tables_fake_pagination_single_page() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(tables(&[
        ("testSchema", "testTable"),
        ("testSchema", "testtable2"),
        ("testSchema", "testtable3"),
        ("testSchema", "testtable4"),
        ("testSchema", "testtable5"),
    ])));
    let response = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", None, 1),
        )
        .await
        .unwrap();
    assert_eq!(
        response.tables(),
        table_names(&[("testSchema", "testTable")])
    );
    assert_eq!(response.next_token(), Some("1"));
    assert_get_tables_calls(&fixture, "testSchema");
}

#[tokio::test]
async fn do_list_tables_fake_pagination_multi_page() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(five_tables()));
    let response = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", None, 5),
        )
        .await
        .unwrap();
    assert_eq!(
        response.tables(),
        table_names(&[
            ("testSchema", "testTable"),
            ("testSchema", "testTable2"),
            ("testSchema", "testTable3"),
            ("testSchema", "testTable4"),
            ("testSchema", "testTable5"),
        ])
    );
    assert_eq!(response.next_token(), None);
    assert_get_tables_calls(&fixture, "testSchema");
}

#[tokio::test]
async fn do_list_tables_fake_pagination_unlimited_page_size() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(five_tables()));
    let response = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", Some("1"), UNLIMITED_PAGE_SIZE_VALUE),
        )
        .await
        .unwrap();
    assert_eq!(
        response.tables(),
        table_names(&[
            ("testSchema", "testTable2"),
            ("testSchema", "testTable3"),
            ("testSchema", "testTable4"),
            ("testSchema", "testTable5"),
        ])
    );
    assert_eq!(response.next_token(), None);
    assert_get_tables_calls(&fixture, "testSchema");
}

#[tokio::test]
async fn do_list_tables_number_format_exception() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(five_tables()));
    let error = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request(
                "testSchema",
                Some("not_a_valid_number"),
                UNLIMITED_PAGE_SIZE_VALUE,
            ),
        )
        .await
        .unwrap_err();
    assert_connector_exception(&error);
    let calls = fixture.connection.meta.get_tables_calls();
    assert!(
        calls
            .iter()
            .all(|call| *call == get_tables_call("testSchema")),
        "{calls:?}"
    );
}

#[tokio::test]
async fn do_list_tables_invalid_argument_exception_negative_token() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(five_tables()));
    let error = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", Some("-1"), UNLIMITED_PAGE_SIZE_VALUE),
        )
        .await
        .unwrap_err();
    assert_connector_exception(&error);
    let calls = fixture.connection.meta.get_tables_calls();
    assert!(
        calls
            .iter()
            .all(|call| *call == get_tables_call("testSchema")),
        "{calls:?}"
    );
}

#[tokio::test]
async fn do_list_tables_invalid_argument_exception_negative_page_size() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(five_tables()));
    let error = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", Some("0"), -3),
        )
        .await
        .unwrap_err();
    assert_connector_exception(&error);
    let calls = fixture.connection.meta.get_tables_calls();
    assert!(
        calls
            .iter()
            .all(|call| *call == get_tables_call("testSchema")),
        "{calls:?}"
    );
}

#[tokio::test]
async fn do_list_tables_escaped() {
    let fixture = setup();
    fixture.connection.meta.set_tables(Ok(tables(&[
        ("test_Schema", "testTable"),
        ("test_Schema", "testtable2"),
    ])));
    fixture.connection.meta.set_search_string_escape(Some("\\"));
    let response = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("test_Schema", None, UNLIMITED_PAGE_SIZE_VALUE),
        )
        .await
        .unwrap();
    assert_eq!(
        response.tables(),
        table_names(&[("test_Schema", "testTable"), ("test_Schema", "testtable2")])
    );
    assert_get_tables_calls(&fixture, "test\\_Schema");
}

#[tokio::test]
async fn do_list_tables_escaped_exception() {
    let fixture = setup();
    fixture.connection.meta.set_search_string_escape(Some("_"));
    let error = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("test_Schema", None, UNLIMITED_PAGE_SIZE_VALUE),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(error, FederationError::IllegalArgument(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn do_get_table() {
    let fixture = setup();
    fixture.connection.meta.set_columns(Ok(columns_result(vec![
        (Types::INTEGER, 12, "testCol1", 0, 0, Some("_int4")),
        (Types::VARCHAR, 25, "testCol2", 0, 0, Some("VARCHAR")),
        (Types::TIMESTAMP, 93, "testCol3", 0, 0, Some("_timestamp")),
        (
            Types::TIMESTAMP_WITH_TIMEZONE,
            93,
            "testCol4",
            0,
            0,
            Some("_timestamp"),
        ),
        (Types::ARRAY, 0, "testCol5", 0, 0, Some("_array")),
    ])));

    let mut expected_builder = SchemaBuilder::new_builder();
    expected_builder.add_field(FieldBuilder::new_builder("testCol1", DataType::Int32).build());
    expected_builder.add_field(FieldBuilder::new_builder("testCol2", DataType::Utf8).build());
    expected_builder.add_field(FieldBuilder::new_builder("testCol3", DataType::Date64).build());
    expected_builder.add_field(FieldBuilder::new_builder("testCol4", DataType::Date64).build());
    expected_builder.add_list_field("testCol5", DataType::Utf8);
    for field in partition_schema().fields() {
        expected_builder.add_field(Arc::clone(field));
    }
    let expected = expected_builder.build();

    let input_table_name = TableName::new("testSchema", "testTable");
    let response = fixture
        .handler
        .do_get_table(
            &fixture.allocator,
            &get_table_request(&input_table_name, HashMap::new()),
        )
        .await
        .unwrap();
    assert_eq!(response.schema(), &expected);
    assert_eq!(response.table_name(), &input_table_name);
    assert_eq!(response.catalog_name(), "testCatalog");
    let call = &fixture.connection.meta.get_columns_calls()[0];
    assert_eq!(call.catalog.as_deref(), Some("testCatalog"));
    assert_eq!(call.schema_pattern.as_deref(), Some("testSchema"));
    assert_eq!(call.table_name_pattern.as_deref(), Some("testTable"));
    assert_eq!(call.column_name_pattern, None);
}

#[tokio::test]
async fn do_get_query_passthrough_schema() {
    let fixture = setup();
    let query = "select testCol1 from testTable";
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("testCol1", Types::INTEGER)
        .with_label("testCol1")
        .with_precision(0)
        .with_display_size(0)]);
    fixture.connection.on_prepare(move |sql| {
        assert_eq!(sql, "select testCol1 from testTable");
        Ok(PreparedStatement::new(sql).with_metadata(metadata.clone()))
    });

    let mut expected_builder = SchemaBuilder::new_builder();
    expected_builder.add_field(FieldBuilder::new_builder("testCol1", DataType::Int32).build());
    let expected = expected_builder.build();

    let qpt: HashMap<String, Option<String>> = [
        (SCHEMA_FUNCTION_NAME, "system.query"),
        (ENABLE_QUERY_PASSTHROUGH, "true"),
        (NAME, "query"),
        (SCHEMA_NAME, "system"),
        (QUERY, query),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), Some(v.to_string())))
    .collect();
    let input_table_name = TableName::new("testSchema", "testTable");
    let response = fixture
        .handler
        .do_get_query_passthrough_schema(
            &fixture.allocator,
            &get_table_request(&input_table_name, qpt),
        )
        .await
        .unwrap();
    assert_eq!(response.schema(), &expected);
    assert_eq!(response.table_name(), &input_table_name);
    assert_eq!(response.catalog_name(), "testCatalog");
}

#[tokio::test]
async fn do_get_table_case_insensitive() {
    let fixture = setup();
    fixture.connection.meta.set_columns(Ok(columns_result(vec![(
        Types::INTEGER,
        12,
        "testCol1",
        0,
        0,
        None,
    )])));
    let input_table_name = TableName::new("testSchema", "testTable");
    let response = fixture
        .handler
        .do_get_table(
            &fixture.allocator,
            &get_table_request(&input_table_name, HashMap::new()),
        )
        .await
        .unwrap();
    assert_eq!(response.table_name().table_name(), "testTable");
    let calls = fixture.connection.meta.get_columns_calls();
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|call| *call
            == GetColumnsCall {
                catalog: Some("testCatalog".to_string()),
                schema_pattern: Some("testSchema".to_string()),
                table_name_pattern: Some("testTable".to_string()),
                column_name_pattern: None,
            }),
        "{calls:?}"
    );
}

#[tokio::test]
async fn do_get_table_no_columns() {
    let fixture = setup();
    let input_table_name = TableName::new("testSchema", "testTable");
    let response = fixture
        .handler
        .do_get_table(
            &fixture.allocator,
            &get_table_request(&input_table_name, HashMap::new()),
        )
        .await
        .unwrap();
    assert_eq!(response.table_name().table_name(), "testTable");
    assert!(response.schema().fields().is_empty());
}

#[tokio::test]
async fn do_get_table_sql_exception() {
    let fixture = setup();
    fixture
        .connection
        .meta
        .set_columns(Err(SqlException::new("")));
    let input_table_name = TableName::new("testSchema", "testTable");
    let error = fixture
        .handler
        .do_get_table(
            &fixture.allocator,
            &get_table_request(&input_table_name, HashMap::new()),
        )
        .await
        .unwrap_err();
    assert_sql_exception(&error);
}

#[tokio::test]
async fn do_list_schema_names_sql_exception() {
    let fixture = setup();
    fixture
        .connection
        .meta
        .set_schemas(Err(SqlException::new("")));
    let error = fixture
        .handler
        .do_list_schema_names(
            &fixture.allocator,
            &ListSchemasRequest::new(
                Some(federated_identity()),
                Some("testQueryId".to_string()),
                "testCatalog",
            ),
        )
        .await
        .unwrap_err();
    assert_sql_exception(&error);
}

#[tokio::test]
async fn do_list_tables_sql_exception() {
    let fixture = setup();
    fixture
        .connection
        .meta
        .set_tables(Err(SqlException::new("")));
    let error = fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &list_tables_request("testSchema", None, UNLIMITED_PAGE_SIZE_VALUE),
        )
        .await
        .unwrap_err();
    assert_sql_exception(&error);
}
