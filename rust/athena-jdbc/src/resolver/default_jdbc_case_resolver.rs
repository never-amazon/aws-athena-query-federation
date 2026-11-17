use std::collections::HashMap;

use async_trait::async_trait;
use athena_federation_sdk::resolver::{CaseResolver, FederationSdkCasingMode};
use athena_federation_sdk::{FederationError, Result};

use super::jdbc_case_resolver::JdbcCaseResolver;
use crate::manager::PreparedStatementBuilder;
use crate::sql::{JdbcConnection, SqlException};
use crate::validate::runtime_with_cause;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultJdbcCaseResolver {
    base: CaseResolver,
}

impl DefaultJdbcCaseResolver {
    pub fn new(source_type: impl Into<String>) -> DefaultJdbcCaseResolver {
        DefaultJdbcCaseResolver::new_with_casing_modes(
            source_type,
            FederationSdkCasingMode::None,
            FederationSdkCasingMode::None,
        )
    }

    pub fn new_with_casing_modes(
        source_type: impl Into<String>,
        non_glue_default_mode: FederationSdkCasingMode,
        glue_connection_default_mode: FederationSdkCasingMode,
    ) -> DefaultJdbcCaseResolver {
        DefaultJdbcCaseResolver {
            base: CaseResolver::new(
                source_type,
                non_glue_default_mode,
                glue_connection_default_mode,
            ),
        }
    }
}

enum QueryFailure {
    Sql(SqlException),
    Other(FederationError),
}

impl From<SqlException> for QueryFailure {
    fn from(e: SqlException) -> Self {
        QueryFailure::Sql(e)
    }
}

async fn query_column(
    connection: &dyn JdbcConnection,
    query: &str,
    parameters: Vec<String>,
    column_key: &(dyn Fn() -> Result<String> + Sync),
) -> std::result::Result<Vec<String>, QueryFailure> {
    let statement = PreparedStatementBuilder::new()
        .with_connection(connection)
        .with_query(query)
        .and_then(|b| b.with_parameters(parameters))
        .map_err(QueryFailure::Other)?
        .build()
        .await
        .map_err(QueryFailure::Other)?;
    let mut results = Vec::new();
    let mut result_set = connection.execute_query(&statement).await?;
    while result_set.next().await? {
        let key = column_key().map_err(QueryFailure::Other)?;
        let candidate = result_set.get_string(key.as_str())?;
        tracing::debug!(
            "Case insensitive search on columLabel: {}, schema name: {:?}",
            key,
            candidate
        );
        results.push(candidate.unwrap_or_else(|| "null".to_string()));
    }
    Ok(results)
}

pub async fn default_do_get_schema_name_case_insensitively<R: JdbcCaseResolver + ?Sized>(
    resolver: &R,
    connection: &dyn JdbcConnection,
    schema_name_input: &str,
) -> Result<Vec<String>> {
    let query = resolver.get_case_insensitively_schema_name_query_template()?;
    let column_key = || resolver.get_case_insensitively_schema_name_column_key();
    query_column(
        connection,
        &query,
        vec![schema_name_input.to_lowercase()],
        &column_key,
    )
    .await
    .map_err(|failure| match failure {
        QueryFailure::Other(e) => e,
        QueryFailure::Sql(e) => runtime_with_cause(
            format!("getSchemaNameCaseInsensitively query failed for {schema_name_input}"),
            FederationError::from(e),
        ),
    })
}

pub async fn default_do_get_table_name_case_insensitively<R: JdbcCaseResolver + ?Sized>(
    resolver: &R,
    connection: &dyn JdbcConnection,
    schema_name_in_correct_case: &str,
    table_name_input: &str,
) -> Result<Vec<String>> {
    let templates = resolver.get_case_insensitively_table_name_query_template()?;
    tracing::debug!(
        "Number of query templates: {} for case insensitive search for tableName",
        templates.len()
    );
    let column_key = || resolver.get_case_insensitively_table_name_column_key();
    let mut results = Vec::new();
    for template in templates {
        let found = query_column(
            connection,
            &template,
            vec![
                schema_name_in_correct_case.to_string(),
                table_name_input.to_lowercase(),
            ],
            &column_key,
        )
        .await
        .map_err(|failure| match failure {
            QueryFailure::Other(e) => e,
            QueryFailure::Sql(e) => runtime_with_cause(
                format!(
                    "getTableNameCaseInsensitively query failed for schema: {schema_name_in_correct_case} tableNameInput: {table_name_input}"
                ),
                FederationError::from(e),
            ),
        })?;
        results.extend(found);
    }
    Ok(results)
}

#[async_trait]
impl JdbcCaseResolver for DefaultJdbcCaseResolver {
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
}
