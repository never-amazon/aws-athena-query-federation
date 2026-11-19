use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use indexmap::IndexMap;
use prost::Message;

use crate::calcite::rel::RelNode;
use crate::calcite::rel_data_type::{RelDataType, RelDataTypeBuilder};
use crate::calcite::rel_to_sql::RelToSqlConverter;
use crate::calcite::rex::RexNode;
use crate::calcite::sql_dialect::SqlDialect;
use crate::calcite::sql_node::SqlNode;
use crate::error::{Result, SubstraitError};
use crate::isthmus::SubstraitToCalcite;
use crate::proto::plan_rel::RelType as PlanRelType;
use crate::proto::r#type::Kind;
use crate::proto::rel::RelType;
use crate::proto::{FetchRel, FilterRel, Plan, ProjectRel, ReadRel, Rel, SortRel, Type};

pub const PLAN_PARSER_RECURSION_LIMIT: usize = 2000;

const TOO_DEEP_MESSAGE: &str = "Protocol message had too many levels of nesting.  May be malicious.  Use CodedInputStream.setRecursionLimit() to increase the depth limit.";

const JAVA_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

fn find_rel<T>(rel: &Rel, extract: fn(&RelType) -> Option<&T>) -> Option<&T> {
    let mut current = rel;
    loop {
        let rel_type = current.rel_type.as_ref()?;
        if let Some(found) = extract(rel_type) {
            return Some(found);
        }
        current = match rel_type {
            RelType::Filter(f) => f.input.as_deref(),
            RelType::Fetch(f) => f.input.as_deref(),
            RelType::Sort(s) => s.input.as_deref(),
            RelType::Aggregate(a) => a.input.as_deref(),
            RelType::Project(p) => p.input.as_deref(),
            _ => None,
        }?;
    }
}

pub fn filter_rel(rel: &Rel) -> Option<&FilterRel> {
    find_rel(rel, |t| match t {
        RelType::Filter(f) => Some(f.as_ref()),
        _ => None,
    })
}

pub fn fetch_rel(rel: &Rel) -> Option<&FetchRel> {
    find_rel(rel, |t| match t {
        RelType::Fetch(f) => Some(f.as_ref()),
        _ => None,
    })
}

pub fn sort_rel(rel: &Rel) -> Option<&SortRel> {
    find_rel(rel, |t| match t {
        RelType::Sort(s) => Some(s.as_ref()),
        _ => None,
    })
}

pub fn read_rel(rel: &Rel) -> Option<&ReadRel> {
    find_rel(rel, |t| match t {
        RelType::Read(r) => Some(r.as_ref()),
        _ => None,
    })
}

pub fn project_rel(rel: &Rel) -> Option<&ProjectRel> {
    find_rel(rel, |t| match t {
        RelType::Project(p) => Some(p.as_ref()),
        _ => None,
    })
}

pub fn decode(plan_string: &str) -> Result<Plan> {
    let wrap =
        |cause: SubstraitError| SubstraitError::wrap("Failed to parse Substrait plan", cause);
    let bytes = JAVA_BASE64
        .decode(plan_string)
        .map_err(|e| wrap(e.into()))?;
    if let Err(DepthCheck::TooDeep) = check_message_depth(&bytes, 0) {
        return Err(wrap(SubstraitError::Runtime(TOO_DEEP_MESSAGE.into())));
    }
    Plan::decode(bytes.as_slice()).map_err(|e| wrap(e.into()))
}

pub fn table_columns(read_rel: &ReadRel) -> Result<Vec<String>> {
    let Some(base_schema) = &read_rel.base_schema else {
        return Ok(Vec::new());
    };
    let types = base_schema
        .r#struct
        .as_ref()
        .map_or(&[][..], |s| s.types.as_slice());
    let mut columns = Vec::new();
    collect_column_names(&base_schema.names, types, Some(&mut columns), 0)?;
    Ok(columns)
}

