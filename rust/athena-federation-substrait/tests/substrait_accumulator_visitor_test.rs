use athena_federation_substrait::ErrorKind;
use athena_federation_substrait::SqlTypeName;
use athena_federation_substrait::SubstraitAccumulatorVisitor;
use athena_federation_substrait::{operators, SqlKind, SqlOperator};
use athena_federation_substrait::{plan, SqlDialect};
use athena_federation_substrait::{RelDataType, RelDataTypeSystem};
use athena_federation_substrait::{SqlIdentifier, SqlLiteral, SqlNode, SqlSelect};
use athena_federation_substrait::{SubstraitTypeAndValue, SubstraitValue};

fn struct_type(fields: &[(&str, SqlTypeName)]) -> RelDataType {
    RelDataType::row(
        fields
            .iter()
            .map(|(n, t)| {
                (
                    n.to_string(),
                    RelDataType::create_sql_type(RelDataTypeSystem::Default, *t),
                )
            })
            .collect(),
        false,
    )
}

fn schema() -> RelDataType {
    struct_type(&[
        ("int_col", SqlTypeName::Integer),
        ("bigint_col", SqlTypeName::Bigint),
        ("varchar_col", SqlTypeName::Varchar),
        ("bool_col", SqlTypeName::Boolean),
        ("float_col", SqlTypeName::Float),
    ])
}

fn id(name: &str) -> SqlNode {
    SqlNode::identifier(name)
}

fn compound(names: &[&str]) -> SqlNode {
    SqlNode::Identifier(SqlIdentifier::new(
        names.iter().map(std::string::ToString::to_string).collect(),
    ))
}

fn num(v: &str) -> SqlNode {
    SqlNode::literal(SqlLiteral::exact(v))
}

fn chars(v: &str) -> SqlNode {
    SqlNode::literal(SqlLiteral::char_string(v))
}

fn call(op: &'static SqlOperator, operands: Vec<SqlNode>) -> SqlNode {
    SqlNode::call(op, operands)
}

fn run(schema: &RelDataType, node: &SqlNode) -> (SqlNode, Vec<SubstraitTypeAndValue>) {
    let mut accumulator = Vec::new();
    let result = SubstraitAccumulatorVisitor::new(&mut accumulator, schema)
        .visit(node)
        .expect("visit");
    (result, accumulator)
}

fn operand(node: &SqlNode, i: usize) -> &SqlNode {
    &node.as_call().expect("call").operands[i]
}

fn select_where(where_clause: SqlNode) -> SqlNode {
    SqlNode::Select(Box::new(SqlSelect {
        select_list: vec![SqlNode::Identifier(SqlIdentifier::simple("*"))],
        from: Some(id("test_table")),
        where_clause: Some(where_clause),
        ..SqlSelect::default()
    }))
}

fn where_of(node: &SqlNode) -> &SqlNode {
    node.as_select()
        .and_then(SqlSelect::get_where)
        .expect("where")
}

#[test]
fn test_visit_simple_identifier() {
    let node = id("int_col");
    assert_eq!(run(&schema(), &node).0, node);
}

#[test]
fn test_visit_non_simple_identifier() {
    let node = compound(&["s", "int_col"]);
    assert_eq!(run(&schema(), &node).0, node);
}

#[test]
fn test_visit_standalone_literal_skipped() {
    let node = chars("test");
    let (result, acc) = run(&schema(), &node);
    assert_eq!(result, node);
    assert_eq!(acc.len(), 0);
}

#[test]
fn test_accumulator_non_nls_string_value() {
    let (_, acc) = run(
        &schema(),
        &call(&operators::EQUALS, vec![id("int_col"), num("123")]),
    );
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].get_type(), SqlTypeName::Integer);
    assert_eq!(acc[0].column_name(), "int_col");
}

#[test]
fn test_accumulator_nls_string_value() {
    let (_, acc) = run(
        &schema(),
        &call(&operators::EQUALS, vec![id("varchar_col"), chars("hello")]),
    );
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].get_type(), SqlTypeName::Varchar);
    assert_eq!(acc[0].value(), &SubstraitValue::String("hello".to_string()));
}

#[test]
fn test_accumulator_field_not_found_throws() {
    let s = schema();
    let mut accumulator = Vec::new();
    let err = SubstraitAccumulatorVisitor::new(&mut accumulator, &s)
        .visit(&call(
            &operators::EQUALS,
            vec![id("no_such_col"), chars("x")],
        ))
        .expect_err("missing field");
    assert_eq!(err.kind(), ErrorKind::IllegalArgument);
    assert!(err.message().contains("field no_such_col not found"));
    assert_eq!(
        err.message(),
        "field no_such_col not found in schema with fields: [int_col, bigint_col, varchar_col, bool_col, float_col]"
    );
}

