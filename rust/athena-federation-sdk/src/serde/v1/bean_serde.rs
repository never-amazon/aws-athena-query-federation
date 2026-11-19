use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{
    DataType, Field, Fields, IntervalUnit, Schema, TimeUnit, UnionFields, UnionMode,
};
use indexmap::{IndexMap, IndexSet};
use serde_json::Value as Json;

use crate::data::{ArrowType, Block};
use crate::domain::predicate::expression::{
    ConstantExpression, FederationExpression, FunctionCallExpression, VariableExpression,
};
use crate::domain::predicate::functions::FunctionName;
use crate::domain::predicate::{
    AllOrNoneValueSet, Constraints, Direction, EquatableValueSet, Marker, OrderByField, QueryPlan,
    Range, SortedRangeSet, ValueSet,
};
use crate::domain::spill::{S3SpillLocation, SpillLocation};
use crate::domain::split::Split;
use crate::domain::table_name::TableName;
use crate::exceptions::{FederationError, Result};
use crate::java_cast::double_to_long;
use crate::metadata::optimizations::OptimizationSubType;
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse, MetadataHeader,
};
use crate::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::request::{FederationRequest, FederationResponse, PingRequest, PingResponse};
use crate::security::{EncryptionKey, FederatedIdentity};
use crate::serde::base::{write_type, StringMapValue};
use crate::serde::java_hash::java_hash_map_order;
use crate::serde::json::{decode_base64, JsonGenerator, JsonParser};
use crate::serde::v2::arrow_type_serde::{interval_unit_name, time_unit_name, union_mode_name};
use crate::serde::{
    block_deserializer, block_serializer, schema_deserializer, schema_serializer, SerDeContext,
};
use crate::udf::{
    UserDefinedFunctionRequest, UserDefinedFunctionResponse, UserDefinedFunctionType,
};

pub const REQUEST_SUB_TYPES: &[&str] = &[
    "ListSchemasRequest",
    "ListTablesRequest",
    "GetTableRequest",
    "GetTableLayoutRequest",
    "GetSplitsRequest",
    "ReadRecordsRequest",
    "UserDefinedFunctionRequest",
    "PingRequest",
    "GetDataSourceCapabilitiesRequest",
];

pub const RESPONSE_SUB_TYPES: &[&str] = &[
    "ListSchemasResponse",
    "ListTablesResponse",
    "GetTableResponse",
    "GetTableLayoutResponse",
    "GetSplitsResponse",
    "ReadRecordsResponse",
    "RemoteReadRecordsResponse",
    "UserDefinedFunctionResponse",
    "PingResponse",
];

fn io(message: impl Into<String>) -> FederationError {
    FederationError::io(message.into())
}

fn to_io(e: FederationError) -> FederationError {
    match e {
        FederationError::Io { .. } => e,
        other => FederationError::io(other.to_string()),
    }
}

pub fn serialize_block(block: Option<&Block>, jgen: &mut JsonGenerator) -> Result<()> {
    match block {
        Some(b) => block_serializer::serialize(b, jgen),
        None => jgen.write_null(),
    }
}

pub fn serialize_schema(schema: &Schema, jgen: &mut JsonGenerator) -> Result<()> {
    schema_serializer::serialize(schema, jgen)
}

fn write_string_map_bean<V: StringMapValue>(
    jgen: &mut JsonGenerator,
    name: &str,
    values: &HashMap<String, V>,
) -> Result<()> {
    jgen.write_object_field_start(name)?;
    for key in java_hash_map_order(values.keys()) {
        jgen.write_string_field(key, values.get(key).and_then(|v| v.as_opt_str()))?;
    }
    jgen.write_end_object()
}

fn write_strings<'a, V: StringMapValue + 'a, I: IntoIterator<Item = &'a V>>(
    jgen: &mut JsonGenerator,
    name: &str,
    values: I,
) -> Result<()> {
    jgen.write_array_field_start(name)?;
    for v in values {
        jgen.write_string(v.as_opt_str())?;
    }
    jgen.write_end_array()
}

fn serialize_identity(
    identity: Option<&FederatedIdentity>,
    jgen: &mut JsonGenerator,
) -> Result<()> {
    let Some(identity) = identity else {
        return jgen.write_null();
    };
    jgen.write_start_object()?;
    jgen.write_string_field("arn", identity.arn())?;
    jgen.write_string_field("account", identity.account())?;
    write_string_map_bean(jgen, "principalTags", identity.principal_tags())?;
    write_strings(jgen, "iamGroups", identity.iam_groups())?;
    write_string_map_bean(jgen, "configOptions", identity.config_options())?;
    jgen.write_end_object()
}

fn serialize_table_name(table_name: &TableName, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    jgen.write_string_field("schemaName", Some(table_name.schema_name()))?;
    jgen.write_string_field("tableName", Some(table_name.table_name()))?;
    jgen.write_string_field(
        "qualifiedTableName",
        Some(&table_name.get_qualified_table_name()),
    )?;
    jgen.write_end_object()
}

pub fn arrow_type_bean_name(arrow_type: &ArrowType) -> &'static str {
    match arrow_type {
        DataType::Null => "null",
        DataType::Struct(_) => "struct",
        DataType::List(_) => "list",
        DataType::LargeList(_) => "largelist",
        DataType::FixedSizeList(_, _) => "fixedsizelist",
        DataType::Union(_, _) => "union",
        DataType::Map(_, _) => "map",
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => "int",
        DataType::Float16 | DataType::Float32 | DataType::Float64 => "floatingpoint",
        DataType::Utf8 => "utf8",
        DataType::Utf8View => "utf8view",
        DataType::LargeUtf8 => "largeutf8",
        DataType::Binary => "binary",
        DataType::BinaryView => "binaryview",
        DataType::LargeBinary => "largebinary",
        DataType::FixedSizeBinary(_) => "fixedsizebinary",
        DataType::Boolean => "bool",
        DataType::Decimal32(_, _)
        | DataType::Decimal64(_, _)
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _) => "decimal",
        DataType::Date32 | DataType::Date64 => "date",
        DataType::Time32(_) | DataType::Time64(_) => "time",
        DataType::Timestamp(_, _) => "timestamp",
        DataType::Interval(_) => "interval",
        DataType::Duration(_) => "duration",
        DataType::ListView(_) => "listview",
        DataType::LargeListView(_) => "largelistview",
        DataType::RunEndEncoded(_, _) => "runendencoded",
        DataType::Dictionary(_, value) => arrow_type_bean_name(value),
    }
}

