use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use athena_jdbc::connection::{JdbcDriver, JdbcProperties};
use athena_jdbc::sql::{JdbcConnection, SqlError};
use snowflake_connector_rs::{
    AuthConfig, Client, ClientConfig, EndpointConfig, KeyPairConfig, PasswordConfig, QueryConfig,
    SessionConfig,
};
use url::Url;

use super::connection::{positive_seconds, SnowflakeConnection};
use super::executor::{from_connector_error, ConnectorExecutor, TimestampTypeMapping};
use crate::snowflake_constants;
use crate::utils::SnowflakePrivateKey;

const URL_PREFIX: &str = "jdbc:snowflake://";
const QUERY_TIMEOUT: &str = "QUERYTIMEOUT";
const QUERY_RESULT_FORMAT: &str = "QUERY_RESULT_FORMAT";
const CLIENT_TIMESTAMP_TYPE_MAPPING: &str = "CLIENT_TIMESTAMP_TYPE_MAPPING";
const JSON_RESULT_FORMAT: &str = "JSON";
pub const QUERY_RESPONSE_TIMEOUT: Duration = Duration::from_mins(15);

const CLIENT_SIDE_PROPERTIES: [&str; 24] = [
    "USER",
    "PASSWORD",
    "ACCOUNT",
    "WAREHOUSE",
    "DB",
    "DATABASE",
    "SCHEMA",
    "ROLE",
    "AUTHENTICATOR",
    "TOKEN",
    "PRIVATEKEY",
    "PRIVATE_KEY_FILE",
    "PRIVATE_KEY_FILE_PWD",
    "PRIVATE_KEY_BASE64",
    "PRIVATE_KEY_PWD",
    "PASSCODE",
    "PASSCODEINPASSWORD",
    "SSL",
    "LOGINTIMEOUT",
    "NETWORKTIMEOUT",
    "QUERYTIMEOUT",
    "TRACING",
    "APPLICATION",
    "DATABASETERM",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authenticator {
    Snowflake,
    OAuth,
    SnowflakeJwt,
    Unsupported(String),
}

impl Authenticator {
    fn parse(value: Option<&str>) -> Authenticator {
        let Some(value) = value else {
            return Authenticator::Snowflake;
        };
        if value.eq_ignore_ascii_case("oauth") {
            Authenticator::OAuth
        } else if value.eq_ignore_ascii_case("snowflake_jwt") {
            Authenticator::SnowflakeJwt
        } else if value.eq_ignore_ascii_case("snowflake") {
            Authenticator::Snowflake
        } else {
            Authenticator::Unsupported(value.to_string())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnowflakeConnectionSettings {
    pub base_url: Url,
    pub account: String,
    pub user: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
    pub authenticator: Authenticator,
    pub warehouse: Option<String>,
    pub database: Option<String>,
    pub schema: Option<String>,
    pub role: Option<String>,
    pub query_timeout: Option<Duration>,
    pub timestamp_mapping: TimestampTypeMapping,
    pub session_parameters: Vec<(String, String)>,
}

impl SnowflakeConnectionSettings {
    pub fn session_config(&self) -> SessionConfig {
        let mut session = SessionConfig::new();
        if let Some(warehouse) = &self.warehouse {
            session = session.with_warehouse(warehouse);
        }
        if let Some(database) = &self.database {
            session = session.with_database(database);
        }
        if let Some(schema) = &self.schema {
            session = session.with_schema(schema);
        }
        if let Some(role) = &self.role {
            session = session.with_role(role);
        }
        for (key, value) in &self.session_parameters {
            session = session.with_session_parameter(key, serde_json::Value::String(value.clone()));
        }
        session
    }

    pub fn auth_config(
        &self,
        private_key: Option<&SnowflakePrivateKey>,
    ) -> Result<AuthConfig, SqlError> {
        if let Some(private_key) = private_key {
            let pem = private_key
                .to_pem()
                .map_err(|e| SqlError::snowflake(e.to_string()))?;
            return Ok(AuthConfig::key_pair(KeyPairConfig::from_pem(pem)));
        }
        match &self.authenticator {
            Authenticator::OAuth => {
                let token = self
                    .token
                    .as_deref()
                    .or(self.password.as_deref())
                    .ok_or_else(|| SqlError::snowflake("Missing OAuth token."))?;
                Ok(AuthConfig::oauth(token))
            }
            Authenticator::SnowflakeJwt => Err(SqlError::snowflake(
                "Private key provided is invalid or not supported: null",
            )),
            Authenticator::Unsupported(authenticator) => Err(SqlError::snowflake(format!(
                "Unsupported authenticator: {authenticator}"
            ))),
            Authenticator::Snowflake => {
                let password = self
                    .password
                    .as_deref()
                    .ok_or_else(|| SqlError::snowflake("Missing password."))?;
                Ok(AuthConfig::password(PasswordConfig::from(password)))
            }
        }
    }

    pub fn client_config(
        &self,
        private_key: Option<&SnowflakePrivateKey>,
    ) -> Result<ClientConfig, SqlError> {
        let user = self
            .user
            .as_deref()
            .ok_or_else(|| SqlError::snowflake("Missing user name."))?;
        Ok(
            ClientConfig::new(user, &self.account, self.auth_config(private_key)?)
                .with_session(self.session_config())
                .with_query(QueryConfig::new().with_query_response_timeout(QUERY_RESPONSE_TIMEOUT))
                .with_endpoint(EndpointConfig::CustomBaseUrl(self.base_url.clone())),
        )
    }
}

fn session_parameters(parameters: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let default_result_format = (!parameters.contains_key(QUERY_RESULT_FORMAT)).then(|| {
        (
            QUERY_RESULT_FORMAT.to_string(),
            JSON_RESULT_FORMAT.to_string(),
        )
    });
    parameters
        .iter()
        .filter(|(key, _)| !CLIENT_SIDE_PROPERTIES.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .chain(default_result_format)
        .collect()
}

fn query_timeout(value: Option<&String>) -> Result<Option<Duration>, SqlError> {
    let Some(value) = value else {
        return Ok(None);
    };
    value.parse::<i32>().map(positive_seconds).map_err(|_| {
        SqlError::snowflake(
            "Invalid parameter value java.lang.String for parameter type java.lang.Integer.",
        )
    })
}

pub fn url_decode(text: &str) -> Result<String, SqlError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                let byte = text
                    .get(i + 1..i + 3)
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                    .ok_or_else(|| {
                        SqlError::snowflake(
                            "URLDecoder: Illegal hex characters in escape (%) pattern",
                        )
                    })?;
                out.push(byte);
                i += 3;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

pub fn parse_connection_settings(
    url: &str,
    properties: &JdbcProperties,
) -> Result<SnowflakeConnectionSettings, SqlError> {
    let rest = url
        .strip_prefix(URL_PREFIX)
        .ok_or_else(|| SqlError::snowflake(format!("Invalid Connect String: {url}.")))?;
    let (authority_and_path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let authority = authority_and_path.split('/').next().unwrap_or_default();
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() => (host, Some(port)),
        Some((host, _)) => (host, None),
        None => (authority, None),
    };
    if host.is_empty() {
        return Err(SqlError::snowflake(format!(
            "Invalid Connect String: {url}."
        )));
    }
    let mut parameters = BTreeMap::new();
    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            parameters.insert(url_decode(key)?.to_uppercase(), url_decode(value)?);
        }
    }
    for (key, value) in properties {
        parameters.insert(key.to_uppercase(), value.clone());
    }
    let ssl_off = parameters
        .get("SSL")
        .is_some_and(|v| v.eq_ignore_ascii_case("off") || v.eq_ignore_ascii_case("false"));
    let scheme = if ssl_off { "http" } else { "https" };
    let base = match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    let base_url = Url::parse(&base)
        .map_err(|e| SqlError::snowflake(format!("Invalid Connect String: {url}. {e}")))?;
    let account = parameters
        .get("ACCOUNT")
        .cloned()
        .unwrap_or_else(|| host.split('.').next().unwrap_or(host).to_string());
    let query_timeout = query_timeout(parameters.get(QUERY_TIMEOUT))?;
    let get = |key: &str| parameters.get(key).cloned();
    Ok(SnowflakeConnectionSettings {
        base_url,
        account,
        user: get("USER"),
        password: get("PASSWORD"),
        token: get("TOKEN"),
        authenticator: Authenticator::parse(parameters.get("AUTHENTICATOR").map(String::as_str)),
        warehouse: get("WAREHOUSE"),
        database: get("DB").or_else(|| get("DATABASE")),
        schema: get("SCHEMA"),
        role: get("ROLE"),
        query_timeout,
        timestamp_mapping: TimestampTypeMapping::from_parameter(
            parameters
                .get(CLIENT_TIMESTAMP_TYPE_MAPPING)
                .map(String::as_str),
        ),
        session_parameters: session_parameters(&parameters),
    })
}

#[derive(Debug, Clone, Copy)]
pub enum SnowflakeAuth<'a> {
    Properties,
    KeyPair(&'a SnowflakePrivateKey),
}

#[async_trait]
pub trait SnowflakeDriver: Send + Sync {
    fn driver_class_name(&self) -> &str;

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
        auth: SnowflakeAuth<'_>,
    ) -> Result<Box<dyn JdbcConnection>, SqlError>;
}

pub(crate) struct PooledSnowflakeDriver(pub(crate) Arc<dyn SnowflakeDriver>);

#[async_trait]
impl JdbcDriver for PooledSnowflakeDriver {
    fn driver_class_name(&self) -> &str {
        self.0.driver_class_name()
    }

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        self.0
            .connect(url, properties, SnowflakeAuth::Properties)
            .await
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SnowflakeJdbcDriver;

impl SnowflakeJdbcDriver {
    pub fn new() -> SnowflakeJdbcDriver {
        SnowflakeJdbcDriver
    }
}

#[async_trait]
impl SnowflakeDriver for SnowflakeJdbcDriver {
    fn driver_class_name(&self) -> &str {
        snowflake_constants::SNOWFLAKE_DRIVER_CLASS
    }

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
        auth: SnowflakeAuth<'_>,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        let private_key = match auth {
            SnowflakeAuth::Properties => None,
            SnowflakeAuth::KeyPair(private_key) => Some(private_key),
        };
        let settings = parse_connection_settings(url, properties)?;
        let config = settings.client_config(private_key)?;
        let client = Client::new(config).map_err(|e| from_connector_error(&e))?;
        let session = client
            .create_session()
            .await
            .map_err(|e| from_connector_error(&e))?;
        Ok(Box::new(
            SnowflakeConnection::new(Arc::new(ConnectorExecutor::new(
                session,
                settings.timestamp_mapping,
            )))
            .with_query_timeout(settings.query_timeout),
        ))
    }
}
