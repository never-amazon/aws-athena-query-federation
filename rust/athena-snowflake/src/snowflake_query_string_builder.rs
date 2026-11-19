use std::collections::HashMap;

use arrow::datatypes::Schema;
use athena_federation_sdk::data::{minor_type_of, MinorType, Value};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::{Split, TableName};
use athena_federation_sdk::{FederationError, Result};
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{SplitDialect, SplitQuery, SplitQueryBuilder, TypeAndValue};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Timelike};

use crate::partition_page::PartitionPage;
use crate::snowflake_constants;
use crate::snowflake_dialect::SnowflakeDialect;
use athena_jdbc::java_null::OrJavaNull;

pub struct SnowflakeSplitDialect;

pub type SnowflakeQueryStringBuilder = SplitQueryBuilder<SnowflakeSplitDialect>;

pub fn query_string_builder() -> SnowflakeQueryStringBuilder {
    SplitQueryBuilder::new(
        snowflake_constants::DOUBLE_QUOTE_CHAR,
        SnowflakeSplitDialect,
    )
}

pub fn base_export_sql(
    builder: &SnowflakeQueryStringBuilder,
    table: &TableName,
    table_schema: &Schema,
    constraints: &Constraints,
) -> Result<Option<String>> {
    let mut column_names = table_schema
        .fields()
        .iter()
        .map(|field| field.name())
        .filter(|name| !name.eq_ignore_ascii_case(snowflake_constants::BLOCK_PARTITION_COLUMN_NAME))
        .map(|name| builder.quote(name))
        .collect::<Vec<_>>()
        .join(", ");
    if column_names.is_empty() {
        column_names = "*".to_string();
    }
    let mut accumulator = Vec::new();
    let split = Split::new(None, None, HashMap::new());
    let query = SplitQuery {
        catalog_name: "",
        table_name: table,
        schema: table_schema,
        constraints,
        split: &split,
    };
    let sql_base_string =
        builder.build_sql_string_literal(&query, &column_names, &mut accumulator)?;
    let expanded = expand_sql(&sql_base_string, &accumulator)?;
    tracing::info!(
        "Expanded Generated SQL : {}",
        expanded.as_deref().or_java_null()
    );
    Ok(expanded)
}

pub fn expand_sql(sql: &str, accumulator: &[TypeAndValue]) -> Result<Option<String>> {
    if sql.is_empty() {
        return Ok(None);
    }
    let literals = accumulator
        .iter()
        .map(sql_literal)
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(replace_placeholders(sql, &literals)))
}

pub fn append_order_by_limit_offset(
    builder: &SnowflakeQueryStringBuilder,
    split: Option<&Split>,
    constraints: &Constraints,
) -> String {
    let order_by_clause = builder.extract_order_by_clause(constraints);
    let Some(page) = split.and_then(PartitionPage::from_split) else {
        let order_by = if order_by_clause.is_empty() {
            String::new()
        } else {
            format!(" {order_by_clause}")
        };
        if let Some(limit) = constraints.limit() {
            return format!("{order_by} LIMIT {limit}");
        }
        return order_by;
    };
    let order_by = if order_by_clause.is_empty() {
        " ORDER BY ".to_string()
    } else {
        format!(" {order_by_clause}, ")
    };
    format!(
        "{order_by}{}{}",
        page.primary_key,
        limit_offset(&page.limit_within(constraints.limit()), page.offset)
    )
}

fn limit_offset(limit: &str, offset: &str) -> String {
    format!(" LIMIT {limit} OFFSET {offset}")
}

fn number_long(value: &Value) -> Result<i64> {
    value
        .number_long_value()
        .ok_or_else(|| FederationError::class_cast(value.java_class_name(), "java.lang.Number"))
}