fn serialize_arrow_type(arrow_type: &ArrowType, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    jgen.write_string_field("name", Some(arrow_type_bean_name(arrow_type)))?;
    match arrow_type {
        DataType::FixedSizeList(_, size) => jgen.write_number_field_i32("listSize", *size)?,
        DataType::Union(fields, mode) => {
            jgen.write_string_field("mode", Some(union_mode_name(*mode)))?;
            jgen.write_array_field_start("typeIds")?;
            for (id, _) in fields.iter() {
                jgen.write_number_i32(i32::from(id))?;
            }
            jgen.write_end_array()?;
        }
        DataType::Map(_, sorted) => jgen.write_boolean_field("keysSorted", *sorted)?,
        DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64 => {
            let width = match arrow_type {
                DataType::Int8 => 8,
                DataType::Int16 => 16,
                DataType::Int32 => 32,
                _ => 64,
            };
            jgen.write_number_field_i32("bitWidth", width)?;
            jgen.write_boolean_field("isSigned", true)?;
        }
        DataType::UInt8 | DataType::UInt16 | DataType::UInt32 | DataType::UInt64 => {
            let width = match arrow_type {
                DataType::UInt8 => 8,
                DataType::UInt16 => 16,
                DataType::UInt32 => 32,
                _ => 64,
            };
            jgen.write_number_field_i32("bitWidth", width)?;
            jgen.write_boolean_field("isSigned", false)?;
        }
        DataType::Float16 => jgen.write_string_field("precision", Some("HALF"))?,
        DataType::Float32 => jgen.write_string_field("precision", Some("SINGLE"))?,
        DataType::Float64 => jgen.write_string_field("precision", Some("DOUBLE"))?,
        DataType::FixedSizeBinary(w) => jgen.write_number_field_i32("byteWidth", *w)?,
        DataType::Decimal32(p, s)
        | DataType::Decimal64(p, s)
        | DataType::Decimal128(p, s)
        | DataType::Decimal256(p, s) => {
            let width = match arrow_type {
                DataType::Decimal32(_, _) => 32,
                DataType::Decimal64(_, _) => 64,
                DataType::Decimal128(_, _) => 128,
                _ => 256,
            };
            jgen.write_number_field_i32("precision", i32::from(*p))?;
            jgen.write_number_field_i32("scale", i32::from(*s))?;
            jgen.write_number_field_i32("bitWidth", width)?;
        }
        DataType::Date32 => jgen.write_string_field("unit", Some("DAY"))?,
        DataType::Date64 => jgen.write_string_field("unit", Some("MILLISECOND"))?,
        DataType::Time32(unit) => {
            jgen.write_string_field("unit", Some(time_unit_name(unit)))?;
            jgen.write_number_field_i32("bitWidth", 32)?;
        }
        DataType::Time64(unit) => {
            jgen.write_string_field("unit", Some(time_unit_name(unit)))?;
            jgen.write_number_field_i32("bitWidth", 64)?;
        }
        DataType::Timestamp(unit, tz) => {
            jgen.write_string_field("unit", Some(time_unit_name(unit)))?;
            jgen.write_string_field("timezone", tz.as_deref())?;
        }
        DataType::Interval(unit) => {
            jgen.write_string_field("unit", Some(interval_unit_name(unit)))?;
        }
        DataType::Duration(unit) => jgen.write_string_field("unit", Some(time_unit_name(unit)))?,
        _ => {}
    }
    jgen.write_end_object()
}

pub fn serialize_marker(marker: &Marker, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    jgen.write_field_name("valueBlock")?;
    serialize_block(Some(&marker.get_value_block()?), jgen)?;
    jgen.write_string_field("bound", Some(marker.bound().as_str()))?;
    jgen.write_boolean_field("nullValue", marker.is_null_value())?;
    jgen.write_end_object()
}

fn serialize_value_set(value_set: &ValueSet, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    match value_set {
        ValueSet::EquatableValueSet(v) => {
            write_type(jgen, "equatable")?;
            jgen.write_field_name("valueBlock")?;
            serialize_block(Some(v.value_block()), jgen)?;
            jgen.write_boolean_field("whiteList", v.is_allow_list())?;
            jgen.write_boolean_field("nullAllowed", v.is_null_allowed())?;
            jgen.write_field_name("values")?;
            serialize_block(Some(v.values()), jgen)?;
        }
        ValueSet::SortedRangeSet(v) => {
            write_type(jgen, "sortable")?;
            jgen.write_field_name("type")?;
            serialize_arrow_type(v.get_type(), jgen)?;
            jgen.write_array_field_start("ranges")?;
            for range in v.ordered_ranges() {
                jgen.write_start_object()?;
                jgen.write_field_name("low")?;
                serialize_marker(range.low(), jgen)?;
                jgen.write_field_name("high")?;
                serialize_marker(range.high(), jgen)?;
                jgen.write_field_name("type")?;
                serialize_arrow_type(range.get_type(), jgen)?;
                jgen.write_end_object()?;
            }
            jgen.write_end_array()?;
            jgen.write_boolean_field("nullAllowed", v.is_null_allowed())?;
        }
        ValueSet::AllOrNoneValueSet(v) => {
            write_type(jgen, "allOrNone")?;
            jgen.write_field_name("type")?;
            serialize_arrow_type(v.get_type(), jgen)?;
            jgen.write_boolean_field("nullAllowed", v.is_null_allowed())?;
        }
    }
    jgen.write_end_object()
}

fn serialize_expression(expression: &FederationExpression, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    match expression {
        FederationExpression::FunctionCall(f) => {
            write_type(jgen, "functionCall")?;
            jgen.write_field_name("type")?;
            serialize_arrow_type(f.get_type(), jgen)?;
            jgen.write_object_field_start("functionName")?;
            jgen.write_string_field("functionName", Some(f.function_name().function_name()))?;
            jgen.write_end_object()?;
            jgen.write_array_field_start("arguments")?;
            for a in f.arguments() {
                serialize_expression(a, jgen)?;
            }
            jgen.write_end_array()?;
            jgen.write_array_field_start("children")?;
            for a in f.children() {
                serialize_expression(a, jgen)?;
            }
            jgen.write_end_array()?;
        }
        FederationExpression::Variable(v) => {
            write_type(jgen, "variable")?;
            jgen.write_string_field("columnName", Some(v.column_name()))?;
            jgen.write_field_name("type")?;
            serialize_arrow_type(v.get_type(), jgen)?;
            jgen.write_array_field_start("children")?;
            jgen.write_end_array()?;
        }
        FederationExpression::Constant(c) => {
            write_type(jgen, "constant")?;
            jgen.write_field_name("valueBlock")?;
            serialize_block(c.values(), jgen)?;
            jgen.write_field_name("type")?;
            serialize_arrow_type(c.get_type(), jgen)?;
            jgen.write_array_field_start("children")?;
            jgen.write_end_array()?;
        }
    }
    jgen.write_end_object()
}

