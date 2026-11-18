use std::collections::HashMap;

use arrow_schema::DataType;

use crate::error::{Result, SubstraitError};
use crate::literal::{self, LiteralValue};
use crate::model::column_predicate::{ColumnPredicate, PredicateValue};
use crate::model::logical_expression::LogicalExpression;
use crate::model::substrait_operator::SubstraitOperator;
use crate::proto::expression::field_reference::ReferenceType;
use crate::proto::expression::reference_segment;
use crate::proto::expression::RexType;
use crate::proto::extensions::simple_extension_declaration::MappingType;
use crate::proto::extensions::SimpleExtensionDeclaration;
use crate::proto::function_argument::ArgType;
use crate::proto::{Expression, FunctionArgument};

pub mod function_names {
    pub const NOT_BOOL: &str = "not:bool";
    pub const AND_BOOL: &str = "and:bool";
    pub const OR_BOOL: &str = "or:bool";
    pub const GT_ANY_ANY: &str = "gt:any_any";
    pub const GT_PTS_PTS: &str = "gt:pts_pts";
    pub const GTE_ANY_ANY: &str = "gte:any_any";
    pub const GTE_PTS_PTS: &str = "gte:pts_pts";
    pub const LT_ANY_ANY: &str = "lt:any_any";
    pub const LT_PTS_PTS: &str = "lt:pts_pts";
    pub const LTE_ANY_ANY: &str = "lte:any_any";
    pub const LTE_PTS_PTS: &str = "lte:pts_pts";
    pub const EQUAL_ANY_ANY: &str = "equal:any_any";
    pub const NOT_EQUAL_ANY_ANY: &str = "not_equal:any_any";
    pub const IS_NULL_ANY: &str = "is_null:any";
    pub const IS_NOT_NULL_ANY: &str = "is_not_null:any";
}

use function_names as N;

struct ScalarFunctionInfo<'a> {
    function_name: Option<String>,
    arguments: &'a [FunctionArgument],
}

pub fn column_predicates_map(
    extension_declaration_list: &[SimpleExtensionDeclaration],
    expression: &Expression,
    column_names: &[String],
) -> Result<HashMap<Option<String>, Vec<ColumnPredicate>>> {
    let predicates = column_predicates(extension_declaration_list, expression, column_names)?;
    let mut map: HashMap<Option<String>, Vec<ColumnPredicate>> = HashMap::new();
    for p in predicates {
        map.entry(p.column().map(str::to_string))
            .or_default()
            .push(p);
    }
    Ok(map)
}

pub fn column_predicates(
    extension_declaration_list: &[SimpleExtensionDeclaration],
    expression: &Expression,
    column_names: &[String],
) -> Result<Vec<ColumnPredicate>> {
    let mut predicates = Vec::new();
    let Some(info) = extract_scalar_function_info(expression, extension_declaration_list) else {
        return Ok(predicates);
    };
    let name = info.function_name.as_deref();
    if name == Some(N::NOT_BOOL) {
        if let Some(p) = handle_not_operator(&info, extension_declaration_list, column_names)? {
            predicates.push(p);
        }
        return Ok(predicates);
    }
    if is_logical_operator(name) {
        for argument in info.arguments {
            predicates.extend(column_predicates(
                extension_declaration_list,
                argument_value(argument),
                column_names,
            )?);
        }
        return Ok(predicates);
    }
    if info.arguments.len() == 2 {
        predicates.push(create_binary_column_predicate(&info, column_names)?);
    }
    if info.arguments.len() == 1 {
        predicates.push(create_unary_column_predicate(&info, column_names)?);
    }
    Ok(predicates)
}

pub fn logical_expression(
    extension_declaration_list: &[SimpleExtensionDeclaration],
    expression: Option<&Expression>,
    column_names: &[String],
) -> Result<Option<LogicalExpression>> {
    let Some(expression) = expression else {
        return Ok(None);
    };
    let Some(info) = extract_scalar_function_info(expression, extension_declaration_list) else {
        return Ok(None);
    };
    let name = info.function_name.as_deref();
    if name == Some(N::NOT_BOOL) {
        return Ok(
            handle_not_operator(&info, extension_declaration_list, column_names)?
                .map(LogicalExpression::from_leaf),
        );
    }
    if is_logical_operator(name) {
        let mut children = Vec::new();
        for argument in info.arguments {
            if let Some(child) = logical_expression(
                extension_declaration_list,
                Some(argument_value(argument)),
                column_names,
            )? {
                children.push(child);
            }
        }
        return Ok(Some(LogicalExpression::new(
            map_to_operator(name)?,
            children,
        )));
    }
    if info.arguments.len() == 2 {
        return Ok(Some(LogicalExpression::from_leaf(
            create_binary_column_predicate(&info, column_names)?,
        )));
    }
    if info.arguments.len() == 1 {
        return Ok(Some(LogicalExpression::from_leaf(
            create_unary_column_predicate(&info, column_names)?,
        )));
    }
    Ok(None)
}

