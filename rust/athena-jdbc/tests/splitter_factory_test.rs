use athena_jdbc::splits::SplitterFactory;
use athena_jdbc::sql::{ColumnMetadata, ResultSet, ResultSetMetaData, SqlValue, Types};

const TEST_COLUMN_NAME: &str = "testColumn";
const MAX_SPLITS: i32 = 10;

async fn result_set_of_type(column_type: i32) -> ResultSet {
    let metadata = ResultSetMetaData::new(vec![
        ColumnMetadata::new("low", column_type),
        ColumnMetadata::new("high", column_type),
    ]);
    let mut result_set = ResultSet::from_rows(metadata, vec![vec![SqlValue::Null, SqlValue::Null]]);
    assert!(result_set.next().await.unwrap());
    result_set
}

async fn splitter_present(column_type: i32) -> bool {
    let result_set = result_set_of_type(column_type).await;
    SplitterFactory::new()
        .get_splitter(TEST_COLUMN_NAME, &result_set, MAX_SPLITS)
        .unwrap()
        .is_some()
}

#[tokio::test]
async fn get_integer_splitter() {
    let result_set = result_set_of_type(Types::INTEGER).await;
    let mut splitter = SplitterFactory::new()
        .get_splitter(TEST_COLUMN_NAME, &result_set, MAX_SPLITS)
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
    assert!(!splitter_present(Types::VARCHAR).await);
}

#[tokio::test]
async fn get_float_splitter() {
    assert!(!splitter_present(Types::FLOAT).await);
}

#[tokio::test]
async fn get_double_splitter() {
    assert!(!splitter_present(Types::DOUBLE).await);
}

#[tokio::test]
async fn get_date_splitter() {
    assert!(!splitter_present(Types::DATE).await);
}

#[tokio::test]
async fn get_decimal_splitter() {
    assert!(!splitter_present(Types::DECIMAL).await);
}
