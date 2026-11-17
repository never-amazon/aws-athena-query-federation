use std::collections::HashMap;

use athena_federation_sdk::security::FederatedIdentity;

pub fn fake_identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}
