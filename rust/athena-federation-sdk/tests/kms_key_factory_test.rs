use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, EncryptionKeyFactory, KmsClient,
    KmsKeyFactory,
};
use aws_sdk_kms::types::DataKeySpec;

const MASTER_KEY_ID: &str = "test-key-id";

type SeenCall = (
    String,
    DataKeySpec,
    Option<AwsRequestOverrideConfiguration>,
    i32,
);

#[derive(Default)]
struct MockKmsClient {
    data_key: Option<Result<Vec<u8>, AwsServiceError>>,
    random: Option<Vec<u8>>,
    generate_data_key_calls: AtomicUsize,
    generate_random_calls: AtomicUsize,
    seen: Mutex<Vec<SeenCall>>,
}

#[async_trait]
impl KmsClient for MockKmsClient {
    async fn generate_data_key(
        &self,
        key_id: &str,
        key_spec: DataKeySpec,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError> {
        self.generate_data_key_calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push((
            key_id.to_string(),
            key_spec,
            override_configuration.cloned(),
            0,
        ));
        self.data_key.clone().unwrap_or_else(|| Ok(Vec::new()))
    }

    async fn generate_random(&self, number_of_bytes: i32) -> Result<Vec<u8>, AwsServiceError> {
        self.generate_random_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(last) = self.seen.lock().unwrap().last_mut() {
            last.3 = number_of_bytes;
        }
        Ok(self.random.clone().unwrap_or_default())
    }
}

fn mock(test_plaintext_key: &[u8], test_nonce: &[u8]) -> Arc<MockKmsClient> {
    Arc::new(MockKmsClient {
        data_key: Some(Ok(test_plaintext_key.to_vec())),
        random: Some(test_nonce.to_vec()),
        ..MockKmsClient::default()
    })
}

#[tokio::test]
async fn test_create() {
    let test_plaintext_key = [1u8, 2, 3, 4, 5];
    let test_nonce = [9u8, 8, 7];
    let mock_kms_client = mock(&test_plaintext_key, &test_nonce);
    let kms_key_factory = KmsKeyFactory::new(mock_kms_client.clone(), MASTER_KEY_ID);

    let result = kms_key_factory.create().await.unwrap();

    assert_eq!(test_plaintext_key.as_slice(), result.get_key());
    assert_eq!(test_nonce.as_slice(), result.get_nonce());
    assert_eq!(
        1,
        mock_kms_client
            .generate_data_key_calls
            .load(Ordering::SeqCst)
    );
    assert_eq!(
        1,
        mock_kms_client.generate_random_calls.load(Ordering::SeqCst)
    );
    let seen = mock_kms_client.seen.lock().unwrap();
    assert_eq!(
        (MASTER_KEY_ID.to_string(), DataKeySpec::Aes128, None, 12),
        seen[0]
    );
}

#[tokio::test]
async fn test_not_found_exception() {
    let mock_kms_client = Arc::new(MockKmsClient {
        data_key: Some(Err(AwsServiceError::with_code(
            "NotFoundException",
            "Key not found",
        ))),
        ..MockKmsClient::default()
    });
    let kms_key_factory = KmsKeyFactory::new(mock_kms_client, MASTER_KEY_ID);
    let ex = kms_key_factory.create().await.unwrap_err();
    let connector = ex
        .as_connector_exception()
        .expect("AthenaConnectorException");
    assert_eq!(
        Some(FederationSourceErrorCode::EntityNotFoundException),
        connector.error_code()
    );
    assert_eq!("Key not found", connector.get_message());
}

#[tokio::test]
async fn test_create_with_override_configuration() {
    let test_plaintext_key = [1u8, 2, 3, 4, 5];
    let test_nonce = [9u8, 8, 7];
    let override_config = AwsRequestOverrideConfiguration::builder().build();
    let mock_kms_client = mock(&test_plaintext_key, &test_nonce);
    let kms_key_factory = KmsKeyFactory::new(mock_kms_client.clone(), MASTER_KEY_ID);

    let result = kms_key_factory
        .create_with_override_configuration(Some(&override_config))
        .await
        .unwrap();

    assert_eq!(test_plaintext_key.as_slice(), result.get_key());
    assert_eq!(test_nonce.as_slice(), result.get_nonce());
    assert_eq!(
        1,
        mock_kms_client
            .generate_data_key_calls
            .load(Ordering::SeqCst)
    );
    assert_eq!(
        1,
        mock_kms_client.generate_random_calls.load(Ordering::SeqCst)
    );
    assert_eq!(
        Some(override_config),
        mock_kms_client.seen.lock().unwrap()[0].2
    );
}

#[tokio::test]
async fn test_create_with_null_override_configuration() {
    let test_plaintext_key = [1u8, 2, 3, 4, 5];
    let test_nonce = [9u8, 8, 7];
    let mock_kms_client = mock(&test_plaintext_key, &test_nonce);
    let kms_key_factory = KmsKeyFactory::new(mock_kms_client, MASTER_KEY_ID);

    let result = kms_key_factory
        .create_with_override_configuration(None)
        .await
        .unwrap();

    assert_eq!(test_plaintext_key.as_slice(), result.get_key());
    assert_eq!(test_nonce.as_slice(), result.get_nonce());
}