fn collect_column_names(
    names: &[String],
    types: &[Type],
    mut columns: Option<&mut Vec<String>>,
    mut name_idx: usize,
) -> Result<usize> {
    for t in types {
        let name = names.get(name_idx).ok_or_else(|| {
            SubstraitError::IndexOutOfBounds(format!(
                "Index {} out of bounds for length {}",
                name_idx,
                names.len()
            ))
        })?;
        if let Some(columns) = columns.as_deref_mut() {
            columns.push(name.clone());
        }
        name_idx = match &t.kind {
            Some(Kind::Struct(s)) => collect_column_names(names, &s.types, None, name_idx + 1)?,
            _ => name_idx + 1,
        };
    }
    Ok(name_idx)
}

pub fn to_sql_node(plan_string: &str, sql_dialect: &SqlDialect) -> Result<SqlNode> {
    let attempt = || -> Result<SqlNode> {
        let plan = decode(plan_string)?;
        let node = rel_node(&plan, sql_dialect)?;
        let mut converter = RelToSqlConverter::new(*sql_dialect);
        let result = converter.visit_root(&node)?;
        Ok(result.to_statement(sql_dialect))
    };
    attempt().map_err(|e| {
        tracing::error!("Failed to parse Substrait plan: {}", e);
        SubstraitError::wrap("Failed to parse Substrait plan", e)
    })
}

pub fn table_schema(plan_string: &str, sql_dialect: &SqlDialect) -> Result<RelDataType> {
    let attempt = || -> Result<RelDataType> {
        let plan = decode(plan_string)?;
        let first = plan.relations.first().ok_or_else(|| {
            SubstraitError::IndexOutOfBounds("Index 0 out of bounds for length 0".into())
        })?;
        let read = match &first.rel_type {
            Some(PlanRelType::Root(root)) => root.input.as_ref().and_then(read_rel),
            _ => None,
        };
        if read.is_none_or(|r| r.base_schema.is_none()) {
            return Err(SubstraitError::Runtime(
                "Unable to extract base table schema from Substrait plan".into(),
            ));
        }
        let rel_node = rel_node(&plan, sql_dialect)?;
        let mut builder = RelDataTypeBuilder::new();
        traverse(&rel_node, &mut builder, &mut IndexMap::new());
        Ok(builder.build())
    };
    attempt().map_err(|e| {
        tracing::error!("Failed to extract table schema from Substrait plan: {}", e);
        SubstraitError::wrap("Failed to extract table schema from Substrait plan", e)
    })
}

pub fn column_remapping(
    plan_string: &str,
    sql_dialect: &SqlDialect,
) -> Result<IndexMap<String, Option<String>>> {
    let plan = decode(plan_string)?;
    let mut column_remapping = IndexMap::new();
    let rel_node = rel_node(&plan, sql_dialect)?;
    let mut builder = RelDataTypeBuilder::new();
    traverse(&rel_node, &mut builder, &mut column_remapping);
    tracing::debug!(
        "Column rename mapping (original to renamed): {:?}",
        column_remapping
    );
    Ok(column_remapping)
}

fn rel_node(plan: &Plan, sql_dialect: &SqlDialect) -> Result<RelNode> {
    SubstraitToCalcite::convert_plan(plan, sql_dialect).map_err(|e| {
        tracing::error!("Failed to convert from Substrait plan to RelNode: {}", e);
        SubstraitError::wrap("Failed to convert from Substrait plan to RelNode", e)
    })
}