#[test]
fn test_accumulator_null_value_throws() {
    let null_schema = struct_type(&[("null_col", SqlTypeName::Null)]);
    let mut accumulator = Vec::new();
    let err = SubstraitAccumulatorVisitor::new(&mut accumulator, &null_schema)
        .visit(&call(
            &operators::EQUALS,
            vec![id("null_col"), SqlNode::literal(SqlLiteral::null())],
        ))
        .expect_err("null value");
    assert_eq!(err.kind(), ErrorKind::NullPointer);
    assert!(err.message().contains("value is null"));
}

#[test]
fn test_binary_comparison_left_id_right_lit() {
    let ops: [&'static SqlOperator; 6] = [
        &operators::EQUALS,
        &operators::NOT_EQUALS,
        &operators::GREATER_THAN,
        &operators::LESS_THAN,
        &operators::GREATER_THAN_OR_EQUAL,
        &operators::LESS_THAN_OR_EQUAL,
    ];
    for op in ops {
        let (result, acc) = run(&schema(), &call(op, vec![id("int_col"), num("42")]));
        assert_eq!(acc.len(), 1, "{}", op.name);
        assert!(
            matches!(operand(&result, 1), SqlNode::DynamicParam(0)),
            "{}",
            op.name
        );
    }
}

#[test]
fn test_binary_comparison_reversed_operands() {
    let (result, acc) = run(
        &schema(),
        &call(&operators::EQUALS, vec![num("99"), id("int_col")]),
    );
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].column_name(), "int_col");
    assert!(matches!(operand(&result, 0), SqlNode::DynamicParam(_)));
}

#[test]
fn test_binary_comparison_non_simple_id_falls_through() {
    let (_, acc) = run(
        &schema(),
        &call(
            &operators::EQUALS,
            vec![compound(&["s", "int_col"]), num("1")],
        ),
    );
    assert_eq!(acc.len(), 0);
}

#[test]
fn test_binary_comparison_no_match_falls_through() {
    let (_, acc) = run(
        &schema(),
        &call(&operators::EQUALS, vec![id("int_col"), id("bigint_col")]),
    );
    assert_eq!(acc.len(), 0);
}

#[test]
fn test_in_with_literals() {
    let node = call(
        &operators::IN,
        vec![id("int_col"), SqlNode::NodeList(vec![num("1"), num("2")])],
    );
    assert_eq!(run(&schema(), &node).1.len(), 2);
}

#[test]
fn test_in_first_operand_not_identifier() {
    let node = call(
        &operators::IN,
        vec![num("0"), SqlNode::NodeList(vec![num("1")])],
    );
    assert_eq!(run(&schema(), &node).1.len(), 0);
}

#[test]
fn test_in_non_simple_identifier() {
    let node = call(
        &operators::IN,
        vec![
            compound(&["s", "int_col"]),
            SqlNode::NodeList(vec![num("1")]),
        ],
    );
    assert_eq!(run(&schema(), &node).1.len(), 0);
}

#[test]
fn test_in_mixed_literal_and_non_literal() {
    let node = call(
        &operators::IN,
        vec![
            id("int_col"),
            SqlNode::NodeList(vec![num("1"), id("bigint_col"), num("3")]),
        ],
    );
    assert_eq!(run(&schema(), &node).1.len(), 2);
}

#[test]
fn test_between_bound_combinations() {
    let cases = [
        (num("10"), num("20"), 2, "both_bounds_literal"),
        (num("10"), id("bigint_col"), 1, "only_lower_literal"),
        (id("bigint_col"), num("20"), 1, "only_upper_literal"),
        (
            id("bigint_col"),
            id("float_col"),
            0,
            "neither_bound_literal",
        ),
    ];
    for (lower, upper, expected, name) in cases {
        let node = call(&operators::BETWEEN, vec![id("int_col"), lower, upper]);
        assert_eq!(run(&schema(), &node).1.len(), expected, "{name}");
    }
}

#[test]
fn test_between_first_operand_not_identifier() {
    let node = call(&operators::BETWEEN, vec![num("5"), num("1"), num("10")]);
    assert_eq!(run(&schema(), &node).1.len(), 0);
}

#[test]
fn test_between_non_simple_identifier() {
    let node = call(
        &operators::BETWEEN,
        vec![compound(&["s", "int_col"]), num("1"), num("10")],
    );
    assert_eq!(run(&schema(), &node).1.len(), 0);
}

#[test]
fn test_like_basic() {
    let (_, acc) = run(
        &schema(),
        &call(&operators::LIKE, vec![id("varchar_col"), chars("%test%")]),
    );
    assert_eq!(acc.len(), 1);
    assert_eq!(
        acc[0].value(),
        &SubstraitValue::String("%test%".to_string())
    );
}

