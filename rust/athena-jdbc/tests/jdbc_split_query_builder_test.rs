mod jdbc_support;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow::datatypes::{DataType, Field, FieldRef, Schema, TimeUnit};
use bigdecimal::BigDecimal;
use chrono::NaiveDate;
use indexmap::IndexMap;

use athena_federation_sdk::data::{ArrowType, BlockAllocatorImpl};
use athena_federation_sdk::domain::predicate::functions::FunctionName;
use athena_federation_sdk::domain::predicate::{
    Constraints, Direction, Marker, OrderByField, QueryPlan, Range, SortedRangeSet, ValueSet,
    DEFAULT_NO_LIMIT,
};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::Result;
use athena_federation_substrait::SqlDialect;
use athena_jdbc::manager::{
    FederationExpressionParser, JdbcSplitQueryBuilder, JdbcSplitQueryBuilderBase, TypeAndValue,
};
use athena_jdbc::sql::{PreparedStatement, SqlValue};

use jdbc_support::FakeConnection;

const TEST_CATALOG: &str = "catalog";
const TEST_SCHEMA: &str = "schema";
const TEST_TABLE: &str = "table";
const TEST_COL1: &str = "col1";
const TEST_COL2: &str = "col2";
const TEST_COL3: &str = "col3";
const TEST_PARTITION_COL: &str = "partition_col";
const TEST_PARTITION_VALUE: &str = "2024";
const QUOTE_CHAR: &str = "\"";

