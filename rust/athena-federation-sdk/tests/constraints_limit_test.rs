use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use athena_federation_sdk::data::BlockAllocatorImpl;
use athena_federation_sdk::domain::predicate::{Constraints, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::{Split, TableName};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationRequest;
use athena_federation_sdk::security::FederatedIdentity;
use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
use indexmap::IndexMap;

fn request(limit: i64) -> FederationRequest {
    let constraints = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        limit,
        HashMap::new(),
        None,
    );
    ReadRecordsRequest::new(
        Some(FederatedIdentity::new(
            "arn",
            "0123456789",
            HashMap::new(),
            Vec::new(),
            HashMap::new(),
        )),
        "catalog",
        "query",
        TableName::new("schema", "table"),
        Schema::empty(),
        Split::new(None, None, HashMap::new()),
        constraints,
        100,
        100,
    )
    .into()
}

fn constraints_of(request: &FederationRequest) -> &Constraints {
    match request {
        FederationRequest::ReadRecords(r) => r.constraints(),
        other => panic!("unexpected request {other:?}"),
    }
}

#[test]
fn wire_keeps_no_limit_zero_and_positive_limits_apart() {
    let mapper = ObjectMapper::new(SerdeVersion::V6, Arc::new(BlockAllocatorImpl::new()));
    for (wire, limit) in [
        (DEFAULT_NO_LIMIT, None),
        (0, None),
        (-7, None),
        (25, Some(25)),
    ] {
        let json = mapper.write_request(&request(wire)).unwrap();
        assert!(
            json.contains(&format!("\"limit\":{wire},")),
            "limit {wire} not written verbatim in {json}"
        );
        let back = mapper.read_request(json.as_bytes()).unwrap();
        assert_eq!(wire, constraints_of(&back).wire_limit());
        assert_eq!(limit, constraints_of(&back).limit());
        assert_eq!(json, mapper.write_request(&back).unwrap());
    }
}
