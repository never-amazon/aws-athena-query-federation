use athena_federation_sdk::domain::predicate::Constraints;

use crate::error::{Result, SubstraitError};
use crate::java_cast::long_to_int;
use crate::model::substrait_rel_model::SubstraitRelModel;
use crate::proto::expression::field_reference::ReferenceType;
use crate::proto::expression::reference_segment;
use crate::proto::expression::RexType;
use crate::proto::fetch_rel::CountMode;
use crate::proto::plan_rel::RelType;
use crate::proto::sort_field::{SortDirection, SortKind};
use crate::proto::{Expression, Plan, Rel, SortRel};
use crate::substrait_metadata_parser::SubstraitMetadataParser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericSortField {
    column_name: String,
    ascending: bool,
}

impl GenericSortField {
    pub fn new(column_name: impl Into<String>, ascending: bool) -> GenericSortField {
        GenericSortField {
            column_name: column_name.into(),
            ascending,
        }
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    pub fn is_ascending(&self) -> bool {
        self.ascending
    }
}

pub struct LimitAndSortHelper;

fn root_input_model(plan: &Plan) -> Result<SubstraitRelModel> {
    let plan_rel = plan
        .relations
        .first()
        .ok_or_else(|| SubstraitError::index_out_of_bounds("Index 0 out of bounds for length 0"))?;
    let default_rel = Rel::default();
    let input = match &plan_rel.rel_type {
        Some(RelType::Root(root)) => root.input.as_ref().unwrap_or(&default_rel),
        _ => &default_rel,
    };
    SubstraitRelModel::build_substrait_rel_model(Some(input))
}

impl LimitAndSortHelper {
    pub fn get_limit(plan: Option<&Plan>, constraints: &Constraints) -> Result<Option<i32>> {
        match plan {
            Some(plan) => {
                let model = root_input_model(plan)?;
                if model.fetch_rel().is_some() && fetch_limit(&model) > 0 {
                    Ok(Some(fetch_limit(&model)))
                } else {
                    Ok(None)
                }
            }
            None => {
                if constraints.has_limit() {
                    Ok(Some(long_to_int(constraints.limit())))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub fn get_sort_from_plan(plan: Option<&Plan>) -> Option<Vec<GenericSortField>> {
        let plan = plan?;
        if plan.relations.is_empty() {
            return None;
        }
        let result = (|| -> Result<Option<Vec<GenericSortField>>> {
            let model = root_input_model(plan)?;
            let Some(sort_rel) = model.sort_rel() else {
                return Ok(None);
            };
            let table_columns = SubstraitMetadataParser::get_table_columns(&model)?;
            Ok(Some(extract_generic_sort_fields(sort_rel, &table_columns)))
        })();
        match result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to extract sort from plan{}", e);
                None
            }
        }
    }
}

fn fetch_limit(model: &SubstraitRelModel) -> i32 {
    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    match model.fetch_rel().and_then(|f| f.count_mode.as_ref()) {
        Some(CountMode::Count(c)) => long_to_int(*c),
        _ => 0,
    }
}

fn extract_generic_sort_fields(
    sort_rel: &SortRel,
    table_columns: &[String],
) -> Vec<GenericSortField> {
    let mut sort_fields = Vec::new();
    for sort_field in &sort_rel.sorts {
        let default_expr = Expression::default();
        let expr = sort_field.expr.as_ref().unwrap_or(&default_expr);
        match extract_field_index_from_expression(expr) {
            Ok(field_index) => {
                if let Some(column) = usize::try_from(field_index)
                    .ok()
                    .and_then(|i| table_columns.get(i))
                {
                    let direction = match sort_field.sort_kind {
                        Some(SortKind::Direction(d)) => {
                            SortDirection::try_from(d).unwrap_or(SortDirection::Unspecified)
                        }
                        _ => SortDirection::Unspecified,
                    };
                    let ascending = direction == SortDirection::AscNullsFirst
                        || direction == SortDirection::AscNullsLast;
                    sort_fields.push(GenericSortField::new(column.clone(), ascending));
                }
            }
            Err(e) => {
                tracing::warn!("Failed to extract sort field, skipping: {}", e.message());
            }
        }
    }
    sort_fields
}

fn extract_field_index_from_expression(expression: &Expression) -> Result<i32> {
    if let Some(RexType::Selection(sel)) = &expression.rex_type {
        if let Some(ReferenceType::DirectReference(segment)) = &sel.reference_type {
            if let Some(reference_segment::ReferenceType::StructField(sf)) = &segment.reference_type
            {
                return Ok(sf.field);
            }
        }
    }
    Err(SubstraitError::illegal_argument(
        "Cannot extract field index from expression",
    ))
}
