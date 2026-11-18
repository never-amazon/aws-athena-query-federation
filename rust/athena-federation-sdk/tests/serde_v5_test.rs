mod serde_support;

use athena_federation_sdk::metadata::GetTableRequest;
use athena_federation_sdk::request::FederationRequest;
use serde_support::*;

fn query_passthrough_arguments() -> std::collections::HashMap<String, Option<String>> {
    string_map(&[("query", "SELECT * FROM DUMMY_TABLE;")])
}

mod get_table_request_ser_de_v5_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> GetTableRequest {
        GetTableRequest::new(
            Some(t.federated_identity.clone()),
            Some("test-query-id".to_string()),
            "test-catalog",
            table_name(),
            query_passthrough_arguments(),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper_v5, expected(&t));
        assert_eq!(fixture("serde/v5", "GetTableRequest.json"), actual);
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let text = fixture("serde/v5", "GetTableRequest.json");
        let actual: FederationRequest = t
            .mapper_v5
            .read_request(text.as_bytes())
            .expect("deserialize");
        assert_eq!(FederationRequest::from(expected(&t)), actual);
    }
}

mod read_records_request_ser_de_v5_test {
    use super::*;

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let expected = read_records_request(
            t.alloc(),
            t.federated_identity.clone(),
            query_passthrough_arguments(),
            None,
        );
        let actual = write_request(&t.mapper_v5, expected);
        assert_serialized_matches(
            &fixture("serde/v5", "ReadRecordsRequest.json"),
            &actual,
            &t.mapper_v5,
        );
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let expected = read_records_request(
            t.alloc(),
            t.federated_identity.clone(),
            query_passthrough_arguments(),
            None,
        );
        let text = fixture("serde/v5", "ReadRecordsRequest.json");
        let actual: FederationRequest = t
            .mapper_v5
            .read_request(text.as_bytes())
            .expect("deserialize");
        assert_eq!(FederationRequest::from(expected), actual);
    }
}

mod object_mapper_factory_v5_test {
    use std::sync::Arc;

    use arrow::datatypes::Schema;
    use athena_federation_sdk::data::BlockAllocatorImpl;
    use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
    use athena_federation_sdk::FederationError;

    #[test]
    fn test_strict_serializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V5, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.write_schema(&Schema::empty());
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }

    #[test]
    fn test_strict_deserializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V5, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.read_schema(b"{\"fields\" : [ ]}");
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }
}
