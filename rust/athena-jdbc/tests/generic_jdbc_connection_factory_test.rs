mod jdbc_support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::credentials::{DefaultCredentials, StaticCredentialsProvider};
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, GenericJdbcConnectionFactory,
    JdbcConnectionFactory, JdbcDriver, JdbcProperties,
};
use athena_jdbc::sql::{JdbcConnection, SqlError};
use jdbc_support::FakeConnection;

const H2_DRIVER_CLASS: &str = "org.h2.Driver";
const H2_DEFAULT_PORT: i32 = 9092;
const TEST_CATALOG: &str = "testCatalog";
const TEST_ENGINE: &str = "h2";
const TEST_SECRET_NAME: &str = "testSecret";

#[derive(Default)]
struct DriverState {
    calls: Vec<(String, JdbcProperties)>,
    owner: Option<String>,
    error: Option<SqlError>,
    opened: usize,
}

#[derive(Clone)]
struct FakeDriver {
    class_name: String,
    state: Arc<Mutex<DriverState>>,
}

impl FakeDriver {
    fn new() -> FakeDriver {
        FakeDriver::with_class_name(H2_DRIVER_CLASS)
    }

    fn with_class_name(class_name: &str) -> FakeDriver {
        FakeDriver {
            class_name: class_name.to_string(),
            state: Arc::new(Mutex::new(DriverState::default())),
        }
    }

    fn failing(error: SqlError) -> FakeDriver {
        let driver = FakeDriver::new();
        driver.state.lock().unwrap().error = Some(error);
        driver
    }

    fn calls(&self) -> Vec<(String, JdbcProperties)> {
        self.state.lock().unwrap().calls.clone()
    }
}

#[async_trait]
impl JdbcDriver for FakeDriver {
    fn driver_class_name(&self) -> &str {
        &self.class_name
    }

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push((url.to_string(), properties.clone()));
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        let user = properties.get("user").map(|u| u.to_uppercase());
        match (&state.owner, &user) {
            (None, Some(user)) => state.owner = Some(user.clone()),
            (Some(owner), Some(user)) if owner != user => {
                return Err(SqlError::new("Wrong user name or password [28000-224]"));
            }
            _ => {}
        }
        state.opened += 1;
        let connection = FakeConnection::new();
        *connection.state.schema.lock().unwrap() =
            Some(format!("{}-conn{}", self.class_name, state.opened));
        *connection.state.catalog.lock().unwrap() = user;
        Ok(Box::new(connection))
    }
}

fn unique_h2_url() -> String {
    format!("jdbc:h2:mem:{};DB_CLOSE_DELAY=-1", uuid_like())
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    format!("db_{}", NEXT.fetch_add(1, Ordering::SeqCst))
}

fn info(driver_class: &str) -> DatabaseConnectionInfo {
    DatabaseConnectionInfo::new(driver_class, H2_DEFAULT_PORT).unwrap()
}

fn config(url: &str) -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new(TEST_CATALOG, TEST_ENGINE, url).unwrap()
}

fn config_with_secret(url: &str) -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new_with_secret(TEST_CATALOG, TEST_ENGINE, url, TEST_SECRET_NAME)
        .unwrap()
}

fn pooled_factory(
    config: DatabaseConnectionConfig,
    driver: &FakeDriver,
) -> GenericJdbcConnectionFactory {
    GenericJdbcConnectionFactory::new(
        config,
        Some(HashMap::new()),
        info(H2_DRIVER_CLASS),
        Arc::new(driver.clone()),
    )
    .unwrap()
}

fn fas_options() -> HashMap<String, String> {
    HashMap::from([(
        environment_constants::FAS_TOKEN.to_string(),
        "someToken".to_string(),
    )])
}

fn direct_factory(url: &str, driver: &FakeDriver) -> GenericJdbcConnectionFactory {
    GenericJdbcConnectionFactory::new_with_config_options(
        config(url),
        Some(HashMap::new()),
        info(H2_DRIVER_CLASS),
        Some(&fas_options()),
        Arc::new(driver.clone()),
    )
    .unwrap()
}

fn credentials(user: &str, password: &str) -> StaticCredentialsProvider {
    StaticCredentialsProvider::new(DefaultCredentials::new(user, password).unwrap()).unwrap()
}

#[test]
fn match_secret_name_pattern() {
    let jdbc_connection_string = "mysql://jdbc:mysql://mysql.host:3333/default?${secret!@+=_}";
    assert!(GenericJdbcConnectionFactory::secret_name_pattern().is_match(jdbc_connection_string));
}

#[test]
fn match_incorrect_secret_name_pattern() {
    let jdbc_connection_string = "mysql://jdbc:mysql://mysql.host:3333/default?${secret!@+=*_}";
    assert!(!GenericJdbcConnectionFactory::secret_name_pattern().is_match(jdbc_connection_string));
}

