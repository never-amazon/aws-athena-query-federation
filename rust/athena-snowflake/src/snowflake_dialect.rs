use athena_federation_substrait::SqlDialect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnowflakeDialect {
    inner: SqlDialect,
}

impl SnowflakeDialect {
    pub fn default_dialect() -> SqlDialect {
        SqlDialect::snowflake()
    }

    pub fn new(catalog_casing_filter: bool) -> SnowflakeDialect {
        SnowflakeDialect {
            inner: SqlDialect::snowflake_with_casing(catalog_casing_filter).without_char_set(),
        }
    }

    pub fn quote_identifier<'a>(&self, buf: &'a mut String, identifier: &str) -> &'a mut String {
        buf.push_str(&self.inner.quote_identifier(identifier));
        buf
    }

    pub fn sql_dialect(&self) -> &SqlDialect {
        &self.inner
    }

    pub fn into_sql_dialect(self) -> SqlDialect {
        self.inner
    }
}
