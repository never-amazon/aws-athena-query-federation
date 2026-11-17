use athena_federation_substrait::SqlDialect;
use athena_federation_substrait::SubstraitRelUtils;
use athena_federation_substrait::SubstraitSqlUtils;
use base64::Engine;

fn dialect() -> SqlDialect {
    SqlDialect::ansi()
}

fn encoded_plan(query: &str) -> String {
    let mut lines = include_str!("fixtures/calcite-extended.tsv").lines();
    while let Some(line) = lines.next() {
        if line == format!("QUERY\tstd\t{query}") {
            let plan = lines
                .next()
                .and_then(|l| l.strip_prefix("PLAN\t"))
                .expect("plan row");
            return plan.to_string();
        }
    }
    panic!("no fixture plan for {query}");
}

fn sql(query: &str) -> String {
    let d = dialect();
    SubstraitSqlUtils::get_sql_node_from_substrait_plan(&encoded_plan(query), &d)
        .expect("sql node")
        .to_sql_string(&d)
}

#[test]
fn test_get_table_schema_from_substrait_plan_all_arrow_types() {
    let schema = SubstraitSqlUtils::get_table_schema_from_substrait_plan(
        &encoded_plan("SELECT * FROM test_table LIMIT 1"),
        &dialect(),
    )
    .expect("schema");
    assert!(schema.field_count() > 0);
    let names = schema.field_names();
    assert!(
        names.contains(&"bool_col".to_string()),
        "Missing boolean type"
    );
    assert!(
        names.contains(&"array_col".to_string()),
        "Missing array type"
    );
    assert!(names.contains(&"map_col".to_string()), "Missing map type");
}

#[test]
fn test_get_table_schema_from_substrait_plan_basic_types() {
    let schema = SubstraitSqlUtils::get_table_schema_from_substrait_plan(
        &encoded_plan("SELECT * FROM test_table"),
        &dialect(),
    )
    .expect("schema");
    assert!(!schema.field_list().is_empty());
    let names = schema.field_names();
    assert!(names.contains(&"varchar_col".to_string()));
    assert!(names.contains(&"int_col".to_string()));
}

#[test]
fn test_get_table_schema_from_substrait_plan_with_where_clause() {
    let schema = SubstraitSqlUtils::get_table_schema_from_substrait_plan(
        &encoded_plan("SELECT * FROM test_table WHERE varchar_col = 'test'"),
        &dialect(),
    )
    .expect("schema");
    assert!(schema.field_count() > 0);
}

#[test]
fn test_get_table_schema_from_substrait_plan_complex_query() {
    let schema = SubstraitSqlUtils::get_table_schema_from_substrait_plan(
        &encoded_plan(
            "SELECT * FROM test_table WHERE varchar_col LIKE 'A%' ORDER BY int_col DESC LIMIT 10",
        ),
        &dialect(),
    )
    .expect("schema");
    assert!(schema.field_count() > 0);
}

#[test]
fn test_get_sql_node_from_substrait_plan_invalid_base64() {
    let err = SubstraitSqlUtils::get_sql_node_from_substrait_plan(
        "This is not valid base64!!!@#$",
        &dialect(),
    )
    .expect_err("invalid base64");
    assert!(err.message().contains("Failed to parse Substrait plan"));
}

#[test]
fn test_get_sql_node_from_substrait_plan_empty_string() {
    let err =
        SubstraitSqlUtils::get_sql_node_from_substrait_plan("", &dialect()).expect_err("empty");
    assert!(err.message().contains("Failed to parse Substrait plan"));
}

#[test]
fn test_get_sql_node_from_substrait_plan_invalid_protobuf() {
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"invalid protobuf data");
    let err = SubstraitSqlUtils::get_sql_node_from_substrait_plan(&encoded, &dialect())
        .expect_err("invalid proto");
    assert!(
        err.message().contains("Failed to parse Substrait plan")
            || err
                .message()
                .contains("Failed to convert from Substrait plan to RelNode")
    );
}

#[test]
fn test_get_table_schema_from_substrait_plan_invalid_base64() {
    let err = SubstraitSqlUtils::get_table_schema_from_substrait_plan(
        "Not valid base64 content!!!",
        &dialect(),
    )
    .expect_err("invalid base64");
    assert!(err
        .message()
        .contains("Failed to extract table schema from Substrait plan"));
}

#[test]
fn test_get_table_schema_from_substrait_plan_empty_plan() {
    let err =
        SubstraitSqlUtils::get_table_schema_from_substrait_plan("", &dialect()).expect_err("empty");
    assert!(err
        .message()
        .contains("Failed to extract table schema from Substrait plan"));
}

#[test]
fn test_sql_conversion_is_not_null() {
    assert!(!sql("SELECT * FROM test_table WHERE varchar_col IS NOT NULL").is_empty());
}

#[test]
fn test_sql_conversion_between() {
    assert!(!sql("SELECT * FROM test_table WHERE int_col BETWEEN 10 AND 100").is_empty());
}

