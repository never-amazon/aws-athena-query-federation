use crate::exceptions::Result;
use crate::serde::base::{
    assert_field_name, get_next_string_field, no_enum_constant, require_non_null,
};
use crate::serde::federated_identity_serde;
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;
use crate::udf::{UserDefinedFunctionRequest, UserDefinedFunctionType};

const IDENTITY_FIELD: &str = "identity";
const INPUT_RECORDS_FIELD: &str = "inputRecords";
const OUTPUT_SCHEMA_FIELD: &str = "outputSchema";
const METHOD_NAME_FIELD: &str = "methodName";
const FUNCTION_TYPE_FIELD: &str = "functionType";

pub fn do_typed_serialize(
    request: &UserDefinedFunctionRequest,
    jgen: &mut JsonGenerator,
    version: i32,
) -> Result<()> {
    jgen.write_field_name(IDENTITY_FIELD)?;
    federated_identity_serde::serialize(request.get_identity(), jgen)?;
    jgen.write_field_name(INPUT_RECORDS_FIELD)?;
    super::block_serde::serialize(Some(request.get_input_records()), jgen, version)?;
    jgen.write_field_name(OUTPUT_SCHEMA_FIELD)?;
    crate::serde::v3::schema_serde_v3::serialize_for_version(
        request.get_output_schema(),
        jgen,
        version,
    )?;
    jgen.write_string_field(METHOD_NAME_FIELD, Some(request.get_method_name()))?;
    jgen.write_string_field(
        FUNCTION_TYPE_FIELD,
        Some(request.get_function_type().name()),
    )
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<UserDefinedFunctionRequest> {
    assert_field_name(jparser, Some(IDENTITY_FIELD))?;
    let identity = federated_identity_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(INPUT_RECORDS_FIELD))?;
    let input_records = super::block_serde::deserialize(jparser, ctx)?;
    assert_field_name(jparser, Some(OUTPUT_SCHEMA_FIELD))?;
    let output_schema = super::block_serde::deserialize_schema_for_version(jparser, ctx.version)?;
    let method_name = get_next_string_field(jparser, METHOD_NAME_FIELD)?;
    let function_type_name = get_next_string_field(jparser, FUNCTION_TYPE_FIELD)?;
    let function_type = UserDefinedFunctionType::value_of(
        function_type_name.as_deref().unwrap_or(""),
    )
    .ok_or_else(|| {
        no_enum_constant(
            "com.amazonaws.athena.connector.lambda.udf.UserDefinedFunctionType",
            function_type_name.as_deref(),
        )
    })?;
    Ok(UserDefinedFunctionRequest::new(
        identity,
        require_non_null(input_records, "inputRecords is null")?,
        output_schema,
        require_non_null(method_name, "methodName is null")?,
        function_type,
    ))
}
