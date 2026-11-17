use crate::error::{Result, SubstraitError};
use crate::model::substrait_field::SubstraitField;
use crate::model::substrait_rel_model::SubstraitRelModel;
use crate::proto::r#type::Kind;
use crate::proto::{NamedStruct, Type};

pub struct SubstraitMetadataParser;

impl SubstraitMetadataParser {
    pub fn get_table_columns(substrait_rel_model: &SubstraitRelModel) -> Result<Vec<String>> {
        let read_rel = substrait_rel_model
            .read_rel()
            .ok_or_else(|| SubstraitError::null_pointer("readRel is null"))?;
        let default_schema = NamedStruct::default();
        let base_schema = read_rel.base_schema.as_ref().unwrap_or(&default_schema);
        let types = base_schema
            .r#struct
            .as_ref()
            .map_or(&[][..], |s| s.types.as_slice());
        let mut connector_schema = Vec::new();
        parse_schema_fields(&base_schema.names, types, &mut connector_schema, 0)?;
        Ok(connector_schema
            .iter()
            .map(|f| f.name().to_string())
            .collect())
    }
}

fn parse_schema_fields(
    names: &[String],
    types: &[Type],
    columns: &mut Vec<SubstraitField>,
    mut name_idx: usize,
) -> Result<usize> {
    for t in types {
        let name = names.get(name_idx).ok_or_else(|| {
            SubstraitError::index_out_of_bounds(format!(
                "Index {} out of bounds for length {}",
                name_idx,
                names.len()
            ))
        })?;
        if let Some(Kind::Struct(s)) = &t.kind {
            let mut children = Vec::new();
            name_idx = parse_schema_fields(names, &s.types, &mut children, name_idx + 1)?;
            columns.push(SubstraitField::new(name.clone(), "struct", Some(children)));
        } else {
            columns.push(SubstraitField::new(
                name.clone(),
                convert_substrait_type_to_athena(t),
                None,
            ));
            name_idx += 1;
        }
    }
    Ok(name_idx)
}

pub fn type_kind_case_name(t: &Type) -> &'static str {
    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    match &t.kind {
        None => "KIND_NOT_SET",
        Some(k) => match k {
            Kind::Bool(_) => "BOOL",
            Kind::I8(_) => "I8",
            Kind::I16(_) => "I16",
            Kind::I32(_) => "I32",
            Kind::I64(_) => "I64",
            Kind::Fp32(_) => "FP32",
            Kind::Fp64(_) => "FP64",
            Kind::String(_) => "STRING",
            Kind::Binary(_) => "BINARY",
            Kind::Timestamp(_) => "TIMESTAMP",
            Kind::Date(_) => "DATE",
            Kind::Time(_) => "TIME",
            Kind::IntervalYear(_) => "INTERVAL_YEAR",
            Kind::IntervalDay(_) => "INTERVAL_DAY",
            Kind::IntervalCompound(_) => "INTERVAL_COMPOUND",
            Kind::TimestampTz(_) => "TIMESTAMP_TZ",
            Kind::Uuid(_) => "UUID",
            Kind::FixedChar(_) => "FIXED_CHAR",
            Kind::Varchar(_) => "VARCHAR",
            Kind::FixedBinary(_) => "FIXED_BINARY",
            Kind::Decimal(_) => "DECIMAL",
            Kind::PrecisionTime(_) => "PRECISION_TIME",
            Kind::PrecisionTimestamp(_) => "PRECISION_TIMESTAMP",
            Kind::PrecisionTimestampTz(_) => "PRECISION_TIMESTAMP_TZ",
            Kind::Struct(_) => "STRUCT",
            Kind::List(_) => "LIST",
            Kind::Map(_) => "MAP",
            Kind::UserDefined(_) => "USER_DEFINED",
            Kind::UserDefinedTypeReference(_) => "USER_DEFINED_TYPE_REFERENCE",
        },
    }
}

fn convert_substrait_type_to_athena(t: &Type) -> &'static str {
    type_kind_case_name(t)
}