const SUCCESS_PLANS: [&str; 12] = [
    "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CAEaBAoCEAEiDBoKEggKBBICCAQiACILGgkKB2ICSFKQAwEiLxotGisIARoECgIQASIMGgoSCAoEEgIIBCIAIhMaEQoPYgpNYW5hZ2VtZW50kAMBIi4aLBoqCAEaBAoCEAEiDBoKEggKBBICCAQiACISGhAKDmIJTWFya2V0aW5nkAMBIioaKBomCAEaBAoCEAEiDBoKEggKBBICCAQiACIOGgwKCmIFU2FsZXOQAwEiCRoHCgUIAZADARoKEggKBBICCAgiABoKEggKBBICCAQiABoKEggKBBICCAEiABoKEggKBBICCAIiABoKEggKBBICCAMiAA==",
    "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRISGhAIAhACGgpndDphbnlfYW55GuYCEuMCCuACGt0CCgIKABLSAhLPAgoCCgAS2QEK1gEKAgoAEqQBCgJpZAoEbmFtZQoDYWdlCgZzYWxhcnkKCmRlcGFydG1lbnQKCWhpcmVfZGF0ZQoJaXNfYWN0aXZlCgVzY29yZQoEY2l0eQoHY291bnRyeQoHcGFydF9pZBJKCgQqAhABCgRiAhABCgQqAhABCgnCAQYIAhAKIAEKBGICEAEKBYIBAhABCgQKAhABCgRaAhABCgRiAhABCgRiAhABCgRiAhABGAI6KQoKZ2x1ZV90ZWFtMgobdGVzdF90YWJsZV9xdWVyeV9mZWRlcmF0aW9uGm0aaxoECgIQASJYGlYaVBoECgIQASIlGiMaIQgBGgQKAhABIgwaChIICgQSAggGIgAiCRoHCgUIAJADASIlGiMaIQgCGgQKAhABIgwaChIICgQSAggCIgAiCRoHCgUoHpADASIJGgcKBQgBkAMBGAAgZA==",
    "GucFEuQFCuEFGt4FCgIKABLTBSrQBQoCCgASuQUKtgUKAgoAEpUFChFjY19jYWxsX2NlbnRlcl9zawoRY2NfY2FsbF9jZW50ZXJfaWQKEWNjX3JlY19zdGFydF9kYXRlCg9jY19yZWNfZW5kX2RhdGUKEWNjX2Nsb3NlZF9kYXRlX3NrCg9jY19vcGVuX2RhdGVfc2sKB2NjX25hbWUKCGNjX2NsYXNzCgxjY19lbXBsb3llZXMKCGNjX3NxX2Z0CghjY19ob3VycwoKY2NfbWFuYWdlcgoJY2NfbWt0X2lkCgxjY19ta3RfY2xhc3MKC2NjX21rdF9kZXNjChFjY19tYXJrZXRfbWFuYWdlcgoLY2NfZGl2aXNpb24KEGNjX2RpdmlzaW9uX25hbWUKCmNjX2NvbXBhbnkKD2NjX2NvbXBhbnlfbmFtZQoQY2Nfc3RyZWV0X251bWJlcgoOY2Nfc3RyZWV0X25hbWUKDmNjX3N0cmVldF90eXBlCg9jY19zdWl0ZV9udW1iZXIKB2NjX2NpdHkKCWNjX2NvdW50eQoIY2Nfc3RhdGUKBmNjX3ppcAoKY2NfY291bnRyeQoNY2NfZ210X29mZnNldAoRY2NfdGF4X3BlcmNlbnRhZ2UKB3BhcnRfaWQSzgEKBCoCEAEKBGICEAEKBYIBAhABCgWCAQIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoJwgEGCAIQBSABCgnCAQYIAhAFIAEKBGICEAEYAjoYCgl3YXJlaG91c2UKC2NhbGxfY2VudGVyGg4KChIICgQSAggGIgAQAhgAIGQ=",
    "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRqzAhKwAgqtAhqqAgoCCgASnwISnAIKAgoAEtkBCtYBCgIKABKkAQoCaWQKBG5hbWUKA2FnZQoGc2FsYXJ5CgpkZXBhcnRtZW50CgloaXJlX2RhdGUKCWlzX2FjdGl2ZQoFc2NvcmUKBGNpdHkKB2NvdW50cnkKB3BhcnRfaWQSSgoEKgIQAQoEYgIQAQoEKgIQAQoJwgEGCAIQCiABCgRiAhABCgWCAQIQAQoECgIQAQoEWgIQAQoEYgIQAQoEYgIQAQoEYgIQARgCOikKCmdsdWVfdGVhbTIKG3Rlc3RfdGFibGVfcXVlcnlfZmVkZXJhdGlvbho6GjgaBAoCEAEiJRojGiEIARoECgIQASIMGgoSCAoEEgIIBiIAIgkaBwoFCAGQAwEiCRoHCgUIAZADARgAIAU=",
    "Go8BEowBCokBCoYBCgIKABJbCglkZXB0X25hbWUKDG1hbmFnZXJfbmFtZQoGYnVkZ2V0Cghsb2NhdGlvbgoHcGFydF9pZBIlCgRiAhABCgRiAhABCgnCAQYIAhAMIAEKBGICEAEKBGICEAEYAjojCgpnbHVlX3RlYW0yChV0ZXN0X3RhYmxlX2RlcGFydG1lbnQ=",
    "GucFEuQFCuEFGt4FCgIKABLTBSrQBQoCCgASuQUKtgUKAgoAEpUFChFjY19jYWxsX2NlbnRlcl9zawoRY2NfY2FsbF9jZW50ZXJfaWQKEWNjX3JlY19zdGFydF9kYXRlCg9jY19yZWNfZW5kX2RhdGUKEWNjX2Nsb3NlZF9kYXRlX3NrCg9jY19vcGVuX2RhdGVfc2sKB2NjX25hbWUKCGNjX2NsYXNzCgxjY19lbXBsb3llZXMKCGNjX3NxX2Z0CghjY19ob3VycwoKY2NfbWFuYWdlcgoJY2NfbWt0X2lkCgxjY19ta3RfY2xhc3MKC2NjX21rdF9kZXNjChFjY19tYXJrZXRfbWFuYWdlcgoLY2NfZGl2aXNpb24KEGNjX2RpdmlzaW9uX25hbWUKCmNjX2NvbXBhbnkKD2NjX2NvbXBhbnlfbmFtZQoQY2Nfc3RyZWV0X251bWJlcgoOY2Nfc3RyZWV0X25hbWUKDmNjX3N0cmVldF90eXBlCg9jY19zdWl0ZV9udW1iZXIKB2NjX2NpdHkKCWNjX2NvdW50eQoIY2Nfc3RhdGUKBmNjX3ppcAoKY2NfY291bnRyeQoNY2NfZ210X29mZnNldAoRY2NfdGF4X3BlcmNlbnRhZ2UKB3BhcnRfaWQSzgEKBCoCEAEKBGICEAEKBYIBAhABCgWCAQIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoJwgEGCAIQBSABCgnCAQYIAhAFIAEKBGICEAEYAjoYCgl3YXJlaG91c2UKC2NhbGxfY2VudGVyGg4KChIICgQSAggCIgAQAhgAIGQ=",
    "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSEhoQCAIQARoKZ3Q6YW55X2FueRIVGhMIAhACGg1lcXVhbDphbnlfYW55GuYCEuMCCuACGt0CCgIKABLSAhLPAgoCCgAS2QEK1gEKAgoAEqQBCgJpZAoEbmFtZQoDYWdlCgZzYWxhcnkKCmRlcGFydG1lbnQKCWhpcmVfZGF0ZQoJaXNfYWN0aXZlCgVzY29yZQoEY2l0eQoHY291bnRyeQoHcGFydF9pZBJKCgQqAhABCgRiAhABCgQqAhABCgnCAQYIAhAKIAEKBGICEAEKBYIBAhABCgQKAhABCgRaAhABCgRiAhABCgRiAhABCgRiAhABGAI6KQoKZ2x1ZV90ZWFtMgobdGVzdF90YWJsZV9xdWVyeV9mZWRlcmF0aW9uGm0aaxoECgIQASJYGlYaVBoECgIQASIlGiMaIQgBGgQKAhABIgwaChIICgQSAggCIgAiCRoHCgUoIJADASIlGiMaIQgCGgQKAhABIgwaChIICgQSAggGIgAiCRoHCgUIAZADASIJGgcKBQgBkAMBGAAgBQ==",
    "GucFEuQFCuEFGt4FCgIKABLTBSrQBQoCCgASuQUKtgUKAgoAEpUFChFjY19jYWxsX2NlbnRlcl9zawoRY2NfY2FsbF9jZW50ZXJfaWQKEWNjX3JlY19zdGFydF9kYXRlCg9jY19yZWNfZW5kX2RhdGUKEWNjX2Nsb3NlZF9kYXRlX3NrCg9jY19vcGVuX2RhdGVfc2sKB2NjX25hbWUKCGNjX2NsYXNzCgxjY19lbXBsb3llZXMKCGNjX3NxX2Z0CghjY19ob3VycwoKY2NfbWFuYWdlcgoJY2NfbWt0X2lkCgxjY19ta3RfY2xhc3MKC2NjX21rdF9kZXNjChFjY19tYXJrZXRfbWFuYWdlcgoLY2NfZGl2aXNpb24KEGNjX2RpdmlzaW9uX25hbWUKCmNjX2NvbXBhbnkKD2NjX2NvbXBhbnlfbmFtZQoQY2Nfc3RyZWV0X251bWJlcgoOY2Nfc3RyZWV0X25hbWUKDmNjX3N0cmVldF90eXBlCg9jY19zdWl0ZV9udW1iZXIKB2NjX2NpdHkKCWNjX2NvdW50eQoIY2Nfc3RhdGUKBmNjX3ppcAoKY2NfY291bnRyeQoNY2NfZ210X29mZnNldAoRY2NfdGF4X3BlcmNlbnRhZ2UKB3BhcnRfaWQSzgEKBCoCEAEKBGICEAEKBYIBAhABCgWCAQIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEKgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoEYgIQAQoJwgEGCAIQBSABCgnCAQYIAhAFIAEKBGICEAEYAjoYCgl3YXJlaG91c2UKC2NhbGxfY2VudGVyGg4KChIICgQSAggCIgAQAhgAIGQ=",
    "GpMCEpACCo0CGooCCgIKABL/ASr8AQoCCgAS5QEK4gEKAgoAEscBCgpvX29yZGVya2V5CglvX2N1c3RrZXkKDW9fb3JkZXJzdGF0dXMKDG9fdG90YWxwcmljZQoLb19vcmRlcmRhdGUKD29fb3JkZXJwcmlvcml0eQoHb19jbGVyawoOb19zaGlwcHJpb3JpdHkKCW9fY29tbWVudAoJcGFydGl0aW9uEkQKBDoCEAEKBDoCEAEKBGICEAEKCcIBBggCEAwgAQoFggECEAEKBGICEAEKBGICEAEKBDoCEAEKBGICEAEKBGICEAEYAjoSCgh0cGNoX3NmMQoGb3JkZXJzGg4KChIICgQSAggEIgAQAhgAIGQ=",
];

