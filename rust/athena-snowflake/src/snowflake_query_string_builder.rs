use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use athena_federation_sdk::data::{MinorType, Value};
use athena_federation_sdk::domain::predicate::{Constraints, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::{FederationError, Result};
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{
    default_build_sql_string_literal, FederationExpressionParser, JdbcSplitQueryBuilder,
    JdbcSplitQueryBuilderBase, TypeAndValue,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Timelike};

use crate::snowflake_constants::SnowflakeConstants;
use crate::snowflake_dialect::SnowflakeDialect;

pub struct SnowflakeQueryStringBuilder {
    base: JdbcSplitQueryBuilderBase,
}

impl SnowflakeQueryStringBuilder {
    pub fn new(
        quote_characters: impl Into<String>,
        federation_expression_parser: Arc<dyn FederationExpressionParser>,
    ) -> SnowflakeQueryStringBuilder {
        SnowflakeQueryStringBuilder {
            base: JdbcSplitQueryBuilderBase::new_with_federation_expression_parser(
                quote_characters,
                federation_expression_parser,
            ),
        }
    }

    pub fn get_base_export_sql_string(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
        table_schema: &Schema,
        constraints: &Constraints,
    ) -> Result<Option<String>> {
        let mut column_names = table_schema
            .fields()
            .iter()
            .map(|field| field.name())
            .filter(|name| {
                !name.eq_ignore_ascii_case(SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME)
            })
            .map(|name| self.quote(name))
            .collect::<Vec<_>>()
            .join(", ");
        if column_names.is_empty() {
            column_names = "*".to_string();
        }
        let mut accumulator = Vec::new();
        let split = Split::new(None, None, HashMap::new());
        let sql_base_string = self.build_sql_string_literal(
            catalog,
            schema,
            table,
            table_schema,
            constraints,
            &split,
            &column_names,
            &mut accumulator,
        )?;
        let expanded = self.expand_sql(&sql_base_string, &accumulator)?;
        tracing::info!(
            "Expanded Generated SQL : {}",
            expanded.as_deref().unwrap_or("null")
        );
        Ok(expanded)
    }

    pub fn expand_sql(&self, sql: &str, accumulator: &[TypeAndValue]) -> Result<Option<String>> {
        if sql.is_empty() {
            return Ok(None);
        }
        let literals = accumulator
            .iter()
            .map(sql_literal)
            .collect::<Result<Vec<_>>>()?;
        Ok(Some(replace_placeholders(sql, &literals)))
    }

    pub fn single_quote(&self, name: &str) -> String {
        single_quote(name)
    }

    pub fn append_order_by_limit_offset(
        &self,
        split: Option<&Split>,
        constraints: &Constraints,
    ) -> String {
        let order_by_clause = self.extract_order_by_clause(constraints);
        let Some(page) = split.and_then(split_page) else {
            let order_by = if order_by_clause.is_empty() {
                String::new()
            } else {
                format!(" {order_by_clause}")
            };
            if constraints.limit() > 0 {
                return format!("{order_by} LIMIT {}", constraints.limit());
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
            self.append_limit_offset_with_value(
                &page.limit_within(constraints.limit()),
                Some(page.offset)
            )
        )
    }
}

struct SplitPage<'a> {
    primary_key: &'a str,
    limit: &'a str,
    offset: &'a str,
}

impl SplitPage<'_> {
    fn limit_within(&self, constraint_limit: i64) -> String {
        if constraint_limit <= 0 {
            return self.limit.to_string();
        }
        self.limit
            .parse::<i64>()
            .map_or(constraint_limit, |limit| limit.min(constraint_limit))
            .to_string()
    }
}

fn split_page(split: &Split) -> Option<SplitPage<'_>> {
    let partition_val = split.get_property(SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME)?;
    let primary_marker = SnowflakeConstants::PARTITION_PRIMARY_MARKER;
    let limit_marker_text = SnowflakeConstants::PARTITION_LIMIT_MARKER;
    let offset_marker_text = SnowflakeConstants::PARTITION_OFFSET_MARKER;
    let (Some(primary_index), Some(limit_marker), Some(offset_marker)) = (
        partition_val.find(primary_marker),
        partition_val.rfind(limit_marker_text),
        partition_val.rfind(offset_marker_text),
    ) else {
        return None;
    };
    let primary_start = primary_index + primary_marker.len();
    if primary_start > limit_marker || limit_marker > offset_marker {
        tracing::warn!("Malformed partition value: {partition_val}");
        return None;
    }
    let primary_key = &partition_val[primary_start..limit_marker];
    let limit = &partition_val[limit_marker + limit_marker_text.len()..offset_marker];
    let offset = &partition_val[offset_marker + offset_marker_text.len()..];
    if primary_key.is_empty() {
        return None;
    }
    if !is_ascii_digits(limit) || !is_ascii_digits(offset) {
        tracing::warn!(
            "Non-numeric limit/offset in partition value: limit={limit}, offset={offset}"
        );
        return None;
    }
    Some(SplitPage {
        primary_key,
        limit,
        offset,
    })
}

