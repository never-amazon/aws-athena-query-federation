use arrow::datatypes::{FieldRef, Schema};
use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};

use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::data::{arrow_type_to_string, ArrowType, MinorType, Value};
use athena_federation_sdk::domain::predicate::{Bound, Constraints, ValueSet};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::{FederationError, Result};
use athena_federation_substrait::{
    parameterize, plan, SqlDialect, SqlIdentifier, SqlKind, SqlNode, SqlTypeName,
    SubstraitTypeAndValue, SubstraitValue,
};

use super::expression_parser::ExpressionParser;
use super::type_and_value::TypeAndValue;
use crate::sql::{JdbcConnection, PreparedStatement};
use crate::validate::runtime_with_cause;

const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1000;

pub struct SplitQuery<'a> {
    pub catalog_name: &'a str,
    pub table_name: &'a TableName,
    pub schema: &'a Schema,
    pub constraints: &'a Constraints,
    pub split: &'a Split,
}

impl<'a> SplitQuery<'a> {
    pub fn of(request: &'a ReadRecordsRequest) -> SplitQuery<'a> {
        SplitQuery {
            catalog_name: request.catalog_name(),
            table_name: request.table_name(),
            schema: request.schema(),
            constraints: request.constraints(),
            split: request.split(),
        }
    }
}

pub trait SplitDialect: Send + Sync + Sized {
    fn table_clause(
        &self,
        builder: &SplitQueryBuilder<Self>,
        table: &TableName,
        split: &Split,
    ) -> String;

    fn partition_where_clauses(&self, _split: &Split) -> Vec<String> {
        Vec::new()
    }

    fn split_table(&self, _builder: &SplitQueryBuilder<Self>, _split: &Split) -> String {
        String::new()
    }

    fn split_paging_clause(&self, _builder: &SplitQueryBuilder<Self>, _split: &Split) -> String {
        String::new()
    }

    fn sql_dialect(&self, _uppercase_only: Option<bool>) -> SqlDialect {
        SqlDialect::ansi()
    }

    fn complex_expressions(
        &self,
        builder: &SplitQueryBuilder<Self>,
        constraints: &Constraints,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<Vec<String>> {
        builder
            .expression_parser()
            .parse_complex_expressions(constraints, accumulator)
    }

    fn append_order_by_and_limit(
        &self,
        builder: &SplitQueryBuilder<Self>,
        sql: &mut String,
        _split: &Split,
        constraints: &Constraints,
    ) {
        let order_by_clause = builder.extract_order_by_clause(constraints);
        if !order_by_clause.is_empty() {
            sql.push(' ');
            sql.push_str(&order_by_clause);
        }
        if constraints.limit() > 0 {
            sql.push_str(" LIMIT ");
            sql.push_str(&constraints.limit().to_string());
        }
        tracing::info!("Generated SQL : {}", sql);
    }
}

pub struct SplitQueryBuilder<D> {
    quote_char: String,
    expression_parser: ExpressionParser,
    dialect: D,
}

impl<D: SplitDialect> SplitQueryBuilder<D> {
    pub fn new(quote_char: impl Into<String>, dialect: D) -> SplitQueryBuilder<D> {
        let quote_char = quote_char.into();
        SplitQueryBuilder {
            expression_parser: ExpressionParser::new(quote_char.as_str()),
            quote_char,
            dialect,
        }
    }

    pub fn dialect(&self) -> &D {
        &self.dialect
    }

    pub fn expression_parser(&self) -> &ExpressionParser {
        &self.expression_parser
    }

    pub fn quote(&self, name: &str) -> String {
        let quote = &self.quote_char;
        let escaped = name.replace(quote.as_str(), &format!("{quote}{quote}"));
        format!("{quote}{escaped}{quote}")
    }

    pub fn column_names(&self, query: &SplitQuery<'_>) -> String {
        query
            .schema
            .fields()
            .iter()
            .map(|field| field.name())
            .filter(|name| !query.split.properties().contains_key(name.as_str()))
            .map(|name| self.quote(name))
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub async fn build_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        self.prepare_statement_with_sql(jdbc_connection, query, &self.column_names(query))
            .await
    }

    pub async fn prepare_statement_with_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
        column_names: &str,
    ) -> Result<PreparedStatement> {
        if query.constraints.query_plan().is_some() {
            let sql_dialect = self.sql_dialect_for_split(query.split);
            return self
                .prepare_statement_with_calcite_sql(
                    jdbc_connection,
                    query.constraints,
                    &sql_dialect,
                    query.split,
                )
                .await;
        }
        let mut accumulator = Vec::new();
        let sql = self.build_sql_string_literal(query, column_names, &mut accumulator)?;
        let mut statement = jdbc_connection.prepare_statement(&sql).await?;
        for (i, type_and_value) in accumulator.iter().enumerate() {
            bind_type_and_value(&mut statement, i + 1, type_and_value)?;
        }
        Ok(statement)
    }

    fn sql_dialect_for_split(&self, split: &Split) -> SqlDialect {
        let uppercase_only = split
            .get_property(environment_constants::CATALOG_CASING_FILTER)
            .map(|filter| {
                tracing::debug!("Found catalogCasingFilter for getSqlDialect: {}", filter);
                filter == environment_constants::UPPERCASE_ONLY
            });
        self.dialect.sql_dialect(uppercase_only)
    }

