use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::exceptions::{
    AthenaConnectorException, ErrorDetails, FederationSourceErrorCode,
};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, GenericJdbcConnectionFactory,
    JdbcConnectionFactory, JdbcDriver, JdbcProperties,
};
use athena_jdbc::sql::{JdbcConnection, SqlException};

use crate::snowflake_constants::SnowflakeConstants;
use crate::sql::{url_decode, SnowflakeDriver, SnowflakeJdbcDriver};
use crate::utils::{SnowflakeAuthType, SnowflakeAuthUtils, SnowflakePrivateKey};

const KEY_PAIR_FAILURE: &str = "Failed to create Snowflake connection with key-pair authentication";

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
        let jdbc_driver: Arc<dyn JdbcDriver> = driver.clone();
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
        credential_map: &HashMap<String, String>,
    ) -> Result<Box<dyn JdbcConnection>> {
        self.open_key_pair_connection(credential_map)
            .await
            .map_err(|message| {
                tracing::error!("{KEY_PAIR_FAILURE}: {message}");
                FederationError::from(SqlException::new(format!("{KEY_PAIR_FAILURE}: {message}")))
            })
    }

    async fn open_key_pair_connection(
        &self,
        credential_map: &HashMap<String, String>,
    ) -> std::result::Result<Box<dyn JdbcConnection>, String> {
        let username = credential_map.get(SnowflakeConstants::USER);
        let private_key_pem = credential_map
            .get(SnowflakeConstants::PEM_PRIVATE_KEY)
            .map(String::as_str)
            .unwrap_or_default();
        let passphrase = credential_map
            .get(SnowflakeConstants::PEM_PRIVATE_KEY_PASSPHRASE)
            .map(String::as_str);
        let private_key = create_private_key_off_the_async_thread(private_key_pem, passphrase)
            .await
            .map_err(|e| e.to_string())?;
        let mut properties = self.key_pair_base_properties.clone();
        let connection_string = GenericJdbcConnectionFactory::secret_name_pattern()
            .replace_all(
                self.generic
                    .database_connection_config()
                    .jdbc_connection_string(),
                "",
            )
            .into_owned();
        let base_url = move_query_parameters_to_properties(&connection_string, &mut properties)?;
        let Some(username) = username else {
            return Err("null".to_string());
        };
        properties.insert(SnowflakeConstants::USER.to_string(), username.clone());
        properties.insert(
            SnowflakeConstants::AUTHENTICATOR.to_string(),
            SnowflakeAuthType::SnowflakeJwt.value().to_string(),
        );
        tracing::debug!(
            "Creating Snowflake connection with key-pair authentication for user: {username}"
        );
        let driver_class_name = self.generic.database_connection_info().driver_class_name();
        if self.driver.driver_class_name() != driver_class_name {
            return Err(driver_class_name.to_string());
        }
        self.driver
            .connect_with_private_key(&base_url, &properties, &private_key)
            .await
            .map_err(|e| e.message().to_string())
    }
}

fn move_query_parameters_to_properties(
    url: &str,
    properties: &mut JdbcProperties,
) -> std::result::Result<String, String> {
    let Some(query_index) = url.find('?') else {
        return Ok(url.to_string());
    };
    for param in url[query_index + 1..].split('&') {
        if let Some((key, value)) = param.split_once('=') {
            properties.insert(
                url_decode(key).map_err(|e| e.message().to_string())?,
                url_decode(value).map_err(|e| e.message().to_string())?,
            );
        }
    }
    Ok(url[..query_index].to_string())
}

async fn create_private_key_off_the_async_thread(
    private_key_pem: &str,
    passphrase: Option<&str>,
) -> Result<SnowflakePrivateKey> {
    let private_key_pem = private_key_pem.to_string();
    let passphrase = passphrase.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        SnowflakeAuthUtils::create_private_key(&private_key_pem, passphrase.as_deref())
    })
    .await
    .map_err(|e| {
        FederationError::AthenaConnector(AthenaConnectorException::with_cause(
            e.to_string(),
            Arc::new(e),
            ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
        ))
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
        if SnowflakeAuthUtils::determine_auth_type(&credential_map)?
            == SnowflakeAuthType::SnowflakeJwt
        {
            return self.get_key_pair_connection(&credential_map).await;
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
        let result = create_private_key_off_the_async_thread(SCRYPT_KEY, Some("test")).await;
        let during = ticks.load(std::sync::atomic::Ordering::SeqCst) - before;
        ticker.abort();
        result.unwrap();
        assert!(during > 1, "ticks while deriving the key: {during}");
    }

    #[tokio::test]
    async fn private_key_errors_are_unchanged_off_the_async_thread() {
        let error = create_private_key_off_the_async_thread(SCRYPT_KEY, Some("wrong"))
            .await
            .unwrap_err();
        assert_eq!("Invalid private key format", error.to_string());
        let error = create_private_key_off_the_async_thread(SCRYPT_KEY, None)
            .await
            .unwrap_err();
        assert_eq!("Invalid private key format", error.to_string());
    }
}
