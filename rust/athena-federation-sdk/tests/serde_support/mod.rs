#![allow(
    dead_code,
    reason = "shared test helpers; each test binary uses a subset"
)]

use athena_federation_sdk::data::block_utils;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, SchemaBuilder, Value,
};
use athena_federation_sdk::domain::predicate::expression::{
    ConstantExpression, FederationExpression, FunctionCallExpression, VariableExpression,
};
use athena_federation_sdk::domain::predicate::functions::StandardFunctions;
use athena_federation_sdk::domain::predicate::{
    AllOrNoneValueSet, Constraints, Direction, EquatableValueSet, OrderByField, QueryPlan, Range,
    SortedRangeSet, ValueSet, DEFAULT_NO_LIMIT,
};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::{FederatedIdentity, FederationRequest, FederationResponse};
use athena_federation_sdk::security::EncryptionKey;
use athena_federation_sdk::serde::java_hash::java_hash_order_default;
use athena_federation_sdk::serde::{
    block_deserializer, schema_from_bytes, ObjectMapper, SerDeContext, SerdeVersion,
};
use indexmap::IndexMap;
use serde_json::Value as Json;

pub const TEST_ALLOCATOR_ID: &str = "test-allocator-id";

pub struct TypedSerDe {
    pub allocator: Arc<BlockAllocatorImpl>,
    pub mapper: ObjectMapper,
    pub mapper_v4: ObjectMapper,
    pub mapper_v5: ObjectMapper,
    pub mapper_v6: ObjectMapper,
    pub federated_identity: FederatedIdentity,
}

impl TypedSerDe {
    pub fn before() -> TypedSerDe {
        let allocator = Arc::new(BlockAllocatorImpl::with_id(TEST_ALLOCATOR_ID));
        TypedSerDe {
            mapper: indented(allocator.clone(), SerdeVersion::V2),
            mapper_v4: indented(allocator.clone(), SerdeVersion::V4),
            mapper_v5: indented(allocator.clone(), SerdeVersion::V5),
            mapper_v6: indented(allocator.clone(), SerdeVersion::V6),
            allocator,
            federated_identity: federated_identity(),
        }
    }

    pub fn alloc(&self) -> &dyn BlockAllocator {
        self.allocator.as_ref()
    }
}

pub fn indented(allocator: Arc<BlockAllocatorImpl>, version: SerdeVersion) -> ObjectMapper {
    ObjectMapper::new(version, allocator).indented()
}

pub fn federated_identity() -> FederatedIdentity {
    FederatedIdentity::new(
        "testArn",
        "0123456789",
        HashMap::new(),
        Vec::new(),
        HashMap::new(),
    )
}

