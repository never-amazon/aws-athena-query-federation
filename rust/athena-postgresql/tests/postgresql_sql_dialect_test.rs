use athena_federation_substrait::DatabaseProduct;
use athena_postgresql::PostgresqlSqlDialect;

#[test]
fn test_default_dialect() {
    let dialect = PostgresqlSqlDialect::default_dialect();
    assert_eq!(DatabaseProduct::Postgresql, dialect.database_product());
    assert!(!dialect.is_upper_casing());
}

#[test]
fn test_quote_identifier_with_filter() {
    let dialect = PostgresqlSqlDialect::new(true);
    let mut buf = String::new();
    dialect.quote_identifier(&mut buf, "employees");
    assert_eq!("\"EMPLOYEES\"", buf);
    assert!(dialect.sql_dialect().is_upper_casing());
}

#[test]
fn test_quote_identifier_without_filter() {
    let dialect = PostgresqlSqlDialect::new(false);
    let mut buf = String::new();
    dialect.quote_identifier(&mut buf, "employees");
    assert_eq!("\"employees\"", buf);
    assert!(!dialect.sql_dialect().is_upper_casing());
}
