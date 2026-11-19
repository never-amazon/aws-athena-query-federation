use std::collections::HashMap;

use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::resolver::{CaseResolver, FederationSdkCasingMode};
use athena_federation_sdk::{FederationError, Result};

use crate::manager::PreparedStatementBuilder;
use crate::sql::{JdbcConnection, SqlError};
use crate::validate::runtime_with_cause;

pub const ANNOTATION_SUPPORTED_SOURCE_TYPES: [&str; 2] = ["snowflake", "saphana"];
const ANNOTATION_CASE_UPPER: &str = "upper";
const ANNOTATION_CASE_LOWER: &str = "lower";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseLookup {
    pub schema_query: &'static str,
    pub schema_column: &'static str,
    pub table_queries: &'static [&'static str],
    pub table_column: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnnotationFallback {
    #[default]
    Unchanged,
    Uppercase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JdbcCaseResolver {
    base: CaseResolver,
    lookup: Option<CaseLookup>,
    annotation_fallback: AnnotationFallback,
}

enum QueryFailure {
    Sql(SqlError),
    Other(FederationError),
}

impl From<SqlError> for QueryFailure {
    fn from(e: SqlError) -> Self {
        QueryFailure::Sql(e)
    }
}

async fn query_column(
    connection: &dyn JdbcConnection,
    query: &str,
    parameters: Vec<String>,
    column_key: &str,
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
        let candidate = result_set.get_string(column_key)?;
        tracing::debug!(
            "Case insensitive search on columLabel: {}, schema name: {:?}",
            column_key,
            candidate
        );
        results.push(candidate.unwrap_or_else(|| "null".to_string()));
    }
    Ok(results)
}

impl JdbcCaseResolver {
    pub fn new(
        source_type: impl Into<String>,
        non_glue_default_mode: FederationSdkCasingMode,
        glue_connection_default_mode: FederationSdkCasingMode,
    ) -> JdbcCaseResolver {
        JdbcCaseResolver {
            base: CaseResolver::new(
                source_type,
                non_glue_default_mode,
                glue_connection_default_mode,
            ),
            lookup: None,
            annotation_fallback: AnnotationFallback::Unchanged,
        }
    }

    #[must_use]
    pub fn with_lookup(mut self, lookup: CaseLookup) -> JdbcCaseResolver {
        self.lookup = Some(lookup);
        self
    }

    #[must_use]
    pub fn with_annotation_fallback(mut self, fallback: AnnotationFallback) -> JdbcCaseResolver {
        self.annotation_fallback = fallback;
        self
    }

    pub fn case_resolver(&self) -> &CaseResolver {
        &self.base
    }

    pub fn lookup(&self) -> Option<&CaseLookup> {
        self.lookup.as_ref()
    }

    pub fn source_type(&self) -> &str {
        &self.base.source_type
    }

    pub fn get_casing_mode(
        &self,
        config_options: &HashMap<String, String>,
    ) -> Result<FederationSdkCasingMode> {
        self.base.get_casing_mode(config_options)
    }

    fn require_lookup(&self, suffix: &str) -> Result<&CaseLookup> {
        self.lookup.as_ref().ok_or_else(|| {
            FederationError::UnsupportedOperation(format!(
                "CASE_INSENSITIVE_SEARCH is not supported for type: '{}', please provide {suffix}",
                self.base.source_type
            ))
        })
    }

