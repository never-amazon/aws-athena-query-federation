use athena_federation_substrait::plan;
use athena_federation_substrait::proto::expression::field_reference::{
    ReferenceType, RootReference, RootType,
};
use athena_federation_substrait::proto::expression::if_then::IfClause;
use athena_federation_substrait::proto::expression::literal::{self, LiteralType};
use athena_federation_substrait::proto::expression::reference_segment::{self, StructField};
use athena_federation_substrait::proto::expression::window_function::{bound, Bound, BoundsType};
use athena_federation_substrait::proto::expression::{
    Cast, FieldReference, IfThen, Literal, ReferenceSegment, RexType, ScalarFunction,
    SingularOrList, WindowFunction,
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
use athena_federation_substrait::proto::sort_field::{SortDirection, SortKind};
use athena_federation_substrait::proto::{
    Expression, FilterRel, FunctionArgument, JoinRel, NamedStruct, Plan, PlanRel, ProjectRel,
    ReadRel, Rel, RelRoot, SortField, Type,
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

fn null_of(t: Type) -> Expression {
    Expression {
        rex_type: Some(RexType::Literal(literal_of(LiteralType::Null(t), true))),
    }
}

fn list_of(element: Type) -> Type {
    ty(Kind::List(Box::new(r#type::List {
        r#type: Some(Box::new(element)),
        type_variation_reference: 0,
        nullability: NULLABLE,
    })))
}

fn map_of(key: Type, value: Type) -> Type {
    ty(Kind::Map(Box::new(r#type::Map {
        key: Some(Box::new(key)),
        value: Some(Box::new(value)),
        type_variation_reference: 0,
        nullability: NULLABLE,
    })))
}

fn varchar(length: i32) -> Type {
    ty(Kind::Varchar(r#type::VarChar {
        length,
        type_variation_reference: 0,
        nullability: REQUIRED,
    }))
}

fn decimal(precision: i32, scale: i32) -> Type {
    ty(Kind::Decimal(r#type::Decimal {
        scale,
        precision,
        type_variation_reference: 0,
        nullability: REQUIRED,
    }))
}

