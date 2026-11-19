mod snowflake_support;

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::credentials::{Credentials, CredentialsProvider};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, JdbcConnectionFactory, JdbcDriver,
    JdbcProperties,
};
use athena_jdbc::sql::{JdbcConnection, SqlError};
use athena_snowflake::connection::SnowflakeConnectionFactory;
use athena_snowflake::snowflake_constants;
use athena_snowflake::sql::SnowflakeDriver;
use athena_snowflake::utils::{SnowflakeAuthUtils, SnowflakePrivateKey};
use snowflake_support::FakeConnection;

const TESTUSER: &str = "testuser";
const TESTPASSWORD: &str = "testpassword";
const PASSPHRASE: &str = "test-passphrase";
const CONNECTION_STRING: &str = "jdbc:snowflake://test.snowflakecomputing.com";
const FAKE_HOST_FAILURE: &str = "Could not connect to test.snowflakecomputing.com";
const CONNECTION_MARKER: &str = "fake-connection";

const VALID_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQDd4UYv6P0RH5bD\n\
OAmqJfqQLFH6eGgnRU0M8ef8sjrTHFFyQoxaCuh3aCXk5Nk1tWVhSyBdhCUStVUN\n\
51nmg3KejPziwW19ljvuw9O53aFUhzbYCbWZX5v702CZV3PCWMFubZtz272WL+Ed\n\
afkKmZ3dA8WubyFv+zA0nosualr5B5HJLmIpKPHhEYNY/KslGgJ6gxTuc/uEafAo\n\
wAiq2daiWq4/VU2uoryGcNS2mb6AtoXlCkyTbFglGQ2DwUE+XBOuPghhjgcXxAV+\n\
u4Ct80Hcwxdmalel90axI68Pgog9I92tLMqPuXMB0dlJqXMrm8khiHUWEUD0AiHn\n\
Okgif9FBAgMBAAECggEAK/Rvyhf24kiGvl/AkmN5sswE5YCq6XSyIeleUtIgEyhd\n\
/iuu0Duu99C7CRBI83ne/po6bCw8h+tCUUHOrnrF3yEBDA7TmPiFIPTMYdrFFhC/\n\
TacGwb6OESI71cTcWtjoYO73x8j1UuZjS0duHap2+oy/waGkVyHE1/e8jNJm+UQn\n\
LI1m+SGr9h1QJMw83odtc2At/EJmgGCsXKGf4QOo5vuItEt0ETlSzpyDuo49U1Yc\n\
hS460Q8qwYBb2UOq1TV5/4TP5rvqnHxD3P3Os0fkB31E57Vt5GRnXng2NHl8zWSI\n\
k/DciOZrVmoHhV2+LtuEhqyqKq7shElyKAnasfNfXQKBgQDyux70rv0vpgUyDG7k\n\
R2FUdb0AvU1/QGaSNC4UmZh2FgW63tFWN2IAeHSyCW6fFnRA9H8B+jobOTvd5zlk\n\
jzLcONMjjCXY7Rzl6BdRAFlSrc3awk1ZFz5lzuJa1eZcyPdNlgjD6ukh8Oe4Z+E2\n\
CzahqhYR3gZ4RjVVePrUBZc8bwKBgQDqAlsni9100lbyCes6OLbs3TTyU86gVZ4y\n\
2HjcpkEfWPUlixBkS92IQajhA4jJN0drJcSmwU2cPsGxKrAf3b+uawKYuxEycAD4\n\
zELnO2wBN840/aqO9OU/terfIB7gIg+yNosBEXsmR/XSkthvrAFQW64Tzqq2ofhJ\n\
tbnKNLoFTwKBgQCR1Yj4u3YUvaubcUrizlwU0ZRXsGSyUrANoVoaOFoYgIy6xIqD\n\
QU0ZOUXrfa0/2c7JA9yXBI8Ksi1c2kJqP3rwQxff4KEQnw1guDviB9Ki0RfAGkM8\n\
hrGPm2f16VO5ZWjn7X+gri+Zm9KswYTW4o/VO4p8+6dpdc0vaDtTTXSa8wKBgQDI\n\
VaAIKRs8Kd65c3hn7anc+jubwdfBN8spqF1AOSzDU3r72yP7WV+ciOm1STtPV/3S\n\
YxEJ2a9BthBIRk1l2oeBIKembW/5iB2s4eHc5+g82rO0df4QSOQCORGMV4lXRJG/\n\
diZmkbqmOTDgR0r3v3+YdgR/fIx1shBHukYGMEGYzQKBgQCTtkr2mlbMDMmuaGBd\n\
GbDgGdkEtHjPdRHPxurqk5IUMWaxDyendX7cccmKc2QVkRPNg68ThZs6QKPTxQ5g\n\
Qz94I2WPt0DzBb5KVS6U2cnDp0xUtxBmg8eYkdamISpyL0w8oPIXSXVmlfvljmCl\n\
RlR+O9r/fhM4ThYnZOGaBVpwbg==\n\
-----END PRIVATE KEY-----
";

