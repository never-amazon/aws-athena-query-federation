use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use athena_federation_sdk::data::{GetObjectRequest, PutObjectRequest, S3Client};
use athena_federation_sdk::handlers::{
    AwsClients, CompositeHandler, HandlerContext, MetadataContext,
};
use athena_federation_sdk::lambda::{LambdaContext, LambdaError, RequestStreamHandler};
use athena_federation_sdk::security::{
    AwsCrypto, AwsRequestOverrideConfiguration, AwsService, AwsServiceError, KeySource, KmsClient,
    KmsEncryptionProvider, SecretsManagerClient,
};
use athena_federation_sdk::{AthenaClient, FederationError};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionConfigBuilder, DEFAULT_CONNECTION_STRING_PROPERTY,
};
use athena_jdbc::manager::jdbc_util;
use athena_jdbc::{MultiplexingJdbcMetadataHandler, RecordMultiplexer};
use athena_postgresql::{
    constants, PostGreSqlMetadataDialect, PostGreSqlMetadataHandler, PostGreSqlMuxMetadataHandler,
    PostGreSqlMuxRecordHandler, PostGreSqlRecordDialect, PostGreSqlRecordHandler,
};
use aws_credential_types::Credentials;
use aws_sdk_kms::types::DataKeySpec;
use base64::Engine;
use indexmap::IndexMap;
use serde_json::{json, Value};

const SECRET_NAME: &str = "parity-pg-secret";
const FUNCTION_NAME: &str = "parity-pg-fn";

type RunnerResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Default)]
struct StubState {
    calls: Mutex<BTreeSet<String>>,
    s3_objects: Mutex<BTreeMap<String, Vec<u8>>>,
    secrets: HashMap<String, String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl StubState {
    fn record(&self, call: &str) {
        lock(&self.calls).insert(call.to_string());
    }
}

#[derive(Clone)]
struct Stub {
    state: Arc<StubState>,
}

fn stub_call_error(service: &str, name: &str) -> AwsServiceError {
    AwsServiceError::new(format!("{service} stub does not implement {name}"))
}

#[async_trait]
impl S3Client for Stub {
    async fn put_object(
        &self,
        request: PutObjectRequest,
        body: Vec<u8>,
    ) -> Result<(), AwsServiceError> {
        self.state.record("s3.putObject");
        lock(&self.state.s3_objects)
            .insert(format!("{}/{}", request.bucket(), request.key()), body);
        Ok(())
    }

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError> {
        self.state.record("s3.getObject");
        lock(&self.state.s3_objects)
            .get(&format!("{}/{}", request.bucket(), request.key()))
            .cloned()
            .ok_or_else(|| {
                AwsServiceError::service(
                    AwsService::S3,
                    Some("NoSuchKey".to_string()),
                    Some(format!("stub has no object {}", request.key())),
                )
            })
    }

    async fn head_bucket(&self, _bucket: &str) -> Result<(), AwsServiceError> {
        self.state.record("s3.headBucket");
        Ok(())
    }

    fn with_credentials(&self, _credentials: &Credentials) -> Arc<dyn S3Client> {
        Arc::new(self.clone())
    }
}

#[async_trait]
impl SecretsManagerClient for Stub {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError> {
        self.state.record("secretsmanager.getSecretValue");
        self.state
            .secrets
            .get(secret_id)
            .cloned()
            .map(Some)
            .ok_or_else(|| {
                AwsServiceError::service(
                    AwsService::SecretsManager,
                    Some("ResourceNotFoundException".to_string()),
                    Some("Secrets Manager can't find the specified secret.".to_string()),
                )
            })
    }

    async fn put_secret_value(
        &self,
        _secret_id: &str,
        _secret_string: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<(), AwsServiceError> {
        self.state.record("secretsmanager.putSecretValue");
        Err(stub_call_error("Secrets Manager", "putSecretValue"))
    }
}

#[async_trait]
impl AthenaClient for Stub {
    async fn get_query_execution_state(
        &self,
        _query_execution_id: &str,
    ) -> Result<Option<String>, AwsServiceError> {
        self.state.record("athena.getQueryExecution");
        Ok(Some("RUNNING".to_string()))
    }

