mod serde_support;

use std::collections::HashMap;

use athena_federation_sdk::domain::predicate::functions::StandardFunctions;
use athena_federation_sdk::metadata::optimizations::pushdown::ComplexExpressionPushdownSubType;
use athena_federation_sdk::metadata::optimizations::Capability;
use athena_federation_sdk::metadata::GetDataSourceCapabilitiesResponse;
use athena_federation_sdk::request::{FederationRequest, FederationResponse};
use indexmap::IndexMap;
use serde_support::*;

mod get_data_source_capabilities_response_ser_de_test {
    use super::*;

    fn expected() -> GetDataSourceCapabilitiesResponse {
        let add = StandardFunctions::Add.function_name();
        let subtract = StandardFunctions::Subtract.function_name();
        let properties = ComplexExpressionPushdownSubType::SupportedFunctionExpressionTypes
            .with_sub_type_properties(&[add.function_name(), subtract.function_name()])
            .expect("properties");
        let (name, sub_types) = Capability::ComplexExpression(vec![properties]).into_entry();
        let mut capabilities = IndexMap::new();
        capabilities.insert(name, sub_types);
        GetDataSourceCapabilitiesResponse::new("test-catalog", capabilities)
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_response(&t.mapper_v4, expected());
        assert_eq!(
            fixture("serde/v4", "GetDataSourceCapabilitiesResponse.json"),
            actual
        );
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let text = fixture("serde/v4", "GetDataSourceCapabilitiesResponse.json");
        let actual: FederationResponse = t
            .mapper_v4
            .read_response(text.as_bytes())
            .expect("deserialize");
        assert_eq!(FederationResponse::from(expected()), actual);
    }
}

mod read_records_request_ser_de_v4_test {
    use super::*;

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let expected = read_records_request(
            t.alloc(),
            t.federated_identity.clone(),
            HashMap::new(),
            None,
        );
        let actual = write_request(&t.mapper_v4, expected);
        assert_serialized_matches(
            &fixture("serde/v4", "ReadRecordsRequest.json"),
            &actual,
            &t.mapper_v4,
        );
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let expected = read_records_request(
            t.alloc(),
            t.federated_identity.clone(),
            HashMap::new(),
            None,
        );
        let text = fixture("serde/v4", "ReadRecordsRequest.json");
        let actual: FederationRequest = t
            .mapper_v4
            .read_request(text.as_bytes())
            .expect("deserialize");
        assert_eq!(FederationRequest::from(expected), actual);
    }
}

mod object_mapper_factory_v4_test {
    use std::sync::Arc;

    use arrow::datatypes::Schema;
    use athena_federation_sdk::data::BlockAllocatorImpl;
    use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
    use athena_federation_sdk::FederationError;

    #[test]
    fn test_strict_serializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V4, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.write_schema(&Schema::empty());
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }

    #[test]
    fn test_strict_deserializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V4, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.read_schema(b"{\"fields\" : [ ]}");
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }
}
