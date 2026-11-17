mod identity_util;

use athena_federation_sdk::request::FederatedIdentity as RequestFederatedIdentity;

#[test]
fn fake_identity_has_arn_account_and_empty_collections() {
    let identity: RequestFederatedIdentity = identity_util::fake_identity();
    assert_eq!(Some("arn"), identity.get_arn());
    assert_eq!(Some("account"), identity.get_account());
    assert!(identity.get_principal_tags().is_empty());
    assert!(identity.get_iam_groups().is_empty());
    assert!(identity.get_config_options().is_empty());
}
