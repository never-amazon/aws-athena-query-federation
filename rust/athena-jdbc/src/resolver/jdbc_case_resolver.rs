use std::collections::HashMap;

use async_trait::async_trait;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::resolver::{CaseResolver, FederationSdkCasingMode};
use athena_federation_sdk::{FederationError, Result};

use crate::sql::JdbcConnection;

pub const ANNOTATION_SUPPORTED_SOURCE_TYPES: [&str; 2] = ["snowflake", "saphana"];
const ANNOTATION_CASE_UPPER: &str = "upper";
const ANNOTATION_CASE_LOWER: &str = "lower";

fn unsupported_for_type(resolver: &CaseResolver, suffix: &str) -> FederationError {
    FederationError::UnsupportedOperation(format!(
        "CASE_INSENSITIVE_SEARCH is not supported for type: '{}'{suffix}",
        resolver.source_type
    ))
}

#[async_trait]
pub trait JdbcCaseResolver: Send + Sync {
    fn case_resolver(&self) -> &CaseResolver;

    fn source_type(&self) -> &str {
        &self.case_resolver().source_type
    }

    fn get_casing_mode(
        &self,
        config_options: &HashMap<String, String>,
    ) -> Result<FederationSdkCasingMode> {
        self.case_resolver().get_casing_mode(config_options)
    }

    async fn do_get_schema_name_case_insensitively(
        &self,
        _connection: &dyn JdbcConnection,
        _schema_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        Err(unsupported_for_type(self.case_resolver(), ""))
    }

    async fn do_get_table_name_case_insensitively(
        &self,
        _connection: &dyn JdbcConnection,
        _schema_name_in_correct_case: &str,
        _table_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        Err(unsupported_for_type(self.case_resolver(), ""))
    }

    fn get_case_insensitively_schema_name_query_template(&self) -> Result<String> {
        Err(unsupported_for_type(
            self.case_resolver(),
            ", please provide SchemaNameQuery",
        ))
    }

    fn get_case_insensitively_schema_name_column_key(&self) -> Result<String> {
        Err(unsupported_for_type(
            self.case_resolver(),
            ", please provide SchemaNameColumnKey",
        ))
    }

    fn get_case_insensitively_table_name_query_template(&self) -> Result<Vec<String>> {
        Err(unsupported_for_type(
            self.case_resolver(),
            ", please provide TableNameQuery",
        ))
    }

    fn get_case_insensitively_table_name_column_key(&self) -> Result<String> {
        Err(unsupported_for_type(
            self.case_resolver(),
            ", please provide TableNameColumnKey",
        ))
    }

    fn get_table_name_from_query_annotation(&self, table: &TableName) -> TableName {
        table_name_from_query_annotation(table)
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

#[async_trait]
pub trait JdbcCaseResolverExt: JdbcCaseResolver {
    async fn get_adjusted_table_name_object(
        &self,
        connection: &dyn JdbcConnection,
        table_name_object: &TableName,
        config_options: &HashMap<String, String>,
    ) -> Result<TableName>;

    async fn get_adjusted_schema_name_string(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_input: &str,
        config_options: &HashMap<String, String>,
    ) -> Result<String>;

    async fn get_adjusted_table_name_string(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_in_correct_case: &str,
        table_name_input: &str,
        config_options: &HashMap<String, String>,
    ) -> Result<String>;
}

#[async_trait]
impl<T: JdbcCaseResolver + ?Sized> JdbcCaseResolverExt for T {
    async fn get_adjusted_table_name_object(
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

    async fn get_adjusted_schema_name_string(
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
                    .do_get_schema_name_case_insensitively(
                        connection,
                        schema_name_input,
                        config_options,
                    )
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

    async fn get_adjusted_table_name_string(
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
                    .do_get_table_name_case_insensitively(
                        connection,
                        schema_name_in_correct_case,
                        table_name_input,
                        config_options,
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
