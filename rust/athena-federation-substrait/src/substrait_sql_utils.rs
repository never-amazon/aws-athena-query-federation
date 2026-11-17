use indexmap::IndexMap;

use crate::calcite::rel::RelNode;
use crate::calcite::rel_data_type::{RelDataType, RelDataTypeBuilder};
use crate::calcite::rel_to_sql::RelToSqlConverter;
use crate::calcite::rex::RexNode;
use crate::calcite::sql_dialect::SqlDialect;
use crate::calcite::sql_node::SqlNode;
use crate::error::{Result, SubstraitError};
use crate::isthmus::SubstraitToCalcite;
use crate::proto::plan_rel::RelType as PlanRelType;
use crate::proto::Plan;
use crate::substrait_rel_utils::SubstraitRelUtils;

pub struct SubstraitSqlUtils;

impl SubstraitSqlUtils {
    pub fn get_sql_node_from_substrait_plan(plan_string: &str, sql_dialect: &SqlDialect) -> Result<SqlNode> {
        let attempt = || -> Result<SqlNode> {
            let plan = SubstraitRelUtils::deserialize_substrait_plan(plan_string)?;
            Self::get_sql_node_from_plan(&plan, sql_dialect)
        };
        attempt().map_err(|e| {
            tracing::error!("Failed to parse Substrait plan: {}", e);
            SubstraitError::wrap("Failed to parse Substrait plan", e)
        })
    }

    fn get_sql_node_from_plan(plan: &Plan, sql_dialect: &SqlDialect) -> Result<SqlNode> {
        let node = Self::get_rel_node_from_substrait_plan(plan, sql_dialect)?;
        let mut converter = RelToSqlConverter::new(sql_dialect.clone());
        let result = converter.visit_root(&node)?;
        Ok(result.as_statement(sql_dialect))
    }

    pub fn get_table_schema_from_substrait_plan(plan_string: &str, sql_dialect: &SqlDialect) -> Result<RelDataType> {
        let attempt = || -> Result<RelDataType> {
            let plan = SubstraitRelUtils::deserialize_substrait_plan(plan_string)?;
            Self::get_table_schema_from_plan(&plan, sql_dialect)
        };
        attempt().map_err(|e| {
            tracing::error!("Failed to extract table schema from Substrait plan: {}", e);
            SubstraitError::wrap("Failed to extract table schema from Substrait plan", e)
        })
    }

    pub fn get_column_remapping(
        plan_string: &str,
        sql_dialect: &SqlDialect,
    ) -> Result<IndexMap<String, Option<String>>> {
        let plan = SubstraitRelUtils::deserialize_substrait_plan(plan_string)?;
        let mut column_remapping = IndexMap::new();
        let rel_node = Self::get_rel_node_from_substrait_plan(&plan, sql_dialect)?;
        let mut builder = RelDataTypeBuilder::new();
        traverse(&rel_node, &mut builder, &mut column_remapping);
        tracing::debug!("Column rename mapping (original to renamed): {:?}", column_remapping);
        Ok(column_remapping)
    }

    fn get_table_schema_from_plan(plan: &Plan, sql_dialect: &SqlDialect) -> Result<RelDataType> {
        let first = plan
            .relations
            .first()
            .ok_or_else(|| SubstraitError::index_out_of_bounds("Index 0 out of bounds for length 0"))?;
        let rel = match &first.rel_type {
            Some(PlanRelType::Root(root)) => root.input.as_ref(),
            _ => None,
        };
        let read_rel = SubstraitRelUtils::get_read_rel(rel);
        if read_rel.is_none_or(|r| r.base_schema.is_none()) {
            return Err(SubstraitError::runtime(
                "Unable to extract base table schema from Substrait plan",
            ));
        }
        let rel_node = Self::get_rel_node_from_substrait_plan(plan, sql_dialect)?;
        let mut builder = RelDataTypeBuilder::new();
        traverse(&rel_node, &mut builder, &mut IndexMap::new());
        Ok(builder.build())
    }

    fn get_rel_node_from_substrait_plan(plan: &Plan, sql_dialect: &SqlDialect) -> Result<RelNode> {
        SubstraitToCalcite::convert_plan(plan, sql_dialect).map_err(|e| {
            tracing::error!("Failed to convert from Substrait plan to RelNode: {}", e);
            SubstraitError::wrap("Failed to convert from Substrait plan to RelNode", e)
        })
    }
}

fn traverse(node: &RelNode, builder: &mut RelDataTypeBuilder, rename_mapping: &mut IndexMap<String, Option<String>>) {
    match node {
        RelNode::TableScan { .. } | RelNode::Values { .. } => {
            builder.add_all(node.row_type().get_field_list());
        }
        RelNode::Project {
            input,
            exprs,
            row_type,
        } => {
            let input_fields = input.row_type().get_field_list();
            let output_fields = row_type.get_field_list();
            builder.add_all(output_fields);
            for (i, rex) in exprs.iter().enumerate() {
                let output_name = output_fields[i].get_name().to_string();
                match rex {
                    RexNode::InputRef { index, .. } => {
                        let original_name = input_fields[*index].get_name();
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
            let input_fields = input.row_type().get_field_list();
            let output_fields = row_type.get_field_list();
            builder.add_all(output_fields);
            let mut output_idx = 0usize;
            let mut sorted = group_set.clone();
            sorted.sort_unstable();
            for group_index in sorted {
                let output_name = output_fields[output_idx].get_name().to_string();
                let original_name = input_fields[group_index].get_name();
                if output_name != original_name {
                    rename_mapping.insert(output_name, Some(original_name.to_string()));
                }
                output_idx += 1;
            }
            while output_idx < output_fields.len() {
                rename_mapping.insert(output_fields[output_idx].get_name().to_string(), None);
                output_idx += 1;
            }
        }
        RelNode::Filter { .. } | RelNode::Sort { .. } => {}
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
