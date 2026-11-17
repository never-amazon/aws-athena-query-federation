use athena_federation_substrait::SqlDialect;
use athena_federation_substrait::SubstraitAccumulatorVisitor;
use athena_federation_substrait::SubstraitSqlUtils;

struct Case {
    query: String,
    plan: String,
    sql: Vec<(String, String)>,
    params: Vec<(String, String, String)>,
    remap: String,
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn load() -> Vec<Case> {
    let text = include_str!("fixtures/calcite-reference.tsv");
    let mut cases: Vec<Case> = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(2, '\t');
        let key = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default();
        match key {
            "QUERY" => cases.push(Case {
                query: rest.to_string(),
                plan: String::new(),
                sql: Vec::new(),
                params: Vec::new(),
                remap: String::new(),
            }),
            "REMAP" => cases.last_mut().expect("case").remap = rest.to_string(),
            "PLAN" => cases.last_mut().expect("case").plan = rest.to_string(),
            k if k.starts_with("CALCITE_") => cases
                .last_mut()
                .expect("case")
                .sql
                .push((k["CALCITE_".len()..].to_string(), unescape(rest))),
            k if k.starts_with("PARAM_") => {
                let mut fields = rest.splitn(2, '\t');
                let sql = unescape(fields.next().unwrap_or_default());
                let values = fields.next().unwrap_or_default().to_string();
                cases.last_mut().expect("case").params.push((
                    k["PARAM_".len()..].to_string(),
                    sql,
                    values,
                ));
            }
            _ => {}
        }
    }
    cases
}

fn dialect(name: &str) -> SqlDialect {
    match name {
        "ansi" => SqlDialect::ansi(),
        "postgres" => SqlDialect::postgresql(),
        "snowflake" => SqlDialect::snowflake(),
        other => panic!("unknown dialect {other}"),
    }
}

#[test]
fn calcite_reference_sql_matches() {
    let cases = load();
    assert_eq!(cases.len(), 23);
    let mut failures = Vec::new();
    let mut total = 0;
    for case in &cases {
        for (d, expected) in &case.sql {
            total += 1;
            let actual =
                SubstraitSqlUtils::get_sql_node_from_substrait_plan(&case.plan, &dialect(d))
                    .map(|n| n.to_sql_string(&dialect(d)));
            match actual {
                Ok(a) if &a == expected => {}
                Ok(a) => failures.push(format!(
                    "[{d}] {}\n  expected: {expected:?}\n  actual:   {a:?}",
                    case.query
                )),
                Err(e) => failures.push(format!("[{d}] {}\n  error: {e:?}", case.query)),
            }
        }
    }
    assert_eq!(total, 69);
    assert!(
        failures.is_empty(),
        "{} of {total} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn parameterize(plan: &str, d: &SqlDialect) -> Result<(String, String), String> {
    let node = SubstraitSqlUtils::get_sql_node_from_substrait_plan(plan, d)
        .map_err(|e| format!("{e:?}"))?;
    let schema = SubstraitSqlUtils::get_table_schema_from_substrait_plan(plan, d)
        .map_err(|e| format!("{e:?}"))?;
    let mut accumulator = Vec::new();
    let rewritten = SubstraitAccumulatorVisitor::new(&mut accumulator, &schema)
        .visit(&node)
        .map_err(|e| format!("{e:?}"))?;
    let values: Vec<String> = accumulator.iter().map(ToString::to_string).collect();
    Ok((
        rewritten.to_sql_string(d),
        format!("[{}]", values.join(", ")),
    ))
}

#[test]
fn calcite_reference_params_match() {
    let cases = load();
    let mut failures = Vec::new();
    let mut total = 0;
    for case in &cases {
        for (d, expected_sql, expected_values) in &case.params {
            total += 1;
            match parameterize(&case.plan, &dialect(d)) {
                Ok((sql, values)) if &sql == expected_sql && &values == expected_values => {}
                Ok((sql, values)) => failures.push(format!(
                    "[{d}] {}\n  expected: {expected_sql:?} {expected_values}\n  actual:   {sql:?} {values}",
                    case.query
                )),
                Err(e) => failures.push(format!("[{d}] {}\n  error: {e}", case.query)),
            }
        }
    }
    assert_eq!(total, 69);
    assert!(
        failures.is_empty(),
        "{} of {total} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn calcite_reference_column_remapping_matches() {
    let cases = load();
    let mut failures = Vec::new();
    for case in &cases {
        let remap = SubstraitSqlUtils::get_column_remapping(&case.plan, &SqlDialect::ansi())
            .expect("remap");
        let entries: Vec<String> = remap
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_deref().unwrap_or("null")))
            .collect();
        let actual = format!("{{{}}}", entries.join(", "));
        if actual != case.remap {
            failures.push(format!(
                "{}\n  expected: {}\n  actual:   {actual}",
                case.query, case.remap
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
