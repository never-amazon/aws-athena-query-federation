#[path = "../../athena-jdbc/tests/jdbc_support/mod.rs"]
mod jdbc_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use athena_federation_sdk::data::{Block, BlockAllocator, BlockAllocatorImpl};
use athena_federation_sdk::domain::predicate::{
    Constraints, Marker, Range, SortedRangeSet, ValueSet,
};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{MetadataHandler, RecordHandler};
use athena_federation_sdk::metadata::{GetTableRequest, ListSchemasRequest, ListTablesRequest};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::FederatedIdentity;
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcDriver};
use athena_jdbc::sql::JdbcConnection;
use athena_postgresql::{PgDriver, PostGreSqlMetadataHandler, PostGreSqlRecordHandler};
use chrono::NaiveDate;
use indexmap::IndexMap;
use jdbc_support::Fakes;

const URL_VARIABLE: &str = "ATHENA_POSTGRESQL_IT_JDBC_URL";
const CATALOG: &str = "default";
const MOVIES: &str = "movies";
const BDAY: &str = "bday";

struct Database {
    url: String,
    schema: String,
}

fn database(test: &str) -> Option<Database> {
    let Ok(url) = std::env::var(URL_VARIABLE) else {
        println!("skipping {test}: set {URL_VARIABLE} to a jdbc:postgresql:// URL to run it");
        return None;
    };
    Some(Database {
        url,
        schema: format!("u8_integ_{test}_{}", std::process::id()),
    })
}

async fn run(connection: &dyn JdbcConnection, sql: &str) {
    let statement = connection.prepare_statement(sql).await.unwrap();
    connection.execute(&statement).await.unwrap();
}

impl Database {
    async fn set_up(&self) {
        let connection = PgDriver.connect(&self.url, &HashMap::new()).await.unwrap();
        let schema = &self.schema;
        run(
            connection.as_ref(),
            &format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
        )
        .await;
        run(connection.as_ref(), &format!("CREATE SCHEMA {schema}")).await;
        run(
            connection.as_ref(),
            &format!("CREATE TABLE {schema}.{MOVIES} (year int, title varchar, director varchar, actors varchar[])"),
        )
        .await;
        run(
            connection.as_ref(),
            &format!("INSERT INTO {schema}.{MOVIES} VALUES (2014, 'Interstellar', 'Christopher Nolan', '{{\"Matthew McConaughey\", \"John Lithgow\", \"Ann Hathaway\", \"David Gyasi\", \"Michael Caine\", \"Jessica Chastain\", \"Matt Damon\", \"Casey Affleck\"}}'), (1986, 'Aliens', 'James Cameron', '{{\"Sigourney Weaver\", \"Paul Reiser\", \"Lance Henriksen\", \"Bill Paxton\"}}')"),
        )
        .await;
        run(
            connection.as_ref(),
            &format!("CREATE TABLE {schema}.{BDAY} (first_name varchar, last_name varchar, birthday date)"),
        )
        .await;
        run(
            connection.as_ref(),
            &format!("INSERT INTO {schema}.{BDAY} VALUES ('Joe', 'Schmoe', date('2002-05-05')), ('Jane', 'Doe', date('2005-10-12')), ('John', 'Smith', date('2006-02-10'))"),
        )
        .await;
        connection.close().await.unwrap();
    }

    async fn tear_down(&self) {
        let connection = PgDriver.connect(&self.url, &HashMap::new()).await.unwrap();
        run(
            connection.as_ref(),
            &format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema),
        )
        .await;
        connection.close().await.unwrap();
    }

    fn config(&self) -> (DatabaseConnectionConfig, HashMap<String, String>) {
        let config = DatabaseConnectionConfig::new(CATALOG, "postgres", &self.url).unwrap();
        (config, HashMap::new())
    }

    fn metadata_handler(&self) -> PostGreSqlMetadataHandler {
        let (config, options) = self.config();
        PostGreSqlMetadataHandler::new(Fakes::new().clients(), config, options).unwrap()
    }

    fn record_handler(&self) -> PostGreSqlRecordHandler {
        let (config, options) = self.config();
        PostGreSqlRecordHandler::new(Fakes::new().clients(), config, options).unwrap()
    }

    async fn select(
        &self,
        table: &str,
        schema: Schema,
        summary: IndexMap<String, ValueSet>,
        column: &str,
    ) -> Vec<String> {
        let split = Split::new_builder(
            Some(SpillLocation::S3(S3SpillLocation::new(
                "integ-bucket",
                "integ-prefix/integ-query/split",
                true,
            ))),
            None,
        )
        .add(
            PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
            "*",
        )
        .add(PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME, "*")
        .build();
        let request = ReadRecordsRequest::new(
            Some(identity()),
            CATALOG,
            "integ-query",
            TableName::new(&self.schema, table),
            schema,
            split,
            Constraints::new(summary, Vec::new(), Vec::new(), -1, HashMap::new(), None),
            100_000_000,
            100_000_000,
        );
        let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
        let response = self
            .record_handler()
            .do_read_records(&allocator, &request)
            .await
            .unwrap();
        let FederationResponse::ReadRecords(response) = response else {
            panic!("expected an inline ReadRecordsResponse, got {response:?}");
        };
        column_values(response.records(), column)
    }
}