    pub fn build_sql_string_literal(
        &self,
        query: &SplitQuery<'_>,
        column_names: &str,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        let mut sql = String::from("SELECT ");
        sql.push_str(column_names);
        if column_names.is_empty() {
            sql.push_str("null");
        }
        sql.push_str(
            &self
                .dialect
                .table_clause(self, query.table_name, query.split),
        );

        let mut clauses = self.to_conjuncts(
            query.schema.fields(),
            query.constraints,
            accumulator,
            query.split,
        )?;
        clauses.extend(self.dialect.partition_where_clauses(query.split));
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }

        self.dialect
            .append_order_by_and_limit(self, &mut sql, query.split, query.constraints);
        Ok(sql)
    }

    pub fn extract_order_by_clause(&self, constraints: &Constraints) -> String {
        let order_by_clause = constraints.order_by_clause();
        if order_by_clause.is_empty() {
            return String::new();
        }
        let fields = order_by_clause
            .iter()
            .map(|order_by_field| {
                let direction = order_by_field.direction();
                let ordering = if direction.is_ascending() {
                    "ASC"
                } else {
                    "DESC"
                };
                let nulls_handling = if direction.is_nulls_first() {
                    "NULLS FIRST"
                } else {
                    "NULLS LAST"
                };
                format!(
                    "{} {} {}",
                    self.quote(order_by_field.column_name()),
                    ordering,
                    nulls_handling
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("ORDER BY {fields}")
    }

    pub async fn prepare_statement_with_calcite_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        constraints: &Constraints,
        sql_dialect: &SqlDialect,
        split: &Split,
    ) -> Result<PreparedStatement> {
        let split_table = self.dialect.split_table(self, split);
        let split_restriction = self.split_restriction(split);
        let result = async {
            let base64_encoded_plan = constraints
                .query_plan()
                .map(|plan| plan.substrait_plan().to_string())
                .ok_or_else(|| {
                    FederationError::null_pointer(
                        "Cannot invoke \"com.amazonaws.athena.connector.lambda.domain.predicate.QueryPlan.getSubstraitPlan()\" because the return value of \"com.amazonaws.athena.connector.lambda.domain.predicate.Constraints.getQueryPlan()\" is null",
                    )
                })?;
            tracing::debug!("CalciteSql substrait plan: {}", base64_encoded_plan);
            let (sql, accumulator) = build_calcite_sql(
                &base64_encoded_plan,
                sql_dialect,
                &split_table,
                &split_restriction,
            )?;
            let mut statement = jdbc_connection.prepare_statement(&sql).await?;
            match jdbc_connection.parameter_count(&statement).await? {
                Some(count) if count != accumulator.len() => {
                    tracing::warn!(
                        "Parameter count mismatch: SQL has {} parameters, accumulator has {}. Skipping parameter binding.",
                        count,
                        accumulator.len()
                    );
                }
                _ => handle_data_types_for_prepared_statement(&mut statement, &accumulator)?,
            }
            tracing::debug!("CalciteSql prepared statement: {}", statement.sql());
            Ok(statement)
        }
        .await;
        result.map_err(|e| {
            tracing::error!("Failed to prepare statement with Calcite {}", e);
            runtime_with_cause("Failed to prepare statement with Calcite", e)
        })
    }

    fn split_restriction(&self, split: &Split) -> String {
        let split_clauses = self.dialect.partition_where_clauses(split);
        let mut restriction = if split_clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", split_clauses.join(" AND "))
        };
        restriction.push_str(&self.dialect.split_paging_clause(self, split));
        restriction
    }

    fn to_conjuncts(
        &self,
        columns: &[FieldRef],
        constraints: &Constraints,
        accumulator: &mut Vec<TypeAndValue>,
        split: &Split,
    ) -> Result<Vec<String>> {
        let mut conjuncts = Vec::new();
        for column in columns {
            if split.properties().contains_key(column.name().as_str()) {
                continue;
            }
            if let Some(value_set) = constraints.summary().get(column.name().as_str()) {
                conjuncts.push(self.to_predicate(
                    column.name(),
                    value_set,
                    column.data_type(),
                    accumulator,
                )?);
            }
        }
        conjuncts.extend(
            self.dialect
                .complex_expressions(self, constraints, accumulator)?,
        );
        Ok(conjuncts)
    }

    fn to_predicate(
        &self,
        column_name: &str,
        value_set: &ValueSet,
        arrow_type: &ArrowType,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        let mut disjuncts = Vec::new();
        let mut single_values: Vec<Value> = Vec::new();

        if let Some(sorted_range_set) = value_set.as_sorted_range_set() {
            if value_set.is_none() && value_set.is_null_allowed() {
                return Ok(format!("({} IS NULL)", self.quote(column_name)));
            }
            if value_set.is_null_allowed() {
                disjuncts.push(format!("({} IS NULL)", self.quote(column_name)));
            }
            let ranges = sorted_range_set.ordered_ranges();
            if ranges.len() == 1
                && !value_set.is_null_allowed()
                && ranges[0].low().is_lower_unbounded()
                && ranges[0].high().is_upper_unbounded()
            {
                return Ok(format!("({} IS NOT NULL)", self.quote(column_name)));
            }
            for range in ranges {
                if range.is_single_value() {
                    single_values.push(range.low().value()?.clone());
                } else {
                    let low = if range.low().is_lower_unbounded() {
                        None
                    } else {
                        Some((range.low().bound(), range.low().value()?))
                    };
                    let high = if range.high().is_upper_unbounded() {
                        None
                    } else {
                        Some((range.high().bound(), range.high().value()?))
                    };
                    disjuncts.push(self.range_to_predicate(
                        column_name,
                        low,
                        high,
                        arrow_type,
                        accumulator,
                    )?);
                }
            }

            if single_values.len() == 1 {
                disjuncts.push(self.to_predicate_with_operator(
                    column_name,
                    "=",
                    &single_values[0],
                    arrow_type,
                    accumulator,
                )?);
            } else if single_values.len() > 1 {
                for value in &single_values {
                    accumulator.push(TypeAndValue::new(arrow_type.clone(), value.clone())?);
                }
                let values = vec!["?"; single_values.len()].join(",");
                disjuncts.push(format!("{} IN ({})", self.quote(column_name), values));
            }
        }

        Ok(format!("({})", disjuncts.join(" OR ")))
    }

    pub(crate) fn range_to_predicate(
        &self,
        column_name: &str,
        low: Option<(Bound, &Value)>,
        high: Option<(Bound, &Value)>,
        arrow_type: &ArrowType,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        let mut range_conjuncts = Vec::new();
        if let Some((bound, value)) = low {
            let operator = match bound {
                Bound::Above => ">",
                Bound::Exactly => ">=",
                Bound::Below => {
                    return Err(FederationError::invalid_input(
                        "Low marker should never use BELOW bound",
                    ))
                }
            };
            range_conjuncts.push(self.to_predicate_with_operator(
                column_name,
                operator,
                value,
                arrow_type,
                accumulator,
            )?);
        }
        if let Some((bound, value)) = high {
            let operator = match bound {
                Bound::Above => {
                    return Err(FederationError::invalid_input(
                        "High marker should never use ABOVE bound",
                    ))
                }
                Bound::Exactly => "<=",
                Bound::Below => "<",
            };
            range_conjuncts.push(self.to_predicate_with_operator(
                column_name,
                operator,
                value,
                arrow_type,
                accumulator,
            )?);
        }
        if range_conjuncts.is_empty() {
            return Err(FederationError::IllegalState(String::new()));
        }
        Ok(format!("({})", range_conjuncts.join(" AND ")))
    }

    fn to_predicate_with_operator(
        &self,
        column_name: &str,
        operator: &str,
        value: &Value,
        arrow_type: &ArrowType,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        accumulator.push(TypeAndValue::new(arrow_type.clone(), value.clone())?);
        Ok(format!("{} {} ?", self.quote(column_name), operator))
    }
}

fn build_calcite_sql(
    base64_encoded_plan: &str,
    sql_dialect: &SqlDialect,
    split_table: &str,
    split_restriction: &str,
) -> Result<(String, Vec<SubstraitTypeAndValue>)> {
    let sql_node = plan::to_sql_node(base64_encoded_plan, sql_dialect)?;
    if sql_node.as_select().is_none() {
        return Err(FederationError::Runtime(
            "Unsupported Query Type. Only SELECT Query is supported.".to_string(),
        ));
    }
    let table_schema = plan::table_schema(base64_encoded_plan, sql_dialect)?;
    let (mut parameterized_node, accumulator) = parameterize(sql_node, &table_schema)?;
    tracing::debug!(
        "CalciteSql parameterized sql with dialect {:?}: {}",
        sql_dialect,
        parameterized_node.to_sql_string(sql_dialect)
    );
    tracing::debug!("CalciteSql parameters: {:?}", accumulator);

    if (!split_table.is_empty() || !split_restriction.is_empty())
        && !restrict_scanned_table(
            &mut parameterized_node,
            split_table,
            split_restriction,
            sql_dialect,
        )
    {
        return Err(FederationError::UnsupportedOperation(format!(
            "Cannot apply the split to a query plan that does not read one table: {}",
            parameterized_node.to_sql_string(sql_dialect)
        )));
    }
    Ok((parameterized_node.to_sql_string(sql_dialect), accumulator))
}

fn restrict_scanned_table(
    node: &mut SqlNode,
    split_table: &str,
    split_restriction: &str,
    sql_dialect: &SqlDialect,
) -> bool {
    let SqlNode::Select(select) = node else {
        return false;
    };
    match &mut select.from {
        Some(SqlNode::Identifier(table)) => {
            let alias = table.names.last().cloned().unwrap_or_default();
            let restricted = restricted_table(table, split_table, split_restriction, sql_dialect);
            select.from = Some(SqlNode::as_alias(restricted, alias));
            true
        }
        Some(SqlNode::Call(call)) if call.op.kind == SqlKind::As => match call.operands.first_mut()
        {
            Some(SqlNode::Identifier(table)) => {
                let restricted =
                    restricted_table(table, split_table, split_restriction, sql_dialect);
                call.operands[0] = restricted;
                true
            }
            Some(source) => {
                restrict_scanned_table(source, split_table, split_restriction, sql_dialect)
            }
            None => false,
        },
        _ => false,
    }
}

fn restricted_table(
    table: &SqlIdentifier,
    split_table: &str,
    split_restriction: &str,
    sql_dialect: &SqlDialect,
) -> SqlNode {
    let source = if split_table.is_empty() {
        SqlNode::Identifier(table.clone()).to_sql_string(sql_dialect)
    } else {
        split_table.to_string()
    };
    SqlNode::Text(format!("(SELECT * FROM {source}{split_restriction})"))
}

fn java_class_cast(value: &Value, target: &str) -> FederationError {
    FederationError::class_cast(value.java_class_name(), target)
}

fn number_value(value: &Value) -> Result<&Value> {
    if value.is_number() {
        Ok(value)
    } else {
        Err(java_class_cast(value, "java.lang.Number"))
    }
}

fn number_int_value(value: &Value) -> Result<i32> {
    number_value(value)?
        .number_int_value()
        .ok_or_else(|| java_class_cast(value, "java.lang.Number"))
}

fn number_long_value(value: &Value) -> Result<i64> {
    number_value(value)?
        .number_long_value()
        .ok_or_else(|| java_class_cast(value, "java.lang.Number"))
}

fn bind_type_and_value(
    statement: &mut PreparedStatement,
    index: usize,
    type_and_value: &TypeAndValue,
) -> Result<()> {
    let value = type_and_value.value();
    let minor_type = MinorType::get_minor_type_for_arrow_type(type_and_value.get_type());
    match minor_type {
        MinorType::BIGINT => {
            let v = value
                .as_i64()
                .ok_or_else(|| java_class_cast(value, "java.lang.Long"))?;
            statement.set_long(index, v)?;
        }
        MinorType::INT => statement.set_int(index, number_int_value(value)?)?,
        MinorType::SMALLINT => {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "Number.shortValue narrows the same way"
            )]
            let v = number_int_value(value)? as i16;
            statement.set_short(index, v)?;
        }
        MinorType::TINYINT => {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "Number.byteValue narrows the same way"
            )]
            let v = number_int_value(value)? as i8;
            statement.set_byte(index, v)?;
        }
        MinorType::FLOAT8 => {
            let Value::Double(v) = value else {
                return Err(java_class_cast(value, "java.lang.Double"));
            };
            statement.set_double(index, *v)?;
        }
        MinorType::FLOAT4 => {
            let v = value
                .as_f32()
                .ok_or_else(|| java_class_cast(value, "java.lang.Float"))?;
            statement.set_float(index, v)?;
        }
        MinorType::BIT => {
            let v = value
                .as_bool()
                .ok_or_else(|| java_class_cast(value, "java.lang.Boolean"))?;
            statement.set_boolean(index, v)?;
        }
        MinorType::DATEDAY => {
            let days = number_long_value(value)?;
            let utc_millis = days.wrapping_mul(MILLIS_PER_DAY);
            statement.set_date(index, date_of_epoch_millis(utc_millis))?;
        }
        MinorType::DATEMILLI => {
            let Value::LocalDateTime(timestamp) = value else {
                return Err(java_class_cast(value, "java.time.LocalDateTime"));
            };
            statement.set_timestamp(index, *timestamp)?;
        }
        MinorType::VARCHAR => statement.set_string(index, value.java_to_string())?,
        MinorType::VARBINARY => {
            let bytes = value
                .as_bytes()
                .ok_or_else(|| java_class_cast(value, "[B"))?;
            statement.set_bytes(index, bytes.to_vec())?;
        }
        MinorType::DECIMAL => {
            let Value::Decimal(decimal) = value else {
                return Err(java_class_cast(value, "java.math.BigDecimal"));
            };
            statement.set_big_decimal(index, decimal.clone())?;
        }
        other => {
            return Err(FederationError::not_supported(format!(
                "Can't handle type: {}, {}",
                arrow_type_to_string(type_and_value.get_type()),
                other
            )))
        }
    }
    Ok(())
}

