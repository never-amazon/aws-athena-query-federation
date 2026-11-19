mod serde_support;

use athena_federation_sdk::domain::predicate::QueryPlan;
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationRequest;
use serde_support::*;

mod read_records_request_ser_de_v6_test {
    use super::*;

    fn expected(t: &TypedSerDe) -> ReadRecordsRequest {
        read_records_request(
            t.alloc(),
            t.federated_identity.clone(),
            string_map(&[("query", "SELECT * FROM DUMMY_TABLE;")]),
            Some(QueryPlan::new(Some("1.0.0".to_string()), "TestPlan")),
        )
    }

    #[test]
    fn serialize() {
        let t = TypedSerDe::before();
        let actual = write_request(&t.mapper_v6, expected(&t));
        assert_serialized_matches(
            &fixture("serde/v6", "ReadRecordsRequest.json"),
            &actual,
            &t.mapper_v6,
        );
    }

    #[test]
    fn deserialize() {
        let t = TypedSerDe::before();
        let text = fixture("serde/v6", "ReadRecordsRequest.json");
        let actual: FederationRequest = t
            .mapper_v6
            .read_request(text.as_bytes())
            .expect("deserialize");
        assert_eq!(FederationRequest::from(expected(&t)), actual);
    }
}

mod object_mapper_factory_v6_test {
    use std::sync::Arc;

    use arrow::datatypes::Schema;
    use athena_federation_sdk::data::BlockAllocatorImpl;
    use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
    use athena_federation_sdk::FederationError;

    #[test]
    fn test_strict_serializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V6, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.write_schema(&Schema::empty());
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }

    #[test]
    fn test_strict_deserializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V6, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.read_schema(b"{\"fields\" : [ ]}");
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }
}
