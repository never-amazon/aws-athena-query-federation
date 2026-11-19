use athena_federation_substrait::plan;
use athena_federation_substrait::proto::expression::field_reference::{
    ReferenceType, RootReference, RootType,
};
use athena_federation_substrait::proto::expression::literal::interval_day_to_second::PrecisionMode;
use athena_federation_substrait::proto::expression::literal::{self, LiteralType};
use athena_federation_substrait::proto::expression::reference_segment::{self, StructField};
use athena_federation_substrait::proto::expression::{
    Cast, FieldReference, Literal, ReferenceSegment, RexType,
};
use athena_federation_substrait::proto::plan_rel;
use athena_federation_substrait::proto::r#type::{self, Kind, Nullability};
use athena_federation_substrait::proto::read_rel::{NamedTable, ReadType};
use athena_federation_substrait::proto::rel::RelType;
use athena_federation_substrait::proto::{
    Expression, NamedStruct, Plan, PlanRel, ProjectRel, ReadRel, Rel, RelRoot, Type,
};
use athena_federation_substrait::SqlDialect;
use base64::Engine;
use prost::Message;
use std::error::Error;

const NULLABLE: i32 = Nullability::Nullable as i32;
const REQUIRED: i32 = Nullability::Required as i32;

fn rel(rel_type: RelType) -> Rel {
    Rel {
        rel_type: Some(rel_type),
    }
}

fn ty(kind: Kind) -> Type {
    Type { kind: Some(kind) }
}

fn i64_type() -> Type {
    ty(Kind::I64(r#type::I64 {
        type_variation_reference: 0,
        nullability: REQUIRED,
    }))
}

