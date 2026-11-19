use athena_federation_substrait::plan;
use athena_federation_substrait::proto::expression::field_reference::{
    ReferenceType, RootReference, RootType,
};
use athena_federation_substrait::proto::expression::if_then::IfClause;
use athena_federation_substrait::proto::expression::literal::{self, LiteralType};
use athena_federation_substrait::proto::expression::reference_segment::{self, StructField};
use athena_federation_substrait::proto::expression::{
    Cast, FieldReference, IfThen, Literal, ReferenceSegment, RexType, ScalarFunction,
    SingularOrList,
};
use athena_federation_substrait::proto::extensions::simple_extension_declaration::{
    ExtensionFunction, MappingType,
};
use athena_federation_substrait::proto::extensions::{
    SimpleExtensionDeclaration, SimpleExtensionUri,
};
use athena_federation_substrait::proto::function_argument::ArgType;
use athena_federation_substrait::proto::plan_rel;
use athena_federation_substrait::proto::r#type::{self, Kind, Nullability};
use athena_federation_substrait::proto::read_rel::{NamedTable, ReadType};
use athena_federation_substrait::proto::rel::RelType;
use athena_federation_substrait::proto::{
    Expression, FilterRel, FunctionArgument, JoinRel, NamedStruct, Plan, PlanRel, ProjectRel,
    ReadRel, Rel, RelRoot, Type,
};
use athena_federation_substrait::{SqlDialect, SubstraitError};
use base64::Engine;
use prost::Message;
use std::error::Error;

const NULLABLE: i32 = Nullability::Nullable as i32;
const REQUIRED: i32 = Nullability::Required as i32;
const FACTORIAL: (&str, &str) = ("/functions_arithmetic.yaml", "factorial:i64");
const NOT_A_NUMBER: &str =
    "Character N is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark.";
const INFINITE: &str =
    "Character I is neither a decimal digit number, decimal point, nor \"e\" notation exponential mark.";

fn rel(rel_type: RelType) -> Rel {
    Rel {
        rel_type: Some(rel_type),
    }
}

fn ty(kind: Kind) -> Type {
    Type { kind: Some(kind) }
}

fn i64_type(nullability: i32) -> Type {
    ty(Kind::I64(r#type::I64 {
        type_variation_reference: 0,
        nullability,
    }))
}

fn fp64_type(nullability: i32) -> Type {
    ty(Kind::Fp64(r#type::Fp64 {
        type_variation_reference: 0,
        nullability,
    }))
}

fn columns() -> Vec<(&'static str, Type)> {
    let mut all = leading_columns();
    all.extend(trailing_columns());
    all
}

