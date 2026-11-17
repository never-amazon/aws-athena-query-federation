mod serde_support;

use std::collections::HashMap;

use arrow::datatypes::DataType;
use athena_federation_sdk::data::SchemaBuilder;
use athena_federation_sdk::domain::predicate::{Constraints, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::metadata::optimizations::querypassthrough::QueryPassthroughSignature;
use athena_federation_sdk::metadata::GetTableRequest;
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::{FederatedIdentity, FederationRequest};
use athena_federation_sdk::FederationError;
use indexmap::IndexMap;
use serde_support::*;

const DIR: &str = "serde/null_map_values";

fn nullable_map(entries: &[(&str, Option<&str>)]) -> HashMap<String, Option<String>> {
    entries.iter().map(|(k, v)| (k.to_string(), v.map(|s| s.to_string()))).collect()
}

fn null_tags() -> HashMap<String, Option<String>> {
    nullable_map(&[("dept", None), ("team", Some("athena"))])
}

fn null_config_options() -> HashMap<String, Option<String>> {
    nullable_map(&[("region", None), ("stage", Some("beta"))])
}

fn null_query_passthrough_arguments() -> HashMap<String, Option<String>> {
    nullable_map(&[("schema", None), ("query", Some("SELECT 1"))])
}

fn null_identity() -> FederatedIdentity {
    FederatedIdentity::new("testArn", "0123456789", null_tags(), Vec::new(), null_config_options())
}

fn get_table_request() -> GetTableRequest {
    GetTableRequest::new(
        Some(null_identity()),
        Some("test-query-id".to_string()),
        "test-catalog",
        table_name(),
        null_query_passthrough_arguments(),
    )
}

fn read_records_request() -> ReadRecordsRequest {
    let schema = SchemaBuilder::new_builder().add_field_with_type("year", DataType::Int32).build();
    let split = Split::new_builder(Some(spill_location("test-split-id")), Some(encryption_key("test-key", "test-nonce")))
        .add("year", "2017")
        .build();
    let constraints = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        null_query_passthrough_arguments(),
        None,
    );
    ReadRecordsRequest::new(
        Some(federated_identity()),
        "test-catalog",
        "test-query-id",
        table_name(),
        schema,
        split,
        constraints,
        100_000_000_000,
        100_000_000_000,
    )
}

fn assert_get_table_request_round_trip(version: i32, expected_query_passthrough_arguments: HashMap<String, Option<String>>) {
    let t = TypedSerDe::before();
    let mapper = indented(t.allocator.clone(), version);
    let text = fixture(DIR, &format!("GetTableRequest-v{}.json", version));

    let actual = write_request(&mapper, get_table_request());
    assert_serialized_matches(&text, &actual, &mapper);

    let parsed: FederationRequest = mapper.read_value(text.as_bytes()).expect("deserialize");
    let request = match parsed {
        FederationRequest::GetTable(r) => r,
        other => panic!("expected GetTableRequest, got {:?}", other),
    };
    let identity = request.get_identity().expect("identity");
    assert_eq!(&null_tags(), identity.get_principal_tags());
    assert_eq!(&null_config_options(), identity.get_config_options());
    assert_eq!(&expected_query_passthrough_arguments, request.get_query_passthrough_arguments());

    let rewritten = write_request(&mapper, request);
    assert_serialized_matches(&text, &rewritten, &mapper);
}

fn assert_read_records_request_round_trip(version: i32) {
    let t = TypedSerDe::before();
    let mapper = indented(t.allocator.clone(), version);
    let text = fixture(DIR, &format!("ReadRecordsRequest-v{}.json", version));

    let actual = write_request(&mapper, read_records_request());
    assert_serialized_matches(&text, &actual, &mapper);

    let parsed: FederationRequest = mapper.read_value(text.as_bytes()).expect("deserialize");
    assert_eq!(FederationRequest::from(read_records_request()), parsed);
    let request = match parsed {
        FederationRequest::ReadRecords(r) => r,
        other => panic!("expected ReadRecordsRequest, got {:?}", other),
    };
    assert_eq!(&null_query_passthrough_arguments(), request.get_constraints().get_query_passthrough_arguments());

    let rewritten = write_request(&mapper, request);
    assert_serialized_matches(&text, &rewritten, &mapper);
}