const ENCRYPTED_PRIVATE_KEY_PEM_PBES2: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----\n\
MIIFLTBXBgkqhkiG9w0BBQ0wSjApBgkqhkiG9w0BBQwwHAQIOd0338i1gJICAggA\n\
MAwGCCqGSIb3DQIJBQAwHQYJYIZIAWUDBAEqBBB/6GmJysxU/RDBYCFkiweVBIIE\n\
0F58t5lRY7HrPQJRzmqyMq+c2jvXYUDNu4XkIDSs+fPLf/QQUsxmr2300IhsdSwQ\n\
EzQEsxEhjba5STh+jpy/W+i3q9yTOsVCKSVDV4d3k25t1ypvsWi9gCrqXzRonMAu\n\
r0STN3pXNaisyGiohIwbgfOfdhLRIuS3u/BlOYxfGtJgSDq4YV1DQX5J6aXn5HMc\n\
x2xDD6iqOtgZ+fjs81gCg6G2ap+GiRAP8maT2+ZjuDkQeordwm5ub9TmiLWgdmlg\n\
fKxKB1jQou6wb9Ct284ltvik+fipGUxOH1lHSKICdHh4LsC09zgSTNkqHoHaXnL2\n\
lZG5I8mdXJW9/PUo2PnqN7j2fAKTVYWwaRLE72nQv6PgBhkiJYSTJkKgRHDkJJJM\n\
yZL8jg7coa8KAklP8rF/qqgYc7VkDUqNImneS135B2VtXCIvsTgtzYc7ZTNHGHFy\n\
1GlbXj3XGemF4Qkf19T4wilHxIRGzuFjj0kAIyx3PGMIJoQhRKPN5CJ/E2rz3vow\n\
HPQuZ4sAbWEEU4J8b/nHpMGffKb3c/Puem48sX4usYdy4eKEQ12YQgMncD9FK8tq\n\
3craN75TpG5V+Wq53M4wOv+D6twUK/Y8GfefhoOs8+p/6quJuFfFkk9BdQmftlIw\n\
R/F3pE/90g1YyX4ReGZdK/P3HCAUK+giuOPJ0BUdlCc5vq/50kVj/l8D0Efax9gi\n\
YcpU6RgdPPBSu9HWO5EHhMmHkOTkrmBFt/2qWNHlT4i33lg9gVTf2KYt3eNSpmTX\n\
SWKQFBZZhIWoE2mKeRIvl5j4pe6dfb75qFZwa+JImtidcbvaihmg4zaUNzWdxFoV\n\
PbhroOH6I3jv28T8ELMYlfoaexOWFbj1T4RCk2SBP+8+VEowBbkHGeTJWgMHTtlW\n\
bh+Xb/JckDoXoT20kJ9xG6VfZR634vfo0EqgMBVqBl834KQlz/GjmitKpWx3ouPO\n\
ahsoZM0mAFpTcp5ReRBVbuGHMCu0lvp1uTnUZubZf/mkNAnzGgPddGY8xgxm8N7u\n\
MTBrOLjwmjSZ6VvHkjhedPr4F12P6HLfiBLgEj4y5rnr7WkXiAcV5aTauHndxJWo\n\
RGfAetAvvz967mqHA0hNt1v0+TKP3wtB9JqiOACIg7fT4iLSf6XIboT+n+jqkccj\n\
lM3PAfMjzFa09ag2fu1JGejt2fXr3/BOJ9KkZPceqANBej98DysIMOFIQxyLujJ+\n\
DWaPgLo7GhFDT9cgVqaFTSNl3gyCbFWCX/d3Pa7crZhcJ3OzhdJwVvv/ooNXR5Gl\n\
/Chd5bU3/hQg0VoETgfRoOAs27TVxLc94gcXyL+2/V8D+6KU0gwNF0KOTV04xpL2\n\
PkDFF+1RrKRgwEdy2p2jTQY44K29hT8uzeRRcU262J0+CMzqsAtkFoUVOsx2UGgu\n\
Fg2ez6ccdtBwAJHWRcyW2Oz9C7+hhMWvypI1CLQDqFT9lS0T3tAbT0XJzFQRT1Jk\n\
ejwJBpWdkt39VudWP6X0ozycqZ5mfILrMIIYEKAXd2r2U5ZcPQWDzpE3aMzHxaFT\n\
EzP9kpcL69XkBtjiBpP/SjFEFE2pOGxFMTH8Ug+dLK9V/jI1T73fKaUxuvV/qXgr\n\
/Qe8Up46AZGfWKALbN/FWrXOkanMFuPkHcIMdqAjeMuW\n\
-----END ENCRYPTED PRIVATE KEY-----
";

