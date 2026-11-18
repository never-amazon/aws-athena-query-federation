use crate::exceptions::Result;
use crate::request::FederationResponse;
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SERDE_VERSION_FOUR};

pub fn serialize(response: &FederationResponse, jgen: &mut JsonGenerator) -> Result<()> {
    crate::serde::v2::federation_response_serde::serialize(response, jgen, SERDE_VERSION_FOUR)
}

pub fn do_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<FederationResponse> {
    let ctx = SerDeContext {
        version: SERDE_VERSION_FOUR,
        allocator: ctx.allocator,
    };
    crate::serde::v2::federation_response_serde::do_deserialize(jparser, &ctx)
}