fn date_of_epoch_millis(millis: i64) -> NaiveDate {
    timestamp_of_epoch_millis(millis).date()
}

fn timestamp_of_epoch_millis(millis: i64) -> NaiveDateTime {
    DateTime::from_timestamp_millis(millis).map_or(
        if millis < 0 {
            NaiveDateTime::MIN
        } else {
            NaiveDateTime::MAX
        },
        |d| d.naive_utc(),
    )
}

fn substrait_class_cast(value: &SubstraitValue, target: &str) -> FederationError {
    FederationError::class_cast(value.java_class_name(), target)
}

fn substrait_is_number(value: &SubstraitValue) -> bool {
    matches!(
        value,
        SubstraitValue::Byte(_)
            | SubstraitValue::Short(_)
            | SubstraitValue::Integer(_)
            | SubstraitValue::Long(_)
            | SubstraitValue::Float(_)
            | SubstraitValue::Double(_)
            | SubstraitValue::Decimal(_)
    )
}

fn substrait_long_value(value: &SubstraitValue) -> Result<i64> {
    if !substrait_is_number(value) {
        return Err(substrait_class_cast(value, "java.lang.Number"));
    }
    value
        .as_i64()
        .ok_or_else(|| substrait_class_cast(value, "java.lang.Number"))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Number.intValue narrows the same way"
)]
fn substrait_int_value(value: &SubstraitValue) -> Result<i32> {
    match value {
        SubstraitValue::Float(v) => Ok(*v as i32),
        SubstraitValue::Double(v) => Ok(*v as i32),
        other => substrait_long_value(other).map(|v| v as i32),
    }
}

