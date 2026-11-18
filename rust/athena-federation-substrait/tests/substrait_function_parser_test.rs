use arrow_schema::DataType;
use athena_federation_substrait::predicate::{self, function_names};
use athena_federation_substrait::proto::expression::field_reference::{
    ReferenceType, RootReference, RootType,
};
use athena_federation_substrait::proto::expression::literal::LiteralType;
use athena_federation_substrait::proto::expression::reference_segment::{self, StructField};
use athena_federation_substrait::proto::expression::{
    FieldReference, Literal, ReferenceSegment, RexType, ScalarFunction,
};
use athena_federation_substrait::proto::extensions::simple_extension_declaration::{
    ExtensionFunction, MappingType,
};
use athena_federation_substrait::proto::extensions::SimpleExtensionDeclaration;
use athena_federation_substrait::proto::function_argument::ArgType;
use athena_federation_substrait::proto::{Expression, FunctionArgument};
use athena_federation_substrait::LiteralValue;
use athena_federation_substrait::SubstraitError;
use athena_federation_substrait::SubstraitOperator;
use athena_federation_substrait::{ColumnPredicate, PredicateValue};

fn column_names() -> Vec<String> {
    ["id", "name", "age", "active"]
        .iter()
        .map(std::string::ToString::to_string)
        .collect()
}

fn create_extension_declaration(anchor: u32, function_name: &str) -> SimpleExtensionDeclaration {
    SimpleExtensionDeclaration {
        mapping_type: Some(MappingType::ExtensionFunction(ExtensionFunction {
            function_anchor: anchor,
            name: function_name.to_string(),
            ..Default::default()
        })),
    }
}

fn arg(e: Expression) -> FunctionArgument {
    FunctionArgument {
        arg_type: Some(ArgType::Value(e)),
    }
}

fn scalar(function_ref: u32, args: Vec<Expression>) -> Expression {
    Expression {
        rex_type: Some(RexType::ScalarFunction(ScalarFunction {
            function_reference: function_ref,
            arguments: args.into_iter().map(arg).collect(),
            ..Default::default()
        })),
    }
}

fn create_field_reference(field_index: i32) -> Expression {
    Expression {
        rex_type: Some(RexType::Selection(Box::new(FieldReference {
            reference_type: Some(ReferenceType::DirectReference(ReferenceSegment {
                reference_type: Some(reference_segment::ReferenceType::StructField(Box::new(
                    StructField {
                        field: field_index,
                        child: None,
                    },
                ))),
            })),
            root_type: Some(RootType::RootReference(RootReference {})),
        }))),
    }
}

fn literal(literal_type: LiteralType) -> Expression {
    Expression {
        rex_type: Some(RexType::Literal(Literal {
            literal_type: Some(literal_type),
            ..Default::default()
        })),
    }
}

fn create_binary_expression(function_ref: u32, field_index: i32, value: i32) -> Expression {
    scalar(
        function_ref,
        vec![
            create_field_reference(field_index),
            literal(LiteralType::I32(value)),
        ],
    )
}

fn create_binary_expression_with_string(
    function_ref: u32,
    field_index: i32,
    value: &str,
) -> Expression {
    scalar(
        function_ref,
        vec![
            create_field_reference(field_index),
            literal(LiteralType::String(value.to_string())),
        ],
    )
}

fn create_binary_expression_with_boolean(
    function_ref: u32,
    field_index: i32,
    value: bool,
) -> Expression {
    scalar(
        function_ref,
        vec![
            create_field_reference(field_index),
            literal(LiteralType::Boolean(value)),
        ],
    )
}

fn create_unary_expression(function_ref: u32, field_index: i32) -> Expression {
    scalar(function_ref, vec![create_field_reference(field_index)])
}

fn create_not_expression(function_ref: u32, inner: Expression) -> Expression {
    scalar(function_ref, vec![inner])
}

fn create_logical_expression(function_ref: u32, left: Expression, right: Expression) -> Expression {
    scalar(function_ref, vec![left, right])
}

fn int_value(v: i32) -> PredicateValue {
    PredicateValue::Literal(LiteralValue::Int(v))
}

fn string_value(v: &str) -> PredicateValue {
    PredicateValue::Literal(LiteralValue::String(v.to_string()))
}

fn list_len(p: &ColumnPredicate) -> usize {
    match p.value() {
        Some(PredicateValue::Predicates(v)) => v.len(),
        Some(PredicateValue::Values(v)) => v.len(),
        other => panic!("expected a list value, got {other:?}"),
    }
}