fn assert_split_null_property_rejected(version: i32) {
    let t = TypedSerDe::before();
    let mapper = indented(t.allocator.clone(), version);
    let text = fixture(DIR, &format!("ReadRecordsRequest-v{}.json", version)).replace("\"year\" : \"2017\"", "\"year\" : null");
    let result = mapper.read_value::<FederationRequest>(text.as_bytes());
    match result {
        Err(FederationError::NullPointer(message)) => assert_eq!("null value in entry: year=null", message),
        other => panic!("expected NullPointer, got {:?}", other),
    }
}

#[test]
fn get_table_request_null_map_values_v1() {
    assert_get_table_request_round_trip(1, null_query_passthrough_arguments());
}

#[test]
fn get_table_request_null_map_values_v2() {
    assert_get_table_request_round_trip(2, HashMap::new());
}

#[test]
fn get_table_request_null_map_values_v3() {
    assert_get_table_request_round_trip(3, HashMap::new());
}

#[test]
fn get_table_request_null_map_values_v4() {
    assert_get_table_request_round_trip(4, HashMap::new());
}

#[test]
fn get_table_request_null_map_values_v5() {
    assert_get_table_request_round_trip(5, null_query_passthrough_arguments());
}

#[test]
fn get_table_request_null_map_values_v6() {
    assert_get_table_request_round_trip(6, null_query_passthrough_arguments());
}

#[test]
fn read_records_request_null_query_passthrough_argument_v1() {
    assert_read_records_request_round_trip(1);
}

#[test]
fn read_records_request_null_query_passthrough_argument_v5() {
    assert_read_records_request_round_trip(5);
}

#[test]
fn read_records_request_null_query_passthrough_argument_v6() {
    assert_read_records_request_round_trip(6);
}

#[test]
fn split_null_property_rejected_v5() {
    assert_split_null_property_rejected(5);
}

#[test]
fn split_null_property_rejected_v6() {
    assert_split_null_property_rejected(6);
}

struct SystemQuerySignature;

impl QueryPassthroughSignature for SystemQuerySignature {
    fn get_function_schema(&self) -> String {
        "system".to_string()
    }

    fn get_function_name(&self) -> String {
        "query".to_string()
    }

    fn get_function_arguments(&self) -> Vec<String> {
        vec!["QUERY".to_string()]
    }
}

#[test]
fn verify_rejects_null_function_signature() {
    let args = nullable_map(&[("schemaFunctionName", None), ("QUERY", Some("SELECT 1"))]);
    match SystemQuerySignature.verify(&args) {
        Err(FederationError::NullPointer(message)) => assert_eq!(
            "Cannot invoke \"String.equalsIgnoreCase(String)\" because \"receivedSignature\" is null",
            message
        ),
        other => panic!("expected NullPointer, got {:?}", other),
    }
}

#[test]
fn verify_rejects_null_argument_value() {
    let args = nullable_map(&[("schemaFunctionName", Some("SYSTEM.QUERY")), ("QUERY", None)]);
    let err = SystemQuerySignature.verify(&args).expect_err("null argument");
    assert!(err.as_connector_exception().is_some(), "{:?}", err);
    assert_eq!("Missing Query Passthrough Value for Argument: QUERY", err.to_string());
}

fn null_property_split() -> Split {
    let mut properties = HashMap::new();
    properties.insert("year".to_string(), None);
    Split::new_nullable(Some(spill_location("test-split-id")), Some(encryption_key("test-key", "test-nonce")), properties)
}

fn read_records_request_with_null_split_property() -> ReadRecordsRequest {
    let schema = SchemaBuilder::new_builder().add_field_with_type("year", DataType::Int32).build();
    let constraints = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        null_query_passthrough_arguments(),
        None,
    );
    ReadRecordsRequest::new(
        Some(federated_identity()),
        "test-catalog",
        "test-query-id",
        table_name(),
        schema,
        null_property_split(),
        constraints,
        100_000_000_000,
        100_000_000_000,
    )
}