#[derive(Clone)]
struct KeyPairCall {
    url: String,
    properties: JdbcProperties,
    private_key: SnowflakePrivateKey,
}

#[derive(Default)]
struct RecordingDriver {
    connection: FakeConnection,
    pooled_error: Mutex<Option<SqlError>>,
    key_pair_error: Mutex<Option<SqlError>>,
    pooled_calls: Mutex<Vec<(String, JdbcProperties)>>,
    key_pair_calls: Mutex<Vec<KeyPairCall>>,
}

impl RecordingDriver {
    fn new() -> Arc<RecordingDriver> {
        let driver = RecordingDriver::default();
        driver.connection.set_catalog(Some(CONNECTION_MARKER));
        Arc::new(driver)
    }

    fn unreachable_host() -> Arc<RecordingDriver> {
        let driver = RecordingDriver::new();
        *driver.pooled_error.lock().unwrap() = Some(SqlError::new(FAKE_HOST_FAILURE));
        driver
    }

    fn pooled_calls(&self) -> Vec<(String, JdbcProperties)> {
        self.pooled_calls.lock().unwrap().clone()
    }

    fn key_pair_calls(&self) -> Vec<KeyPairCall> {
        self.key_pair_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl JdbcDriver for RecordingDriver {
    fn driver_class_name(&self) -> &str {
        snowflake_constants::SNOWFLAKE_DRIVER_CLASS
    }

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
    ) -> std::result::Result<Box<dyn JdbcConnection>, SqlError> {
        self.pooled_calls
            .lock()
            .unwrap()
            .push((url.to_string(), properties.clone()));
        match self.pooled_error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(Box::new(self.connection.clone())),
        }
    }
}

#[async_trait]
impl SnowflakeDriver for RecordingDriver {
    async fn connect_with_private_key(
        &self,
        url: &str,
        properties: &JdbcProperties,
        private_key: &SnowflakePrivateKey,
    ) -> std::result::Result<Box<dyn JdbcConnection>, SqlError> {
        self.key_pair_calls.lock().unwrap().push(KeyPairCall {
            url: url.to_string(),
            properties: properties.clone(),
            private_key: private_key.clone(),
        });
        match self.key_pair_error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(Box::new(self.connection.clone())),
        }
    }
}

struct MapCredentialsProvider {
    credentials: HashMap<String, String>,
}

impl MapCredentialsProvider {
    fn new(entries: &[(&str, &str)]) -> MapCredentialsProvider {
        MapCredentialsProvider {
            credentials: entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        }
    }
}

