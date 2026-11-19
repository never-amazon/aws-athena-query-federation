mod handler_support;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::data::{MinorType, SchemaBuilder, SimpleBlockWriter};
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    get_source_table_name, populate_source_table_name_if_available, GlueMetadataHandler,
    MetadataContext, MetadataHandler, COLUMN_NAME_MAPPING_PROPERTY,
    DATETIME_FORMAT_MAPPING_PROPERTY, DATETIME_FORMAT_MAPPING_PROPERTY_NORMALIZED,
    GET_TABLES_REQUEST_MAX_RESULTS, SOURCE_TABLE_PROPERTY,
};
use athena_federation_sdk::lambda::LambdaContext;
use athena_federation_sdk::metadata::glue::GlueClient;
use athena_federation_sdk::metadata::{
    GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
    UNLIMITED_PAGE_SIZE_VALUE,
};
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, FederatedIdentity, KeySource,
};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use aws_sdk_glue::operation::get_databases::{GetDatabasesInput, GetDatabasesOutput};
use aws_sdk_glue::operation::get_table::{GetTableInput, GetTableOutput};
use aws_sdk_glue::operation::get_tables::{GetTablesInput, GetTablesOutput};
use aws_sdk_glue::types::{Column, Database, StorageDescriptor, Table};
use handler_support::Fakes;

const ACCOUNT_ID: &str = "account";
const QUERY_ID: &str = "queryId";
const CATALOG: &str = "default";
const SCHEMA: &str = "database1";
const TABLE: &str = "table1";

fn fake_identity() -> FederatedIdentity {
    FederatedIdentity::new(
        "arn",
        ACCOUNT_ID,
        HashMap::new(),
        Vec::new(),
        HashMap::new(),
    )
}

fn table_named(name: &str) -> Table {
    Table::builder().name(name).build().unwrap()
}

fn un_paginated_tables() -> Vec<Table> {
    ["table3", "table2", "table5", "table4", "table1"]
        .into_iter()
        .map(table_named)
        .collect()
}

fn full_list_response() -> ListTablesResponse {
    ListTablesResponse::new(
        CATALOG,
        (1..=5)
            .map(|i| TableName::new(SCHEMA, format!("table{i}")))
            .collect(),
        None,
    )
}

#[derive(Default)]
struct FakeGlue {
    databases: Vec<Database>,
    database_next_token: Option<String>,
    database_calls: AtomicUsize,
    table: Mutex<Option<Table>>,
}