fn interval_day() -> Type {
    ty(Kind::IntervalDay(r#type::IntervalDay {
        precision: Some(3),
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn select_null(t: Type, dialect: &SqlDialect) -> Result<String, String> {
    let plan_string = encode(
        &[],
        rel(RelType::Project(Box::new(ProjectRel {
            input: Some(Box::new(small_read())),
            expressions: vec![null_of(t)],
            ..Default::default()
        }))),
    );
    match plan::to_sql_node(&plan_string, dialect) {
        Ok(node) => Ok(node.to_sql_string(dialect).replace('\n', " ")),
        Err(error) => {
            let mut source: &dyn Error = &error;
            while let Some(next) = source.source() {
                source = next;
            }
            Err(source.to_string())
        }
    }
}

#[test]
fn null_collection_casts_spell_element_types_like_calcite() {
    let string = ty(Kind::String(r#type::String {
        type_variation_reference: 0,
        nullability: REQUIRED,
    }));
    let row = ty(Kind::Struct(r#type::Struct {
        types: vec![i64_type(REQUIRED), varchar(5), decimal(10, 2)],
        type_variation_reference: 0,
        nullability: REQUIRED,
    }));
    let cases = [
        (
            list_of(varchar(5)),
            "VARCHAR(5) CHARACTER SET `UTF-8` ARRAY",
            "VARCHAR(5) ARRAY",
            "VARCHAR(5) CHARACTER SET \"UTF-8\" ARRAY",
        ),
        (
            list_of(list_of(i64_type(REQUIRED))),
            "BIGINT ARRAY ARRAY",
            "BIGINT ARRAY ARRAY",
            "BIGINT ARRAY ARRAY",
        ),
        (
            list_of(decimal(38, 30)),
            "DECIMAL(19, 30) ARRAY",
            "DECIMAL(19, 30) ARRAY",
            "DECIMAL(19, 30) ARRAY",
        ),
        (
            list_of(varchar(100_000)),
            "VARCHAR(65536) CHARACTER SET `UTF-8` ARRAY",
            "VARCHAR(100000) ARRAY",
            "VARCHAR(65536) CHARACTER SET \"UTF-8\" ARRAY",
        ),
        (
            row.clone(),
            "ROW(`f0` BIGINT, `f1` VARCHAR(5) CHARACTER SET `UTF-8`, `f2` DECIMAL(10, 2))",
            "ROW(\"f0\" BIGINT, \"f1\" VARCHAR(5), \"f2\" DECIMAL(10, 2))",
            "ROW(\"f0\" BIGINT, \"f1\" VARCHAR(5) CHARACTER SET \"UTF-8\", \"f2\" DECIMAL(10, 2))",
        ),
        (
            list_of(row),
            "ROW(`f0` BIGINT, `f1` VARCHAR(5) CHARACTER SET `UTF-8`, `f2` DECIMAL(10, 2)) ARRAY",
            "ROW(\"f0\" BIGINT, \"f1\" VARCHAR(5), \"f2\" DECIMAL(10, 2)) ARRAY",
            "ROW(\"f0\" BIGINT, \"f1\" VARCHAR(5) CHARACTER SET \"UTF-8\", \"f2\" DECIMAL(10, 2)) ARRAY",
        ),
        (
            map_of(string, list_of(i64_type(NULLABLE))),
            "MAP< VARCHAR CHARACTER SET `UTF-8`, BIGINT ARRAY >",
            "MAP< VARCHAR, BIGINT ARRAY >",
            "MAP< VARCHAR CHARACTER SET \"UTF-8\", BIGINT ARRAY >",
        ),
    ];
    for (t, ansi, postgresql, snowflake) in cases {
        assert_eq!(
            Ok(format!(
                "SELECT `k`, `v`, CAST(NULL AS {ansi}) AS `$f2` FROM `u`"
            )),
            select_null(t.clone(), &SqlDialect::ansi())
        );
        assert_eq!(
            Ok(format!(
                "SELECT \"k\", \"v\", CAST(NULL AS {postgresql}) AS \"$f2\" FROM \"u\""
            )),
            select_null(t.clone(), &SqlDialect::postgresql())
        );
        assert_eq!(
            Ok(format!(
                "SELECT \"k\", \"v\", CAST(NULL AS {snowflake}) AS \"$f2\" FROM \"u\""
            )),
            select_null(t, &SqlDialect::snowflake())
        );
    }
}

#[test]
fn interval_casts_fail_in_convert_type_to_spec_like_calcite() {
    let year = ty(Kind::IntervalYear(r#type::IntervalYear {
        type_variation_reference: 0,
        nullability: NULLABLE,
    }));
    let day_second = "Unsupported type when convertTypeToSpec: INTERVAL_DAY_SECOND";
    let cases = [
        (interval_day(), day_second),
        (
            year,
            "Unsupported type when convertTypeToSpec: INTERVAL_YEAR_MONTH",
        ),
        (list_of(interval_day()), day_second),
        (map_of(i64_type(REQUIRED), interval_day()), day_second),
    ];
    for dialect in [
        SqlDialect::ansi(),
        SqlDialect::postgresql(),
        SqlDialect::snowflake(),
    ] {
        for (t, message) in &cases {
            assert_eq!(
                Err((*message).to_string()),
                select_null(t.clone(), &dialect)
            );
        }
    }
    let cast = Expression {
        rex_type: Some(RexType::Cast(Box::new(Cast {
            r#type: Some(interval_day()),
            input: Some(Box::new(field(column("iy")))),
            failure_behavior: 0,
        }))),
    };
    let plan_string = encode(
        &[],
        rel(RelType::Project(Box::new(ProjectRel {
            input: Some(Box::new(read())),
            expressions: vec![cast],
            ..Default::default()
        }))),
    );
    assert_eq!(day_second, root_cause(&plan_string));
}

const NOT_HANDLED: &str = "of type io.substrait.expression.ImmutableFieldReference not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.";

fn selection(segment: ReferenceSegment, root: RootType) -> Expression {
    Expression {
        rex_type: Some(RexType::Selection(Box::new(FieldReference {
            reference_type: Some(ReferenceType::DirectReference(segment)),
            root_type: Some(root),
        }))),
    }
}

