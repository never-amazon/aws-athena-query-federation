use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};

use async_trait::async_trait;
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::{FederationError, Result};
use regex::Regex;

use super::database_connection_config::DatabaseConnectionConfig;
use super::database_connection_info::DatabaseConnectionInfo;
use super::jdbc_connection_factory::JdbcConnectionFactory;
use crate::sql::{
    DatabaseMetaData, JdbcConnection, PreparedStatement, ResultSet, ResultSetMetaData, SqlException,
};

pub type JdbcProperties = HashMap<String, String>;

const MAXIMUM_POOL_SIZE: usize = 10;
const VALIDATION_TIMEOUT_SECONDS: i32 = 5;

static SECRET_NAME_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\$\{[a-zA-Z0-9:/_+=.@!-]+\})").expect("the pattern is a valid regex")
});

#[async_trait]
pub trait JdbcDriver: Send + Sync {
    fn driver_class_name(&self) -> &str;

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
    ) -> std::result::Result<Box<dyn JdbcConnection>, SqlException>;
}

#[derive(Default)]
struct PoolState {
    generation: u64,
    identity: Option<String>,
    idle: Vec<Arc<dyn JdbcConnection>>,
}

#[derive(Default)]
struct ConnectionPool {
    state: Mutex<PoolState>,
}

impl ConnectionPool {
    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub struct GenericJdbcConnectionFactory {
    database_connection_config: DatabaseConnectionConfig,
    jdbc_properties: Mutex<JdbcProperties>,
    database_connection_info: DatabaseConnectionInfo,
    use_direct_connection: bool,
    driver: Arc<dyn JdbcDriver>,
    pool: Arc<ConnectionPool>,
}

impl GenericJdbcConnectionFactory {
    pub fn new(
        database_connection_config: DatabaseConnectionConfig,
        properties: Option<JdbcProperties>,
        database_connection_info: DatabaseConnectionInfo,
        driver: Arc<dyn JdbcDriver>,
    ) -> Result<GenericJdbcConnectionFactory> {
        GenericJdbcConnectionFactory::new_with_config_options(
            database_connection_config,
            properties,
            database_connection_info,
            None,
            driver,
        )
    }

    pub fn new_with_config_options(
        database_connection_config: DatabaseConnectionConfig,
        properties: Option<JdbcProperties>,
        database_connection_info: DatabaseConnectionInfo,
        config_options: Option<&HashMap<String, String>>,
        driver: Arc<dyn JdbcDriver>,
    ) -> Result<GenericJdbcConnectionFactory> {
        let use_direct_connection =
            config_options.is_some_and(|o| o.contains_key(environment_constants::FAS_TOKEN));
        if use_direct_connection {
            tracing::info!("Glue managed connection detected, using direct JDBC connections");
            if driver.driver_class_name() != database_connection_info.driver_class_name() {
                return Err(FederationError::invalid_input(format!(
                    "JDBC driver not found: {}",
                    database_connection_info.driver_class_name()
                )));
            }
        }
        Ok(GenericJdbcConnectionFactory {
            database_connection_config,
            jdbc_properties: Mutex::new(properties.unwrap_or_default()),
            database_connection_info,
            use_direct_connection,
            driver,
            pool: Arc::new(ConnectionPool::default()),
        })
    }

    pub fn secret_name_pattern() -> &'static Regex {
        &SECRET_NAME_PATTERN
    }

    pub fn database_connection_config(&self) -> &DatabaseConnectionConfig {
        &self.database_connection_config
    }

    pub fn database_connection_info(&self) -> &DatabaseConnectionInfo {
        &self.database_connection_info
    }

    pub fn jdbc_properties(&self) -> JdbcProperties {
        self.properties().clone()
    }

    fn properties(&self) -> MutexGuard<'_, JdbcProperties> {
        self.jdbc_properties
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub async fn get_connection_from_managed_pool(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let derived_jdbc_string = self.get_derived_jdbc_string(credentials_provider).await?;
        let properties = self.jdbc_properties();
        let pool_identity = build_pool_identity(&derived_jdbc_string, &properties);
        let (generation, idle, replaced) = {
            let mut state = self.pool.lock();
            let mut replaced = Vec::new();
            if state.identity.as_deref() != Some(pool_identity.as_str()) {
                if state.identity.is_some() {
                    tracing::debug!("Connection identity changed; replacing existing data source");
                }
                replaced = std::mem::take(&mut state.idle);
                state.generation += 1;
                state.identity = Some(pool_identity);
                tracing::debug!("Create data source");
            }
            (state.generation, state.idle.pop(), replaced)
        };
        close_all(replaced).await;
        if let Some(connection) = idle {
            if let Some(connection) = reset_for_reuse(connection).await {
                return Ok(self.pooled(connection, generation));
            }
        }
        let connection = self
            .driver
            .connect(&derived_jdbc_string, &properties)
            .await
            .map_err(handle_sql_exception_when_get_connection)?;
        Ok(self.pooled(Arc::from(connection), generation))
    }

    fn pooled(
        &self,
        connection: Arc<dyn JdbcConnection>,
        generation: u64,
    ) -> Box<dyn JdbcConnection> {
        Box::new(PooledConnection {
            inner: connection,
            pool: Arc::downgrade(&self.pool),
            generation,
        })
    }

    async fn create_direct_connection(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let derived_jdbc_string = self.get_derived_jdbc_string(credentials_provider).await?;
        let connection_props = self.jdbc_properties();
        self.driver
            .connect(&derived_jdbc_string, &connection_props)
            .await
            .map_err(handle_sql_exception_when_get_connection)
    }

