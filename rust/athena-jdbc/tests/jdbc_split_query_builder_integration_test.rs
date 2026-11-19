mod jdbc_support;

use std::collections::HashMap;

use indexmap::IndexMap;

use athena_federation_sdk::data::java_format::big_decimal_to_string;
use athena_federation_sdk::domain::predicate::{Constraints, QueryPlan, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{SplitDialect, SplitQueryBuilder};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement, SqlError, SqlValue};

use jdbc_support::{FakeConnection, ResultSetData};

const QUOTE_CHAR: &str = "\"";

const JAVA_PROBE: &str = include_str!("fixtures/split_query_builder_integration.tsv");

const CALCITE_SQL_TEST_CASES: [(&str, &str, &str, usize); 47] = [
    ("Basic Data Retrieval", "Simple select", "SELECT * FROM test_table LIMIT 10", 3),
    ("Basic Data Retrieval", "Specific columns", "SELECT int_col, varchar_col, bigint_col, decimal_col FROM test_table LIMIT 100", 3),
    ("Basic Data Retrieval", "Count records", "SELECT COUNT(*) FROM test_table", 1),
    ("Basic Data Retrieval", "Distinct values", "SELECT DISTINCT bool_col FROM test_table", 2),
    ("Basic Data Retrieval", "Distinct years", "SELECT DISTINCT EXTRACT(YEAR FROM date_col) as \"year\" FROM test_table", 3),
    ("Filtering & Conditions", "Numeric filter", "SELECT * FROM test_table WHERE int_col > 1000", 2),
    ("Filtering & Conditions", "Range filter", "SELECT * FROM test_table WHERE int_col BETWEEN 100 AND 500", 0),
    ("Filtering & Conditions", "String exact match", "SELECT * FROM test_table WHERE varchar_col = 'varchar_value'", 1),
    ("Filtering & Conditions", "String pattern LIKE", "SELECT * FROM test_table WHERE varchar_col LIKE '%varchar%'", 2),
    ("Filtering & Conditions", "IN clause", "SELECT * FROM test_table WHERE varchar_col IN ('varchar_value', 'another_varchar', 'third_value')", 3),
    ("Filtering & Conditions", "Year filter", "SELECT * FROM test_table WHERE EXTRACT(YEAR FROM date_col) = 2020", 1),
    ("Filtering & Conditions", "Year range", "SELECT * FROM test_table WHERE EXTRACT(YEAR FROM date_col) >= 2020", 2),
    ("Filtering & Conditions", "Not null check", "SELECT * FROM test_table WHERE decimal_col IS NOT NULL", 3),
    ("Filtering & Conditions", "Boolean filters", "SELECT * FROM test_table WHERE bool_col = true ORDER BY timestamp_col", 2),
    ("Data Type Conversions", "Price conversion", "SELECT int_col, CAST(decimal_col AS DOUBLE) as price_num FROM test_table", 3),
    ("Data Type Conversions", "Year conversion", "SELECT int_col, CAST(EXTRACT(YEAR FROM date_col) AS INTEGER) as year_int FROM test_table", 3),
    ("Aggregations", "Basic stats", "SELECT COUNT(*), AVG(CAST(decimal_col AS DOUBLE)) FROM test_table", 1),
    ("Aggregations", "Min/Max year", "SELECT MAX(EXTRACT(YEAR FROM date_col)), MIN(EXTRACT(YEAR FROM date_col)) FROM test_table", 1),
    ("Aggregations", "Sum price", "SELECT SUM(CAST(decimal_col AS DOUBLE)) FROM test_table", 1),
    ("Aggregations", "Group by status", "SELECT bool_col, COUNT(*) FROM test_table GROUP BY bool_col", 2),
    ("Aggregations", "Group by year", "SELECT EXTRACT(YEAR FROM date_col) as \"year\", COUNT(*) FROM test_table GROUP BY EXTRACT(YEAR FROM date_col) ORDER BY \"year\"", 3),
    ("Aggregations", "Avg price by model", "SELECT varchar_col, AVG(CAST(decimal_col AS DOUBLE)) FROM test_table GROUP BY varchar_col", 3),
    ("Complex Filtering", "Multiple conditions", "SELECT * FROM test_table WHERE EXTRACT(YEAR FROM date_col) >= 2020 AND bool_col = true AND CAST(decimal_col AS DOUBLE) < 80000", 1),
    ("Complex Filtering", "Pattern matching", "SELECT * FROM test_table WHERE varchar_col LIKE 'varchar%'", 1),
    ("Complex Filtering", "Case statement", "SELECT int_col, varchar_col, CASE WHEN EXTRACT(YEAR FROM date_col) >= 2020 THEN CAST('Recent' AS VARCHAR) ELSE CAST('Old' AS VARCHAR) END as age_category FROM test_table", 3),
    ("Sorting & Limiting", "Order by ID desc", "SELECT * FROM test_table ORDER BY int_col DESC LIMIT 50", 3),
    ("Sorting & Limiting", "Order by price asc", "SELECT * FROM test_table ORDER BY CAST(decimal_col AS DOUBLE) ASC LIMIT 20", 3),
    ("Sorting & Limiting", "Multiple sort", "SELECT * FROM test_table ORDER BY EXTRACT(YEAR FROM date_col) DESC, varchar_col ASC LIMIT 100", 3),
    ("String Functions", "Case conversion", "SELECT UPPER(varchar_col), LOWER(varchar_col) FROM test_table LIMIT 10", 3),
    ("Date/Time Functions", "Age calculation", "SELECT *, (2024 - EXTRACT(YEAR FROM date_col)) as age FROM test_table LIMIT 10", 3),
    ("Window Functions", "Row numbering", "SELECT *, ROW_NUMBER() OVER (ORDER BY int_col) as row_num FROM test_table LIMIT 10", 3),
    ("Window Functions", "Price ranking", "SELECT *, RANK() OVER (ORDER BY CAST(decimal_col AS DOUBLE) DESC) as price_rank FROM test_table LIMIT 20", 3),
    ("Window Functions", "Partition window", "SELECT *, COUNT(*) OVER (PARTITION BY bool_col) as status_count FROM test_table", 3),
    ("Performance Testing", "Large result set", "SELECT * FROM test_table LIMIT 10000", 3),
    ("Performance Testing", "Complex aggregation", "SELECT varchar_col, bool_col, COUNT(*), AVG(CAST(decimal_col AS DOUBLE)), MIN(CAST(int_col AS INTEGER)), MAX(EXTRACT(YEAR FROM date_col)) FROM test_table GROUP BY varchar_col, bool_col ORDER BY COUNT(*) DESC", 3),
    ("Data Quality Checks", "Duplicate check", "SELECT int_col, COUNT(*) FROM test_table GROUP BY int_col HAVING COUNT(*) > 1", 0),
    ("Data Quality Checks", "Completeness check", "SELECT COUNT(*) as total_rows, COUNT(int_col) as int_count, COUNT(varchar_col) as varchar_count, COUNT(decimal_col) as price_count FROM test_table", 1),
    ("Data Quality Checks", "Value ranges", "SELECT MIN(EXTRACT(YEAR FROM date_col)) as min_year, MAX(EXTRACT(YEAR FROM date_col)) as max_year, MIN(CAST(decimal_col AS DOUBLE)) as min_price, MAX(CAST(decimal_col AS DOUBLE)) as max_price FROM test_table", 1),
    ("Boolean Column Tests", "boolean column with values", "SELECT * FROM test_table WHERE bool_col = true LIMIT 10", 2),
    ("Boolean Column Tests", "Standalone boolean column", "SELECT * FROM test_table WHERE bool_col LIMIT 10", 2),
    ("Boolean Column Tests", "Standalone boolean column with NOT", "SELECT * FROM test_table WHERE NOT bool_col LIMIT 10", 1),
    ("Boolean Column Tests", "Explicit boolean comparison true", "SELECT * FROM test_table WHERE bool_col = true LIMIT 10", 2),
    ("Boolean Column Tests", "Explicit boolean comparison false", "SELECT * FROM test_table WHERE bool_col = false LIMIT 10", 1),
    ("Boolean Column Tests", "Complex condition with NOT boolean", "SELECT * FROM test_table WHERE int_col > 1000 AND NOT bool_col LIMIT 10", 1),
    ("Boolean Column Tests", "Complex condition with boolean", "SELECT * FROM test_table WHERE int_col > 1000 AND bool_col LIMIT 10", 1),
    ("Boolean Column Tests", "OR condition with boolean", "SELECT * FROM test_table WHERE bool_col OR int_col = 1500 LIMIT 10", 2),
    ("Boolean Column Tests", "Boolean in complex expression", "SELECT * FROM test_table WHERE (int_col > 1000 AND bool_col) OR (int_col < 500 AND NOT bool_col) LIMIT 10", 1),
];