fn leading_columns() -> Vec<(&'static str, Type)> {
    vec![
        (
            "s",
            ty(Kind::String(r#type::String {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "vc",
            ty(Kind::Varchar(r#type::VarChar {
                length: 12,
                type_variation_reference: 0,
                nullability: REQUIRED,
            })),
        ),
        (
            "fc",
            ty(Kind::FixedChar(r#type::FixedChar {
                length: 3,
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "ts",
            ty(Kind::PrecisionTimestamp(r#type::PrecisionTimestamp {
                precision: 3,
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "dec",
            ty(Kind::Decimal(r#type::Decimal {
                scale: 2,
                precision: 10,
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "lst",
            ty(Kind::List(Box::new(r#type::List {
                r#type: Some(Box::new(i64_type(NULLABLE))),
                type_variation_reference: 0,
                nullability: NULLABLE,
            }))),
        ),
        (
            "st",
            ty(Kind::Struct(r#type::Struct {
                types: vec![i64_type(REQUIRED), fp64_type(NULLABLE)],
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
    ]
}

fn trailing_columns() -> Vec<(&'static str, Type)> {
    vec![
        (
            "bin",
            ty(Kind::Binary(r#type::Binary {
                type_variation_reference: 0,
                nullability: REQUIRED,
            })),
        ),
        (
            "fb",
            ty(Kind::FixedBinary(r#type::FixedBinary {
                length: 4,
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "tm",
            ty(Kind::PrecisionTime(r#type::PrecisionTime {
                precision: 3,
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "tz",
            ty(Kind::PrecisionTimestampTz(r#type::PrecisionTimestampTz {
                precision: 3,
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "dt",
            ty(Kind::Date(r#type::Date {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        ("i", i64_type(REQUIRED)),
        (
            "iy",
            ty(Kind::IntervalYear(r#type::IntervalYear {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "id",
            ty(Kind::IntervalDay(r#type::IntervalDay {
                precision: Some(3),
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        (
            "mp",
            ty(Kind::Map(Box::new(r#type::Map {
                key: Some(Box::new(i64_type(REQUIRED))),
                value: Some(Box::new(fp64_type(NULLABLE))),
                type_variation_reference: 0,
                nullability: NULLABLE,
            }))),
        ),
        ("d", fp64_type(NULLABLE)),
        (
            "f",
            ty(Kind::Fp32(r#type::Fp32 {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
    ]
}

fn column(name: &str) -> i32 {
    let index = columns()
        .iter()
        .position(|(n, _)| *n == name)
        .expect("column");
    i32::try_from(index).expect("index")
}

fn read() -> Rel {
    let mut names = Vec::new();
    let mut types = Vec::new();
    for (name, t) in columns() {
        names.push(name.to_string());
        if let Some(Kind::Struct(st)) = &t.kind {
            names.extend((0..st.types.len()).map(|i| format!("{name}{i}")));
        }
        types.push(t);
    }
    rel(RelType::Read(Box::new(ReadRel {
        base_schema: Some(NamedStruct {
            names,
            r#struct: Some(r#type::Struct {
                types,
                type_variation_reference: 0,
                nullability: REQUIRED,
            }),
        }),
        read_type: Some(ReadType::NamedTable(NamedTable {
            names: vec!["t".into()],
            advanced_extension: None,
        })),
        ..Default::default()
    })))
}

fn small_read() -> Rel {
    rel(RelType::Read(Box::new(ReadRel {
        base_schema: Some(NamedStruct {
            names: vec!["k".into(), "v".into()],
            r#struct: Some(r#type::Struct {
                types: vec![i64_type(REQUIRED), fp64_type(NULLABLE)],
                type_variation_reference: 0,
                nullability: REQUIRED,
            }),
        }),
        read_type: Some(ReadType::NamedTable(NamedTable {
            names: vec!["u".into()],
            advanced_extension: None,
        })),
        ..Default::default()
    })))
}

fn direct(segment: StructField) -> Expression {
    Expression {
        rex_type: Some(RexType::Selection(Box::new(FieldReference {
            reference_type: Some(ReferenceType::DirectReference(ReferenceSegment {
                reference_type: Some(reference_segment::ReferenceType::StructField(Box::new(
                    segment,
                ))),
            })),
            root_type: Some(RootType::RootReference(RootReference {})),
        }))),
    }
}

fn field(index: i32) -> Expression {
    direct(StructField {
        field: index,
        child: None,
    })
}

fn literal_of(literal_type: LiteralType, nullable: bool) -> Literal {
    Literal {
        nullable,
        type_variation_reference: 0,
        literal_type: Some(literal_type),
    }
}

fn lit(literal_type: LiteralType) -> Expression {
    Expression {
        rex_type: Some(RexType::Literal(literal_of(literal_type, false))),
    }
}

fn value(e: Expression) -> FunctionArgument {
    FunctionArgument {
        arg_type: Some(ArgType::Value(e)),
    }
}

fn enum_arg(v: &str) -> FunctionArgument {
    FunctionArgument {
        arg_type: Some(ArgType::Enum(v.to_string())),
    }
}

fn call(anchor: u32, args: Vec<FunctionArgument>, output: Type) -> Expression {
    Expression {
        rex_type: Some(RexType::ScalarFunction(ScalarFunction {
            function_reference: anchor,
            arguments: args,
            output_type: Some(output),
            ..Default::default()
        })),
    }
}

fn encode(functions: &[(&str, &str)], root: Rel) -> String {
    let mut uris: Vec<&str> = Vec::new();
    let mut extensions = Vec::new();
    for (anchor, (uri, name)) in (1u32..).zip(functions) {
        let position = uris.iter().position(|u| u == uri).unwrap_or_else(|| {
            uris.push(uri);
            uris.len() - 1
        });
        extensions.push(SimpleExtensionDeclaration {
            mapping_type: Some(MappingType::ExtensionFunction(ExtensionFunction {
                extension_uri_reference: u32::try_from(position + 1).expect("anchor"),
                function_anchor: anchor,
                name: (*name).to_string(),
            })),
        });
    }
    #[allow(
        deprecated,
        reason = "Isthmus 0.65 reads extension URIs from the deprecated field"
    )]
    let plan = Plan {
        extension_uris: (1u32..)
            .zip(&uris)
            .map(|(anchor, uri)| SimpleExtensionUri {
                extension_uri_anchor: anchor,
                uri: (*uri).to_string(),
            })
            .collect(),
        extensions,
        relations: vec![PlanRel {
            rel_type: Some(plan_rel::RelType::Root(RelRoot {
                input: Some(root),
                names: Vec::new(),
            })),
        }],
        ..Default::default()
    };
    base64::engine::general_purpose::STANDARD.encode(plan.encode_to_vec())
}

fn project(function: (&str, &str), input: Rel, args: Vec<FunctionArgument>) -> String {
    encode(
        &[function],
        rel(RelType::Project(Box::new(ProjectRel {
            input: Some(Box::new(input)),
            expressions: vec![call(1, args, i64_type(NULLABLE))],
            ..Default::default()
        }))),
    )
}

fn equals(column_name: &str, rhs: LiteralType) -> String {
    let condition = call(
        1,
        vec![value(field(column(column_name))), value(lit(rhs))],
        ty(Kind::Bool(r#type::Boolean {
            type_variation_reference: 0,
            nullability: NULLABLE,
        })),
    );
    encode(
        &[("/functions_comparison.yaml", "equal:any_any")],
        rel(RelType::Filter(Box::new(FilterRel {
            input: Some(Box::new(read())),
            condition: Some(Box::new(condition)),
            ..Default::default()
        }))),
    )
}

fn sql(plan_string: &str) -> String {
    let dialect = SqlDialect::ansi();
    plan::to_sql_node(plan_string, &dialect)
        .expect("sql node")
        .to_sql_string(&dialect)
}

fn root_cause(plan_string: &str) -> String {
    let error = plan::to_sql_node(plan_string, &SqlDialect::ansi()).expect_err("error");
    let mut source: &dyn Error = &error;
    while let Some(next) = source.source() {
        source = next;
    }
    source.to_string()
}

fn unmapped(args: Vec<FunctionArgument>) -> String {
    root_cause(&project(FACTORIAL, read(), args))
}

fn factorial_of(types: &str) -> String {
    format!("Unable to convert scalar function factorial({types}).")
}

#[test]
fn non_finite_approximate_literals_fail_like_java_big_decimal() {
    let cases = [
        ("d", LiteralType::Fp64(f64::NAN), NOT_A_NUMBER),
        ("d", LiteralType::Fp64(f64::INFINITY), INFINITE),
        ("d", LiteralType::Fp64(f64::NEG_INFINITY), INFINITE),
        ("f", LiteralType::Fp32(f32::NAN), NOT_A_NUMBER),
        ("f", LiteralType::Fp32(f32::INFINITY), INFINITE),
        ("i", LiteralType::Fp64(f64::NAN), NOT_A_NUMBER),
    ];
    for (column_name, rhs, message) in cases {
        let error = plan::to_sql_node(&equals(column_name, rhs), &SqlDialect::ansi())
            .expect_err("non-finite literal");
        assert_eq!("Failed to parse Substrait plan", error.to_string());
        let SubstraitError::Wrapped { cause, .. } = error else {
            panic!("expected a wrapped error, got {error:?}");
        };
        assert!(
            matches!(&*cause, SubstraitError::NumberFormat(m) if m == message),
            "{cause:?}"
        );
    }
    assert!(sql(&equals("d", LiteralType::Fp64(1.5))).ends_with("WHERE `d` = 1.5E0"));
}

#[test]
fn unmapped_function_lists_field_types_like_java() {
    let cases = [
        ("s", "string?"),
        ("vc", "varchar<12>"),
        ("fc", "char<3>?"),
        ("ts", "precision_timestamp<3>?"),
        ("dec", "decimal<10,2>?"),
        ("lst", "list<i64?>?"),
        ("st", "struct<i64, fp64?>?"),
        ("bin", "binary"),
        ("fb", "fixedbinary<4>?"),
        ("tm", "precision_time<3>?"),
        ("tz", "precision_timestamp_tz<3>?"),
        ("dt", "date?"),
        ("i", "i64"),
        ("iy", "interval_year?"),
        ("id", "interval_day?"),
        ("mp", "map<i64,fp64?>?"),
    ];
    for (column_name, expected) in cases {
        assert_eq!(
            factorial_of(expected),
            unmapped(vec![value(field(column(column_name)))]),
            "{column_name}"
        );
    }
}

#[test]
fn unmapped_function_lists_literal_and_expression_types_like_java() {
    let struct_literal = LiteralType::Struct(literal::Struct {
        fields: vec![
            literal_of(LiteralType::I32(1), false),
            literal_of(LiteralType::String("x".into()), false),
        ],
    });
    let list_literal = LiteralType::List(literal::List {
        values: vec![literal_of(LiteralType::I32(1), true)],
    });
    let null_list = LiteralType::Null(ty(Kind::List(Box::new(r#type::List {
        r#type: Some(Box::new(i64_type(NULLABLE))),
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))));
    let if_then = |then: Expression, otherwise: Expression| Expression {
        rex_type: Some(RexType::IfThen(Box::new(IfThen {
            ifs: vec![IfClause {
                r#if: Some(lit(LiteralType::Boolean(true))),
                then: Some(then),
            }],
            r#else: Some(Box::new(otherwise)),
        }))),
    };
    let cast = Expression {
        rex_type: Some(RexType::Cast(Box::new(Cast {
            r#type: Some(i64_type(NULLABLE)),
            input: Some(Box::new(field(column("i")))),
            failure_behavior: 0,
        }))),
    };
    let or_list = Expression {
        rex_type: Some(RexType::SingularOrList(Box::new(SingularOrList {
            value: Some(Box::new(field(column("i")))),
            options: vec![lit(LiteralType::I64(1))],
        }))),
    };
    let cases = [
        (
            vec![lit(LiteralType::I64(3)), field(column("d"))],
            "i64, fp64?",
        ),
        (
            vec![
                lit(LiteralType::String("ab".into())),
                lit(LiteralType::FixedChar("a\u{1F600}".into())),
                lit(LiteralType::VarChar(literal::VarChar {
                    value: "x".into(),
                    length: 7,
                })),
                lit(LiteralType::Binary(vec![1])),
                lit(LiteralType::FixedBinary(vec![1, 2])),
            ],
            "string, char<3>, varchar<7>, binary, fixedbinary<2>",
        ),
        (
            vec![
                lit(LiteralType::Decimal(literal::Decimal {
                    value: vec![0; 16],
                    precision: 10,
                    scale: 2,
                })),
                lit(LiteralType::Date(1)),
                lit(LiteralType::PrecisionTimestamp(
                    literal::PrecisionTimestamp {
                        precision: 3,
                        value: 1,
                    },
                )),
                lit(LiteralType::Fp32(1.0)),
                lit(LiteralType::Boolean(true)),
            ],
            "decimal<10,2>, date, precision_timestamp<3>, fp32, boolean",
        ),
        (
            vec![lit(null_list), lit(list_literal), lit(struct_literal)],
            "list<i64?>?, list<i32?>, struct<i32, string>",
        ),
        (
            vec![
                if_then(field(column("dec")), lit(LiteralType::I32(1))),
                if_then(lit(LiteralType::I64(1)), field(column("vc"))),
            ],
            "i32?, varchar<12>",
        ),
        (vec![cast, or_list], "i64?, boolean?"),
    ];
    for (args, expected) in cases {
        assert_eq!(
            factorial_of(expected),
            unmapped(args.into_iter().map(value).collect())
        );
    }
}

#[test]
fn unmapped_function_lists_enum_and_type_arguments_like_java() {
    let message = root_cause(&project(
        (
            "/functions_datetime.yaml",
            "round_calendar:pts_req_req_req_i64",
        ),
        read(),
        vec![
            value(field(column("ts"))),
            enum_arg("FLOOR"),
            enum_arg(""),
            enum_arg("YEAR"),
            value(lit(LiteralType::I64(2))),
        ],
    ));
    assert_eq!(
        "Unable to convert scalar function round_calendar(precision_timestamp<3>?, Optional[FLOOR], Optional[], Optional[YEAR], i64).",
        message
    );
    let type_arg = FunctionArgument {
        arg_type: Some(ArgType::Type(i64_type(REQUIRED))),
    };
    assert_eq!(factorial_of("i64"), unmapped(vec![type_arg]));
}

#[test]
fn unmapped_function_lists_join_and_project_field_types_like_java() {
    let cases = [
        (1, "i64, fp64?"),
        (3, "i64, fp64?"),
        (4, "i64?, fp64?"),
        (2, "i64?, fp64?"),
    ];
    let left_i = column("i");
    let right_v = i32::try_from(columns().len() + 1).expect("index");
    for (join_type, expected) in cases {
        let join = rel(RelType::Join(Box::new(JoinRel {
            left: Some(Box::new(read())),
            right: Some(Box::new(small_read())),
            expression: Some(Box::new(lit(LiteralType::Boolean(true)))),
            r#type: join_type,
            ..Default::default()
        })));
        let args = vec![value(field(left_i)), value(field(right_v))];
        assert_eq!(
            factorial_of(expected),
            root_cause(&project(FACTORIAL, join, args)),
            "join type {join_type}"
        );
    }
    let add = ("/functions_arithmetic.yaml", "add:i64_i64");
    let inner = rel(RelType::Project(Box::new(ProjectRel {
        input: Some(Box::new(small_read())),
        expressions: vec![call(
            1,
            vec![value(field(0)), value(lit(LiteralType::I64(1)))],
            i64_type(REQUIRED),
        )],
        ..Default::default()
    })));
    let outer = rel(RelType::Project(Box::new(ProjectRel {
        input: Some(Box::new(inner)),
        expressions: vec![call(2, vec![value(field(2))], i64_type(NULLABLE))],
        ..Default::default()
    })));
    assert_eq!(
        factorial_of("i64"),
        root_cause(&encode(&[add, FACTORIAL], outer))
    );
}

#[test]
fn missing_function_argument_fails_like_java() {
    let missing = FunctionArgument { arg_type: None };
    let message = "Unable to convert FunctionArgument .";
    assert_eq!(message, unmapped(vec![missing.clone()]));
    let mapped = project(
        ("/functions_arithmetic.yaml", "add:i64_i64"),
        read(),
        vec![value(field(column("i"))), missing],
    );
    assert_eq!(message, root_cause(&mapped));
}