#[derive(Default)]
struct FakeExpressionParser {
    expressions: Mutex<Vec<String>>,
    calls: AtomicUsize,
}

impl FederationExpressionParser for FakeExpressionParser {
    fn map_function_to_data_source_syntax(
        &self,
        _function_name: &FunctionName,
        _arrow_type: &ArrowType,
        _arguments: &[String],
    ) -> Result<String> {
        Ok(String::new())
    }

    fn parse_complex_expressions(
        &self,
        _columns: &[FieldRef],
        _constraints: &Constraints,
        _accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<Vec<String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.expressions.lock().unwrap().clone())
    }
}

struct TestBuilder {
    base: JdbcSplitQueryBuilderBase,
}

impl JdbcSplitQueryBuilder for TestBuilder {
    fn query_builder_base(&self) -> &JdbcSplitQueryBuilderBase {
        &self.base
    }

    fn get_from_clause_with_split(
        &self,
        _catalog: &str,
        schema: &str,
        table: &str,
        _split: &Split,
    ) -> String {
        format!(" FROM \"{schema}\".\"{table}\"")
    }

    fn get_partition_where_clauses(&self, _split: &Split) -> Vec<String> {
        vec![format!(
            "\"{TEST_PARTITION_COL}\" = '{TEST_PARTITION_VALUE}'"
        )]
    }
}

struct Fixture {
    allocator: BlockAllocatorImpl,
    parser: Arc<FakeExpressionParser>,
    builder: TestBuilder,
    connection: FakeConnection,
    split: Split,
}

