use athena_federation_substrait::plan;
use athena_federation_substrait::SqlDialect;
use athena_federation_substrait::SqlNode;
use athena_federation_substrait::SubstraitAccumulatorVisitor;
use std::fmt::Write;

const DIALECTS: [&str; 5] = [
    "ansi",
    "postgres",
    "snowflake",
    "postgres_upper",
    "snowflake_upper",
];

const JAVA_HEAP_EXHAUSTED: [&str; 2] = [
    "std\tSELECT * FROM test_table WHERE int_col NOT IN (7, NULL)",
    "std\tSELECT * FROM test_table WHERE int_col = 5 AND int_col = 6",
];

#[derive(Default)]
struct Case {
    query: String,
    plan: String,
    rows: Vec<(String, String)>,
}

fn load() -> Vec<Case> {
    parse(include_str!("fixtures/calcite-verifier.tsv"))
}

fn parse(text: &str) -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    for line in text.lines() {
        let (key, rest) = line.split_once('\t').unwrap_or((line, ""));
        match key {
            "QUERY" => cases.push(Case {
                query: rest.to_string(),
                ..Case::default()
            }),
            "PLAN" => cases.last_mut().expect("case").plan = rest.to_string(),
            _ => cases
                .last_mut()
                .expect("case")
                .rows
                .push((key.to_string(), rest.to_string())),
        }
    }
    cases
}

fn dialect(name: &str) -> SqlDialect {
    match name {
        "ansi" => SqlDialect::ansi(),
        "postgres" => SqlDialect::postgresql(),
        "snowflake" => SqlDialect::snowflake(),
        "postgres_upper" => SqlDialect::postgresql_with_casing(true),
        "snowflake_upper" => SqlDialect::snowflake_with_casing(true),
        other => panic!("unknown dialect {other}"),
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

fn error_text(e: &athena_federation_substrait::SubstraitError) -> String {
    format!("ERROR {}", esc(&e.to_string()))
}

fn param_row(plan: &str, d: &SqlDialect, node: &SqlNode) -> String {
    let run = || -> athena_federation_substrait::Result<String> {
        let schema = plan::table_schema(plan, d)?;
        let mut accumulator = Vec::new();
        let rewritten = SubstraitAccumulatorVisitor::new(&mut accumulator, &schema).visit(node)?;
        let mut out = esc(&rewritten.to_sql_string(d));
        for v in &accumulator {
            let class = v.value().java_class_name();
            let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
            write!(
                out,
                "\t{}|{}|{}|{}",
                v.get_type(),
                simple,
                esc(&v.value().to_string()),
                v.column_name()
            )
            .unwrap();
        }
        Ok(out)
    };
    run().unwrap_or_else(|e| error_text(&e))
}

fn rust_rows(plan: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for name in DIALECTS {
        let d = dialect(name);
        match plan::to_sql_node(plan, &d) {
            Ok(node) => {
                rows.push((format!("CALCITE_{name}"), esc(&node.to_sql_string(&d))));
                if matches!(node, SqlNode::Select(_)) {
                    rows.push((format!("PARAM_{name}"), param_row(plan, &d, &node)));
                }
            }
            Err(e) => rows.push((format!("CALCITE_{name}"), error_text(&e))),
        }
    }
    let schema = match plan::table_schema(plan, &SqlDialect::ansi()) {
        Ok(t) => esc(&t.get_full_type_string()),
        Err(e) => error_text(&e),
    };
    rows.push(("SCHEMA".to_string(), schema));
    let remap = match plan::column_remapping(plan, &SqlDialect::ansi()) {
        Ok(m) => m.iter().fold(String::new(), |mut acc, (k, v)| {
            write!(acc, "\t{k}={}", v.as_deref().unwrap_or("null")).unwrap();
            acc
        }),
        Err(e) => format!("\t{}", error_text(&e)),
    };
    rows.push((
        "REMAP".to_string(),
        remap.trim_start_matches('\t').to_string(),
    ));
    rows
}

fn diff(case: &Case, actual: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    let keys: Vec<&String> = case.rows.iter().map(|(k, _)| k).collect();
    let actual_keys: Vec<&String> = actual.iter().map(|(k, _)| k).collect();
    if keys != actual_keys {
        out.push(format!(
            "{}\n  expected rows: {keys:?}\n  actual rows:   {actual_keys:?}",
            case.query
        ));
    }
    for (key, expected) in &case.rows {
        let got = actual
            .iter()
            .find(|(k, _)| k == key)
            .map_or("<missing>", |(_, v)| v.as_str());
        if got != expected {
            out.push(format!(
                "{key} {}\n  expected: {expected}\n  actual:   {got}",
                case.query
            ));
        }
    }
    out
}

fn check(cases: &[Case]) {
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut matched = 0;
    for case in cases
        .iter()
        .filter(|c| !JAVA_HEAP_EXHAUSTED.contains(&c.query.as_str()))
    {
        let actual = rust_rows(&case.plan);
        checked += case.rows.len();
        matched += case
            .rows
            .iter()
            .filter(|(k, v)| actual.iter().any(|(ak, av)| ak == k && av == v))
            .count();
        failures.extend(diff(case, &actual));
    }
    assert!(
        failures.is_empty(),
        "{} differences, {matched} of {checked} rows match:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn calcite_verifier_oracle_matches() {
    let cases = load();
    assert_eq!(cases.len(), 111);
    check(&cases);
}

#[test]
fn calcite_verifier_extra_oracle_matches() {
    let cases = parse(include_str!("fixtures/calcite-verifier-extra.tsv"));
    assert_eq!(cases.len(), 43);
    check(&cases);
}

#[test]
fn java_heap_exhaustion_cases_render_without_hanging() {
    let cases = load();
    for query in JAVA_HEAP_EXHAUSTED {
        let case = cases.iter().find(|c| c.query == query).expect("case");
        assert!(case.rows.iter().all(|(_, v)| v == "ERROR Java heap space"));
        let actual = rust_rows(&case.plan);
        let ansi = &actual
            .iter()
            .find(|(k, _)| k == "CALCITE_ansi")
            .expect("ansi")
            .1;
        assert_eq!(
            ansi,
            "SELECT *\\nFROM (VALUES (NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)) AS `t` (`bool_col`, `tinyint_col`, `smallint_col`, `int_col`, `bigint_col`, `float_col`, `double_col`, `real_col`, `varchar_col`, `char_col`, `binary_col`, `varbinary_col`, `date_col`, `time_col`, `timestamp_col`, `decimal_col`, `decimal_col2`, `decimal_col3`, `array_col`, `int_array_col`, `map_col`, `map_with_decimal`, `nested_array`)\\nWHERE 1 = 0"
        );
    }
}
