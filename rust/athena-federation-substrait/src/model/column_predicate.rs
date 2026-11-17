use std::fmt;

use arrow_schema::DataType;

use crate::model::substrait_operator::SubstraitOperator;
use crate::substrait_literal_converter::LiteralValue;

#[derive(Debug, Clone, PartialEq)]
pub enum PredicateValue {
    Literal(LiteralValue),
    Predicates(Vec<ColumnPredicate>),
    Values(Vec<Option<PredicateValue>>),
}

impl From<LiteralValue> for PredicateValue {
    fn from(value: LiteralValue) -> PredicateValue {
        PredicateValue::Literal(value)
    }
}

impl fmt::Display for PredicateValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PredicateValue::Literal(v) => write!(f, "{v}"),
            PredicateValue::Predicates(list) => {
                f.write_str("[")?;
                for (i, p) in list.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{p}")?;
                }
                f.write_str("]")
            }
            PredicateValue::Values(list) => {
                f.write_str("[")?;
                for (i, v) in list.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    match v {
                        Some(v) => write!(f, "{v}")?,
                        None => f.write_str("null")?,
                    }
                }
                f.write_str("]")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnPredicate {
    column_name: Option<String>,
    substrait_operator: SubstraitOperator,
    value: Option<PredicateValue>,
    arrow_type: Option<DataType>,
}

impl ColumnPredicate {
    pub fn new(
        column: Option<String>,
        substrait_operator: SubstraitOperator,
        value: Option<PredicateValue>,
        arrow_type: Option<DataType>,
    ) -> ColumnPredicate {
        ColumnPredicate {
            column_name: column,
            substrait_operator,
            value,
            arrow_type,
        }
    }

    pub fn get_column(&self) -> Option<&str> {
        self.column_name.as_deref()
    }

    pub fn get_operator(&self) -> SubstraitOperator {
        self.substrait_operator
    }

    pub fn get_value(&self) -> Option<&PredicateValue> {
        self.value.as_ref()
    }

    pub fn get_arrow_type(&self) -> Option<&DataType> {
        self.arrow_type.as_ref()
    }
}

impl fmt::Display for ColumnPredicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let column = self.column_name.as_deref().unwrap_or("null");
        let value = match &self.value {
            Some(v) => v.to_string(),
            None => "null".to_string(),
        };
        write!(f, "{} {} '{}'", column, self.substrait_operator, value)
    }
}