fn struct_segment(field: i32, child: Option<ReferenceSegment>) -> ReferenceSegment {
    ReferenceSegment {
        reference_type: Some(reference_segment::ReferenceType::StructField(Box::new(
            StructField {
                field,
                child: child.map(Box::new),
            },
        ))),
    }
}

fn nested(path: &[i32], child: Option<ReferenceSegment>) -> Expression {
    let segment = path
        .iter()
        .rev()
        .fold(child, |child, field| Some(struct_segment(*field, child)))
        .expect("path");
    selection(segment, RootType::RootReference(RootReference {}))
}

fn project_rel(input: Rel, expressions: Vec<Expression>) -> Rel {
    rel(RelType::Project(Box::new(ProjectRel {
        input: Some(Box::new(input)),
        expressions,
        ..Default::default()
    })))
}

fn filter_rel(input: Rel, condition: Expression) -> Rel {
    rel(RelType::Filter(Box::new(FilterRel {
        input: Some(Box::new(input)),
        condition: Some(Box::new(condition)),
        ..Default::default()
    })))
}

fn boolean_type() -> Type {
    ty(Kind::Bool(r#type::Boolean {
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn single_column_read(column_type: Type) -> Rel {
    rel(RelType::Read(Box::new(ReadRel {
        base_schema: Some(NamedStruct {
            names: vec!["c".into()],
            r#struct: Some(r#type::Struct {
                types: vec![column_type],
                type_variation_reference: 0,
                nullability: REQUIRED,
            }),
        }),
        read_type: Some(ReadType::NamedTable(NamedTable {
            names: vec!["w".into()],
            advanced_extension: None,
        })),
        ..Default::default()
    })))
}

fn user_defined(type_reference: u32) -> Type {
    ty(Kind::UserDefined(r#type::UserDefined {
        type_reference,
        type_variation_reference: 0,
        nullability: REQUIRED,
        type_parameters: Vec::new(),
    }))
}

#[allow(
    deprecated,
    reason = "Isthmus 0.65 reads extension URIs from the deprecated field"
)]
fn declare_type(plan_string: &str, uri: &str, name: &str, anchor: u32) -> String {
    use athena_federation_substrait::proto::extensions::simple_extension_declaration::ExtensionType;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(plan_string)
        .expect("base64");
    let mut plan = Plan::decode(bytes.as_slice()).expect("plan");
    plan.extension_uris.push(SimpleExtensionUri {
        extension_uri_anchor: 50,
        uri: uri.into(),
    });
    plan.extensions.push(SimpleExtensionDeclaration {
        mapping_type: Some(MappingType::ExtensionType(ExtensionType {
            extension_uri_reference: 50,
            type_anchor: anchor,
            name: name.into(),
        })),
    });
    base64::engine::general_purpose::STANDARD.encode(plan.encode_to_vec())
}

fn root_cause_error(plan_string: &str) -> (String, String) {
    let error = plan::to_sql_node(plan_string, &SqlDialect::ansi()).expect_err("error");
    let mut source: &SubstraitError = &error;
    while let SubstraitError::Wrapped { cause, .. } = source {
        source = cause;
    }
    (
        source.java_kind().java_class_name().to_string(),
        source.to_string(),
    )
}