#[test]
fn split_null_property_kept_v1() {
    let t = TypedSerDe::before();
    let mapper = indented(t.allocator.clone(), 1);
    let text = fixture(DIR, "ReadRecordsRequest-null-split-property-v1.json");

    let actual = write_request(&mapper, read_records_request_with_null_split_property());
    assert_serialized_matches(&text, &actual, &mapper);

    let parsed: FederationRequest = mapper.read_value(text.as_bytes()).expect("deserialize");
    assert_eq!(FederationRequest::from(read_records_request_with_null_split_property()), parsed);
    let request = match parsed {
        FederationRequest::ReadRecords(r) => r,
        other => panic!("expected ReadRecordsRequest, got {:?}", other),
    };
    let split = request.get_split();
    assert_eq!(Some(&None), split.get_properties().get("year"));
    assert_eq!(None, split.get_property("year"));
    assert!(split.to_string().ends_with("properties={year=null}}"), "{}", split);

    let rewritten = write_request(&mapper, request);
    assert_serialized_matches(&text, &rewritten, &mapper);
}

fn assert_split_null_property_written_then_rejected(version: i32) {
    let t = TypedSerDe::before();
    let mapper = indented(t.allocator.clone(), version);
    let text = fixture(DIR, &format!("ReadRecordsRequest-null-split-property-v{}.json", version));

    let actual = write_request(&mapper, read_records_request_with_null_split_property());
    assert_serialized_matches(&text, &actual, &mapper);

    match mapper.read_value::<FederationRequest>(text.as_bytes()) {
        Err(FederationError::NullPointer(message)) => assert_eq!("null value in entry: year=null", message),
        other => panic!("expected NullPointer, got {:?}", other),
    }
}

#[test]
fn split_null_property_rejected_v2() {
    assert_split_null_property_written_then_rejected(2);
}

#[test]
fn split_null_property_rejected_v3() {
    assert_split_null_property_written_then_rejected(3);
}

#[test]
fn split_null_property_rejected_v4() {
    assert_split_null_property_written_then_rejected(4);
}

#[test]
fn split_null_property_written_then_rejected_v5() {
    assert_split_null_property_written_then_rejected(5);
}

#[test]
fn split_null_property_written_then_rejected_v6() {
    assert_split_null_property_written_then_rejected(6);
}

fn null_group_request() -> GetTableRequest {
    let identity = FederatedIdentity::new(
        "testArn",
        "0123456789",
        HashMap::new(),
        vec![Some("admins".to_string()), None, Some("users".to_string())],
        HashMap::new(),
    );
    GetTableRequest::new(Some(identity), Some("test-query-id".to_string()), "test-catalog", table_name(), HashMap::new())
}

fn assert_null_group_round_trip(version: i32) {
    let t = TypedSerDe::before();
    let mapper = indented(t.allocator.clone(), version);
    let text = fixture(DIR, &format!("GetTableRequest-null-group-v{}.json", version));

    let actual = write_request(&mapper, null_group_request());
    assert_serialized_matches(&text, &actual, &mapper);

    let parsed: FederationRequest = mapper.read_value(text.as_bytes()).expect("deserialize");
    let request = match parsed {
        FederationRequest::GetTable(r) => r,
        other => panic!("expected GetTableRequest, got {:?}", other),
    };
    assert_eq!(
        &[Some("admins".to_string()), None, Some("users".to_string())],
        request.get_identity().expect("identity").get_iam_groups()
    );

    let rewritten = write_request(&mapper, request);
    assert_serialized_matches(&text, &rewritten, &mapper);
}

#[test]
fn federated_identity_null_group_kept_v1() {
    assert_null_group_round_trip(1);
}

#[test]
fn federated_identity_null_group_kept_v2() {
    assert_null_group_round_trip(2);
}

#[test]
fn federated_identity_null_group_kept_v3() {
    assert_null_group_round_trip(3);
}

#[test]
fn federated_identity_null_group_kept_v4() {
    assert_null_group_round_trip(4);
}

#[test]
fn federated_identity_null_group_kept_v5() {
    assert_null_group_round_trip(5);
}

#[test]
fn federated_identity_null_group_kept_v6() {
    assert_null_group_round_trip(6);
}
