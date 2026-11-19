use std::collections::HashMap;
use std::hash::BuildHasher;
use std::time::Duration;

pub const SNOWFLAKE_NAME: &str = "snowflake";
pub const SNOWFLAKE_DRIVER_CLASS: &str = "net.snowflake.client.api.driver.SnowflakeDriver";
pub const SNOWFLAKE_DEFAULT_PORT: i32 = 1025;
pub const MAX_PARTITION_COUNT: i32 = 50;
pub const SINGLE_SPLIT_LIMIT_COUNT: i32 = 10000;
pub const LARGE_TABLE_THRESHOLD: i64 = 100_000_000;
pub const UNIQUENESS_CHECK_TIMEOUT: Duration = Duration::from_mins(2);
pub const DOUBLE_QUOTE_CHAR: &str = "\"";
pub const SINGLE_QUOTE_CHAR: &str = "'";
pub const BLOCK_PARTITION_COLUMN_NAME: &str = "partition";
pub const PARTITION_PRIMARY_MARKER: &str = "-primary-";
pub const PARTITION_LIMIT_MARKER: &str = "-limit-";
pub const PARTITION_OFFSET_MARKER: &str = "-offset-";
pub const S3_ENHANCED_PARTITION_COLUMN_NAME: &str = "s3_column_name_list";
pub const SSL_CERT_FILE_LOCATION: &str = "SSL_CERT_FILE";
pub const SSL_CERT_FILE_LOCATION_VALUE: &str = "/tmp/cacert.pem";
pub const SNOWFLAKE_SPLIT_QUERY_ID: &str = "query_id";
pub const SNOWFLAKE_SPLIT_EXPORT_BUCKET: &str = "exportBucket";
pub const SNOWFLAKE_SPLIT_OBJECT_KEY: &str = "s3ObjectKey";
pub const AUTHENTICATOR: &str = "authenticator";
pub const AUTH_CODE: &str = "auth_code";
pub const CLIENT_ID: &str = "client_id";
pub const TOKEN_URL: &str = "token_url";
pub const REDIRECT_URI: &str = "redirect_uri";
pub const CLIENT_SECRET: &str = "client_secret";
pub const SF_USER: &str = "sfUser";
pub const PEM_PRIVATE_KEY: &str = "pem_private_key";
pub const PEM_PRIVATE_KEY_PASSPHRASE: &str = "pem_private_key_passphrase";
pub const PRIVATE_KEY: &str = "privateKey";
pub const USERNAME: &str = "username";
pub const PASSWORD: &str = "password";
pub const USERNAME_UPPERCASE: &str = "USERNAME";
pub const PASSWORD_UPPERCASE: &str = "PASSWORD";
pub const USER: &str = "user";
pub const SNOWFLAKE_ENABLE_S3_EXPORT: &str = "snowflake_enable_s3_export";
pub const STORAGE_INTEGRATION_CONFIG_KEY: &str = "snowflake_storage_integration_name";
pub const STORAGE_INTEGRATION_PROPERTY_KEY: &str = "property";
pub const STORAGE_INTEGRATION_PROPERTY_VALUE_KEY: &str = "property_value";
pub const STORAGE_INTEGRATION_BUCKET_KEY: &str = "STORAGE_ALLOWED_LOCATIONS";
pub const STORAGE_INTEGRATION_STORAGE_PROVIDER_KEY: &str = "STORAGE_PROVIDER";
pub const COUNT_RECORDS_QUERY: &str = "SELECT row_count\nFROM   information_schema.tables\nWHERE  table_type = 'BASE TABLE'\nAND table_schema= ?\nAND TABLE_NAME = ? ";
pub const SHOW_PRIMARY_KEYS_QUERY: &str = "SHOW PRIMARY KEYS IN ";
pub const LIST_PAGINATED_TABLES_QUERY: &str = "SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ? ORDER BY TABLE_NAME LIMIT ? OFFSET ?";
pub const VIEW_CHECK_QUERY: &str =
    "SELECT * FROM information_schema.views WHERE table_schema = ? AND table_name = ?";

pub fn describe_storage_integration_query(integration: &str) -> String {
    format!("DESC STORAGE INTEGRATION {integration}")
}

pub fn copy_into_query(target: &str, query: &str, integration: &str) -> String {
    format!("COPY INTO {target} FROM ({query}) STORAGE_INTEGRATION = {integration} HEADER = TRUE FILE_FORMAT = (TYPE = 'PARQUET', COMPRESSION = 'SNAPPY') MAX_FILE_SIZE = 52428800")
}

pub fn jdbc_properties() -> HashMap<String, String> {
    HashMap::from([
        ("databaseTerm".to_string(), "SCHEMA".to_string()),
        (
            "CLIENT_RESULT_COLUMN_CASE_INSENSITIVE".to_string(),
            "true".to_string(),
        ),
    ])
}

pub fn is_s3_export_enabled<S: BuildHasher>(config_options: &HashMap<String, String, S>) -> bool {
    config_options
        .get(SNOWFLAKE_ENABLE_S3_EXPORT)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

pub(crate) fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|text| text.chars().all(is_java_whitespace))
}

pub(crate) fn is_java_whitespace(c: char) -> bool {
    match c {
        '\u{1C}'..='\u{1F}' => true,
        '\u{85}' | '\u{A0}' | '\u{2007}' | '\u{202F}' => false,
        _ => c.is_whitespace(),
    }
}

pub(crate) fn java_trim(value: &str) -> &str {
    value.trim_matches(|c: char| c <= ' ')
}