fn is_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
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
    let literal = match MinorType::get_minor_type_for_arrow_type(type_and_value.get_type()) {
        MinorType::DATEDAY => {
            let days = number_long(value)?;
            let date = NaiveDate::from_ymd_opt(1970, 1, 1)
                .and_then(|epoch| epoch.checked_add_signed(chrono::Duration::days(days)))
                .ok_or_else(|| {
                    FederationError::Runtime(format!("Invalid value for EpochDay: {days}"))
                })?;
            format!("DATE {}", quote(&date.format("%Y-%m-%d").to_string()))
        }
        MinorType::DATEMILLI => format!("TIMESTAMP {}", quote(&instant_text(number_long(value)?)?)),
        MinorType::TIMESTAMPMILLITZ | MinorType::TIMESTAMPMICROTZ => match value {
            Value::LocalDateTime(timestamp) => {
                format!("TIMESTAMP {}", quote(&sql_timestamp_text(timestamp)))
            }
            other => format!("TIMESTAMP {}", quote(&instant_text(number_long(other)?)?)),
        },
        MinorType::INT
        | MinorType::SMALLINT
        | MinorType::TINYINT
        | MinorType::BIGINT
        | MinorType::FLOAT4
        | MinorType::FLOAT8
        | MinorType::DECIMAL => value.java_to_string(),
        _ => quote(&value.java_to_string()),
    };
    Ok(literal)
}

fn instant_text(millis: i64) -> Result<String> {
    DateTime::from_timestamp_millis(millis)
        .map(|instant| instant.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
        .ok_or_else(|| {
            FederationError::Runtime(format!(
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

fn single_quote(text: &str) -> String {
    let quote = SnowflakeConstants::SINGLE_QUOTE_CHAR;
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

impl JdbcSplitQueryBuilder for SnowflakeQueryStringBuilder {
    fn query_builder_base(&self) -> &JdbcSplitQueryBuilderBase {
        &self.base
    }

    fn get_from_clause_with_split(
        &self,
        _catalog: &str,
        schema: &str,
        table: &str,
        _split: &Split,
    ) -> String {
        let mut table_name = String::new();
        if !schema.is_empty() {
            table_name.push_str(&self.quote(schema));
            table_name.push('.');
        }
        table_name.push_str(&self.quote(table));
        format!(" FROM {table_name}")
    }

    fn get_partition_where_clauses(&self, _split: &Split) -> Vec<String> {
        Vec::new()
    }

    fn quote(&self, name: &str) -> String {
        let quote = SnowflakeConstants::DOUBLE_QUOTE_CHAR;
        format!("{quote}{}{quote}", name.replace(quote, "\"\""))
    }

    fn build_sql_string_literal(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
        table_schema: &Schema,
        constraints: &Constraints,
        split: &Split,
        column_names: &str,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        let filter_constraints = Constraints::new(
            constraints.summary().clone(),
            constraints.expression().to_vec(),
            Vec::new(),
            DEFAULT_NO_LIMIT,
            constraints.query_passthrough_arguments().clone(),
            constraints.query_plan().cloned(),
        );
        let mut sql = default_build_sql_string_literal(
            self,
            catalog,
            schema,
            table,
            table_schema,
            &filter_constraints,
            split,
            column_names,
            accumulator,
        )?;
        sql.push_str(&self.append_order_by_limit_offset(Some(split), constraints));
        tracing::info!("Generated Snowflake SQL : {}", sql);
        Ok(sql)
    }

    fn get_sql_dialect(&self) -> SqlDialect {
        SnowflakeDialect::default_dialect()
    }

    fn get_sql_dialect_with_catalog_casing_filter(
        &self,
        catalog_casing_filter: bool,
    ) -> SqlDialect {
        SnowflakeDialect::new(catalog_casing_filter).into_sql_dialect()
    }
}