#[async_trait]
impl CredentialsProvider for MapCredentialsProvider {
    async fn get_credential(&self) -> Result<Credentials> {
        Err(FederationError::illegal_state(
            "the factory reads the credential map",
        ))
    }

    async fn get_credential_map(&self) -> Result<HashMap<String, String>> {
        Ok(self.credentials.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn factory_for(
    connection_string: &str,
    driver: &Arc<RecordingDriver>,
) -> SnowflakeConnectionFactory {
    let properties: JdbcProperties =
        HashMap::from([("test.property".to_string(), "test.value".to_string())]);
    let snowflake_driver: Arc<dyn SnowflakeDriver> = driver.clone();
    SnowflakeConnectionFactory::new_with_driver(
        DatabaseConnectionConfig::new("default", "snowflake", connection_string).unwrap(),
        Some(properties),
        DatabaseConnectionInfo::new(
            snowflake_constants::SNOWFLAKE_DRIVER_CLASS,
            snowflake_constants::SNOWFLAKE_DEFAULT_PORT,
        )
        .unwrap(),
        None,
        snowflake_driver,
    )
    .unwrap()
}

fn factory(driver: &Arc<RecordingDriver>) -> SnowflakeConnectionFactory {
    factory_for(CONNECTION_STRING, driver)
}

fn valid_key() -> SnowflakePrivateKey {
    SnowflakeAuthUtils::create_private_key(VALID_PRIVATE_KEY_PEM, None).unwrap()
}

async fn assert_fake_connection(connection: &dyn JdbcConnection) {
    assert_eq!(
        Some(CONNECTION_MARKER.to_string()),
        connection.get_catalog().await.unwrap()
    );
}

fn assert_key_pair_failure(error: &FederationError, cause: &str) {
    let sql_exception = SqlError::from_federation_error(error)
        .unwrap_or_else(|| panic!("expected a SqlError, got {error:?}"));
    assert_eq!(
        format!("Failed to create Snowflake connection with key-pair authentication: {cause}"),
        sql_exception.message()
    );
}

fn assert_illegal_argument(error: &FederationError, message: &str) {
    assert!(
        matches!(error, FederationError::IllegalArgument(actual) if actual == message),
        "expected IllegalArgument({message}), got {error:?}"
    );
}

#[tokio::test]
async fn test_get_connection_with_null_credentials_provider() {
    let driver = RecordingDriver::unreachable_host();
    let error = factory(&driver).get_connection(None).await.err().unwrap();
    assert_eq!(FAKE_HOST_FAILURE, error.to_string());
}

#[tokio::test]
async fn test_get_connection_with_key_pair_auth() {
    let driver = RecordingDriver::new();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);
    let connection = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .unwrap();
    assert_fake_connection(connection.as_ref()).await;

    let calls = driver.key_pair_calls();
    assert_eq!(1, calls.len());
    assert_eq!(CONNECTION_STRING, calls[0].url);
    assert_eq!(valid_key(), calls[0].private_key);
    assert_eq!(
        Some(&TESTUSER.to_string()),
        calls[0].properties.get(snowflake_constants::USER)
    );
    assert_eq!(
        Some(&"snowflake_jwt".to_string()),
        calls[0].properties.get(snowflake_constants::AUTHENTICATOR)
    );
    assert!(driver.pooled_calls().is_empty());
}

#[tokio::test]
async fn test_get_connection_with_key_pair_auth_using_username_field() {
    let driver = RecordingDriver::new();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);
    let connection = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .unwrap();
    assert_fake_connection(connection.as_ref()).await;

    let calls = driver.key_pair_calls();
    assert_eq!(1, calls.len());
    assert_eq!(
        Some(&TESTUSER.to_string()),
        calls[0].properties.get(snowflake_constants::USER)
    );
    assert!(driver.pooled_calls().is_empty());
}

#[tokio::test]
async fn test_get_connection_with_password_auth() {
    let driver = RecordingDriver::new();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    let connection = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .unwrap();
    assert_fake_connection(connection.as_ref()).await;

    let calls = driver.pooled_calls();
    assert_eq!(1, calls.len());
    assert_eq!(CONNECTION_STRING, calls[0].0);
    assert_eq!(
        Some(&TESTPASSWORD.to_string()),
        calls[0].1.get(snowflake_constants::PASSWORD)
    );
    assert!(driver.key_pair_calls().is_empty());
}

#[tokio::test]
async fn test_get_connection_with_o_auth_auth() {
    let driver = RecordingDriver::new();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::AUTH_CODE, "test-auth-code"),
    ]);
    let connection = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .unwrap();
    assert_fake_connection(connection.as_ref()).await;

    let calls = driver.pooled_calls();
    assert_eq!(1, calls.len());
    assert_eq!(
        Some(&"test-auth-code".to_string()),
        calls[0].1.get(snowflake_constants::AUTH_CODE)
    );
    assert!(driver.key_pair_calls().is_empty());
}

