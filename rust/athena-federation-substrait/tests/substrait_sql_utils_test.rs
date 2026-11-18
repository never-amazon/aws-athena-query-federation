use athena_federation_substrait::parameterize;
use athena_federation_substrait::plan;
use athena_federation_substrait::SqlDialect;
use athena_federation_substrait::SubstraitValue;
use base64::Engine;
use std::error::Error;

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
    plan::to_sql_node(&encoded_plan(query), &d)
        .expect("sql node")
        .to_sql_string(&d)
}

#[test]
fn test_get_table_schema_from_substrait_plan_all_arrow_types() {
    let schema = plan::table_schema(
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
    let schema =
        plan::table_schema(&encoded_plan("SELECT * FROM test_table"), &dialect()).expect("schema");
    assert!(!schema.field_list().is_empty());
    let names = schema.field_names();
    assert!(names.contains(&"varchar_col".to_string()));
    assert!(names.contains(&"int_col".to_string()));
}

#[test]
fn test_get_table_schema_from_substrait_plan_with_where_clause() {
    let schema = plan::table_schema(
        &encoded_plan("SELECT * FROM test_table WHERE varchar_col = 'test'"),
        &dialect(),
    )
    .expect("schema");
    assert!(schema.field_count() > 0);
}

#[test]
fn test_get_table_schema_from_substrait_plan_complex_query() {
    let schema = plan::table_schema(
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
    let err = plan::to_sql_node("This is not valid base64!!!@#$", &dialect())
        .expect_err("invalid base64");
    assert!(err.to_string().contains("Failed to parse Substrait plan"));
}

#[test]
fn test_get_sql_node_from_substrait_plan_empty_string() {
    let err = plan::to_sql_node("", &dialect()).expect_err("empty");
    assert!(err.to_string().contains("Failed to parse Substrait plan"));
}

#[test]
fn test_get_sql_node_from_substrait_plan_invalid_protobuf() {
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"invalid protobuf data");
    let err = plan::to_sql_node(&encoded, &dialect()).expect_err("invalid proto");
    assert!(
        err.to_string().contains("Failed to parse Substrait plan")
            || err
                .to_string()
                .contains("Failed to convert from Substrait plan to RelNode")
    );
}

#[test]
fn test_get_table_schema_from_substrait_plan_invalid_base64() {
    let err =
        plan::table_schema("Not valid base64 content!!!", &dialect()).expect_err("invalid base64");
    assert!(err
        .to_string()
        .contains("Failed to extract table schema from Substrait plan"));
}

#[test]
fn test_get_table_schema_from_substrait_plan_empty_plan() {
    let err = plan::table_schema("", &dialect()).expect_err("empty");
    assert!(err
        .to_string()
        .contains("Failed to extract table schema from Substrait plan"));
}

#[test]
fn test_sql_conversion_is_not_null() {
    assert!(!sql("SELECT * FROM test_table WHERE varchar_col IS NOT NULL").is_empty());
}

#[test]
fn test_sql_conversion_keeps_fractions_of_timestamps_before_the_epoch() {
    let cases = [
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKbHQ6YW55X2FueRrNCRLKCQqrBzqoBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEvYEEvMECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaRxpFCAEaBAoCEAIiGxoZWhcKB4oCBAgDGAISChIICgQSAggOIgAYAiIeGhxaGgoHigIECAMYAhINCgtwmPj/////////ARgCGggSBgoCEgAiABoKEggKBBICCAEiABoKEggKBBICCAIiABoKEggKBBICCAMiABoKEggKBBICCAQiABoKEggKBBICCAUiABoKEggKBBICCAYiABoKEggKBBICCAciABoKEggKBBICCAgiABoKEggKBBICCAkiABoKEggKBBICCAoiABoKEggKBBICCAsiABoKEggKBBICCAwiABoKEggKBBICCA0iABoKEggKBBICCA4iABoKEggKBBICCA8iABoKEggKBBICCBAiABoKEggKBBICCBEiABoKEggKBBICCBIiABoKEggKBBICCBMiABoKEggKBBICCBQiABoKEggKBBICCBUiABoKEggKBBICCBYiABIIYm9vbF9jb2wSC3RpbnlpbnRfY29sEgxzbWFsbGludF9jb2wSB2ludF9jb2wSCmJpZ2ludF9jb2wSCWZsb2F0X2NvbBIKZG91YmxlX2NvbBIIcmVhbF9jb2wSC3ZhcmNoYXJfY29sEghjaGFyX2NvbBIKYmluYXJ5X2NvbBINdmFyYmluYXJ5X2NvbBIIZGF0ZV9jb2wSCHRpbWVfY29sEg10aW1lc3RhbXBfY29sEgtkZWNpbWFsX2NvbBIMZGVjaW1hbF9jb2wyEgxkZWNpbWFsX2NvbDMSCWFycmF5X2NvbBINaW50X2FycmF5X2NvbBIHbWFwX2NvbBIQbWFwX3dpdGhfZGVjaW1hbBIMbmVzdGVkX2FycmF5MgsQSioHaXN0aG11cw==",
            "TIMESTAMP '1969-12-31 23:59:59.999'",
        ),
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKbHQ6YW55X2FueRrNCRLKCQqrBzqoBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEvYEEvMECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaRxpFCAEaBAoCEAIiGxoZWhcKB4oCBAgDGAISChIICgQSAggOIgAYAiIeGhxaGgoHigIECAMYAhINCgtw6Keft76G0Mn/ARgCGggSBgoCEgAiABoKEggKBBICCAEiABoKEggKBBICCAIiABoKEggKBBICCAMiABoKEggKBBICCAQiABoKEggKBBICCAUiABoKEggKBBICCAYiABoKEggKBBICCAciABoKEggKBBICCAgiABoKEggKBBICCAkiABoKEggKBBICCAoiABoKEggKBBICCAsiABoKEggKBBICCAwiABoKEggKBBICCA0iABoKEggKBBICCA4iABoKEggKBBICCA8iABoKEggKBBICCBAiABoKEggKBBICCBEiABoKEggKBBICCBIiABoKEggKBBICCBMiABoKEggKBBICCBQiABoKEggKBBICCBUiABoKEggKBBICCBYiABIIYm9vbF9jb2wSC3RpbnlpbnRfY29sEgxzbWFsbGludF9jb2wSB2ludF9jb2wSCmJpZ2ludF9jb2wSCWZsb2F0X2NvbBIKZG91YmxlX2NvbBIIcmVhbF9jb2wSC3ZhcmNoYXJfY29sEghjaGFyX2NvbBIKYmluYXJ5X2NvbBINdmFyYmluYXJ5X2NvbBIIZGF0ZV9jb2wSCHRpbWVfY29sEg10aW1lc3RhbXBfY29sEgtkZWNpbWFsX2NvbBIMZGVjaW1hbF9jb2wyEgxkZWNpbWFsX2NvbDMSCWFycmF5X2NvbBINaW50X2FycmF5X2NvbBIHbWFwX2NvbBIQbWFwX3dpdGhfZGVjaW1hbBIMbmVzdGVkX2FycmF5MgsQSioHaXN0aG11cw==",
            "TIMESTAMP '1000-01-01 12:00:00.001'",
        ),
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKbHQ6YW55X2FueRrNCRLKCQqrBzqoBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEvYEEvMECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaRxpFCAEaBAoCEAIiGxoZWhcKB4oCBAgBGAISChIICgQSAggOIgAYAiIeGhxaGgoHigIECAEYAhINCgtwoNu1mKPmu///ARgCGggSBgoCEgAiABoKEggKBBICCAEiABoKEggKBBICCAIiABoKEggKBBICCAMiABoKEggKBBICCAQiABoKEggKBBICCAUiABoKEggKBBICCAYiABoKEggKBBICCAciABoKEggKBBICCAgiABoKEggKBBICCAkiABoKEggKBBICCAoiABoKEggKBBICCAsiABoKEggKBBICCAwiABoKEggKBBICCA0iABoKEggKBBICCA4iABoKEggKBBICCA8iABoKEggKBBICCBAiABoKEggKBBICCBEiABoKEggKBBICCBIiABoKEggKBBICCBMiABoKEggKBBICCBQiABoKEggKBBICCBUiABoKEggKBBICCBYiABIIYm9vbF9jb2wSC3RpbnlpbnRfY29sEgxzbWFsbGludF9jb2wSB2ludF9jb2wSCmJpZ2ludF9jb2wSCWZsb2F0X2NvbBIKZG91YmxlX2NvbBIIcmVhbF9jb2wSC3ZhcmNoYXJfY29sEghjaGFyX2NvbBIKYmluYXJ5X2NvbBINdmFyYmluYXJ5X2NvbBIIZGF0ZV9jb2wSCHRpbWVfY29sEg10aW1lc3RhbXBfY29sEgtkZWNpbWFsX2NvbBIMZGVjaW1hbF9jb2wyEgxkZWNpbWFsX2NvbDMSCWFycmF5X2NvbBINaW50X2FycmF5X2NvbBIHbWFwX2NvbBIQbWFwX3dpdGhfZGVjaW1hbBIMbmVzdGVkX2FycmF5MgsQSioHaXN0aG11cw==",
            "TIMESTAMP '1960-06-30 08:15:30.5'",
        ),
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKbHQ6YW55X2FueRrLCRLICQqpBzqmBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEvQEEvEECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaRRpDCAEaBAoCEAIiGxoZWhcKB4oCBAgDGAISChIICgQSAggOIgAYAiIcGhpaGAoHigIECAMYAhILCglw+KCjxsjg6QIYAhoIEgYKAhIAIgAaChIICgQSAggBIgAaChIICgQSAggCIgAaChIICgQSAggDIgAaChIICgQSAggEIgAaChIICgQSAggFIgAaChIICgQSAggGIgAaChIICgQSAggHIgAaChIICgQSAggIIgAaChIICgQSAggJIgAaChIICgQSAggKIgAaChIICgQSAggLIgAaChIICgQSAggMIgAaChIICgQSAggNIgAaChIICgQSAggOIgAaChIICgQSAggPIgAaChIICgQSAggQIgAaChIICgQSAggRIgAaChIICgQSAggSIgAaChIICgQSAggTIgAaChIICgQSAggUIgAaChIICgQSAggVIgAaChIICgQSAggWIgASCGJvb2xfY29sEgt0aW55aW50X2NvbBIMc21hbGxpbnRfY29sEgdpbnRfY29sEgpiaWdpbnRfY29sEglmbG9hdF9jb2wSCmRvdWJsZV9jb2wSCHJlYWxfY29sEgt2YXJjaGFyX2NvbBIIY2hhcl9jb2wSCmJpbmFyeV9jb2wSDXZhcmJpbmFyeV9jb2wSCGRhdGVfY29sEgh0aW1lX2NvbBINdGltZXN0YW1wX2NvbBILZGVjaW1hbF9jb2wSDGRlY2ltYWxfY29sMhIMZGVjaW1hbF9jb2wzEglhcnJheV9jb2wSDWludF9hcnJheV9jb2wSB21hcF9jb2wSEG1hcF93aXRoX2RlY2ltYWwSDG5lc3RlZF9hcnJheTILEEoqB2lzdGhtdXM=",
            "TIMESTAMP '2020-06-01 12:00:00.123'",
        ),
    ];
    let d = dialect();
    for (plan, literal) in cases {
        let sql = plan::to_sql_node(plan, &d)
            .expect("sql node")
            .to_sql_string(&d);
        assert!(sql.contains(literal), "{sql}");
    }
}

