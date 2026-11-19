use athena_federation_sdk::credentials::{
    Credentials, CredentialsProvider, DefaultCredentials, DefaultCredentialsProvider,
    StaticCredentialsProvider,
};
use athena_federation_sdk::exceptions::JavaThrowable;

#[tokio::test]
async fn get_static_credential() {
    let expected_credential = DefaultCredentials::new("testUser", "testPassword").unwrap();
    let credentials_provider: Box<dyn CredentialsProvider> =
        Box::new(StaticCredentialsProvider::new(expected_credential.clone()).unwrap());
    let actual = credentials_provider.get_credential().await.unwrap();
    assert_eq!(Credentials::UserPassword(expected_credential), actual);
}

#[tokio::test]
async fn get_rds_secrets_credential() {
    let expected_credential = DefaultCredentials::new("testUser", "testPassword").unwrap();
    let credentials_provider: Box<dyn CredentialsProvider> = Box::new(
        DefaultCredentialsProvider::new(
            "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
        )
        .unwrap(),
    );
    let actual = credentials_provider.get_credential().await.unwrap();
    assert_eq!(Credentials::UserPassword(expected_credential), actual);
}

#[test]
fn get_rds_secrets_credential_io_exception() {
    let ex = DefaultCredentialsProvider::new("").unwrap_err();
    assert!(ex.is_runtime_exception());
}
