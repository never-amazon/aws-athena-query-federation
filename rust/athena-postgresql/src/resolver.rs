use athena_federation_sdk::resolver::FederationSdkCasingMode;
use athena_jdbc::resolver::{CaseLookup, JdbcCaseResolver};

pub const POSTGRESQL_CASE_LOOKUP: CaseLookup = CaseLookup {
    schema_query: "SELECT schema_name FROM information_schema.schemata WHERE lower(schema_name) = ?",
    schema_column: "schema_name",
    table_queries: &[
        "SELECT table_name FROM information_schema.tables WHERE table_schema = ? AND lower(table_name) = ?",
        "select matviewname as \"table_name\" from pg_catalog.pg_matviews mv where schemaname = ? and lower(matviewname) = ?",
    ],
    table_column: "table_name",
};

pub fn postgresql_case_resolver(source_type: impl Into<String>) -> JdbcCaseResolver {
    JdbcCaseResolver::new(
        source_type,
        FederationSdkCasingMode::CaseInsensitiveSearch,
        FederationSdkCasingMode::None,
    )
    .with_lookup(POSTGRESQL_CASE_LOOKUP)
}
