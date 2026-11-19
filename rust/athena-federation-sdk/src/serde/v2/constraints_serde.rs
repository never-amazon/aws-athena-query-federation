use std::collections::HashMap;

use indexmap::IndexMap;

use crate::domain::predicate::expression::FederationExpression;
use crate::domain::predicate::{Constraints, OrderByField, QueryPlan, ValueSet};
use crate::exceptions::{FederationError, Result};
use crate::serde::base::{
    assert_field_name, deserialize_nullable, get_next_long_field, nullable_map,
    read_string_map_entries, require_non_null, serialize_nullable, validate_array_start,
    validate_object_end, validate_object_start, validate_object_start_token, write_string_map,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::{SerDeContext, SerdeVersion};

const SUMMARY_FIELD: &str = "summary";
const EXPRESSION_FIELD: &str = "expression";
const ORDER_BY_CLAUSE: &str = "orderByClause";
const LIMIT_FIELD: &str = "limit";
const QUERY_PASSTHROUGH_ARGUMENTS: &str = "queryPassthroughArguments";
const QUERY_PLAN: &str = "queryPlan";
const NO_LIMIT_VALUE: i64 = -1;

pub fn serialize(
    constraints: Option<&Constraints>,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    serialize_nullable(jgen, constraints, |c, j| do_serialize(c, j, version))
}

pub fn do_serialize(
    constraints: &Constraints,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_object_field_start(SUMMARY_FIELD)?;
    for (column, value_set) in constraints.summary() {
        jgen.write_field_name(column)?;
        super::value_set_serde::serialize(Some(value_set), jgen, version)?;
    }
    jgen.write_end_object()?;
    if version <= SerdeVersion::V3 {
        return Ok(());
    }
    jgen.write_array_field_start(EXPRESSION_FIELD)?;
    for expression in constraints.expression() {
        crate::serde::v4::federation_expression_serde_v4::serialize(
            Some(expression),
            jgen,
            version,
        )?;
    }
    jgen.write_end_array()?;
    jgen.write_array_field_start(ORDER_BY_CLAUSE)?;
    for order_by_field in constraints.order_by_clause() {
        crate::serde::v4::order_by_field_serde_v4::serialize(Some(order_by_field), jgen)?;
    }
    jgen.write_end_array()?;
    jgen.write_number_field_i64(LIMIT_FIELD, constraints.wire_limit())?;
    if version >= SerdeVersion::V5 {
        write_string_map(
            jgen,
            QUERY_PASSTHROUGH_ARGUMENTS,
            constraints.query_passthrough_arguments(),
        )?;
    }
    if version >= SerdeVersion::V6 {
        match constraints.query_plan() {
            Some(plan) => {
                jgen.write_field_name(QUERY_PLAN)?;
                crate::serde::v6::query_plan_serde::serialize(Some(plan), jgen)?;
            }
            None => jgen.write_null_field(QUERY_PLAN)?,
        }
    }
    Ok(())
}

pub fn deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<Option<Constraints>> {
    deserialize_nullable(jparser, |p| do_deserialize(p, ctx))
}

fn read_object_array<T>(
    jparser: &mut JsonParser<'_>,
    mut read: impl FnMut(&mut JsonParser<'_>) -> Result<T>,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => {
                return Err(FederationError::Io(
                    "Unexpected end-of-input: expected close marker for ARRAY".into(),
                ))
            }
            token => {
                validate_object_start_token(token)?;
                items.push(read(jparser)?);
                validate_object_end(jparser)?;
            }
        }
    }
    Ok(items)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Constraints> {
    assert_field_name(jparser, Some(SUMMARY_FIELD))?;
    validate_object_start_token(jparser.next_token()?)?;
    let mut summary: IndexMap<String, ValueSet> = IndexMap::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndObject) => break,
            None => {
                return Err(FederationError::Io(
                    "Unexpected end-of-input: expected close marker for OBJECT".into(),
                ))
            }
            Some(_) => {
                let column = jparser.current_name().unwrap_or_default();
                let value_set = super::value_set_serde::deserialize(jparser, ctx)?;
                let value_set = require_non_null(value_set, "null value in entry: summary")?;
                if summary.contains_key(&column) {
                    return Err(FederationError::IllegalArgument(format!(
                        "Multiple entries with same key: {column}"
                    )));
                }
                summary.insert(column, value_set);
            }
        }
    }
    if ctx.version <= SerdeVersion::V3 {
        return Ok(Constraints::new(
            summary,
            Vec::new(),
            Vec::new(),
            NO_LIMIT_VALUE,
            HashMap::new(),
            None,
        ));
    }

    assert_field_name(jparser, Some(EXPRESSION_FIELD))?;
    validate_array_start(jparser)?;
    let expressions: Vec<FederationExpression> = read_object_array(jparser, |p| {
        crate::serde::v4::federation_expression_serde_v4::do_deserialize(p, ctx)
    })?;

    assert_field_name(jparser, Some(ORDER_BY_CLAUSE))?;
    validate_array_start(jparser)?;
    let order_by: Vec<OrderByField> = read_object_array(
        jparser,
        crate::serde::v4::order_by_field_serde_v4::do_deserialize,
    )?;

    let limit = get_next_long_field(jparser, LIMIT_FIELD)?;
    if ctx.version == SerdeVersion::V4 {
        return Ok(Constraints::new(
            summary,
            expressions,
            order_by,
            limit,
            HashMap::new(),
            None,
        ));
    }

    assert_field_name(jparser, Some(QUERY_PASSTHROUGH_ARGUMENTS))?;
    validate_object_start(jparser)?;
    let query_passthrough_arguments = nullable_map(read_string_map_entries(jparser)?);
    if ctx.version == SerdeVersion::V5 {
        return Ok(Constraints::new(
            summary,
            expressions,
            order_by,
            limit,
            query_passthrough_arguments,
            None,
        ));
    }

    let mut query_plan: Option<QueryPlan> = None;
    assert_field_name(jparser, Some(QUERY_PLAN))?;
    if jparser.next_token()? != Some(JsonToken::ValueNull) {
        validate_object_start_token(jparser.current_token())?;
        query_plan = Some(crate::serde::v6::query_plan_serde::do_deserialize(jparser)?);
        validate_object_end(jparser)?;
    }
    Ok(Constraints::new(
        summary,
        expressions,
        order_by,
        limit,
        query_passthrough_arguments,
        query_plan,
    ))
}
