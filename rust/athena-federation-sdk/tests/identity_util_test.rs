mod identity_util;

use athena_federation_sdk::security::FederatedIdentity as RequestFederatedIdentity;

#[test]
fn fake_identity_has_arn_account_and_empty_collections() {
    let identity: RequestFederatedIdentity = identity_util::fake_identity();
    assert_eq!(Some("arn"), identity.arn());
    assert_eq!(Some("account"), identity.account());
    assert!(identity.principal_tags().is_empty());
    assert!(identity.iam_groups().is_empty());
    assert!(identity.config_options().is_empty());
}
