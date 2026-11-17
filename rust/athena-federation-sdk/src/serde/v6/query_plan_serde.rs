use crate::domain::predicate::QueryPlan;
use crate::exceptions::Result;
use crate::serde::base::{deserialize_nullable, get_next_string_field, require_non_null, serialize_nullable};
use crate::serde::json::{JsonGenerator, JsonParser};

const SUBSTRAIT_VERSION_FIELD: &str = "substraitVersion";
const SUBSTRAIT_PLAN_FIELD: &str = "substraitPlan";

pub fn serialize(query_plan: Option<&QueryPlan>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, query_plan, do_serialize)
}

pub fn do_serialize(query_plan: &QueryPlan, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(SUBSTRAIT_VERSION_FIELD, query_plan.get_substrait_version())?;
    jgen.write_string_field(SUBSTRAIT_PLAN_FIELD, Some(query_plan.get_substrait_plan()))
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<QueryPlan>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<QueryPlan> {
    let substrait_version = get_next_string_field(jparser, SUBSTRAIT_VERSION_FIELD)?;
    let substrait_plan = get_next_string_field(jparser, SUBSTRAIT_PLAN_FIELD)?;
    Ok(QueryPlan::new(substrait_version, require_non_null(substrait_plan, "substraitPlan is null")?))
}
