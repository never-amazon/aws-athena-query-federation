use std::collections::HashMap;
use std::error::Error as _;
use std::hash::BuildHasher;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use aws_config::timeout::TimeoutConfig;
use aws_config::BehaviorVersion;
use aws_sdk_glue::operation::get_databases::GetDatabasesInput;
use aws_sdk_glue::operation::get_table::GetTableInput;
use aws_sdk_glue::operation::get_tables::GetTablesInput;
use aws_sdk_glue::types::{Column, Database, Table};
use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use super::metadata_handler::{require_identity, MetadataHandler};
use crate::data::SchemaBuilder;
use crate::domain::TableName;
use crate::exceptions::{
    AthenaConnectorException, FederationError, FederationSourceErrorCode, JavaCause, JavaThrowable,
    Result,
};
use crate::metadata::glue::{self, GlueClient};
use crate::metadata::{
    GetTableRequest, GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse, MetadataHeader, UNLIMITED_PAGE_SIZE_VALUE,
};
use crate::serde::java_hash::{java_hash_order_default, java_hash_set_order_by_hash};

pub const GET_TABLES_REQUEST_MAX_RESULTS: i32 = 100;
const DISABLE_GLUE: &str = "disable_glue";
const CATALOG_NAME_ENV_OVERRIDE: &str = "glue_catalog";
const CONNECT_TIMEOUT_MS: u64 = 250;
pub const SOURCE_TABLE_PROPERTY: &str = "sourceTable";
pub const COLUMN_NAME_MAPPING_PROPERTY: &str = "columnMapping";
pub const DATETIME_FORMAT_MAPPING_PROPERTY: &str = "datetimeFormatMapping";
pub const DATETIME_FORMAT_MAPPING_PROPERTY_NORMALIZED: &str = "datetimeFormatMappingNormalized";
pub const VIEW_METADATA_FIELD: &str = "_view_template";
pub const GLUE_TABLE_CONTAINS_PREVIOUSLY_UNSUPPORTED_TYPE: &str =
    "glueTableContainsPreviouslyUnsupportedType";

static TABLE_ARN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^arn:(?:aws|aws-cn|aws-us-gov):[a-z]+:[a-z1-9-]+:[0-9]{12}:table/",
        r"([^\n\r\x{85}\x{2028}\x{2029}]+)$"
    ))
    .expect("valid table arn pattern")
});

static FUNCTION_ARN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^arn:aws[a-zA-Z-]*?:lambda:[a-zA-Z0-9-]+:([0-9]{12}):function:[a-zA-Z0-9_-]+$")
        .expect("valid function arn pattern")
});

pub fn is_glue_disabled<S: BuildHasher>(config_options: &HashMap<String, String, S>) -> bool {
    config_options
        .get(DISABLE_GLUE)
        .is_some_and(|v| !v.eq_ignore_ascii_case("false"))
}

pub async fn glue_client_from_config<S: BuildHasher>(
    config_options: &HashMap<String, String, S>,
) -> Option<Arc<dyn GlueClient>> {
    if is_glue_disabled(config_options) {
        return None;
    }
    let timeouts = TimeoutConfig::builder()
        .connect_timeout(Duration::from_millis(CONNECT_TIMEOUT_MS))
        .build();
    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .timeout_config(timeouts)
        .load()
        .await;
    Some(Arc::new(aws_sdk_glue::Client::new(&sdk_config)))
}

