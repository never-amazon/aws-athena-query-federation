use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::util::PaginationHelper;

fn tables(names: &[(&str, &str)]) -> Vec<TableName> {
    names.iter().map(|(s, t)| TableName::new(*s, *t)).collect()
}

fn is_connector_err<T: std::fmt::Debug>(r: athena_federation_sdk::Result<T>) -> bool {
    r.map_err(|e| e.as_connector_exception().is_some())
        .err()
        .unwrap_or(false)
}

#[test]
fn test_validate_and_parse_pagination_arguments_valid_input() {
    assert_eq!(
        0,
        PaginationHelper::validate_and_parse_pagination_arguments(None, 10).unwrap()
    );
    assert_eq!(
        5,
        PaginationHelper::validate_and_parse_pagination_arguments(Some("5"), 10).unwrap()
    );
    assert_eq!(
        0,
        PaginationHelper::validate_and_parse_pagination_arguments(Some("0"), -1).unwrap()
    );
}

#[test]
fn test_validate_and_parse_pagination_arguments_invalid_next_token() {
    assert!(is_connector_err(
        PaginationHelper::validate_and_parse_pagination_arguments(Some("invalid"), 10)
    ));
    assert!(is_connector_err(
        PaginationHelper::validate_and_parse_pagination_arguments(Some("-1"), 10)
    ));
}

#[test]
fn test_validate_and_parse_pagination_arguments_invalid_page_size() {
    assert!(is_connector_err(
        PaginationHelper::validate_and_parse_pagination_arguments(Some("0"), -2)
    ));
}

#[test]
fn test_manual_pagination_normal_case() {
    let all = tables(&[
        ("schema1", "table1"),
        ("schema1", "table2"),
        ("schema2", "table3"),
        ("schema2", "table4"),
        ("schema3", "table5"),
    ]);
    let response = PaginationHelper::manual_pagination(&all, Some("1"), 2, "testCatalog").unwrap();
    let result = response.get_tables();
    assert_eq!("testCatalog", response.get_catalog_name());
    assert_eq!(2, result.len());
    assert_eq!("table2", result[0].get_table_name());
    assert_eq!("table3", result[1].get_table_name());
    assert_eq!(Some("3"), response.get_next_token());
}

#[test]
fn test_manual_pagination_unlimited_page_size() {
    let all = tables(&[
        ("schema1", "table1"),
        ("schema1", "table2"),
        ("schema2", "table3"),
    ]);
    let response = PaginationHelper::manual_pagination(&all, Some("0"), -1, "testCatalog").unwrap();
    assert_eq!("testCatalog", response.get_catalog_name());
    assert_eq!(3, response.get_tables().len());
    assert!(response.get_next_token().is_none());
}

#[test]
fn test_manual_pagination_start_token_past_end() {
    let all = tables(&[("schema1", "table1"), ("schema1", "table2")]);
    let response = PaginationHelper::manual_pagination(&all, Some("3"), 1, "testCatalog").unwrap();
    assert_eq!("testCatalog", response.get_catalog_name());
    assert!(response.get_tables().is_empty());
    assert!(response.get_next_token().is_none());
}

#[test]
fn test_manual_pagination_last_page() {
    let all = tables(&[
        ("schema1", "table1"),
        ("schema1", "table2"),
        ("schema2", "table3"),
    ]);
    let response = PaginationHelper::manual_pagination(&all, Some("2"), 2, "testCatalog").unwrap();
    assert_eq!("testCatalog", response.get_catalog_name());
    assert_eq!(1, response.get_tables().len());
    assert_eq!("table3", response.get_tables()[0].get_table_name());
    assert!(response.get_next_token().is_none());
}

#[test]
fn test_manual_pagination_sorted_results() {
    let all = tables(&[
        ("schema2", "tableC"),
        ("schema1", "tableA"),
        ("schema3", "tableB"),
        ("schema1", "tableD"),
        ("schema2", "tableE"),
    ]);
    let response = PaginationHelper::manual_pagination(&all, Some("0"), 3, "testCatalog").unwrap();
    assert_eq!("testCatalog", response.get_catalog_name());
    assert_eq!(3, response.get_tables().len());
    assert_eq!(Some("3"), response.get_next_token());
    let result = response.get_tables();
    assert_eq!("tableA", result[0].get_table_name());
    assert_eq!("tableB", result[1].get_table_name());
    assert_eq!("tableC", result[2].get_table_name());
    let response = PaginationHelper::manual_pagination(&all, Some("3"), 3, "testCatalog").unwrap();
    let result = response.get_tables();
    assert_eq!(2, result.len());
    assert_eq!("tableD", result[0].get_table_name());
    assert_eq!("tableE", result[1].get_table_name());
    assert!(response.get_next_token().is_none());
}

#[test]
fn test_calculate_next_token_unlimited_page_size() {
    let t = tables(&[("schema1", "table1"), ("schema1", "table2")]);
    assert!(PaginationHelper::calculate_next_token(0, i32::MAX, &t).is_none());
}

#[test]
fn test_calculate_next_token_empty_list() {
    assert!(PaginationHelper::calculate_next_token(5, 10, &[]).is_none());
}

#[test]
fn test_calculate_next_token_partial_page() {
    let t = tables(&[("schema1", "table1"), ("schema1", "table2")]);
    assert!(PaginationHelper::calculate_next_token(10, 5, &t).is_none());
}

#[test]
fn test_calculate_next_token_full_page() {
    let t = tables(&[
        ("schema1", "table1"),
        ("schema1", "table2"),
        ("schema1", "table3"),
    ]);
    assert_eq!(
        Some("3".to_string()),
        PaginationHelper::calculate_next_token(0, 3, &t)
    );
}

#[test]
fn test_calculate_next_token_non_zero_start_token() {
    let t = tables(&[("schema1", "table4"), ("schema1", "table5")]);
    assert_eq!(
        Some("7".to_string()),
        PaginationHelper::calculate_next_token(5, 2, &t)
    );
}

#[test]
fn test_calculate_next_token_large_values() {
    let t = tables(&[("schema1", "table1"), ("schema1", "table2")]);
    assert_eq!(
        Some((i64::from(i32::MAX) - 3).to_string()),
        PaginationHelper::calculate_next_token(i32::MAX - 5, 2, &t)
    );
}
