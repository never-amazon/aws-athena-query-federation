use std::fmt;

use bigdecimal::BigDecimal;

use crate::calcite::operators::{self, SqlKind};
use crate::calcite::rel_data_type::RelDataType;
use crate::calcite::sql_node::{
    LiteralObject, SqlCall, SqlCase, SqlLiteral, SqlLiteralValue, SqlNode, SqlSelect, SqlWindow,
};
use crate::calcite::sql_type_name::SqlTypeName;
use crate::calcite::values::{
    java_big_decimal_to_string, java_double_to_string, java_float_to_string, BitString, DateString,
    TimeString, TimestampString,
};
use crate::error::{Result, SubstraitError};
use crate::java_cast::{double_to_long, long_to_double};

pub fn parameterize(
    mut node: SqlNode,
    schema: &RelDataType,
) -> Result<(SqlNode, Vec<SubstraitTypeAndValue>)> {
    let mut rewriter = Rewriter {
        schema,
        params: Vec::new(),
    };
    rewriter.rewrite(&mut node, None, false)?;
    Ok((node, rewriter.params))
}

struct Rewriter<'a> {
    schema: &'a RelDataType,
    params: Vec<SubstraitTypeAndValue>,
}

fn simple_name(node: &SqlNode) -> Option<&str> {
    match node {
        SqlNode::Identifier(id) if id.is_simple() => id.names.first().map(String::as_str),
        _ => None,
    }
}

fn is_binary_comparison(kind: SqlKind) -> bool {
    matches!(
        kind,
        SqlKind::Equals
            | SqlKind::NotEquals
            | SqlKind::GreaterThan
            | SqlKind::LessThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::LessThanOrEqual
    )
}

fn literal_value(literal: &SqlLiteral) -> SubstraitValue {
    if let SqlLiteralValue::Interval { text, .. } = &literal.value {
        return SubstraitValue::Interval(text.clone());
    }
    match literal.value() {
        LiteralObject::Null => SubstraitValue::Null,
        LiteralObject::Boolean(b) => SubstraitValue::Boolean(b),
        LiteralObject::Decimal(d) => SubstraitValue::Decimal(d),
        LiteralObject::String(s) => SubstraitValue::String(s),
        LiteralObject::Date(d) => SubstraitValue::Date(d),
        LiteralObject::Time(t) => SubstraitValue::Time(t),
        LiteralObject::Timestamp(t) => SubstraitValue::Timestamp(t),
        LiteralObject::Bits(b) => SubstraitValue::Bits(b),
        LiteralObject::Symbol(s) => SubstraitValue::Symbol(s),
    }
}

