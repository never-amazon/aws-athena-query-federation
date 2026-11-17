use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, CachableSecretsManager, SecretsManagerClient,
};

type Answer = Box<dyn Fn(&str) -> Result<Option<String>, AwsServiceError> + Send + Sync>;

#[derive(Default)]
struct MockSecretsManager {
    answer: Mutex<Option<Answer>>,
    calls: AtomicUsize,
}

impl MockSecretsManager {
    fn when_get_secret_value(&self, answer: Answer) {
        *self.answer.lock().unwrap() = Some(answer);
    }

    fn reset(&self) {
        *self.answer.lock().unwrap() = None;
        self.calls.store(0, Ordering::SeqCst);
    }
}

#[async_trait]
impl SecretsManagerClient for MockSecretsManager {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.answer.lock().unwrap().as_ref() {
            Some(answer) => answer(secret_id),
            None => Ok(None),
        }
    }

    async fn put_secret_value(
        &self,
        _secret_id: &str,
        _secret_string: &str,
    ) -> Result<(), AwsServiceError> {
        Ok(())
    }
}

fn now() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn setup() -> (Arc<MockSecretsManager>, CachableSecretsManager) {
    let mock_secrets_manager = Arc::new(MockSecretsManager::default());
    let cachable_secrets_manager = CachableSecretsManager::new(mock_secrets_manager.clone());
    (mock_secrets_manager, cachable_secrets_manager)
}

#[tokio::test]
async fn expiration_test() {
    let (mock_secrets_manager, cachable_secrets_manager) = setup();
    cachable_secrets_manager.add_cache_entry("test", "value", now());
    assert_eq!(
        "value",
        cachable_secrets_manager.get_secret("test").await.unwrap()
    );
    assert_eq!(0, mock_secrets_manager.calls.load(Ordering::SeqCst));

    mock_secrets_manager.reset();
    mock_secrets_manager.when_get_secret_value(Box::new(|secret_id| {
        if secret_id.eq_ignore_ascii_case("test") {
            return Ok(Some("value2".to_string()));
        }
        Err(AwsServiceError::new(""))
    }));
    cachable_secrets_manager.add_cache_entry("test", "value", 0);
    assert_eq!(
        "value2",
        cachable_secrets_manager.get_secret("test").await.unwrap()
    );
}

#[tokio::test]
async fn eviction_test() {
    let (mock_secrets_manager, cachable_secrets_manager) = setup();
    for i in 0..CachableSecretsManager::MAX_CACHE_SIZE {
        cachable_secrets_manager.add_cache_entry(format!("test{i}"), format!("value{i}"), now());
    }
    mock_secrets_manager
        .when_get_secret_value(Box::new(|secret_id| Ok(Some(format!("{secret_id}_value")))));

    assert_eq!(
        "test_value",
        cachable_secrets_manager.get_secret("test").await.unwrap()
    );
    assert_eq!(
        "test0_value",
        cachable_secrets_manager.get_secret("test0").await.unwrap()
    );
    assert_eq!(2, mock_secrets_manager.calls.load(Ordering::SeqCst));
}

#[tokio::test]
async fn resolve_secrets() {
    let (mock_secrets_manager, cachable_secrets_manager) = setup();
    mock_secrets_manager.when_get_secret_value(Box::new(|secret_id| {
        if secret_id.eq_ignore_ascii_case("unknown") {
            return Err(AwsServiceError::new("Unknown secret!"));
        }
        Ok(Some(secret_id.to_string()))
    }));

    let resolve = |s: &'static str| {
        let manager = &cachable_secrets_manager;
        async move { manager.resolve_secrets(Some(s)).await }
    };

    assert_eq!(
        Some("OneSecret".to_string()),
        resolve("${OneSecret}").await.unwrap()
    );
    assert_eq!(
        Some("ThisIsMyStringWithTwoSecretSuperSecretSecrets".to_string()),
        resolve("ThisIsMyStringWith${TwoSecret}SuperSecret${Secrets}")
            .await
            .unwrap()
    );
    assert_eq!(
        Some("ThisIsMyStringWithTwoSecretSuperSecretSecrets".to_string()),
        resolve("ThisIsMyStringWithTwoSecretSuperSecretSecrets")
            .await
            .unwrap()
    );
    assert_eq!(
        Some("ThisIsM}yStringWi${thTwoSecretS{uperSecretSecrets".to_string()),
        resolve("ThisIsM}yStringWi${thTwoSecretS{uperSecretSecrets")
            .await
            .unwrap()
    );
    assert_eq!(
        Some("ThisIs/MyStringWith_AllOf+The@llowed=Special-Characters.".to_string()),
        resolve("ThisIs${/My}StringW${ith_}All${Of+The}${@llowed=}${Special-Characters.}")
            .await
            .unwrap()
    );
    assert!(
        resolve("This${Unknown}").await.is_err(),
        "Should not see this!"
    );
}

#[tokio::test]
async fn resolve_with_default_credentials() {
    let (mock_secrets_manager, cachable_secrets_manager) = setup();
    mock_secrets_manager.when_get_secret_value(Box::new(|secret_id| {
        if secret_id.eq_ignore_ascii_case("unknown") {
            return Err(AwsServiceError::new("Unknown secret!"));
        }
        Ok(Some(
            "{\"username\":\"testuser\", \"password\":\"testpassword\"}".to_string(),
        ))
    }));

    let resolve = |s: Option<&'static str>| {
        let manager = &cachable_secrets_manager;
        async move { manager.resolve_with_default_credentials(s).await }
    };

    assert_eq!(
        Some("testuser:testpassword".to_string()),
        resolve(Some("${OneSecret}")).await.unwrap()
    );
    assert_eq!(
        Some("ThisIsMyStringWithtestuser:testpasswordSuperSecrettestuser:testpassword".to_string()),
        resolve(Some("ThisIsMyStringWith${TwoSecret}SuperSecret${Secrets}"))
            .await
            .unwrap()
    );
    assert_eq!(
        Some("ThisIsMyStringWithTwoSecretSuperSecretSecrets".to_string()),
        resolve(Some("ThisIsMyStringWithTwoSecretSuperSecretSecrets"))
            .await
            .unwrap()
    );
    assert_eq!(
        Some("ThisIsM}yStringWi${thTwoSecretS{uperSecretSecrets".to_string()),
        resolve(Some("ThisIsM}yStringWi${thTwoSecretS{uperSecretSecrets"))
            .await
            .unwrap()
    );
    assert_eq!(
        Some(
            "ThisIstestuser:testpasswordStringWtestuser:testpasswordAlltestuser:testpasswordtestuser:testpasswordtestuser:testpassword"
                .to_string()
        ),
        resolve(Some("ThisIs${/My}StringW${ith_}All${Of+The}${@llowed=}${Special-Characters.}"))
            .await
            .unwrap()
    );
    assert!(
        resolve(Some("This${Unknown}")).await.is_err(),
        "Should not see this!"
    );
    assert_eq!(None, resolve(None).await.unwrap());
}