#[test]
fn test_like_with_escape_character() {
    let node = call(
        &operators::LIKE,
        vec![id("varchar_col"), chars("100\\%%"), chars("\\")],
    );
    let (result, acc) = run(&schema(), &node);
    assert_eq!(acc.len(), 1);
    assert_eq!(result.as_call().expect("call").operands.len(), 3);
}

#[test]
fn test_like_fallthrough_cases() {
    let cases = [
        (chars("val"), chars("%t%"), "first_operand_not_identifier"),
        (
            id("varchar_col"),
            id("varchar_col"),
            "second_operand_not_literal",
        ),
        (
            compound(&["s", "varchar_col"]),
            chars("%t%"),
            "non_simple_identifier",
        ),
    ];
    for (first, second, name) in cases {
        let node = call(&operators::LIKE, vec![first, second]);
        assert_eq!(run(&schema(), &node).1.len(), 0, "like_fallthrough_{name}");
    }
}

#[test]
fn test_not_on_boolean_column() {
    let select = select_where(call(&operators::NOT, vec![id("bool_col")]));
    let (result, acc) = run(&schema(), &select);
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].get_type(), SqlTypeName::Boolean);
    assert_eq!(acc[0].value(), &SubstraitValue::Boolean(false));
    assert_eq!(where_of(&result).kind(), SqlKind::Equals);
}

#[test]
fn test_standalone_boolean_column() {
    let select = select_where(id("bool_col"));
    let (result, acc) = run(&schema(), &select);
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].get_type(), SqlTypeName::Boolean);
    assert_eq!(acc[0].value(), &SubstraitValue::Boolean(true));
    assert_eq!(acc[0].column_name(), "bool_col");
    let w = where_of(&result);
    assert_eq!(w.kind(), SqlKind::Equals);
    assert!(matches!(operand(w, 0), SqlNode::Identifier(_)));
    assert!(matches!(operand(w, 1), SqlNode::DynamicParam(_)));
    assert_eq!(
        operand(w, 0)
            .as_identifier()
            .and_then(SqlIdentifier::get_simple),
        Some("bool_col")
    );
}

#[test]
fn test_boolean_column_in_and_operator() {
    let int_cmp = call(&operators::EQUALS, vec![id("int_col"), num("1")]);
    let select = select_where(call(&operators::AND, vec![id("bool_col"), int_cmp]));
    let (result, acc) = run(&schema(), &select);
    assert_eq!(acc.len(), 2);
    assert_eq!(acc[0].get_type(), SqlTypeName::Boolean);
    assert_eq!(acc[0].value(), &SubstraitValue::Boolean(true));
    assert_eq!(acc[0].column_name(), "bool_col");
    assert_eq!(acc[1].get_type(), SqlTypeName::Integer);
    let w = where_of(&result);
    assert_eq!(w.kind(), SqlKind::And);
    assert!(matches!(operand(w, 0), SqlNode::Call(_)));
    assert!(matches!(operand(w, 1), SqlNode::Call(_)));
    assert_eq!(operand(w, 0).kind(), SqlKind::Equals);
}

#[test]
fn test_boolean_column_in_or_operator() {
    let int_cmp = call(&operators::EQUALS, vec![id("int_col"), num("1")]);
    let select = select_where(call(&operators::OR, vec![id("bool_col"), int_cmp]));
    let (result, acc) = run(&schema(), &select);
    assert_eq!(acc.len(), 2);
    assert_eq!(acc[0].get_type(), SqlTypeName::Boolean);
    assert_eq!(acc[0].value(), &SubstraitValue::Boolean(true));
    assert_eq!(where_of(&result).kind(), SqlKind::Or);
}

#[test]
fn test_explicit_boolean_comparison() {
    let eq = call(
        &operators::EQUALS,
        vec![id("bool_col"), SqlNode::literal(SqlLiteral::boolean(true))],
    );
    let (result, acc) = run(&schema(), &select_where(eq));
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].get_type(), SqlTypeName::Boolean);
    assert_eq!(acc[0].value(), &SubstraitValue::Boolean(true));
    assert_eq!(where_of(&result).kind(), SqlKind::Equals);
}

#[test]
fn test_not_fallthrough_cases() {
    let cases = [
        (id("int_col"), "non_boolean_column"),
        (compound(&["s", "bool_col"]), "non_simple_identifier"),
        (id("no_such_col"), "non_existent_column"),
    ];
    for (operand, name) in cases {
        let node = call(&operators::NOT, vec![operand]);
        assert_eq!(run(&schema(), &node).1.len(), 0, "not_fallthrough_{name}");
    }
}

#[test]
fn test_not_on_non_identifier_operand() {
    let inner = call(&operators::EQUALS, vec![id("int_col"), num("1")]);
    assert_eq!(
        run(&schema(), &call(&operators::NOT, vec![inner])).1.len(),
        1
    );
}