#[test]
fn nested_field_references_fail_with_the_isthmus_field_reference_text() {
    let st = column("st");
    let list_element = ReferenceSegment {
        reference_type: Some(reference_segment::ReferenceType::ListElement(Box::new(
            reference_segment::ListElement {
                offset: 2,
                child: None,
            },
        ))),
    };
    let map_key = ReferenceSegment {
        reference_type: Some(reference_segment::ReferenceType::MapKey(Box::new(
            reference_segment::MapKey {
                map_key: Some(literal_of(LiteralType::I64(1), false)),
                child: None,
            },
        ))),
    };
    let cases = [
        (
            nested(&[st, 1], None),
            "FieldReference{segments=[StructField{offset=1}, StructField{offset=6}], type=FP64{nullable=true}}",
        ),
        (
            nested(&[st, 0], None),
            "FieldReference{segments=[StructField{offset=0}, StructField{offset=6}], type=I64{nullable=false}}",
        ),
        (
            nested(&[column("lst")], Some(list_element)),
            "FieldReference{segments=[ListElement{offset=2}, StructField{offset=5}], type=I64{nullable=true}}",
        ),
        (
            nested(&[column("mp")], Some(map_key)),
            "FieldReference{segments=[MapKey{key=I64Literal{nullable=false, value=1}}, StructField{offset=15}], type=FP64{nullable=true}}",
        ),
    ];
    for (expression, text) in cases {
        assert_eq!(
            format!("Expression {text} {NOT_HANDLED}"),
            root_cause(&encode(&[], project_rel(read(), vec![expression])))
        );
    }
    let after_project = project_rel(
        project_rel(read(), vec![field(st)]),
        vec![nested(&[18, 1], None)],
    );
    assert_eq!(
        format!("Expression FieldReference{{segments=[StructField{{offset=1}}, StructField{{offset=18}}], type=FP64{{nullable=true}}}} {NOT_HANDLED}"),
        root_cause(&encode(&[], after_project))
    );
    let condition = call(
        1,
        vec![
            value(nested(&[st, 0], None)),
            value(lit(LiteralType::I64(1))),
        ],
        boolean_type(),
    );
    assert_eq!(
        format!("Expression FieldReference{{segments=[StructField{{offset=0}}, StructField{{offset=6}}], type=I64{{nullable=false}}}} {NOT_HANDLED}"),
        root_cause(&encode(
            &[("/functions_comparison.yaml", "equal:any_any")],
            filter_rel(read(), condition)
        ))
    );
}

#[test]
fn invalid_nested_field_references_fail_while_building_the_plan_like_isthmus() {
    let empty = ReferenceSegment {
        reference_type: None,
    };
    let cases = [
        (
            nested(&[column("i")], Some(empty)),
            "Unhandled type: REFERENCETYPE_NOT_SET",
        ),
        (nested(&[column("st"), 5], None), "Undefined struct type."),
        (
            nested(&[column("i"), 0], None),
            "This visitor only supports retrieving struct types. Was applied to a non-struct type.",
        ),
        (
            field(100),
            "Field reference offset (100) must be less than number of fields in struct (18)",
        ),
    ];
    for (expression, message) in cases {
        assert_eq!(
            message,
            root_cause(&encode(&[], project_rel(read(), vec![expression])))
        );
    }
}

#[test]
fn outer_references_with_child_segments_stay_rejected() {
    use athena_federation_substrait::proto::expression::field_reference::OuterReference;
    use athena_federation_substrait::proto::expression::subquery::{
        set_predicate, SetPredicate, SubqueryType,
    };
    use athena_federation_substrait::proto::expression::Subquery;
    let outer = |child: Option<ReferenceSegment>| {
        selection(
            struct_segment(column("st"), child),
            RootType::OuterReference(OuterReference { steps_out: 1 }),
        )
    };
    let exists = |outer_field: Expression| Expression {
        rex_type: Some(RexType::Subquery(Box::new(Subquery {
            subquery_type: Some(SubqueryType::SetPredicate(Box::new(SetPredicate {
                predicate_op: set_predicate::PredicateOp::Exists as i32,
                tuples: Some(Box::new(filter_rel(
                    small_read(),
                    call(1, vec![value(field(0)), value(outer_field)], boolean_type()),
                ))),
            }))),
        }))),
    };
    let equal = ("/functions_comparison.yaml", "equal:any_any");
    assert!(
        sql(&encode(&[equal], filter_rel(read(), exists(outer(None)))))
            .ends_with("WHERE EXISTS (SELECT *\nFROM `u`\nWHERE `k` = `t`.`st`)")
    );
    let error = plan::to_sql_node(
        &encode(
            &[equal],
            filter_rel(read(), exists(outer(Some(struct_segment(0, None))))),
        ),
        &SqlDialect::ansi(),
    );
    assert!(error.is_err());
}