#[tokio::test]
async fn get_connection_pooled_mode_with_credentials_strips_secret_and_returns_connection() {
    let base_url = format!(
        "{};INIT=CREATE SCHEMA IF NOT EXISTS TEST\\;--",
        unique_h2_url()
    );
    let raw_url = format!("{base_url}${{{TEST_SECRET_NAME}}}");
    let driver = FakeDriver::new();
    let factory = pooled_factory(config_with_secret(&raw_url), &driver);
    let provider = credentials("sa", "sa");
    {
        let connection = factory.get_connection(Some(&provider)).await.unwrap();
        assert!(connection.is_valid(1).await.unwrap());
    }
    factory.close().await.unwrap();
    let calls = driver.calls();
    assert_eq!(1, calls.len());
    assert_eq!(base_url, calls[0].0);
    assert_eq!(Some(&"sa".to_string()), calls[0].1.get("user"));
}

#[tokio::test]
async fn get_connection_pooled_mode_null_credentials_uses_jdbc_string_as_is() {
    let raw_url = unique_h2_url();
    let driver = FakeDriver::new();
    let factory = pooled_factory(config(&raw_url), &driver);
    {
        let connection = factory.get_connection(None).await.unwrap();
        assert!(connection.is_valid(1).await.unwrap());
    }
    factory.close().await.unwrap();
    assert_eq!(raw_url, driver.calls()[0].0);
}

#[tokio::test]
async fn constructor_with_null_properties_is_accepted_and_pool_initializes() {
    let driver = FakeDriver::new();
    let factory = GenericJdbcConnectionFactory::new(
        config(&unique_h2_url()),
        None,
        info(H2_DRIVER_CLASS),
        Arc::new(driver.clone()),
    )
    .unwrap();
    {
        let connection = factory.get_connection(None).await;
        assert!(connection.is_ok());
    }
    factory.close().await.unwrap();
}

#[tokio::test]
async fn close_before_any_connection_is_no_op() {
    let driver = FakeDriver::new();
    let factory = pooled_factory(config(&unique_h2_url()), &driver);
    factory.close().await.unwrap();
    assert!(driver.calls().is_empty());
}

#[tokio::test]
async fn close_after_pool_initialized_closes_pool_and_allows_reuse() {
    let driver = FakeDriver::new();
    let factory = pooled_factory(config(&unique_h2_url()), &driver);
    {
        let connection = factory.get_connection(None).await;
        assert!(connection.is_ok());
    }
    factory.close().await.unwrap();
    {
        let reopened = factory.get_connection(None).await.unwrap();
        assert!(reopened.is_valid(2).await.unwrap());
    }
    factory.close().await.unwrap();
    assert_eq!(2, driver.calls().len());
}

#[tokio::test]
async fn close_is_idempotent() {
    let driver = FakeDriver::new();
    let factory = pooled_factory(config(&unique_h2_url()), &driver);
    {
        let connection = factory.get_connection(None).await;
        assert!(connection.is_ok());
    }
    factory.close().await.unwrap();
    factory.close().await.unwrap();
}

#[tokio::test]
async fn get_connection_pooled_mode_rebuilds_pool_when_credentials_change() {
    let driver = FakeDriver::new();
    let factory = pooled_factory(config_with_secret(&unique_h2_url()), &driver);
    let tenant_a = credentials("tenantA", "secretA");
    let tenant_b = credentials("tenantB", "secretB");
    {
        let first = factory.get_connection(Some(&tenant_a)).await.unwrap();
        assert_eq!(
            Some("TENANTA".to_string()),
            first.get_catalog().await.unwrap()
        );
    }
    let failure = factory.get_connection(Some(&tenant_b)).await.err().unwrap();
    assert!(
        failure.to_string().contains("Wrong user name or password"),
        "expected an authentication failure for the new identity, got: {failure}"
    );
    factory.close().await.unwrap();
}

#[tokio::test]
async fn get_connection_pooled_mode_reuses_pool_for_same_credentials() {
    let driver = FakeDriver::new();
    let factory = pooled_factory(config(&unique_h2_url()), &driver);
    let first_pool = {
        let first = factory.get_connection(None).await.unwrap();
        first.get_schema().await.unwrap().unwrap()
    };
    {
        let second = factory.get_connection(None).await.unwrap();
        assert_eq!(first_pool, second.get_schema().await.unwrap().unwrap());
    }
    factory.close().await.unwrap();
    assert_eq!(1, driver.calls().len());
}

