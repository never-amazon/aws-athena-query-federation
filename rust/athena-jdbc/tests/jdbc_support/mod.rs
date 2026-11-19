#![allow(
    dead_code,
    reason = "each test binary uses a different subset of these fakes"
)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::data::{GetObjectRequest, PutObjectRequest, S3Client};
use athena_federation_sdk::handlers::AwsClients;
use athena_federation_sdk::security::{
    AwsCrypto, AwsRequestOverrideConfiguration, AwsServiceError, KmsClient, KmsEncryptionProvider,
    SecretsManagerClient,
};
use athena_federation_sdk::AthenaClient;
use athena_jdbc::connection::JdbcConnectionFactory;
use athena_jdbc::sql::{
    ColumnMetadata, DatabaseMetaData, JdbcConnection, JdbcType, PreparedStatement, ResultSet,
    ResultSetMetaData, SqlError, SqlValue,
};
use aws_credential_types::Credentials;
use aws_sdk_kms::types::DataKeySpec;

pub const FAS_ACCESS: &str = "fas-access";

#[derive(Debug, Clone, PartialEq)]
pub struct ResultSetData {
    pub metadata: ResultSetMetaData,
    pub rows: Vec<Vec<SqlValue>>,
}

impl ResultSetData {
    pub fn new(metadata: ResultSetMetaData, rows: Vec<Vec<SqlValue>>) -> ResultSetData {
        ResultSetData { metadata, rows }
    }

    pub fn of_strings(labels: &[&str], rows: &[&[&str]]) -> ResultSetData {
        ResultSetData {
            metadata: ResultSetMetaData::of_labels(labels.iter().copied()),
            rows: rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|v| SqlValue::String((*v).to_string()))
                        .collect()
                })
                .collect(),
        }
    }

    pub fn of_columns(columns: Vec<ColumnMetadata>, rows: Vec<Vec<SqlValue>>) -> ResultSetData {
        ResultSetData {
            metadata: ResultSetMetaData::new(columns),
            rows,
        }
    }

    pub fn empty(labels: &[&str]) -> ResultSetData {
        ResultSetData::of_strings(labels, &[])
    }

    pub fn to_result_set(&self) -> ResultSet {
        ResultSet::from_rows(self.metadata.clone(), self.rows.clone())
    }
}

pub type MetaResult = Result<ResultSetData, SqlError>;

pub type QueryHandler =
    Arc<dyn Fn(&PreparedStatement) -> Result<ResultSetData, SqlError> + Send + Sync>;

pub type PrepareHandler = Arc<dyn Fn(&str) -> Result<PreparedStatement, SqlError> + Send + Sync>;