#[test]
fn function_type_arguments_print_the_isthmus_type_text() {
    let interval_without_precision = ty(Kind::IntervalDay(r#type::IntervalDay {
        precision: None,
        type_variation_reference: 0,
        nullability: NULLABLE,
    }));
    let cases = [
        (i64_type(REQUIRED), "I64{nullable=false}"),
        (
            decimal(10, 2),
            "Decimal{nullable=false, scale=2, precision=10}",
        ),
        (varchar(5), "VarChar{nullable=false, length=5}"),
        (
            ty(Kind::Struct(r#type::Struct {
                types: vec![i64_type(NULLABLE), fp64_type(REQUIRED)],
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
            "Struct{nullable=true, fields=[I64{nullable=true}, FP64{nullable=false}]}",
        ),
        (
            list_of(i64_type(NULLABLE)),
            "ListType{nullable=true, elementType=I64{nullable=true}}",
        ),
        (
            map_of(i64_type(REQUIRED), i64_type(NULLABLE)),
            "Map{nullable=true, key=I64{nullable=false}, value=I64{nullable=true}}",
        ),
        (
            interval_without_precision,
            "IntervalDay{nullable=true, precision=6}",
        ),
    ];
    for (t, text) in cases {
        let type_arg = FunctionArgument {
            arg_type: Some(ArgType::Type(t)),
        };
        let plan_string = project(
            ("/functions_arithmetic.yaml", "add:i64_i64"),
            read(),
            vec![value(field(column("i"))), type_arg],
        );
        assert_eq!(
            format!("FunctionArg {text} not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."),
            root_cause(&plan_string)
        );
    }
}

#[test]
#[allow(
    deprecated,
    reason = "builds the deprecated Substrait types that Isthmus still prints"
)]
fn deprecated_datetime_columns_print_their_substrait_names() {
    let cases = [
        (
            ty(Kind::Timestamp(r#type::Timestamp {
                type_variation_reference: 0,
                nullability: REQUIRED,
            })),
            "timestamp",
        ),
        (
            ty(Kind::Time(r#type::Time {
                type_variation_reference: 0,
                nullability: REQUIRED,
            })),
            "time",
        ),
        (
            ty(Kind::TimestampTz(r#type::TimestampTz {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
            "timestamp_tz?",
        ),
    ];
    for (t, text) in cases {
        let plan_string = project(FACTORIAL, single_column_read(t), vec![value(field(0))]);
        assert_eq!(factorial_of(text), root_cause(&plan_string));
    }
}

#[test]
fn user_defined_types_fail_at_the_extension_lookup_like_isthmus() {
    let unknown = "Unknown type id. Make sure that the type id provided was shared in the extensions section of the plan.";
    let type_arg = FunctionArgument {
        arg_type: Some(ArgType::Type(user_defined(9))),
    };
    let plans = [
        project(FACTORIAL, read(), vec![type_arg.clone()]),
        project(FACTORIAL, read(), vec![value(null_of(user_defined(9)))]),
        project(
            FACTORIAL,
            single_column_read(user_defined(9)),
            vec![value(field(0))],
        ),
        encode(
            &[],
            project_rel(single_column_read(user_defined(9)), vec![field(0)]),
        ),
    ];
    for plan_string in &plans {
        assert_eq!(unknown, root_cause(plan_string));
        assert_eq!(
            "Received a reference for extension /functions_geometry.yaml but that extension is not currently loaded.",
            root_cause(&declare_type(plan_string, "/functions_geometry.yaml", "geometry", 9))
        );
        assert_eq!(
            "Unexpected type with name point. The namespace /functions_arithmetic.yaml is loaded but no type with this name found.",
            root_cause(&declare_type(plan_string, "/functions_arithmetic.yaml", "point", 9))
        );
    }
}

#[test]
fn plan_building_errors_come_before_calcite_errors_like_isthmus() {
    let unmapped = call(1, vec![value(field(column("i")))], i64_type(NULLABLE));
    let not_set = null_of(Type { kind: None });
    let cases = [
        (
            vec![vec![unmapped.clone()], vec![field(100)]],
            "Field reference offset (100) must be less than number of fields in struct (19)",
        ),
        (
            vec![vec![unmapped.clone()], vec![not_set.clone()]],
            "Type is not set: ",
        ),
        (
            vec![vec![unmapped.clone()], vec![call(7, Vec::new(), i64_type(NULLABLE))]],
            "Unknown function id. Make sure that the function id provided was shared in the extensions section of the plan.",
        ),
        (
            vec![vec![unmapped.clone()], vec![nested(&[column("st"), 9], None)]],
            "Undefined struct type.",
        ),
        (vec![vec![unmapped.clone(), not_set]], "Type is not set: "),
        (
            vec![vec![null_of(interval_day())], vec![unmapped]],
            "Unable to convert scalar function factorial(i64).",
        ),
    ];
    for (projects, message) in cases {
        let root = projects.into_iter().fold(read(), project_rel);
        assert_eq!(message, root_cause(&encode(&[FACTORIAL], root)));
    }
}

