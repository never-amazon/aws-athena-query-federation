use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::exceptions::{
    AthenaConnectorException, FederationSourceErrorCode, JavaCause, JavaThrowable,
};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, GenericJdbcConnectionFactory,
    JdbcConnectionFactory, JdbcDriver, JdbcProperties,
};
use athena_jdbc::sql::{JdbcConnection, SqlError};

use crate::snowflake_constants;
use crate::sql::{
    url_decode, PooledSnowflakeDriver, SnowflakeAuth, SnowflakeDriver, SnowflakeJdbcDriver,
};
use crate::utils::{SnowflakeAuthType, SnowflakeAuthUtils, SnowflakePrivateKey};

const KEY_PAIR_FAILURE: &str = "Failed to create Snowflake connection with key-pair authentication";

#[derive(Debug, thiserror::Error)]
enum KeyPairError {
    #[error("{0}")]
    Key(#[source] FederationError),
    #[error("{}", .0.message())]
    Url(#[source] SqlError),
    #[error("null")]
    MissingUser,
    #[error("{0}")]
    DriverNotFound(String),
    #[error("{}", .0.message())]
    Connect(#[source] SqlError),
}

impl JavaThrowable for KeyPairError {
    fn java_class_name(&self) -> String {
        match self {
            KeyPairError::Key(e) => e.java_class_name(),
            KeyPairError::Url(_) => "java.lang.IllegalArgumentException".to_string(),
            KeyPairError::MissingUser => "java.lang.NullPointerException".to_string(),
            KeyPairError::DriverNotFound(_) => "java.lang.ClassNotFoundException".to_string(),
            KeyPairError::Connect(e) => e.java_class_name(),
        }
    }

    fn java_message(&self) -> Option<String> {
        match self {
            KeyPairError::Key(e) => e.java_message(),
            KeyPairError::MissingUser => None,
            KeyPairError::Connect(e) => e.java_message(),
            other => Some(other.to_string()),
        }
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        match self {
            KeyPairError::Key(e) => e.java_cause(),
            KeyPairError::Connect(e) => e.java_cause(),
            KeyPairError::Url(_) | KeyPairError::MissingUser | KeyPairError::DriverNotFound(_) => {
                None
            }
        }
    }

    fn is_runtime_exception(&self) -> bool {
        match self {
            KeyPairError::Key(e) => e.is_runtime_exception(),
            KeyPairError::Url(_) | KeyPairError::MissingUser => true,
            KeyPairError::DriverNotFound(_) | KeyPairError::Connect(_) => false,
        }
    }
}

struct KeyPairCredentials {
    user: Option<String>,
    private_key_pem: String,
    passphrase: Option<String>,
}

enum SnowflakeCredentials {
    KeyPair(KeyPairCredentials),
    Pooled,
}

impl SnowflakeCredentials {
    fn from_map(credential_map: &HashMap<String, String>) -> Result<SnowflakeCredentials> {
        if SnowflakeAuthUtils::determine_auth_type(credential_map)?
            != SnowflakeAuthType::SnowflakeJwt
        {
            return Ok(SnowflakeCredentials::Pooled);
        }
        let get = |key: &str| credential_map.get(key).cloned();
        Ok(SnowflakeCredentials::KeyPair(KeyPairCredentials {
            user: get(snowflake_constants::USER),
            private_key_pem: get(snowflake_constants::PEM_PRIVATE_KEY).unwrap_or_default(),
            passphrase: get(snowflake_constants::PEM_PRIVATE_KEY_PASSPHRASE),
        }))
    }
}

pub struct SnowflakeConnectionFactory {
    generic: GenericJdbcConnectionFactory,
    key_pair_base_properties: JdbcProperties,
    driver: Arc<dyn SnowflakeDriver>,
}

impl SnowflakeConnectionFactory {
    pub fn new(
        database_connection_config: DatabaseConnectionConfig,
        properties: Option<JdbcProperties>,
        database_connection_info: DatabaseConnectionInfo,
    ) -> Result<SnowflakeConnectionFactory> {
        Self::new_with_config_options(
            database_connection_config,
            properties,
            database_connection_info,
            None,
        )
    }

    pub fn new_with_config_options(
        database_connection_config: DatabaseConnectionConfig,
        properties: Option<JdbcProperties>,
        database_connection_info: DatabaseConnectionInfo,
        config_options: Option<&HashMap<String, String>>,
    ) -> Result<SnowflakeConnectionFactory> {
        Self::new_with_driver(
            database_connection_config,
            properties,
            database_connection_info,
            config_options,
            Arc::new(SnowflakeJdbcDriver::new()),
        )
    }

    pub fn new_with_driver(
        database_connection_config: DatabaseConnectionConfig,
        properties: Option<JdbcProperties>,
        database_connection_info: DatabaseConnectionInfo,
        config_options: Option<&HashMap<String, String>>,
        driver: Arc<dyn SnowflakeDriver>,
    ) -> Result<SnowflakeConnectionFactory> {
        let key_pair_base_properties = properties.clone().unwrap_or_default();
        let jdbc_driver: Arc<dyn JdbcDriver> = Arc::new(PooledSnowflakeDriver(Arc::clone(&driver)));
        let generic = GenericJdbcConnectionFactory::new_with_config_options(
            database_connection_config,
            properties,
            database_connection_info,
            config_options,
            jdbc_driver,
        )?;
        Ok(SnowflakeConnectionFactory {
            generic,
            key_pair_base_properties,
            driver,
        })
    }

    pub fn generic(&self) -> &GenericJdbcConnectionFactory {
        &self.generic
    }

    async fn get_key_pair_connection(
        &self,
        credentials: KeyPairCredentials,
    ) -> Result<Box<dyn JdbcConnection>> {
        self.open_key_pair_connection(credentials)
            .await
            .map_err(|error| {
                tracing::error!("{KEY_PAIR_FAILURE}: {error}");
                FederationError::from(SqlError::with_java_cause(
                    format!("{KEY_PAIR_FAILURE}: {error}"),
                    error,
                ))
            })
    }

    async fn open_key_pair_connection(
        &self,
        credentials: KeyPairCredentials,
    ) -> std::result::Result<Box<dyn JdbcConnection>, KeyPairError> {
        let KeyPairCredentials {
            user,
            private_key_pem,
            passphrase,
        } = credentials;
        let private_key = create_private_key_off_the_async_thread(private_key_pem, passphrase)
            .await
            .map_err(KeyPairError::Key)?;
        let mut properties = self.key_pair_base_properties.clone();
        let connection_string = GenericJdbcConnectionFactory::secret_name_pattern()
            .replace_all(
                self.generic
                    .database_connection_config()
                    .jdbc_connection_string(),
                "",
            )
            .into_owned();
        let base_url = move_query_parameters_to_properties(&connection_string, &mut properties)
            .map_err(KeyPairError::Url)?;
        let Some(username) = user else {
            return Err(KeyPairError::MissingUser);
        };
        tracing::debug!(
            "Creating Snowflake connection with key-pair authentication for user: {username}"
        );
        properties.insert(snowflake_constants::USER.to_string(), username);
        properties.insert(
            snowflake_constants::AUTHENTICATOR.to_string(),
            SnowflakeAuthType::SnowflakeJwt.as_str().to_string(),
        );
        let driver_class_name = self.generic.database_connection_info().driver_class_name();
        if self.driver.driver_class_name() != driver_class_name {
            return Err(KeyPairError::DriverNotFound(driver_class_name.to_string()));
        }
        self.driver
            .connect(&base_url, &properties, SnowflakeAuth::KeyPair(&private_key))
            .await
            .map_err(KeyPairError::Connect)
    }
}

fn move_query_parameters_to_properties(
    url: &str,
    properties: &mut JdbcProperties,
) -> std::result::Result<String, SqlError> {
    let Some(query_index) = url.find('?') else {
        return Ok(url.to_string());
    };
    for param in url[query_index + 1..].split('&') {
        if let Some((key, value)) = param.split_once('=') {
            properties.insert(url_decode(key)?, url_decode(value)?);
        }
    }
    Ok(url[..query_index].to_string())
}

async fn create_private_key_off_the_async_thread(
    private_key_pem: String,
    passphrase: Option<String>,
) -> Result<SnowflakePrivateKey> {
    tokio::task::spawn_blocking(move || {
        SnowflakeAuthUtils::create_private_key(&private_key_pem, passphrase.as_deref())
    })
    .await
    .map_err(|e| {
        FederationError::AthenaConnector(
            AthenaConnectorException::new(
                FederationSourceErrorCode::InternalServiceException,
                e.to_string(),
            )
            .with_cause(FederationError::other(e)),
        )
    })?
}

#[async_trait]
impl JdbcConnectionFactory for SnowflakeConnectionFactory {
    async fn get_connection(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let Some(credentials_provider) = credentials_provider else {
            return self.generic.get_connection_from_managed_pool(None).await;
        };
        let credential_map = credentials_provider.get_credential_map().await?;
        if let SnowflakeCredentials::KeyPair(credentials) =
            SnowflakeCredentials::from_map(&credential_map)?
        {
            return self.get_key_pair_connection(credentials).await;
        }
        self.generic
            .get_connection_from_managed_pool(Some(credentials_provider))
            .await
    }

    async fn close(&self) -> Result<()> {
        self.generic.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRYPT_KEY: &str =
        include_str!("../../tests/fixtures/pbes2/pbes2_scrypt_aes256_n_1024_r_8_p_1.p8");

    #[tokio::test]
    async fn private_key_derivation_runs_off_the_async_thread() {
        let ticks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let ticker = tokio::spawn({
            let ticks = ticks.clone();
            async move {
                loop {
                    ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    tokio::task::yield_now().await;
                }
            }
        });
        tokio::task::yield_now().await;
        let before = ticks.load(std::sync::atomic::Ordering::SeqCst);
        let result = create_private_key_off_the_async_thread(
            SCRYPT_KEY.to_string(),
            Some("test".to_string()),
        )
        .await;
        let during = ticks.load(std::sync::atomic::Ordering::SeqCst) - before;
        ticker.abort();
        result.unwrap();
        assert!(during > 1, "ticks while deriving the key: {during}");
    }

    #[tokio::test]
    async fn private_key_errors_are_unchanged_off_the_async_thread() {
        let error = create_private_key_off_the_async_thread(
            SCRYPT_KEY.to_string(),
            Some("wrong".to_string()),
        )
        .await
        .unwrap_err();
        assert_eq!("Invalid private key format", error.to_string());
        let error = create_private_key_off_the_async_thread(SCRYPT_KEY.to_string(), None)
            .await
            .unwrap_err();
        assert_eq!("Invalid private key format", error.to_string());
    }
}