fn column_values(block: &Block, column: &str) -> Vec<String> {
    let reader = block.get_field_reader(column).unwrap();
    (0..block.row_count())
        .filter_map(|row| {
            reader.set_position(row);
            reader.read_text()
        })
        .collect()
}

fn identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

fn days(text: &str) -> i32 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap();
    i32::try_from((date - epoch).num_days()).unwrap()
}

#[tokio::test]
async fn list_databases_integ_test() {
    let Some(db) = database("list_databases") else {
        return;
    };
    db.set_up().await;
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let response = db
        .metadata_handler()
        .do_list_schema_names(
            &allocator,
            &ListSchemasRequest::new(Some(identity()), Some("integ-query".to_string()), CATALOG),
        )
        .await
        .unwrap();
    db.tear_down().await;
    assert!(
        response.schemas().contains(&db.schema),
        "DB not found. {:?}",
        response.schemas()
    );
}

#[tokio::test]
async fn list_tables_integ_test() {
    let Some(db) = database("list_tables") else {
        return;
    };
    db.set_up().await;
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let response = db
        .metadata_handler()
        .do_list_tables(
            &allocator,
            &ListTablesRequest::new(
                Some(identity()),
                Some("integ-query".to_string()),
                CATALOG,
                Some(db.schema.clone()),
                None,
                -1,
            ),
        )
        .await
        .unwrap();
    db.tear_down().await;
    let names: Vec<&str> = response
        .tables()
        .iter()
        .map(TableName::table_name)
        .collect();
    assert_eq!(2, names.len(), "Incorrect number of tables found.");
    assert!(names.contains(&MOVIES), "Table not found: {MOVIES}.");
    assert!(names.contains(&BDAY), "Table not found: {BDAY}.");
}

#[tokio::test]
async fn list_table_schema_integ_test() {
    let Some(db) = database("list_table_schema") else {
        return;
    };
    db.set_up().await;
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let response = db
        .metadata_handler()
        .do_get_table(
            &allocator,
            &GetTableRequest::new(
                Some(identity()),
                Some("integ-query".to_string()),
                CATALOG,
                TableName::new(&db.schema, MOVIES),
                HashMap::new(),
            ),
        )
        .await
        .unwrap();
    db.tear_down().await;
    let fields: HashMap<&str, &DataType> = response
        .schema()
        .fields()
        .iter()
        .map(|f| (f.name().as_str(), f.data_type()))
        .filter(|(name, _)| *name != "partition_name" && *name != "partition_schema_name")
        .collect();
    assert_eq!(4, fields.len(), "Wrong number of columns found.");
    assert_eq!(Some(&&DataType::Int32), fields.get("year"));
    assert_eq!(Some(&&DataType::Utf8), fields.get("title"));
    assert_eq!(Some(&&DataType::Utf8), fields.get("director"));
    let Some(DataType::List(element)) = fields.get("actors").copied() else {
        panic!("Wrong column type for actors. {:?}", fields.get("actors"));
    };
    assert_eq!(&DataType::Utf8, element.data_type());
}

#[tokio::test]
async fn select_column_with_predicate_integ_test() {
    let Some(db) = database("select_predicate") else {
        return;
    };
    db.set_up().await;
    let allocator = BlockAllocatorImpl::new();
    let year_after_2010 = ValueSet::from(
        SortedRangeSet::of_ranges(
            Range::greater_than(&allocator, &DataType::Int32, 2010).unwrap(),
            Vec::new(),
        )
        .unwrap(),
    );
    let schema = Schema::new(vec![
        Field::new("title", DataType::Utf8, true),
        Field::new("year", DataType::Int32, true),
    ]);
    let titles = db
        .select(
            MOVIES,
            schema,
            IndexMap::from([("year".to_string(), year_after_2010)]),
            "title",
        )
        .await;
    db.tear_down().await;
    assert_eq!(1, titles.len(), "Wrong number of DB records found.");
    assert!(titles.contains(&"Interstellar".to_string()));
}

#[tokio::test]
async fn select_column_between_dates_integ_test() {
    let Some(db) = database("select_dates") else {
        return;
    };
    db.set_up().await;
    let allocator = BlockAllocatorImpl::new();
    let october_2005 = ValueSet::from(
        SortedRangeSet::of_ranges(
            Range::new(
                Marker::exactly(&allocator, &DataType::Date32, days("2005-10-01")).unwrap(),
                Marker::exactly(&allocator, &DataType::Date32, days("2005-10-31")).unwrap(),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap(),
    );
    let schema = Schema::new(vec![
        Field::new("first_name", DataType::Utf8, true),
        Field::new("birthday", DataType::Date32, true),
    ]);
    let names = db
        .select(
            BDAY,
            schema,
            IndexMap::from([("birthday".to_string(), october_2005)]),
            "first_name",
        )
        .await;
    db.tear_down().await;
    assert_eq!(1, names.len(), "Wrong number of DB records found.");
    assert!(names.contains(&"Jane".to_string()));
}
