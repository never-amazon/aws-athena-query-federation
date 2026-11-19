use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::Result;
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{SplitDialect, SplitQuery, SplitQueryBuilder};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement};

use crate::constants;
use crate::driver::validate_parameters;
use crate::metadata_handler::{
    get_char_columns, ALL_PARTITIONS, BLOCK_PARTITION_COLUMN_NAME,
    BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
};
use crate::sql_dialect::PostgresqlSqlDialect;

pub struct PostGreSqlSplitDialect;

pub type PostGreSqlQueryStringBuilder = SplitQueryBuilder<PostGreSqlSplitDialect>;

pub fn query_string_builder() -> PostGreSqlQueryStringBuilder {
    SplitQueryBuilder::new(constants::POSTGRES_QUOTE_CHARACTER, PostGreSqlSplitDialect)
}

pub async fn build_sql(
    builder: &PostGreSqlQueryStringBuilder,
    jdbc_connection: &dyn JdbcConnection,
    query: &SplitQuery<'_>,
) -> Result<PreparedStatement> {
    let char_columns = get_char_columns(
        jdbc_connection,
        query.table_name.schema_name(),
        query.table_name.table_name(),
    )
    .await?;
    let column_names = query
        .schema
        .fields()
        .iter()
        .map(|field| field.name())
        .filter(|name| !query.split.properties().contains_key(name.as_str()))
        .map(|name| {
            if char_columns.contains(name) {
                format!("RTRIM({}) AS {}", builder.quote(name), builder.quote(name))
            } else {
                builder.quote(name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let statement = builder
        .prepare_statement_with_sql(jdbc_connection, query, &column_names)
        .await?;
    validate_parameters(statement.parameters())?;
    Ok(statement)
}

impl SplitDialect for PostGreSqlSplitDialect {
    fn table_clause(
        &self,
        builder: &PostGreSqlQueryStringBuilder,
        table: &TableName,
        split: &Split,
    ) -> String {
        let split_table = self.split_table(builder, split);
        if !split_table.is_empty() {
            return format!(" FROM {split_table} ");
        }
        let mut table_name = String::new();
        if !table.schema_name().is_empty() {
            table_name.push_str(&builder.quote(table.schema_name()));
            table_name.push('.');
        }
        table_name.push_str(&builder.quote(table.table_name()));
        format!(" FROM {table_name} ")
    }

    fn split_table(&self, builder: &PostGreSqlQueryStringBuilder, split: &Split) -> String {
        let partition_schema_name = split.get_property(BLOCK_PARTITION_SCHEMA_COLUMN_NAME);
        let partition_name = split.get_property(BLOCK_PARTITION_COLUMN_NAME);
        let all = Some(ALL_PARTITIONS);
        if partition_schema_name == all || partition_name == all {
            return String::new();
        }
        format!(
            "{}.{}",
            builder.quote(partition_schema_name.unwrap_or_default()),
            builder.quote(partition_name.unwrap_or_default())
        )
    }

    fn partition_where_clauses(&self, split: &Split) -> Vec<String> {
        let all = Some(ALL_PARTITIONS);
        let partition_name = split.get_property(BLOCK_PARTITION_COLUMN_NAME);
        if split.get_property(BLOCK_PARTITION_SCHEMA_COLUMN_NAME) == all && partition_name != all {
            return partition_name
                .map(ToString::to_string)
                .into_iter()
                .collect();
        }
        Vec::new()
    }

    fn sql_dialect(&self, uppercase_only: Option<bool>) -> SqlDialect {
        match uppercase_only {
            None => PostgresqlSqlDialect::default_dialect(),
            Some(filter) => PostgresqlSqlDialect::new(filter).sql_dialect(),
        }
    }
}
