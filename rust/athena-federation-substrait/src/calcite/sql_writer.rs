use super::operators::{self, SqlKind, SqlOperator, SqlSyntax};
use super::sql_dialect::SqlDialect;
use super::sql_node::{
    SqlCall, SqlCase, SqlDataTypeSpec, SqlIdentifier, SqlLiteral, SqlLiteralValue, SqlNode,
    SqlSelect, SqlWindow,
};
use super::values::java_big_decimal_to_plain_string;

const NL: &str = "\n";

const COMMA_LEFT_PREC: i32 = 3;
const COMMA_RIGHT_PREC: i32 = 2;
const FETCH_PREC: i32 = 10;
const ORDER_BY_LEFT_PREC: i32 = 0;
const ORDER_BY_RIGHT_PREC: i32 = 1;
const JOIN_LEFT_PREC: i32 = 16;
const JOIN_RIGHT_PREC: i32 = 17;

const FUNCTION_ID_NAMES: [&str; 13] = [
    "USER",
    "CURRENT_USER",
    "SESSION_USER",
    "SYSTEM_USER",
    "CURRENT_PATH",
    "CURRENT_ROLE",
    "CURRENT_CATALOG",
    "CURRENT_SCHEMA",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_DATE",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Select,
    SelectList,
    FromList,
    WhereList,
    GroupByList,
    OrderByList,
    Offset,
    Fetch,
    SubQuery,
    FunCall,
    Simple,
    Identifier,
    Case,
    Window,
    As,
    Join,
    SetOp,
    Values,
}

#[derive(Debug, Clone)]
struct Frame {
    kind: FrameType,
    close: &'static str,
    item_count: usize,
}

impl Frame {
    fn newline_before_sep(&self) -> bool {
        matches!(
            self.kind,
            FrameType::Select
                | FrameType::Offset
                | FrameType::Fetch
                | FrameType::SubQuery
                | FrameType::Join
                | FrameType::FromList
                | FrameType::SetOp
        )
    }
}

pub struct SqlWriter<'a> {
    dialect: &'a SqlDialect,
    buf: String,
    need_whitespace: bool,
    next_whitespace: &'static str,
    frame: Option<Frame>,
    list_stack: Vec<Frame>,
    dynamic_parameters: Vec<usize>,
}

fn need_whitespace_before(s: &str) -> bool {
    !(s == "," || s == "." || s == ")" || s == "[" || s == "]" || s.is_empty())
}

fn need_whitespace_after(s: &str) -> bool {
    !(s == "(" || s == "[" || s == ".")
}