pub fn fixture(dir: &str, name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures");
    path.push(dir);
    path.push(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {}", path.display(), e))
        .trim()
        .to_string()
}

pub fn java_hash_map<V>(entries: Vec<(&str, V)>) -> IndexMap<String, V> {
    let keys: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
    let order: Vec<String> = java_hash_order_default(keys.iter().map(std::string::String::as_str))
        .into_iter()
        .map(std::string::ToString::to_string)
        .collect();
    let mut by_key: HashMap<String, V> = entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    let mut result = IndexMap::new();
    for key in order {
        if let Some(v) = by_key.remove(&key) {
            result.insert(key, v);
        }
    }
    result
}

pub fn string_map(entries: &[(&str, &str)]) -> HashMap<String, Option<String>> {
    entries
        .iter()
        .map(|(k, v)| (k.to_string(), Some(v.to_string())))
        .collect()
}

pub fn set(block: &mut Block, column: &str, row: usize, value: impl Into<Value>) {
    let vector = block.get_field_vector_mut(column).expect("column");
    block_utils::set_value(vector, row, &value.into()).expect("set value");
}

pub fn partitions_schema() -> Schema {
    SchemaBuilder::new_builder()
        .add_field_with_type("year", DataType::Int32)
        .add_field_with_type("month", DataType::Int32)
        .add_field_with_type("day", DataType::Int32)
        .add_field_with_type("col2", DataType::Utf8)
        .add_field_with_type("col3", DataType::Float64)
        .add_field_with_type("col4", DataType::Float64)
        .add_field_with_type("col5", DataType::Float64)
        .build()
}

pub fn fill_partitions(block: &mut Block, rows: usize) {
    for (i, n) in (0..rows).zip(0i32..) {
        set(block, "year", i, 2016 + n);
        set(block, "month", i, (n % 12) + 1);
        set(block, "day", i, (n % 28) + 1);
    }
    block.set_row_count(rows);
}

pub fn summary(allocator: &dyn BlockAllocator) -> IndexMap<String, ValueSet> {
    let float8 = DataType::Float64;
    java_hash_map(vec![
        (
            "col3",
            SortedRangeSet::copy_of(
                &float8,
                vec![Range::greater_than(allocator, &float8, -10000f64).expect("range")],
                false,
            )
            .expect("sorted range set")
            .into(),
        ),
        (
            "col4",
            EquatableValueSet::new_builder(allocator, &float8, false, true)
                .add(1.1f64)
                .build()
                .expect("equatable value set")
                .into(),
        ),
        (
            "col5",
            AllOrNoneValueSet::new(float8.clone(), false, true).into(),
        ),
    ])
}

pub fn simple_constraints(allocator: &dyn BlockAllocator) -> Constraints {
    Constraints::new(
        summary(allocator),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        None,
    )
}

pub fn spill_location(split_id: &str) -> SpillLocation {
    SpillLocation::S3(
        S3SpillLocation::new_builder()
            .with_bucket("athena-virtuoso-test")
            .with_prefix("lambda-spill")
            .with_query_id("test-query-id")
            .with_split_id(split_id)
            .build(),
    )
}

pub fn encryption_key(key: &str, nonce: &str) -> EncryptionKey {
    EncryptionKey::new(key.as_bytes().to_vec(), nonce.as_bytes().to_vec())
}

pub fn read_records_split() -> Split {
    Split::new_builder(
        Some(spill_location("test-split-id")),
        Some(encryption_key("test-key", "test-nonce")),
    )
    .add("year", "2017")
    .add("month", "11")
    .add("day", "1")
    .build()
}

pub fn federation_expression(allocator: &dyn BlockAllocator) -> FederationExpression {
    let constant_block =
        block_utils::new_block(allocator, "col1", &DataType::Int32, vec![10i32]).expect("block");
    FunctionCallExpression::new(
        DataType::Boolean,
        StandardFunctions::GreaterThan.function_name(),
        vec![
            FunctionCallExpression::new(
                DataType::Float64,
                StandardFunctions::Add.function_name(),
                vec![
                    VariableExpression::new("col3", DataType::Float64)
                        .expect("variable")
                        .into(),
                    ConstantExpression::new(Some(constant_block), DataType::Int32).into(),
                ],
            )
            .into(),
            VariableExpression::new("col2", DataType::Float64)
                .expect("variable")
                .into(),
        ],
    )
    .into()
}

pub fn order_by_clause() -> Vec<OrderByField> {
    vec![
        OrderByField::new("col3", Direction::ASC_NULLS_FIRST),
        OrderByField::new("col2", Direction::DESC_NULLS_FIRST),
    ]
}

pub fn read_records_request(
    allocator: &dyn BlockAllocator,
    identity: FederatedIdentity,
    query_passthrough_arguments: HashMap<String, Option<String>>,
    query_plan: Option<QueryPlan>,
) -> ReadRecordsRequest {
    let schema = partitions_schema();
    let mut partitions = allocator.create_block(Arc::new(schema.clone()));
    fill_partitions(&mut partitions, 10);
    let constraints = Constraints::new(
        summary(allocator),
        vec![federation_expression(allocator)],
        order_by_clause(),
        DEFAULT_NO_LIMIT,
        query_passthrough_arguments,
        query_plan,
    );
    ReadRecordsRequest::new(
        Some(identity),
        "test-query-id",
        "test-catalog",
        TableName::new("test-schema", "test-table"),
        schema,
        read_records_split(),
        constraints,
        100_000_000_000,
        100_000_000_000,
    )
}

pub fn table_name() -> TableName {
    TableName::new("test-schema", "test-table")
}

pub fn write_request(mapper: &ObjectMapper, request: impl Into<FederationRequest>) -> String {
    let request: FederationRequest = request.into();
    mapper.write_request(&request).expect("serialize")
}

pub fn write_response(mapper: &ObjectMapper, response: impl Into<FederationResponse>) -> String {
    let response: FederationResponse = response.into();
    mapper.write_response(&response).expect("serialize")
}

pub fn assert_serialized_matches(expected_text: &str, actual_text: &str, mapper: &ObjectMapper) {
    let expected: Json = serde_json::from_str(expected_text).expect("fixture is JSON");
    let actual: Json = serde_json::from_str(actual_text).expect("output is JSON");
    let mut replacements: Vec<(String, String)> = Vec::new();
    compare(&expected, &actual, "$", mapper, &mut replacements);
    let mut normalized = actual_text.to_string();
    for (from, to) in replacements {
        normalized = normalized.replace(&format!("\"{from}\""), &format!("\"{to}\""));
    }
    assert_eq!(
        expected_text, normalized,
        "serialized text differs from the fixture"
    );
}

fn is_block(map: &serde_json::Map<String, Json>) -> bool {
    map.contains_key("aId") && map.contains_key("schema") && map.contains_key("records")
}

fn compare(
    expected: &Json,
    actual: &Json,
    path: &str,
    mapper: &ObjectMapper,
    replacements: &mut Vec<(String, String)>,
) {
    match (expected, actual) {
        (Json::Object(e), Json::Object(a)) => {
            let e_keys: Vec<&String> = e.keys().collect();
            let a_keys: Vec<&String> = a.keys().collect();
            assert_eq!(e_keys, a_keys, "key order differs at {path}");
            if is_block(e) && is_block(a) {
                assert_eq!(e.get("aId"), a.get("aId"), "aId differs at {path}");
                if e != a {
                    let ctx_alloc = BlockAllocatorImpl::with_id(TEST_ALLOCATOR_ID);
                    let ctx = SerDeContext::new(mapper.version(), &ctx_alloc);
                    let e_block = block_deserializer::deserialize_tree(expected, &ctx)
                        .expect("decode fixture block");
                    let a_block = block_deserializer::deserialize_tree(actual, &ctx)
                        .expect("decode output block");
                    assert_eq!(
                        e_block.schema(),
                        a_block.schema(),
                        "block schema differs at {path}"
                    );
                    assert_eq!(e_block, a_block, "block differs at {path}");
                    for key in ["schema", "records"] {
                        if let (Some(Json::String(es)), Some(Json::String(as_))) =
                            (e.get(key), a.get(key))
                        {
                            if es != as_ {
                                replacements.push((as_.clone(), es.clone()));
                            }
                        }
                    }
                }
                return;
            }
            for (k, ev) in e {
                let av = a.get(k).expect("key present");
                compare(ev, av, &format!("{path}.{k}"), mapper, replacements);
            }
        }
        (Json::Array(e), Json::Array(a)) => {
            assert_eq!(e.len(), a.len(), "array length differs at {path}");
            for (i, (ev, av)) in e.iter().zip(a.iter()).enumerate() {
                compare(ev, av, &format!("{path}[{i}]"), mapper, replacements);
            }
        }
        (Json::String(e), Json::String(a))
            if e != a && (path.ends_with(".schema") || path.ends_with(".outputSchema")) =>
        {
            let e_schema = decode_schema(e);
            let a_schema = decode_schema(a);
            assert_eq!(e_schema, a_schema, "schema differs at {path}");
            replacements.push((a.clone(), e.clone()));
        }
        _ => assert_eq!(expected, actual, "value differs at {path}"),
    }
}

pub fn decode_schema(text: &str) -> Schema {
    let bytes = athena_federation_sdk::serde::decode_base64(text).expect("base64");
    schema_from_bytes(&bytes).expect("schema bytes")
}
