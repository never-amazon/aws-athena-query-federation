use std::collections::HashMap;
use std::fmt;

use arrow_schema::DataType;

use crate::error::{Result, SubstraitError};
use crate::literal::{self, LiteralValue};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubstraitOperator {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqualTo,
    LessThanOrEqualTo,
    IsNull,
    IsNotNull,
    And,
    Or,
    Not,
    NotIn,
    Nor,
    Nand,
}

impl fmt::Display for SubstraitOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SubstraitOperator::Equal => "EQUAL",
            SubstraitOperator::NotEqual => "NOT_EQUAL",
            SubstraitOperator::GreaterThan => "GREATER_THAN",
            SubstraitOperator::LessThan => "LESS_THAN",
            SubstraitOperator::GreaterThanOrEqualTo => "GREATER_THAN_OR_EQUAL_TO",
            SubstraitOperator::LessThanOrEqualTo => "LESS_THAN_OR_EQUAL_TO",
            SubstraitOperator::IsNull => "IS_NULL",
            SubstraitOperator::IsNotNull => "IS_NOT_NULL",
            SubstraitOperator::And => "AND",
            SubstraitOperator::Or => "OR",
            SubstraitOperator::Not => "NOT",
            SubstraitOperator::NotIn => "NOT_IN",
            SubstraitOperator::Nor => "NOR",
            SubstraitOperator::Nand => "NAND",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnPredicate {
    Column {
        column: String,
        operator: SubstraitOperator,
        value: Option<LiteralValue>,
        arrow_type: Option<DataType>,
    },
    Values {
        column: String,
        operator: SubstraitOperator,
        values: Vec<Option<LiteralValue>>,
    },
    Group {
        operator: SubstraitOperator,
        children: Vec<ColumnPredicate>,
    },
}

impl ColumnPredicate {
    pub fn column(&self) -> Option<&str> {
        match self {
            ColumnPredicate::Column { column, .. } | ColumnPredicate::Values { column, .. } => {
                Some(column)
            }
            ColumnPredicate::Group { .. } => None,
        }
    }

    pub fn operator(&self) -> SubstraitOperator {
        match self {
            ColumnPredicate::Column { operator, .. }
            | ColumnPredicate::Values { operator, .. }
            | ColumnPredicate::Group { operator, .. } => *operator,
        }
    }

    pub fn value(&self) -> Option<&LiteralValue> {
        match self {
            ColumnPredicate::Column { value, .. } => value.as_ref(),
            ColumnPredicate::Values { .. } | ColumnPredicate::Group { .. } => None,
        }
    }

    pub fn arrow_type(&self) -> Option<&DataType> {
        match self {
            ColumnPredicate::Column { arrow_type, .. } => arrow_type.as_ref(),
            ColumnPredicate::Values { .. } | ColumnPredicate::Group { .. } => None,
        }
    }

    fn negated(self) -> ColumnPredicate {
        match self {
            ColumnPredicate::Column {
                column,
                operator,
                value,
                arrow_type,
            } => {
                let (operator, value) = match operator {
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
                ColumnPredicate::Column {
                    column,
                    operator,
                    value,
                    arrow_type,
                }
            }
            ColumnPredicate::Values { column, values, .. } => ColumnPredicate::Values {
                column,
                operator: SubstraitOperator::Not,
                values,
            },
            ColumnPredicate::Group { children, .. } => ColumnPredicate::Group {
                operator: SubstraitOperator::Not,
                children,
            },
        }
    }
}

fn write_list<T>(
    f: &mut fmt::Formatter<'_>,
    items: &[T],
    item: impl Fn(&mut fmt::Formatter<'_>, &T) -> fmt::Result,
) -> fmt::Result {
    f.write_str("[")?;
    for (i, v) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        item(f, v)?;
    }
    f.write_str("]")
}

impl fmt::Display for ColumnPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} '",
            self.column().unwrap_or("null"),
            self.operator()
        )?;
        match self {
            ColumnPredicate::Column { value: Some(v), .. } => write!(f, "{v}")?,
            ColumnPredicate::Column { value: None, .. } => f.write_str("null")?,
            ColumnPredicate::Values { values, .. } => write_list(f, values, |f, v| match v {
                Some(v) => write!(f, "{v}"),
                None => f.write_str("null"),
            })?,
            ColumnPredicate::Group { children, .. } => {
                write_list(f, children, |f, p| write!(f, "{p}"))?;
            }
        }
        f.write_str("'")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalExpression {
    Leaf(ColumnPredicate),
    Node {
        operator: SubstraitOperator,
        children: Vec<LogicalExpression>,
    },
}

impl LogicalExpression {
    pub fn operator(&self) -> SubstraitOperator {
        match self {
            LogicalExpression::Leaf(predicate) => predicate.operator(),
            LogicalExpression::Node { operator, .. } => *operator,
        }
    }

    pub fn has_complex_logic(&self) -> bool {
        matches!(
            self,
            LogicalExpression::Node {
                operator: SubstraitOperator::And | SubstraitOperator::Or,
                ..
            }
        )
    }
}

struct ScalarFunctionInfo<'a> {
    function_name: Option<&'a str>,
    arguments: &'a [FunctionArgument],
}