impl<'a> SqlWriter<'a> {
    pub fn new(dialect: &'a SqlDialect) -> SqlWriter<'a> {
        SqlWriter {
            dialect,
            buf: String::new(),
            need_whitespace: false,
            next_whitespace: " ",
            frame: None,
            list_stack: Vec::new(),
            dynamic_parameters: Vec::new(),
        }
    }

    pub fn dialect(&self) -> &SqlDialect {
        self.dialect
    }

    pub fn into_string(self) -> String {
        self.buf
    }

    pub fn in_query(&self) -> bool {
        match &self.frame {
            None => true,
            Some(f) => matches!(f.kind, FrameType::Select | FrameType::SetOp),
        }
    }

    pub fn newline_and_indent(&mut self) {
        self.buf.push_str(NL);
        self.need_whitespace = false;
    }

    pub fn start_list(&mut self, frame_type: FrameType, open: &'static str, close: &'static str) {
        if let Some(mut parent) = self.frame.take() {
            parent.item_count += 1;
            self.list_stack.push(parent);
        }
        if frame_type == FrameType::FunCall {
            self.need_whitespace = false;
        }
        self.frame = Some(Frame {
            kind: frame_type,
            close,
            item_count: 0,
        });
        if !open.is_empty() {
            self.keyword(open);
        }
    }

    pub fn end_list(&mut self) {
        let Some(ended) = self.frame.take() else {
            return;
        };
        self.keyword(ended.close);
        self.frame = self.list_stack.pop();
    }

    pub fn literal(&mut self, s: &str) {
        self.print(s);
        self.need_whitespace = true;
    }

    pub fn keyword(&mut self, s: &str) {
        self.maybe_whitespace(s);
        self.buf.push_str(&s.to_uppercase());
        if !s.is_empty() {
            self.need_whitespace = need_whitespace_after(s);
        }
    }

    fn maybe_whitespace(&mut self, s: &str) {
        if self.need_whitespace && need_whitespace_before(s) {
            self.white_space();
        }
    }

    fn white_space(&mut self) {
        if self.need_whitespace {
            if self.next_whitespace == NL {
                self.newline_and_indent();
            } else {
                self.buf.push_str(self.next_whitespace);
            }
            self.next_whitespace = " ";
            self.need_whitespace = false;
        }
    }

    pub fn print(&mut self, s: &str) {
        self.maybe_whitespace(s);
        self.buf.push_str(s);
    }

    pub fn print_int(&mut self, x: i32) {
        self.maybe_whitespace("0");
        self.buf.push_str(&x.to_string());
    }

    pub fn identifier(&mut self, name: &str) {
        self.maybe_whitespace(name);
        let quoted = self.dialect.quote_identifier(name);
        self.buf.push_str(&quoted);
        self.need_whitespace = true;
    }

    pub fn dynamic_param(&mut self, index: usize) {
        self.dynamic_parameters.push(index);
        self.print("?");
        self.need_whitespace = true;
    }

    pub fn set_need_whitespace(&mut self, b: bool) {
        self.need_whitespace = b;
    }

    pub fn sep(&mut self, sep: &str) {
        let print_first = !(sep == "," || sep == ".");
        self.sep_with(sep, print_first);
    }

    pub fn sep_with(&mut self, sep: &str, print_first: bool) {
        let Some(mut f) = self.frame.take() else {
            return;
        };
        let comma_list = matches!(
            f.kind,
            FrameType::FromList | FrameType::Join | FrameType::Values
        );
        let (before, after) = if comma_list && sep == "," {
            (false, true)
        } else {
            (f.newline_before_sep(), f.kind == FrameType::SetOp)
        };
        if f.item_count > 0 && before {
            self.newline_and_indent();
        }
        if f.item_count > 0 || print_first {
            self.keyword(sep);
            self.next_whitespace = if after { NL } else { " " };
        }
        f.item_count += 1;
        self.frame = Some(f);
    }

    pub fn start_fun_call(&mut self, name: &str) {
        self.keyword(name);
        self.need_whitespace = false;
        self.start_list(FrameType::FunCall, "(", ")");
    }

    fn list(
        &mut self,
        frame_type: FrameType,
        sep: &str,
        sep_left_prec: i32,
        sep_right_prec: i32,
        list: &[SqlNode],
    ) {
        self.start_list(frame_type, "", "");
        for node in list {
            self.sep_with(sep, false);
            unparse(self, node, sep_right_prec, sep_left_prec);
        }
        self.end_list();
    }
}

pub fn to_sql_string(node: &SqlNode, dialect: &SqlDialect) -> String {
    let mut w = SqlWriter::new(dialect);
    unparse(&mut w, node, 0, 0);
    w.into_string()
}

pub fn unparse(w: &mut SqlWriter<'_>, node: &SqlNode, left_prec: i32, right_prec: i32) {
    match node {
        SqlNode::Identifier(id) => unparse_identifier(w, id),
        SqlNode::Literal(l) => unparse_literal(w, l),
        SqlNode::DynamicParam(i) => w.dynamic_param(*i),
        SqlNode::NodeList(list) => {
            if left_prec > 0 || right_prec > 0 {
                w.start_list(FrameType::Simple, "(", ")");
            } else {
                w.start_list(FrameType::Simple, "", "");
            }
            for n in list {
                w.sep(",");
                unparse(w, n, 0, 0);
            }
            w.end_list();
        }
        SqlNode::Call(call) => unparse_call_node(w, call, left_prec, right_prec),
        SqlNode::Case(c) => {
            if needs_parentheses(&operators::CASE, left_prec, right_prec) {
                w.start_list(FrameType::Simple, "(", ")");
                unparse_case(w, c);
                w.end_list();
            } else {
                unparse_case(w, c);
            }
        }
        SqlNode::Window(win) => unparse_window(w, win),
        SqlNode::Select(select) => unparse_select_node(w, select, left_prec, right_prec),
        SqlNode::DataType(spec) => unparse_data_type(w, spec),
        SqlNode::Text(text) => w.literal(text),
    }
}

fn unparse_identifier(w: &mut SqlWriter<'_>, id: &SqlIdentifier) {
    if let Some(simple) = id.as_simple() {
        let upper = simple.to_uppercase();
        if !simple.is_empty() && FUNCTION_ID_NAMES.contains(&upper.as_str()) {
            w.start_list(FrameType::Identifier, "", "");
            w.keyword(simple);
            w.end_list();
            return;
        }
    }
    w.start_list(FrameType::Identifier, "", "");
    for name in &id.names {
        w.sep(".");
        if name.is_empty() {
            w.print("*");
            w.set_need_whitespace(true);
        } else {
            w.identifier(name);
        }
    }
    w.end_list();
}

fn unparse_literal(w: &mut SqlWriter<'_>, l: &SqlLiteral) {
    match &l.value {
        SqlLiteralValue::Null => w.keyword("NULL"),
        SqlLiteralValue::Boolean(b) => w.keyword(if *b { "TRUE" } else { "FALSE" }),
        SqlLiteralValue::Exact(d) => w.literal(&java_big_decimal_to_plain_string(d)),
        SqlLiteralValue::Approx(d) => w.literal(&super::values::java_big_decimal_to_scientific(d)),
        SqlLiteralValue::Char(s) => {
            let q = w.dialect().quote_string_literal(s);
            w.literal(&q);
        }
        SqlLiteralValue::Date(d) => w.literal(&format!("DATE '{d}'")),
        SqlLiteralValue::Time(t, p) => {
            let p = usize::try_from(*p).unwrap_or(0);
            w.literal(&format!("TIME '{}'", t.to_string_with_precision(p)));
        }
        SqlLiteralValue::Timestamp(t, p) => {
            let p = usize::try_from(*p).unwrap_or(0);
            let ts = if p > 0 { t.round(p) } else { t.clone() };
            w.literal(&format!("TIMESTAMP '{}'", ts.to_string_with_precision(p)));
        }
        SqlLiteralValue::Binary(b) => w.literal(&format!("X'{}'", b.to_hex_string())),
        SqlLiteralValue::Symbol(s) => w.keyword(s),
        SqlLiteralValue::Interval {
            sign,
            text,
            qualifier,
        } => {
            w.keyword("INTERVAL");
            if *sign == -1 {
                w.print("-");
            }
            w.literal(&format!("'{text}'"));
            w.keyword(qualifier);
        }
    }
}

fn needs_parentheses(op: &SqlOperator, left_prec: i32, right_prec: i32) -> bool {
    left_prec > op.left_prec
        || (op.right_prec <= right_prec && right_prec != 0)
        || (op.right_prec <= right_prec + 1 && op.kind.is_unparse_comparison())
}

fn unparse_call_node(w: &mut SqlWriter<'_>, call: &SqlCall, left_prec: i32, right_prec: i32) {
    if needs_parentheses(call.op, left_prec, right_prec) {
        w.start_list(FrameType::Simple, "(", ")");
        dialect_unparse_call(w, call, 0, 0);
        w.end_list();
    } else {
        dialect_unparse_call(w, call, left_prec, right_prec);
    }
}

fn dialect_unparse_call(w: &mut SqlWriter<'_>, call: &SqlCall, left_prec: i32, right_prec: i32) {
    if w.dialect().database_product() == super::sql_dialect::DatabaseProduct::Snowflake
        && call.op.kind == SqlKind::CharLength
    {
        let rewritten = SqlCall {
            op: &operators::LENGTH,
            operands: call.operands.clone(),
            distinct: false,
        };
        unparse_operator(w, &rewritten, left_prec, right_prec);
        return;
    }
    unparse_operator(w, call, left_prec, right_prec);
}

fn unparse_operand(
    w: &mut SqlWriter<'_>,
    call: &SqlCall,
    i: usize,
    left_prec: i32,
    right_prec: i32,
) {
    if let Some(o) = call.operands.get(i) {
        unparse(w, o, left_prec, right_prec);
    }
}

fn unparse_operand_list(w: &mut SqlWriter<'_>, call: &SqlCall) {
    for o in &call.operands {
        w.sep(",");
        unparse(w, o, 0, 0);
    }
}

fn unparse_binary(
    w: &mut SqlWriter<'_>,
    call: &SqlCall,
    frame: FrameType,
    left_prec: i32,
    right_prec: i32,
) {
    let op = call.op;
    w.start_list(frame, "", "");
    unparse_operand(w, call, 0, left_prec, op.left_prec);
    w.set_need_whitespace(true);
    w.sep(op.name);
    w.set_need_whitespace(true);
    unparse_operand(w, call, 1, op.right_prec, right_prec);
    w.end_list();
}

fn unparse_operator(w: &mut SqlWriter<'_>, call: &SqlCall, left_prec: i32, right_prec: i32) {
    if !unparse_special_operator(w, call, left_prec, right_prec) {
        unparse_by_syntax(w, call, left_prec, right_prec);
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one arm per Calcite operator unparse override"
)]
fn unparse_special_operator(
    w: &mut SqlWriter<'_>,
    call: &SqlCall,
    left_prec: i32,
    right_prec: i32,
) -> bool {
    let op = call.op;
    match op.kind {
        SqlKind::As => {
            w.start_list(FrameType::As, "", "");
            unparse_operand(w, call, 0, left_prec, op.left_prec);
            w.set_need_whitespace(true);
            w.sep("AS");
            w.set_need_whitespace(true);
            unparse_operand(w, call, 1, op.right_prec, right_prec);
            if call.operands.len() > 2 {
                w.start_list(FrameType::Simple, "(", ")");
                for o in &call.operands[2..] {
                    w.sep_with(",", false);
                    unparse(w, o, 0, 0);
                }
                w.end_list();
            }
            w.end_list();
            true
        }
        SqlKind::Cast | SqlKind::SafeCast | SqlKind::Extract => {
            w.start_fun_call(op.name);
            unparse_operand(w, call, 0, 0, 0);
            w.sep(if op.kind == SqlKind::Extract {
                "FROM"
            } else {
                "AS"
            });
            unparse_operand(w, call, 1, 0, 0);
            w.end_list();
            true
        }
        SqlKind::Like => {
            w.start_list(FrameType::Simple, "", "");
            unparse_operand(w, call, 0, op.left_prec, op.right_prec);
            w.sep(op.name);
            unparse_operand(w, call, 1, op.left_prec, op.right_prec);
            if let Some(o2) = call.operands.get(2) {
                w.sep("ESCAPE");
                unparse(w, o2, op.left_prec, op.right_prec);
            }
            w.end_list();
            true
        }
        SqlKind::Between => {
            w.start_list(FrameType::Simple, "", "");
            unparse_operand(w, call, 0, op.left_prec, 0);
            w.sep(op.name);
            unparse_operand(w, call, 1, 0, 0);
            w.sep("AND");
            unparse_operand(w, call, 2, 0, op.right_prec);
            w.end_list();
            true
        }
        SqlKind::Trim => {
            w.start_fun_call(op.name);
            for (i, o) in call.operands.iter().enumerate() {
                if i == 2 {
                    w.sep("FROM");
                }
                unparse(w, o, 0, 0);
            }
            w.end_list();
            true
        }
        SqlKind::ArrayValueConstructor => {
            w.keyword(op.name);
            w.start_list(FrameType::Simple, "[", "]");
            unparse_operand_list(w, call);
            w.end_list();
            true
        }
        SqlKind::Plus | SqlKind::Minus if op.syntax == SqlSyntax::Special => {
            w.start_list(FrameType::Simple, "(", ")");
            unparse_operand(w, call, 0, left_prec, right_prec);
            w.sep(op.name);
            unparse_operand(w, call, 1, left_prec, right_prec);
            w.end_list();
            for o in call.operands.iter().skip(2) {
                unparse(w, o, left_prec, right_prec);
            }
            true
        }
        SqlKind::Join => {
            w.start_list(FrameType::Join, "", "");
            unparse_operand(w, call, 0, left_prec, op.left_prec);
            w.sep_with(op.name, true);
            unparse_operand(w, call, 1, op.right_prec, right_prec);
            if let Some(condition) = call.operands.get(2) {
                w.keyword("ON");
                unparse(w, condition, left_prec, right_prec);
            }
            w.end_list();
            true
        }
        SqlKind::Union | SqlKind::Intersect | SqlKind::Except => {
            unparse_binary(w, call, FrameType::SetOp, left_prec, right_prec);
            true
        }
        SqlKind::ScalarQuery => {
            w.start_list(FrameType::Simple, "(", ")");
            unparse_operand(w, call, 0, 0, 0);
            w.end_list();
            true
        }
        SqlKind::Rollup | SqlKind::Cube | SqlKind::GroupingSets => {
            w.keyword(op.name);
            w.start_list(FrameType::FunCall, "(", ")");
            for o in &call.operands {
                w.sep(",");
                match o {
                    SqlNode::Call(row) if row.op.kind == SqlKind::Row => {
                        w.start_list(FrameType::Simple, "(", ")");
                        for o2 in &row.operands {
                            w.sep(",");
                            unparse(w, o2, 0, 0);
                        }
                        w.end_list();
                    }
                    SqlNode::NodeList(l) if l.is_empty() => w.keyword("()"),
                    other => unparse(w, other, 0, 0),
                }
            }
            w.end_list();
            true
        }
        SqlKind::Values => {
            w.start_list(FrameType::Values, "VALUES", "");
            unparse_operand_list(w, call);
            w.end_list();
            true
        }
        SqlKind::Row => {
            w.start_list(FrameType::Simple, "(", ")");
            unparse_operand_list(w, call);
            w.end_list();
            true
        }
        _ => false,
    }
}

fn unparse_by_syntax(w: &mut SqlWriter<'_>, call: &SqlCall, left_prec: i32, right_prec: i32) {
    let op = call.op;
    match op.syntax {
        SqlSyntax::Binary => unparse_binary(w, call, FrameType::Simple, left_prec, right_prec),
        SqlSyntax::Prefix => {
            w.keyword(op.name);
            unparse_operand(w, call, 0, op.left_prec, op.right_prec);
        }
        SqlSyntax::Postfix => {
            unparse_operand(w, call, 0, op.left_prec, op.right_prec);
            w.keyword(op.name);
        }
        SqlSyntax::FunctionId if call.operands.is_empty() => {
            w.keyword(op.name);
        }
        _ => {
            w.keyword(op.name);
            w.start_list(FrameType::FunCall, "(", ")");
            if call.distinct {
                w.keyword("DISTINCT");
            }
            if call.operands.is_empty() && op.syntax == SqlSyntax::FunctionStar {
                w.sep("*");
            }
            unparse_operand_list(w, call);
            w.end_list();
        }
    }
}

fn unparse_case(w: &mut SqlWriter<'_>, c: &SqlCase) {
    w.start_list(FrameType::Case, "CASE", "END");
    if let Some(v) = &c.value {
        unparse(w, v, 0, 0);
    }
    for (when, then) in c.when_list.iter().zip(c.then_list.iter()) {
        w.sep("WHEN");
        unparse(w, when, 0, 0);
        w.sep("THEN");
        unparse(w, then, 0, 0);
    }
    if let Some(e) = &c.else_expr {
        w.sep("ELSE");
        unparse(w, e, 0, 0);
    }
    w.end_list();
}

fn unparse_window(w: &mut SqlWriter<'_>, win: &SqlWindow) {
    w.start_list(FrameType::Window, "(", ")");
    if !win.partition_list.is_empty() {
        w.sep("PARTITION BY");
        w.start_list(FrameType::Simple, "", "");
        unparse(w, &SqlNode::NodeList(win.partition_list.clone()), 0, 0);
        w.end_list();
    }
    if !win.order_list.is_empty() {
        w.sep("ORDER BY");
        w.start_list(FrameType::Simple, "", "");
        unparse(w, &SqlNode::NodeList(win.order_list.clone()), 0, 0);
        w.end_list();
    }
    match (&win.lower_bound, &win.upper_bound) {
        (None, _) => {}
        (Some(lower), None) => {
            w.sep(if win.is_rows { "ROWS" } else { "RANGE" });
            unparse(w, lower, 0, 0);
        }
        (Some(lower), Some(upper)) => {
            w.sep(if win.is_rows {
                "ROWS BETWEEN"
            } else {
                "RANGE BETWEEN"
            });
            unparse(w, lower, 0, 0);
            w.keyword("AND");
            unparse(w, upper, 0, 0);
        }
    }
    w.end_list();
}

fn unparse_data_type(w: &mut SqlWriter<'_>, spec: &SqlDataTypeSpec) {
    if spec.alien {
        w.keyword(&spec.type_name);
        return;
    }
    w.keyword(&spec.type_name);
    if spec.precision >= 0 {
        w.start_list(FrameType::FunCall, "(", ")");
        w.print_int(spec.precision);
        if spec.scale != i32::MIN {
            w.sep_with(",", true);
            w.print_int(spec.scale);
        }
        w.end_list();
    }
    if let Some(cs) = &spec.charset {
        w.keyword("CHARACTER SET");
        w.identifier(cs);
    }
    if let Some(coll) = &spec.collection {
        w.keyword(coll);
    }
}

fn unparse_select_node(w: &mut SqlWriter<'_>, select: &SqlSelect, left_prec: i32, right_prec: i32) {
    let wrap = !w.in_query()
        || (select.fetch.is_some() && (left_prec > FETCH_PREC || right_prec > FETCH_PREC))
        || (select.offset.is_some() && (left_prec > FETCH_PREC || right_prec > FETCH_PREC))
        || (select.order_by.is_some()
            && (left_prec > ORDER_BY_LEFT_PREC || right_prec > ORDER_BY_RIGHT_PREC));
    if wrap {
        w.start_list(FrameType::SubQuery, "(", ")");
        unparse_select(w, select);
        w.end_list();
    } else {
        unparse_select(w, select);
    }
}

fn flatten_where(node: &SqlNode, kind: SqlKind) -> Vec<SqlNode> {
    let mut list = Vec::new();
    let mut current = node.clone();
    loop {
        match &current {
            SqlNode::Call(c) if c.op.kind == kind && c.operands.len() == 2 => {
                list.insert(0, c.operands[1].clone());
                let next = c.operands[0].clone();
                current = next;
            }
            _ => break,
        }
    }
    list.insert(0, current);
    list
}

fn unparse_select(w: &mut SqlWriter<'_>, select: &SqlSelect) {
    w.start_list(FrameType::Select, "", "");
    w.sep("SELECT");
    for k in &select.keyword_list {
        unparse(w, k, 0, 0);
    }
    w.list(
        FrameType::SelectList,
        ",",
        COMMA_LEFT_PREC,
        COMMA_RIGHT_PREC,
        &select.select_list,
    );
    if let Some(from) = &select.from {
        w.sep("FROM");
        w.start_list(FrameType::FromList, "", "");
        unparse(w, from, JOIN_LEFT_PREC - 1, JOIN_RIGHT_PREC - 1);
        w.end_list();
    }
    if let Some(where_clause) = &select.where_clause {
        w.sep("WHERE");
        let (kind, sep_op) = match where_clause {
            SqlNode::Call(c) if c.op.kind == SqlKind::Or => (SqlKind::Or, &operators::OR),
            _ => (SqlKind::And, &operators::AND),
        };
        let list = flatten_where(where_clause, kind);
        w.list(
            FrameType::WhereList,
            sep_op.name,
            sep_op.left_prec,
            sep_op.right_prec,
            &list,
        );
    }
    if let Some(group_by) = &select.group_by {
        w.sep("GROUP BY");
        let singleton;
        let list = if group_by.is_empty() {
            singleton = [SqlNode::NodeList(Vec::new())];
            &singleton[..]
        } else {
            &group_by[..]
        };
        w.list(
            FrameType::GroupByList,
            ",",
            COMMA_LEFT_PREC,
            COMMA_RIGHT_PREC,
            list,
        );
    }
    if let Some(having) = &select.having {
        w.sep("HAVING");
        unparse(w, having, 0, 0);
    }
    if let Some(q) = &select.qualify {
        w.sep("QUALIFY");
        unparse(w, q, 0, 0);
    }
    if let Some(order_by) = &select.order_by {
        if !order_by.is_empty() {
            w.sep("ORDER BY");
            w.list(
                FrameType::OrderByList,
                ",",
                COMMA_LEFT_PREC,
                COMMA_RIGHT_PREC,
                order_by,
            );
        }
    }
    unparse_fetch_offset(w, select);
    w.end_list();
}

fn unparse_fetch_offset(w: &mut SqlWriter<'_>, select: &SqlSelect) {
    if select.fetch.is_some() || select.offset.is_some() {
        if w.dialect().uses_limit_for_fetch() {
            if let Some(fetch) = &select.fetch {
                w.newline_and_indent();
                w.start_list(FrameType::Fetch, "", "");
                w.keyword("LIMIT");
                unparse(w, fetch, -1, -1);
                w.end_list();
            }
            if let Some(offset) = &select.offset {
                w.newline_and_indent();
                w.start_list(FrameType::Offset, "", "");
                w.keyword("OFFSET");
                unparse(w, offset, -1, -1);
                w.end_list();
            }
        } else {
            if let Some(offset) = &select.offset {
                w.newline_and_indent();
                w.start_list(FrameType::Offset, "", "");
                w.keyword("OFFSET");
                unparse(w, offset, -1, -1);
                w.keyword("ROWS");
                w.end_list();
            }
            if let Some(fetch) = &select.fetch {
                w.newline_and_indent();
                w.start_list(FrameType::Fetch, "", "");
                w.keyword("FETCH");
                w.keyword("NEXT");
                unparse(w, fetch, -1, -1);
                w.keyword("ROWS");
                w.keyword("ONLY");
                w.end_list();
            }
        }
    }
}