impl Fixture {
    fn new() -> Fixture {
        let parser = Arc::new(FakeExpressionParser::default());
        let builder = TestBuilder {
            base: JdbcSplitQueryBuilderBase::new_with_federation_expression_parser(
                QUOTE_CHAR,
                parser.clone(),
            ),
        };
        Fixture {
            allocator: BlockAllocatorImpl::new(),
            parser,
            builder,
            connection: FakeConnection::new(),
            split: Split::new(None, None, HashMap::new()),
        }
    }

    fn set_expressions(&self, expressions: &[&str]) {
        *self.parser.expressions.lock().unwrap() =
            expressions.iter().map(ToString::to_string).collect();
    }

    async fn build(&self, schema: &Schema, constraints: &Constraints) -> Result<PreparedStatement> {
        self.builder
            .build_sql(
                &self.connection,
                TEST_CATALOG,
                TEST_SCHEMA,
                TEST_TABLE,
                schema,
                constraints,
                &self.split,
            )
            .await
    }

    fn prepared(&self) -> String {
        let prepared = self.connection.prepared_sql();
        assert_eq!(prepared.len(), 1);
        prepared[0].clone()
    }

    fn parser_calls(&self) -> usize {
        self.parser.calls.load(Ordering::SeqCst)
    }
}

fn int_schema(names: &[&str]) -> Schema {
    Schema::new(
        names
            .iter()
            .map(|n| Field::new(*n, DataType::Int32, true))
            .collect::<Vec<_>>(),
    )
}

fn constraints_with(
    summary: IndexMap<String, ValueSet>,
    order_by: Vec<OrderByField>,
    limit: i64,
) -> Constraints {
    Constraints::new(summary, Vec::new(), order_by, limit, HashMap::new(), None)
}

fn create_empty_constraints() -> Constraints {
    constraints_with(IndexMap::new(), Vec::new(), DEFAULT_NO_LIMIT)
}

fn create_constraints_with_summary(summary: Vec<(&str, ValueSet)>) -> Constraints {
    constraints_with(
        summary
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
    )
}

fn create_constraints_with_order_by(order_by: Vec<OrderByField>) -> Constraints {
    constraints_with(IndexMap::new(), order_by, DEFAULT_NO_LIMIT)
}

fn create_constraints_with_order_by_and_limit(
    order_by: Vec<OrderByField>,
    limit: i64,
) -> Constraints {
    constraints_with(IndexMap::new(), order_by, limit)
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

fn sorted(set: SortedRangeSet) -> ValueSet {
    ValueSet::from(set)
}

fn single_range_value_set(range: Range) -> ValueSet {
    sorted(SortedRangeSet::of_ranges(range, Vec::new()).unwrap())
}

#[tokio::test]
async fn test_build_sql_int_type() {
    let f = Fixture::new();
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &create_empty_constraints())
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains(&format!("SELECT \"{TEST_COL1}\"")));
}

#[test]
fn test_extract_order_by_clause_asc_nulls_first() {
    let f = Fixture::new();
    let constraints = create_constraints_with_order_by(vec![OrderByField::new(
        TEST_COL1,
        Direction::ASC_NULLS_FIRST,
    )]);
    assert_eq!(
        f.builder.extract_order_by_clause(&constraints),
        format!("ORDER BY \"{TEST_COL1}\" ASC NULLS FIRST")
    );
}