#[test]
fn test_unrecognized_call_kind_falls_through() {
    let eq1 = call(&operators::EQUALS, vec![id("int_col"), num("1")]);
    let eq2 = call(&operators::EQUALS, vec![id("varchar_col"), chars("x")]);
    assert_eq!(
        run(&schema(), &call(&operators::AND, vec![eq1, eq2]))
            .1
            .len(),
        2
    );
}

#[test]
fn test_visit_like_with_concat() {
    let s = struct_type(&[("cc_call_center_id", SqlTypeName::Varchar)]);
    let concat = call(
        &operators::CONCAT,
        vec![id("cc_call_center_id"), chars("a")],
    );
    let (_, acc) = run(
        &s,
        &call(&operators::LIKE, vec![id("cc_call_center_id"), concat]),
    );
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].get_type(), SqlTypeName::Varchar);
    assert_eq!(acc[0].value(), &SubstraitValue::String("a".to_string()));
    assert_eq!(acc[0].column_name(), "cc_call_center_id");
}

#[test]
fn scalar_subquery_binds_each_literal_once() {
    let plan = include_str!("fixtures/scalar-subquery-bool-where.plan").trim();
    let dialect = SqlDialect::ansi();
    let node = plan::to_sql_node(plan, &dialect).unwrap();
    let schema = plan::table_schema(plan, &dialect).unwrap();
    let mut accumulator = Vec::new();
    let rewritten = SubstraitAccumulatorVisitor::new(&mut accumulator, &schema)
        .visit(&node)
        .unwrap();
    let sql = rewritten.to_sql_string(&dialect);
    assert!(sql.ends_with("WHERE `bool_col` = ?))))"), "{sql}");
    assert!(!sql.contains("(`bool_col` = ?) = ?"), "{sql}");
    assert_eq!(1, accumulator.len());
    assert_eq!(SqlTypeName::Boolean, accumulator[0].get_type());
    assert_eq!(&SubstraitValue::Boolean(true), accumulator[0].value());
    assert_eq!("bool_col", accumulator[0].column_name());
}

fn extended_plan(query: &str) -> String {
    let mut lines = include_str!("fixtures/calcite-extended.tsv").lines();
    while let Some(line) = lines.next() {
        if line == format!("QUERY\tstd\t{query}") {
            return lines
                .next()
                .and_then(|l| l.strip_prefix("PLAN\t"))
                .expect("plan row")
                .to_string();
        }
    }
    panic!("no fixture plan for {query}");
}

fn parameterize(plan: &str) -> (String, Vec<SubstraitTypeAndValue>) {
    let dialect = SqlDialect::ansi();
    let node = plan::to_sql_node(plan, &dialect).unwrap();
    let schema = plan::table_schema(plan, &dialect).unwrap();
    let mut accumulator = Vec::new();
    let rewritten = SubstraitAccumulatorVisitor::new(&mut accumulator, &schema)
        .visit(&node)
        .unwrap();
    (rewritten.to_sql_string(&dialect), accumulator)
}

#[test]
fn test_scalar_subquery_where_visited_once() {
    let (sql, accumulator) = parameterize(&extended_plan(
        "SELECT * FROM test_table WHERE int_col = (SELECT MIN(int_col) FROM test_table WHERE bool_col AND bigint_col = 3)",
    ));
    assert!(
        sql.ends_with("WHERE `bool_col` = ? AND `bigint_col` = ?)))"),
        "{sql}"
    );
    assert_eq!(2, accumulator.len());
    assert_eq!(SqlTypeName::Boolean, accumulator[0].get_type());
    assert_eq!(&SubstraitValue::Boolean(true), accumulator[0].value());
    assert_eq!("bool_col", accumulator[0].column_name());
    assert_eq!(SqlTypeName::Bigint, accumulator[1].get_type());
    assert_eq!("3", accumulator[1].value().to_string());
    assert_eq!("bigint_col", accumulator[1].column_name());
}

#[test]
fn scalar_subquery_not_bool_with_literal_binds_once() {
    let (sql, accumulator) = parameterize(&extended_plan(
        "SELECT * FROM test_table WHERE int_col = (SELECT MIN(int_col) FROM test_table WHERE NOT bool_col AND bigint_col = 3)",
    ));
    assert!(
        sql.ends_with("WHERE `bigint_col` = ? AND `bool_col` = ?)))"),
        "{sql}"
    );
    assert_eq!(2, accumulator.len());
    assert_eq!(SqlTypeName::Bigint, accumulator[0].get_type());
    assert_eq!("3", accumulator[0].value().to_string());
    assert_eq!("bigint_col", accumulator[0].column_name());
    assert_eq!(SqlTypeName::Boolean, accumulator[1].get_type());
    assert_eq!(&SubstraitValue::Boolean(false), accumulator[1].value());
    assert_eq!("bool_col", accumulator[1].column_name());
}