fn default_expression() -> &'static Expression {
    static DEFAULT: Expression = Expression { rex_type: None };
    &DEFAULT
}

fn argument_value(argument: &FunctionArgument) -> &Expression {
    match &argument.arg_type {
        Some(ArgType::Value(e)) => e,
        _ => default_expression(),
    }
}

fn map_function_references(list: &[SimpleExtensionDeclaration]) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for extension in list {
        if let Some(MappingType::ExtensionFunction(f)) = &extension.mapping_type {
            map.insert(f.function_anchor, f.name.clone());
        }
    }
    map
}

fn extract_column_name(expr: &Expression, schema_names: &[String]) -> Result<String> {
    let mut expr = expr;
    if let Some(RexType::Cast(cast)) = &expr.rex_type {
        expr = cast.input.as_deref().unwrap_or(default_expression());
    }
    let field_index = match &expr.rex_type {
        Some(RexType::Selection(sel)) => match &sel.reference_type {
            Some(ReferenceType::DirectReference(seg)) => match &seg.reference_type {
                Some(reference_segment::ReferenceType::StructField(sf)) => sf.field,
                _ => 0,
            },
            _ => 0,
        },
        _ => 0,
    };
    usize::try_from(field_index)
        .ok()
        .and_then(|i| schema_names.get(i))
        .cloned()
        .ok_or_else(|| {
            SubstraitError::IndexOutOfBounds(format!(
                "Index {} out of bounds for length {}",
                field_index,
                schema_names.len()
            ))
        })
}

fn extract_value_with_possible_cast(expr: &Expression) -> Result<(LiteralValue, DataType)> {
    if let Some(RexType::Cast(cast)) = &expr.rex_type {
        return literal::extract(cast.input.as_deref().unwrap_or(default_expression()));
    }
    literal::extract(expr)
}

fn extract_scalar_function_info<'a>(
    expression: &'a Expression,
    list: &[SimpleExtensionDeclaration],
) -> Option<ScalarFunctionInfo<'a>> {
    match &expression.rex_type {
        Some(RexType::ScalarFunction(sf)) => {
            let function_map = map_function_references(list);
            Some(ScalarFunctionInfo {
                function_name: function_map.get(&sf.function_reference).cloned(),
                arguments: &sf.arguments,
            })
        }
        _ => None,
    }
}

fn create_unary_column_predicate(
    info: &ScalarFunctionInfo<'_>,
    column_names: &[String],
) -> Result<ColumnPredicate> {
    let column_name = extract_column_name(argument_value(&info.arguments[0]), column_names)?;
    let operator = map_to_operator(info.function_name.as_deref())?;
    Ok(ColumnPredicate::new(
        Some(column_name),
        operator,
        None,
        None,
    ))
}

fn create_binary_column_predicate(
    info: &ScalarFunctionInfo<'_>,
    column_names: &[String],
) -> Result<ColumnPredicate> {
    let column_name = extract_column_name(argument_value(&info.arguments[0]), column_names)?;
    let (value, arrow_type) = extract_value_with_possible_cast(argument_value(&info.arguments[1]))?;
    let operator = map_to_operator(info.function_name.as_deref())?;
    Ok(ColumnPredicate::new(
        Some(column_name),
        operator,
        Some(PredicateValue::Literal(value)),
        Some(arrow_type),
    ))
}

fn is_logical_operator(name: Option<&str>) -> bool {
    name == Some(N::AND_BOOL) || name == Some(N::OR_BOOL)
}