#[tokio::test]
async fn test_to_predicate_with_single_value_set() {
    let f = Fixture::new();
    let value_set = sorted(
        SortedRangeSet::copy_of(
            &DataType::Int32,
            [Range::all(&f.allocator, &DataType::Int32)],
            false,
        )
        .unwrap(),
    );
    let constraints = create_constraints_with_summary(vec![(TEST_COL1, value_set)]);
    f.build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_unsupported_type_throws_exception() {
    let f = Fixture::new();
    let schema = Schema::new(vec![Field::new(
        "unsupported",
        DataType::Duration(TimeUnit::Millisecond),
        true,
    )]);
    let value_set =
        single_range_value_set(Range::equal(&f.allocator, &DataType::Int64, 123_i64).unwrap());
    let constraints = create_constraints_with_summary(vec![("unsupported", value_set)]);
    let error = f.build(&schema, &constraints).await.unwrap_err();
    assert!(error.as_connector_exception().is_some());
    assert!(error.to_string().contains("Can't handle type"));
}

#[tokio::test]
#[allow(
    clippy::approx_constant,
    reason = "the literal is the value the Java test binds"
)]
async fn test_supported_type() {
    let f = Fixture::new();
    let a = &f.allocator;
    let timestamp = NaiveDate::from_ymd_opt(2023, 1, 1)
        .unwrap()
        .and_hms_opt(10, 0, 0)
        .unwrap();
    let cases: Vec<(&str, DataType, Range)> = vec![
        (
            "col_bigint",
            DataType::Int64,
            Range::equal(a, &DataType::Int64, 123_i64).unwrap(),
        ),
        (
            "col_int",
            DataType::Int32,
            Range::equal(a, &DataType::Int32, 456_i32).unwrap(),
        ),
        (
            "col_smallint",
            DataType::Int16,
            Range::equal(a, &DataType::Int16, 7_i16).unwrap(),
        ),
        (
            "col_tinyint",
            DataType::Int8,
            Range::equal(a, &DataType::Int8, 2_i8).unwrap(),
        ),
        (
            "col_float8",
            DataType::Float64,
            Range::equal(a, &DataType::Float64, 3.14159_f64).unwrap(),
        ),
        (
            "col_float4",
            DataType::Float32,
            Range::equal(a, &DataType::Float32, 2.71_f32).unwrap(),
        ),
        (
            "col_bit",
            DataType::Boolean,
            Range::equal(a, &DataType::Boolean, true).unwrap(),
        ),
        (
            "col_dateday",
            DataType::Date32,
            Range::equal(a, &DataType::Date32, 19000_i32).unwrap(),
        ),
        (
            "col_datemilli",
            DataType::Date64,
            Range::equal(a, &DataType::Date64, timestamp).unwrap(),
        ),
        (
            "col_varchar",
            DataType::Utf8,
            Range::equal(a, &DataType::Utf8, "hello").unwrap(),
        ),
        (
            "col_varbinary",
            DataType::Binary,
            Range::equal(a, &DataType::Binary, b"abc".to_vec()).unwrap(),
        ),
    ];
    let schema = Schema::new(
        cases
            .iter()
            .map(|(n, t, _)| Field::new(*n, t.clone(), true))
            .collect::<Vec<_>>(),
    );
    let constraints = create_constraints_with_summary(
        cases
            .iter()
            .map(|(n, _, r)| (*n, single_range_value_set(r.clone())))
            .collect(),
    );
    let stmt = f.build(&schema, &constraints).await.unwrap();
    let sql = f.prepared();
    assert_eq!(stmt.sql(), sql);
    for (name, _, _) in &cases {
        assert!(sql.contains(&format!("\"{name}\"")));
    }
    assert!(sql.contains(&format!("FROM \"{TEST_SCHEMA}\".\"{TEST_TABLE}\"")));
    assert_eq!(stmt.parameters().len(), cases.len());
}

#[tokio::test]
async fn test_in_clause_with_multiple_values() {
    let f = Fixture::new();
    let a = &f.allocator;
    let ranges = [10, 20, 30].map(|v| Range::equal(a, &DataType::Int32, v).unwrap());
    let value_set = sorted(SortedRangeSet::copy_of(&DataType::Int32, ranges, false).unwrap());
    let constraints = create_constraints_with_summary(vec![(TEST_COL1, value_set)]);
    f.build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert!(f
        .prepared()
        .contains(&format!("\"{TEST_COL1}\" IN (?,?,?)")));
}

async fn assert_range_operator(range: Range, expected: &str) {
    let f = Fixture::new();
    let constraints =
        create_constraints_with_summary(vec![(TEST_COL1, single_range_value_set(range))]);
    f.build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert!(f
        .prepared()
        .contains(&format!("\"{TEST_COL1}\" {expected} ?")));
}

#[tokio::test]
async fn test_range_bound_above_for_low_marker() {
    let a = BlockAllocatorImpl::new();
    assert_range_operator(Range::greater_than(&a, &DataType::Int32, 100).unwrap(), ">").await;
}

#[tokio::test]
async fn test_range_bound_exactly_for_low_marker() {
    let a = BlockAllocatorImpl::new();
    assert_range_operator(
        Range::greater_than_or_equal(&a, &DataType::Int32, 100).unwrap(),
        ">=",
    )
    .await;
}

#[tokio::test]
async fn test_range_bound_exactly_for_high_marker() {
    let a = BlockAllocatorImpl::new();
    assert_range_operator(
        Range::less_than_or_equal(&a, &DataType::Int32, 200).unwrap(),
        "<=",
    )
    .await;
}

#[tokio::test]
async fn test_range_bound_below_for_high_marker() {
    let a = BlockAllocatorImpl::new();
    assert_range_operator(Range::less_than(&a, &DataType::Int32, 200).unwrap(), "<").await;
}

#[test]
fn test_quote_method_with_special_characters() {
    let f = Fixture::new();
    assert_eq!(
        f.builder.quote("column\"with\"quotes"),
        "\"column\"\"with\"\"quotes\""
    );
}

#[test]
fn test_append_limit_offset_with_split() {
    let f = Fixture::new();
    assert_eq!(f.builder.append_limit_offset(&f.split), "");
}

