use bigdecimal::BigDecimal;

use super::operators::{self, SqlKind, SqlOperator};
use super::sql_type_name::SqlTypeName;
use super::values::{
    java_big_decimal_to_string, BitString, DateString, TimeString, TimestampString,
};

#[derive(Debug, Clone, PartialEq)]
pub struct SqlIdentifier {
    pub names: Vec<String>,
}

impl SqlIdentifier {
    pub fn new(names: Vec<String>) -> SqlIdentifier {
        SqlIdentifier { names }
    }

    pub fn simple(name: impl Into<String>) -> SqlIdentifier {
        SqlIdentifier {
            names: vec![name.into()],
        }
    }

    pub fn star() -> SqlIdentifier {
        SqlIdentifier {
            names: vec![String::new()],
        }
    }

    pub fn is_simple(&self) -> bool {
        self.names.len() == 1
    }

    pub fn is_star(&self) -> bool {
        self.names.last().map(String::is_empty).unwrap_or(false)
    }

    pub fn get_simple(&self) -> Option<&str> {
        if self.is_simple() {
            self.names.first().map(String::as_str)
        } else {
            None
        }
    }

    pub fn to_dotted(&self) -> String {
        self.names
            .iter()
            .map(|n| {
                if n.is_empty() {
                    "*".to_string()
                } else {
                    n.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlLiteralValue {
    Null,
    Boolean(bool),
    Exact(BigDecimal),
    Approx(BigDecimal),
    Char(String),
    Date(DateString),
    Time(TimeString, i32),
    Timestamp(TimestampString, i32),
    Binary(BitString),
    Symbol(String),
    Interval {
        sign: i32,
        text: String,
        qualifier: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlLiteral {
    pub value: SqlLiteralValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralObject {
    Null,
    Boolean(bool),
    Decimal(BigDecimal),
    String(String),
    Date(DateString),
    Time(TimeString),
    Timestamp(TimestampString),
    Bits(BitString),
    Symbol(String),
}

impl SqlLiteral {
    pub fn null() -> SqlLiteral {
        SqlLiteral {
            value: SqlLiteralValue::Null,
        }
    }

    pub fn boolean(b: bool) -> SqlLiteral {
        SqlLiteral {
            value: SqlLiteralValue::Boolean(b),
        }
    }

    pub fn exact(s: &str) -> SqlLiteral {
        let value = s.parse::<BigDecimal>().unwrap_or_default();
        SqlLiteral {
            value: SqlLiteralValue::Exact(value),
        }
    }

    pub fn exact_decimal(d: BigDecimal) -> SqlLiteral {
        SqlLiteral {
            value: SqlLiteralValue::Exact(d),
        }
    }

    pub fn approx(s: &str) -> SqlLiteral {
        let value = s.parse::<BigDecimal>().unwrap_or_default();
        SqlLiteral {
            value: SqlLiteralValue::Approx(value),
        }
    }

    pub fn char_string(s: impl Into<String>) -> SqlLiteral {
        SqlLiteral {
            value: SqlLiteralValue::Char(s.into()),
        }
    }

    pub fn symbol(s: impl Into<String>) -> SqlLiteral {
        SqlLiteral {
            value: SqlLiteralValue::Symbol(s.into()),
        }
    }

    pub fn get_type_name(&self) -> SqlTypeName {
        match &self.value {
            SqlLiteralValue::Null => SqlTypeName::Null,
            SqlLiteralValue::Boolean(_) => SqlTypeName::Boolean,
            SqlLiteralValue::Exact(_) => SqlTypeName::Decimal,
            SqlLiteralValue::Approx(_) => SqlTypeName::Double,
            SqlLiteralValue::Char(_) => SqlTypeName::Char,
            SqlLiteralValue::Date(_) => SqlTypeName::Date,
            SqlLiteralValue::Time(..) => SqlTypeName::Time,
            SqlLiteralValue::Timestamp(..) => SqlTypeName::Timestamp,
            SqlLiteralValue::Binary(_) => SqlTypeName::Binary,
            SqlLiteralValue::Symbol(_) => SqlTypeName::Symbol,
            SqlLiteralValue::Interval { .. } => SqlTypeName::IntervalDaySecond,
        }
    }

    pub fn get_value(&self) -> LiteralObject {
        match &self.value {
            SqlLiteralValue::Null => LiteralObject::Null,
            SqlLiteralValue::Boolean(b) => LiteralObject::Boolean(*b),
            SqlLiteralValue::Exact(d) | SqlLiteralValue::Approx(d) => {
                LiteralObject::Decimal(d.clone())
            }
            SqlLiteralValue::Char(s) => LiteralObject::String(s.clone()),
            SqlLiteralValue::Date(d) => LiteralObject::Date(d.clone()),
            SqlLiteralValue::Time(t, _) => LiteralObject::Time(t.clone()),
            SqlLiteralValue::Timestamp(t, _) => LiteralObject::Timestamp(t.clone()),
            SqlLiteralValue::Binary(b) => LiteralObject::Bits(b.clone()),
            SqlLiteralValue::Symbol(s) => LiteralObject::Symbol(s.clone()),
            SqlLiteralValue::Interval { text, .. } => LiteralObject::String(text.clone()),
        }
    }

    pub fn to_value(&self) -> String {
        match &self.value {
            SqlLiteralValue::Null => "NULL".to_string(),
            SqlLiteralValue::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            SqlLiteralValue::Exact(d) => super::values::java_big_decimal_to_plain_string(d),
            SqlLiteralValue::Approx(d) => super::values::java_big_decimal_to_scientific(d),
            SqlLiteralValue::Char(s) => s.clone(),
            other => format!("{:?}", other),
        }
    }
}

impl LiteralObject {
    pub fn to_java_string(&self) -> String {
        match self {
            LiteralObject::Null => "null".to_string(),
            LiteralObject::Boolean(b) => b.to_string(),
            LiteralObject::Decimal(d) => java_big_decimal_to_string(d),
            LiteralObject::String(s) => s.clone(),
            LiteralObject::Date(d) => d.to_string(),
            LiteralObject::Time(t) => t.to_string(),
            LiteralObject::Timestamp(t) => t.to_string(),
            LiteralObject::Bits(b) => b.to_string(),
            LiteralObject::Symbol(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlDataTypeSpec {
    pub type_name: String,
    pub precision: i32,
    pub scale: i32,
    pub charset: Option<String>,
    pub alien: bool,
    pub collection: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlCall {
    pub op: &'static SqlOperator,
    pub operands: Vec<SqlNode>,
    pub quantifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlCase {
    pub value: Option<Box<SqlNode>>,
    pub when_list: Vec<SqlNode>,
    pub then_list: Vec<SqlNode>,
    pub else_expr: Option<Box<SqlNode>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlWindow {
    pub partition_list: Vec<SqlNode>,
    pub order_list: Vec<SqlNode>,
    pub is_rows: bool,
    pub lower_bound: Option<Box<SqlNode>>,
    pub upper_bound: Option<Box<SqlNode>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SqlSelect {
    pub keyword_list: Vec<SqlNode>,
    pub select_list: Vec<SqlNode>,
    pub from: Option<SqlNode>,
    pub where_clause: Option<SqlNode>,
    pub group_by: Option<Vec<SqlNode>>,
    pub having: Option<SqlNode>,
    pub window_decls: Vec<SqlNode>,
    pub qualify: Option<SqlNode>,
    pub order_by: Option<Vec<SqlNode>>,
    pub offset: Option<SqlNode>,
    pub fetch: Option<SqlNode>,
}

impl SqlSelect {
    pub fn is_select_star(&self) -> bool {
        is_singleton_star(&self.select_list)
    }

    pub fn get_where(&self) -> Option<&SqlNode> {
        self.where_clause.as_ref()
    }

    pub fn set_where(&mut self, w: Option<SqlNode>) {
        self.where_clause = w;
    }

    pub fn get_select_list(&self) -> &[SqlNode] {
        &self.select_list
    }

    pub fn get_from(&self) -> Option<&SqlNode> {
        self.from.as_ref()
    }

    pub fn get_group(&self) -> Option<&[SqlNode]> {
        self.group_by.as_deref()
    }

    pub fn get_having(&self) -> Option<&SqlNode> {
        self.having.as_ref()
    }

    pub fn get_order_list(&self) -> Option<&[SqlNode]> {
        self.order_by.as_deref()
    }

    pub fn get_offset(&self) -> Option<&SqlNode> {
        self.offset.as_ref()
    }

    pub fn get_fetch(&self) -> Option<&SqlNode> {
        self.fetch.as_ref()
    }
}

pub fn is_singleton_star(list: &[SqlNode]) -> bool {
    list.len() == 1
        && matches!(&list[0], SqlNode::Identifier(id) if id.names.len() == 1 && id.names[0].is_empty())
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlNode {
    Identifier(SqlIdentifier),
    Literal(SqlLiteral),
    DynamicParam(usize),
    Call(SqlCall),
    Case(Box<SqlCase>),
    Window(Box<SqlWindow>),
    NodeList(Vec<SqlNode>),
    Select(Box<SqlSelect>),
    DataType(SqlDataTypeSpec),
}

impl SqlNode {
    pub fn to_sql_string(&self, dialect: &super::sql_dialect::SqlDialect) -> String {
        super::sql_writer::to_sql_string(self, dialect)
    }

    pub fn identifier(name: impl Into<String>) -> SqlNode {
        SqlNode::Identifier(SqlIdentifier::simple(name))
    }

    pub fn star() -> SqlNode {
        SqlNode::Identifier(SqlIdentifier::star())
    }

    pub fn literal(l: SqlLiteral) -> SqlNode {
        SqlNode::Literal(l)
    }

    pub fn call(op: &'static SqlOperator, operands: Vec<SqlNode>) -> SqlNode {
        SqlNode::Call(SqlCall {
            op,
            operands,
            quantifier: None,
        })
    }

    pub fn call_with_quantifier(
        op: &'static SqlOperator,
        quantifier: Option<String>,
        operands: Vec<SqlNode>,
    ) -> SqlNode {
        SqlNode::Call(SqlCall {
            op,
            operands,
            quantifier,
        })
    }

    pub fn as_alias(node: SqlNode, alias: impl Into<String>) -> SqlNode {
        SqlNode::call(&operators::AS, vec![node, SqlNode::identifier(alias)])
    }

    pub fn get_kind(&self) -> SqlKind {
        match self {
            SqlNode::Identifier(_) => SqlKind::Identifier,
            SqlNode::Literal(_) => SqlKind::Literal,
            SqlNode::DynamicParam(_) => SqlKind::DynamicParam,
            SqlNode::Call(c) => c.op.kind,
            SqlNode::Case(_) => SqlKind::Case,
            SqlNode::Window(_) => SqlKind::Window,
            SqlNode::NodeList(_) => SqlKind::NodeList,
            SqlNode::Select(_) => SqlKind::Select,
            SqlNode::DataType(_) => SqlKind::DataType,
        }
    }

    pub fn as_call(&self) -> Option<&SqlCall> {
        match self {
            SqlNode::Call(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_select(&self) -> Option<&SqlSelect> {
        match self {
            SqlNode::Select(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_identifier(&self) -> Option<&SqlIdentifier> {
        match self {
            SqlNode::Identifier(i) => Some(i),
            _ => None,
        }
    }

    pub fn as_literal(&self) -> Option<&SqlLiteral> {
        match self {
            SqlNode::Literal(l) => Some(l),
            _ => None,
        }
    }

    pub fn is_null_literal(&self) -> bool {
        matches!(
            self,
            SqlNode::Literal(SqlLiteral {
                value: SqlLiteralValue::Null
            })
        )
    }

    pub fn alias(&self) -> Option<String> {
        match self {
            SqlNode::Call(c) if c.op.kind == SqlKind::As => {
                c.operands.get(1).map(SqlNode::to_plain_string)
            }
            SqlNode::Call(c) if c.op.kind == SqlKind::Over => None,
            SqlNode::Identifier(id) => id.names.last().cloned(),
            _ => None,
        }
    }

    pub fn to_plain_string(&self) -> String {
        match self {
            SqlNode::Identifier(id) => id.to_dotted(),
            SqlNode::Literal(l) => l.to_value(),
            other => format!("{:?}", other),
        }
    }

    pub fn contains_agg(&self) -> bool {
        match self {
            SqlNode::Call(c) => c.op.aggregate || c.operands.iter().any(SqlNode::contains_agg),
            SqlNode::Case(c) => {
                c.value.as_ref().map(|v| v.contains_agg()).unwrap_or(false)
                    || c.when_list.iter().any(SqlNode::contains_agg)
                    || c.then_list.iter().any(SqlNode::contains_agg)
                    || c.else_expr
                        .as_ref()
                        .map(|v| v.contains_agg())
                        .unwrap_or(false)
            }
            SqlNode::NodeList(l) => l.iter().any(SqlNode::contains_agg),
            SqlNode::Window(w) => {
                w.partition_list.iter().any(SqlNode::contains_agg)
                    || w.order_list.iter().any(SqlNode::contains_agg)
            }
            _ => false,
        }
    }
}

pub fn contains_agg_list(list: &[SqlNode]) -> bool {
    list.iter().any(SqlNode::contains_agg)
}
