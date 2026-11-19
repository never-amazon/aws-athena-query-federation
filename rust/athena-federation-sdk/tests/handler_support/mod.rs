#![allow(
    dead_code,
    reason = "each test binary uses a different subset of these fakes"
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arrow::datatypes::{Schema, SchemaRef};
use async_trait::async_trait;
use athena_federation_sdk::data::{
    ArrowBuf, ArrowRecordBatch, BatchGenerator, Block, BlockAllocator, BlockAllocatorImpl,
    GetObjectRequest, PutObjectRequest, S3Client,
};
use athena_federation_sdk::handlers::AwsClients;
use athena_federation_sdk::security::{
    AwsCrypto, AwsRequestOverrideConfiguration, AwsServiceError, KmsClient, KmsEncryptionProvider,
    SecretsManagerClient,
};
use athena_federation_sdk::AthenaClient;
use aws_credential_types::Credentials;
use aws_sdk_kms::types::DataKeySpec;

pub const FAS_ACCESS: &str = "fas-access";

#[derive(Default)]
pub struct FakeSecretsManager {
    secrets: Mutex<HashMap<String, String>>,
    calls: Mutex<Vec<(String, Option<AwsRequestOverrideConfiguration>)>>,
}

impl FakeSecretsManager {
    pub fn with_secret(name: &str, value: &str) -> FakeSecretsManager {
        let fake = FakeSecretsManager::default();
        fake.secrets
            .lock()
            .unwrap()
            .insert(name.to_string(), value.to_string());
        fake
    }

    pub fn calls(&self) -> Vec<(String, Option<AwsRequestOverrideConfiguration>)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl SecretsManagerClient for FakeSecretsManager {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError> {
        self.calls
            .lock()
            .unwrap()
            .push((secret_id.to_string(), override_configuration.cloned()));
        self.secrets
            .lock()
            .unwrap()
            .get(secret_id)
            .cloned()
            .map(Some)
            .ok_or_else(|| AwsServiceError::with_code("ResourceNotFoundException", "not found"))
    }

    async fn put_secret_value(
        &self,
        _secret_id: &str,
        _secret_string: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<(), AwsServiceError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeAthena {
    pub credentials: Option<Credentials>,
}

#[async_trait]
impl AthenaClient for FakeAthena {
    async fn get_query_execution_state(
        &self,
        _query_execution_id: &str,
    ) -> Result<Option<String>, AwsServiceError> {
        Ok(Some("RUNNING".to_string()))
    }

    fn with_credentials(&self, credentials: &Credentials) -> Arc<dyn AthenaClient> {
        Arc::new(FakeAthena {
            credentials: Some(credentials.clone()),
        })
    }
}

#[derive(Default, Clone)]
pub struct FakeS3 {
    pub objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    pub head_bucket_calls: Arc<Mutex<Vec<String>>>,
    pub credentials: Option<Credentials>,
}

#[async_trait]
impl S3Client for FakeS3 {
    async fn put_object(
        &self,
        request: PutObjectRequest,
        body: Vec<u8>,
    ) -> Result<(), AwsServiceError> {
        self.objects
            .lock()
            .unwrap()
            .insert(format!("{}/{}", request.bucket(), request.key()), body);
        Ok(())
    }

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError> {
        self.objects
            .lock()
            .unwrap()
            .get(&format!("{}/{}", request.bucket(), request.key()))
            .cloned()
            .ok_or_else(|| AwsServiceError::with_code("NoSuchKey", "missing"))
    }

    async fn head_bucket(&self, bucket: &str) -> Result<(), AwsServiceError> {
        self.head_bucket_calls
            .lock()
            .unwrap()
            .push(bucket.to_string());
        Ok(())
    }

    fn with_credentials(&self, credentials: &Credentials) -> Arc<dyn S3Client> {
        Arc::new(FakeS3 {
            objects: Arc::clone(&self.objects),
            head_bucket_calls: Arc::clone(&self.head_bucket_calls),
            credentials: Some(credentials.clone()),
        })
    }
}

pub struct FakeKms;

#[async_trait]
impl KmsClient for FakeKms {
    async fn generate_data_key(
        &self,
        _key_id: &str,
        _key_spec: DataKeySpec,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError> {
        Ok(vec![7u8; 32])
    }

    async fn generate_random(&self, number_of_bytes: usize) -> Result<Vec<u8>, AwsServiceError> {
        Ok(vec![3u8; number_of_bytes])
    }
}

#[derive(Default)]
pub struct FakeCrypto {
    pub calls: Mutex<Vec<(String, Vec<u8>)>>,
}

#[async_trait]
impl AwsCrypto for FakeCrypto {
    async fn decrypt_data(
        &self,
        kms_key_id: &str,
        ciphertext: &[u8],
    ) -> athena_federation_sdk::Result<Vec<u8>> {
        self.calls
            .lock()
            .unwrap()
            .push((kms_key_id.to_string(), ciphertext.to_vec()));
        Ok(format!(
            "{{\"accessToken\":\"{FAS_ACCESS}\",\"secretToken\":\"fas-secret\",\"securityToken\":\"fas-session\"}}"
        )
        .into_bytes())
    }
}

pub struct Fakes {
    pub secrets: Arc<FakeSecretsManager>,
    pub s3: FakeS3,
    pub crypto: Arc<FakeCrypto>,
}

impl Fakes {
    pub fn new() -> Fakes {
        Fakes::with_secrets(FakeSecretsManager::default())
    }

    pub fn with_secrets(secrets: FakeSecretsManager) -> Fakes {
        Fakes {
            secrets: Arc::new(secrets),
            s3: FakeS3::default(),
            crypto: Arc::new(FakeCrypto::default()),
        }
    }

    pub fn clients(&self) -> AwsClients {
        AwsClients {
            secrets_manager: self.secrets.clone(),
            athena: Arc::new(FakeAthena::default()),
            s3: Arc::new(self.s3.clone()),
            kms: Arc::new(FakeKms),
            kms_encryption_provider: Arc::new(KmsEncryptionProvider::with_aws_crypto(
                self.crypto.clone(),
            )),
        }
    }
}

pub fn fas_config_options() -> HashMap<String, Option<String>> {
    HashMap::from([
        (
            "fas_token".to_string(),
            Some("dGVzdC10b2tlbg==".to_string()),
        ),
        (
            "service_kms_key_id".to_string(),
            Some("test-kms-key".to_string()),
        ),
    ])
}

pub struct RecordingAllocator {
    inner: BlockAllocatorImpl,
    pub created: Mutex<Vec<Schema>>,
}

impl RecordingAllocator {
    pub fn new() -> RecordingAllocator {
        RecordingAllocator {
            inner: BlockAllocatorImpl::new(),
            created: Mutex::new(Vec::new()),
        }
    }
}

impl BlockAllocator for RecordingAllocator {
    fn create_block(&self, schema: SchemaRef) -> Block {
        self.created.lock().unwrap().push(schema.as_ref().clone());
        self.inner.create_block(schema)
    }

    fn create_buffer(&self, size: usize) -> athena_federation_sdk::Result<ArrowBuf> {
        self.inner.create_buffer(size)
    }

    fn register_batch(
        &self,
        generator: &mut BatchGenerator<'_>,
    ) -> athena_federation_sdk::Result<ArrowRecordBatch> {
        self.inner.register_batch(generator)
    }

    fn usage(&self) -> usize {
        self.inner.usage()
    }

    fn close(&self) {
        self.inner.close();
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }
}
