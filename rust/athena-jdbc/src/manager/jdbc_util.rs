use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use indexmap::IndexMap;

use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::{FederationError, Result};

use crate::connection::{
    DatabaseConnectionConfig, DatabaseConnectionConfigBuilder, DEFAULT_CONNECTION_STRING_PROPERTY,
};
use crate::sql::{JdbcConnection, PreparedStatement, ResultSet, SqlError};
use crate::validate::not_blank;

const DEFAULT_CATALOG_PREFIX: &str = "lambda:";
const LAMBDA_FUNCTION_NAME_PROPERTY: &str = "AWS_LAMBDA_FUNCTION_NAME";
const TABLES_AND_VIEWS: &str = "Tables and Views";

fn insert_unique<V>(map: &mut IndexMap<String, V>, key: String, value: V) -> Result<()> {
    if map.contains_key(&key) {
        return Err(FederationError::IllegalArgument(format!(
            "Multiple entries with same key: {key}"
        )));
    }
    map.insert(key, value);
    Ok(())
}

fn owned<S: BuildHasher>(config_options: &HashMap<String, String, S>) -> HashMap<String, String> {
    config_options
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn connection_configs<S: BuildHasher>(
    engine: &str,
    config_options: &HashMap<String, String, S>,
) -> Result<(String, Vec<DatabaseConnectionConfig>)> {
    let function_name = not_blank(
        config_options
            .get(LAMBDA_FUNCTION_NAME_PROPERTY)
            .map(String::as_str),
        "Lambda function name not present in environment.",
    )?
    .to_string();
    let configs = DatabaseConnectionConfigBuilder::new()
        .engine(engine)
        .properties(owned(config_options))
        .build()?;
    if configs.is_empty() {
        return Err(FederationError::invalid_input(
            "At least one connection string required.",
        ));
    }
    Ok((function_name, configs))
}

fn missing_default() -> FederationError {
    FederationError::invalid_input(format!(
        "Must provide connection parameters for default database instance {DEFAULT_CONNECTION_STRING_PROPERTY}"
    ))
}

pub fn single_database_config_from_env<S: BuildHasher>(
    database_engine: &str,
    config_options: &HashMap<String, String, S>,
) -> Result<DatabaseConnectionConfig> {
    let configs = DatabaseConnectionConfigBuilder::build_from_system_env(
        database_engine,
        owned(config_options),
    )?;
    configs
        .into_iter()
        .find(|config| config.catalog() == DEFAULT_CONNECTION_STRING_PROPERTY && config.engine() == database_engine)
        .ok_or_else(|| {
            FederationError::invalid_input(format!(
                "Must provide default connection string parameter {DEFAULT_CONNECTION_STRING_PROPERTY} for database type {database_engine}"
            ))
        })
}

pub fn handler_map<H: ?Sized, S: BuildHasher>(
    engine: &str,
    config_options: &HashMap<String, String, S>,
    make_handler: impl Fn(&DatabaseConnectionConfig) -> Result<Arc<H>>,
) -> Result<IndexMap<String, Arc<H>>> {
    let (function_name, configs) = connection_configs(engine, config_options)?;
    let mut handlers: IndexMap<String, Arc<H>> = IndexMap::new();
    let mut default_present = false;
    for config in &configs {
        let handler = make_handler(config)?;
        insert_unique(
            &mut handlers,
            config.catalog().to_string(),
            Arc::clone(&handler),
        )?;
        if config.catalog() == DEFAULT_CONNECTION_STRING_PROPERTY {
            insert_unique(
                &mut handlers,
                format!("{DEFAULT_CATALOG_PREFIX}{function_name}"),
                handler,
            )?;
            default_present = true;
        }
    }
    if !default_present {
        return Err(missing_default());
    }
    Ok(handlers)
}

pub async fn tables(
    connection: &dyn JdbcConnection,
    database_name: &str,
) -> std::result::Result<Vec<TableName>, SqlError> {
    let sql = "SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ?";
    let mut prepared_statement = connection.prepare_statement(sql).await?;
    prepared_statement.bind(database_name);
    tracing::debug!(
        "Prepared Statement for getting tables in schema {} : {}",
        database_name,
        prepared_statement.sql()
    );
    table_metadata(connection, &prepared_statement, TABLES_AND_VIEWS).await
}

pub async fn table_metadata(
    connection: &dyn JdbcConnection,
    prepared_statement: &PreparedStatement,
    table_type: &str,
) -> std::result::Result<Vec<TableName>, SqlError> {
    let listed = async {
        let mut result_set = connection.execute_query(prepared_statement).await?;
        let mut list = Vec::new();
        while result_set.next().await? {
            list.push(read_schema_table_name(&result_set)?);
        }
        Ok(list)
    }
    .await;
    if let Err(ex) = &listed {
        tracing::warn!(
            "Unable to return list of {} from data source! {}",
            table_type,
            ex
        );
    }
    listed
}

pub fn schema_table_name(result_set: &ResultSet) -> Result<TableName> {
    Ok(read_schema_table_name(result_set)?)
}

fn read_schema_table_name(result_set: &ResultSet) -> std::result::Result<TableName, SqlError> {
    Ok(TableName::new(
        result_set.get_string("TABLE_SCHEM")?.unwrap_or_default(),
        result_set.get_string("TABLE_NAME")?.unwrap_or_default(),
    ))
}