struct TestDialect;

impl SplitDialect for TestDialect {
    fn table_clause(
        &self,
        builder: &SplitQueryBuilder<Self>,
        table: &TableName,
        _split: &Split,
    ) -> String {
        format!(" FROM {}", builder.quote(table.table_name()))
    }
}

fn builder() -> SplitQueryBuilder<TestDialect> {
    SplitQueryBuilder::new(QUOTE_CHAR, TestDialect)
}

fn plan_constraints(plan: &str) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        Some(QueryPlan::new(None, plan)),
    )
}

fn split() -> Split {
    Split::new(None, None, HashMap::new())
}

fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => panic!("dangling escape in {field}"),
        }
    }
    out
}

fn probe_lines() -> Vec<Vec<String>> {
    JAVA_PROBE
        .lines()
        .map(|line| line.split('\t').map(unescape).collect())
        .collect()
}

fn java_plan(sql: &str) -> String {
    probe_lines()
        .into_iter()
        .find(|f| f[0] == "plan" && f[1] == sql)
        .map_or_else(
            || panic!("the Java probe has no plan for {sql}"),
            |f| f[2].clone(),
        )
}

fn java_generate_error(sql: &str) -> String {
    probe_lines()
        .into_iter()
        .find(|f| f[0] == "generate_error" && f[1] == sql)
        .map_or_else(
            || panic!("the Java probe has no plan generation error for {sql}"),
            |f| f[2].clone(),
        )
}