#[test]
fn test_append_limit_offset_with_constraints() {
    let f = Fixture::new();
    let constraints = constraints_with(IndexMap::new(), Vec::new(), 100);
    assert_eq!(
        f.builder
            .append_limit_offset_with_constraints(&f.split, &constraints),
        " LIMIT 100"
    );
}

#[test]
fn test_extract_order_by_clause_multiple_fields() {
    let f = Fixture::new();
    let constraints = create_constraints_with_order_by(vec![
        OrderByField::new(TEST_COL1, Direction::ASC_NULLS_LAST),
        OrderByField::new(TEST_COL2, Direction::DESC_NULLS_FIRST),
    ]);
    assert_eq!(
        f.builder.extract_order_by_clause(&constraints),
        format!("ORDER BY \"{TEST_COL1}\" ASC NULLS LAST, \"{TEST_COL2}\" DESC NULLS FIRST")
    );
}

#[test]
fn test_extract_order_by_clause_empty() {
    let f = Fixture::new();
    assert_eq!(
        f.builder
            .extract_order_by_clause(&create_constraints_with_order_by(Vec::new())),
        ""
    );
}

#[tokio::test]
async fn test_build_sql_with_empty_constraints() {
    let f = Fixture::new();
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &create_empty_constraints())
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("SELECT"));
    assert!(f.prepared().contains(TEST_COL1));
}

#[tokio::test]
async fn test_build_sql_with_order_by_and_limit() {
    let f = Fixture::new();
    let constraints = create_constraints_with_order_by_and_limit(
        vec![OrderByField::new(TEST_COL1, Direction::ASC_NULLS_FIRST)],
        50,
    );
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("ORDER BY"));
    assert!(f.prepared().contains("LIMIT"));
}

#[tokio::test]
async fn test_build_sql_with_complex_expressions_and_order_by() {
    let f = Fixture::new();
    f.set_expressions(&["(col1 + col2)", "CASE WHEN col3 > 0 THEN col4 ELSE 0 END"]);
    let constraints = create_constraints_with_order_by(vec![OrderByField::new(
        TEST_COL1,
        Direction::DESC_NULLS_LAST,
    )]);
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("ORDER BY"));
    assert_eq!(f.parser_calls(), 1);
}

#[tokio::test]
async fn test_build_sql_with_order_by_limit_and_complex_expressions() {
    let f = Fixture::new();
    f.set_expressions(&[
        "(col1 * col2 + col3)",
        "CASE WHEN col4 > 100 THEN col5 ELSE col6 END",
        "COALESCE(col7, col8, 'default')",
    ]);
    let constraints = create_constraints_with_order_by_and_limit(
        vec![
            OrderByField::new(TEST_COL1, Direction::ASC_NULLS_FIRST),
            OrderByField::new(TEST_COL2, Direction::DESC_NULLS_LAST),
        ],
        25,
    );
    let stmt = f
        .build(
            &int_schema(&[TEST_COL1, TEST_COL2, TEST_COL3]),
            &constraints,
        )
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("ORDER BY"));
    assert!(f.prepared().contains("LIMIT"));
    assert_eq!(f.parser_calls(), 1);
}

#[tokio::test]
async fn test_build_sql_with_complex_nested_expressions() {
    let f = Fixture::new();
    f.set_expressions(&[
        "((col1 + col2) * (col3 - col4))",
        "CASE WHEN (col5 > col6) AND (col7 < col8) THEN col9 ELSE col10 END",
        "NULLIF(COALESCE(col11, col12), col13)",
    ]);
    let constraints = create_constraints_with_order_by_and_limit(
        vec![OrderByField::new(TEST_COL1, Direction::ASC_NULLS_FIRST)],
        10,
    );
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("ORDER BY"));
    assert!(f.prepared().contains("LIMIT"));
    assert_eq!(f.parser_calls(), 1);
}

#[tokio::test]
async fn test_build_sql_with_aggregate_functions_in_order_by() {
    let f = Fixture::new();
    f.set_expressions(&[
        "SUM(col1 + col2)",
        "AVG(CASE WHEN col3 > 0 THEN col4 ELSE 0 END)",
        "COUNT(DISTINCT col5)",
    ]);
    let constraints = create_constraints_with_order_by_and_limit(
        vec![OrderByField::new("SUM(col1)", Direction::DESC_NULLS_LAST)],
        100,
    );
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("ORDER BY"));
    assert!(f.prepared().contains("LIMIT"));
    assert_eq!(f.parser_calls(), 1);
}