#[test]
fn test_sql_conversion_rejects_date_time_literals_outside_years_one_to9999() {
    let cases = [
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRqwCRKtCQqOBzqLBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEtkEEtYECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaKhooCAEaBAoCEAIiDBoKEggKBBICCAwiACIQGg4KDIAByJDT////////ARoIEgYKAhIAIgAaChIICgQSAggBIgAaChIICgQSAggCIgAaChIICgQSAggDIgAaChIICgQSAggEIgAaChIICgQSAggFIgAaChIICgQSAggGIgAaChIICgQSAggHIgAaChIICgQSAggIIgAaChIICgQSAggJIgAaChIICgQSAggKIgAaChIICgQSAggLIgAaChIICgQSAggMIgAaChIICgQSAggNIgAaChIICgQSAggOIgAaChIICgQSAggPIgAaChIICgQSAggQIgAaChIICgQSAggRIgAaChIICgQSAggSIgAaChIICgQSAggTIgAaChIICgQSAggUIgAaChIICgQSAggVIgAaChIICgQSAggWIgASCGJvb2xfY29sEgt0aW55aW50X2NvbBIMc21hbGxpbnRfY29sEgdpbnRfY29sEgpiaWdpbnRfY29sEglmbG9hdF9jb2wSCmRvdWJsZV9jb2wSCHJlYWxfY29sEgt2YXJjaGFyX2NvbBIIY2hhcl9jb2wSCmJpbmFyeV9jb2wSDXZhcmJpbmFyeV9jb2wSCGRhdGVfY29sEgh0aW1lX2NvbBINdGltZXN0YW1wX2NvbBILZGVjaW1hbF9jb2wSDGRlY2ltYWxfY29sMhIMZGVjaW1hbF9jb2wzEglhcnJheV9jb2wSDWludF9hcnJheV9jb2wSB21hcF9jb2wSEG1hcF93aXRoX2RlY2ltYWwSDG5lc3RlZF9hcnJheTILEEoqB2lzdGhtdXM=",
            "Date literal -0043-03-15 is outside the years 0001 to 9999",
        ),
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRqwCRKtCQqOBzqLBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEtkEEtYECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaKhooCAEaBAoCEAIiDBoKEggKBBICCAwiACIQGg4KDIABk4vU////////ARoIEgYKAhIAIgAaChIICgQSAggBIgAaChIICgQSAggCIgAaChIICgQSAggDIgAaChIICgQSAggEIgAaChIICgQSAggFIgAaChIICgQSAggGIgAaChIICgQSAggHIgAaChIICgQSAggIIgAaChIICgQSAggJIgAaChIICgQSAggKIgAaChIICgQSAggLIgAaChIICgQSAggMIgAaChIICgQSAggNIgAaChIICgQSAggOIgAaChIICgQSAggPIgAaChIICgQSAggQIgAaChIICgQSAggRIgAaChIICgQSAggSIgAaChIICgQSAggTIgAaChIICgQSAggUIgAaChIICgQSAggVIgAaChIICgQSAggWIgASCGJvb2xfY29sEgt0aW55aW50X2NvbBIMc21hbGxpbnRfY29sEgdpbnRfY29sEgpiaWdpbnRfY29sEglmbG9hdF9jb2wSCmRvdWJsZV9jb2wSCHJlYWxfY29sEgt2YXJjaGFyX2NvbBIIY2hhcl9jb2wSCmJpbmFyeV9jb2wSDXZhcmJpbmFyeV9jb2wSCGRhdGVfY29sEgh0aW1lX2NvbBINdGltZXN0YW1wX2NvbBILZGVjaW1hbF9jb2wSDGRlY2ltYWxfY29sMhIMZGVjaW1hbF9jb2wzEglhcnJheV9jb2wSDWludF9hcnJheV9jb2wSB21hcF9jb2wSEG1hcF93aXRoX2RlY2ltYWwSDG5lc3RlZF9hcnJheTILEEoqB2lzdGhtdXM=",
            "Date literal 0000-02-29 is outside the years 0001 to 9999",
        ),
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRqqCRKnCQqIBzqFBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEtMEEtAECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaJBoiCAEaBAoCEAIiDBoKEggKBBICCAwiACIKGggKBoABoYGzARoIEgYKAhIAIgAaChIICgQSAggBIgAaChIICgQSAggCIgAaChIICgQSAggDIgAaChIICgQSAggEIgAaChIICgQSAggFIgAaChIICgQSAggGIgAaChIICgQSAggHIgAaChIICgQSAggIIgAaChIICgQSAggJIgAaChIICgQSAggKIgAaChIICgQSAggLIgAaChIICgQSAggMIgAaChIICgQSAggNIgAaChIICgQSAggOIgAaChIICgQSAggPIgAaChIICgQSAggQIgAaChIICgQSAggRIgAaChIICgQSAggSIgAaChIICgQSAggTIgAaChIICgQSAggUIgAaChIICgQSAggVIgAaChIICgQSAggWIgASCGJvb2xfY29sEgt0aW55aW50X2NvbBIMc21hbGxpbnRfY29sEgdpbnRfY29sEgpiaWdpbnRfY29sEglmbG9hdF9jb2wSCmRvdWJsZV9jb2wSCHJlYWxfY29sEgt2YXJjaGFyX2NvbBIIY2hhcl9jb2wSCmJpbmFyeV9jb2wSDXZhcmJpbmFyeV9jb2wSCGRhdGVfY29sEgh0aW1lX2NvbBINdGltZXN0YW1wX2NvbBILZGVjaW1hbF9jb2wSDGRlY2ltYWxfY29sMhIMZGVjaW1hbF9jb2wzEglhcnJheV9jb2wSDWludF9hcnJheV9jb2wSB21hcF9jb2wSEG1hcF93aXRoX2RlY2ltYWwSDG5lc3RlZF9hcnJheTILEEoqB2lzdGhtdXM=",
            "Date literal +10000-01-01 is outside the years 0001 to 9999",
        ),
        (
            "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRq8CRK5CQqaBzqXBwobEhkKFxcYGRobHB0eHyAhIiMkJSYnKCkqKywtEuUEEuIECgIKABKjBAqgBAoCCgAS/gMKCGJvb2xfY29sCgt0aW55aW50X2NvbAoMc21hbGxpbnRfY29sCgdpbnRfY29sCgpiaWdpbnRfY29sCglmbG9hdF9jb2wKCmRvdWJsZV9jb2wKCHJlYWxfY29sCgt2YXJjaGFyX2NvbAoIY2hhcl9jb2wKCmJpbmFyeV9jb2wKDXZhcmJpbmFyeV9jb2wKCGRhdGVfY29sCgh0aW1lX2NvbAoNdGltZXN0YW1wX2NvbAoLZGVjaW1hbF9jb2wKDGRlY2ltYWxfY29sMgoMZGVjaW1hbF9jb2wzCglhcnJheV9jb2wKDWludF9hcnJheV9jb2wKB21hcF9jb2wKEG1hcF93aXRoX2RlY2ltYWwKDG5lc3RlZF9hcnJheRLfAQoECgIQAgoEEgIQAgoEGgIQAgoEKgIQAgoEOgIQAgoEWgIQAgoEWgIQAgoEUgIQAgoEYgIQAgoHqgEECAEYAgoHugEECAEYAgoEagIQAgoFggECEAIKBYoBAhACCgWKAgIYAgoJwgEGCAQQEyACCgnCAQYIAhAKIAIKCcIBBggKEBMgAgoL2gEICgRiAhACGAIKC9oBCAoEKgIQAhgCChHiAQ4KBGICEAISBCoCEAIgAgoW4gETCgRiAhACEgnCAQYIAhAKIAIgAgoS2gEPCgvaAQgKBCoCEAIYAhgCGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaNho0CAEaBAoCEAIiDBoKEggKBBICCA4iACIcGhpaGAoFigICGAISDQoLcIDg+rOj3ZWP/wEYAhoIEgYKAhIAIgAaChIICgQSAggBIgAaChIICgQSAggCIgAaChIICgQSAggDIgAaChIICgQSAggEIgAaChIICgQSAggFIgAaChIICgQSAggGIgAaChIICgQSAggHIgAaChIICgQSAggIIgAaChIICgQSAggJIgAaChIICgQSAggKIgAaChIICgQSAggLIgAaChIICgQSAggMIgAaChIICgQSAggNIgAaChIICgQSAggOIgAaChIICgQSAggPIgAaChIICgQSAggQIgAaChIICgQSAggRIgAaChIICgQSAggSIgAaChIICgQSAggTIgAaChIICgQSAggUIgAaChIICgQSAggVIgAaChIICgQSAggWIgASCGJvb2xfY29sEgt0aW55aW50X2NvbBIMc21hbGxpbnRfY29sEgdpbnRfY29sEgpiaWdpbnRfY29sEglmbG9hdF9jb2wSCmRvdWJsZV9jb2wSCHJlYWxfY29sEgt2YXJjaGFyX2NvbBIIY2hhcl9jb2wSCmJpbmFyeV9jb2wSDXZhcmJpbmFyeV9jb2wSCGRhdGVfY29sEgh0aW1lX2NvbBINdGltZXN0YW1wX2NvbBILZGVjaW1hbF9jb2wSDGRlY2ltYWxfY29sMhIMZGVjaW1hbF9jb2wzEglhcnJheV9jb2wSDWludF9hcnJheV9jb2wSB21hcF9jb2wSEG1hcF93aXRoX2RlY2ltYWwSDG5lc3RlZF9hcnJheTILEEoqB2lzdGhtdXM=",
            "Timestamp literal on -0043-03-15 is outside the years 0001 to 9999",
        ),
    ];
    let d = dialect();
    for (plan, message) in cases {
        let error = plan::to_sql_node(plan, &d).unwrap_err();
        let cause = error.source().expect("cause");
        assert_eq!(
            "Failed to convert from Substrait plan to RelNode",
            cause.to_string()
        );
        assert_eq!(message, cause.source().expect("root").to_string());
    }
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
        plan::table_schema(&plan, &dialect()).expect("schema");
    }
}