    pub async fn schema_name_case_insensitively(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_input: &str,
    ) -> Result<Vec<String>> {
        let lookup = self.require_lookup("SchemaNameQuery")?;
        query_column(
            connection,
            lookup.schema_query,
            vec![schema_name_input.to_lowercase()],
            lookup.schema_column,
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

    pub async fn table_name_case_insensitively(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_in_correct_case: &str,
        table_name_input: &str,
    ) -> Result<Vec<String>> {
        let lookup = self.require_lookup("TableNameQuery")?;
        tracing::debug!(
            "Number of query templates: {} for case insensitive search for tableName",
            lookup.table_queries.len()
        );
        let mut results = Vec::new();
        for template in lookup.table_queries {
            let found = query_column(
                connection,
                template,
                vec![
                    schema_name_in_correct_case.to_string(),
                    table_name_input.to_lowercase(),
                ],
                lookup.table_column,
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

    pub fn get_table_name_from_query_annotation(&self, table: &TableName) -> TableName {
        if self.annotation_fallback == AnnotationFallback::Uppercase {
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
        }
        table_name_from_query_annotation(table)
    }

    pub async fn get_adjusted_table_name_object(
        &self,
        connection: &dyn JdbcConnection,
        table_name_object: &TableName,
        config_options: &HashMap<String, String>,
    ) -> Result<TableName> {
        if self.get_casing_mode(config_options)? == FederationSdkCasingMode::Annotation {
            if ANNOTATION_SUPPORTED_SOURCE_TYPES.contains(&self.source_type()) {
                return Ok(self.get_table_name_from_query_annotation(table_name_object));
            }
            return Err(FederationError::UnsupportedOperation(format!(
                "casing mode `ANNOTATION` is not supported for type: '{}'",
                self.source_type()
            )));
        }
        let schema_name_in_correct_casing = self
            .get_adjusted_schema_name_string(
                connection,
                table_name_object.schema_name(),
                config_options,
            )
            .await?;
        let table_name_in_correct_casing = self
            .get_adjusted_table_name_string(
                connection,
                &schema_name_in_correct_casing,
                table_name_object.table_name(),
                config_options,
            )
            .await?;
        Ok(TableName::new(
            schema_name_in_correct_casing,
            table_name_in_correct_casing,
        ))
    }

    pub async fn get_adjusted_schema_name_string(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_input: &str,
        config_options: &HashMap<String, String>,
    ) -> Result<String> {
        match self.get_casing_mode(config_options)? {
            FederationSdkCasingMode::Lower => {
                tracing::debug!(
                    "casing mode is `LOWER`: adjusting casing from input to lower case from:{}, to:{}",
                    schema_name_input,
                    schema_name_input.to_lowercase()
                );
                Ok(schema_name_input.to_lowercase())
            }
            FederationSdkCasingMode::Upper => {
                tracing::debug!(
                    "casing mode is `UPPER`: adjusting casing from input to upper case from:{}, to:{}",
                    schema_name_input,
                    schema_name_input.to_uppercase()
                );
                Ok(schema_name_input.to_uppercase())
            }
            FederationSdkCasingMode::CaseInsensitiveSearch => {
                let strings = self
                    .schema_name_case_insensitively(connection, schema_name_input)
                    .await?;
                if strings.len() != 1 {
                    return Err(FederationError::invalid_input(format!(
                        "Schema name case insensitive match failed. More than one Schema that matches '{schema_name_input}'"
                    )));
                }
                tracing::info!(
                    "casing mode is `CASE_INSENSITIVE_SEARCH`: adjusting casing for `Schema` from {}, to {}",
                    schema_name_input,
                    strings[0]
                );
                Ok(strings.into_iter().next().unwrap_or_default())
            }
            FederationSdkCasingMode::Annotation => Err(FederationError::UnsupportedOperation(
                "casing mode `ANNOTATION` is not supported for Name level. Please use CASE_INSENSITIVE_SEARCH mode".to_string(),
            )),
            FederationSdkCasingMode::None => {
                tracing::debug!(
                    "casing mode is `NONE`: not adjust casing from input: {}",
                    schema_name_input
                );
                Ok(schema_name_input.to_string())
            }
        }
    }

    pub async fn get_adjusted_table_name_string(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_in_correct_case: &str,
        table_name_input: &str,
        config_options: &HashMap<String, String>,
    ) -> Result<String> {
        match self.get_casing_mode(config_options)? {
            FederationSdkCasingMode::Lower => Ok(table_name_input.to_lowercase()),
            FederationSdkCasingMode::Upper => Ok(table_name_input.to_uppercase()),
            FederationSdkCasingMode::CaseInsensitiveSearch => {
                let strings = self
                    .table_name_case_insensitively(
                        connection,
                        schema_name_in_correct_case,
                        table_name_input,
                    )
                    .await?;
                if strings.len() != 1 {
                    return Err(FederationError::invalid_input(format!(
                        "Table name case insensitive match failed. More than one table that matches '{table_name_input}' was returned from Database {schema_name_in_correct_case}"
                    )));
                }
                tracing::info!(
                    "casing mode is `ANNOTATION`: adjusting casing for `Table` from {}, to {}",
                    table_name_input,
                    strings[0]
                );
                Ok(strings.into_iter().next().unwrap_or_default())
            }
            FederationSdkCasingMode::Annotation => Err(FederationError::UnsupportedOperation(
                "casing mode `ANNOTATION` is not supported for Name level. Please use CASE_INSENSITIVE_SEARCH mode".to_string(),
            )),
            FederationSdkCasingMode::None => Ok(table_name_input.to_string()),
        }
    }
}

pub fn table_name_from_query_annotation(table: &TableName) -> TableName {
    tracing::info!("getTableNameFromQueryAnnotation: {}", table);
    if !table.table_name().contains('@') {
        return TableName::new(table.schema_name(), table.table_name());
    }
    let parts: Vec<&str> = table.table_name().split('@').collect();
    let table_name = parts[0];
    let hint = parts.get(1).copied().unwrap_or_default();
    let mut schema_case = ANNOTATION_CASE_UPPER.to_string();
    let mut table_case = ANNOTATION_CASE_UPPER.to_string();
    for detail in hint.split('&') {
        let hint_detail: Vec<&str> = detail.split('=').collect();
        if hint_detail[0].contains("schema") {
            schema_case = hint_detail.get(1).copied().unwrap_or_default().to_string();
        } else if hint_detail[0].contains("table") {
            table_case = hint_detail.get(1).copied().unwrap_or_default().to_string();
        }
    }
    let schema = table.schema_name();
    let is = |value: &str, expected: &str| value.eq_ignore_ascii_case(expected);
    if is(&schema_case, ANNOTATION_CASE_LOWER) && is(&table_case, ANNOTATION_CASE_LOWER) {
        TableName::new(schema.to_lowercase(), table_name.to_lowercase())
    } else if is(&schema_case, ANNOTATION_CASE_LOWER) && is(&table_case, ANNOTATION_CASE_UPPER) {
        TableName::new(schema.to_lowercase(), table_name.to_uppercase())
    } else if is(&schema_case, ANNOTATION_CASE_UPPER) && is(&table_case, ANNOTATION_CASE_LOWER) {
        TableName::new(schema.to_uppercase(), table_name.to_lowercase())
    } else {
        TableName::new(schema.to_uppercase(), table_name.to_uppercase())
    }
}