    fn with_credentials(&self, _credentials: &Credentials) -> Arc<dyn AthenaClient> {
        Arc::new(self.clone())
    }
}

#[async_trait]
impl KmsClient for Stub {
    async fn generate_data_key(
        &self,
        _key_id: &str,
        _key_spec: DataKeySpec,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError> {
        self.state.record("kms.generateDataKey");
        Err(stub_call_error("KMS", "generateDataKey"))
    }

    async fn generate_random(&self, _number_of_bytes: usize) -> Result<Vec<u8>, AwsServiceError> {
        self.state.record("kms.generateRandom");
        Err(stub_call_error("KMS", "generateRandom"))
    }
}

#[async_trait]
impl AwsCrypto for Stub {
    async fn decrypt_data(
        &self,
        _kms_key_id: &str,
        _ciphertext: &[u8],
    ) -> athena_federation_sdk::Result<Vec<u8>> {
        self.state.record("kms.decrypt");
        Err(FederationError::Runtime(
            "KMS stub does not implement decrypt".to_string(),
        ))
    }
}

impl Stub {
    fn clients(&self) -> AwsClients {
        let stub = Arc::new(self.clone());
        AwsClients {
            secrets_manager: stub.clone(),
            athena: stub.clone(),
            s3: stub.clone(),
            kms: stub.clone(),
            kms_encryption_provider: Arc::new(KmsEncryptionProvider::with_aws_crypto(stub)),
        }
    }
}

fn expand(value: &str) -> RunnerResult<String> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("{{") {
        let Some(length) = rest[start + 2..].find("}}") else {
            break;
        };
        let name = &rest[start + 2..start + 2 + length];
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        result.push_str(&rest[..start]);
        if valid {
            let replacement = std::env::var(name)
                .map_err(|_| format!("case env refers to unset variable {name}"))?;
            result.push_str(&replacement);
        } else {
            result.push_str(&rest[start..start + 4 + length]);
        }
        rest = &rest[start + 4 + length..];
    }
    result.push_str(rest);
    Ok(result)
}

fn case_config(meta: &Value) -> RunnerResult<HashMap<String, String>> {
    let mut config: HashMap<String, String> = std::env::vars().collect();
    if let Some(env) = meta.get("env").and_then(Value::as_object) {
        for (key, value) in env {
            match value {
                Value::Null => {
                    config.remove(key);
                }
                Value::String(text) => {
                    config.insert(key.clone(), expand(text)?);
                }
                other => {
                    config.insert(key.clone(), expand(&other.to_string())?);
                }
            }
        }
    }
    Ok(config)
}

fn metadata_handler(
    clients: &AwsClients,
    db_config: &DatabaseConnectionConfig,
    config: &HashMap<String, String>,
) -> athena_federation_sdk::Result<PostGreSqlMetadataHandler> {
    let factory = Arc::new(PostGreSqlMetadataDialect::connection_factory(
        db_config, config,
    )?);
    let disabled = config
        .get("disable_spill_encryption")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let key_source = if disabled {
        None
    } else {
        Some(KeySource::Local)
    };
    let context = MetadataContext::builder(db_config.engine(), clients.clone())
        .config_options(config.clone())
        .encryption_key_factory(key_source)
        .spill_bucket(None)
        .spill_prefix(None)
        .build()?;
    Ok(PostGreSqlMetadataDialect::handler(
        context,
        db_config.clone(),
        factory,
    ))
}

fn record_handler(
    clients: &AwsClients,
    db_config: &DatabaseConnectionConfig,
    config: &HashMap<String, String>,
) -> athena_federation_sdk::Result<PostGreSqlRecordHandler> {
    let factory = Arc::new(PostGreSqlMetadataDialect::connection_factory(
        db_config, config,
    )?);
    let context = HandlerContext::new(db_config.engine(), clients.clone(), config.clone())?;
    Ok(PostGreSqlRecordDialect::handler(
        context,
        db_config.clone(),
        factory,
    ))
}

fn single_handler(
    clients: &AwsClients,
    config: &HashMap<String, String>,
) -> athena_federation_sdk::Result<CompositeHandler> {
    let db_config = jdbc_util::single_database_config_from_env(constants::POSTGRES_NAME, config)?;
    let metadata = metadata_handler(clients, &db_config, config)?;
    let record = record_handler(clients, &db_config, config)?;
    Ok(CompositeHandler::new(
        Arc::new(metadata),
        Arc::new(record),
        None,
    ))
}

fn mux_handler(
    clients: &AwsClients,
    config: &HashMap<String, String>,
) -> athena_federation_sdk::Result<CompositeHandler> {
    let function_name = config
        .get("AWS_LAMBDA_FUNCTION_NAME")
        .map_or("null", String::as_str);
    let configs = DatabaseConnectionConfigBuilder::new()
        .engine(constants::POSTGRES_NAME)
        .properties(config.clone())
        .build()?;
    let mut metadata_map: IndexMap<String, Arc<PostGreSqlMetadataHandler>> = IndexMap::new();
    let mut record_map: IndexMap<String, Arc<PostGreSqlRecordHandler>> = IndexMap::new();
    let mut default_config = None;
    for db_config in configs {
        let metadata = Arc::new(metadata_handler(clients, &db_config, config)?);
        let record = Arc::new(record_handler(clients, &db_config, config)?);
        metadata_map.insert(db_config.catalog().to_string(), Arc::clone(&metadata));
        record_map.insert(db_config.catalog().to_string(), Arc::clone(&record));
        if db_config.catalog() == DEFAULT_CONNECTION_STRING_PROPERTY {
            metadata_map.insert(format!("lambda:{function_name}"), metadata);
            record_map.insert(format!("lambda:{function_name}"), record);
            default_config = Some(db_config);
        }
    }
    let default_config = default_config.ok_or_else(|| {
        FederationError::IllegalState("mux mode needs a default connection string".to_string())
    })?;
    let metadata_context = MetadataContext::builder(default_config.engine(), clients.clone())
        .config_options(config.clone())
        .encryption_key_factory(None)
        .spill_bucket(None)
        .spill_prefix(None)
        .build()?;
    let metadata: PostGreSqlMuxMetadataHandler = MultiplexingJdbcMetadataHandler::new(
        metadata_context,
        Arc::new(PostGreSqlMetadataDialect::connection_factory(
            &default_config,
            config,
        )?),
        metadata_map,
    )?;
    let record_context =
        HandlerContext::new(default_config.engine(), clients.clone(), config.clone())?;
    let record: PostGreSqlMuxRecordHandler = RecordMultiplexer::handler(
        record_context,
        default_config.clone(),
        Arc::new(PostGreSqlMetadataDialect::connection_factory(
            &default_config,
            config,
        )?),
        record_map,
    )?;
    Ok(CompositeHandler::new(
        Arc::new(metadata),
        Arc::new(record),
        None,
    ))
}

fn secrets() -> HashMap<String, String> {
    match std::env::var("PG_PASSWORD") {
        Ok(password) => HashMap::from([(
            SECRET_NAME.to_string(),
            json!({"username": "parity", "password": password}).to_string(),
        )]),
        Err(_) => HashMap::new(),
    }
}

fn context(name: &str) -> LambdaContext {
    LambdaContext::new()
        .with_aws_request_id(format!("parity-{name}"))
        .with_function_name(FUNCTION_NAME)
        .with_invoked_function_arn(Some(format!(
            "arn:aws:lambda:us-east-1:123456789012:function:{FUNCTION_NAME}"
        )))
}

fn aws_report(state: &StubState) -> Value {
    let calls: Vec<Value> = lock(&state.calls).iter().map(|c| json!(c)).collect();
    let objects: Vec<Value> = lock(&state.s3_objects)
        .iter()
        .map(|(path, bytes)| {
            json!({
                "path": path,
                "size": bytes.len(),
                "bytes": base64::engine::general_purpose::STANDARD.encode(bytes),
            })
        })
        .collect();
    json!({"calls": calls, "s3Objects": objects})
}

fn remove_if_present(path: &Path) -> RunnerResult<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

async fn run_case(case_dir: &Path, out_dir: &Path) -> RunnerResult<bool> {
    let name = case_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let request = std::fs::read(case_dir.join("request.json"))?;
    let meta: Value = serde_json::from_slice(&std::fs::read(case_dir.join("case.json"))?)?;
    let config = case_config(&meta)?;
    let mode = meta
        .get("handler")
        .and_then(Value::as_str)
        .unwrap_or("single")
        .to_string();

    std::fs::create_dir_all(out_dir)?;
    for file in ["rust-response.json", "rust-error.txt", "rust-aws.json"] {
        remove_if_present(&out_dir.join(file))?;
    }
    std::fs::write(out_dir.join("request.json"), &request)?;
    std::fs::copy(case_dir.join("case.json"), out_dir.join("case.json"))?;

    let stub = Stub {
        state: Arc::new(StubState {
            secrets: secrets(),
            ..StubState::default()
        }),
    };
    let clients = stub.clients();
    let handler = match mode.as_str() {
        "single" => single_handler(&clients, &config),
        "mux" => mux_handler(&clients, &config),
        other => Err(FederationError::IllegalArgument(format!(
            "unknown handler mode {other}"
        ))),
    };
    let outcome = match handler {
        Ok(handler) => {
            handler
                .handle_request(&request, Some(&context(&name)))
                .await
        }
        Err(e) => Err(e),
    };
    let ok = match &outcome {
        Ok(response) => {
            std::fs::write(out_dir.join("rust-response.json"), response)?;
            true
        }
        Err(error) => {
            std::fs::write(
                out_dir.join("rust-error.txt"),
                LambdaError::new(error).to_report(),
            )?;
            false
        }
    };
    let state = &stub.state;
    if !lock(&state.calls).is_empty() || !lock(&state.s3_objects).is_empty() {
        std::fs::write(
            out_dir.join("rust-aws.json"),
            serde_json::to_vec_pretty(&aws_report(state))?,
        )?;
    }
    println!("{} {name}", if ok { "ok   " } else { "error" });
    Ok(ok)
}

fn case_dirs(corpus: &Path, filter: Option<&str>) -> RunnerResult<Vec<PathBuf>> {
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(corpus)? {
        let path = entry?.path();
        let matches = path
            .file_name()
            .is_some_and(|n| filter.is_none_or(|f| n.to_string_lossy().contains(f)));
        if path.is_dir() && matches {
            cases.push(path);
        }
    }
    cases.sort();
    Ok(cases)
}

#[tokio::main]
async fn main() -> RunnerResult<()> {
    let corpus = PathBuf::from(std::env::var("PARITY_WIRE_CORPUS")?);
    let out = PathBuf::from(std::env::var("PARITY_WIRE_OUT")?);
    let filter = std::env::var("PARITY_WIRE_FILTER")
        .ok()
        .filter(|f| !f.is_empty());
    let cases = case_dirs(&corpus, filter.as_deref())?;
    std::fs::create_dir_all(&out)?;
    let mut errors = 0;
    for case_dir in &cases {
        let name = case_dir
            .file_name()
            .map(std::ffi::OsStr::to_os_string)
            .unwrap_or_default();
        if !run_case(case_dir, &out.join(name)).await? {
            errors += 1;
        }
    }
    println!("cases={} errors={errors}", cases.len());
    Ok(())
}