#[tokio::test]
async fn test_get_connection_with_key_pair_auth_and_private_key_creation_exception() {
    let driver = RecordingDriver::new();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (
            snowflake_constants::PEM_PRIVATE_KEY,
            "-----BEGIN PRIVATE KEY-----\nnot-a-private-key\n-----END PRIVATE KEY-----",
        ),
    ]);
    let error = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .err()
        .unwrap();
    assert_key_pair_failure(&error, "Invalid private key format");
    assert!(driver.key_pair_calls().is_empty());
    assert!(driver.pooled_calls().is_empty());
}

#[tokio::test]
async fn test_get_connection_with_key_pair_auth_and_generic_exception() {
    let driver = RecordingDriver::new();
    *driver.key_pair_error.lock().unwrap() = Some(SqlError::new("Generic error"));
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);
    let error = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .err()
        .unwrap();
    assert_key_pair_failure(&error, "Generic error");
    assert_eq!(1, driver.key_pair_calls().len());
}

#[tokio::test]
async fn test_build_connection_string_with_secret_placeholders() {
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);

    let driver = RecordingDriver::new();
    let connection = factory_for(
        "jdbc:snowflake://test.snowflakecomputing.com?secretName=test-secret",
        &driver,
    )
    .get_connection(Some(&provider))
    .await
    .unwrap();
    assert_fake_connection(connection.as_ref()).await;
    let calls = driver.key_pair_calls();
    assert_eq!(1, calls.len());
    assert_eq!(CONNECTION_STRING, calls[0].url);
    assert_eq!(
        Some(&"test-secret".to_string()),
        calls[0].properties.get("secretName")
    );

    let driver = RecordingDriver::new();
    factory_for(
        "jdbc:snowflake://test.snowflakecomputing.com?${test-secret}&warehouse=wh",
        &driver,
    )
    .get_connection(Some(&provider))
    .await
    .unwrap();
    let calls = driver.key_pair_calls();
    assert_eq!(1, calls.len());
    assert_eq!(CONNECTION_STRING, calls[0].url);
    assert_eq!(
        Some(&"wh".to_string()),
        calls[0].properties.get("warehouse")
    );
    assert!(calls[0]
        .properties
        .iter()
        .all(|(key, value)| !key.contains("${") && !value.contains("${")));
}

#[tokio::test]
async fn test_get_connection_with_key_pair_auth_and_missing_username() {
    let driver = RecordingDriver::new();
    let factory = factory(&driver);

    let empty = MapCredentialsProvider::new(&[]);
    let error = factory.get_connection(Some(&empty)).await.err().unwrap();
    assert_illegal_argument(&error, "Credentials cannot be null or empty");

    let provider = MapCredentialsProvider::new(&[(
        snowflake_constants::PEM_PRIVATE_KEY,
        VALID_PRIVATE_KEY_PEM,
    )]);
    let error = factory.get_connection(Some(&provider)).await.err().unwrap();
    assert_key_pair_failure(&error, "null");
    assert!(driver.key_pair_calls().is_empty());
    assert!(driver.pooled_calls().is_empty());
}