fn substrait_double_value(value: &SubstraitValue) -> Result<f64> {
    if !substrait_is_number(value) {
        return Err(substrait_class_cast(value, "java.lang.Number"));
    }
    value
        .as_f64()
        .ok_or_else(|| substrait_class_cast(value, "java.lang.Number"))
}

pub(crate) fn java_sql_date_value_of(text: &str) -> Result<NaiveDate> {
    let invalid = || FederationError::IllegalArgument(String::new());
    let mut parts = text.splitn(3, '-');
    let (Some(year), Some(month), Some(day)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid());
    };
    let digits = |s: &str, min: usize, max: usize| {
        s.len() >= min && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit())
    };
    if !digits(year, 4, 4) || !digits(month, 1, 2) || !digits(day, 1, 2) {
        return Err(invalid());
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        year.parse::<i32>(),
        month.parse::<u32>(),
        day.parse::<u32>(),
    ) else {
        return Err(invalid());
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(invalid());
    }
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|first| first.checked_add_days(chrono::Days::new(u64::from(day - 1))))
        .ok_or_else(invalid)
}

pub(crate) fn java_sql_timestamp_value_of(text: &str) -> Result<NaiveDateTime> {
    let invalid = || {
        FederationError::IllegalArgument(
            "Timestamp format must be yyyy-mm-dd hh:mm:ss[.fffffffff]".to_string(),
        )
    };
    let trimmed = text.trim();
    let (date_part, time_part) = trimmed.split_once(' ').ok_or_else(invalid)?;
    let date = java_sql_date_value_of(date_part).map_err(|_| invalid())?;
    let (hms, nanos_text) = match time_part.split_once('.') {
        Some((hms, fraction)) => (hms, Some(fraction)),
        None => (time_part, None),
    };
    let mut fields = hms.split(':');
    let (Some(h), Some(m), Some(s), None) =
        (fields.next(), fields.next(), fields.next(), fields.next())
    else {
        return Err(invalid());
    };
    let two_digits = |v: &str| v.len() == 2 && v.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(h) || !two_digits(m) || !two_digits(s) {
        return Err(invalid());
    }
    let (Ok(h), Ok(m), Ok(s)) = (h.parse::<u32>(), m.parse::<u32>(), s.parse::<u32>()) else {
        return Err(invalid());
    };
    let nanos = match nanos_text {
        None => 0,
        Some(fraction) => {
            if fraction.is_empty()
                || fraction.len() > 9
                || !fraction.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(invalid());
            }
            let padded = format!("{fraction:0<9}");
            padded.parse::<u32>().map_err(|_| invalid())?
        }
    };
    if h > 23 || m > 59 || s > 59 {
        return Err(invalid());
    }
    let time = NaiveTime::from_hms_nano_opt(h, m, s, nanos).ok_or_else(invalid)?;
    Ok(date.and_time(time))
}