fn glue_is_null(method: &str) -> FederationError {
    FederationError::null_pointer(format!(
        "Cannot invoke \"software.amazon.awssdk.services.glue.GlueClient.{method}\" because \"this.awsGlue\" is null"
    ))
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
struct GlueBuildError(aws_smithy_types::error::operation::BuildError);

impl JavaThrowable for GlueBuildError {
    fn java_class_name(&self) -> String {
        "java.lang.IllegalArgumentException".to_string()
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        self.0.source().map(JavaCause::foreign)
    }
}

fn build_error(e: aws_smithy_types::error::operation::BuildError) -> FederationError {
    FederationError::external(GlueBuildError(e))
}

fn function_owner(function_arn: Option<&str>) -> Option<String> {
    let function_arn = function_arn?;
    FUNCTION_ARN_REGEX
        .captures(function_arn)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[async_trait]
pub trait GlueMetadataHandler: MetadataHandler {
    fn aws_glue(&self) -> Option<&Arc<dyn GlueClient>>;

    fn get_catalog(&self, request: MetadataHeader<'_>) -> Result<Option<String>> {
        if let Some(catalog) = self
            .context()
            .config_options()
            .get(CATALOG_NAME_ENV_OVERRIDE)
        {
            return Ok(Some(catalog.clone()));
        }
        if let Some(context) = request.context {
            if let Some(owner) = function_owner(context.invoked_function_arn()) {
                tracing::debug!("Function Owner: {}", owner);
                return Ok(Some(owner));
            }
        }
        let identity = request.identity.ok_or_else(|| {
            FederationError::null_pointer("Cannot invoke \"com.amazonaws.athena.connector.lambda.security.FederatedIdentity.getAccount()\" because the return value of \"com.amazonaws.athena.connector.lambda.metadata.MetadataRequest.getIdentity()\" is null")
        })?;
        Ok(identity.account().map(str::to_string))
    }

    async fn do_list_schema_names_with_filter(
        &self,
        request: &ListSchemasRequest,
        filter: Option<&(dyn for<'d> Fn(&'d Database) -> bool + Sync)>,
    ) -> Result<ListSchemasResponse> {
        let identity = require_identity(request.identity())?;
        let override_config = self
            .context()
            .handler()
            .request_override_config(Some(identity.config_options()))
            .await?;
        let glue = self
            .aws_glue()
            .ok_or_else(|| glue_is_null("getDatabasesPaginator(...)"))?;
        let catalog = self.get_catalog(request.header())?;
        let mut schemas = Vec::new();
        let mut next_token: Option<String> = None;
        loop {
            let input = GetDatabasesInput::builder()
                .set_catalog_id(catalog.clone())
                .set_next_token(next_token.clone())
                .build()
                .map_err(build_error)?;
            let response = glue
                .get_databases(input, override_config.as_ref())
                .await
                .map_err(FederationError::AwsService)?;
            for database in response.database_list() {
                if filter.is_none_or(|f| f(database)) {
                    schemas.push(database.name().to_string());
                }
            }
            next_token = response
                .next_token()
                .filter(|token| !token.is_empty())
                .map(str::to_string);
            if next_token.is_none() {
                break;
            }
        }
        Ok(ListSchemasResponse::new(request.catalog_name(), schemas))
    }

    async fn do_list_tables_with_filter(
        &self,
        request: &ListTablesRequest,
        filter: Option<&(dyn for<'t> Fn(&'t Table) -> bool + Sync)>,
    ) -> Result<ListTablesResponse> {
        let mut tables: IndexSet<TableName> = IndexSet::new();
        let mut next_token = request.next_token().map(str::to_string);
        let mut page_size = request.page_size();
        let identity = require_identity(request.identity())?;
        let override_config = self
            .context()
            .handler()
            .request_override_config(Some(identity.config_options()))
            .await?;
        let glue = self
            .aws_glue()
            .ok_or_else(|| glue_is_null("getTables(...)"))?;
        let schema_name = request.schema_name().unwrap_or_default();
        tracing::info!(
            "Starting pagination at {} with page size {}",
            next_token.as_deref().unwrap_or("null"),
            page_size
        );
        loop {
            let mut builder = GetTablesInput::builder()
                .set_catalog_id(self.get_catalog(request.header())?)
                .set_database_name(request.schema_name().map(str::to_string))
                .set_next_token(next_token.clone());
            if page_size != UNLIMITED_PAGE_SIZE_VALUE {
                let max_results = page_size.min(GET_TABLES_REQUEST_MAX_RESULTS);
                builder = builder.max_results(max_results);
                page_size -= max_results;
            }
            let response = glue
                .get_tables(
                    builder.build().map_err(build_error)?,
                    override_config.as_ref(),
                )
                .await
                .map_err(FederationError::AwsService)?;
            for next in response.table_list() {
                if filter.is_none_or(|f| f(next)) {
                    tables.insert(TableName::new(schema_name, next.name()));
                }
            }
            next_token = response.next_token().map(str::to_string);
            tracing::info!(
                "{} tables returned. Next token is {}",
                tables.len(),
                next_token.as_deref().unwrap_or("null")
            );
            if next_token.is_none() || !(page_size == UNLIMITED_PAGE_SIZE_VALUE || page_size > 0) {
                break;
            }
        }
        Ok(ListTablesResponse::new(
            request.catalog_name(),
            java_hash_set_order_by_hash(tables.into_iter().collect(), TableName::java_hash_code),
            next_token,
        ))
    }

    async fn do_get_table_with_filter(
        &self,
        request: &GetTableRequest,
        filter: Option<&(dyn for<'t> Fn(&'t Table) -> bool + Sync)>,
    ) -> Result<GetTableResponse> {
        let table_name = request.table_name();
        let identity = require_identity(request.identity())?;
        let override_config = self
            .context()
            .handler()
            .request_override_config(Some(identity.config_options()))
            .await?;
        let glue = self
            .aws_glue()
            .ok_or_else(|| glue_is_null("getTable(...)"))?;
        let input = GetTableInput::builder()
            .set_catalog_id(self.get_catalog(request.header())?)
            .database_name(table_name.schema_name())
            .name(table_name.table_name())
            .build()
            .map_err(build_error)?;
        let response = glue
            .get_table(input, override_config.as_ref())
            .await
            .map_err(FederationError::AwsService)?;
        let table = response.table().ok_or_else(|| {
            FederationError::null_pointer("Cannot invoke \"software.amazon.awssdk.services.glue.model.Table.parameters()\" because \"table\" is null")
        })?;
        if filter.is_some_and(|f| !f(table)) {
            return Err(FederationError::connector(
                FederationSourceErrorCode::EntityNotFoundException,
                format!("No matching table found {table_name}"),
            ));
        }
        table_to_response(self, request, table)
    }

    fn convert_field(&self, name: &str, glue_type: Option<&str>) -> Result<Option<Field>> {
        let lexed = match glue_type {
            Some(glue_type) => glue::lex(name, glue_type),
            None => Err(FederationError::null_pointer(
                "Cannot invoke \"String.length()\" because \"this.input\" is null",
            )),
        };
        lexed.map_err(|ex| {
            if ex.is_runtime_exception() {
                FederationError::AthenaConnector(
                    AthenaConnectorException::new(
                        FederationSourceErrorCode::OperationNotSupportedException,
                        format!(
                            "Error converting field[{name}] with type[{}]",
                            glue_type.unwrap_or("null")
                        ),
                    )
                    .with_response(ex),
                )
            } else {
                ex
            }
        })
    }
}