#[test]
fn function_declarations_must_match_the_default_extension_catalog() {
    let equal = |declaration: (&str, &str)| {
        encode(
            &[declaration],
            filter_rel(
                read(),
                call(
                    1,
                    vec![value(field(column("i"))), value(lit(LiteralType::I64(1)))],
                    boolean_type(),
                ),
            ),
        )
    };
    assert!(sql(&equal(("/functions_comparison.yaml", "equal:any_any"))).ends_with("WHERE `i` = 1"));
    let cases = [
        (
            ("/functions_arithmetic.yaml", "equal:any_any"),
            "Unexpected scalar function with key equal:any_any. The namespace /functions_arithmetic.yaml is loaded but no scalar function with this key found.",
        ),
        (
            ("/functions_comparison.yaml", "equal"),
            "Unexpected scalar function with key equal. The namespace /functions_comparison.yaml is loaded but no scalar function with this key found.",
        ),
        (
            ("/nope.yaml", "equal:any_any"),
            "Received a reference for extension /nope.yaml but that extension is not currently loaded.",
        ),
    ];
    for (declaration, message) in cases {
        assert_eq!(message, root_cause(&equal(declaration)));
    }
    let mut plan = Plan::decode(
        base64::engine::general_purpose::STANDARD
            .decode(equal(("/functions_comparison.yaml", "equal:any_any")))
            .expect("base64")
            .as_slice(),
    )
    .expect("plan");
    if let Some(MappingType::ExtensionFunction(f)) = &mut plan.extensions[0].mapping_type {
        f.extension_uri_reference = 77;
    }
    let plan_string = base64::engine::general_purpose::STANDARD.encode(plan.encode_to_vec());
    assert_eq!(
        (
            "java.lang.IllegalStateException".to_string(),
            "Could not find extension URI of 77".to_string()
        ),
        root_cause_error(&plan_string)
    );
}

#[test]
fn aggregate_and_window_declarations_use_their_own_catalog_messages() {
    use athena_federation_substrait::proto::aggregate_rel::{Grouping, Measure};
    use athena_federation_substrait::proto::expression::WindowFunction;
    use athena_federation_substrait::proto::{AggregateFunction, AggregateRel};
    let i = column("i");
    let aggregate = |declaration: (&str, &str)| {
        #[allow(
            deprecated,
            reason = "Isthmus 0.65 reads grouping expressions from the deprecated field"
        )]
        let grouping = Grouping {
            grouping_expressions: vec![field(i)],
            expression_references: Vec::new(),
        };
        encode(
            &[declaration],
            rel(RelType::Aggregate(Box::new(AggregateRel {
                input: Some(Box::new(read())),
                groupings: vec![grouping],
                measures: vec![Measure {
                    measure: Some(AggregateFunction {
                        function_reference: 1,
                        arguments: vec![value(field(i))],
                        output_type: Some(i64_type(NULLABLE)),
                        phase: 3,
                        invocation: 1,
                        ..Default::default()
                    }),
                    filter: None,
                }],
                ..Default::default()
            }))),
        )
    };
    let window = |declaration: (&str, &str)| {
        encode(
            &[declaration],
            project_rel(
                read(),
                vec![Expression {
                    rex_type: Some(RexType::WindowFunction(WindowFunction {
                        function_reference: 1,
                        arguments: vec![value(field(i))],
                        output_type: Some(i64_type(NULLABLE)),
                        phase: 3,
                        invocation: 1,
                        bounds_type: 1,
                        ..Default::default()
                    })),
                }],
            ),
        )
    };
    let add = ("/functions_arithmetic.yaml", "add:i64_i64");
    assert_eq!(
        "SELECT `i`, SUM(`i`) AS `$f1`\nFROM `t`\nGROUP BY `i`",
        sql(&aggregate(("/functions_arithmetic.yaml", "sum:i64")))
    );
    assert_eq!(
        "Unexpected aggregate function with key add:i64_i64. The namespace /functions_arithmetic.yaml is loaded but no aggregate function with this key was found.",
        root_cause(&aggregate(add))
    );
    assert_eq!(
        "Received a reference for extension /nope.yaml but that extension is not currently loaded.",
        root_cause(&aggregate(("/nope.yaml", "sum:i64")))
    );
    assert_eq!(
        "Unexpected window aggregate function with key add:i64_i64. The namespace /functions_arithmetic.yaml is loaded but no window aggregate function with this key was found.",
        root_cause(&window(add))
    );
}