#[tokio::test]
async fn test_build_sql_with_decimal_type() {
    let f = Fixture::new();
    let decimal_type = DataType::Decimal128(10, 2);
    let schema = Schema::new(vec![Field::new("decimal_col", decimal_type.clone(), true)]);
    let value: BigDecimal = "123.45".parse().unwrap();
    let value_set =
        single_range_value_set(Range::equal(&f.allocator, &decimal_type, value.clone()).unwrap());
    let constraints = create_constraints_with_summary(vec![("decimal_col", value_set)]);
    let stmt = f.build(&schema, &constraints).await.unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("\"decimal_col\""));
    assert_eq!(stmt.parameter(1), Some(&SqlValue::Decimal(value)));
}

#[tokio::test]
async fn test_build_sql_with_null_only_value_set() {
    let f = Fixture::new();
    let value_set = sorted(SortedRangeSet::only_null(&DataType::Int32));
    let constraints = create_constraints_with_summary(vec![(TEST_COL1, value_set)]);
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("IS NULL"));
}

#[tokio::test]
async fn test_build_sql_with_not_null_value_set() {
    let f = Fixture::new();
    let value_set = sorted(
        SortedRangeSet::copy_of(
            &DataType::Int32,
            [Range::all(&f.allocator, &DataType::Int32)],
            false,
        )
        .unwrap(),
    );
    let constraints = create_constraints_with_summary(vec![(TEST_COL1, value_set)]);
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("IS NOT NULL"));
}

#[tokio::test]
async fn test_build_sql_with_null_allowed_and_ranges() {
    let f = Fixture::new();
    let range = Range::equal(&f.allocator, &DataType::Int32, 100).unwrap();
    let value_set = sorted(SortedRangeSet::copy_of(&DataType::Int32, [range], true).unwrap());
    let constraints = create_constraints_with_summary(vec![(TEST_COL1, value_set)]);
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &constraints)
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("IS NULL"));
}

#[test]
fn test_build_sql_with_unhandled_bound_exception() {
    let a = BlockAllocatorImpl::new();
    let low = Marker::below(&a, &DataType::Int32, 100).unwrap();
    let high = Marker::upper_unbounded(&a, &DataType::Int32);
    let error = Range::new(low, high).unwrap_err();
    assert!(error
        .to_string()
        .contains("low bound must be EXACTLY or ABOVE"));
}

#[test]
fn test_build_sql_with_unhandled_high_bound_exception() {
    let a = BlockAllocatorImpl::new();
    let low = Marker::lower_unbounded(&a, &DataType::Int32);
    let high = Marker::above(&a, &DataType::Int32, 200).unwrap();
    let error = Range::new(low, high).unwrap_err();
    assert!(error
        .to_string()
        .contains("high bound must be EXACTLY or BELOW"));
}

#[tokio::test]
async fn test_build_sql_with_empty_column_names() {
    let mut f = Fixture::new();
    f.split = Split::new(
        None,
        None,
        HashMap::from([(TEST_COL1.to_string(), "filtered_value".to_string())]),
    );
    let stmt = f
        .build(&int_schema(&[TEST_COL1]), &create_empty_constraints())
        .await
        .unwrap();
    assert_eq!(stmt.sql(), f.prepared());
    assert!(f.prepared().contains("null"));
}

#[test]
fn test_get_sql_dialect() {
    let f = Fixture::new();
    assert_eq!(f.builder.get_sql_dialect(), SqlDialect::ansi());
}

#[test]
fn test_constructor_without_expression_parser() {
    let builder = TestBuilder {
        base: JdbcSplitQueryBuilderBase::new(QUOTE_CHAR),
    };
    assert_eq!(builder.quote("test"), "\"test\"");
}

#[tokio::test]
async fn test_prepare_statement_with_calcite_sql_success() {
    for plan in SUCCESS_PLANS {
        let f = Fixture::new();
        let result = f
            .builder
            .prepare_statement_with_calcite_sql(
                &f.connection,
                &plan_constraints(plan),
                &SqlDialect::ansi(),
                &f.split,
            )
            .await;
        let statement = result.unwrap_or_else(|e| panic!("plan {plan} failed: {e:?}"));
        assert_eq!(statement.sql(), f.prepared());
    }
}

#[tokio::test]
async fn test_prepare_statement_with_calcite_sql_parameter_count_mismatch() {
    let plan = "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSEhoQCAIQARoKZ3Q6YW55X2FueRq8ARK5AQq2ARKzAQoCCgAScQpvCgIKABJYCgJpZAoEbmFtZQoFZW1haWwKA2FnZQoIbG9jYXRpb24KDnBhcnRpdGlvbl9uYW1lEiYKBCoCEAEKBGICEAEKBGICEAEKBCoCEAEKBGICEAEKBGICEAEYAjoPCgZ0ZXN0ZGIKBXVzZXJzGjoaOBoECgIQASIlGiMaIQgBGgQKAhABIgwaChIICgQSAggDIgAiCRoHCgUoPJADASIJGgcKBQgBkAMB";
    let f = Fixture::new();
    f.connection
        .on_prepare(|sql| Ok(PreparedStatement::new(sql).with_parameter_count(0)));
    let statement = f
        .builder
        .prepare_statement_with_calcite_sql(
            &f.connection,
            &plan_constraints(plan),
            &SqlDialect::ansi(),
            &f.split,
        )
        .await
        .unwrap();
    let sql = f.prepared();
    assert!(sql.contains("SELECT"));
    assert!(sql.contains("`testdb`.`users`"));
    assert!(sql.contains("`age` > ?"));
    assert!(statement.parameters().is_empty());
}