fn serialize_constraints(constraints: &Constraints, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    jgen.write_object_field_start("summary")?;
    for (k, v) in constraints.summary() {
        jgen.write_field_name(k)?;
        serialize_value_set(v, jgen)?;
    }
    jgen.write_end_object()?;
    jgen.write_array_field_start("expression")?;
    for e in constraints.expression() {
        serialize_expression(e, jgen)?;
    }
    jgen.write_end_array()?;
    jgen.write_array_field_start("orderByClause")?;
    for o in constraints.order_by_clause() {
        jgen.write_start_object()?;
        jgen.write_string_field("columnName", Some(o.column_name()))?;
        jgen.write_string_field("direction", Some(o.direction().as_str()))?;
        jgen.write_end_object()?;
    }
    jgen.write_end_array()?;
    jgen.write_number_field_i64("limit", constraints.wire_limit())?;
    write_string_map_bean(
        jgen,
        "queryPassthroughArguments",
        constraints.query_passthrough_arguments(),
    )?;
    jgen.write_field_name("queryPlan")?;
    match constraints.query_plan() {
        Some(plan) => {
            jgen.write_start_object()?;
            jgen.write_string_field("substraitVersion", plan.substrait_version())?;
            jgen.write_string_field("substraitPlan", Some(plan.substrait_plan()))?;
            jgen.write_end_object()?;
        }
        None => jgen.write_null()?,
    }
    jgen.write_boolean_field("queryPassThrough", constraints.is_query_pass_through())?;
    jgen.write_end_object()
}

fn serialize_spill_location(location: &SpillLocation, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    match location {
        SpillLocation::S3(s3) => {
            write_type(jgen, "S3SpillLocation")?;
            jgen.write_string_field("bucket", s3.bucket_value())?;
            jgen.write_string_field("key", Some(s3.key()))?;
            jgen.write_boolean_field("directory", s3.is_directory())?;
        }
    }
    jgen.write_end_object()
}

fn serialize_encryption_key(key: Option<&EncryptionKey>, jgen: &mut JsonGenerator) -> Result<()> {
    match key {
        Some(k) => {
            jgen.write_start_object()?;
            jgen.write_binary_field("key", k.key())?;
            jgen.write_binary_field("nonce", k.nonce())?;
            jgen.write_end_object()
        }
        None => jgen.write_null(),
    }
}

fn serialize_split(split: &Split, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    jgen.write_field_name("spillLocation")?;
    match split.spill_location() {
        Some(l) => serialize_spill_location(l, jgen)?,
        None => jgen.write_null()?,
    }
    jgen.write_field_name("encryptionKey")?;
    serialize_encryption_key(split.encryption_key(), jgen)?;
    write_string_map_bean(jgen, "properties", split.properties())?;
    jgen.write_end_object()
}

fn write_metadata_header(request: MetadataHeader<'_>, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_field_name("identity")?;
    serialize_identity(request.identity, jgen)?;
    jgen.write_string_field("queryId", request.query_id)?;
    jgen.write_string_field("catalogName", Some(request.catalog_name))
}

fn write_metadata_trailer(request: MetadataHeader<'_>, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field("requestType", Some(request.request_type.as_str()))?;
    jgen.write_null_field("context")
}