#[tokio::test]
async fn test_get_connection_with_key_pair_auth_and_missing_private_key() {
    let driver = RecordingDriver::new();
    let factory = factory(&driver);

    let empty = MapCredentialsProvider::new(&[]);
    let error = factory.get_connection(Some(&empty)).await.err().unwrap();
    assert_illegal_argument(&error, "Credentials cannot be null or empty");
    assert!(driver.key_pair_calls().is_empty());
    assert!(driver.pooled_calls().is_empty());

    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, " "),
    ]);
    factory.get_connection(Some(&provider)).await.unwrap();
    assert!(driver.key_pair_calls().is_empty());
    assert_eq!(1, driver.pooled_calls().len());
}

#[tokio::test]
async fn test_get_connection_with_encrypted_key_pair_auth() {
    let driver = RecordingDriver::new();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (
            snowflake_constants::PEM_PRIVATE_KEY,
            ENCRYPTED_PRIVATE_KEY_PEM_PBES2,
        ),
        (snowflake_constants::PEM_PRIVATE_KEY_PASSPHRASE, PASSPHRASE),
    ]);
    let connection = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .unwrap();
    assert_fake_connection(connection.as_ref()).await;

    let calls = driver.key_pair_calls();
    assert_eq!(1, calls.len());
    assert_eq!(valid_key(), calls[0].private_key);
    assert!(!calls[0]
        .properties
        .contains_key(snowflake_constants::PEM_PRIVATE_KEY_PASSPHRASE));
}

#[tokio::test]
async fn get_connection_password_auth_uses_pool_not_driver_manager() {
    let driver = RecordingDriver::unreachable_host();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USERNAME, TESTUSER),
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    let error = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .err()
        .unwrap();
    assert_eq!(FAKE_HOST_FAILURE, error.to_string());
    assert_eq!(1, driver.pooled_calls().len());
    assert!(driver.key_pair_calls().is_empty());
}

#[tokio::test]
async fn get_connection_oauth_auth_uses_pool_not_driver_manager() {
    let driver = RecordingDriver::unreachable_host();
    let provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::AUTH_CODE, "test-auth-code"),
        ("token", "some-oauth-token"),
    ]);
    let error = factory(&driver)
        .get_connection(Some(&provider))
        .await
        .err()
        .unwrap();
    assert_eq!(FAKE_HOST_FAILURE, error.to_string());
    assert_eq!(1, driver.pooled_calls().len());
    assert!(driver.key_pair_calls().is_empty());
}

#[tokio::test]
async fn get_connection_null_provider_uses_pool_not_driver_manager() {
    let driver = RecordingDriver::unreachable_host();
    let error = factory(&driver).get_connection(None).await.err().unwrap();
    assert_eq!(FAKE_HOST_FAILURE, error.to_string());
    let calls = driver.pooled_calls();
    assert_eq!(1, calls.len());
    assert_eq!(CONNECTION_STRING, calls[0].0);
    assert!(driver.key_pair_calls().is_empty());
}

#[tokio::test]
async fn get_connection_key_pair_after_password_does_not_send_earlier_password() {
    let driver = RecordingDriver::unreachable_host();
    let factory = factory(&driver);
    let password_provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, "otheruser"),
        (snowflake_constants::PASSWORD, TESTPASSWORD),
    ]);
    let key_pair_provider = MapCredentialsProvider::new(&[
        (snowflake_constants::USER, TESTUSER),
        (snowflake_constants::PEM_PRIVATE_KEY, VALID_PRIVATE_KEY_PEM),
    ]);

    assert!(factory
        .get_connection(Some(&password_provider))
        .await
        .is_err());
    assert_eq!(
        Some(&TESTPASSWORD.to_string()),
        factory
            .generic()
            .jdbc_properties()
            .get(snowflake_constants::PASSWORD)
    );
    factory
        .get_connection(Some(&key_pair_provider))
        .await
        .unwrap();

    let calls = driver.key_pair_calls();
    assert_eq!(1, calls.len());
    let properties = &calls[0].properties;
    assert!(!properties.contains_key(snowflake_constants::PASSWORD));
    assert_eq!(
        Some(&TESTUSER.to_string()),
        properties.get(snowflake_constants::USER)
    );
    assert_eq!(
        Some(&"test.value".to_string()),
        properties.get("test.property")
    );
}
