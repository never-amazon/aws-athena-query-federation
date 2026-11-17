use std::collections::HashMap;

pub struct SnowflakeConstants;

impl SnowflakeConstants {
    pub const SNOWFLAKE_NAME: &'static str = "snowflake";
    pub const SNOWFLAKE_DRIVER_CLASS: &'static str =
        "net.snowflake.client.api.driver.SnowflakeDriver";
    pub const SNOWFLAKE_DEFAULT_PORT: i32 = 1025;
    pub const MAX_PARTITION_COUNT: i32 = 50;
    pub const SINGLE_SPLIT_LIMIT_COUNT: i32 = 10000;
    pub const LARGE_TABLE_THRESHOLD: i64 = 100_000_000;
    pub const UNIQUENESS_CHECK_TIMEOUT_SECONDS: i32 = 120;
    pub const DOUBLE_QUOTE_CHAR: &'static str = "\"";
    pub const SINGLE_QUOTE_CHAR: &'static str = "'";
    pub const BLOCK_PARTITION_COLUMN_NAME: &'static str = "partition";
    pub const PARTITION_PRIMARY_MARKER: &'static str = "-primary-";
    pub const PARTITION_LIMIT_MARKER: &'static str = "-limit-";
    pub const PARTITION_OFFSET_MARKER: &'static str = "-offset-";
    pub const S3_ENHANCED_PARTITION_COLUMN_NAME: &'static str = "s3_column_name_list";
    pub const SSL_CERT_FILE_LOCATION: &'static str = "SSL_CERT_FILE";
    pub const SSL_CERT_FILE_LOCATION_VALUE: &'static str = "/tmp/cacert.pem";
    pub const SNOWFLAKE_SPLIT_QUERY_ID: &'static str = "query_id";
    pub const SNOWFLAKE_SPLIT_EXPORT_BUCKET: &'static str = "exportBucket";
    pub const SNOWFLAKE_SPLIT_OBJECT_KEY: &'static str = "s3ObjectKey";
    pub const AUTHENTICATOR: &'static str = "authenticator";
    pub const AUTH_CODE: &'static str = "auth_code";
    pub const CLIENT_ID: &'static str = "client_id";
    pub const TOKEN_URL: &'static str = "token_url";
    pub const REDIRECT_URI: &'static str = "redirect_uri";
    pub const CLIENT_SECRET: &'static str = "client_secret";
    pub const SF_USER: &'static str = "sfUser";
    pub const PEM_PRIVATE_KEY: &'static str = "pem_private_key";
    pub const PEM_PRIVATE_KEY_PASSPHRASE: &'static str = "pem_private_key_passphrase";
    pub const PRIVATE_KEY: &'static str = "privateKey";
    pub const USERNAME: &'static str = "username";
    pub const PASSWORD: &'static str = "password";
    pub const USERNAME_UPPERCASE: &'static str = "USERNAME";
    pub const PASSWORD_UPPERCASE: &'static str = "PASSWORD";
    pub const USER: &'static str = "user";
    pub const SNOWFLAKE_ENABLE_S3_EXPORT: &'static str = "snowflake_enable_s3_export";
    pub const STORAGE_INTEGRATION_CONFIG_KEY: &'static str = "snowflake_storage_integration_name";
    pub const DESCRIBE_STORAGE_INTEGRATION_TEMPLATE: &'static str = "DESC STORAGE INTEGRATION %s";
    pub const STORAGE_INTEGRATION_PROPERTY_KEY: &'static str = "property";
    pub const STORAGE_INTEGRATION_PROPERTY_VALUE_KEY: &'static str = "property_value";
    pub const STORAGE_INTEGRATION_BUCKET_KEY: &'static str = "STORAGE_ALLOWED_LOCATIONS";
    pub const STORAGE_INTEGRATION_STORAGE_PROVIDER_KEY: &'static str = "STORAGE_PROVIDER";
    pub const COUNT_RECORDS_QUERY: &'static str = "SELECT row_count\nFROM   information_schema.tables\nWHERE  table_type = 'BASE TABLE'\nAND table_schema= ?\nAND TABLE_NAME = ? ";
    pub const SHOW_PRIMARY_KEYS_QUERY: &'static str = "SHOW PRIMARY KEYS IN ";
    pub const COPY_INTO_QUERY_TEMPLATE: &'static str = "COPY INTO '%s' FROM (%s) STORAGE_INTEGRATION = %s HEADER = TRUE FILE_FORMAT = (TYPE = 'PARQUET', COMPRESSION = 'SNAPPY') MAX_FILE_SIZE = 52428800";
    pub const LIST_PAGINATED_TABLES_QUERY: &'static str = "SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ? ORDER BY TABLE_NAME LIMIT ? OFFSET ?";
    pub const VIEW_CHECK_QUERY: &'static str =
        "SELECT * FROM information_schema.views WHERE table_schema = ? AND table_name = ?";

    pub fn jdbc_properties() -> HashMap<String, String> {
        HashMap::from([
            ("databaseTerm".to_string(), "SCHEMA".to_string()),
            (
                "CLIENT_RESULT_COLUMN_CASE_INSENSITIVE".to_string(),
                "true".to_string(),
            ),
        ])
    }

    pub fn is_s3_export_enabled(config_options: &HashMap<String, String>) -> bool {
        config_options
            .get(Self::SNOWFLAKE_ENABLE_S3_EXPORT)
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    }
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