#[test]
fn missing_plan_parts_fail_with_the_isthmus_messages() {
    use athena_federation_substrait::proto::expression::subquery::{
        Scalar, SetPredicate, SubqueryType,
    };
    use athena_federation_substrait::proto::expression::Subquery;
    let missing_if = Expression {
        rex_type: Some(RexType::IfThen(Box::new(IfThen {
            ifs: vec![IfClause {
                r#if: None,
                then: Some(lit(LiteralType::I64(1))),
            }],
            r#else: Some(Box::new(lit(LiteralType::I64(2)))),
        }))),
    };
    assert_eq!(
        "Unknown type: REXTYPE_NOT_SET",
        root_cause(&encode(&[], project_rel(read(), vec![missing_if])))
    );
    let join = rel(RelType::Join(Box::new(JoinRel {
        left: Some(Box::new(read())),
        right: Some(Box::new(small_read())),
        expression: None,
        r#type: 1,
        ..Default::default()
    })));
    assert_eq!(
        "Unknown type: REXTYPE_NOT_SET",
        root_cause(&encode(&[], join))
    );
    let unspecified = Expression {
        rex_type: Some(RexType::Subquery(Box::new(Subquery {
            subquery_type: Some(SubqueryType::SetPredicate(Box::new(SetPredicate {
                predicate_op: 0,
                tuples: Some(Box::new(small_read())),
            }))),
        }))),
    };
    assert_eq!(
        "Cannot handle SetPredicate when PredicateOp is PREDICATE_OP_UNSPECIFIED.",
        root_cause(&encode(&[], filter_rel(read(), unspecified)))
    );
    let two_columns = Expression {
        rex_type: Some(RexType::Subquery(Box::new(Subquery {
            subquery_type: Some(SubqueryType::Scalar(Box::new(Scalar {
                input: Some(Box::new(small_read())),
            }))),
        }))),
    };
    assert_eq!(
        "Scalar subquery must have exactly one field",
        root_cause(&project(FACTORIAL, read(), vec![value(two_columns)]))
    );
    let no_output = Expression {
        rex_type: Some(RexType::ScalarFunction(ScalarFunction {
            function_reference: 1,
            arguments: vec![value(field(column("i"))), value(field(column("i")))],
            output_type: None,
            ..Default::default()
        })),
    };
    assert_eq!(
        "Type is not set: ",
        root_cause(&encode(
            &[("/functions_arithmetic.yaml", "add:i64_i64")],
            project_rel(read(), vec![no_output])
        ))
    );
}

#[test]
fn base_schema_errors_match_java_list_messages() {
    let short_names = rel(RelType::Read(Box::new(ReadRel {
        base_schema: Some(NamedStruct {
            names: vec!["a".into()],
            r#struct: Some(r#type::Struct {
                types: vec![i64_type(REQUIRED), i64_type(REQUIRED)],
                type_variation_reference: 0,
                nullability: REQUIRED,
            }),
        }),
        read_type: Some(ReadType::NamedTable(NamedTable {
            names: vec!["w".into()],
            advanced_extension: None,
        })),
        ..Default::default()
    })));
    assert_eq!(
        (
            "java.lang.IndexOutOfBoundsException".to_string(),
            "Index: 1, Size: 1".to_string()
        ),
        root_cause_error(&encode(&[], short_names))
    );
    let uuid = ty(Kind::Uuid(r#type::Uuid {
        type_variation_reference: 0,
        nullability: REQUIRED,
    }));
    assert_eq!(
        (
            "java.lang.UnsupportedOperationException".to_string(),
            "Unknown expression type.".to_string()
        ),
        root_cause_error(&encode(
            &[],
            project_rel(single_column_read(uuid), vec![field(0)])
        ))
    );
}