pub(crate) fn handle_data_types_for_prepared_statement(
    statement: &mut PreparedStatement,
    accumulator: &[SubstraitTypeAndValue],
) -> Result<()> {
    for (i, type_and_value) in accumulator.iter().enumerate() {
        let index = i + 1;
        let value = type_and_value.value();
        match type_and_value.ty() {
            SqlTypeName::Bigint => statement.set_long(index, substrait_long_value(value)?)?,
            SqlTypeName::Double => statement.set_double(index, substrait_double_value(value)?)?,
            SqlTypeName::Integer => statement.set_int(index, substrait_int_value(value)?)?,
            SqlTypeName::Smallint => {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "Number.shortValue narrows the same way"
                )]
                let v = substrait_int_value(value)? as i16;
                statement.set_short(index, v)?;
            }
            SqlTypeName::Tinyint => {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "Number.byteValue narrows the same way"
                )]
                let v = substrait_int_value(value)? as i8;
                statement.set_byte(index, v)?;
            }
            SqlTypeName::Char | SqlTypeName::Varchar => {
                statement.set_string(index, value.to_string())?;
            }
            SqlTypeName::Varbinary => {
                let SubstraitValue::Bits(bits) = value else {
                    return Err(substrait_class_cast(
                        value,
                        "org.apache.calcite.util.BitString",
                    ));
                };
                statement.set_bytes(index, bits.as_byte_array().to_vec())?;
            }
            SqlTypeName::Float => {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "Number.floatValue narrows the same way"
                )]
                let v = match value {
                    SubstraitValue::Float(f) => *f,
                    other => substrait_double_value(other)? as f32,
                };
                statement.set_float(index, v)?;
            }
            SqlTypeName::Decimal => {
                let SubstraitValue::Decimal(decimal) = value else {
                    return Err(substrait_class_cast(value, "java.math.BigDecimal"));
                };
                statement.set_big_decimal(index, BigDecimal::clone(decimal))?;
            }
            SqlTypeName::Boolean => {
                let SubstraitValue::Boolean(b) = value else {
                    return Err(substrait_class_cast(value, "java.lang.Boolean"));
                };
                statement.set_boolean(index, *b)?;
            }
            SqlTypeName::Date => bind_substrait_date(statement, index, type_and_value)?,
            SqlTypeName::Timestamp => match value {
                SubstraitValue::Timestamp(timestamp) => {
                    statement.set_timestamp(
                        index,
                        java_sql_timestamp_value_of(&timestamp.to_string())?,
                    )?;
                }
                other if substrait_is_number(other) => {
                    let millis = substrait_long_value(other)?;
                    statement.set_timestamp(index, timestamp_of_epoch_millis(millis))?;
                }
                other => {
                    return Err(FederationError::not_supported(format!(
                        "Can't handle timestamp format: {}, value class: {}",
                        type_and_value.ty(),
                        other.java_class_name()
                    )))
                }
            },
            other => {
                return Err(FederationError::not_supported(format!(
                    "Can't handle type: {other}, {other}"
                )))
            }
        }
    }
    Ok(())
}