pub fn serialize_request(request: &FederationRequest, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    write_type(jgen, request.type_name())?;
    match request {
        FederationRequest::Ping(r) => {
            jgen.write_field_name("identity")?;
            serialize_identity(r.identity(), jgen)?;
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_string_field("queryId", Some(r.query_id()))?;
        }
        FederationRequest::ListSchemas(r) => {
            write_metadata_header(r.header(), jgen)?;
            write_metadata_trailer(r.header(), jgen)?;
        }
        FederationRequest::ListTables(r) => {
            write_metadata_header(r.header(), jgen)?;
            jgen.write_string_field("schemaName", r.schema_name())?;
            jgen.write_string_field("nextToken", r.next_token())?;
            jgen.write_number_field_i32("pageSize", r.page_size())?;
            write_metadata_trailer(r.header(), jgen)?;
        }
        FederationRequest::GetTable(r) => {
            write_metadata_header(r.header(), jgen)?;
            jgen.write_field_name("tableName")?;
            serialize_table_name(r.table_name(), jgen)?;
            write_string_map_bean(
                jgen,
                "queryPassthroughArguments",
                r.query_passthrough_arguments(),
            )?;
            write_metadata_trailer(r.header(), jgen)?;
            jgen.write_boolean_field("queryPassthrough", r.is_query_passthrough())?;
        }
        FederationRequest::GetTableLayout(r) => {
            write_metadata_header(r.header(), jgen)?;
            jgen.write_field_name("tableName")?;
            serialize_table_name(r.table_name(), jgen)?;
            jgen.write_field_name("constraints")?;
            serialize_constraints(r.constraints(), jgen)?;
            jgen.write_field_name("schema")?;
            serialize_schema(r.schema(), jgen)?;
            write_strings(jgen, "partitionCols", r.partition_cols())?;
            write_metadata_trailer(r.header(), jgen)?;
        }
        FederationRequest::GetSplits(r) => {
            write_metadata_header(r.header(), jgen)?;
            jgen.write_field_name("tableName")?;
            serialize_table_name(r.table_name(), jgen)?;
            jgen.write_field_name("partitions")?;
            serialize_block(Some(r.partitions()), jgen)?;
            write_strings(jgen, "partitionCols", r.partition_cols())?;
            jgen.write_field_name("constraints")?;
            serialize_constraints(r.constraints(), jgen)?;
            jgen.write_string_field("continuationToken", r.continuation_token())?;
            write_metadata_trailer(r.header(), jgen)?;
        }
        FederationRequest::ReadRecords(r) => {
            jgen.write_field_name("identity")?;
            serialize_identity(r.identity(), jgen)?;
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_string_field("queryId", Some(r.query_id()))?;
            jgen.write_field_name("tableName")?;
            serialize_table_name(r.table_name(), jgen)?;
            jgen.write_field_name("schema")?;
            serialize_schema(r.schema(), jgen)?;
            jgen.write_field_name("split")?;
            serialize_split(r.split(), jgen)?;
            jgen.write_field_name("constraints")?;
            serialize_constraints(r.constraints(), jgen)?;
            jgen.write_number_field_i64("maxBlockSize", r.max_block_size())?;
            jgen.write_number_field_i64("maxInlineBlockSize", r.max_inline_block_size())?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationRequest::UserDefinedFunction(r) => {
            jgen.write_field_name("identity")?;
            serialize_identity(r.identity(), jgen)?;
            jgen.write_field_name("inputRecords")?;
            serialize_block(Some(r.input_records()), jgen)?;
            jgen.write_field_name("outputSchema")?;
            serialize_schema(r.output_schema(), jgen)?;
            jgen.write_string_field("methodName", Some(r.method_name()))?;
            jgen.write_string_field("functionType", Some(r.function_type().as_str()))?;
        }
        FederationRequest::GetDataSourceCapabilities(r) => {
            write_metadata_header(r.header(), jgen)?;
            write_metadata_trailer(r.header(), jgen)?;
        }
    }
    jgen.write_end_object()
}

pub fn serialize_response(response: &FederationResponse, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    write_type(jgen, response.type_name())?;
    match response {
        FederationResponse::Ping(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_string_field("queryId", Some(r.query_id()))?;
            jgen.write_string_field("sourceType", r.source_type())?;
            jgen.write_number_field_i32("capabilities", r.capabilities())?;
            jgen.write_number_field_i32("serDeVersion", r.ser_de_version())?;
        }
        FederationResponse::ListSchemas(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            write_strings(jgen, "schemas", r.schemas())?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::ListTables(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_array_field_start("tables")?;
            for t in r.tables() {
                serialize_table_name(t, jgen)?;
            }
            jgen.write_end_array()?;
            jgen.write_string_field("nextToken", r.next_token())?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::GetTable(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_field_name("tableName")?;
            serialize_table_name(r.table_name(), jgen)?;
            jgen.write_field_name("schema")?;
            serialize_schema(r.schema(), jgen)?;
            write_strings(jgen, "partitionColumns", r.partition_columns())?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::GetTableLayout(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_field_name("tableName")?;
            serialize_table_name(r.table_name(), jgen)?;
            jgen.write_field_name("partitions")?;
            serialize_block(Some(r.partitions()), jgen)?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::GetSplits(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_array_field_start("splits")?;
            for s in r.splits() {
                serialize_split(s, jgen)?;
            }
            jgen.write_end_array()?;
            jgen.write_string_field("continuationToken", r.continuation_token())?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::ReadRecords(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_field_name("records")?;
            serialize_block(Some(r.records()), jgen)?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::RemoteReadRecords(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_field_name("schema")?;
            serialize_schema(r.schema(), jgen)?;
            jgen.write_array_field_start("remoteBlocks")?;
            for b in r.remote_blocks() {
                serialize_spill_location(b, jgen)?;
            }
            jgen.write_end_array()?;
            jgen.write_field_name("encryptionKey")?;
            serialize_encryption_key(r.encryption_key(), jgen)?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
        FederationResponse::UserDefinedFunction(r) => {
            jgen.write_field_name("records")?;
            serialize_block(Some(r.records()), jgen)?;
            jgen.write_string_field("methodName", Some(r.method_name()))?;
        }
        FederationResponse::GetDataSourceCapabilities(r) => {
            jgen.write_string_field("catalogName", Some(r.catalog_name()))?;
            jgen.write_object_field_start("capabilities")?;
            for (name, sub_types) in r.capabilities() {
                jgen.write_array_field_start(name)?;
                for s in sub_types {
                    jgen.write_start_object()?;
                    jgen.write_string_field("subType", Some(s.sub_type()))?;
                    write_strings(jgen, "properties", s.properties())?;
                    jgen.write_end_object()?;
                }
                jgen.write_end_array()?;
            }
            jgen.write_end_object()?;
            jgen.write_string_field("requestType", Some(r.request_type().as_str()))?;
        }
    }
    jgen.write_end_object()
}

fn prop<'a>(node: &'a Json, name: &str) -> Option<&'a Json> {
    match node.get(name) {
        Some(Json::Null) | None => None,
        Some(v) => Some(v),
    }
}

fn scalar_text(value: &Json, name: &str) -> Result<String> {
    match value {
        Json::String(s) => Ok(s.clone()),
        Json::Number(n) => Ok(n.to_string()),
        Json::Bool(b) => Ok(b.to_string()),
        _ => Err(io(format!(
            "Cannot deserialize value of type `java.lang.String` from {} for property \"{}\"",
            kind(value),
            name
        ))),
    }
}

fn kind(value: &Json) -> &'static str {
    match value {
        Json::Null => "Null value",
        Json::Bool(_) => "Boolean value",
        Json::Number(_) => "Number value",
        Json::String(_) => "String value",
        Json::Array(_) => "Array value (token `JsonToken.START_ARRAY`)",
        Json::Object(_) => "Object value (token `JsonToken.START_OBJECT`)",
    }
}

fn opt_string(node: &Json, name: &str) -> Result<Option<String>> {
    prop(node, name).map(|v| scalar_text(v, name)).transpose()
}

fn req_string(node: &Json, name: &str) -> Result<String> {
    opt_string(node, name)?.ok_or_else(|| io(format!("{name} is null")))
}

fn long_value(node: &Json, name: &str) -> Result<i64> {
    match prop(node, name) {
        None => Ok(0),
        Some(Json::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(double_to_long))
            .ok_or_else(|| io(format!("Numeric value ({n}) out of range of long"))),
        Some(Json::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return Ok(0);
            }
            t.parse::<i64>().map_err(|_| {
                io(format!(
                    "Cannot deserialize value of type `long` from String \"{s}\""
                ))
            })
        }
        Some(Json::Bool(b)) => Err(io(format!(
            "Cannot coerce Boolean value ({b}) to `long` value"
        ))),
        Some(other) => Err(io(format!(
            "Cannot deserialize value of type `long` from {}",
            kind(other)
        ))),
    }
}

fn int_value(node: &Json, name: &str) -> Result<i32> {
    let v = long_value(node, name)?;
    i32::try_from(v).map_err(|_| io(format!("Numeric value ({v}) out of range of int")))
}

fn bool_value(node: &Json, name: &str) -> Result<bool> {
    match prop(node, name) {
        None => Ok(false),
        Some(Json::Bool(b)) => Ok(*b),
        Some(Json::Number(n)) => Ok(n.as_i64().is_some_and(|v| v != 0)),
        Some(Json::String(s)) => match s.trim() {
            "true" | "True" | "TRUE" => Ok(true),
            "false" | "False" | "FALSE" | "" => Ok(false),
            other => Err(io(format!(
                "Cannot deserialize value of type `boolean` from String \"{other}\""
            ))),
        },
        Some(other) => Err(io(format!(
            "Cannot deserialize value of type `boolean` from {}",
            kind(other)
        ))),
    }
}

fn required_list<T>(node: &Json, name: &str, item: impl Fn(&Json) -> Result<T>) -> Result<Vec<T>> {
    match prop(node, name) {
        None => Err(io(format!("{name} is null"))),
        Some(v) => list_items(v).into_iter().map(item).collect(),
    }
}

fn list_items(value: &Json) -> Vec<&Json> {
    match value {
        Json::Array(items) => items.iter().collect(),
        Json::Null => Vec::new(),
        other => vec![other],
    }
}

fn string_list(node: &Json, name: &str) -> Result<Option<Vec<String>>> {
    match prop(node, name) {
        None => Ok(None),
        Some(v) => list_items(v)
            .into_iter()
            .map(|item| scalar_text(item, name))
            .collect::<Result<Vec<String>>>()
            .map(Some),
    }
}

fn nullable_string_list(node: &Json, name: &str) -> Result<Option<Vec<Option<String>>>> {
    match prop(node, name) {
        None => Ok(None),
        Some(v) => list_items(v)
            .into_iter()
            .map(|item| match item {
                Json::Null => Ok(None),
                other => scalar_text(other, name).map(Some),
            })
            .collect::<Result<Vec<Option<String>>>>()
            .map(Some),
    }
}

fn nullable_string_map(node: &Json, name: &str) -> Result<Option<HashMap<String, Option<String>>>> {
    match prop(node, name) {
        None => Ok(None),
        Some(Json::Object(map)) => {
            let mut result = HashMap::new();
            for (k, v) in map {
                let value = match v {
                    Json::Null => None,
                    other => Some(scalar_text(other, name)?),
                };
                result.insert(k.clone(), value);
            }
            Ok(Some(result))
        }
        Some(other) => Err(io(format!(
            "Cannot deserialize value of type `java.util.Map` from {}",
            kind(other)
        ))),
    }
}

fn binary(node: &Json, name: &str) -> Result<Option<Vec<u8>>> {
    match prop(node, name) {
        None => Ok(None),
        Some(Json::String(s)) => decode_base64(s).map(Some),
        Some(other) => Err(io(format!(
            "Cannot deserialize value of type `byte[]` from {}",
            kind(other)
        ))),
    }
}

fn type_id<'a>(node: &'a Json, base: &str) -> Result<&'a str> {
    match node.get("@type") {
        Some(Json::String(s)) => Ok(s.as_str()),
        _ => Err(io(format!(
            "Could not resolve subtype of {base}: missing type id property '@type'"
        ))),
    }
}

fn identity_from(node: &Json) -> Result<Option<FederatedIdentity>> {
    let Some(value) = prop(node, "identity") else {
        return Ok(None);
    };
    Ok(Some(FederatedIdentity::new_nullable(
        opt_string(value, "arn")?,
        opt_string(value, "account")?,
        nullable_string_map(value, "principalTags")?.unwrap_or_default(),
        nullable_string_list(value, "iamGroups")?.unwrap_or_default(),
        nullable_string_map(value, "configOptions")?.unwrap_or_default(),
    )))
}

fn table_name_from(value: Option<&Json>) -> Result<Option<TableName>> {
    match value {
        None => Ok(None),
        Some(v) => Ok(Some(TableName::new(
            req_string(v, "schemaName")?,
            req_string(v, "tableName")?,
        ))),
    }
}

fn block_from(value: Option<&Json>, ctx: &SerDeContext<'_>) -> Result<Option<Block>> {
    match value {
        None => Ok(None),
        Some(v) => block_deserializer::deserialize_tree(v, ctx).map(Some),
    }
}

fn schema_from(value: Option<&Json>) -> Result<Option<Schema>> {
    match value {
        None => Ok(None),
        Some(v) => schema_deserializer::deserialize_tree(v).map(Some),
    }
}

fn enum_text<'a>(value: &'a Json, enum_name: &str) -> Result<&'a str> {
    match value {
        Json::String(s) => Ok(s.as_str()),
        other => Err(io(format!(
            "Cannot deserialize value of type `{}` from {}",
            enum_name,
            kind(other)
        ))),
    }
}