fn window_bound(kind: &str) -> Option<Bound> {
    let kind = match kind {
        "none" => return None,
        "empty" => return Some(Bound { kind: None }),
        "unbounded" => bound::Kind::Unbounded(bound::Unbounded {}),
        "current" => bound::Kind::CurrentRow(bound::CurrentRow {}),
        "preceding" => bound::Kind::Preceding(bound::Preceding { offset: 2 }),
        _ => bound::Kind::Following(bound::Following { offset: 3 }),
    };
    Some(Bound { kind: Some(kind) })
}

fn window_sql(bounds_type: BoundsType, ordered: bool, lower: &str, upper: &str) -> String {
    let sorts = if ordered {
        vec![SortField {
            expr: Some(field(column("i"))),
            sort_kind: Some(SortKind::Direction(SortDirection::AscNullsFirst as i32)),
        }]
    } else {
        vec![]
    };
    let window = WindowFunction {
        function_reference: 1,
        arguments: vec![value(field(column("i")))],
        output_type: Some(i64_type(NULLABLE)),
        phase: 3,
        invocation: 1,
        bounds_type: bounds_type as i32,
        lower_bound: window_bound(lower),
        upper_bound: window_bound(upper),
        sorts,
        ..Default::default()
    };
    let plan_string = encode(
        &[("/functions_arithmetic.yaml", "sum:i64")],
        project_rel(
            read(),
            vec![Expression {
                rex_type: Some(RexType::WindowFunction(window)),
            }],
        ),
    );
    let text = sql(&plan_string);
    let start = text.find("OVER (").expect("over clause");
    let end = text[start..].find(") AS").expect("over end") + start + 1;
    text[start..end].to_string()
}

#[test]
fn window_frames_follow_calcite_make_window() {
    let whole = "OVER (RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)";
    let ordered = "OVER (ORDER BY `i` NULLS FIRST";
    let cases = [
        (BoundsType::Rows, false, "none", "none", whole.to_string()),
        (
            BoundsType::Rows,
            false,
            "empty",
            "unbounded",
            whole.to_string(),
        ),
        (
            BoundsType::Rows,
            true,
            "unbounded",
            "unbounded",
            format!("{ordered} RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)"),
        ),
        (
            BoundsType::Rows,
            true,
            "none",
            "current",
            format!("{ordered} ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)"),
        ),
        (
            BoundsType::Rows,
            true,
            "current",
            "none",
            format!("{ordered} ROWS BETWEEN CURRENT ROW AND UNBOUNDED FOLLOWING)"),
        ),
        (
            BoundsType::Rows,
            false,
            "preceding",
            "following",
            "OVER (ROWS BETWEEN 2 PRECEDING AND 3 FOLLOWING)".to_string(),
        ),
        (
            BoundsType::Range,
            false,
            "preceding",
            "current",
            whole.to_string(),
        ),
        (
            BoundsType::Range,
            false,
            "current",
            "current",
            whole.to_string(),
        ),
        (
            BoundsType::Range,
            true,
            "unbounded",
            "none",
            format!("{ordered} RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)"),
        ),
        (
            BoundsType::Range,
            true,
            "preceding",
            "following",
            format!("{ordered} RANGE BETWEEN 2 PRECEDING AND 3 FOLLOWING)"),
        ),
        (
            BoundsType::Range,
            true,
            "current",
            "unbounded",
            format!("{ordered} RANGE BETWEEN CURRENT ROW AND UNBOUNDED FOLLOWING)"),
        ),
    ];
    for (bounds_type, is_ordered, lower, upper, expected) in cases {
        assert_eq!(
            expected,
            window_sql(bounds_type, is_ordered, lower, upper),
            "{bounds_type:?} ordered={is_ordered} {lower} {upper}"
        );
    }
}