fn bind_substrait_date(
    statement: &mut PreparedStatement,
    index: usize,
    type_and_value: &SubstraitTypeAndValue,
) -> Result<()> {
    let value = type_and_value.value();
    match value {
        number if substrait_is_number(number) => {
            let days = substrait_long_value(number)?;
            let utc_millis = days.wrapping_mul(MILLIS_PER_DAY);
            statement.set_date(index, date_of_epoch_millis(utc_millis))?;
        }
        SubstraitValue::Date(date) => {
            statement.set_date(index, java_sql_date_value_of(&date.to_string())?)?;
        }
        SubstraitValue::Timestamp(timestamp) => {
            statement.set_timestamp(index, java_sql_timestamp_value_of(&timestamp.to_string())?)?;
        }
        other => match java_sql_date_value_of(&other.to_string()) {
            Ok(date) => statement.set_date(index, date)?,
            Err(_) => {
                return Err(FederationError::not_supported(format!(
                    "Can't handle date format: {}, value type: {}, value: {}",
                    type_and_value.ty(),
                    other.java_class_name(),
                    other
                )))
            }
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveDateTime};

    use arrow::datatypes::DataType;
    use athena_federation_sdk::data::java_format::big_decimal_to_string;
    use athena_federation_sdk::data::Value;
    use athena_federation_sdk::domain::predicate::Bound;
    use athena_federation_sdk::domain::split::Split;
    use athena_federation_sdk::FederationError;
    use athena_federation_substrait::{
        DateString, SqlTypeName, SubstraitTypeAndValue, SubstraitValue, TimestampString,
    };

    use super::{handle_data_types_for_prepared_statement, SplitDialect, SplitQueryBuilder};
    use crate::sql::{PreparedStatement, ResultSet, ResultSetMetaData, SqlValue};
    use athena_federation_sdk::domain::table_name::TableName;

    const JAVA_PROBE: &str =
        include_str!("../../tests/fixtures/split_query_builder_integration.tsv");

    struct TestDialect;

    impl SplitDialect for TestDialect {
        fn table_clause(
            &self,
            _builder: &SplitQueryBuilder<Self>,
            table: &TableName,
            _split: &Split,
        ) -> String {
            format!(
                " FROM \"{}\".\"{}\"",
                table.schema_name(),
                table.table_name()
            )
        }

        fn partition_where_clauses(&self, _split: &Split) -> Vec<String> {
            vec!["\"partition_col\" = '2024'".to_string()]
        }
    }

    fn tv(type_name: SqlTypeName, value: SubstraitValue, column: &str) -> SubstraitTypeAndValue {
        SubstraitTypeAndValue::new(type_name, value, column).unwrap()
    }

    fn bind(accumulator: &[SubstraitTypeAndValue]) -> Result<PreparedStatement, FederationError> {
        let mut statement = PreparedStatement::new("SELECT 1");
        handle_data_types_for_prepared_statement(&mut statement, accumulator)?;
        Ok(statement)
    }

    fn timestamp(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn message(error: &FederationError) -> String {
        error.to_string()
    }

    #[test]
    fn test_invalid_bound_below_for_low_marker() {
        let builder = SplitQueryBuilder::new("\"", TestDialect);
        let low = Value::Int(100);
        let high = Value::Int(200);
        let mut accumulator = Vec::new();
        let error = builder
            .range_to_predicate(
                "col1",
                Some((Bound::Below, &low)),
                Some((Bound::Exactly, &high)),
                &DataType::Int32,
                &mut accumulator,
            )
            .unwrap_err();
        assert!(error.as_connector_exception().is_some());
        assert!(message(&error).contains("Low marker should never use BELOW bound"));
    }

    #[test]
    fn test_invalid_bound_above_for_high_marker() {
        let builder = SplitQueryBuilder::new("\"", TestDialect);
        let low = Value::Int(100);
        let high = Value::Int(200);
        let mut accumulator = Vec::new();
        let error = builder
            .range_to_predicate(
                "col1",
                Some((Bound::Exactly, &low)),
                Some((Bound::Above, &high)),
                &DataType::Int32,
                &mut accumulator,
            )
            .unwrap_err();
        assert!(error.as_connector_exception().is_some());
        assert!(message(&error).contains("High marker should never use ABOVE bound"));
    }

    #[test]
    fn test_handle_timestamp_with_timestamp_string() {
        let statement = bind(&[tv(
            SqlTypeName::Timestamp,
            SubstraitValue::Timestamp(TimestampString::new("2025-01-10 10:10:10")),
            "EVENT_TIMESTAMP",
        )])
        .unwrap();
        assert!(matches!(
            statement.parameter(1),
            Some(SqlValue::Timestamp(_))
        ));
        assert_eq!(statement.parameters().len(), 1);
    }

    #[test]
    fn test_handle_timestamp_with_numeric_value() {
        let expected_millis = 1_704_880_210_000_i64;
        let statement = bind(&[tv(
            SqlTypeName::Timestamp,
            SubstraitValue::Long(expected_millis),
            "EVENT_TIMESTAMP",
        )])
        .unwrap();
        let expected = chrono::DateTime::from_timestamp_millis(expected_millis)
            .unwrap()
            .naive_utc();
        assert_eq!(statement.parameter(1), Some(&SqlValue::Timestamp(expected)));
    }

    #[test]
    fn test_handle_timestamp_with_unsupported_format() {
        let error = bind(&[tv(
            SqlTypeName::Timestamp,
            SubstraitValue::String("2025-01-10".to_string()),
            "EVENT_TIMESTAMP",
        )])
        .unwrap_err();
        assert!(error.as_connector_exception().is_some());
        assert!(message(&error).contains("Can't handle timestamp format"));
        assert!(message(&error).contains("String"));
    }

    #[test]
    fn test_handle_multiple_timestamps() {
        let ts2_millis = 1_704_880_210_000_i64;
        let statement = bind(&[
            tv(
                SqlTypeName::Timestamp,
                SubstraitValue::Timestamp(TimestampString::new("2025-01-10 10:10:10")),
                "EVENT_TIMESTAMP1",
            ),
            tv(
                SqlTypeName::Timestamp,
                SubstraitValue::Long(ts2_millis),
                "EVENT_TIMESTAMP2",
            ),
        ])
        .unwrap();
        assert!(matches!(
            statement.parameter(1),
            Some(SqlValue::Timestamp(_))
        ));
        let expected = chrono::DateTime::from_timestamp_millis(ts2_millis)
            .unwrap()
            .naive_utc();
        assert_eq!(statement.parameter(2), Some(&SqlValue::Timestamp(expected)));
    }

    #[test]
    fn test_handle_timestamp_with_mixed_types() {
        let statement = bind(&[
            tv(
                SqlTypeName::Varchar,
                SubstraitValue::String("test_value".to_string()),
                "COL_VARCHAR",
            ),
            tv(
                SqlTypeName::Timestamp,
                SubstraitValue::Timestamp(TimestampString::new("2025-01-10 10:10:10")),
                "EVENT_TIMESTAMP",
            ),
            tv(
                SqlTypeName::Integer,
                SubstraitValue::Integer(12345),
                "COL_INT",
            ),
        ])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::String("test_value".to_string()))
        );
        assert_eq!(
            statement.parameter(2),
            Some(&SqlValue::Timestamp(timestamp("2025-01-10 10:10:10")))
        );
        assert_eq!(statement.parameter(3), Some(&SqlValue::Integer(12345)));
    }

    #[test]
    fn test_handle_date_with_date_string_value() {
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::Date(DateString::new("2025-01-10")),
            "EVENT_DATE",
        )])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::Date(date("2025-01-10")))
        );
    }

    #[test]
    fn test_handle_date_with_sql_date_value() {
        let days = date("2025-01-10")
            .signed_duration_since(NaiveDate::default())
            .num_days();
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::Date(DateString::from_days_since_epoch(
                i32::try_from(days).unwrap(),
            )),
            "EVENT_DATE",
        )])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::Date(date("2025-01-10")))
        );
    }

    #[test]
    fn test_handle_date_with_util_date_value() {
        let instant = chrono::DateTime::from_timestamp_millis(1_704_844_800_000)
            .unwrap()
            .date_naive();
        let days = instant
            .signed_duration_since(NaiveDate::default())
            .num_days();
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::Date(DateString::from_days_since_epoch(
                i32::try_from(days).unwrap(),
            )),
            "EVENT_DATE",
        )])
        .unwrap();
        assert_eq!(statement.parameter(1), Some(&SqlValue::Date(instant)));
    }

    #[test]
    fn test_handle_date_with_string_value() {
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::String("2025-01-10".to_string()),
            "EVENT_DATE",
        )])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::Date(date("2025-01-10")))
        );
    }

    #[test]
    fn test_handle_date_with_numeric_day_value() {
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::Long(19000),
            "EVENT_DATE",
        )])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::Date(
                NaiveDate::default() + chrono::Days::new(19000)
            ))
        );
    }

    #[test]
    fn test_handle_date_with_numeric_millisecond_value() {
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::Long(1_704_844_800_000),
            "EVENT_DATE",
        )])
        .unwrap();
        assert!(matches!(statement.parameter(1), Some(SqlValue::Date(_))));
    }

    #[test]
    fn test_handle_date_with_timestamp_string_value() {
        let statement = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::Timestamp(TimestampString::new("2025-01-10 10:10:10")),
            "EVENT_DATE",
        )])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::Timestamp(timestamp("2025-01-10 10:10:10")))
        );
    }

    #[test]
    fn test_handle_date_with_invalid_string_value() {
        let invalid_date_string = "invalid-date-format";
        let error = bind(&[tv(
            SqlTypeName::Date,
            SubstraitValue::String(invalid_date_string.to_string()),
            "EVENT_DATE",
        )])
        .unwrap_err();
        assert!(error.as_connector_exception().is_some());
        assert!(message(&error).contains("Can't handle date format"));
        assert!(message(&error).contains("value type"));
        assert!(message(&error).contains(invalid_date_string));
    }

    #[test]
    fn test_handle_date_with_mixed_types() {
        let statement = bind(&[
            tv(
                SqlTypeName::Varchar,
                SubstraitValue::String("test_value".to_string()),
                "COL_VARCHAR",
            ),
            tv(
                SqlTypeName::Date,
                SubstraitValue::Date(DateString::new("2025-01-10")),
                "EVENT_DATE",
            ),
            tv(
                SqlTypeName::Integer,
                SubstraitValue::Integer(12345),
                "COL_INT",
            ),
        ])
        .unwrap();
        assert_eq!(
            statement.parameter(1),
            Some(&SqlValue::String("test_value".to_string()))
        );
        assert_eq!(
            statement.parameter(2),
            Some(&SqlValue::Date(date("2025-01-10")))
        );
        assert_eq!(statement.parameter(3), Some(&SqlValue::Integer(12345)));
    }

    fn java_multiple_parameters_run() -> Vec<Vec<&'static str>> {
        JAVA_PROBE
            .lines()
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .skip_while(|f| f[..] != ["test", "testRealPreparedStatementWithMultipleParameters"])
            .skip(1)
            .take_while(|f| f[0] != "end")
            .collect()
    }

    #[tokio::test]
    async fn test_real_prepared_statement_with_multiple_parameters() {
        let sql = "SELECT * FROM test_schema.test_table WHERE int_col > ? AND decimal_col < ? AND bool_col = ?";
        let mut stmt = PreparedStatement::new(sql);
        let int_param = tv(
            SqlTypeName::Integer,
            SubstraitValue::Integer(500),
            "int_col",
        );
        let decimal_param = tv(
            SqlTypeName::Decimal,
            SubstraitValue::Decimal("80000.00".parse().unwrap()),
            "decimal_col",
        );
        let bool_param = tv(
            SqlTypeName::Boolean,
            SubstraitValue::Boolean(true),
            "bool_col",
        );
        let accumulator = [int_param, decimal_param, bool_param];
        handle_data_types_for_prepared_statement(&mut stmt, &accumulator).unwrap();

        let java = java_multiple_parameters_run();
        assert!(java.contains(&vec!["prepare", sql]), "{java:?}");
        assert!(java.contains(&vec!["result", "passed"]), "{java:?}");
        let java_binds: Vec<&[&str]> = java
            .iter()
            .filter(|f| f[0] == "bind")
            .map(|f| &f[1..])
            .collect();
        assert_eq!(
            java_binds,
            [
                &["setInt", "1", "java.lang.Integer", "500"][..],
                &["setBigDecimal", "2", "java.math.BigDecimal", "80000.00"][..],
                &["setBoolean", "3", "java.lang.Boolean", "true"][..],
            ]
        );
        assert_eq!(
            stmt.parameters(),
            &[
                SqlValue::Integer(500),
                SqlValue::Decimal("80000.00".parse().unwrap()),
                SqlValue::Boolean(true),
            ]
        );
        let Some(SqlValue::Decimal(decimal)) = stmt.parameter(2) else {
            panic!("expected a decimal parameter");
        };
        assert_eq!(big_decimal_to_string(decimal), "80000.00");

        let h2_row: Vec<SqlValue> = java
            .iter()
            .filter(|f| f[0] == "get")
            .map(|f| match f[1] {
                "getInt" => SqlValue::Integer(f[3].parse().unwrap()),
                "getBoolean" => SqlValue::Boolean(f[3].parse().unwrap()),
                other => panic!("unexpected getter {other}"),
            })
            .collect();
        let mut rs = ResultSet::from_rows(
            ResultSetMetaData::of_labels(["int_col", "bool_col"]),
            vec![h2_row],
        );
        assert!(rs.next().await.unwrap(), "Should find matching record");
        assert_eq!(Some(1000), rs.get_int("int_col").unwrap());
        assert_eq!(Some(true), rs.get_boolean("bool_col").unwrap());
    }

    #[test]
    fn handle_data_types_reports_java_class_cast_text() {
        let error = bind(&[tv(
            SqlTypeName::Bigint,
            SubstraitValue::String("x".to_string()),
            "c",
        )])
        .unwrap_err();
        assert!(matches!(error, FederationError::ClassCast(_)), "{error:?}");
        assert_eq!(
            message(&error),
            "class java.lang.String cannot be cast to class java.lang.Number (java.lang.String and java.lang.Number are in module java.base of loader 'bootstrap')"
        );
    }
}
