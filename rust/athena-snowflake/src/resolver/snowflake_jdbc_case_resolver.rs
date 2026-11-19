use athena_federation_sdk::resolver::FederationSdkCasingMode;
use athena_jdbc::resolver::{AnnotationFallback, CaseLookup, JdbcCaseResolver};

pub const SNOWFLAKE_CASE_LOOKUP: CaseLookup = CaseLookup {
    schema_query: "select * from INFORMATION_SCHEMA.SCHEMATA where lower(SCHEMA_NAME) = ?",
    schema_column: "SCHEMA_NAME",
    table_queries: &[
        "select * from INFORMATION_SCHEMA.TABLES where TABLE_SCHEMA = ? and lower(TABLE_NAME) = ?",
    ],
    table_column: "TABLE_NAME",
};

pub fn snowflake_case_resolver(
    source_type: impl Into<String>,
    casing_mode: FederationSdkCasingMode,
) -> JdbcCaseResolver {
    JdbcCaseResolver::new(source_type, casing_mode, casing_mode)
        .with_lookup(SNOWFLAKE_CASE_LOOKUP)
        .with_annotation_fallback(AnnotationFallback::Uppercase)
}