fn traverse(
    node: &RelNode,
    builder: &mut RelDataTypeBuilder,
    rename_mapping: &mut IndexMap<String, Option<String>>,
) {
    match node {
        RelNode::TableScan { .. } | RelNode::Values { .. } => {
            builder.add_all(node.row_type().field_list());
        }
        RelNode::Project {
            input,
            exprs,
            row_type,
        } => {
            let input_fields = input.row_type().field_list();
            let output_fields = row_type.field_list();
            builder.add_all(output_fields);
            for (i, rex) in exprs.iter().enumerate() {
                let output_name = output_fields[i].name().to_string();
                match rex {
                    RexNode::InputRef { index, .. } => {
                        let original_name = input_fields[*index].name();
                        if output_name != original_name {
                            rename_mapping.insert(output_name, Some(original_name.to_string()));
                        }
                    }
                    _ => {
                        rename_mapping.insert(output_name, None);
                    }
                }
            }
        }
        RelNode::Aggregate {
            input,
            group_set,
            row_type,
            ..
        } => {
            let input_fields = input.row_type().field_list();
            let output_fields = row_type.field_list();
            builder.add_all(output_fields);
            let mut output_idx = 0usize;
            let mut sorted = group_set.clone();
            sorted.sort_unstable();
            for group_index in sorted {
                let output_name = output_fields[output_idx].name().to_string();
                let original_name = input_fields[group_index].name();
                if output_name != original_name {
                    rename_mapping.insert(output_name, Some(original_name.to_string()));
                }
                output_idx += 1;
            }
            while output_idx < output_fields.len() {
                rename_mapping.insert(output_fields[output_idx].name().to_string(), None);
                output_idx += 1;
            }
        }
        RelNode::Filter { .. }
        | RelNode::Sort { .. }
        | RelNode::Join { .. }
        | RelNode::SetOp { .. } => {}
    }
    for input in node.inputs() {
        traverse(input, builder, rename_mapping);
    }
    let keys: Vec<String> = rename_mapping.keys().cloned().collect();
    for key in keys {
        let intermediate = rename_mapping.get(&key).cloned().flatten();
        if let Some(intermediate) = intermediate {
            if let Some(target) = rename_mapping.get(&intermediate).cloned() {
                rename_mapping.insert(key, target);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DepthCheck {
    TooDeep,
    Malformed,
}

fn read_varint(buf: &[u8], pos: &mut usize) -> std::result::Result<u64, DepthCheck> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos).ok_or(DepthCheck::Malformed)?;
        *pos += 1;
        if shift >= 64 {
            return Err(DepthCheck::Malformed);
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

fn check_message_depth(buf: &[u8], depth: usize) -> std::result::Result<(), DepthCheck> {
    let mut pos = 0usize;
    check_fields(buf, &mut pos, depth, None)
}

fn check_fields(
    buf: &[u8],
    pos: &mut usize,
    depth: usize,
    end_group: Option<u64>,
) -> std::result::Result<(), DepthCheck> {
    if depth > PLAN_PARSER_RECURSION_LIMIT {
        return Err(DepthCheck::TooDeep);
    }
    while *pos < buf.len() {
        let key = read_varint(buf, pos)?;
        let field = key >> 3;
        if field == 0 {
            return Err(DepthCheck::Malformed);
        }
        match key & 7 {
            0 => {
                read_varint(buf, pos)?;
            }
            1 => {
                *pos = pos
                    .checked_add(8)
                    .filter(|p| *p <= buf.len())
                    .ok_or(DepthCheck::Malformed)?;
            }
            2 => {
                let len =
                    usize::try_from(read_varint(buf, pos)?).map_err(|_| DepthCheck::Malformed)?;
                let end = pos
                    .checked_add(len)
                    .filter(|p| *p <= buf.len())
                    .ok_or(DepthCheck::Malformed)?;
                let sub = &buf[*pos..end];
                if let Err(DepthCheck::TooDeep) = check_message_depth(sub, depth + 1) {
                    return Err(DepthCheck::TooDeep);
                }
                *pos = end;
            }
            3 => {
                check_fields(buf, pos, depth + 1, Some(field))?;
            }
            4 => {
                return if end_group == Some(field) {
                    Ok(())
                } else {
                    Err(DepthCheck::Malformed)
                };
            }
            5 => {
                *pos = pos
                    .checked_add(4)
                    .filter(|p| *p <= buf.len())
                    .ok_or(DepthCheck::Malformed)?;
            }
            _ => return Err(DepthCheck::Malformed),
        }
    }
    if end_group.is_some() {
        return Err(DepthCheck::Malformed);
    }
    Ok(())
}