fn java_run(header: &[&str]) -> Vec<Vec<String>> {
    let lines = probe_lines();
    let start = lines
        .iter()
        .position(|f| f[0] == "test" && f[1..] == *header)
        .unwrap_or_else(|| panic!("the Java probe did not run {header:?}"));
    let events: Vec<Vec<String>> = lines[start + 1..]
        .iter()
        .take_while(|f| f[0] != "end")
        .cloned()
        .collect();
    assert!(
        events.contains(&vec!["result".to_string(), "passed".to_string()]),
        "the Java test failed in the probe: {events:?}"
    );
    events
}

fn java_bind(method: &str, value: &str) -> SqlValue {
    match method {
        "setInt" => SqlValue::Integer(value.parse().unwrap()),
        "setLong" => SqlValue::BigInt(value.parse().unwrap()),
        "setShort" => SqlValue::SmallInt(value.parse().unwrap()),
        "setByte" => SqlValue::TinyInt(value.parse().unwrap()),
        "setDouble" => SqlValue::Double(value.parse().unwrap()),
        "setFloat" => SqlValue::Real(value.parse().unwrap()),
        "setBoolean" => SqlValue::Boolean(value.parse().unwrap()),
        "setString" => SqlValue::String(value.to_string()),
        "setBigDecimal" => SqlValue::Decimal(value.parse().unwrap()),
        other => panic!("the Java probe recorded an unmapped setter {other}"),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct JavaStatement {
    sql: String,
    parameter_count: Option<usize>,
    parameters: Vec<SqlValue>,
    rows: usize,
}

fn java_statement(events: &[Vec<String>]) -> JavaStatement {
    let sql = events
        .iter()
        .find(|f| f[0] == "prepare")
        .map(|f| f[1].clone())
        .expect("the Java test prepared no statement");
    let parameter_count = events
        .iter()
        .find(|f| f[0] == "parameter_count")
        .map(|f| f[1].parse().unwrap());
    let mut binds: Vec<(usize, SqlValue)> = events
        .iter()
        .filter(|f| f[0] == "bind")
        .map(|f| (f[2].parse().unwrap(), java_bind(&f[1], &f[4])))
        .collect();
    binds.sort_by_key(|(index, _)| *index);
    let rows = events
        .iter()
        .filter(|f| f[0] == "next" && f[1] == "true")
        .count();
    JavaStatement {
        sql,
        parameter_count,
        parameters: binds.into_iter().map(|(_, value)| value).collect(),
        rows,
    }
}

fn decimal_text(values: &[SqlValue]) -> Vec<Option<String>> {
    values
        .iter()
        .map(|value| match value {
            SqlValue::Decimal(decimal) => Some(big_decimal_to_string(decimal)),
            _ => None,
        })
        .collect()
}

fn h2_connection(java: &JavaStatement) -> FakeConnection {
    let connection = FakeConnection::new();
    let parameter_count = java.parameter_count;
    connection.on_prepare(move |sql| {
        let statement = PreparedStatement::new(sql);
        Ok(match parameter_count {
            Some(count) => statement.with_parameter_count(count),
            None => statement,
        })
    });
    let java = java.clone();
    connection.on_query(move |statement| {
        if statement.sql() != java.sql || statement.parameters() != java.parameters {
            return Err(SqlError::new(format!(
                "H2 never ran {} with {:?}",
                statement.sql(),
                statement.parameters()
            )));
        }
        let data: Vec<Vec<&str>> = (0..java.rows).map(|_| vec!["1"]).collect();
        let refs: Vec<&[&str]> = data.iter().map(Vec::as_slice).collect();
        Ok(ResultSetData::of_strings(&["c"], &refs))
    });
    connection
}

async fn prepare_like_java(
    sql_query: &str,
    java: &JavaStatement,
) -> (FakeConnection, PreparedStatement) {
    let connection = h2_connection(java);
    let statement = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints(&java_plan(sql_query)),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap_or_else(|e| panic!("{sql_query} failed: {e:?}"));
    assert_eq!(statement.sql(), java.sql, "{sql_query}");
    assert_eq!(statement.parameters(), java.parameters, "{sql_query}");
    assert_eq!(
        decimal_text(statement.parameters()),
        decimal_text(&java.parameters),
        "{sql_query}"
    );
    (connection, statement)
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql() {
    let java = java_statement(&java_run(&["testRealPreparedStatementWithCalciteSql"]));
    let (connection, statement) = prepare_like_java("SELECT * FROM test_table", &java).await;
    let mut result_set = connection.execute_query(&statement).await.unwrap();
    while result_set.next_row().await.unwrap().is_some() {}
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql_parameterized() {
    for (category, test_case_name, sql_query, expected_row_count) in CALCITE_SQL_TEST_CASES {
        let count = expected_row_count.to_string();
        let java = java_statement(&java_run(&[
            "testRealPreparedStatementWithCalciteSqlParameterized",
            category,
            test_case_name,
            sql_query,
            &count,
        ]));
        let (connection, statement) = prepare_like_java(sql_query, &java).await;
        let mut result_set = connection.execute_query(&statement).await.unwrap();
        let mut row_count = 0;
        while result_set.next_row().await.unwrap().is_some() {
            row_count += 1;
        }
        assert_eq!(
            expected_row_count, row_count,
            "{category} - {test_case_name}: expected {expected_row_count} rows but got {row_count}"
        );
    }
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql_error_cases() {
    let events = java_run(&["testRealPreparedStatementWithCalciteSqlErrorCases"]);
    assert!(events.iter().all(|f| f[0] != "prepare"), "{events:?}");
    let error = java_generate_error("SELECT * FORM test_table");
    assert!(
        error.starts_with("org.apache.calcite.sql.parser.SqlParseException: "),
        "{error}"
    );

    let connection = FakeConnection::new();
    let error = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints("SELECT * FORM test_table"),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Failed to prepare statement with Calcite"
    );
    assert!(connection.prepared_sql().is_empty());
}

#[tokio::test]
async fn test_real_prepared_statement_with_calcite_sql_invalid_column() {
    let events = java_run(&["testRealPreparedStatementWithCalciteSqlInvalidColumn"]);
    assert!(events.iter().all(|f| f[0] != "prepare"), "{events:?}");
    let message = java_generate_error("SELECT invalid_column FROM test_table");
    let lower = message.to_lowercase();
    assert!(
        lower.contains("column") || lower.contains("not found") || lower.contains("invalid"),
        "Error message should indicate column problem: {message}"
    );

    let connection = FakeConnection::new();
    let error = builder()
        .prepare_statement_with_calcite_sql(
            &connection,
            &plan_constraints("SELECT invalid_column FROM test_table"),
            &SqlDialect::ansi(),
            &split(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Failed to prepare statement with Calcite"
    );
    assert!(connection.prepared_sql().is_empty());
}
