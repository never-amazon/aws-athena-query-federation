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
pub struct SnowflakeConnectionSettings {
    pub base_url: Url,
    pub account: String,
    pub parameters: BTreeMap<String, String>,
}

impl SnowflakeConnectionSettings {
    fn get(&self, key: &str) -> Option<&str> {
        self.parameters.get(key).map(String::as_str)
    }

    pub fn user(&self) -> Option<&str> {
        self.get("USER")
    }

    pub fn authenticator(&self) -> Option<&str> {
        self.get("AUTHENTICATOR")
    }

    pub fn session_parameters(&self) -> impl Iterator<Item = (&str, &str)> {
        let default_result_format = (!self.parameters.contains_key(QUERY_RESULT_FORMAT))
            .then_some((QUERY_RESULT_FORMAT, JSON_RESULT_FORMAT));
        self.parameters
            .iter()
            .filter(|(key, _)| !CLIENT_SIDE_PROPERTIES.contains(&key.as_str()))
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .chain(default_result_format)
    }

    pub fn timestamp_type_mapping(&self) -> TimestampTypeMapping {
        TimestampTypeMapping::from_parameter(self.get(CLIENT_TIMESTAMP_TYPE_MAPPING))
    }

    pub fn query_timeout(&self) -> Result<Option<Duration>, SqlError> {
        let Some(value) = self.get(QUERY_TIMEOUT) else {
            return Ok(None);
        };
        value.parse::<i32>().map(positive_seconds).map_err(|_| {
            SqlError::snowflake(
                "Invalid parameter value java.lang.String for parameter type java.lang.Integer.",
            )
        })
    }

    pub fn session_config(&self) -> SessionConfig {
        let mut session = SessionConfig::new();
        if let Some(warehouse) = self.get("WAREHOUSE") {
            session = session.with_warehouse(warehouse);
        }
        if let Some(database) = self.get("DB").or_else(|| self.get("DATABASE")) {
            session = session.with_database(database);
        }
        if let Some(schema) = self.get("SCHEMA") {
            session = session.with_schema(schema);
        }
        if let Some(role) = self.get("ROLE") {
            session = session.with_role(role);
        }
        for (key, value) in self.session_parameters() {
            session =
                session.with_session_parameter(key, serde_json::Value::String(value.to_string()));
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
        let authenticator = self.authenticator().unwrap_or("snowflake");
        if authenticator.eq_ignore_ascii_case("oauth") {
            let token = self
                .get("TOKEN")
                .or_else(|| self.get("PASSWORD"))
                .ok_or_else(|| SqlError::snowflake("Missing OAuth token."))?;
            return Ok(AuthConfig::oauth(token));
        }
        if authenticator.eq_ignore_ascii_case("snowflake_jwt") {
            return Err(SqlError::snowflake(
                "Private key provided is invalid or not supported: null",
            ));
        }
        if !authenticator.eq_ignore_ascii_case("snowflake") {
            return Err(SqlError::snowflake(format!(
                "Unsupported authenticator: {authenticator}"
            )));
        }
        let password = self
            .get("PASSWORD")
            .ok_or_else(|| SqlError::snowflake("Missing password."))?;
        Ok(AuthConfig::password(PasswordConfig::from(password)))
    }

    pub fn client_config(
        &self,
        private_key: Option<&SnowflakePrivateKey>,
    ) -> Result<ClientConfig, SqlError> {
        let user = self
            .user()
            .ok_or_else(|| SqlError::snowflake("Missing user name."))?;
        Ok(
            ClientConfig::new(user, &self.account, self.auth_config(private_key)?)
                .with_session(self.session_config())
                .with_query(QueryConfig::new().with_query_response_timeout(QUERY_RESPONSE_TIMEOUT))
                .with_endpoint(EndpointConfig::CustomBaseUrl(self.base_url.clone())),
        )
    }
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
    Ok(SnowflakeConnectionSettings {
        base_url,
        account,
        parameters,
    })
}

#[async_trait]
pub trait SnowflakeDriver: JdbcDriver {
    async fn connect_with_private_key(
        &self,
        url: &str,
        properties: &JdbcProperties,
        private_key: &SnowflakePrivateKey,
    ) -> Result<Box<dyn JdbcConnection>, SqlError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SnowflakeJdbcDriver;

impl SnowflakeJdbcDriver {
    pub fn new() -> SnowflakeJdbcDriver {
        SnowflakeJdbcDriver
    }

    async fn open(
        &self,
        url: &str,
        properties: &JdbcProperties,
        private_key: Option<&SnowflakePrivateKey>,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        let settings = parse_connection_settings(url, properties)?;
        let query_timeout = settings.query_timeout()?;
        let config = settings.client_config(private_key)?;
        let client = Client::new(config).map_err(|e| from_connector_error(&e))?;
        let session = client
            .create_session()
            .await
            .map_err(|e| from_connector_error(&e))?;
        Ok(Box::new(
            SnowflakeConnection::new(
                Arc::new(ConnectorExecutor::new(
                    session,
                    settings.timestamp_type_mapping(),
                )),
                settings.user().map(ToString::to_string),
            )
            .with_query_timeout(query_timeout),
        ))
    }
}

#[async_trait]
impl JdbcDriver for SnowflakeJdbcDriver {
    fn driver_class_name(&self) -> &str {
        snowflake_constants::SNOWFLAKE_DRIVER_CLASS
    }

    async fn connect(
        &self,
        url: &str,
        properties: &JdbcProperties,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        self.open(url, properties, None).await
    }
}

#[async_trait]
impl SnowflakeDriver for SnowflakeJdbcDriver {
    async fn connect_with_private_key(
        &self,
        url: &str,
        properties: &JdbcProperties,
        private_key: &SnowflakePrivateKey,
    ) -> Result<Box<dyn JdbcConnection>, SqlError> {
        self.open(url, properties, Some(private_key)).await
    }
}
