use athena_federation_substrait::SqlDialect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PostgresqlSqlDialect {
    catalog_casing_filter: bool,
}

impl PostgresqlSqlDialect {
    pub fn default_dialect() -> SqlDialect {
        SqlDialect::postgresql()
    }

    pub fn new(catalog_casing_filter: bool) -> PostgresqlSqlDialect {
        PostgresqlSqlDialect {
            catalog_casing_filter,
        }
    }

    pub fn catalog_casing_filter(&self) -> bool {
        self.catalog_casing_filter
    }

    pub fn quote_identifier(&self, buf: &mut String, identifier: &str) {
        let identifier = if self.catalog_casing_filter {
            identifier.to_uppercase()
        } else {
            identifier.to_string()
        };
        buf.push('"');
        buf.push_str(&identifier.replace('"', "\"\""));
        buf.push('"');
    }

    pub fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::postgresql_with_casing(self.catalog_casing_filter)
    }
}
