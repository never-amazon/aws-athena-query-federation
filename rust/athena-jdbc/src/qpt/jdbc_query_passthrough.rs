use athena_federation_sdk::metadata::optimizations::querypassthrough::QueryPassthroughSignature;

pub const NAME: &str = "query";
pub const SCHEMA_NAME: &str = "system";
pub const QUERY: &str = "QUERY";
pub const ARGUMENTS: [&str; 1] = [QUERY];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct JdbcQueryPassthrough;

impl QueryPassthroughSignature for JdbcQueryPassthrough {
    fn function_schema(&self) -> String {
        SCHEMA_NAME.to_string()
    }

    fn function_name(&self) -> String {
        NAME.to_string()
    }

    fn function_arguments(&self) -> Vec<String> {
        ARGUMENTS.iter().map(ToString::to_string).collect()
    }
}