fn table_to_response<H: GlueMetadataHandler + ?Sized>(
    handler: &H,
    request: &GetTableRequest,
    table: &Table,
) -> Result<GetTableResponse> {
    let table_name = request.table_name();
    let mut schema_builder = SchemaBuilder::new_builder();
    for (key, value) in table_parameters(table) {
        schema_builder.add_metadata(key, value);
    }
    let column_name_mapping = get_column_name_mapping(table)?;
    let date_time_format_mapping = date_time_format_mapping(table)?;
    let mut datetime_format_mapping_with_column_name: Vec<(String, String)> = Vec::new();
    let partition_keys: &[Column] = table.partition_keys();
    let partition_names: Vec<&str> = partition_keys
        .iter()
        .map(|next| mapped_name(&column_name_mapping, next.name()))
        .collect();
    let partition_cols: IndexSet<String> = java_hash_order_default(partition_names)
        .into_iter()
        .map(str::to_string)
        .collect();
    let storage_descriptor = table.storage_descriptor().ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"software.amazon.awssdk.services.glue.model.StorageDescriptor.columns()\" because the return value of \"software.amazon.awssdk.services.glue.model.Table.storageDescriptor()\" is null")
    })?;
    let mut glue_table_contains_previously_unsupported_type = false;
    for next in storage_descriptor.columns().iter().chain(partition_keys) {
        let raw_column_name = next.name();
        let mapped_column_name = mapped_name(&column_name_mapping, raw_column_name);
        let glue_type = next.r#type();
        tracing::info!(
            "Column {} with registered type {}",
            raw_column_name,
            glue_type.unwrap_or("null")
        );
        let arrow_field = handler
            .convert_field(mapped_column_name, glue_type)?
            .ok_or_else(|| {
                FederationError::null_pointer("Cannot invoke \"org.apache.arrow.vector.types.pojo.Field.getName()\" because \"field\" is null")
            })?;
        schema_builder.add_field(arrow_field.clone());
        if let Some(comment) = next.comment().filter(|c| !c.trim().is_empty()) {
            schema_builder.add_metadata(mapped_column_name, comment);
        }
        if let Some(format) = date_time_format_mapping.get(raw_column_name) {
            match datetime_format_mapping_with_column_name
                .iter_mut()
                .find(|(k, _)| k == mapped_column_name)
            {
                Some(entry) => entry.1.clone_from(format),
                None => datetime_format_mapping_with_column_name
                    .push((mapped_column_name.to_string(), format.clone())),
            }
        }
        if !glue_table_contains_previously_unsupported_type
            && is_previously_unsupported(glue_type.unwrap_or_default(), &arrow_field)
        {
            glue_table_contains_previously_unsupported_type = true;
        }
    }
    populate_datetime_format_mapping_if_available(
        &mut schema_builder,
        &datetime_format_mapping_with_column_name,
    );
    populate_source_table_name_if_available(table, &mut schema_builder);
    if let Some(view) = table.view_original_text().filter(|v| !v.is_empty()) {
        schema_builder.add_metadata(VIEW_METADATA_FIELD, view);
    }
    schema_builder.add_metadata(
        GLUE_TABLE_CONTAINS_PREVIOUSLY_UNSUPPORTED_TYPE,
        if glue_table_contains_previously_unsupported_type {
            "true"
        } else {
            "false"
        },
    );
    Ok(GetTableResponse::new(
        request.catalog_name(),
        table_name.clone(),
        schema_builder.build(),
        partition_cols,
    ))
}