impl Rewriter<'_> {
    fn rewrite(
        &mut self,
        node: &mut SqlNode,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        match node {
            SqlNode::Select(select) => {
                self.rewrite_select(select, column, in_where)?;
                Ok(true)
            }
            SqlNode::Call(call) => {
                if let Some(name) = self.negated_boolean_column(call) {
                    *node = call.operands.swap_remove(0);
                    self.bool_equals(node, &name, false)?;
                    return Ok(true);
                }
                self.rewrite_call(call, column, in_where)
            }
            SqlNode::Identifier(_) => {
                let Some(name) = simple_name(node)
                    .filter(|name| in_where && self.is_boolean_column(name))
                    .map(str::to_string)
                else {
                    return Ok(false);
                };
                self.bool_equals(node, &name, true)?;
                Ok(true)
            }
            SqlNode::Literal(literal) => {
                let Some(column) = column else {
                    tracing::debug!(
                        "Standalone literal {} without column context, skipping",
                        literal.to_value()
                    );
                    return Ok(false);
                };
                let index = self.add_param(column, literal)?;
                *node = SqlNode::DynamicParam(index);
                Ok(true)
            }
            SqlNode::NodeList(list) => self.rewrite_all(list, column, in_where),
            SqlNode::Case(case) => self.rewrite_case(case, column, in_where),
            SqlNode::Window(window) => self.rewrite_window(window, column, in_where),
            SqlNode::DynamicParam(_) | SqlNode::DataType(_) | SqlNode::Text(_) => Ok(false),
        }
    }

    fn rewrite_all(
        &mut self,
        list: &mut [SqlNode],
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = false;
        for node in list {
            changed |= self.rewrite(node, column, in_where)?;
        }
        Ok(changed)
    }

    fn rewrite_opt(
        &mut self,
        node: Option<&mut SqlNode>,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        match node {
            Some(node) => self.rewrite(node, column, in_where),
            None => Ok(false),
        }
    }

    fn rewrite_case(
        &mut self,
        case: &mut SqlCase,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = self.rewrite_opt(case.value.as_deref_mut(), column, in_where)?;
        changed |= self.rewrite_all(&mut case.when_list, column, in_where)?;
        changed |= self.rewrite_all(&mut case.then_list, column, in_where)?;
        changed |= self.rewrite_opt(case.else_expr.as_deref_mut(), column, in_where)?;
        Ok(changed)
    }

    fn rewrite_window(
        &mut self,
        window: &mut SqlWindow,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = self.rewrite_all(&mut window.partition_list, column, in_where)?;
        changed |= self.rewrite_all(&mut window.order_list, column, in_where)?;
        changed |= self.rewrite_opt(window.lower_bound.as_deref_mut(), column, in_where)?;
        changed |= self.rewrite_opt(window.upper_bound.as_deref_mut(), column, in_where)?;
        Ok(changed)
    }

    fn rewrite_select(
        &mut self,
        select: &mut SqlSelect,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<()> {
        self.rewrite_opt(select.where_clause.as_mut(), column, true)?;
        self.rewrite_all(&mut select.keyword_list, column, in_where)?;
        self.rewrite_all(&mut select.select_list, column, in_where)?;
        self.rewrite_opt(select.from.as_mut(), column, in_where)?;
        if let Some(group_by) = &mut select.group_by {
            self.rewrite_all(group_by, column, in_where)?;
        }
        self.rewrite_opt(select.having.as_mut(), column, in_where)?;
        self.rewrite_all(&mut select.window_decls, column, in_where)?;
        self.rewrite_opt(select.qualify.as_mut(), column, in_where)?;
        if let Some(order_by) = &mut select.order_by {
            self.rewrite_all(order_by, column, in_where)?;
        }
        self.rewrite_opt(select.offset.as_mut(), column, in_where)?;
        self.rewrite_opt(select.fetch.as_mut(), column, in_where)?;
        Ok(())
    }

    fn rewrite_call(
        &mut self,
        call: &mut SqlCall,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let kind = call.op.kind;
        let rewritten = match kind {
            SqlKind::And | SqlKind::Or => return self.rewrite_logical(call, column, in_where),
            _ if is_binary_comparison(kind) => self.rewrite_comparison(call, in_where)?,
            SqlKind::In => self.rewrite_in(call, in_where)?,
            SqlKind::Between => self.rewrite_between(call, in_where)?,
            SqlKind::Like => self.rewrite_like(call, in_where)?,
            _ => false,
        };
        if rewritten {
            return Ok(true);
        }
        self.rewrite_all(&mut call.operands, column, in_where)
    }

    fn rewrite_logical(
        &mut self,
        call: &mut SqlCall,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = false;
        for operand in &mut call.operands {
            let boolean_column = simple_name(operand)
                .filter(|name| self.is_boolean_column(name))
                .map(str::to_string);
            match boolean_column {
                Some(name) => {
                    self.bool_equals(operand, &name, true)?;
                    changed = true;
                }
                None => changed |= self.rewrite(operand, column, in_where)?,
            }
        }
        Ok(changed)
    }

    fn rewrite_comparison(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let (Some(left), Some(right)) = (call.operands.first(), call.operands.get(1)) else {
            return Ok(false);
        };
        let target = match simple_name(left) {
            Some(name) => Some((name.to_string(), 1)),
            None => simple_name(right).map(|name| (name.to_string(), 0)),
        };
        match target {
            Some((name, value_index)) => {
                self.rewrite(&mut call.operands[value_index], Some(&name), in_where)
            }
            None => Ok(false),
        }
    }

    fn rewrite_in(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let Some(name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return Ok(false);
        };
        match call.operands.get_mut(1) {
            Some(SqlNode::NodeList(values)) => self.rewrite_all(values, Some(&name), in_where),
            _ => Ok(false),
        }
    }

    fn rewrite_between(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let Some(name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return Ok(false);
        };
        let [_, lower, upper, ..] = call.operands.as_mut_slice() else {
            return Ok(false);
        };
        let lower_changed = self.rewrite(lower, Some(&name), in_where)?;
        let upper_changed = self.rewrite(upper, Some(&name), in_where)?;
        Ok(lower_changed || upper_changed)
    }

    fn rewrite_like(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let Some(name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return Ok(false);
        };
        match call.operands.get_mut(1) {
            Some(pattern) => self.rewrite(pattern, Some(&name), in_where),
            None => Ok(false),
        }
    }

    fn negated_boolean_column(&self, call: &SqlCall) -> Option<String> {
        if call.op.kind != SqlKind::Not {
            return None;
        }
        match call.operands.as_slice() {
            [operand] => simple_name(operand)
                .filter(|name| self.is_boolean_column(name))
                .map(str::to_string),
            _ => None,
        }
    }

    fn bool_equals(&mut self, slot: &mut SqlNode, name: &str, value: bool) -> Result<()> {
        let index = self.add_param(name, &SqlLiteral::boolean(value))?;
        let identifier = std::mem::replace(slot, SqlNode::DynamicParam(index));
        *slot = SqlNode::call(
            &operators::EQUALS,
            vec![identifier, SqlNode::DynamicParam(index)],
        );
        Ok(())
    }

    fn add_param(&mut self, column_name: &str, literal: &SqlLiteral) -> Result<usize> {
        let field = self.schema.field(column_name, true, true).ok_or_else(|| {
            SubstraitError::IllegalArgument(format!(
                "field {column_name} not found in schema with fields: [{}]",
                self.schema.field_names().join(", ")
            ))
        })?;
        let type_name: SqlTypeName = field.ty().sql_type_name();
        self.params.push(SubstraitTypeAndValue::new(
            type_name,
            literal_value(literal),
            column_name,
        )?);
        Ok(self.params.len() - 1)
    }

    fn is_boolean_column(&self, column_name: &str) -> bool {
        self.schema
            .field(column_name, true, true)
            .is_some_and(|f| f.ty().sql_type_name() == SqlTypeName::Boolean)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SubstraitValue {
    Null,
    Boolean(bool),
    Byte(i8),
    Short(i16),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Decimal(BigDecimal),
    String(String),
    Bytes(Vec<u8>),
    Date(DateString),
    Time(TimeString),
    Timestamp(TimestampString),
    Bits(BitString),
    Symbol(String),
    Interval(String),
}

impl SubstraitValue {
    pub fn is_null(&self) -> bool {
        matches!(self, SubstraitValue::Null)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            SubstraitValue::Byte(v) => Some(i64::from(*v)),
            SubstraitValue::Short(v) => Some(i64::from(*v)),
            SubstraitValue::Integer(v) => Some(i64::from(*v)),
            SubstraitValue::Long(v) => Some(*v),
            SubstraitValue::Float(v) => Some(double_to_long(f64::from(*v))),
            SubstraitValue::Double(v) => Some(double_to_long(*v)),
            SubstraitValue::Decimal(d) => {
                let (unscaled, scale) = d.with_scale(0).as_bigint_and_exponent();
                if scale != 0 {
                    return None;
                }
                let bytes = unscaled.to_signed_bytes_le();
                let mut buf = [if unscaled.sign() == num_bigint::Sign::Minus {
                    0xFF
                } else {
                    0
                }; 8];
                for (i, b) in bytes.iter().take(8).enumerate() {
                    buf[i] = *b;
                }
                Some(i64::from_le_bytes(buf))
            }
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            SubstraitValue::Float(v) => Some(f64::from(*v)),
            SubstraitValue::Double(v) => Some(*v),
            SubstraitValue::Decimal(d) => java_big_decimal_to_string(d).parse().ok(),
            other => other.as_i64().map(long_to_double),
        }
    }

    pub fn as_decimal(&self) -> Option<&BigDecimal> {
        match self {
            SubstraitValue::Decimal(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            SubstraitValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn java_class_name(&self) -> &'static str {
        match self {
            SubstraitValue::Null => "null",
            SubstraitValue::Boolean(_) => "java.lang.Boolean",
            SubstraitValue::Byte(_) => "java.lang.Byte",
            SubstraitValue::Short(_) => "java.lang.Short",
            SubstraitValue::Integer(_) => "java.lang.Integer",
            SubstraitValue::Long(_) => "java.lang.Long",
            SubstraitValue::Float(_) => "java.lang.Float",
            SubstraitValue::Double(_) => "java.lang.Double",
            SubstraitValue::Decimal(_) => "java.math.BigDecimal",
            SubstraitValue::String(_) => "java.lang.String",
            SubstraitValue::Bytes(_) => "[B",
            SubstraitValue::Date(_) => "org.apache.calcite.util.DateString",
            SubstraitValue::Time(_) => "org.apache.calcite.util.TimeString",
            SubstraitValue::Timestamp(_) => "org.apache.calcite.util.TimestampString",
            SubstraitValue::Bits(_) => "org.apache.calcite.util.BitString",
            SubstraitValue::Symbol(_) => "java.lang.Enum",
            SubstraitValue::Interval(_) => {
                "org.apache.calcite.sql.SqlIntervalLiteral$IntervalValue"
            }
        }
    }
}

impl fmt::Display for SubstraitValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubstraitValue::Null => f.write_str("null"),
            SubstraitValue::Boolean(b) => write!(f, "{b}"),
            SubstraitValue::Byte(v) => write!(f, "{v}"),
            SubstraitValue::Short(v) => write!(f, "{v}"),
            SubstraitValue::Integer(v) => write!(f, "{v}"),
            SubstraitValue::Long(v) => write!(f, "{v}"),
            SubstraitValue::Float(v) => f.write_str(&java_float_to_string(*v)),
            SubstraitValue::Double(v) => f.write_str(&java_double_to_string(*v)),
            SubstraitValue::Decimal(d) => f.write_str(&java_big_decimal_to_string(d)),
            SubstraitValue::String(s) | SubstraitValue::Symbol(s) | SubstraitValue::Interval(s) => {
                f.write_str(s)
            }
            SubstraitValue::Bytes(b) => write!(f, "{b:?}"),
            SubstraitValue::Date(d) => write!(f, "{d}"),
            SubstraitValue::Time(t) => write!(f, "{t}"),
            SubstraitValue::Timestamp(t) => write!(f, "{t}"),
            SubstraitValue::Bits(b) => write!(f, "{b}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubstraitTypeAndValue {
    type_name: SqlTypeName,
    value: SubstraitValue,
    column_name: String,
}

impl SubstraitTypeAndValue {
    pub fn new(
        type_name: SqlTypeName,
        value: SubstraitValue,
        column_name: impl Into<String>,
    ) -> Result<SubstraitTypeAndValue> {
        if value.is_null() {
            return Err(SubstraitError::NullPointer("value is null".into()));
        }
        Ok(SubstraitTypeAndValue {
            type_name,
            value,
            column_name: column_name.into(),
        })
    }

    pub fn ty(&self) -> SqlTypeName {
        self.type_name
    }

    pub fn value(&self) -> &SubstraitValue {
        &self.value
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }
}

impl fmt::Display for SubstraitTypeAndValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TypeAndValue{{type={}, value={}}}",
            self.type_name, self.value
        )
    }
}
