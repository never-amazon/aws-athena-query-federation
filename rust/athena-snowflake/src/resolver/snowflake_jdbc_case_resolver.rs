use std::collections::HashMap;

use async_trait::async_trait;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::resolver::{CaseResolver, FederationSdkCasingMode};
use athena_federation_sdk::Result;
use athena_jdbc::resolver::{
    default_do_get_schema_name_case_insensitively, default_do_get_table_name_case_insensitively,
    table_name_from_query_annotation, JdbcCaseResolver,
};
use athena_jdbc::sql::JdbcConnection;

const SCHEMA_NAME_QUERY_TEMPLATE: &str =
    "select * from INFORMATION_SCHEMA.SCHEMATA where lower(SCHEMA_NAME) = ?";
const TABLE_NAME_QUERY_TEMPLATE: &str =
    "select * from INFORMATION_SCHEMA.TABLES where TABLE_SCHEMA = ? and lower(TABLE_NAME) = ?";
const SCHEMA_NAME_COLUMN_KEY: &str = "SCHEMA_NAME";
const TABLE_NAME_COLUMN_KEY: &str = "TABLE_NAME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnowflakeJdbcCaseResolver {
    base: CaseResolver,
}

impl SnowflakeJdbcCaseResolver {
    pub fn new(source_type: impl Into<String>) -> SnowflakeJdbcCaseResolver {
        Self::new_with_casing_mode(source_type, FederationSdkCasingMode::None)
    }

    pub fn new_with_casing_mode(
        source_type: impl Into<String>,
        casing_mode: FederationSdkCasingMode,
    ) -> SnowflakeJdbcCaseResolver {
        SnowflakeJdbcCaseResolver {
            base: CaseResolver::new(source_type, casing_mode, casing_mode),
        }
    }
}

#[async_trait]
impl JdbcCaseResolver for SnowflakeJdbcCaseResolver {
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
        Ok(vec![TABLE_NAME_QUERY_TEMPLATE.to_string()])
    }

    fn get_case_insensitively_table_name_column_key(&self) -> Result<String> {
        Ok(TABLE_NAME_COLUMN_KEY.to_string())
    }

    fn get_table_name_from_query_annotation(&self, table: &TableName) -> TableName {
        tracing::debug!("Snowflake: getTableNameFromQueryAnnotation: {table}");
        if !table.table_name().contains('@') {
            tracing::info!(
                "Snowflake: ANNOTATION enabled, but no ANNOTATION found, back to original behavior UPPER_CASE.: {table}"
            );
            return TableName::new(
                table.schema_name().to_uppercase(),
                table.table_name().to_uppercase(),
            );
        }
        table_name_from_query_annotation(table)
    }
}
