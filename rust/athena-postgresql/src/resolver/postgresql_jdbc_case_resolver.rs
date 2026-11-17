use std::collections::HashMap;

use async_trait::async_trait;
use athena_federation_sdk::resolver::{CaseResolver, FederationSdkCasingMode};
use athena_federation_sdk::Result;
use athena_jdbc::resolver::{
    default_do_get_schema_name_case_insensitively, default_do_get_table_name_case_insensitively,
    JdbcCaseResolver,
};
use athena_jdbc::sql::JdbcConnection;

const SCHEMA_NAME_QUERY_TEMPLATE: &str =
    "SELECT schema_name FROM information_schema.schemata WHERE lower(schema_name) = ?";
const TABLE_NAME_QUERY_TEMPLATE: &str = "SELECT table_name FROM information_schema.tables WHERE table_schema = ? AND lower(table_name) = ?";
const TABLE_NAME_QUERY_MATERIALIZED_VIEW: &str = "select matviewname as \"table_name\" from pg_catalog.pg_matviews mv where schemaname = ? and lower(matviewname) = ?";
const SCHEMA_NAME_COLUMN_KEY: &str = "schema_name";
const TABLE_NAME_COLUMN_KEY: &str = "table_name";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostGreSqlJdbcCaseResolver {
    base: CaseResolver,
}

impl PostGreSqlJdbcCaseResolver {
    pub fn new(source_type: impl Into<String>) -> PostGreSqlJdbcCaseResolver {
        PostGreSqlJdbcCaseResolver {
            base: CaseResolver::new(
                source_type,
                FederationSdkCasingMode::CaseInsensitiveSearch,
                FederationSdkCasingMode::None,
            ),
        }
    }
}

#[async_trait]
impl JdbcCaseResolver for PostGreSqlJdbcCaseResolver {
    fn case_resolver(&self) -> &CaseResolver {
        &self.base
    }

    async fn do_get_schema_name_case_insensitively(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        default_do_get_schema_name_case_insensitively(self, connection, schema_name_input).await
    }

    async fn do_get_table_name_case_insensitively(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_in_correct_case: &str,
        table_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        default_do_get_table_name_case_insensitively(
            self,
            connection,
            schema_name_in_correct_case,
            table_name_input,
        )
        .await
    }

    fn get_case_insensitively_schema_name_query_template(&self) -> Result<String> {
        Ok(SCHEMA_NAME_QUERY_TEMPLATE.to_string())
    }

    fn get_case_insensitively_schema_name_column_key(&self) -> Result<String> {
        Ok(SCHEMA_NAME_COLUMN_KEY.to_string())
    }

    fn get_case_insensitively_table_name_query_template(&self) -> Result<Vec<String>> {
        Ok(vec![
            TABLE_NAME_QUERY_TEMPLATE.to_string(),
            TABLE_NAME_QUERY_MATERIALIZED_VIEW.to_string(),
        ])
    }

    fn get_case_insensitively_table_name_column_key(&self) -> Result<String> {
        Ok(TABLE_NAME_COLUMN_KEY.to_string())
    }
}