fn mapped_name<'a>(mapping: &'a IndexMap<String, String>, raw: &'a str) -> &'a str {
    mapping.get(raw).map_or(raw, String::as_str)
}

fn table_parameters(table: &Table) -> Vec<(&str, &str)> {
    let Some(parameters) = table.parameters() else {
        return Vec::new();
    };
    let keys: Vec<&String> = parameters.keys().collect();
    crate::serde::java_hash::java_hash_map_order(keys)
        .into_iter()
        .filter_map(|k| parameters.get(k).map(|v| (k.as_str(), v.as_str())))
        .collect()
}

fn table_parameter<'a>(table: &'a Table, key: &str) -> Option<&'a str> {
    table
        .parameters()
        .and_then(|p| p.get(key))
        .map(String::as_str)
}

fn is_previously_unsupported(glue_type: &str, arrow_field: &Field) -> bool {
    let current_result = matches!(
        arrow_field.data_type(),
        DataType::Decimal32(..)
            | DataType::Decimal64(..)
            | DataType::Decimal128(..)
            | DataType::Decimal256(..)
            | DataType::Map(..)
    ) || glue_type.contains("set<");
    if !current_result {
        for child in crate::data::get_children(arrow_field) {
            if is_previously_unsupported("", &child) {
                return true;
            }
        }
    }
    current_result
}

pub fn populate_source_table_name_if_available(table: &Table, schema_builder: &mut SchemaBuilder) {
    if table_parameter(table, SOURCE_TABLE_PROPERTY).is_some() {
        return;
    }
    let location = table.storage_descriptor().and_then(|sd| sd.location());
    if let Some(location) = location {
        if let Some(source) = TABLE_ARN_REGEX.captures(location).and_then(|c| c.get(1)) {
            schema_builder.add_metadata(SOURCE_TABLE_PROPERTY, source.as_str());
        }
    }
}

pub fn get_source_table_name(schema: &Schema) -> Option<&str> {
    schema
        .metadata()
        .get(SOURCE_TABLE_PROPERTY)
        .map(String::as_str)
}

pub fn get_column_name_mapping(table: &Table) -> Result<IndexMap<String, String>> {
    match table_parameter(table, COLUMN_NAME_MAPPING_PROPERTY) {
        Some(param) if !param.is_empty() => split_map(param),
        _ => Ok(IndexMap::new()),
    }
}

fn date_time_format_mapping(table: &Table) -> Result<IndexMap<String, String>> {
    match table_parameter(table, DATETIME_FORMAT_MAPPING_PROPERTY) {
        Some(param) if !param.is_empty() => split_map(param),
        _ => Ok(IndexMap::new()),
    }
}

fn split_map(sequence: &str) -> Result<IndexMap<String, String>> {
    let mut map = IndexMap::new();
    for entry in sequence.split(',').map(str::trim) {
        let invalid =
            || FederationError::IllegalArgument(format!("Chunk [{entry}] is not a valid entry"));
        let mut fields = entry.split('=');
        let key = fields.next().ok_or_else(invalid)?;
        if map.contains_key(key) {
            return Err(FederationError::IllegalArgument(format!(
                "Duplicate key [{key}] found."
            )));
        }
        let value = fields.next().ok_or_else(invalid)?;
        if fields.next().is_some() {
            return Err(invalid());
        }
        map.insert(key.to_string(), value.to_string());
    }
    Ok(map)
}

fn populate_datetime_format_mapping_if_available(
    schema_builder: &mut SchemaBuilder,
    date_time_format_mapping: &[(String, String)],
) {
    if date_time_format_mapping.is_empty() {
        return;
    }
    let order = java_hash_order_default(date_time_format_mapping.iter().map(|(k, _)| k.as_str()));
    let joined: Vec<String> = order
        .into_iter()
        .filter_map(|k| {
            date_time_format_mapping
                .iter()
                .find(|(key, _)| key == k)
                .map(|(key, value)| format!("{key}={value}"))
        })
        .collect();
    schema_builder.add_metadata(
        DATETIME_FORMAT_MAPPING_PROPERTY_NORMALIZED,
        &joined.join(","),
    );
}
