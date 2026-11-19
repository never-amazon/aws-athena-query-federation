mod serde_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use athena_federation_sdk::data::{block_utils, Block, BlockAllocatorImpl, SchemaBuilder, Value};
use athena_federation_sdk::domain::predicate::{
    Constraints, Marker, Range, SortedRangeSet, ValueSet, DEFAULT_NO_LIMIT,
};
use athena_federation_sdk::metadata::GetTableLayoutRequest;
use athena_federation_sdk::request::{
    FederationRequest, FederationResponse, PingRequest, PingResponse,
};
use athena_federation_sdk::security::FederatedIdentity;
use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
use serde_support::*;

fn assert_serialization_request(object: &FederationRequest) {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let ser_de = ObjectMapper::new(SerdeVersion::V2, allocator);
    let out = ser_de.write_request(object).expect("serialize");
    let actual: FederationRequest = ser_de.read_request(out.as_bytes()).expect("deserialize");
    assert_eq!(*object, actual);
}

fn assert_serialization_response(object: &FederationResponse) {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let ser_de = ObjectMapper::new(SerdeVersion::V2, allocator);
    let out = ser_de.write_response(object).expect("serialize");
    let actual: FederationResponse = ser_de.read_response(out.as_bytes()).expect("deserialize");
    assert_eq!(*object, actual);
}

mod block_serialization_test {
    use super::*;

    #[test]
    fn serialization_test() {
        let other_allocator = BlockAllocatorImpl::new();
        let expected =
            block_utils::new_block(&other_allocator, "col1", &DataType::Int32, vec![21i32])
                .expect("block");
        let serializer = ObjectMapper::new(SerdeVersion::V1, Arc::new(BlockAllocatorImpl::new()));
        let out = serializer.write_block(&expected).expect("serialize");
        let actual: Block = serializer.read_block(out.as_bytes()).expect("deserialize");
        assert_eq!(expected, actual);
    }
}

mod marker_serialization_test {
    use super::*;

    #[test]
    fn serialization_test() {
        let allocator = Arc::new(BlockAllocatorImpl::new());
        let serializer = ObjectMapper::new(SerdeVersion::V1, Arc::new(BlockAllocatorImpl::new()));
        let expected_value = 1024i32;
        let expected_marker =
            Marker::exactly(allocator.as_ref(), &DataType::Int32, expected_value).expect("marker");
        let out = serializer
            .write_marker(&expected_marker)
            .expect("serialize");
        let deserializer = ObjectMapper::new(SerdeVersion::V1, allocator.clone());
        let actual_marker: Marker = deserializer
            .read_marker(out.as_bytes())
            .expect("deserialize");
        assert_eq!(
            expected_marker.schema().metadata(),
            actual_marker.schema().metadata()
        );
        assert_eq!(
            expected_marker.schema().fields(),
            actual_marker.schema().fields()
        );
        assert_eq!(expected_marker.bound(), actual_marker.bound());
        assert_eq!(
            expected_marker.value().expect("value"),
            actual_marker.value().expect("value")
        );
        assert_eq!(
            &Value::from(expected_value),
            actual_marker.value().expect("value")
        );
        assert!(!actual_marker.is_null_value());
    }

    #[test]
    fn nullable_serialization_test() {
        let allocator = Arc::new(BlockAllocatorImpl::new());
        let serializer = ObjectMapper::new(SerdeVersion::V1, Arc::new(BlockAllocatorImpl::new()));
        let expected_marker = Marker::null_marker(allocator.as_ref(), &DataType::Int32);
        let out = serializer
            .write_marker(&expected_marker)
            .expect("serialize");
        let deserializer = ObjectMapper::new(SerdeVersion::V1, allocator.clone());
        let actual_marker: Marker = deserializer
            .read_marker(out.as_bytes())
            .expect("deserialize");
        assert_eq!(
            expected_marker.schema().metadata(),
            actual_marker.schema().metadata()
        );
        assert_eq!(
            expected_marker.schema().fields(),
            actual_marker.schema().fields()
        );
        assert_eq!(expected_marker.bound(), actual_marker.bound());
        assert!(actual_marker.is_null_value());
    }
}

mod schema_serialization_test {
    use super::*;

    #[test]
    fn serialization_test() {
        let object_mapper =
            ObjectMapper::new(SerdeVersion::V1, Arc::new(BlockAllocatorImpl::new()));
        let expected_schema: Schema = SchemaBuilder::new()
            .add_metadata("meta1", "meta-value-1")
            .add_metadata("meta2", "meta-value-2")
            .add_field_with_type("intfield1", DataType::Int32)
            .add_field_with_type("doublefield2", DataType::Float64)
            .add_field_with_type("varcharfield3", DataType::Utf8)
            .build();
        let out = object_mapper
            .write_schema(&expected_schema)
            .expect("serialize");
        let actual_schema: Schema = object_mapper
            .read_schema(out.as_bytes())
            .expect("deserialize");
        assert_eq!(expected_schema.fields(), actual_schema.fields());
        assert_eq!(expected_schema.metadata(), actual_schema.metadata());
    }
}