#[test]
fn test_get_column_predicates_map_with_single_predicate() {
    let extensions = vec![create_extension_declaration(
        1,
        function_names::EQUAL_ANY_ANY,
    )];
    let expression = create_binary_expression(1, 0, 123);
    let result =
        predicate::column_predicates_map(&extensions, &expression, &column_names()).unwrap();
    assert_eq!(1, result.len());
    assert!(result.contains_key(&Some("id".to_string())));
    assert_eq!(1, result[&Some("id".to_string())].len());
    let predicate = &result[&Some("id".to_string())][0];
    assert_eq!(Some("id"), predicate.column());
    assert_eq!(SubstraitOperator::Equal, predicate.operator());
    assert_eq!(Some(&int_value(123)), predicate.value());
}

#[test]
fn test_get_column_predicates_map_with_multiple_predicates() {
    let extensions = vec![
        create_extension_declaration(1, "equal:any_any"),
        create_extension_declaration(2, "gt:any_any"),
        create_extension_declaration(3, "and:bool"),
    ];
    let and_expression = create_logical_expression(
        3,
        create_binary_expression(1, 0, 123),
        create_binary_expression(2, 2, 18),
    );
    let result =
        predicate::column_predicates_map(&extensions, &and_expression, &column_names()).unwrap();
    assert_eq!(2, result.len());
    assert!(result.contains_key(&Some("id".to_string())));
    assert!(result.contains_key(&Some("age".to_string())));
    let id_predicate = &result[&Some("id".to_string())][0];
    assert_eq!(SubstraitOperator::Equal, id_predicate.operator());
    assert_eq!(Some(&int_value(123)), id_predicate.value());
    let age_predicate = &result[&Some("age".to_string())][0];
    assert_eq!(SubstraitOperator::GreaterThan, age_predicate.operator());
    assert_eq!(Some(&int_value(18)), age_predicate.value());
}

#[test]
fn test_parse_column_predicates_with_unary_operator() {
    let extensions = vec![create_extension_declaration(1, "is_null:any")];
    let expression = create_unary_expression(1, 1);
    let result = predicate::column_predicates(&extensions, &expression, &column_names()).unwrap();
    assert_eq!(1, result.len());
    assert_eq!(Some("name"), result[0].column());
    assert_eq!(SubstraitOperator::IsNull, result[0].operator());
    assert!(result[0].value().is_none());
}

#[test]
fn test_parse_column_predicates_with_string_literal() {
    let extensions = vec![create_extension_declaration(1, "equal:any_any")];
    let expression = create_binary_expression_with_string(1, 1, "John");
    let result = predicate::column_predicates(&extensions, &expression, &column_names()).unwrap();
    assert_eq!(1, result.len());
    assert_eq!(Some("name"), result[0].column());
    assert_eq!(SubstraitOperator::Equal, result[0].operator());
    assert_eq!(Some(&string_value("John")), result[0].value());
    assert_eq!(Some(&DataType::Utf8), result[0].arrow_type());
}

#[test]
fn test_parse_column_predicates_with_boolean_literal() {
    let extensions = vec![create_extension_declaration(1, "equal:any_any")];
    let expression = create_binary_expression_with_boolean(1, 3, true);
    let result = predicate::column_predicates(&extensions, &expression, &column_names()).unwrap();
    assert_eq!(1, result.len());
    assert_eq!(Some("active"), result[0].column());
    assert_eq!(SubstraitOperator::Equal, result[0].operator());
    assert_eq!(
        Some(&PredicateValue::Literal(LiteralValue::Boolean(true))),
        result[0].value()
    );
    assert_eq!(Some(&DataType::Boolean), result[0].arrow_type());
}

#[test]
fn test_parse_column_predicates_with_unsupported_operator() {
    let extensions = vec![create_extension_declaration(1, "unsupported:function")];
    let expression = create_binary_expression(1, 0, 123);
    let err = predicate::column_predicates(&extensions, &expression, &column_names()).unwrap_err();
    assert!(matches!(err, SubstraitError::Unsupported(_)));
}