fn union_type_from(value: &Json) -> Result<ArrowType> {
    let mode = match req_string(value, "mode")?.as_str() {
        "Sparse" => UnionMode::Sparse,
        "Dense" => UnionMode::Dense,
        other => {
            return Err(io(format!(
                "Cannot deserialize value of type `UnionMode` from String \"{other}\""
            )))
        }
    };
    let ids: Vec<i8> = match prop(value, "typeIds") {
        None => Vec::new(),
        Some(v) => list_items(v)
            .into_iter()
            .map(|i| {
                i.as_i64()
                    .and_then(|x| i8::try_from(x).ok())
                    .ok_or_else(|| io("Union type id out of range"))
            })
            .collect::<Result<Vec<i8>>>()?,
    };
    let fields: Vec<Field> = ids
        .iter()
        .map(|i| Field::new(format!("$data${i}"), DataType::Null, true))
        .collect();
    Ok(DataType::Union(
        UnionFields::try_new(ids, fields).map_err(|e| io(e.to_string()))?,
        mode,
    ))
}

fn decimal_type_from(value: &Json) -> Result<ArrowType> {
    let precision = u8::try_from(int_value(value, "precision")?)
        .map_err(|_| io("Decimal precision out of range"))?;
    let scale =
        i8::try_from(int_value(value, "scale")?).map_err(|_| io("Decimal scale out of range"))?;
    let width = match prop(value, "bitWidth") {
        None => 128,
        Some(_) => int_value(value, "bitWidth")?,
    };
    Ok(match width {
        32 => DataType::Decimal32(precision, scale),
        64 => DataType::Decimal64(precision, scale),
        256 => DataType::Decimal256(precision, scale),
        _ => DataType::Decimal128(precision, scale),
    })
}