#[tokio::test]
async fn test_prepare_statement_with_calcite_sql_partition_clauses_before_fetch_next_no_existing_where(
) {
    let plan = SUCCESS_PLANS[8];
    let f = Fixture::new();
    f.builder
        .prepare_statement_with_calcite_sql(
            &f.connection,
            &plan_constraints(plan),
            &SqlDialect::ansi(),
            &f.split,
        )
        .await
        .unwrap();
    assert_eq!(
        f.prepared(),
        "SELECT *\nFROM (SELECT * FROM `warehouse`.`call_center` WHERE \"partition_col\" = '2024') AS `call_center`\nORDER BY `cc_rec_start_date`\nFETCH NEXT 100 ROWS ONLY"
    );
}

#[tokio::test]
async fn test_prepare_statement_with_calcite_sql_partition_clauses_before_fetch_next_with_existing_where(
) {
    let plan = "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIOGgwIARoIYW5kOmJvb2wSFRoTCAIQARoNZXF1YWw6YW55X2FueRrUAxLRAwrOAxrLAwoCCgASwAMSvQMKAgoAEvkCCvYCCgIKABLdAgoKbF9vcmRlcmtleQoJbF9wYXJ0a2V5CglsX3N1cHBrZXkKDGxfbGluZW51bWJlcgoKbF9xdWFudGl0eQoPbF9leHRlbmRlZHByaWNlCgpsX2Rpc2NvdW50CgVsX3RheAoMbF9yZXR1cm5mbGFnCgxsX2xpbmVzdGF0dXMKCmxfc2hpcGRhdGUKDGxfY29tbWl0ZGF0ZQoNbF9yZWNlaXB0ZGF0ZQoObF9zaGlwaW5zdHJ1Y3QKCmxfc2hpcG1vZGUKCWxfY29tbWVudAoOcGFydGl0aW9uX25hbWUSfwoEKgIQAQoEKgIQAQoEKgIQAQoEKgIQAQoJwgEGCAIQDyABCgnCAQYIAhAPIAEKCcIBBggCEA8gAQoJwgEGCAIQDyABCgRiAhABCgRiAhABCgWCAQIQAQoFggECEAEKBYIBAhABCgRiAhABCgRiAhABCgRiAhABCgRiAhABGAI6EAoEdHBjaAoIbGluZWl0ZW0aOxo5GgQKAhABIiYaJBoiCAEaBAoCEAEiDBoKEggKBBICCAgiACIKGggKBmIBUpADASIJGgcKBQgBkAMBGAAgMg==";
    let f = Fixture::new();
    f.builder
        .prepare_statement_with_calcite_sql(
            &f.connection,
            &plan_constraints(plan),
            &SqlDialect::ansi(),
            &f.split,
        )
        .await
        .unwrap();
    assert_eq!(
        f.prepared(),
        "SELECT *\nFROM (SELECT * FROM `tpch`.`lineitem` WHERE \"partition_col\" = '2024') AS `lineitem`\nWHERE `l_returnflag` = ?\nFETCH NEXT 50 ROWS ONLY"
    );
}

#[tokio::test]
async fn test_prepare_statement_with_calcite_sql_partition_clauses_keep_or_precedence() {
    let plan = "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIPGg0IARABGgdvcjpib29sEhIaEAgCEAIaCmx0OmFueV9hbnkSFRoTCAIQAxoNZXF1YWw6YW55X2FueRrPARLMAQq/ATq8AQoGEgQKAgIDEpsBEpgBCgIKABI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRpTGlEIARoECgIQASIgGh4aHAgCGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAUiJRojGiEIAxoECgIQASIMGgoSCAoEEgIIASIAIgkaBwoFYgNuMTcaCBIGCgISACIAGgoSCAoEEgIIASIAEgJJRBIETkFNRTILEEoqB2lzdGhtdXM=";
    let f = Fixture::new();
    f.builder
        .prepare_statement_with_calcite_sql(
            &f.connection,
            &plan_constraints(plan),
            &SqlDialect::ansi(),
            &f.split,
        )
        .await
        .unwrap();
    assert_eq!(
        f.prepared(),
        "SELECT *\nFROM (SELECT * FROM `test_schema`.`test_table` WHERE \"partition_col\" = '2024') AS `test_table`\nWHERE `id` < ? OR `name` = ?"
    );
}
