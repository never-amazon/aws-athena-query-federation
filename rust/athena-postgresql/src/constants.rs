pub struct PostGreSqlConstants;

impl PostGreSqlConstants {
    pub const POSTGRES_NAME: &'static str = "postgres";
    pub const POSTGRESQL_DRIVER_CLASS: &'static str = crate::driver::POSTGRESQL_DRIVER_CLASS;
    pub const POSTGRESQL_DEFAULT_PORT: i32 = 5432;
    pub const POSTGRES_QUOTE_CHARACTER: &'static str = "\"";
}