fn sql_literal(type_and_value: &TypeAndValue) -> Result<String> {
    let value = type_and_value.value();
    if value.is_null() {
        return Ok("NULL".to_string());
    }
    let quote = single_quote;
    let literal = match minor_type_of(type_and_value.get_type()) {
        MinorType::DateDay => {
            let days = number_long(value)?;
            let date = NaiveDate::from_ymd_opt(1970, 1, 1)
                .and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(days)))
                .ok_or_else(|| {
                    FederationError::runtime(format!("Invalid value for EpochDay: {days}"))
                })?;
            format!("DATE {}", quote(&date.format("%Y-%m-%d").to_string()))
        }
        MinorType::DateMilli => format!("TIMESTAMP {}", quote(&instant_text(number_long(value)?)?)),
        MinorType::TimestampMilliTz | MinorType::TimestampMicroTz => match value {
            Value::LocalDateTime(timestamp) => {
                format!("TIMESTAMP {}", quote(&sql_timestamp_text(timestamp)))
            }
            other => format!("TIMESTAMP {}", quote(&instant_text(number_long(other)?)?)),
        },
        MinorType::Int
        | MinorType::SmallInt
        | MinorType::TinyInt
        | MinorType::BigInt
        | MinorType::Float4
        | MinorType::Float8
        | MinorType::Decimal => value.java_to_string(),
        _ => quote(&value.java_to_string()),
    };
    Ok(literal)
}

fn instant_text(millis: i64) -> Result<String> {
    DateTime::from_timestamp_millis(millis)
        .map(|instant| instant.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
        .ok_or_else(|| {
            FederationError::runtime(format!(
                "Instant exceeds minimum or maximum instant: {millis}"
            ))
        })
}

fn sql_timestamp_text(timestamp: &NaiveDateTime) -> String {
    let nanos = timestamp.nanosecond() % 1_000_000_000;
    let mut fraction = format!("{nanos:09}");
    while fraction.len() > 1 && fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{}.{fraction}", timestamp.format("%Y-%m-%d %H:%M:%S"))
}

pub fn single_quote(text: &str) -> String {
    let quote = snowflake_constants::SINGLE_QUOTE_CHAR;
    format!(
        "{quote}{}{quote}",
        text.replace('\\', "\\\\").replace(quote, "''")
    )
}

fn replace_placeholders(sql: &str, literals: &[String]) -> String {
    let mut expanded = String::with_capacity(sql.len());
    let mut literals = literals.iter();
    let mut open_quote = None;
    let mut chars = sql.chars();
    while let Some(c) = chars.next() {
        match open_quote {
            Some('\'') if c == '\\' => {
                expanded.push(c);
                if let Some(escaped) = chars.next() {
                    expanded.push(escaped);
                }
            }
            Some(quote) => {
                if c == quote {
                    open_quote = None;
                }
                expanded.push(c);
            }
            None if c == '\'' || c == '"' => {
                open_quote = Some(c);
                expanded.push(c);
            }
            None if c == '?' => match literals.next() {
                Some(literal) => expanded.push_str(literal),
                None => expanded.push(c),
            },
            None => expanded.push(c),
        }
    }
    expanded
}

impl SplitDialect for SnowflakeSplitDialect {
    fn table_clause(
        &self,
        builder: &SnowflakeQueryStringBuilder,
        table: &TableName,
        _split: &Split,
    ) -> String {
        let mut table_name = String::new();
        if !table.schema_name().is_empty() {
            table_name.push_str(&builder.quote(table.schema_name()));
            table_name.push('.');
        }
        table_name.push_str(&builder.quote(table.table_name()));
        format!(" FROM {table_name}")
    }

    fn split_paging_clause(&self, _builder: &SnowflakeQueryStringBuilder, split: &Split) -> String {
        PartitionPage::from_split(split).map_or_else(String::new, |page| {
            format!(
                " ORDER BY {}{}",
                page.primary_key,
                limit_offset(page.limit, page.offset)
            )
        })
    }

    fn append_order_by_and_limit(
        &self,
        builder: &SnowflakeQueryStringBuilder,
        sql: &mut String,
        split: &Split,
        constraints: &Constraints,
    ) {
        tracing::info!("Generated SQL : {}", sql);
        sql.push_str(&append_order_by_limit_offset(
            builder,
            Some(split),
            constraints,
        ));
        tracing::info!("Generated Snowflake SQL : {}", sql);
    }

    fn sql_dialect(&self, uppercase_only: Option<bool>) -> SqlDialect {
        SnowflakeDialect::new(uppercase_only.unwrap_or(false)).into_sql_dialect()
    }
}