#[async_trait]
impl GlueClient for FakeGlue {
    async fn get_databases(
        &self,
        request: GetDatabasesInput,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> std::result::Result<GetDatabasesOutput, AwsServiceError> {
        assert_eq!(Some(ACCOUNT_ID), request.catalog_id());
        let first_page = self.database_calls.fetch_add(1, Ordering::SeqCst) == 0;
        Ok(GetDatabasesOutput::builder()
            .set_database_list(Some(self.databases.clone()))
            .set_next_token(if first_page {
                self.database_next_token.clone()
            } else {
                None
            })
            .build()
            .unwrap())
    }

    async fn get_tables(
        &self,
        request: GetTablesInput,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> std::result::Result<GetTablesOutput, AwsServiceError> {
        let next_token = request.next_token();
        let page_size = request.max_results().unwrap_or(UNLIMITED_PAGE_SIZE_VALUE);
        assert_eq!(Some(ACCOUNT_ID), request.catalog_id());
        assert_eq!(Some(SCHEMA), request.database_name());
        if page_size == UNLIMITED_PAGE_SIZE_VALUE {
            return Ok(GetTablesOutput::builder()
                .set_table_list(Some(un_paginated_tables()))
                .build());
        }
        let mut sorted = un_paginated_tables();
        sorted.sort_by(|a, b| a.name().cmp(b.name()));
        let page_size = usize::try_from(page_size).unwrap();
        let paginated: Vec<Table> = sorted
            .into_iter()
            .filter(|t| next_token.is_none_or(|token| t.name() >= token))
            .take(page_size + 1)
            .collect();
        if paginated.len() > page_size {
            Ok(GetTablesOutput::builder()
                .set_table_list(Some(paginated[..page_size].to_vec()))
                .next_token(paginated[page_size].name())
                .build())
        } else {
            Ok(GetTablesOutput::builder()
                .set_table_list(Some(paginated))
                .build())
        }
    }

    async fn get_table(
        &self,
        request: GetTableInput,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> std::result::Result<GetTableOutput, AwsServiceError> {
        assert_eq!(Some(ACCOUNT_ID), request.catalog_id());
        assert_eq!(Some(SCHEMA), request.database_name());
        assert_eq!(Some(TABLE), request.name());
        Ok(GetTableOutput::builder()
            .set_table(self.table.lock().unwrap().clone())
            .build())
    }
}

struct TestGlueHandler {
    context: MetadataContext,
    glue: Arc<dyn GlueClient>,
}

fn unsupported() -> FederationError {
    FederationError::UnsupportedOperation(String::new())
}

#[async_trait]
impl MetadataHandler for TestGlueHandler {
    fn context(&self) -> &MetadataContext {
        &self.context
    }

    async fn list_schema_names(&self, request: &ListSchemasRequest) -> Result<ListSchemasResponse> {
        self.do_list_schema_names_with_filter(request, None).await
    }

    async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse> {
        self.do_list_tables_with_filter(request, None).await
    }

    async fn get_table(&self, request: &GetTableRequest) -> Result<GetTableResponse> {
        self.do_get_table_with_filter(request, None).await
    }

    async fn get_partitions(
        &self,
        _block_writer: &mut SimpleBlockWriter<'_>,
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        Err(unsupported())
    }

    async fn get_splits(&self, _request: &GetSplitsRequest) -> Result<GetSplitsResponse> {
        Err(unsupported())
    }
}

impl GlueMetadataHandler for TestGlueHandler {
    fn aws_glue(&self) -> Option<&Arc<dyn GlueClient>> {
        Some(&self.glue)
    }
}

struct Setup {
    handler: TestGlueHandler,
    glue: Arc<FakeGlue>,
}

fn set_up_with(databases: Vec<Database>) -> Setup {
    set_up_with_glue(FakeGlue {
        databases,
        ..FakeGlue::default()
    })
}

fn set_up_with_glue(glue: FakeGlue) -> Setup {
    let glue = Arc::new(glue);
    let context = MetadataContext::builder("glue-test", Fakes::new().clients())
        .config_options(HashMap::new())
        .encryption_key_factory(Some(KeySource::Local))
        .spill_bucket(Some("spill-bucket".into()))
        .spill_prefix(Some("spill-prefix".into()))
        .build()
        .unwrap();
    let dyn_glue: Arc<dyn GlueClient> = glue.clone();
    Setup {
        handler: TestGlueHandler {
            context,
            glue: dyn_glue,
        },
        glue,
    }
}

fn set_up() -> Setup {
    set_up_with(Vec::new())
}

fn list_tables_request(next_token: Option<&str>, page_size: i32) -> ListTablesRequest {
    ListTablesRequest::new(
        Some(fake_identity()),
        Some(QUERY_ID.into()),
        CATALOG,
        Some(SCHEMA.into()),
        next_token.map(str::to_string),
        page_size,
    )
}

fn get_table_request() -> GetTableRequest {
    GetTableRequest::new(
        Some(fake_identity()),
        Some(QUERY_ID.into()),
        CATALOG,
        TableName::new(SCHEMA, TABLE),
        HashMap::new(),
    )
}

fn column(name: &str, glue_type: &str, comment: &str) -> Column {
    Column::builder()
        .name(name)
        .r#type(glue_type)
        .comment(comment)
        .build()
        .unwrap()
}

fn minor_type(response: &GetTableResponse, name: &str) -> MinorType {
    let field = response.schema().field_with_name(name).unwrap();
    MinorType::get_minor_type_for_arrow_type(field.data_type())
}

#[tokio::test]
async fn do_list_schema_names() {
    let databases = vec![
        Database::builder().name("db1").build().unwrap(),
        Database::builder().name("db2").build().unwrap(),
    ];
    let s = set_up_with(databases.clone());
    let req = ListSchemasRequest::new(Some(fake_identity()), Some(QUERY_ID.into()), CATALOG);
    let res = s.handler.list_schema_names(&req).await.unwrap();
    let expected: Vec<String> = databases.iter().map(|d| d.name().to_string()).collect();
    assert_eq!(expected, res.schemas());
}

#[tokio::test]
async fn do_list_schema_names_stops_on_empty_next_token() {
    let s = set_up_with_glue(FakeGlue {
        databases: vec![Database::builder().name("db1").build().unwrap()],
        database_next_token: Some(String::new()),
        ..FakeGlue::default()
    });
    let req = ListSchemasRequest::new(Some(fake_identity()), Some(QUERY_ID.into()), CATALOG);
    let res = s.handler.list_schema_names(&req).await.unwrap();
    assert_eq!(["db1".to_string()].as_slice(), res.schemas());
    assert_eq!(1, s.glue.database_calls.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_list_tables_with_unlimited_page_size() {
    let s = set_up();
    let req = list_tables_request(None, UNLIMITED_PAGE_SIZE_VALUE);
    let actual_response = s.handler.list_tables(&req).await.unwrap();
    assert_eq!(full_list_response(), actual_response, "Lists do not match.");
}

#[tokio::test]
async fn do_list_tables_with_large_page_size() {
    let s = set_up();
    let req = list_tables_request(None, GET_TABLES_REQUEST_MAX_RESULTS + 50);
    let actual_response = s.handler.list_tables(&req).await.unwrap();
    assert_eq!(full_list_response(), actual_response, "Lists do not match.");
}

#[tokio::test]
async fn do_list_tables_with_pagination() {
    let s = set_up();
    let req = list_tables_request(None, 3);
    let expected_response = ListTablesResponse::new(
        CATALOG,
        vec![
            TableName::new(SCHEMA, "table1"),
            TableName::new(SCHEMA, "table2"),
            TableName::new(SCHEMA, "table3"),
        ],
        Some("table4".into()),
    );
    let actual_response = s.handler.list_tables(&req).await.unwrap();
    assert_eq!(expected_response, actual_response, "Lists do not match.");

    let req = list_tables_request(actual_response.next_token(), 3);
    let expected_response = ListTablesResponse::new(
        CATALOG,
        vec![
            TableName::new(SCHEMA, "table4"),
            TableName::new(SCHEMA, "table5"),
        ],
        None,
    );
    let actual_response = s.handler.list_tables(&req).await.unwrap();
    assert_eq!(expected_response, actual_response, "Lists do not match.");
}

#[tokio::test]
async fn do_list_tables_orders_tables_like_java_hash_set() {
    let s = set_up();
    let req = list_tables_request(None, UNLIMITED_PAGE_SIZE_VALUE);
    let actual_response = s.handler.list_tables(&req).await.unwrap();
    let names: Vec<&str> = actual_response
        .tables()
        .iter()
        .map(TableName::table_name)
        .collect();
    assert_eq!(
        vec!["table4", "table3", "table5", "table2", "table1"],
        names
    );
}

#[tokio::test]
async fn do_get_table() {
    let s = set_up();
    let source_table = "My-Table";
    let expected_params = HashMap::from([
        (SOURCE_TABLE_PROPERTY.to_string(), source_table.to_string()),
        (
            COLUMN_NAME_MAPPING_PROPERTY.to_string(),
            "col2=Col2,col3=Col3, col4=Col4".to_string(),
        ),
        (
            DATETIME_FORMAT_MAPPING_PROPERTY.to_string(),
            "col2=someformat2, col1=someformat1 ".to_string(),
        ),
    ]);
    let columns = vec![
        column("col1", "int", "comment"),
        column("col2", "bigint", "comment"),
        column("col3", "string", "comment"),
        column("col4", "timestamp", "comment"),
        column("col5", "date", "comment"),
        column("col6", "timestamptz", "comment"),
        column("col7", "timestamptz", "comment"),
        column("partition_col1", "int", "comment"),
    ];
    let sd = StorageDescriptor::builder()
        .set_columns(Some(columns))
        .build();
    let return_table = Table::builder()
        .storage_descriptor(sd)
        .name(TABLE)
        .set_parameters(Some(expected_params))
        .build()
        .unwrap();
    *s.glue.table.lock().unwrap() = Some(return_table);

    let res = s.handler.get_table(&get_table_request()).await.unwrap();

    let schema = res.schema();
    assert_eq!(8, schema.fields().len());
    assert!(!schema.metadata().is_empty());
    assert!(schema
        .metadata()
        .contains_key(DATETIME_FORMAT_MAPPING_PROPERTY));
    assert_eq!(
        Some("Col2=someformat2,col1=someformat1"),
        schema
            .metadata()
            .get(DATETIME_FORMAT_MAPPING_PROPERTY_NORMALIZED)
            .map(String::as_str)
    );
    assert_eq!(Some(source_table), get_source_table_name(schema));

    for name in [
        "partition_col1",
        "col1",
        "Col2",
        "Col3",
        "Col4",
        "col5",
        "col6",
        "col7",
    ] {
        assert!(schema.field_with_name(name).is_ok(), "{name}");
    }

    assert_eq!(MinorType::INT, minor_type(&res, "partition_col1"));
    assert_eq!(MinorType::INT, minor_type(&res, "col1"));
    assert_eq!(MinorType::BIGINT, minor_type(&res, "Col2"));
    assert_eq!(MinorType::VARCHAR, minor_type(&res, "Col3"));
    assert_eq!(MinorType::DATEMILLI, minor_type(&res, "Col4"));
    assert_eq!(MinorType::DATEDAY, minor_type(&res, "col5"));
    assert_eq!(MinorType::TIMESTAMPMILLITZ, minor_type(&res, "col6"));
    assert_eq!(MinorType::TIMESTAMPMILLITZ, minor_type(&res, "col7"));
}

#[test]
fn populate_source_table_from_location() {
    for partition in ["aws", "aws-cn", "aws-us-gov"] {
        let storage_descriptor = StorageDescriptor::builder()
            .location(format!(
                "arn:{partition}:dynamodb:us-east-1:012345678910:table/My-Table"
            ))
            .build();
        let table = Table::builder()
            .name(TABLE)
            .set_parameters(Some(HashMap::new()))
            .storage_descriptor(storage_descriptor)
            .build()
            .unwrap();
        let mut schema_builder = SchemaBuilder::new_builder();
        populate_source_table_name_if_available(&table, &mut schema_builder);
        let schema = schema_builder.build();
        assert_eq!(Some("My-Table"), get_source_table_name(&schema));
    }
}

#[test]
fn source_table_arn_dot_excludes_java_line_terminators_only() {
    let cases = [
        ("t\u{e9}", Some("t\u{e9}")),
        ("a\tb", Some("a\tb")),
        ("a\rb", None),
        ("a\u{85}b", None),
        ("a\u{2028}b", None),
        ("a\u{2029}b", None),
        ("a\nb", None),
        ("x\n", None),
    ];
    for (suffix, expected) in cases {
        let storage_descriptor = StorageDescriptor::builder()
            .location(format!(
                "arn:aws:glue:us-east-1:123456789012:table/{suffix}"
            ))
            .build();
        let table = Table::builder()
            .name(TABLE)
            .set_parameters(Some(HashMap::new()))
            .storage_descriptor(storage_descriptor)
            .build()
            .unwrap();
        let mut schema_builder = SchemaBuilder::new_builder();
        populate_source_table_name_if_available(&table, &mut schema_builder);
        let schema = schema_builder.build();
        assert_eq!(expected, get_source_table_name(&schema), "{suffix:?}");
    }
}

#[test]
fn function_arn_owner_digits_are_ascii_only() {
    let s = set_up();
    let mut req = get_table_request();
    req.set_context(Some(LambdaContext::default().with_invoked_function_arn(
        Some(
            "arn:aws:lambda:us-east-1:\u{661}\u{662}\u{663}\u{664}\u{665}\u{666}\u{667}\u{668}\u{669}\u{660}\u{661}\u{662}:function:fn"
                .into(),
        ),
    )));
    let catalog = s.handler.get_catalog(req.header()).unwrap();
    assert_eq!(Some(ACCOUNT_ID.to_string()), catalog);
}

#[tokio::test]
async fn do_get_table_empty_comment() {
    let s = set_up();
    let expected_params = HashMap::from([
        (SOURCE_TABLE_PROPERTY.to_string(), "My-Table".to_string()),
        ("col1".to_string(), "col1".to_string()),
    ]);
    let sd = StorageDescriptor::builder()
        .set_columns(Some(vec![column("col1", "int", " ")]))
        .build();
    let result_table = Table::builder()
        .storage_descriptor(sd)
        .name(TABLE)
        .set_parameters(Some(expected_params))
        .build()
        .unwrap();
    *s.glue.table.lock().unwrap() = Some(result_table);

    let res = s.handler.get_table(&get_table_request()).await.unwrap();

    assert!(res.schema().field_with_name("col1").is_ok());
    assert_eq!(MinorType::INT, minor_type(&res, "col1"));
}

#[test]
fn test_get_catalog() {
    let s = set_up();
    let mut req = get_table_request();
    let catalog = s.handler.get_catalog(req.header()).unwrap();
    assert_eq!(Some(ACCOUNT_ID.to_string()), catalog);

    req.set_context(Some(LambdaContext::default().with_invoked_function_arn(
        Some("arn:aws:lambda:us-east-1:012345678912:function:athena-123".into()),
    )));
    let catalog = s.handler.get_catalog(req.header()).unwrap();
    assert_eq!(Some("012345678912".to_string()), catalog);

    req.set_context(Some(LambdaContext::default().with_invoked_function_arn(
        Some("arn:aws:lambda:us-east-1:012345678912:function:".into()),
    )));
    let catalog = s.handler.get_catalog(req.header()).unwrap();
    assert_eq!(Some(ACCOUNT_ID.to_string()), catalog);

    req.set_context(Some(
        LambdaContext::default().with_invoked_function_arn(None),
    ));
    let catalog = s.handler.get_catalog(req.header()).unwrap();
    assert_eq!(Some(ACCOUNT_ID.to_string()), catalog);
}
