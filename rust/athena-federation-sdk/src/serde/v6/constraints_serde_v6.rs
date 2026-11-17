use crate::domain::predicate::Constraints;
use crate::exceptions::Result;
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SERDE_VERSION_SIX};

pub fn serialize(constraints: Option<&Constraints>, jgen: &mut JsonGenerator) -> Result<()> {
    crate::serde::v2::constraints_serde::serialize(constraints, jgen, SERDE_VERSION_SIX)
}

pub fn deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<Option<Constraints>> {
    let ctx = SerDeContext {
        version: SERDE_VERSION_SIX,
        allocators: ctx.allocators,
    };
    crate::serde::v2::constraints_serde::deserialize(jparser, &ctx)
}
