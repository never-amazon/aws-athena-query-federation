use crate::exceptions::Result;
use crate::request::FederationRequest;
use crate::serde::base::{get_type, no_serde_configured, serialize_object, write_type};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{ping_request_serde, SerDeContext, SerdeVersion};

pub const V2_SUB_TYPES: &[&str] = &[
    "PingRequest",
    "ListSchemasRequest",
    "ListTablesRequest",
    "GetTableRequest",
    "GetTableLayoutRequest",
    "GetSplitsRequest",
    "ReadRecordsRequest",
    "UserDefinedFunctionRequest",
];

pub const V4_SUB_TYPES: &[&str] = &[
    "PingRequest",
    "ListSchemasRequest",
    "ListTablesRequest",
    "GetTableRequest",
    "GetTableLayoutRequest",
    "GetSplitsRequest",
    "ReadRecordsRequest",
    "UserDefinedFunctionRequest",
    "GetDataSourceCapabilitiesRequest",
];

pub fn sub_types_for_version(version: SerdeVersion) -> &'static [&'static str] {
    if version >= SerdeVersion::V4 {
        V4_SUB_TYPES
    } else {
        V2_SUB_TYPES
    }
}

pub fn serialize(
    request: &FederationRequest,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    serialize_object(jgen, |j| do_serialize(request, j, version))
}

pub fn do_serialize(
    request: &FederationRequest,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    let type_name = request.type_name();
    if !sub_types_for_version(version).contains(&type_name) {
        return Err(no_serde_configured(type_name));
    }
    write_type(jgen, type_name)?;
    match request {
        FederationRequest::Ping(r) => ping_request_serde::do_typed_serialize(r, jgen),
        FederationRequest::ListSchemas(r) => {
            super::list_schemas_request_serde::do_typed_serialize(r, jgen)
        }
        FederationRequest::ListTables(r) => {
            super::list_tables_request_serde::do_typed_serialize(r, jgen)
        }
        FederationRequest::GetTable(r) => {
            if version >= SerdeVersion::V5 {
                crate::serde::v5::get_table_request_serde_v5::do_typed_serialize(r, jgen)
            } else {
                super::get_table_request_serde::do_typed_serialize(r, jgen)
            }
        }
        FederationRequest::GetTableLayout(r) => {
            super::get_table_layout_request_serde::do_typed_serialize(r, jgen, version)
        }
        FederationRequest::GetSplits(r) => {
            super::get_splits_request_serde::do_typed_serialize(r, jgen, version)
        }
        FederationRequest::ReadRecords(r) => {
            super::read_records_request_serde::do_typed_serialize(r, jgen, version)
        }
        FederationRequest::UserDefinedFunction(r) => {
            super::user_defined_function_request_serde::do_typed_serialize(r, jgen, version)
        }
        FederationRequest::GetDataSourceCapabilities(r) => {
            crate::serde::v4::get_data_source_capabilities_request_serde_v4::do_typed_serialize(
                r, jgen,
            )
        }
    }
}

pub fn do_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<FederationRequest> {
    let type_name = get_type(jparser)?;
    let name = type_name.as_deref().unwrap_or("null");
    if !sub_types_for_version(ctx.version).contains(&name) {
        return Err(no_serde_configured(name));
    }
    Ok(match name {
        "PingRequest" => ping_request_serde::do_typed_deserialize(jparser)?.into(),
        "ListSchemasRequest" => {
            super::list_schemas_request_serde::do_typed_deserialize(jparser)?.into()
        }
        "ListTablesRequest" => {
            super::list_tables_request_serde::do_typed_deserialize(jparser)?.into()
        }
        "GetTableRequest" => {
            if ctx.version >= SerdeVersion::V5 {
                crate::serde::v5::get_table_request_serde_v5::do_typed_deserialize(jparser)?.into()
            } else {
                super::get_table_request_serde::do_typed_deserialize(jparser)?.into()
            }
        }
        "GetTableLayoutRequest" => {
            super::get_table_layout_request_serde::do_typed_deserialize(jparser, ctx)?.into()
        }
        "GetSplitsRequest" => {
            super::get_splits_request_serde::do_typed_deserialize(jparser, ctx)?.into()
        }
        "ReadRecordsRequest" => {
            super::read_records_request_serde::do_typed_deserialize(jparser, ctx)?.into()
        }
        "UserDefinedFunctionRequest" => {
            super::user_defined_function_request_serde::do_typed_deserialize(jparser, ctx)?.into()
        }
        "GetDataSourceCapabilitiesRequest" => {
            crate::serde::v4::get_data_source_capabilities_request_serde_v4::do_typed_deserialize(
                jparser,
            )?
            .into()
        }
        other => return Err(no_serde_configured(other)),
    })
}