pub fn arrow_type_from(value: &Json) -> Result<ArrowType> {
    let name = match value.get("name") {
        Some(Json::String(s)) => s.as_str(),
        _ => {
            return Err(io(
                "Could not resolve subtype of org.apache.arrow.vector.types.pojo.ArrowType: missing type id property 'name'",
            ))
        }
    };
    let unit = |field: &str| -> Result<String> { req_string(value, field) };
    let time_unit = |field: &str| -> Result<TimeUnit> {
        match unit(field)?.as_str() {
            "SECOND" => Ok(TimeUnit::Second),
            "MILLISECOND" => Ok(TimeUnit::Millisecond),
            "MICROSECOND" => Ok(TimeUnit::Microsecond),
            "NANOSECOND" => Ok(TimeUnit::Nanosecond),
            other => Err(io(format!(
                "Cannot deserialize value of type `TimeUnit` from String \"{other}\""
            ))),
        }
    };
    Ok(match name {
        "null" => DataType::Null,
        "struct" => DataType::Struct(Fields::empty()),
        "list" => DataType::List(crate::serde::v2::arrow_type_serde::placeholder_child()),
        "largelist" => DataType::LargeList(crate::serde::v2::arrow_type_serde::placeholder_child()),
        "fixedsizelist" => DataType::FixedSizeList(crate::serde::v2::arrow_type_serde::placeholder_child(), int_value(value, "listSize")?),
        "union" => union_type_from(value)?,
        "map" => DataType::Map(
            Arc::new(Field::new("entries", DataType::Struct(Fields::empty()), false)),
            bool_value(value, "keysSorted")?,
        ),
        "int" => match (int_value(value, "bitWidth")?, bool_value(value, "isSigned")?) {
            (8, true) => DataType::Int8,
            (16, true) => DataType::Int16,
            (32, true) => DataType::Int32,
            (64, true) => DataType::Int64,
            (8, false) => DataType::UInt8,
            (16, false) => DataType::UInt16,
            (32, false) => DataType::UInt32,
            (64, false) => DataType::UInt64,
            (w, s) => return Err(io(format!("Unsupported Int bitWidth {w} isSigned {s}"))),
        },
        "floatingpoint" => match req_string(value, "precision")?.as_str() {
            "HALF" => DataType::Float16,
            "SINGLE" => DataType::Float32,
            "DOUBLE" => DataType::Float64,
            other => return Err(io(format!("Cannot deserialize value of type `FloatingPointPrecision` from String \"{other}\""))),
        },
        "utf8" => DataType::Utf8,
        "utf8view" => DataType::Utf8View,
        "largeutf8" => DataType::LargeUtf8,
        "binary" => DataType::Binary,
        "binaryview" => DataType::BinaryView,
        "largebinary" => DataType::LargeBinary,
        "fixedsizebinary" => DataType::FixedSizeBinary(int_value(value, "byteWidth")?),
        "bool" => DataType::Boolean,
        "decimal" => decimal_type_from(value)?,
        "date" => match unit("unit")?.as_str() {
            "DAY" => DataType::Date32,
            "MILLISECOND" => DataType::Date64,
            other => return Err(io(format!("Cannot deserialize value of type `DateUnit` from String \"{other}\""))),
        },
        "time" => {
            let u = time_unit("unit")?;
            match (u, int_value(value, "bitWidth")?) {
                (TimeUnit::Second | TimeUnit::Millisecond, 32) => DataType::Time32(u),
                (TimeUnit::Microsecond | TimeUnit::Nanosecond, 64) => DataType::Time64(u),
                (u, w) => return Err(io(format!("Unsupported Time unit {} with bitWidth {}", time_unit_name(&u), w))),
            }
        }
        "timestamp" => DataType::Timestamp(time_unit("unit")?, opt_string(value, "timezone")?.map(std::convert::Into::into)),
        "interval" => match unit("unit")?.as_str() {
            "YEAR_MONTH" => DataType::Interval(IntervalUnit::YearMonth),
            "DAY_TIME" => DataType::Interval(IntervalUnit::DayTime),
            "MONTH_DAY_NANO" => DataType::Interval(IntervalUnit::MonthDayNano),
            other => return Err(io(format!("Cannot deserialize value of type `IntervalUnit` from String \"{other}\""))),
        },
        "duration" => DataType::Duration(time_unit("unit")?),
        other => {
            return Err(io(format!(
                "Could not resolve type id '{other}' as a subtype of `org.apache.arrow.vector.types.pojo.ArrowType`"
            )))
        }
    })
}

fn arrow_type_prop(node: &Json, name: &str) -> Result<ArrowType> {
    match prop(node, name) {
        None => Err(io(format!("{name} is null"))),
        Some(v) => arrow_type_from(v),
    }
}

pub fn marker_from_tree(node: &Json, ctx: &SerDeContext<'_>) -> Result<Marker> {
    let block =
        block_from(prop(node, "valueBlock"), ctx)?.ok_or_else(|| io("valueBlock is null"))?;
    let bound = match prop(node, "bound") {
        None => return Err(io("bound is null")),
        Some(v) => {
            crate::serde::v2::marker_serde::bound_value_of(Some(enum_text(v, "Marker.Bound")?))
                .map_err(to_io)?
        }
    };
    let null_value = bool_value(node, "nullValue")?;
    Marker::from_value_block(&block, bound, null_value).map_err(to_io)
}

fn value_set_from(node: &Json, ctx: &SerDeContext<'_>) -> Result<ValueSet> {
    let id = type_id(
        node,
        "com.amazonaws.athena.connector.lambda.domain.predicate.ValueSet",
    )?;
    Ok(match id {
        "equatable" => {
            let block = block_from(prop(node, "valueBlock"), ctx)?.ok_or_else(|| io("valueBlock is null"))?;
            EquatableValueSet::new(block, bool_value(node, "whiteList")?, bool_value(node, "nullAllowed")?).into()
        }
        "sortable" => {
            let arrow_type = arrow_type_prop(node, "type")?;
            let mut ranges = Vec::new();
            if let Some(v) = prop(node, "ranges") {
                for item in list_items(v) {
                    let low = marker_from_tree(prop(item, "low").ok_or_else(|| io("low is null"))?, ctx)?;
                    let high = marker_from_tree(prop(item, "high").ok_or_else(|| io("high is null"))?, ctx)?;
                    ranges.push(Range::new(low, high).map_err(to_io)?);
                }
            }
            SortedRangeSet::copy_of(&arrow_type, ranges, bool_value(node, "nullAllowed")?)
                .map_err(to_io)?
                .into()
        }
        "allOrNone" => AllOrNoneValueSet::new(arrow_type_prop(node, "type")?, bool_value(node, "all")?, bool_value(node, "nullAllowed")?).into(),
        other => {
            return Err(io(format!(
                "Could not resolve type id '{other}' as a subtype of `com.amazonaws.athena.connector.lambda.domain.predicate.ValueSet`"
            )))
        }
    })
}

fn expression_from(node: &Json, ctx: &SerDeContext<'_>) -> Result<FederationExpression> {
    let id = type_id(
        node,
        "com.amazonaws.athena.connector.lambda.domain.predicate.expression.FederationExpression",
    )?;
    let children: Vec<&Json> = prop(node, "children").map(list_items).unwrap_or_default();
    Ok(match id {
        "functionCall" => {
            let arrow_type = arrow_type_prop(node, "type")?;
            let function_name = match prop(node, "functionName") {
                None => return Err(io("functionName is null")),
                Some(v) => FunctionName::new(req_string(v, "functionName")?),
            };
            let mut arguments = Vec::new();
            if let Some(v) = prop(node, "arguments") {
                for item in list_items(v) {
                    arguments.push(expression_from(item, ctx)?);
                }
            }
            for child in children {
                arguments.push(expression_from(child, ctx)?);
            }
            FunctionCallExpression::new(arrow_type, function_name, arguments).into()
        }
        "variable" => {
            if !children.is_empty() {
                return Err(io(
                    "java.lang.UnsupportedOperationException (through reference chain: children)",
                ));
            }
            VariableExpression::new(
                req_string(node, "columnName")?,
                arrow_type_prop(node, "type")?,
            )
            .map_err(to_io)?
            .into()
        }
        "constant" => {
            if !children.is_empty() {
                return Err(io(
                    "java.lang.UnsupportedOperationException (through reference chain: children)",
                ));
            }
            ConstantExpression::new(
                block_from(prop(node, "valueBlock"), ctx)?,
                arrow_type_prop(node, "type")?,
            )
            .into()
        }
        other => {
            return Err(io(format!(
                "Could not resolve type id '{other}' as a subtype of `FederationExpression`"
            )))
        }
    })
}