#[test]
fn test_substrait_plan_generation() {
    let encoded = encoded_plan("SELECT * FROM test_table LIMIT 10");
    let plan = plan::decode(&encoded).expect("plan");
    assert!(!plan.relations.is_empty());
}

fn patched(query: &str, value: &str) -> String {
    format!("{query} [patched UNICODE1 => {value}]")
}

fn patched_sql(query: &str, value: &str) -> String {
    sql(&patched(query, value))
}

#[test]
fn test_sql_conversion_non_latin1_string_literals() {
    let d = dialect();
    let equality = encoded_plan(&patched(
        "SELECT * FROM test_table WHERE varchar_col = 'UNICODE1'",
        "日本語",
    ));
    let node = plan::to_sql_node(&equality, &d).expect("sql node");
    let s = node.to_sql_string(&d);
    assert!(s.ends_with("WHERE `varchar_col` = '日本語'"), "{s}");

    let schema = plan::table_schema(&equality, &d).expect("schema");
    let (parameterized, accumulator) = parameterize(node, &schema).expect("visit");
    let parameterized = parameterized.to_sql_string(&d);
    assert!(
        parameterized.ends_with("WHERE `varchar_col` = ?"),
        "{parameterized}"
    );
    assert_eq!(1, accumulator.len());
    assert_eq!(
        &SubstraitValue::String("日本語".to_string()),
        accumulator[0].value()
    );

    assert!(patched_sql(
        "SELECT * FROM test_table WHERE varchar_col IN ('UNICODE1', 'b')",
        "😀"
    )
    .ends_with("WHERE `varchar_col` IN ('b', '😀')"));
    assert!(patched_sql(
        "SELECT * FROM test_table WHERE varchar_col LIKE 'UNICODE1'",
        "%日本%"
    )
    .ends_with("WHERE `varchar_col` LIKE '%日本%'"));
    assert!(patched_sql(
        "SELECT * FROM test_table WHERE UPPER(varchar_col) = 'UNICODE1'",
        "日本語"
    )
    .ends_with("WHERE UPPER(`varchar_col`) = '日本語'"));
    assert!(patched_sql(
        "SELECT varchar_col, 'UNICODE1' AS k FROM test_table",
        "日'本😀"
    )
    .contains("'日''本😀' AS `$f24`"));
    assert!(patched_sql(
        "SELECT * FROM test_table WHERE UPPER(varchar_col) = 'UNICODE1'",
        "ab"
    )
    .ends_with("WHERE UPPER(`varchar_col`) = 'ab'"));
}

