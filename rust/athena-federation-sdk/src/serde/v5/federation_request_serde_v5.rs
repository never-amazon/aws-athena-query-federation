use crate::exceptions::Result;
use crate::request::FederationRequest;
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SERDE_VERSION_FIVE};

pub fn serialize(request: &FederationRequest, jgen: &mut JsonGenerator) -> Result<()> {
    crate::serde::v2::federation_request_serde::serialize(request, jgen, SERDE_VERSION_FIVE)
}

pub fn do_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<FederationRequest> {
    let ctx = SerDeContext {
        version: SERDE_VERSION_FIVE,
        allocator: ctx.allocator,
    };
    crate::serde::v2::federation_request_serde::do_deserialize(jparser, &ctx)
}