#[test]
fn test_sql_conversion_case_expression() {
    assert!(!sql(
        "SELECT CASE WHEN int_col > 100 THEN CAST('high' AS VARCHAR) ELSE CAST('low' AS VARCHAR) END FROM test_table"
    )
    .is_empty());
}

#[test]
fn test_sql_conversion_multiple_aggregations() {
    assert!(!sql("SELECT varchar_col, COUNT(*), SUM(int_col), AVG(int_col) FROM test_table GROUP BY varchar_col")
        .is_empty());
}

#[test]
fn test_sql_conversion_not_in() {
    let s = sql("SELECT * FROM test_table WHERE varchar_col NOT IN ('val1', 'val2')");
    assert!(s.contains("NOT IN") || s.contains("<>"));
}

#[test]
fn test_sql_conversion_having() {
    assert!(!sql(
        "SELECT varchar_col, COUNT(*) FROM test_table GROUP BY varchar_col HAVING COUNT(*) > 5"
    )
    .is_empty());
}

#[test]
fn test_sql_conversion_distinct() {
    let s = sql("SELECT DISTINCT varchar_col FROM test_table").to_uppercase();
    assert!(s.contains("DISTINCT") || s.contains("GROUP BY"));
}

#[test]
fn test_sql_conversion_complex_arithmetic() {
    assert!(!sql("SELECT (int_col * 2 + 10) / 3 AS calculated_result FROM test_table").is_empty());
}

fn provide_sql_test_cases() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "SELECT * FROM test_table WHERE varchar_col = 'value1' AND int_col > 100",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` = 'value1' AND `int_col` > 100",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col = 'value1' OR int_col < 50",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` = 'value1' OR `int_col` < 50",
        ),
        (
            "SELECT * FROM test_table WHERE NOT (varchar_col = 'excluded_value')",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` <> 'excluded_value'",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col != 'excluded_value'",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` <> 'excluded_value'",
        ),
        (
            "SELECT * FROM test_table WHERE int_col < 100",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `int_col` < 100",
        ),
        (
            "SELECT * FROM test_table WHERE int_col >= 50",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `int_col` >= 50",
        ),
        (
            "SELECT varchar_col, int_col + 10 as added_value FROM test_table WHERE int_col + 5 > 100",
            "SELECT `varchar_col` AS `varchar_col0`, `int_col` + 10 AS `$f24`\nFROM `test_schema`.`test_table`\nWHERE `int_col` + 5 > 100",
        ),
        (
            "SELECT * FROM test_table WHERE int_col % 2 = 0",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE MOD(`int_col`, 2) = 0",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col LIKE 'prefix%'",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` LIKE 'prefix%'",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col IN ('value1', 'value2', 'value3')",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` IN ('value1', 'value2', 'value3')",
        ),
        (
            "SELECT * FROM test_table WHERE int_col IN (10, 20, 30)",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `int_col` IN (10, 20, 30)",
        ),
        (
            "SELECT * FROM test_table LIMIT 10",
            "SELECT *\nFROM `test_schema`.`test_table`\nFETCH NEXT 10 ROWS ONLY",
        ),
        (
            "SELECT * FROM test_table ORDER BY varchar_col ASC",
            "SELECT *\nFROM `test_schema`.`test_table`\nORDER BY `varchar_col`",
        ),
        (
            "SELECT * FROM test_table ORDER BY varchar_col DESC",
            "SELECT *\nFROM `test_schema`.`test_table`\nORDER BY `varchar_col` DESC",
        ),
        (
            "SELECT * FROM test_table ORDER BY varchar_col ASC, int_col DESC",
            "SELECT *\nFROM `test_schema`.`test_table`\nORDER BY `varchar_col`, `int_col` DESC",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col LIKE 'A%' AND int_col > 100 ORDER BY varchar_col LIMIT 25",
            "SELECT *\nFROM `test_schema`.`test_table`\nWHERE `varchar_col` LIKE 'A%' AND `int_col` > 100\nORDER BY `varchar_col`\nFETCH NEXT 25 ROWS ONLY",
        ),
        (
            "SELECT varchar_col, COUNT(*) FROM test_table WHERE varchar_col IS NOT NULL GROUP BY varchar_col ORDER BY COUNT(*) DESC LIMIT 50",
            "SELECT `varchar_col` AS `varchar_col0`, COUNT(*) AS `$f1`\nFROM `test_schema`.`test_table`\nGROUP BY `varchar_col`\nORDER BY 2 DESC\nFETCH NEXT 50 ROWS ONLY",
        ),
    ]
}

#[test]
fn test_sql_conversion() {
    for (input, expected) in provide_sql_test_cases() {
        let plan = encoded_plan(input);
        assert_eq!(sql(input), expected, "{input}");
        SubstraitSqlUtils::get_table_schema_from_substrait_plan(&plan, &dialect()).expect("schema");
    }
}

#[test]
fn test_substrait_plan_generation() {
    let encoded = encoded_plan("SELECT * FROM test_table LIMIT 10");
    let plan = SubstraitRelUtils::deserialize_substrait_plan(&encoded).expect("plan");
    assert!(!plan.relations.is_empty());
}
