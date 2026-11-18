mod object_mapper_factory_v3_test {
    use std::sync::Arc;

    use arrow::datatypes::DataType;
    use athena_federation_sdk::data::BlockAllocatorImpl;
    use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
    use athena_federation_sdk::FederationError;

    #[test]
    fn test_strict_serializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V3, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.write_value_as_string(&DataType::Null);
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }

    #[test]
    fn test_strict_deserializer() {
        let mapper = ObjectMapper::new(SerdeVersion::V3, Arc::new(BlockAllocatorImpl::new()));
        let result = mapper.read_value_from_str::<DataType>(
            "{\"@type\" : \"FloatingPoint\", \"precision\" : \"DOUBLE\"}",
        );
        assert!(
            matches!(result, Err(FederationError::AthenaConnector(_))),
            "{result:?}"
        );
    }
}
