pub struct EnvironmentConstants;

impl EnvironmentConstants {
    pub const CONNECT_TIMEOUT: i32 = 2000;
    pub const DEFAULT_GLUE_CONNECTION: &'static str = "glue_connection";
    pub const SECRET_NAME: &'static str = "secret_name";
    pub const SPILL_KMS_KEY_ID: &'static str = "spill_kms_key_id";
    pub const KMS_KEY_ID: &'static str = "kms_key_id";
    pub const DEFAULT: &'static str = "default";
    pub const DEFAULT_DOCDB: &'static str = "default_docdb";
    pub const DEFAULT_HBASE: &'static str = "default_hbase";
    pub const HOST: &'static str = "HOST";
    pub const PORT: &'static str = "PORT";
    pub const JDBC_PARAMS: &'static str = "JDBC_PARAMS";
    pub const DATABASE: &'static str = "DATABASE";
    pub const SESSION_CONFS: &'static str = "SESSION_CONFS";
    pub const HIVE_CONFS: &'static str = "HIVE_CONFS";
    pub const HIVE_VARS: &'static str = "HIVE_VARS";
    pub const WAREHOUSE: &'static str = "WAREHOUSE";
    pub const SCHEMA: &'static str = "SCHEMA";
    pub const PROJECT_ID: &'static str = "PROJECT_ID";
    pub const CLUSTER_RES_ID: &'static str = "CLUSTER_RESOURCE_ID";
    pub const GRAPH_TYPE: &'static str = "GRAPH_TYPE";
    pub const HBASE_PORT: &'static str = "HBASE_PORT";
    pub const ZOOKEEPER_PORT: &'static str = "ZOOKEEPER_PORT";
    pub const CUSTOM_AUTH_TYPE: &'static str = "CUSTOM_AUTH_TYPE";
    pub const GLUE_CERTIFICATES_S3_REFERENCE: &'static str = "CERTIFICATE_S3_REFERENCE";
    pub const ENFORCE_SSL: &'static str = "ENFORCE_SSL";
    pub const FAS_TOKEN: &'static str = "fas_token";
    pub const SERVICE_KMS: &'static str = "service_kms_key_id";
    pub const CATALOG_CASING_FILTER: &'static str = "CATALOG_CASING_FILTER";
    pub const UPPERCASE_ONLY: &'static str = "UPPERCASE_ONLY";
}