#[derive(Debug, Clone, PartialEq)]
pub struct GetTablesCall {
    pub catalog: Option<String>,
    pub schema_pattern: Option<String>,
    pub table_name_pattern: Option<String>,
    pub types: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GetColumnsCall {
    pub catalog: Option<String>,
    pub schema_pattern: Option<String>,
    pub table_name_pattern: Option<String>,
    pub column_name_pattern: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GetPrimaryKeysCall {
    pub catalog: Option<String>,
    pub schema: Option<String>,
    pub table: String,
}

#[derive(Default)]
pub struct MetaDataState {
    pub schemas: Mutex<Option<MetaResult>>,
    pub tables: Mutex<Option<MetaResult>>,
    pub columns: Mutex<Option<MetaResult>>,
    pub primary_keys: Mutex<Option<MetaResult>>,
    pub search_string_escape: Mutex<Option<String>>,
    pub identifier_quote_string: Mutex<String>,
    pub database_product_name: Mutex<String>,
    pub user_name: Mutex<Option<String>>,
    pub get_tables_calls: Mutex<Vec<GetTablesCall>>,
    pub get_columns_calls: Mutex<Vec<GetColumnsCall>>,
    pub get_primary_keys_calls: Mutex<Vec<GetPrimaryKeysCall>>,
    pub get_schemas_calls: AtomicUsize,
}

#[derive(Clone, Default)]
pub struct FakeDatabaseMetaData {
    pub state: Arc<MetaDataState>,
}

impl FakeDatabaseMetaData {
    pub fn set_schemas(&self, result: MetaResult) {
        *self.state.schemas.lock().unwrap() = Some(result);
    }

    pub fn set_tables(&self, result: MetaResult) {
        *self.state.tables.lock().unwrap() = Some(result);
    }

    pub fn set_columns(&self, result: MetaResult) {
        *self.state.columns.lock().unwrap() = Some(result);
    }

    pub fn set_primary_keys(&self, result: MetaResult) {
        *self.state.primary_keys.lock().unwrap() = Some(result);
    }

    pub fn set_search_string_escape(&self, escape: Option<&str>) {
        *self.state.search_string_escape.lock().unwrap() = escape.map(ToString::to_string);
    }

    pub fn set_database_product_name(&self, name: &str) {
        *self.state.database_product_name.lock().unwrap() = name.to_string();
    }

    pub fn get_tables_calls(&self) -> Vec<GetTablesCall> {
        self.state.get_tables_calls.lock().unwrap().clone()
    }

    pub fn get_columns_calls(&self) -> Vec<GetColumnsCall> {
        self.state.get_columns_calls.lock().unwrap().clone()
    }

    pub fn get_primary_keys_calls(&self) -> Vec<GetPrimaryKeysCall> {
        self.state.get_primary_keys_calls.lock().unwrap().clone()
    }

    fn take(slot: &Mutex<Option<MetaResult>>, labels: &[&str]) -> Result<ResultSet, SqlError> {
        match slot.lock().unwrap().as_ref() {
            Some(Ok(data)) => Ok(data.to_result_set()),
            Some(Err(e)) => Err(e.clone()),
            None => Ok(ResultSetData::empty(labels).to_result_set()),
        }
    }
}

#[async_trait]
impl DatabaseMetaData for FakeDatabaseMetaData {
    async fn get_schemas(&self) -> Result<ResultSet, SqlError> {
        self.state.get_schemas_calls.fetch_add(1, Ordering::SeqCst);
        FakeDatabaseMetaData::take(&self.state.schemas, &["TABLE_SCHEM", "TABLE_CATALOG"])
    }

    async fn get_schemas_with_catalog(
        &self,
        _catalog: Option<&str>,
        _schema_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlError> {
        self.get_schemas().await
    }

    async fn get_tables(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        types: Option<&[&str]>,
    ) -> Result<ResultSet, SqlError> {
        self.state
            .get_tables_calls
            .lock()
            .unwrap()
            .push(GetTablesCall {
                catalog: catalog.map(ToString::to_string),
                schema_pattern: schema_pattern.map(ToString::to_string),
                table_name_pattern: table_name_pattern.map(ToString::to_string),
                types: types.map(|t| t.iter().map(ToString::to_string).collect()),
            });
        FakeDatabaseMetaData::take(&self.state.tables, &["TABLE_NAME", "TABLE_SCHEM"])
    }

    async fn get_columns(
        &self,
        catalog: Option<&str>,
        schema_pattern: Option<&str>,
        table_name_pattern: Option<&str>,
        column_name_pattern: Option<&str>,
    ) -> Result<ResultSet, SqlError> {
        self.state
            .get_columns_calls
            .lock()
            .unwrap()
            .push(GetColumnsCall {
                catalog: catalog.map(ToString::to_string),
                schema_pattern: schema_pattern.map(ToString::to_string),
                table_name_pattern: table_name_pattern.map(ToString::to_string),
                column_name_pattern: column_name_pattern.map(ToString::to_string),
            });
        FakeDatabaseMetaData::take(
            &self.state.columns,
            &[
                "DATA_TYPE",
                "COLUMN_SIZE",
                "DECIMAL_DIGITS",
                "COLUMN_NAME",
                "TYPE_NAME",
            ],
        )
    }

    async fn get_primary_keys(
        &self,
        catalog: Option<&str>,
        schema: Option<&str>,
        table: &str,
    ) -> Result<ResultSet, SqlError> {
        self.state
            .get_primary_keys_calls
            .lock()
            .unwrap()
            .push(GetPrimaryKeysCall {
                catalog: catalog.map(ToString::to_string),
                schema: schema.map(ToString::to_string),
                table: table.to_string(),
            });
        FakeDatabaseMetaData::take(&self.state.primary_keys, &["COLUMN_NAME"])
    }

    fn get_search_string_escape(&self) -> Result<Option<String>, SqlError> {
        Ok(self.state.search_string_escape.lock().unwrap().clone())
    }

    fn get_identifier_quote_string(&self) -> Result<String, SqlError> {
        Ok(self.state.identifier_quote_string.lock().unwrap().clone())
    }

    fn get_database_product_name(&self) -> Result<String, SqlError> {
        Ok(self.state.database_product_name.lock().unwrap().clone())
    }

    async fn get_user_name(&self) -> Result<Option<String>, SqlError> {
        Ok(self.state.user_name.lock().unwrap().clone())
    }
}

pub struct ConnectionState {
    pub catalog: Mutex<Option<String>>,
    pub schema: Mutex<Option<String>>,
    pub prepared: Mutex<Vec<String>>,
    pub executed: Mutex<Vec<PreparedStatement>>,
    pub query_handler: Mutex<Option<QueryHandler>>,
    pub prepare_handler: Mutex<Option<PrepareHandler>>,
    pub auto_commit: Mutex<bool>,
    pub auto_commit_calls: Mutex<Vec<bool>>,
    pub commits: AtomicUsize,
    pub rollbacks: AtomicUsize,
    pub closes: AtomicUsize,
    pub valid: Mutex<Result<bool, SqlError>>,
}

impl Default for ConnectionState {
    fn default() -> ConnectionState {
        ConnectionState {
            catalog: Mutex::new(None),
            schema: Mutex::new(None),
            prepared: Mutex::new(Vec::new()),
            executed: Mutex::new(Vec::new()),
            query_handler: Mutex::new(None),
            prepare_handler: Mutex::new(None),
            auto_commit: Mutex::new(true),
            auto_commit_calls: Mutex::new(Vec::new()),
            commits: AtomicUsize::new(0),
            rollbacks: AtomicUsize::new(0),
            closes: AtomicUsize::new(0),
            valid: Mutex::new(Ok(true)),
        }
    }
}

#[derive(Clone, Default)]
pub struct FakeConnection {
    pub state: Arc<ConnectionState>,
    pub meta: FakeDatabaseMetaData,
}

impl FakeConnection {
    pub fn new() -> FakeConnection {
        FakeConnection::default()
    }

    pub fn set_catalog(&self, catalog: Option<&str>) {
        *self.state.catalog.lock().unwrap() = catalog.map(ToString::to_string);
    }

    pub fn on_query<F>(&self, handler: F)
    where
        F: Fn(&PreparedStatement) -> Result<ResultSetData, SqlError> + Send + Sync + 'static,
    {
        *self.state.query_handler.lock().unwrap() = Some(Arc::new(handler));
    }

    pub fn on_prepare<F>(&self, handler: F)
    where
        F: Fn(&str) -> Result<PreparedStatement, SqlError> + Send + Sync + 'static,
    {
        *self.state.prepare_handler.lock().unwrap() = Some(Arc::new(handler));
    }

    pub fn prepared_sql(&self) -> Vec<String> {
        self.state.prepared.lock().unwrap().clone()
    }

    pub fn executed(&self) -> Vec<PreparedStatement> {
        self.state.executed.lock().unwrap().clone()
    }

    pub fn commits(&self) -> usize {
        self.state.commits.load(Ordering::SeqCst)
    }

    pub fn rollbacks(&self) -> usize {
        self.state.rollbacks.load(Ordering::SeqCst)
    }

    pub fn closes(&self) -> usize {
        self.state.closes.load(Ordering::SeqCst)
    }

    pub fn auto_commit_calls(&self) -> Vec<bool> {
        self.state.auto_commit_calls.lock().unwrap().clone()
    }

    pub fn set_valid(&self, valid: Result<bool, SqlError>) {
        *self.state.valid.lock().unwrap() = valid;
    }
}

#[async_trait]
impl JdbcConnection for FakeConnection {
    fn metadata(&self) -> &dyn DatabaseMetaData {
        &self.meta
    }

    async fn get_catalog(&self) -> Result<Option<String>, SqlError> {
        Ok(self.state.catalog.lock().unwrap().clone())
    }

    async fn get_schema(&self) -> Result<Option<String>, SqlError> {
        Ok(self.state.schema.lock().unwrap().clone())
    }

    async fn prepare_statement(&self, sql: &str) -> Result<PreparedStatement, SqlError> {
        self.state.prepared.lock().unwrap().push(sql.to_string());
        let handler = self.state.prepare_handler.lock().unwrap().clone();
        match handler {
            Some(handler) => handler(sql),
            None => Ok(PreparedStatement::new(sql)),
        }
    }

    async fn execute_query(&self, statement: &PreparedStatement) -> Result<ResultSet, SqlError> {
        self.state.executed.lock().unwrap().push(statement.clone());
        let handler = self.state.query_handler.lock().unwrap().clone();
        match handler {
            Some(handler) => handler(statement).map(|data| data.to_result_set()),
            None => Ok(ResultSetData::empty(&[]).to_result_set()),
        }
    }

    async fn execute(&self, statement: &PreparedStatement) -> Result<bool, SqlError> {
        self.state.executed.lock().unwrap().push(statement.clone());
        Ok(true)
    }

    async fn set_auto_commit(&self, auto_commit: bool) -> Result<(), SqlError> {
        self.state
            .auto_commit_calls
            .lock()
            .unwrap()
            .push(auto_commit);
        *self.state.auto_commit.lock().unwrap() = auto_commit;
        Ok(())
    }

    async fn get_auto_commit(&self) -> Result<bool, SqlError> {
        Ok(*self.state.auto_commit.lock().unwrap())
    }

    async fn commit(&self) -> Result<(), SqlError> {
        self.state.commits.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn rollback(&self) -> Result<(), SqlError> {
        self.state.rollbacks.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn is_valid(&self, _timeout_seconds: i32) -> Result<bool, SqlError> {
        self.state.valid.lock().unwrap().clone()
    }

    async fn close(&self) -> Result<(), SqlError> {
        self.state.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct FakeConnectionFactory {
    pub connection: FakeConnection,
    pub connection_error: Arc<Mutex<Option<SqlError>>>,
    pub close_error: Arc<Mutex<Option<String>>>,
    pub get_connection_calls: Arc<AtomicUsize>,
    pub close_calls: Arc<AtomicUsize>,
    pub had_credentials: Arc<Mutex<Vec<bool>>>,
}

impl FakeConnectionFactory {
    pub fn new(connection: FakeConnection) -> FakeConnectionFactory {
        FakeConnectionFactory {
            connection,
            ..FakeConnectionFactory::default()
        }
    }

    pub fn close_calls(&self) -> usize {
        self.close_calls.load(Ordering::SeqCst)
    }

    pub fn get_connection_calls(&self) -> usize {
        self.get_connection_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl JdbcConnectionFactory for FakeConnectionFactory {
    async fn get_connection(
        &self,
        credentials_provider: Option<&dyn CredentialsProvider>,
    ) -> athena_federation_sdk::Result<Box<dyn JdbcConnection>> {
        self.get_connection_calls.fetch_add(1, Ordering::SeqCst);
        self.had_credentials
            .lock()
            .unwrap()
            .push(credentials_provider.is_some());
        if let Some(error) = self.connection_error.lock().unwrap().clone() {
            return Err(error.into());
        }
        Ok(Box::new(self.connection.clone()))
    }

    async fn close(&self) -> athena_federation_sdk::Result<()> {
        self.close_calls.fetch_add(1, Ordering::SeqCst);
        match self.close_error.lock().unwrap().clone() {
            Some(message) => Err(athena_federation_sdk::FederationError::runtime(message)),
            None => Ok(()),
        }
    }
}

#[derive(Default)]
pub struct FakeSecretsManager {
    secrets: Mutex<HashMap<String, String>>,
    calls: Mutex<Vec<String>>,
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

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl SecretsManagerClient for FakeSecretsManager {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError> {
        self.calls.lock().unwrap().push(secret_id.to_string());
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

pub struct FakeAthena {
    pub state: Mutex<String>,
}

impl Default for FakeAthena {
    fn default() -> FakeAthena {
        FakeAthena {
            state: Mutex::new("RUNNING".to_string()),
        }
    }
}

#[async_trait]
impl AthenaClient for FakeAthena {
    async fn get_query_execution_state(
        &self,
        _query_execution_id: &str,
    ) -> Result<Option<String>, AwsServiceError> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }

    fn with_credentials(&self, _credentials: &Credentials) -> Arc<dyn AthenaClient> {
        Arc::new(FakeAthena {
            state: Mutex::new(self.state.lock().unwrap().clone()),
        })
    }
}

#[derive(Default, Clone)]
pub struct FakeS3 {
    pub objects: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FakeS3 {
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.objects.lock().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }
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

    async fn head_bucket(&self, _bucket: &str) -> Result<(), AwsServiceError> {
        Ok(())
    }

    fn with_credentials(&self, _credentials: &Credentials) -> Arc<dyn S3Client> {
        Arc::new(self.clone())
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
pub struct FakeCrypto;

#[async_trait]
impl AwsCrypto for FakeCrypto {
    async fn decrypt_data(
        &self,
        _kms_key_id: &str,
        _ciphertext: &[u8],
    ) -> athena_federation_sdk::Result<Vec<u8>> {
        Ok(format!(
            "{{\"accessToken\":\"{FAS_ACCESS}\",\"secretToken\":\"fas-secret\",\"securityToken\":\"fas-session\"}}"
        )
        .into_bytes())
    }
}

pub struct Fakes {
    pub secrets: Arc<FakeSecretsManager>,
    pub s3: FakeS3,
    pub athena: Arc<FakeAthena>,
}

impl Default for Fakes {
    fn default() -> Fakes {
        Fakes::new()
    }
}

impl Fakes {
    pub fn new() -> Fakes {
        Fakes::with_secrets(FakeSecretsManager::default())
    }

    pub fn with_secrets(secrets: FakeSecretsManager) -> Fakes {
        Fakes {
            secrets: Arc::new(secrets),
            s3: FakeS3::default(),
            athena: Arc::new(FakeAthena::default()),
        }
    }

    pub fn clients(&self) -> AwsClients {
        AwsClients {
            secrets_manager: self.secrets.clone(),
            athena: self.athena.clone(),
            s3: Arc::new(self.s3.clone()),
            kms: Arc::new(FakeKms),
            kms_encryption_provider: Arc::new(KmsEncryptionProvider::with_aws_crypto(Arc::new(
                FakeCrypto,
            ))),
        }
    }
}

pub fn string_column(label: &str) -> ColumnMetadata {
    ColumnMetadata::new(label, JdbcType::Varchar)
}

pub fn addr<T: ?Sized>(value: &T) -> usize {
    std::ptr::from_ref(value).cast::<()>().addr()
}