fn direction_from(value: &Json) -> Result<Direction> {
    let text = enum_text(value, "OrderByField.Direction")?;
    Direction::from_name(text).ok_or_else(|| {
        io(format!(
            "Cannot deserialize value of type `OrderByField$Direction` from String \"{text}\": not one of the values accepted for Enum class"
        ))
    })
}

fn constraints_from(value: Option<&Json>, ctx: &SerDeContext<'_>) -> Result<Option<Constraints>> {
    let Some(node) = value else { return Ok(None) };
    let mut summary: IndexMap<String, ValueSet> = IndexMap::new();
    if let Some(Json::Object(map)) = prop(node, "summary") {
        for (k, v) in map {
            if matches!(v, Json::Null) {
                return Err(io(format!("null value in summary entry {k}")));
            }
            summary.insert(k.clone(), value_set_from(v, ctx)?);
        }
    }
    let mut expressions = Vec::new();
    if let Some(v) = prop(node, "expression") {
        for item in list_items(v) {
            expressions.push(expression_from(item, ctx)?);
        }
    }
    let mut order_by = Vec::new();
    if let Some(v) = prop(node, "orderByClause") {
        for item in list_items(v) {
            let direction = match prop(item, "direction") {
                None => return Err(io("direction is null")),
                Some(d) => direction_from(d)?,
            };
            order_by.push(OrderByField::new(
                req_string(item, "columnName")?,
                direction,
            ));
        }
    }
    let query_plan = match prop(node, "queryPlan") {
        None => None,
        Some(p) => Some(QueryPlan::new(
            opt_string(p, "substraitVersion")?,
            req_string(p, "substraitPlan")?,
        )),
    };
    Ok(Some(Constraints::new(
        summary,
        expressions,
        order_by,
        long_value(node, "limit")?,
        nullable_string_map(node, "queryPassthroughArguments")?.unwrap_or_default(),
        query_plan,
    )))
}

fn spill_location_from(value: &Json) -> Result<SpillLocation> {
    let id = type_id(
        value,
        "com.amazonaws.athena.connector.lambda.domain.spill.SpillLocation",
    )?;
    match id {
        "S3SpillLocation" => Ok(SpillLocation::S3(S3SpillLocation::new(
            req_string(value, "bucket")?,
            req_string(value, "key")?,
            bool_value(value, "directory")?,
        ))),
        other => Err(io(format!(
            "Could not resolve type id '{other}' as a subtype of `SpillLocation`"
        ))),
    }
}

fn encryption_key_from(value: Option<&Json>) -> Result<Option<EncryptionKey>> {
    match value {
        None => Ok(None),
        Some(v) => Ok(Some(EncryptionKey::new(
            binary(v, "key")?.unwrap_or_default(),
            binary(v, "nonce")?.unwrap_or_default(),
        ))),
    }
}

fn split_from(value: &Json) -> Result<Split> {
    let spill_location = prop(value, "spillLocation")
        .map(spill_location_from)
        .transpose()?;
    let encryption_key = encryption_key_from(prop(value, "encryptionKey"))?;
    let properties =
        nullable_string_map(value, "properties")?.ok_or_else(|| io("properties is null"))?;
    Ok(Split::new_nullable(
        spill_location,
        encryption_key,
        properties,
    ))
}

fn required<T>(value: Option<T>, name: &str) -> Result<T> {
    value.ok_or_else(|| io(format!("{name} is null")))
}

