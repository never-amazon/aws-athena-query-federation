use crate::exceptions::Result;
use crate::request::FederationResponse;
use crate::serde::base::{get_type, no_serde_configured, serialize_object, write_type};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{ping_response_serde, SerDeContext};

pub const V2_SUB_TYPES: &[&str] = &[
    "PingResponse",
    "ListSchemasResponse",
    "ListTablesResponse",
    "GetTableResponse",
    "GetTableLayoutResponse",
    "GetSplitsResponse",
    "ReadRecordsResponse",
    "RemoteReadRecordsResponse",
    "UserDefinedFunctionResponse",
];

pub const V4_SUB_TYPES: &[&str] = &[
    "PingResponse",
    "ListSchemasResponse",
    "ListTablesResponse",
    "GetTableResponse",
    "GetTableLayoutResponse",
    "GetSplitsResponse",
    "ReadRecordsResponse",
    "RemoteReadRecordsResponse",
    "UserDefinedFunctionResponse",
    "GetDataSourceCapabilitiesResponse",
];

pub fn sub_types_for_version(version: i32) -> &'static [&'static str] {
    if version >= 4 {
        V4_SUB_TYPES
    } else {
        V2_SUB_TYPES
    }
}

pub fn serialize(response: &FederationResponse, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    serialize_object(jgen, |j| do_serialize(response, j, version))
}

pub fn do_serialize(response: &FederationResponse, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    let type_name = response.type_name();
    if !sub_types_for_version(version).contains(&type_name) {
        return Err(no_serde_configured(type_name));
    }
    write_type(jgen, type_name)?;
    match response {
        FederationResponse::Ping(r) => ping_response_serde::do_typed_serialize(r, jgen),
        FederationResponse::ListSchemas(r) => super::list_schemas_response_serde::do_typed_serialize(r, jgen),
        FederationResponse::ListTables(r) => super::list_tables_response_serde::do_typed_serialize(r, jgen),
        FederationResponse::GetTable(r) => super::get_table_response_serde::do_typed_serialize(r, jgen, version),
        FederationResponse::GetTableLayout(r) => super::get_table_layout_response_serde::do_typed_serialize(r, jgen, version),
        FederationResponse::GetSplits(r) => super::get_splits_response_serde::do_typed_serialize(r, jgen),
        FederationResponse::ReadRecords(r) => super::read_records_response_serde::do_typed_serialize(r, jgen, version),
        FederationResponse::RemoteReadRecords(r) => {
            super::remote_read_records_response_serde::do_typed_serialize(r, jgen, version)
        }
        FederationResponse::UserDefinedFunction(r) => {
            super::user_defined_function_response_serde::do_typed_serialize(r, jgen, version)
        }
        FederationResponse::GetDataSourceCapabilities(r) => {
            crate::serde::v4::get_data_source_capabilities_response_serde_v4::do_typed_serialize(r, jgen)
        }
    }
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<FederationResponse> {
    let type_name = get_type(jparser)?;
    let name = type_name.as_deref().unwrap_or("null");
    if !sub_types_for_version(ctx.version).contains(&name) {
        return Err(no_serde_configured(name));
    }
    Ok(match name {
        "PingResponse" => ping_response_serde::do_typed_deserialize(jparser)?.into(),
        "ListSchemasResponse" => super::list_schemas_response_serde::do_typed_deserialize(jparser)?.into(),
        "ListTablesResponse" => super::list_tables_response_serde::do_typed_deserialize(jparser)?.into(),
        "GetTableResponse" => super::get_table_response_serde::do_typed_deserialize(jparser, ctx)?.into(),
        "GetTableLayoutResponse" => super::get_table_layout_response_serde::do_typed_deserialize(jparser, ctx)?.into(),
        "GetSplitsResponse" => super::get_splits_response_serde::do_typed_deserialize(jparser)?.into(),
        "ReadRecordsResponse" => super::read_records_response_serde::do_typed_deserialize(jparser, ctx)?.into(),
        "RemoteReadRecordsResponse" => super::remote_read_records_response_serde::do_typed_deserialize(jparser, ctx)?.into(),
        "UserDefinedFunctionResponse" => super::user_defined_function_response_serde::do_typed_deserialize(jparser, ctx)?.into(),
        "GetDataSourceCapabilitiesResponse" => {
            crate::serde::v4::get_data_source_capabilities_response_serde_v4::do_typed_deserialize(jparser)?.into()
        }
        other => return Err(no_serde_configured(other)),
    })
}