    async fn get_derived_jdbc_string(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> Result<String> {
        let connection_string = self.database_connection_config.jdbc_connection_string();
        match credentials_provider {
            Some(provider) => {
                let derived = SECRET_NAME_PATTERN
                    .replace_all(connection_string, "")
                    .into_owned();
                let credential_map = provider.get_credential_map().await?;
                self.properties().extend(credential_map);
                Ok(derived)
            }
            None => Ok(connection_string.to_string()),
        }
    }
}

fn build_pool_identity(derived_jdbc_string: &str, properties: &JdbcProperties) -> String {
    let sorted: BTreeMap<&String, &String> = properties.iter().collect();
    let mut identity = derived_jdbc_string.to_string();
    for (key, value) in sorted {
        identity.push('\n');
        identity.push_str(key);
        identity.push('=');
        identity.push_str(value);
    }
    identity
}

fn handle_sql_exception_when_get_connection(e: SqlException) -> FederationError {
    let message = e.message();
    if !message.trim().is_empty() && message.contains("Name or service not known") {
        return FederationError::invalid_input(message);
    }
    if !message.trim().is_empty()
        && message.contains("Incorrect username or password was specified.")
    {
        return FederationError::connector(
            FederationSourceErrorCode::InvalidCredentialsException,
            message,
        );
    }
    FederationError::from(e)
}

async fn close_all(connections: Vec<Arc<dyn JdbcConnection>>) {
    for connection in connections {
        if let Err(e) = connection.close().await {
            tracing::debug!("Failed to close pooled connection: {}", e);
        }
    }
}

async fn reset_for_reuse(connection: Arc<dyn JdbcConnection>) -> Option<Arc<dyn JdbcConnection>> {
    let reset = async {
        if !connection.is_valid(VALIDATION_TIMEOUT_SECONDS).await? {
            return Ok(false);
        }
        if !connection.get_auto_commit().await? {
            connection.rollback().await?;
            connection.set_auto_commit(true).await?;
        }
        Ok::<bool, SqlException>(true)
    }
    .await;
    match reset {
        Ok(true) => Some(connection),
        Ok(false) => {
            close_all(vec![connection]).await;
            None
        }
        Err(e) => {
            tracing::debug!("Discarding pooled connection that failed validation: {}", e);
            close_all(vec![connection]).await;
            None
        }
    }
}

#[async_trait]
impl JdbcConnectionFactory for GenericJdbcConnectionFactory {
    async fn get_connection(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> Result<Box<dyn JdbcConnection>> {
        if self.use_direct_connection {
            return self.create_direct_connection(credentials_provider).await;
        }
        self.get_connection_from_managed_pool(credentials_provider)
            .await
    }

    async fn close(&self) -> Result<()> {
        let stale = {
            let mut state = self.pool.lock();
            state.generation += 1;
            state.identity = None;
            std::mem::take(&mut state.idle)
        };
        close_all(stale).await;
        Ok(())
    }
}

struct PooledConnection {
    inner: Arc<dyn JdbcConnection>,
    pool: Weak<ConnectionPool>,
    generation: u64,
}

impl PooledConnection {
    fn inner(&self) -> &dyn JdbcConnection {
        self.inner.as_ref()
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        let connection = Arc::clone(&self.inner);
        if let Some(pool) = self.pool.upgrade() {
            let mut state = pool.lock();
            if state.generation == self.generation
                && state.identity.is_some()
                && state.idle.len() < MAXIMUM_POOL_SIZE
            {
                state.idle.push(connection);
            }
        }
    }
}

#[async_trait]
impl JdbcConnection for PooledConnection {
    fn metadata(&self) -> &dyn DatabaseMetaData {
        self.inner().metadata()
    }

    async fn get_catalog(&self) -> std::result::Result<Option<String>, SqlException> {
        self.inner().get_catalog().await
    }

    async fn get_schema(&self) -> std::result::Result<Option<String>, SqlException> {
        self.inner().get_schema().await
    }

    async fn prepare_statement(
        &self,
        sql: &str,
    ) -> std::result::Result<PreparedStatement, SqlException> {
        self.inner().prepare_statement(sql).await
    }

    async fn execute_query(
        &self,
        statement: &PreparedStatement,
    ) -> std::result::Result<ResultSet, SqlException> {
        self.inner().execute_query(statement).await
    }

    async fn execute(
        &self,
        statement: &PreparedStatement,
    ) -> std::result::Result<bool, SqlException> {
        self.inner().execute(statement).await
    }

    async fn execute_query_with_sql(
        &self,
        sql: &str,
    ) -> std::result::Result<ResultSet, SqlException> {
        self.inner().execute_query_with_sql(sql).await
    }

    async fn statement_metadata(
        &self,
        statement: &PreparedStatement,
    ) -> std::result::Result<Option<Arc<ResultSetMetaData>>, SqlException> {
        self.inner().statement_metadata(statement).await
    }

    async fn parameter_count(
        &self,
        statement: &PreparedStatement,
    ) -> std::result::Result<Option<usize>, SqlException> {
        self.inner().parameter_count(statement).await
    }

    async fn set_auto_commit(&self, auto_commit: bool) -> std::result::Result<(), SqlException> {
        self.inner().set_auto_commit(auto_commit).await
    }

    async fn get_auto_commit(&self) -> std::result::Result<bool, SqlException> {
        self.inner().get_auto_commit().await
    }

    async fn commit(&self) -> std::result::Result<(), SqlException> {
        self.inner().commit().await
    }

    async fn rollback(&self) -> std::result::Result<(), SqlException> {
        self.inner().rollback().await
    }

    async fn is_valid(&self, timeout_seconds: i32) -> std::result::Result<bool, SqlException> {
        self.inner().is_valid(timeout_seconds).await
    }

    async fn close(&self) -> std::result::Result<(), SqlException> {
        Ok(())
    }
}