pub fn deserialize_request(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<FederationRequest> {
    let tree = jparser.read_tree()?;
    request_from_tree(&tree, ctx).map_err(to_io)
}

fn udf_request_from(
    node: &Json,
    identity: Option<FederatedIdentity>,
    ctx: &SerDeContext<'_>,
) -> Result<UserDefinedFunctionRequest> {
    let function_type = function_type_from(node)?;
    Ok(UserDefinedFunctionRequest::new(
        identity,
        required(block_from(prop(node, "inputRecords"), ctx)?, "inputRecords")?,
        required(schema_from(prop(node, "outputSchema"))?, "outputSchema")?,
        req_string(node, "methodName")?,
        function_type,
    ))
}

fn function_type_from(node: &Json) -> Result<UserDefinedFunctionType> {
    let v = prop(node, "functionType").ok_or_else(|| io("functionType is null"))?;
    UserDefinedFunctionType::from_name(enum_text(v, "UserDefinedFunctionType")?)
        .ok_or_else(|| io("Cannot deserialize value of type `UserDefinedFunctionType`"))
}

pub fn request_from_tree(node: &Json, ctx: &SerDeContext<'_>) -> Result<FederationRequest> {
    let id = type_id(
        node,
        "com.amazonaws.athena.connector.lambda.request.FederationRequest",
    )?;
    if !REQUEST_SUB_TYPES.contains(&id) {
        return Err(io(format!(
            "Could not resolve type id '{id}' as a subtype of `com.amazonaws.athena.connector.lambda.request.FederationRequest`"
        )));
    }
    let identity = identity_from(node)?;
    let query_id = opt_string(node, "queryId")?;
    let catalog_name = opt_string(node, "catalogName")?;
    Ok(match id {
        "PingRequest" => PingRequest::new(
            identity,
            required(catalog_name, "catalogName")?,
            required(query_id, "queryId")?,
        )
        .into(),
        "ListSchemasRequest" => {
            ListSchemasRequest::new(identity, query_id, required(catalog_name, "catalogName")?)
                .into()
        }
        "ListTablesRequest" => ListTablesRequest::new(
            identity,
            query_id,
            required(catalog_name, "catalogName")?,
            opt_string(node, "schemaName")?,
            opt_string(node, "nextToken")?,
            int_value(node, "pageSize")?,
        )
        .into(),
        "GetTableRequest" => GetTableRequest::new(
            identity,
            query_id,
            required(catalog_name, "catalogName")?,
            required(table_name_from(prop(node, "tableName"))?, "tableName")?,
            nullable_string_map(node, "queryPassthroughArguments")?
                .ok_or_else(|| io("queryPassthroughArguments is null"))?,
        )
        .into(),
        "GetTableLayoutRequest" => {
            let partition_cols =
                string_list(node, "partitionCols")?.ok_or_else(|| io("partitionCols is null"))?;
            GetTableLayoutRequest::new(
                identity,
                query_id,
                required(catalog_name, "catalogName")?,
                required(table_name_from(prop(node, "tableName"))?, "tableName")?,
                required(
                    constraints_from(prop(node, "constraints"), ctx)?,
                    "constraints",
                )?,
                required(schema_from(prop(node, "schema"))?, "schema")?,
                partition_cols,
            )
            .into()
        }
        "GetSplitsRequest" => GetSplitsRequest::new(
            identity,
            query_id,
            required(catalog_name, "catalogName")?,
            required(table_name_from(prop(node, "tableName"))?, "tableName")?,
            required(block_from(prop(node, "partitions"), ctx)?, "partitions")?,
            string_list(node, "partitionCols")?.ok_or_else(|| io("partitionCols is null"))?,
            required(
                constraints_from(prop(node, "constraints"), ctx)?,
                "constraints",
            )?,
            opt_string(node, "continuationToken")?,
        )
        .into(),
        "ReadRecordsRequest" => ReadRecordsRequest::new(
            identity,
            required(catalog_name, "catalogName")?,
            required(query_id, "queryId")?,
            required(table_name_from(prop(node, "tableName"))?, "tableName")?,
            required(schema_from(prop(node, "schema"))?, "schema")?,
            required(prop(node, "split").map(split_from).transpose()?, "split")?,
            required(
                constraints_from(prop(node, "constraints"), ctx)?,
                "constraints",
            )?,
            long_value(node, "maxBlockSize")?,
            long_value(node, "maxInlineBlockSize")?,
        )
        .into(),
        "UserDefinedFunctionRequest" => udf_request_from(node, identity, ctx)?.into(),
        "GetDataSourceCapabilitiesRequest" => GetDataSourceCapabilitiesRequest::new(
            identity,
            query_id,
            required(catalog_name, "catalogName")?,
        )
        .into(),
        other => return Err(io(format!("Could not resolve type id '{other}'"))),
    })
}

pub fn deserialize_response(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<FederationResponse> {
    let tree = jparser.read_tree()?;
    response_from_tree(&tree, ctx).map_err(to_io)
}

pub fn response_from_tree(node: &Json, ctx: &SerDeContext<'_>) -> Result<FederationResponse> {
    let id = type_id(
        node,
        "com.amazonaws.athena.connector.lambda.request.FederationResponse",
    )?;
    if !RESPONSE_SUB_TYPES.contains(&id) {
        return Err(io(format!(
            "Could not resolve type id '{id}' as a subtype of `com.amazonaws.athena.connector.lambda.request.FederationResponse`: known type ids = [GetSplitsResponse, GetTableLayoutResponse, GetTableResponse, ListSchemasResponse, ListTablesResponse, PingResponse, ReadRecordsResponse, RemoteReadRecordsResponse, UserDefinedFunctionResponse]"
        )));
    }
    let catalog_name = opt_string(node, "catalogName")?;
    Ok(match id {
        "PingResponse" => PingResponse::new(
            required(catalog_name, "catalogName")?,
            req_string(node, "queryId")?,
            opt_string(node, "sourceType")?,
            int_value(node, "capabilities")?,
            int_value(node, "serDeVersion")?,
        )
        .into(),
        "ListSchemasResponse" => ListSchemasResponse::new(
            required(catalog_name, "catalogName")?,
            string_list(node, "schemas")?.ok_or_else(|| io("schemas is null"))?,
        )
        .into(),
        "ListTablesResponse" => {
            let tables = required_list(node, "tables", |t| {
                table_name_from(Some(t)).and_then(|o| required(o, "table"))
            })?;
            ListTablesResponse::new(
                required(catalog_name, "catalogName")?,
                tables,
                opt_string(node, "nextToken")?,
            )
            .into()
        }
        "GetTableResponse" => {
            let cols: IndexSet<String> = string_list(node, "partitionColumns")?
                .ok_or_else(|| io("partitionColumns is null"))?
                .into_iter()
                .collect();
            GetTableResponse::new(
                required(catalog_name, "catalogName")?,
                required(table_name_from(prop(node, "tableName"))?, "tableName")?,
                required(schema_from(prop(node, "schema"))?, "schema")?,
                cols,
            )
            .into()
        }
        "GetTableLayoutResponse" => GetTableLayoutResponse::new(
            required(catalog_name, "catalogName")?,
            required(table_name_from(prop(node, "tableName"))?, "tableName")?,
            required(block_from(prop(node, "partitions"), ctx)?, "partitions")?,
        )
        .into(),
        "GetSplitsResponse" => {
            let splits = required_list(node, "splits", split_from)?;
            GetSplitsResponse::new(
                required(catalog_name, "catalogName")?,
                splits,
                opt_string(node, "continuationToken")?,
            )
            .into()
        }
        "ReadRecordsResponse" => ReadRecordsResponse::new(
            required(catalog_name, "catalogName")?,
            required(block_from(prop(node, "records"), ctx)?, "records")?,
        )
        .into(),
        "RemoteReadRecordsResponse" => {
            let blocks = required_list(node, "remoteBlocks", spill_location_from)?;
            RemoteReadRecordsResponse::new(
                required(catalog_name, "catalogName")?,
                required(schema_from(prop(node, "schema"))?, "schema")?,
                blocks,
                encryption_key_from(prop(node, "encryptionKey"))?,
            )
            .into()
        }
        "UserDefinedFunctionResponse" => UserDefinedFunctionResponse::new(
            required(block_from(prop(node, "records"), ctx)?, "records")?,
            req_string(node, "methodName")?,
        )
        .into(),
        other => return Err(io(format!("Could not resolve type id '{other}'"))),
    })
}

pub fn capabilities_from_tree(node: &Json) -> Result<GetDataSourceCapabilitiesResponse> {
    let mut capabilities: IndexMap<String, Vec<OptimizationSubType>> = IndexMap::new();
    if let Some(Json::Object(map)) = prop(node, "capabilities") {
        for (k, v) in map {
            let mut items = Vec::new();
            for item in list_items(v) {
                items.push(OptimizationSubType::new(
                    req_string(item, "subType")?,
                    string_list(item, "properties")?.unwrap_or_default(),
                ));
            }
            capabilities.insert(k.clone(), items);
        }
    }
    Ok(GetDataSourceCapabilitiesResponse::new(
        req_string(node, "catalogName")?,
        capabilities,
    ))
}

pub fn deserialize_block_tree(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<Block> {
    let tree = jparser.read_tree()?;
    block_deserializer::deserialize_tree(&tree, ctx)
}

pub fn deserialize_schema_tree(jparser: &mut JsonParser<'_>) -> Result<Schema> {
    let tree = jparser.read_tree()?;
    schema_deserializer::deserialize_tree(&tree)
}
