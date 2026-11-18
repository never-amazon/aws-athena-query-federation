mod object_mapper_factory_v3_test {
    use std::sync::Arc;

    use arrow::datatypes::Schema;
    use athena_federation_sdk::data::BlockAllocatorImpl;
    use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
    use athena_federation_sdk::FederationError;

    #[test]
    fn test_strict_serializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V3, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.write_schema(&Schema::empty());
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }

    #[test]
    fn test_strict_deserializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V3, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.read_schema(b"{\"fields\" : [ ]}");
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }
}