fn map_to_operator(name: Option<&str>) -> Result<SubstraitOperator> {
    let name = name.ok_or_else(|| SubstraitError::NullPointer("function name is null".into()))?;
    Ok(match name {
        N::GT_ANY_ANY | N::GT_PTS_PTS => SubstraitOperator::GreaterThan,
        N::GTE_ANY_ANY | N::GTE_PTS_PTS => SubstraitOperator::GreaterThanOrEqualTo,
        N::LT_ANY_ANY | N::LT_PTS_PTS => SubstraitOperator::LessThan,
        N::LTE_ANY_ANY | N::LTE_PTS_PTS => SubstraitOperator::LessThanOrEqualTo,
        N::EQUAL_ANY_ANY => SubstraitOperator::Equal,
        N::NOT_EQUAL_ANY_ANY => SubstraitOperator::NotEqual,
        N::IS_NULL_ANY => SubstraitOperator::IsNull,
        N::IS_NOT_NULL_ANY => SubstraitOperator::IsNotNull,
        N::AND_BOOL => SubstraitOperator::And,
        N::OR_BOOL => SubstraitOperator::Or,
        N::NOT_BOOL => SubstraitOperator::Not,
        other => {
            return Err(SubstraitError::Unsupported(format!(
                "Unsupported operator function: {other}"
            )))
        }
    })
}

fn handle_not_operator(
    not_info: &ScalarFunctionInfo<'_>,
    list: &[SimpleExtensionDeclaration],
    column_names: &[String],
) -> Result<Option<ColumnPredicate>> {
    if not_info.arguments.len() != 1 {
        return Ok(None);
    }
    let inner_expression = argument_value(&not_info.arguments[0]);
    let inner_info = extract_scalar_function_info(inner_expression, list);
    let inner_name = inner_info.as_ref().and_then(|i| i.function_name.as_deref());
    if inner_name == Some(N::AND_BOOL) || inner_name == Some(N::OR_BOOL) {
        let children = column_predicates(list, inner_expression, column_names)?;
        let operator = if inner_name == Some(N::AND_BOOL) {
            SubstraitOperator::Nand
        } else {
            SubstraitOperator::Nor
        };
        return Ok(Some(ColumnPredicate::new(
            None,
            operator,
            Some(PredicateValue::Predicates(children)),
            None,
        )));
    }
    let inner_predicates = column_predicates(list, inner_expression, column_names)?;
    if is_not_in_pattern(&inner_predicates) {
        return Ok(create_not_in_predicate(&inner_predicates));
    }
    if inner_predicates.len() == 1 {
        return Ok(Some(create_negated_predicate(&inner_predicates[0])));
    }
    Ok(None)
}

fn is_not_in_pattern(predicates: &[ColumnPredicate]) -> bool {
    if predicates.len() <= 1 {
        return false;
    }
    let first_column = predicates[0].column();
    predicates
        .iter()
        .all(|p| p.operator() == SubstraitOperator::Equal && p.column() == first_column)
}

fn create_not_in_predicate(equal_predicates: &[ColumnPredicate]) -> Option<ColumnPredicate> {
    let first = equal_predicates.first()?;
    let excluded: Vec<Option<PredicateValue>> = equal_predicates
        .iter()
        .map(|p| p.value().cloned())
        .collect();
    Some(ColumnPredicate::new(
        first.column().map(str::to_string),
        SubstraitOperator::NotIn,
        Some(PredicateValue::Values(excluded)),
        None,
    ))
}

fn create_negated_predicate(predicate: &ColumnPredicate) -> ColumnPredicate {
    let column = predicate.column().map(str::to_string);
    let value = predicate.value().cloned();
    let arrow_type = predicate.arrow_type().cloned();
    let (operator, value) = match predicate.operator() {
        SubstraitOperator::Equal => (SubstraitOperator::NotEqual, value),
        SubstraitOperator::NotEqual => (SubstraitOperator::Equal, value),
        SubstraitOperator::GreaterThan => (SubstraitOperator::LessThanOrEqualTo, value),
        SubstraitOperator::GreaterThanOrEqualTo => (SubstraitOperator::LessThan, value),
        SubstraitOperator::LessThan => (SubstraitOperator::GreaterThanOrEqualTo, value),
        SubstraitOperator::LessThanOrEqualTo => (SubstraitOperator::GreaterThan, value),
        SubstraitOperator::IsNull => (SubstraitOperator::IsNotNull, None),
        SubstraitOperator::IsNotNull => (SubstraitOperator::IsNull, None),
        _ => (SubstraitOperator::Not, value),
    };
    ColumnPredicate::new(column, operator, value, arrow_type)
}
