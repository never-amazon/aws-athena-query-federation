use athena_federation_sdk::metadata::QueryPassthroughSignature;

pub const NAME: &str = "query";
pub const SCHEMA_NAME: &str = "system";
pub const QUERY: &str = "QUERY";
pub const ARGUMENTS: [&str; 1] = [QUERY];

pub const JDBC_QUERY_PASSTHROUGH: QueryPassthroughSignature = QueryPassthroughSignature {
    schema: SCHEMA_NAME,
    name: NAME,
    arguments: &ARGUMENTS,
};
