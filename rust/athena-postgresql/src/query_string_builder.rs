use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::Result;
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{
    FederationExpressionParser, JdbcSplitQueryBuilder, JdbcSplitQueryBuilderBase,
};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement};

use crate::driver::check_parameters;
use crate::metadata_handler::PostGreSqlMetadataHandler;
use crate::sql_dialect::PostgresqlSqlDialect;

pub struct PostGreSqlQueryStringBuilder {
    base: JdbcSplitQueryBuilderBase,
}

impl PostGreSqlQueryStringBuilder {
    pub fn new(
        quote_characters: impl Into<String>,
        federation_expression_parser: Arc<dyn FederationExpressionParser>,
    ) -> PostGreSqlQueryStringBuilder {
        PostGreSqlQueryStringBuilder {
            base: JdbcSplitQueryBuilderBase::new_with_federation_expression_parser(
                quote_characters,
                federation_expression_parser,
            ),
        }
    }
}

#[async_trait]
impl JdbcSplitQueryBuilder for PostGreSqlQueryStringBuilder {
    fn query_builder_base(&self) -> &JdbcSplitQueryBuilderBase {
        &self.base
    }

    async fn build_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        catalog: &str,
        schema: &str,
        table: &str,
        table_schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> Result<PreparedStatement> {
        let char_columns =
            PostGreSqlMetadataHandler::get_char_columns(jdbc_connection, schema, table).await?;
        let column_names = table_schema
            .fields()
            .iter()
            .map(|field| field.name())
            .filter(|name| !split.properties().contains_key(name.as_str()))
            .map(|name| {
                if char_columns.contains(name) {
                    format!("RTRIM({}) AS {}", self.quote(name), self.quote(name))
                } else {
                    self.quote(name)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let statement = self
            .prepare_statement_with_sql(
                jdbc_connection,
                catalog,
                schema,
                table,
                table_schema,
                constraints,
                split,
                &column_names,
            )
            .await?;
        check_parameters(statement.parameters())?;
        Ok(statement)
    }

    fn get_from_clause_with_split(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
        split: &Split,
    ) -> String {
        let mut table_name = String::new();
        if !catalog.is_empty() {
            table_name.push_str(&self.quote(catalog));
            table_name.push('.');
        }
        if !schema.is_empty() {
            table_name.push_str(&self.quote(schema));
            table_name.push('.');
        }
        table_name.push_str(&self.quote(table));
        let split_table = self.get_split_table(split);
        if split_table.is_empty() {
            return format!(" FROM {table_name} ");
        }
        format!(" FROM {split_table} ")
    }

    fn get_split_table(&self, split: &Split) -> String {
        let partition_schema_name =
            split.get_property(PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME);
        let partition_name =
            split.get_property(PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME);
        let all = Some(PostGreSqlMetadataHandler::ALL_PARTITIONS);
        if partition_schema_name == all || partition_name == all {
            return String::new();
        }
        format!(
            "{}.{}",
            self.quote(partition_schema_name.unwrap_or_default()),
            self.quote(partition_name.unwrap_or_default())
        )
    }

    fn get_partition_where_clauses(&self, split: &Split) -> Vec<String> {
        let all = Some(PostGreSqlMetadataHandler::ALL_PARTITIONS);
        let partition_name =
            split.get_property(PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME);
        if split.get_property(PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME) == all
            && partition_name != all
        {
            return partition_name
                .map(ToString::to_string)
                .into_iter()
                .collect();
        }
        Vec::new()
    }

    fn get_sql_dialect(&self) -> SqlDialect {
        PostgresqlSqlDialect::default_dialect()
    }

    fn get_sql_dialect_with_catalog_casing_filter(
        &self,
        catalog_casing_filter: bool,
    ) -> SqlDialect {
        PostgresqlSqlDialect::new(catalog_casing_filter).sql_dialect()
    }
}
