use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::security::{AwsCrypto, FasToken, KmsEncryptionProvider};
use athena_federation_sdk::Result;
use base64::Engine;

struct MockAwsCrypto {
    expected_key_id: &'static str,
    expected_ciphertext: Vec<u8>,
    result: Vec<u8>,
    calls: Mutex<Vec<(String, Vec<u8>)>>,
}

#[async_trait]
impl AwsCrypto for MockAwsCrypto {
    async fn decrypt_data(&self, kms_key_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>> {
        self.calls
            .lock()
            .unwrap()
            .push((kms_key_id.to_string(), ciphertext.to_vec()));
        if kms_key_id == self.expected_key_id && ciphertext == self.expected_ciphertext.as_slice() {
            Ok(self.result.clone())
        } else {
            Ok(Vec::new())
        }
    }
}

#[tokio::test]
async fn test_decrypt() {
    let aws_crypto = Arc::new(MockAwsCrypto {
        expected_key_id: "test",
        expected_ciphertext: b"data".to_vec(),
        result: b"test".to_vec(),
        calls: Mutex::new(Vec::new()),
    });
    let kms_encryption_provider = KmsEncryptionProvider::with_aws_crypto(aws_crypto.clone());

    let encode = base64::engine::general_purpose::STANDARD.encode(b"data");
    let s = kms_encryption_provider
        .decrypt_data("test", &encode)
        .await
        .unwrap();

    assert_eq!("test", s);
    assert_eq!(
        vec![("test".to_string(), b"data".to_vec())],
        *aws_crypto.calls.lock().unwrap()
    );
}

#[tokio::test]
async fn test_fas_decrypt() {
    let fas_token = FasToken::new("access", "secret", "security");
    let tokens = serde_json::to_string(&fas_token).unwrap();
    let serialized_tokens = base64::engine::general_purpose::STANDARD.encode(tokens.as_bytes());
    let aws_crypto = Arc::new(MockAwsCrypto {
        expected_key_id: "test",
        expected_ciphertext: tokens.as_bytes().to_vec(),
        result: tokens.as_bytes().to_vec(),
        calls: Mutex::new(Vec::new()),
    });
    let kms_encryption_provider = KmsEncryptionProvider::with_aws_crypto(aws_crypto.clone());

    let s = kms_encryption_provider
        .get_fas_credentials("test", &serialized_tokens)
        .await
        .unwrap();

    assert_eq!("access", s.access_key_id());
    assert_eq!("secret", s.secret_access_key());
    assert_eq!(Some("security"), s.session_token());
    assert_eq!(
        vec![("test".to_string(), tokens.as_bytes().to_vec())],
        *aws_crypto.calls.lock().unwrap()
    );
}