#[test]
fn test_parse_column_predicates_with_empty_expression() {
    let result =
        predicate::column_predicates(&[], &Expression::default(), &column_names()).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_not_operator_handling() {
    let cases = [
        ("equal:any_any", SubstraitOperator::NotEqual, 123, 123),
        ("not_equal:any_any", SubstraitOperator::Equal, 456, 456),
        ("gt:any_any", SubstraitOperator::LessThanOrEqualTo, 100, 100),
        ("gte:any_any", SubstraitOperator::LessThan, 200, 200),
        (
            "lt:any_any",
            SubstraitOperator::GreaterThanOrEqualTo,
            50,
            50,
        ),
        ("lte:any_any", SubstraitOperator::GreaterThan, 75, 75),
    ];
    for (inner_operator, expected_operator, value, expected_value) in cases {
        let extensions = vec![
            create_extension_declaration(1, "not:bool"),
            create_extension_declaration(2, inner_operator),
        ];
        let not_expression = create_not_expression(1, create_binary_expression(2, 0, value));
        let result =
            predicate::column_predicates(&extensions, &not_expression, &column_names()).unwrap();
        assert_eq!(1, result.len(), "{inner_operator}");
        assert_eq!(Some("id"), result[0].column());
        assert_eq!(expected_operator, result[0].operator(), "{inner_operator}");
        assert_eq!(Some(&int_value(expected_value)), result[0].value());
    }
}

#[test]
fn test_not_null_operators() {
    let extensions = vec![
        create_extension_declaration(1, "not:bool"),
        create_extension_declaration(2, "is_null:any"),
        create_extension_declaration(3, "is_not_null:any"),
    ];
    let not_is_null = create_not_expression(1, create_unary_expression(2, 0));
    let result1 = predicate::column_predicates(&extensions, &not_is_null, &column_names()).unwrap();
    assert_eq!(1, result1.len());
    assert_eq!(SubstraitOperator::IsNotNull, result1[0].operator());
    let not_is_not_null = create_not_expression(1, create_unary_expression(3, 0));
    let result2 =
        predicate::column_predicates(&extensions, &not_is_not_null, &column_names()).unwrap();
    assert_eq!(1, result2.len());
    assert_eq!(SubstraitOperator::IsNull, result2[0].operator());
}

#[test]
fn test_not_and_operator_nand() {
    let extensions = vec![
        create_extension_declaration(1, "not:bool"),
        create_extension_declaration(2, "and:bool"),
        create_extension_declaration(3, "equal:any_any"),
    ];
    let and_expression = create_logical_expression(
        2,
        create_binary_expression(3, 0, 123),
        create_binary_expression(3, 1, 456),
    );
    let not_and = create_not_expression(1, and_expression);
    let result = predicate::column_predicates(&extensions, &not_and, &column_names()).unwrap();
    assert_eq!(1, result.len());
    assert!(result[0].column().is_none());
    assert_eq!(SubstraitOperator::Nand, result[0].operator());
    assert!(matches!(
        result[0].value(),
        Some(PredicateValue::Predicates(_))
    ));
    assert_eq!(2, list_len(&result[0]));
}

#[test]
fn test_not_or_operator_nor() {
    let extensions = vec![
        create_extension_declaration(1, "not:bool"),
        create_extension_declaration(2, "or:bool"),
        create_extension_declaration(3, "equal:any_any"),
    ];
    let or_expression = create_logical_expression(
        2,
        create_binary_expression(3, 0, 123),
        create_binary_expression(3, 1, 456),
    );
    let not_or = create_not_expression(1, or_expression);
    let result = predicate::column_predicates(&extensions, &not_or, &column_names()).unwrap();
    assert_eq!(1, result.len());
    assert!(result[0].column().is_none());
    assert_eq!(SubstraitOperator::Nor, result[0].operator());
    assert!(matches!(
        result[0].value(),
        Some(PredicateValue::Predicates(_))
    ));
    assert_eq!(2, list_len(&result[0]));
}

#[test]
fn test_not_in_pattern() {
    let extensions = vec![
        create_extension_declaration(1, "not:bool"),
        create_extension_declaration(2, "or:bool"),
        create_extension_declaration(3, "equal:any_any"),
    ];
    let inner_or = create_logical_expression(
        2,
        create_binary_expression(3, 0, 10),
        create_binary_expression(3, 0, 20),
    );
    let outer_or = create_logical_expression(2, inner_or, create_binary_expression(3, 0, 30));
    let not_in = create_not_expression(1, outer_or);
    let result = predicate::column_predicates(&extensions, &not_in, &column_names()).unwrap();
    assert_eq!(1, result.len());
    let predicate = &result[0];
    if predicate.operator() == SubstraitOperator::NotIn {
        assert_eq!(Some("id"), predicate.column());
        assert_eq!(3, list_len(predicate));
    } else {
        assert!(predicate.column().is_none());
        assert_eq!(SubstraitOperator::Nor, predicate.operator());
    }
}

#[test]
fn test_parse_logical_expression_with_single_predicate() {
    let extensions = vec![create_extension_declaration(1, "equal:any_any")];
    let expression = create_binary_expression(1, 0, 123);
    let result = predicate::logical_expression(&extensions, Some(&expression), &column_names())
        .unwrap()
        .unwrap();
    assert!(result.is_leaf());
    assert_eq!(SubstraitOperator::Equal, result.operator());
    let leaf = result.leaf_predicate().unwrap();
    assert_eq!(Some("id"), leaf.column());
    assert_eq!(Some(&int_value(123)), leaf.value());
}

#[test]
fn test_parse_logical_expression_with_and_operator() {
    let extensions = vec![
        create_extension_declaration(1, "equal:any_any"),
        create_extension_declaration(2, "and:bool"),
    ];
    let and_expression = create_logical_expression(
        2,
        create_binary_expression(1, 0, 123),
        create_binary_expression_with_string(1, 1, "test"),
    );
    let result = predicate::logical_expression(&extensions, Some(&and_expression), &column_names())
        .unwrap()
        .unwrap();
    assert!(!result.is_leaf());
    assert_eq!(SubstraitOperator::And, result.operator());
    let children = result.children().unwrap();
    assert_eq!(2, children.len());
    let left = &children[0];
    assert!(left.is_leaf());
    assert_eq!(SubstraitOperator::Equal, left.operator());
    assert_eq!(Some("id"), left.leaf_predicate().unwrap().column());
    assert_eq!(
        Some(&int_value(123)),
        left.leaf_predicate().unwrap().value()
    );
    let right = &children[1];
    assert!(right.is_leaf());
    assert_eq!(SubstraitOperator::Equal, right.operator());
    assert_eq!(Some("name"), right.leaf_predicate().unwrap().column());
    assert_eq!(
        Some(&string_value("test")),
        right.leaf_predicate().unwrap().value()
    );
}

#[test]
fn test_parse_logical_expression_with_or_operator() {
    let extensions = vec![
        create_extension_declaration(1, "equal:any_any"),
        create_extension_declaration(2, "or:bool"),
    ];
    let or_expression = create_logical_expression(
        2,
        create_binary_expression(1, 0, 123),
        create_binary_expression_with_string(1, 1, "test"),
    );
    let result = predicate::logical_expression(&extensions, Some(&or_expression), &column_names())
        .unwrap()
        .unwrap();
    assert!(!result.is_leaf());
    assert_eq!(SubstraitOperator::Or, result.operator());
    let children = result.children().unwrap();
    assert_eq!(2, children.len());
    assert!(result.has_complex_logic());
    assert!(children[0].is_leaf());
    assert!(children[1].is_leaf());
}

#[test]
fn test_parse_logical_expression_with_unary_predicate() {
    let extensions = vec![create_extension_declaration(1, "is_null:any")];
    let expression = create_unary_expression(1, 0);
    let result = predicate::logical_expression(&extensions, Some(&expression), &column_names())
        .unwrap()
        .unwrap();
    assert!(result.is_leaf());
    assert_eq!(SubstraitOperator::IsNull, result.operator());
    assert_eq!(Some("id"), result.leaf_predicate().unwrap().column());
}

#[test]
fn test_parse_logical_expression_with_null_expression() {
    let result = predicate::logical_expression(&[], None, &column_names()).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_parse_logical_expression_complex_logic_detection() {
    let equal_ext = create_extension_declaration(1, "equal:any_any");
    let extensions = vec![equal_ext.clone()];
    let expression = create_binary_expression(1, 0, 123);
    let leaf = predicate::logical_expression(&extensions, Some(&expression), &column_names())
        .unwrap()
        .unwrap();
    assert!(!leaf.has_complex_logic());
    let extensions = vec![equal_ext, create_extension_declaration(2, "or:bool")];
    let or_expression = create_logical_expression(
        2,
        create_binary_expression(1, 0, 123),
        create_binary_expression_with_string(1, 1, "test"),
    );
    let or_result =
        predicate::logical_expression(&extensions, Some(&or_expression), &column_names())
            .unwrap()
            .unwrap();
    assert!(or_result.has_complex_logic());
}

#[test]
fn test_parse_logical_expression_with_not_operator() {
    let extensions = vec![
        create_extension_declaration(1, "not:bool"),
        create_extension_declaration(2, "equal:any_any"),
    ];
    let not_expression = create_not_expression(1, create_binary_expression(2, 0, 123));
    let result = predicate::logical_expression(&extensions, Some(&not_expression), &column_names())
        .unwrap()
        .unwrap();
    assert!(result.is_leaf());
    assert_eq!(SubstraitOperator::NotEqual, result.operator());
    assert_eq!(Some("id"), result.leaf_predicate().unwrap().column());
    assert_eq!(
        Some(&int_value(123)),
        result.leaf_predicate().unwrap().value()
    );
}