#[tokio::test]
async fn get_connection_pooled_mode_distinct_factories_do_not_block_each_other() {
    let driver_one = FakeDriver::new();
    let driver_two = FakeDriver::with_class_name(H2_DRIVER_CLASS);
    let one = pooled_factory(config(&unique_h2_url()), &driver_one);
    let two = GenericJdbcConnectionFactory::new(
        DatabaseConnectionConfig::new("otherCatalog", TEST_ENGINE, &unique_h2_url()).unwrap(),
        Some(HashMap::new()),
        info(H2_DRIVER_CLASS),
        Arc::new(driver_two.clone()),
    )
    .unwrap();
    {
        let (a, b) = tokio::join!(one.get_connection(None), two.get_connection(None));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_ne!(driver_one.calls()[0].0, driver_two.calls()[0].0);
        assert!(a.is_valid(1).await.unwrap());
        assert!(b.is_valid(1).await.unwrap());
    }
    one.close().await.unwrap();
    two.close().await.unwrap();
}

#[test]
fn constructor_direct_mode_missing_driver_throws_athena_connector_exception() {
    let driver = FakeDriver::new();
    let result = GenericJdbcConnectionFactory::new_with_config_options(
        config(&unique_h2_url()),
        Some(HashMap::new()),
        info("not.a.real.jdbc.Driver"),
        Some(&fas_options()),
        Arc::new(driver),
    );
    let ex = result.err().unwrap();
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidInputException),
        ex.error_code()
    );
    assert!(ex.to_string().contains("JDBC driver not found"));
}

#[test]
fn constructor_pooled_mode_no_fas_token_does_not_attempt_driver_load() {
    let driver = FakeDriver::new();
    let factory = GenericJdbcConnectionFactory::new_with_config_options(
        config(&unique_h2_url()),
        Some(HashMap::new()),
        info("not.a.real.jdbc.Driver"),
        Some(&HashMap::new()),
        Arc::new(driver),
    );
    assert!(factory.is_ok());
}

#[tokio::test]
async fn get_connection_direct_mode_with_credentials_strips_secret_and_merges_properties() {
    let raw_url = format!("jdbc:h2:mem:direct?param=${{{TEST_SECRET_NAME}}}");
    let driver = FakeDriver::new();
    let factory = GenericJdbcConnectionFactory::new_with_config_options(
        config_with_secret(&raw_url),
        Some(HashMap::from([(
            "existing".to_string(),
            "value".to_string(),
        )])),
        info(H2_DRIVER_CLASS),
        Some(&fas_options()),
        Arc::new(driver.clone()),
    )
    .unwrap();
    let provider = credentials("sa", "sa");
    let result = factory.get_connection(Some(&provider)).await;
    assert!(result.is_ok());
    let calls = driver.calls();
    assert_eq!(1, calls.len());
    assert_eq!("jdbc:h2:mem:direct?param=", calls[0].0);
    let actual_props = &calls[0].1;
    assert_eq!(Some(&"value".to_string()), actual_props.get("existing"));
    assert_eq!(Some(&"sa".to_string()), actual_props.get("user"));
    assert_eq!(Some(&"sa".to_string()), actual_props.get("password"));
}

#[tokio::test]
async fn get_connection_direct_mode_null_credentials_passes_url_through() {
    let raw_url = unique_h2_url();
    let driver = FakeDriver::new();
    let factory = direct_factory(&raw_url, &driver);
    let result = factory.get_connection(None).await;
    assert!(result.is_ok());
    assert_eq!(raw_url, driver.calls()[0].0);
}

#[tokio::test]
async fn get_connection_direct_mode_name_or_service_not_known_wraps_as_invalid_input_exception() {
    let driver = FakeDriver::failing(SqlError::new(
        "Communications link failure: Name or service not known",
    ));
    let factory = direct_factory(&unique_h2_url(), &driver);
    let ex = factory.get_connection(None).await.err().unwrap();
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidInputException),
        ex.error_code()
    );
}

#[tokio::test]
async fn get_connection_direct_mode_incorrect_credentials_wraps_as_invalid_credentials_exception() {
    let driver = FakeDriver::failing(SqlError::new(
        "Incorrect username or password was specified.",
    ));
    let factory = direct_factory(&unique_h2_url(), &driver);
    let ex = factory.get_connection(None).await.err().unwrap();
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidCredentialsException),
        ex.error_code()
    );
}

#[tokio::test]
async fn get_connection_direct_mode_generic_sql_exception_propagates() {
    let expected = SqlError::new("Some other database error");
    let driver = FakeDriver::failing(expected.clone());
    let factory = direct_factory(&unique_h2_url(), &driver);
    let ex = factory.get_connection(None).await.err().unwrap();
    assert_eq!(Some(&expected), SqlError::from_federation_error(&ex));
}
