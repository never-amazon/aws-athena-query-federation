mod serde_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::DataType;
use athena_federation_sdk::data::{Block, SchemaBuilder};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::metadata::{
    GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use athena_federation_sdk::request::{FederationRequest, FederationResponse};
use athena_federation_sdk::udf::{UserDefinedFunctionRequest, UserDefinedFunctionResponse, UserDefinedFunctionType};
use indexmap::IndexSet;
use serde_support::*;

fn fixture_v2(name: &str) -> String {
    fixture("serde/v2", name)
}

fn assert_request_roundtrip_from_fixture(t: &TypedSerDe, text: &str, expected: FederationRequest) {
    let actual: FederationRequest = t.mapper.read_value(text.as_bytes()).expect("deserialize");
    assert_eq!(expected.type_name(), actual.type_name());
    assert_eq!(expected, actual);
}

fn assert_response_roundtrip_from_fixture(t: &TypedSerDe, text: &str, expected: FederationResponse) {
    let actual: FederationResponse = t.mapper.read_value(text.as_bytes()).expect("deserialize");
    assert_eq!(expected.type_name(), actual.type_name());
    assert_eq!(expected, actual);
}

mod get_table_layout_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> GetTableLayoutRequest {
        GetTableLayoutRequest::new(
            Some(t.federated_identity.clone()),
            Some("test-query-id".to_string()),
            "test-catalog",
            table_name(),
            simple_constraints(t.alloc()),
            partitions_schema(),
            ["year", "month", "day"],
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("GetTableLayoutRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("GetTableLayoutRequest.json"), expected(&t).into());
    }
}

mod get_table_layout_response_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> GetTableLayoutResponse {
        let schema = SchemaBuilder::new_builder()
            .add_field_with_type("year", DataType::Int32)
            .add_field_with_type("month", DataType::Int32)
            .add_field_with_type("day", DataType::Int32)
            .add_field_with_type("col3", DataType::Utf8)
            .build();
        let mut partitions = t.alloc().create_block(Arc::new(schema));
        fill_partitions(&mut partitions, 10);
        GetTableLayoutResponse::new("test-catalog", table_name(), partitions)
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("GetTableLayoutResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("GetTableLayoutResponse.json"), expected(&t).into());
    }
}

mod get_splits_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> GetSplitsRequest {
        let mut partitions = t.alloc().create_block(Arc::new(partitions_schema()));
        fill_partitions(&mut partitions, 10);
        GetSplitsRequest::new(
            Some(t.federated_identity.clone()),
            Some("test-query-id".to_string()),
            "test-catalog",
            table_name(),
            partitions,
            vec!["year".to_string(), "month".to_string(), "day".to_string()],
            simple_constraints(t.alloc()),
            Some("test-continuation-token".to_string()),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("GetSplitsRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("GetSplitsRequest.json"), expected(&t).into());
    }
}

mod get_splits_response_ser_de_test {
    use super::*;

    fn split(split_id: &str, key: &str, nonce: &str, day: &str) -> Split {
        Split::new_builder(Some(spill_location(split_id)), Some(encryption_key(key, nonce)))
            .add("year", "2017")
            .add("month", "11")
            .add("day", day)
            .build()
    }

    fn expected() -> GetSplitsResponse {
        GetSplitsResponse::new(
            "test-catalog",
            vec![
                split("test-split-id-1", "test-key-1", "test-nonce-1", "1"),
                split("test-split-id-2", "test-key-2", "test-nonce-2", "2"),
            ],
            Some("test-continuation-token".to_string()),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected());
        assert_serialized_matches(&fixture_v2("GetSplitsResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("GetSplitsResponse.json"), expected().into());
    }
}

mod get_table_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> GetTableRequest {
        GetTableRequest::new(
            Some(t.federated_identity.clone()),
            Some("test-query-id".to_string()),
            "test-catalog",
            table_name(),
            HashMap::new(),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("GetTableRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("GetTableRequest.json"), expected(&t).into());
    }
}

mod get_table_response_ser_de_test {
    use super::*;

    fn expected() -> GetTableResponse {
        let schema = SchemaBuilder::new_builder()
            .add_field_with_type("year", DataType::Int32)
            .add_field_with_type("month", DataType::Int32)
            .add_field_with_type("day", DataType::Int32)
            .add_field_with_type("col3", DataType::Utf8)
            .build();
        let partition_columns: IndexSet<String> = ["year", "month", "day"].iter().map(|s| s.to_string()).collect();
        GetTableResponse::new("test-catalog", table_name(), schema, partition_columns)
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected());
        assert_serialized_matches(&fixture_v2("GetTableResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("GetTableResponse.json"), expected().into());
    }
}

mod list_schemas_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> ListSchemasRequest {
        ListSchemasRequest::new(Some(t.federated_identity.clone()), Some("test-query-id".to_string()), "test-catalog")
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("ListSchemasRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("ListSchemasRequest.json"), expected(&t).into());
    }
}

mod list_schemas_response_ser_de_test {
    use super::*;

    fn expected() -> ListSchemasResponse {
        ListSchemasResponse::new("test-catalog", vec!["schema1".to_string(), "schema2".to_string()])
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected());
        assert_serialized_matches(&fixture_v2("ListSchemasResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("ListSchemasResponse.json"), expected().into());
    }
}

mod list_tables_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> ListTablesRequest {
        ListTablesRequest::new(
            Some(t.federated_identity.clone()),
            Some("test-query-id".to_string()),
            "test-catalog",
            Some("test-schema".to_string()),
            Some("table4".to_string()),
            25,
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("ListTablesRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("ListTablesRequest.json"), expected(&t).into());
    }
}

mod list_tables_response_ser_de_test {
    use super::*;

    fn expected() -> ListTablesResponse {
        ListTablesResponse::new(
            "test-catalog",
            vec![TableName::new("schema1", "table1"), TableName::new("schema1", "table2")],
            Some("table3".to_string()),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected());
        assert_serialized_matches(&fixture_v2("ListTablesResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("ListTablesResponse.json"), expected().into());
    }
}

mod read_records_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> ReadRecordsRequest {
        let schema = partitions_schema();
        let mut partitions = t.alloc().create_block(Arc::new(schema.clone()));
        fill_partitions(&mut partitions, 10);
        ReadRecordsRequest::new(
            Some(t.federated_identity.clone()),
            "test-query-id",
            "test-catalog",
            table_name(),
            schema,
            read_records_split(),
            simple_constraints(t.alloc()),
            100_000_000_000,
            100_000_000_000,
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("ReadRecordsRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("ReadRecordsRequest.json"), expected(&t).into());
    }
}

fn year_month_day_block(t: &TypedSerDe) -> Block {
    let schema = SchemaBuilder::new_builder()
        .add_field_with_type("year", DataType::Int32)
        .add_field_with_type("month", DataType::Int32)
        .add_field_with_type("day", DataType::Int32)
        .build();
    let mut records = t.alloc().create_block(Arc::new(schema));
    fill_partitions(&mut records, 10);
    records
}

mod read_records_response_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> ReadRecordsResponse {
        ReadRecordsResponse::new("test-catalog", year_month_day_block(t))
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("ReadRecordsResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("ReadRecordsResponse.json"), expected(&t).into());
    }
}

mod remote_read_records_response_ser_de_test {
    use super::*;

    fn expected() -> RemoteReadRecordsResponse {
        let schema = SchemaBuilder::new_builder()
            .add_field_with_type("year", DataType::Int32)
            .add_field_with_type("month", DataType::Int32)
            .add_field_with_type("day", DataType::Int32)
            .build();
        RemoteReadRecordsResponse::new(
            "test-catalog",
            schema,
            vec![spill_location("test-split-id-1"), spill_location("test-split-id-2")],
            Some(encryption_key("test-key", "test-nonce")),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected());
        assert_serialized_matches(&fixture_v2("RemoteReadRecordsResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("RemoteReadRecordsResponse.json"), expected().into());
    }
}

mod user_defined_function_request_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> UserDefinedFunctionRequest {
        let input_schema = SchemaBuilder::new_builder()
            .add_field_with_type("factor1", DataType::Int32)
            .add_field_with_type("factor2", DataType::Int32)
            .build();
        let output_schema = SchemaBuilder::new_builder()
            .add_field_with_type("product", DataType::Int32)
            .build();
        let mut input_records = t.alloc().create_block(Arc::new(input_schema));
        input_records.set_row_count(1);
        set(&mut input_records, "factor1", 0, 2i32);
        set(&mut input_records, "factor2", 0, 3i32);
        input_records.set_row_count(1);
        UserDefinedFunctionRequest::new(
            Some(t.federated_identity.clone()),
            input_records,
            output_schema,
            "test-method",
            UserDefinedFunctionType::SCALAR,
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("UserDefinedFunctionRequest.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_request_roundtrip_from_fixture(&t, &fixture_v2("UserDefinedFunctionRequest.json"), expected(&t).into());
    }
}

mod user_defined_function_response_ser_de_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> UserDefinedFunctionResponse {
        let output_schema = SchemaBuilder::new_builder()
            .add_field_with_type("product", DataType::Int32)
            .build();
        let mut records = t.alloc().create_block(Arc::new(output_schema));
        for i in 0..10usize {
            set(&mut records, "product", i, (i as i32) * 2);
        }
        records.set_row_count(10);
        UserDefinedFunctionResponse::new(records, "test-method")
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper, expected(&t));
        assert_serialized_matches(&fixture_v2("UserDefinedFunctionResponse.json"), &actual, &t.mapper);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        assert_response_roundtrip_from_fixture(&t, &fixture_v2("UserDefinedFunctionResponse.json"), expected(&t).into());
    }
}

mod lambda_function_exception_ser_de_test {
    use super::*;
    use athena_federation_sdk::serde::LambdaException;

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let error_type = "com.amazonaws.services.dynamodbv2.model.ResourceNotFoundException";
        let error_message = "Requested resource not found (Service: AmazonDynamoDBv2; Status Code: 400; Error Code: ResourceNotFoundException; Request ID: RIB6NOH4BNMAK6KQG88R5VE583VV4KQNSO5AEMVJF66Q9ASUAAJG)";
        let stack_trace = "[[com.amazonaws.http.AmazonHttpClient$RequestExecutor.handleErrorResponse(AmazonHttpClient.java:1701)], [com.amazonaws.http.AmazonHttpClient$RequestExecutor.executeOneRequest(AmazonHttpClient.java:1356)]]";
        let expected_message = format!("{}. Stack trace: {}\nErrorType: {}", error_message, stack_trace, error_type);
        let text = fixture_v2("LambdaFunctionException.json");
        let actual: LambdaException = t.mapper.read_value(text.as_bytes()).expect("deserialize");
        assert_eq!(Some(expected_message.as_str()), actual.get_message());
        assert!(actual.get_cause().is_none());
    }
}

mod arrow_ser_de_test {
    use athena_federation_sdk::data::SupportedTypes;
    use athena_federation_sdk::serde::v2::arrow_type_serde::{get_delegate_ser_de_map, java_type_name};

    fn arrow_type_class(supported_type: &SupportedTypes) -> String {
        match supported_type.get_arrow_minor_type().get_type() {
            Ok(arrow_type) => java_type_name(&arrow_type).to_string(),
            Err(_) => match supported_type {
                SupportedTypes::TIMESTAMPMILLITZ | SupportedTypes::TIMESTAMPMICROTZ => "Timestamp".to_string(),
                other => format!("{:?}", other),
            },
        }
    }

    fn assert_all_supported_types_covered(kind: &str) {
        let delegates: Vec<String> = get_delegate_ser_de_map().iter().map(|s| s.to_lowercase()).collect();
        for supported_type in SupportedTypes::values() {
            let class = arrow_type_class(supported_type);
            assert!(
                delegates.contains(&class.to_lowercase()),
                "No {} for supported type {:?} with ArrowType {}",
                kind,
                supported_type,
                class
            );
        }
    }

    #[test]
    fn test_supported_types_have_serializers() {
        assert_all_supported_types_covered("serializer");
    }

    #[test]
    fn test_supported_types_have_deserializers() {
        assert_all_supported_types_covered("deserializer");
    }
}

mod object_mapper_factory_v2_test {
    use std::sync::Arc;

    use arrow::datatypes::DataType;
    use athena_federation_sdk::data::BlockAllocatorImpl;
    use athena_federation_sdk::serde::{VersionedObjectMapperFactory, SERDE_VERSION_TWO};
    use athena_federation_sdk::FederationError;

    #[test]
    fn test_strict_serializer() {
        let mapper = VersionedObjectMapperFactory::create_with_version(Arc::new(BlockAllocatorImpl::new()), SERDE_VERSION_TWO).unwrap();
        let result = mapper.write_value_as_string(&DataType::Null);
        assert!(matches!(result, Err(FederationError::AthenaConnector(_))), "{:?}", result);
    }

    #[test]
    fn test_strict_deserializer() {
        let mapper = VersionedObjectMapperFactory::create_with_version(Arc::new(BlockAllocatorImpl::new()), SERDE_VERSION_TWO).unwrap();
        let result = mapper.read_value_from_str::<DataType>("{\"@type\" : \"FloatingPoint\", \"precision\" : \"DOUBLE\"}");
        assert!(matches!(result, Err(FederationError::AthenaConnector(_))), "{:?}", result);
    }
}
