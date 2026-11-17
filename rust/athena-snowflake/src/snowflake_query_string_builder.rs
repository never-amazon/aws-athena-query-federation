use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use athena_federation_sdk::data::{MinorType, Value};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::{FederationError, Result};
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{
    FederationExpressionParser, JdbcSplitQueryBuilder, JdbcSplitQueryBuilderBase, TypeAndValue,
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
        let mut sql = sql.to_string();
        for type_and_value in accumulator {
            let literal = sql_literal(type_and_value)?;
            sql = java_replace_first_placeholder(&sql, &literal)?;
        }
        Ok(Some(sql))
    }

    pub fn single_quote(&self, name: &str) -> String {
        let quote = SnowflakeConstants::SINGLE_QUOTE_CHAR;
        format!("{quote}{}{quote}", name.replace(quote, "''"))
    }

    pub fn append_limit_offset_with_optional_split(&self, split: Option<&Split>) -> String {
        let Some(partition_val) =
            split.and_then(|s| s.get_property(SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME))
        else {
            return String::new();
        };
        let primary_marker = SnowflakeConstants::PARTITION_PRIMARY_MARKER;
        let limit_marker_text = SnowflakeConstants::PARTITION_LIMIT_MARKER;
        let offset_marker_text = SnowflakeConstants::PARTITION_OFFSET_MARKER;
        let (Some(primary_index), Some(limit_marker), Some(offset_marker)) = (
            partition_val.find(primary_marker),
            partition_val.rfind(limit_marker_text),
            partition_val.rfind(offset_marker_text),
        ) else {
            return String::new();
        };
        let primary_start = primary_index + primary_marker.len();
        if primary_start > limit_marker || limit_marker > offset_marker {
            tracing::warn!("Malformed partition value: {partition_val}");
            return String::new();
        }
        let primary_key = &partition_val[primary_start..limit_marker];
        let limit = &partition_val[limit_marker + limit_marker_text.len()..offset_marker];
        let offset = &partition_val[offset_marker + offset_marker_text.len()..];
        if primary_key.is_empty() {
            return String::new();
        }
        if !is_ascii_digits(limit) || !is_ascii_digits(offset) {
            tracing::warn!(
                "Non-numeric limit/offset in partition value: limit={limit}, offset={offset}"
            );
            return String::new();
        }
        format!(
            "ORDER BY {primary_key} {}",
            self.append_limit_offset_with_value(limit, Some(offset))
        )
    }
}

fn is_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn number_long(value: &Value) -> Result<i64> {
    value.number_long_value().ok_or_else(|| {
        FederationError::ClassCast(format!(
            "class {} cannot be cast to class java.lang.Number",
            value.java_class_name()
        ))
    })
}

fn sql_literal(type_and_value: &TypeAndValue) -> Result<String> {
    let value = type_and_value.value();
    if value.is_null() {
        return Ok("NULL".to_string());
    }
    let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
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

fn java_replace_first_placeholder(sql: &str, replacement: &str) -> Result<String> {
    let Some(position) = sql.find('?') else {
        return Ok(sql.to_string());
    };
    let mut expanded = String::with_capacity(replacement.len());
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let escaped = chars.next().ok_or_else(|| {
                    FederationError::illegal_argument("character to be escaped is missing")
                })?;
                expanded.push(escaped);
            }
            '$' => match chars.next() {
                None => {
                    return Err(FederationError::illegal_argument(
                        "Illegal group reference: group index is missing",
                    ))
                }
                Some('{') => {
                    let name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                    return Err(FederationError::illegal_argument(format!(
                        "No group with name {{{name}}}"
                    )));
                }
                Some('0') => {
                    while chars.next_if_eq(&'0').is_some() {}
                    expanded.push('?');
                }
                Some(digit) if digit.is_ascii_digit() => {
                    return Err(FederationError::IndexOutOfBounds(format!(
                        "No group {digit}"
                    )));
                }
                Some(_) => {
                    return Err(FederationError::illegal_argument("Illegal group reference"));
                }
            },
            other => expanded.push(other),
        }
    }
    Ok(format!(
        "{}{expanded}{}",
        &sql[..position],
        &sql[position + 1..]
    ))
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
        format!(" FROM {table_name} ")
    }

    fn get_partition_where_clauses(&self, _split: &Split) -> Vec<String> {
        Vec::new()
    }

    fn quote(&self, name: &str) -> String {
        let quote = SnowflakeConstants::DOUBLE_QUOTE_CHAR;
        format!("{quote}{}{quote}", name.replace(quote, "\"\""))
    }

    fn append_limit_offset(&self, split: &Split) -> String {
        self.append_limit_offset_with_optional_split(Some(split))
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