fn where_clause(sql: &str) -> &str {
    &sql[sql.find("WHERE").expect("where clause")..]
}

#[test]
fn test_sql_conversion_char_casts_count_code_points() {
    let cases = [
        (
            "SELECT * FROM test_table WHERE varchar_col = CAST('abc' AS VARCHAR(2)) [patched abc => a\u{1F600}b]",
            "WHERE `varchar_col` = 'a\u{1F600}'",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col = CAST('abc' AS VARCHAR(2)) [patched abc => \u{1F600}b]",
            "WHERE `varchar_col` = '\u{1F600}b'",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col = CAST('abc' AS CHAR(2)) [patched abc => a\u{1F600}b]",
            "WHERE `varchar_col` = 'a\u{1F600}'",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col = CAST('ab' AS CHAR(1)) [patched ab => \u{1F600}]",
            "WHERE `varchar_col` = '\u{1F600}'",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col = CAST('a' AS CHAR(3)) [patched a => \u{1F600}]",
            "WHERE `varchar_col` = '\u{1F600}  '",
        ),
        (
            "SELECT * FROM test_table WHERE varchar_col = CAST('abc' AS VARCHAR(2))",
            "WHERE `varchar_col` = 'ab'",
        ),
    ];
    for (query, expected) in cases {
        assert_eq!(expected, where_clause(&sql(query)), "{query}");
    }
}
