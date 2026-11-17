use athena_federation_substrait::SqlDialect;
use athena_federation_substrait::SubstraitAccumulatorVisitor;
use athena_federation_substrait::SubstraitSqlUtils;
use std::fmt::Write;

#[derive(Default)]
struct Case {
    query: String,
    plan: String,
    rows: Vec<(String, String)>,
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') | None => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
        }
    }
    out
}

fn load() -> Vec<Case> {
    let mut cases: Vec<Case> = Vec::new();
    for line in include_str!("fixtures/calcite-extended.tsv").lines() {
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

fn error_text(e: &athena_federation_substrait::SubstraitError) -> String {
    format!("ERROR {}", e.message().replace('\n', "\\n"))
}

fn calcite_row(plan: &str, d: &SqlDialect) -> String {
    match SubstraitSqlUtils::get_sql_node_from_substrait_plan(plan, d) {
        Ok(node) => node.to_sql_string(d).replace('\n', "\\n"),
        Err(e) => error_text(&e),
    }
}

fn param_row(plan: &str, d: &SqlDialect) -> Option<String> {
    let node = SubstraitSqlUtils::get_sql_node_from_substrait_plan(plan, d).ok()?;
    let run = || -> athena_federation_substrait::Result<String> {
        let schema = SubstraitSqlUtils::get_table_schema_from_substrait_plan(plan, d)?;
        let mut accumulator = Vec::new();
        let rewritten = SubstraitAccumulatorVisitor::new(&mut accumulator, &schema).visit(&node)?;
        let mut out = rewritten.to_sql_string(d).replace('\n', "\\n");
        for v in &accumulator {
            let class = v.get_value().java_class_name();
            let simple = class.rsplit('.').next().unwrap_or(class);
            write!(
                out,
                "\t{}|{}|{}|{}",
                v.get_type(),
                simple,
                v.get_value(),
                v.get_column_name()
            )
            .unwrap();
        }
        Ok(out)
    };
    Some(run().unwrap_or_else(|e| error_text(&e)))
}

fn schema_row(plan: &str) -> String {
    match SubstraitSqlUtils::get_table_schema_from_substrait_plan(plan, &SqlDialect::ansi()) {
        Ok(t) => t.get_full_type_string(),
        Err(e) => error_text(&e),
    }
}

fn remap_row(plan: &str) -> String {
    match SubstraitSqlUtils::get_column_remapping(plan, &SqlDialect::ansi()) {
        Ok(m) => m
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_deref().unwrap_or("null")))
            .collect::<Vec<_>>()
            .join("\t"),
        Err(e) => error_text(&e),
    }
}

#[test]
fn calcite_extended_oracle_matches() {
    let cases = load();
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in &cases {
        for (key, expected) in &case.rows {
            let actual = if let Some(d) = key.strip_prefix("CALCITE_") {
                calcite_row(&case.plan, &dialect(d))
            } else if let Some(d) = key.strip_prefix("PARAM_") {
                param_row(&case.plan, &dialect(d)).unwrap_or_default()
            } else if key == "SCHEMA" {
                schema_row(&case.plan)
            } else if key == "REMAP" {
                remap_row(&case.plan)
            } else {
                continue;
            };
            checked += 1;
            if unescape(&actual) != unescape(expected) {
                failures.push(format!(
                    "{key} {}\n  expected: {expected}\n  actual:   {actual}",
                    case.query
                ));
            }
        }
    }
    assert_eq!(checked, 159 * 12);
    assert!(
        failures.is_empty(),
        "{} of {checked} rows differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