impl ScalarFunctionInfo<'_> {
    fn operator(&self) -> Result<SubstraitOperator> {
        let name = self
            .function_name
            .ok_or_else(|| SubstraitError::NullPointer("function name is null".into()))?;
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

    fn is_logical(&self) -> bool {
        matches!(self.function_name, Some(N::AND_BOOL | N::OR_BOOL))
    }

    fn leaf_predicate(&self, column_names: &[String]) -> Result<Option<ColumnPredicate>> {
        match self.arguments {
            [column, value] => {
                let column = extract_column_name(argument_value(column), column_names)?;
                let (value, arrow_type) = extract_value_with_possible_cast(argument_value(value))?;
                Ok(Some(ColumnPredicate::Column {
                    column,
                    operator: self.operator()?,
                    value: Some(value),
                    arrow_type: Some(arrow_type),
                }))
            }
            [column] => {
                let column = extract_column_name(argument_value(column), column_names)?;
                Ok(Some(ColumnPredicate::Column {
                    column,
                    operator: self.operator()?,
                    value: None,
                    arrow_type: None,
                }))
            }
            _ => Ok(None),
        }
    }
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
    let Some(info) = scalar_function_info(expression, extension_declaration_list) else {
        return Ok(Vec::new());
    };
    if info.function_name == Some(N::NOT_BOOL) {
        return Ok(
            not_predicate(&info, extension_declaration_list, column_names)?
                .into_iter()
                .collect(),
        );
    }
    if info.is_logical() {
        let mut predicates = Vec::new();
        for argument in info.arguments.iter().filter_map(argument_value) {
            predicates.extend(column_predicates(
                extension_declaration_list,
                argument,
                column_names,
            )?);
        }
        return Ok(predicates);
    }
    Ok(info.leaf_predicate(column_names)?.into_iter().collect())
}

pub fn logical_expression(
    extension_declaration_list: &[SimpleExtensionDeclaration],
    expression: &Expression,
    column_names: &[String],
) -> Result<Option<LogicalExpression>> {
    let Some(info) = scalar_function_info(expression, extension_declaration_list) else {
        return Ok(None);
    };
    if info.function_name == Some(N::NOT_BOOL) {
        return Ok(
            not_predicate(&info, extension_declaration_list, column_names)?
                .map(LogicalExpression::Leaf),
        );
    }
    if info.is_logical() {
        let mut children = Vec::new();
        for argument in info.arguments.iter().filter_map(argument_value) {
            children.extend(logical_expression(
                extension_declaration_list,
                argument,
                column_names,
            )?);
        }
        return Ok(Some(LogicalExpression::Node {
            operator: info.operator()?,
            children,
        }));
    }
    Ok(info
        .leaf_predicate(column_names)?
        .map(LogicalExpression::Leaf))
}

fn argument_value(argument: &FunctionArgument) -> Option<&Expression> {
    match &argument.arg_type {
        Some(ArgType::Value(e)) => Some(e),
        _ => None,
    }
}

fn uncast(expr: Option<&Expression>) -> Option<&Expression> {
    match expr.and_then(|e| e.rex_type.as_ref()) {
        Some(RexType::Cast(cast)) => cast.input.as_deref(),
        _ => expr,
    }
}

fn extract_column_name(expr: Option<&Expression>, schema_names: &[String]) -> Result<String> {
    let field_index = match uncast(expr).and_then(|e| e.rex_type.as_ref()) {
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

fn extract_value_with_possible_cast(expr: Option<&Expression>) -> Result<(LiteralValue, DataType)> {
    match uncast(expr) {
        Some(expr) => literal::extract(expr),
        None => Err(SubstraitError::IllegalArgument(
            "Expected literal value in expression".into(),
        )),
    }
}

fn scalar_function_info<'a>(
    expression: &'a Expression,
    list: &'a [SimpleExtensionDeclaration],
) -> Option<ScalarFunctionInfo<'a>> {
    let Some(RexType::ScalarFunction(sf)) = &expression.rex_type else {
        return None;
    };
    let function_name = list
        .iter()
        .rev()
        .find_map(|extension| match &extension.mapping_type {
            Some(MappingType::ExtensionFunction(f))
                if f.function_anchor == sf.function_reference =>
            {
                Some(f.name.as_str())
            }
            _ => None,
        });
    Some(ScalarFunctionInfo {
        function_name,
        arguments: &sf.arguments,
    })
}

fn not_predicate(
    not_info: &ScalarFunctionInfo<'_>,
    list: &[SimpleExtensionDeclaration],
    column_names: &[String],
) -> Result<Option<ColumnPredicate>> {
    let [argument] = not_info.arguments else {
        return Ok(None);
    };
    let Some(inner_expression) = argument_value(argument) else {
        return Ok(None);
    };
    let inner_name = scalar_function_info(inner_expression, list).and_then(|i| i.function_name);
    let group_operator = match inner_name {
        Some(N::AND_BOOL) => Some(SubstraitOperator::Nand),
        Some(N::OR_BOOL) => Some(SubstraitOperator::Nor),
        _ => None,
    };
    let mut inner_predicates = column_predicates(list, inner_expression, column_names)?;
    if let Some(operator) = group_operator {
        return Ok(Some(ColumnPredicate::Group {
            operator,
            children: inner_predicates,
        }));
    }
    if let Some(not_in) = not_in_predicate(&inner_predicates) {
        return Ok(Some(not_in));
    }
    if inner_predicates.len() == 1 {
        return Ok(inner_predicates.pop().map(ColumnPredicate::negated));
    }
    Ok(None)
}

fn not_in_predicate(predicates: &[ColumnPredicate]) -> Option<ColumnPredicate> {
    let [first, _, ..] = predicates else {
        return None;
    };
    let ColumnPredicate::Column { column, .. } = first else {
        return None;
    };
    let mut values = Vec::with_capacity(predicates.len());
    for p in predicates {
        match p {
            ColumnPredicate::Column {
                column: c,
                operator: SubstraitOperator::Equal,
                value,
                ..
            } if c == column => values.push(value.clone()),
            _ => return None,
        }
    }
    Some(ColumnPredicate::Values {
        column: column.clone(),
        operator: SubstraitOperator::NotIn,
        values,
    })
}