fn varchar(length: i32) -> Type {
    ty(Kind::Varchar(r#type::VarChar {
        length,
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn fixed_char(length: i32) -> Type {
    ty(Kind::FixedChar(r#type::FixedChar {
        length,
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn decimal(precision: i32, scale: i32) -> Type {
    ty(Kind::Decimal(r#type::Decimal {
        scale,
        precision,
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn timestamp(precision: i32) -> Type {
    ty(Kind::PrecisionTimestamp(r#type::PrecisionTimestamp {
        precision,
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn interval_day(precision: Option<i32>) -> Type {
    ty(Kind::IntervalDay(r#type::IntervalDay {
        precision,
        type_variation_reference: 0,
        nullability: NULLABLE,
    }))
}

fn schema() -> Vec<(&'static str, Type)> {
    vec![
        ("i", i64_type()),
        (
            "s",
            ty(Kind::String(r#type::String {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        ("vc", varchar(10)),
        ("fc", fixed_char(3)),
        ("dec", decimal(10, 2)),
        ("ts", timestamp(3)),
        (
            "tm",
            ty(Kind::PrecisionTime(r#type::PrecisionTime {
                precision: 0,
                type_variation_reference: 0,
                nullability: REQUIRED,
            })),
        ),
        (
            "bin",
            ty(Kind::Binary(r#type::Binary {
                type_variation_reference: 0,
                nullability: NULLABLE,
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
            "iy",
            ty(Kind::IntervalYear(r#type::IntervalYear {
                type_variation_reference: 0,
                nullability: NULLABLE,
            })),
        ),
        ("id_default", interval_day(None)),
        ("id_three", interval_day(Some(3))),
        ("id_unset", interval_day(Some(-1))),
        ("id_min", interval_day(Some(i32::MIN))),
        (
            "lst",
            ty(Kind::List(Box::new(r#type::List {
                r#type: Some(Box::new(varchar(5))),
                type_variation_reference: 0,
                nullability: NULLABLE,
            }))),
        ),
        (
            "mp",
            ty(Kind::Map(Box::new(r#type::Map {
                key: Some(Box::new(fixed_char(2))),
                value: Some(Box::new(decimal(5, 1))),
                type_variation_reference: 0,
                nullability: NULLABLE,
            }))),
        ),
        ("vc_wide", varchar(-5)),
    ]
}

fn read() -> Rel {
    let (names, types): (Vec<String>, Vec<Type>) = schema()
        .into_iter()
        .map(|(n, t)| (n.to_string(), t))
        .unzip();
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

fn column(name: &str) -> Expression {
    let index = schema()
        .iter()
        .position(|(n, _)| *n == name)
        .expect("column");
    Expression {
        rex_type: Some(RexType::Selection(Box::new(FieldReference {
            reference_type: Some(ReferenceType::DirectReference(ReferenceSegment {
                reference_type: Some(reference_segment::ReferenceType::StructField(Box::new(
                    StructField {
                        field: i32::try_from(index).expect("index"),
                        child: None,
                    },
                ))),
            })),
            root_type: Some(RootType::RootReference(RootReference {})),
        }))),
    }
}

fn lit(literal_type: LiteralType) -> Expression {
    Expression {
        rex_type: Some(RexType::Literal(Literal {
            nullable: false,
            type_variation_reference: 0,
            literal_type: Some(literal_type),
        })),
    }
}

fn cast(input: Expression, target: Type) -> Expression {
    Expression {
        rex_type: Some(RexType::Cast(Box::new(Cast {
            r#type: Some(target),
            input: Some(Box::new(input)),
            failure_behavior: 0,
        }))),
    }
}

fn encode(root: Rel) -> String {
    let plan = Plan {
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

fn project(expressions: Vec<Expression>) -> String {
    encode(rel(RelType::Project(Box::new(ProjectRel {
        input: Some(Box::new(read())),
        expressions,
        ..Default::default()
    }))))
}

fn select_list(plan_string: &str, dialect: &SqlDialect) -> String {
    let sql = match plan::to_sql_node(plan_string, dialect) {
        Ok(node) => node.to_sql_string(dialect).replace('\n', " "),
        Err(e) => {
            let mut root: &dyn Error = &e;
            while let Some(next) = root.source() {
                root = next;
            }
            return format!("error: {root}");
        }
    };
    let start = ["`vc_wide`, ", "\"vc_wide\", "]
        .iter()
        .find_map(|marker| sql.find(marker).map(|i| i + marker.len()))
        .unwrap_or(0);
    let end = sql.rfind(" FROM").unwrap_or(sql.len());
    sql[start..end].to_string()
}

fn cast_cases() -> Vec<(&'static str, Expression)> {
    vec![
        ("varchar", cast(column("i"), varchar(10))),
        ("varchar_wide", cast(column("i"), varchar(-5))),
        (
            "string",
            cast(
                column("i"),
                ty(Kind::String(r#type::String {
                    type_variation_reference: 0,
                    nullability: NULLABLE,
                })),
            ),
        ),
        ("char", cast(column("i"), fixed_char(3))),
        ("decimal", cast(column("i"), decimal(10, 2))),
        ("decimal_clamped", cast(column("i"), decimal(30, 25))),
        ("timestamp", cast(column("s"), timestamp(3))),
        ("bigint", cast(column("s"), i64_type())),
        (
            "double",
            cast(
                column("i"),
                ty(Kind::Fp64(r#type::Fp64 {
                    type_variation_reference: 0,
                    nullability: NULLABLE,
                })),
            ),
        ),
        (
            "tinyint",
            cast(
                column("i"),
                ty(Kind::I8(r#type::I8 {
                    type_variation_reference: 0,
                    nullability: NULLABLE,
                })),
            ),
        ),
        (
            "fixed_binary",
            cast(
                column("s"),
                ty(Kind::FixedBinary(r#type::FixedBinary {
                    length: 4,
                    type_variation_reference: 0,
                    nullability: NULLABLE,
                })),
            ),
        ),
        (
            "null_interval",
            lit(LiteralType::Null(interval_day(Some(3)))),
        ),
        ("null_varchar", lit(LiteralType::Null(varchar(7)))),
    ]
}

fn literal_cases() -> Vec<(&'static str, Expression)> {
    vec![
        (
            "interval_day",
            lit(LiteralType::IntervalDayToSecond(
                literal::IntervalDayToSecond {
                    days: 1,
                    seconds: 2,
                    subseconds: 345,
                    precision_mode: Some(PrecisionMode::Precision(3)),
                },
            )),
        ),
        (
            "interval_year",
            lit(LiteralType::IntervalYearToMonth(
                literal::IntervalYearToMonth {
                    years: 1,
                    months: 2,
                },
            )),
        ),
        (
            "timestamp_3",
            lit(LiteralType::PrecisionTimestamp(
                literal::PrecisionTimestamp {
                    precision: 3,
                    value: 1_234_567,
                },
            )),
        ),
        (
            "timestamp_0",
            lit(LiteralType::PrecisionTimestamp(
                literal::PrecisionTimestamp {
                    precision: 0,
                    value: 12,
                },
            )),
        ),
        (
            "timestamp_negative",
            lit(LiteralType::PrecisionTimestamp(
                literal::PrecisionTimestamp {
                    precision: -2,
                    value: 12,
                },
            )),
        ),
        ("time", lit(LiteralType::Time(45_296_789_000))),
        (
            "varchar_fits",
            lit(LiteralType::VarChar(literal::VarChar {
                value: "abc".into(),
                length: 5,
            })),
        ),
        (
            "varchar_longer",
            lit(LiteralType::VarChar(literal::VarChar {
                value: "abcdef".into(),
                length: 2,
            })),
        ),
        (
            "varchar_unbounded",
            lit(LiteralType::VarChar(literal::VarChar {
                value: "abcdef".into(),
                length: u32::MAX,
            })),
        ),
        (
            "decimal_literal",
            lit(LiteralType::Decimal(literal::Decimal {
                value: {
                    let mut v = vec![0u8; 16];
                    v[0] = 0x39;
                    v[1] = 0x30;
                    v
                },
                precision: 6,
                scale: 2,
            })),
        ),
    ]
}

fn expected(name: &str) -> (&'static str, &'static str) {
    match name {
        "varchar" => (
            "CAST(`i` AS VARCHAR(10) CHARACTER SET `UTF-8`) AS `i0`",
            "CAST(\"i\" AS VARCHAR(10)) AS \"i0\"",
        ),
        "varchar_wide" | "string" => (
            "CAST(`i` AS VARCHAR CHARACTER SET `UTF-8`) AS `i0`",
            "CAST(\"i\" AS VARCHAR) AS \"i0\"",
        ),
        "char" => (
            "CAST(`i` AS CHAR(3) CHARACTER SET `UTF-8`) AS `i0`",
            "CAST(\"i\" AS CHAR(3)) AS \"i0\"",
        ),
        "decimal" => (
            "CAST(`i` AS DECIMAL(10, 2)) AS `i0`",
            "CAST(\"i\" AS DECIMAL(10, 2)) AS \"i0\"",
        ),
        "decimal_clamped" => (
            "CAST(`i` AS DECIMAL(19, 19)) AS `i0`",
            "CAST(\"i\" AS DECIMAL(19, 19)) AS \"i0\"",
        ),
        "timestamp" => (
            "CAST(`s` AS TIMESTAMP(3)) AS `s0`",
            "CAST(\"s\" AS TIMESTAMP(3)) AS \"s0\"",
        ),
        "bigint" => (
            "CAST(`s` AS BIGINT) AS `s0`",
            "CAST(\"s\" AS BIGINT) AS \"s0\"",
        ),
        "double" => (
            "CAST(`i` AS DOUBLE) AS `i0`",
            "CAST(\"i\" AS DOUBLE PRECISION) AS \"i0\"",
        ),
        "tinyint" => (
            "CAST(`i` AS TINYINT) AS `i0`",
            "CAST(\"i\" AS SMALLINT) AS \"i0\"",
        ),
        "fixed_binary" => (
            "CAST(`s` AS BINARY(4)) AS `s0`",
            "CAST(\"s\" AS BINARY(4)) AS \"s0\"",
        ),
        "null_interval" => (
            "error: Unsupported type when convertTypeToSpec: INTERVAL_DAY_SECOND",
            "error: Unsupported type when convertTypeToSpec: INTERVAL_DAY_SECOND",
        ),
        "null_varchar" => (
            "CAST(NULL AS VARCHAR(7) CHARACTER SET `UTF-8`) AS `$f17`",
            "CAST(NULL AS VARCHAR(7)) AS \"$f17\"",
        ),
        "interval_day" => (
            "INTERVAL '1 00:00:02.345' DAY TO SECOND(3) AS `$f17`",
            "INTERVAL '1 00:00:02.345' DAY TO SECOND(3) AS \"$f17\"",
        ),
        "interval_year" => (
            "INTERVAL '1-2' YEAR TO MONTH AS `$f17`",
            "INTERVAL '1-2' YEAR TO MONTH AS \"$f17\"",
        ),
        "timestamp_3" => (
            "TIMESTAMP '1970-01-01 00:20:34.567' AS `$f17`",
            "TIMESTAMP '1970-01-01 00:20:34.567' AS \"$f17\"",
        ),
        "timestamp_0" => (
            "TIMESTAMP '1970-01-01 00:00:12' AS `$f17`",
            "TIMESTAMP '1970-01-01 00:00:12' AS \"$f17\"",
        ),
        "timestamp_negative" => (
            "error: Cannot handle PrecisionTimestamp with precision -2.",
            "error: Cannot handle PrecisionTimestamp with precision -2.",
        ),
        "time" => (
            "TIME '12:34:56.789' AS `$f17`",
            "TIME '12:34:56.789' AS \"$f17\"",
        ),
        "varchar_fits" => ("'abc' AS `$f17`", "'abc' AS \"$f17\""),
        "varchar_longer" => ("'ab' AS `$f17`", "'ab' AS \"$f17\""),
        "varchar_unbounded" => ("'abcdef' AS `$f17`", "'abcdef' AS \"$f17\""),
        "decimal_literal" => ("123.45 AS `$f17`", "123.45 AS \"$f17\""),
        other => panic!("no expected SQL for {other}"),
    }
}

#[test]
fn table_schema_prints_precision_scale_and_interval_qualifiers() {
    let schema = plan::table_schema(&encode(read()), &SqlDialect::ansi()).expect("schema");
    assert_eq!(
        "RecordType(BIGINT NOT NULL i, VARCHAR CHARACTER SET \"UTF-8\" s, \
         VARCHAR(10) CHARACTER SET \"UTF-8\" vc, CHAR(3) CHARACTER SET \"UTF-8\" fc, \
         DECIMAL(10, 2) dec, TIMESTAMP(3) ts, TIME(0) NOT NULL tm, VARBINARY bin, BINARY(4) fb, \
         INTERVAL YEAR TO MONTH iy, INTERVAL DAY TO SECOND(6) id_default, \
         INTERVAL DAY TO SECOND(3) id_three, INTERVAL DAY TO SECOND id_unset, \
         INTERVAL DAY TO SECOND(-2147483648) id_min, \
         VARCHAR(5) CHARACTER SET \"UTF-8\" ARRAY lst, \
         (CHAR(2) CHARACTER SET \"UTF-8\", DECIMAL(5, 1)) MAP mp, \
         VARCHAR(-5) CHARACTER SET \"UTF-8\" vc_wide) NOT NULL",
        schema.full_type_string()
    );
}

#[test]
fn cast_targets_and_literals_print_precision_and_scale() {
    for (name, e) in cast_cases().into_iter().chain(literal_cases()) {
        let p = project(vec![e]);
        let (ansi, postgresql) = expected(name);
        assert_eq!(ansi, select_list(&p, &SqlDialect::ansi()), "{name}");
        assert_eq!(
            postgresql,
            select_list(&p, &SqlDialect::postgresql()),
            "{name}"
        );
    }
}
