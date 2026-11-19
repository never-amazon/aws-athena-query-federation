use athena_jdbc::splits::splitter;
use athena_jdbc::sql::{ColumnMetadata, JdbcType, ResultSet, ResultSetMetaData, Row, SqlValue};

const TEST_COLUMN_NAME: &str = "testColumn";
const MAX_SPLITS: i32 = 10;

async fn row_of_type(column_type: JdbcType) -> Row {
    let metadata = ResultSetMetaData::new(vec![
        ColumnMetadata::new("low", column_type),
        ColumnMetadata::new("high", column_type),
    ]);
    let mut result_set = ResultSet::from_rows(metadata, vec![vec![SqlValue::Null, SqlValue::Null]]);
    result_set.next_row().await.unwrap().unwrap()
}

async fn splitter_present(column_type: JdbcType) -> bool {
    let row = row_of_type(column_type).await;
    splitter(TEST_COLUMN_NAME, &row, MAX_SPLITS)
        .unwrap()
        .is_some()
}

#[tokio::test]
async fn get_integer_splitter() {
    let row = row_of_type(JdbcType::Integer).await;
    let mut splitter = splitter(TEST_COLUMN_NAME, &row, MAX_SPLITS)
        .unwrap()
        .unwrap();
    assert!(splitter.has_next());
    assert_eq!(
        "(testColumn >= 0 AND testColumn <= 0)",
        splitter.next_range_clause()
    );
}

#[tokio::test]
async fn get_string_splitter() {
    assert!(!splitter_present(JdbcType::Varchar).await);
}

#[tokio::test]
async fn get_float_splitter() {
    assert!(!splitter_present(JdbcType::Float).await);
}

#[tokio::test]
async fn get_double_splitter() {
    assert!(!splitter_present(JdbcType::Double).await);
}

#[tokio::test]
async fn get_date_splitter() {
    assert!(!splitter_present(JdbcType::Date).await);
}

#[tokio::test]
async fn get_decimal_splitter() {
    assert!(!splitter_present(JdbcType::Decimal).await);
}
