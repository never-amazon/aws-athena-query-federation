use athena_federation_substrait::DatabaseProduct;
use athena_snowflake::SnowflakeDialect;

fn quote(dialect: &SnowflakeDialect, identifier: &str) -> String {
    let mut buf = String::new();
    dialect.quote_identifier(&mut buf, identifier).clone()
}

#[test]
fn quote_identifier_with_casing_filter_uppercases() {
    let dialect = SnowflakeDialect::new(true);

    assert_eq!("\"MYTABLE\"", quote(&dialect, "mytable"));
    assert_eq!("\"MYTABLE\"", quote(&dialect, "MyTable"));
    assert_eq!("\"ALREADY_UPPER\"", quote(&dialect, "ALREADY_UPPER"));
}

#[test]
fn quote_identifier_without_casing_filter_preserves_case() {
    let dialect = SnowflakeDialect::new(false);

    assert_eq!("\"mytable\"", quote(&dialect, "mytable"));
    assert_eq!("\"MyTable\"", quote(&dialect, "MyTable"));
}

#[test]
fn quote_identifier_with_casing_filter_escapes_embedded_quote() {
    let dialect = SnowflakeDialect::new(true);

    assert_eq!("\"WEIRD\"\"NAME\"", quote(&dialect, "weird\"name"));
}

#[test]
fn quote_identifier_appends_to_existing_buffer() {
    let dialect = SnowflakeDialect::new(true);

    let mut buf = String::from("SELECT * FROM ");
    assert_eq!(
        "SELECT * FROM \"T\"",
        dialect.quote_identifier(&mut buf, "t").as_str()
    );
}

#[test]
fn default_dialect_is_available() {
    let dialect = SnowflakeDialect::default_dialect();

    assert_eq!(DatabaseProduct::Snowflake, dialect.database_product());
    assert!(!dialect.is_upper_casing());
}
