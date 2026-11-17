use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use regex::Regex;

use super::aws_request_override_configuration::AwsRequestOverrideConfiguration;
use super::secrets_manager_client::SecretsManagerClient;
use crate::credentials::DefaultCredentialsProvider;
use crate::exceptions::{FederationError, Result};

const MAX_CACHE_AGE_MS: i64 = 60_000;
const SECRET_PATTERN: &str = r"(\$\{[a-zA-Z0-9/_\-.+=@]+\})";
const SECRET_NAME_PATTERN: &str = r"\$\{([a-zA-Z0-9/_\-.+=@]+)\}";

static PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(SECRET_PATTERN).expect("valid secret pattern"));
static NAME_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(SECRET_NAME_PATTERN).expect("valid secret name pattern"));

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: String,
    create_time: i64,
}

impl CacheEntry {
    fn get_age(&self) -> i64 {
        current_time_millis() - self.create_time
    }
}

pub struct CachableSecretsManager {
    cache: Mutex<IndexMap<String, CacheEntry>>,
    secrets_manager: Arc<dyn SecretsManagerClient>,
}

impl CachableSecretsManager {
    pub const MAX_CACHE_SIZE: usize = 10;

    pub fn new(secrets_manager: Arc<dyn SecretsManagerClient>) -> CachableSecretsManager {
        CachableSecretsManager {
            cache: Mutex::new(IndexMap::new()),
            secrets_manager,
        }
    }

    pub fn get_secrets_manager(&self) -> &Arc<dyn SecretsManagerClient> {
        &self.secrets_manager
    }

    fn secret_names(raw_string: &str) -> Vec<(String, String)> {
        PATTERN
            .captures_iter(raw_string)
            .filter_map(|m| {
                let next_secret = m.get(1)?.as_str().to_string();
                let name = NAME_PATTERN
                    .captures(&next_secret)?
                    .get(1)?
                    .as_str()
                    .to_string();
                Some((next_secret, name))
            })
            .collect()
    }

    pub async fn resolve_secrets(&self, raw_string: Option<&str>) -> Result<Option<String>> {
        let Some(raw_string) = raw_string else {
            return Ok(None);
        };
        let mut result = raw_string.to_string();
        for (next_secret, name) in Self::secret_names(raw_string) {
            let secret = self.get_secret(&name).await?;
            result = result.replace(&next_secret, &secret);
        }
        Ok(Some(result))
    }

    pub async fn resolve_with_default_credentials(
        &self,
        raw_string: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(raw_string) = raw_string else {
            return Ok(None);
        };
        let mut result = raw_string.to_string();
        for (next_secret, name) in Self::secret_names(raw_string) {
            let replacement = if let Ok(credentials) = self.use_default_credentials(&name).await {
                credentials
            } else {
                tracing::info!(
                    "Credentials are not in json format. Falling back to <username>:<password>..."
                );
                self.get_secret(&name).await?
            };
            result = result.replace(&next_secret, &replacement);
        }
        Ok(Some(result))
    }

    async fn use_default_credentials(&self, secret: &str) -> Result<String> {
        let secret_string = self.get_secret(secret).await?;
        let provider = DefaultCredentialsProvider::new(&secret_string)?;
        let credentials = provider.get_credential();
        Ok(format!(
            "{}:{}",
            credentials.get_user(),
            credentials.get_password()
        ))
    }

    pub async fn get_secret(&self, secret_name: &str) -> Result<String> {
        self.get_secret_with_override_configuration(secret_name, None)
            .await
    }

    pub async fn get_secret_with_override_configuration(
        &self,
        secret_name: &str,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<String> {
        if let Some(entry) = self.lock_cache().get(secret_name) {
            if entry.get_age() <= MAX_CACHE_AGE_MS {
                return Ok(entry.value.clone());
            }
        }
        tracing::info!("getSecret: Resolving secret[{}].", secret_name);
        let secret_value = self
            .secrets_manager
            .get_secret_value(secret_name, override_configuration)
            .await?
            .ok_or_else(|| {
                FederationError::NullPointer(format!("Secret {secret_name} has no SecretString"))
            })?;
        let entry = CacheEntry {
            value: secret_value.clone(),
            create_time: current_time_millis(),
        };
        let mut cache = self.lock_cache();
        let force = cache.len() >= Self::MAX_CACHE_SIZE;
        Self::evict_cache(&mut cache, force);
        cache.insert(secret_name.to_string(), entry);
        Ok(secret_value)
    }

    fn evict_cache(cache: &mut IndexMap<String, CacheEntry>, force: bool) {
        let before = cache.len();
        cache.retain(|_, entry| entry.get_age() <= MAX_CACHE_AGE_MS);
        let removed = before - cache.len();
        if removed == 0 && force {
            cache.shift_remove_index(0);
        }
    }

    pub fn add_cache_entry(
        &self,
        name: impl Into<String>,
        value: impl Into<String>,
        create_time: i64,
    ) {
        self.lock_cache().insert(
            name.into(),
            CacheEntry {
                value: value.into(),
                create_time,
            },
        );
    }

    fn lock_cache(&self) -> MutexGuard<'_, IndexMap<String, CacheEntry>> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