mod constraint_serialization_test {
    use super::*;

    #[test]
    fn serialization_test() {
        let allocator = BlockAllocatorImpl::new();
        let constraints_map: indexmap::IndexMap<String, ValueSet> = java_hash_map(vec![
            (
                "col2",
                SortedRangeSet::copy_of(
                    &DataType::Int64,
                    vec![Range::greater_than(&allocator, &DataType::Int64, 950i64).unwrap()],
                    false,
                )
                .unwrap()
                .into(),
            ),
            (
                "col3",
                SortedRangeSet::copy_of(
                    &DataType::Boolean,
                    vec![Range::equal(&allocator, &DataType::Boolean, false).unwrap()],
                    false,
                )
                .unwrap()
                .into(),
            ),
            (
                "col4",
                SortedRangeSet::copy_of(
                    &DataType::Float64,
                    vec![Range::greater_than(&allocator, &DataType::Float64, 950.0f64).unwrap()],
                    false,
                )
                .unwrap()
                .into(),
            ),
            (
                "col5",
                SortedRangeSet::copy_of(
                    &DataType::Utf8,
                    vec![
                        Range::equal(&allocator, &DataType::Utf8, "8").unwrap(),
                        Range::equal(&allocator, &DataType::Utf8, "9").unwrap(),
                    ],
                    false,
                )
                .unwrap()
                .into(),
            ),
        ]);
        let req = GetTableLayoutRequest::new(
            Some(FederatedIdentity::new(
                "arn",
                "account",
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
            )),
            Some("queryId".to_string()),
            "default",
            athena_federation_sdk::domain::TableName::new("schema1", "table1"),
            Constraints::new(
                constraints_map,
                Vec::new(),
                Vec::new(),
                DEFAULT_NO_LIMIT,
                HashMap::new(),
                None,
            ),
            SchemaBuilder::new_builder().build(),
            Vec::<String>::new(),
        );
        assert_serialization_request(&req.into());
    }
}

mod ping_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> PingRequest {
        PingRequest::new(
            Some(t.federated_identity.clone()),
            "test-catalog",
            "test-query-id",
        )
    }

    fn read_ping(t: &TypedSerDe, text: &str) -> PingRequest {
        match t.mapper.read_request(text.as_bytes()).expect("deserialize") {
            FederationRequest::Ping(p) => p,
            other => panic!("expected PingRequest, got {}", other.type_name()),
        }
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_eq!(fixture("serde", "PingRequest.json"), actual);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let expected = expected(&t);
        let actual = read_ping(&t, &fixture("serde", "PingRequest.json"));
        assert_eq!(expected, actual);
        assert_eq!(
            expected.identity().and_then(|i| i.arn()),
            actual.identity().and_then(|i| i.arn())
        );
    }

    #[test]
    fn test_backwards_compatibility() {
        let t = TypedSerDe::before();
        assert_serialization_request(&expected(&t).into());
    }

    #[test]
    fn test_forwards_compatibility() {
        let t = TypedSerDe::before();
        let expected = expected(&t);
        let actual = read_ping(&t, &fixture("serde", "PingRequestForwardsCompatible.json"));
        assert_eq!(expected, actual);
        assert_eq!(
            expected.identity().and_then(|i| i.arn()),
            actual.identity().and_then(|i| i.arn())
        );
    }
}

mod ping_response_ser_de_test {
    use super::*;

    fn expected() -> PingResponse {
        PingResponse::new(
            "test-catalog",
            "test-query-id",
            Some("test-source-type".to_string()),
            23,
            2,
        )
    }

    fn read_ping(t: &TypedSerDe, text: &str) -> PingResponse {
        match t
            .mapper
            .read_response(text.as_bytes())
            .expect("deserialize")
        {
            FederationResponse::Ping(p) => p,
            other => panic!("expected PingResponse, got {}", other.type_name()),
        }
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected());
        assert_eq!(fixture("serde", "PingResponse.json"), actual);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_eq!(
            expected(),
            read_ping(&t, &fixture("serde", "PingResponse.json"))
        );
    }

    #[test]
    fn test_backwards_and_forwards_compatibility() {
        assert_serialization_response(&expected().into());
    }

    #[test]
    fn test_forwards_compatibility() {
        let t = TypedSerDe::before();
        assert_eq!(
            expected(),
            read_ping(&t, &fixture("serde", "PingResponseForwardsCompatible.json"))
        );
    }
}
