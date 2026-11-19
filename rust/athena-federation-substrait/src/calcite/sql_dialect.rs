use super::rel_data_type::{RelDataType, RelDataTypeSystem};
use super::sql_node::SqlDataTypeSpec;
use super::sql_type_name::SqlTypeName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseProduct {
    Unknown,
    Postgresql,
    Snowflake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Conformance {
    Default,
    Pragmatic2003,
}

impl Conformance {
    pub fn is_sort_by_alias(self) -> bool {
        self == Conformance::Default
    }

    pub fn allow_explicit_row_value_constructor(self) -> bool {
        self == Conformance::Default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SqlDialect {
    product: DatabaseProduct,
    quote_start: &'static str,
    quote_end: &'static str,
    escaped_quote: &'static str,
    upper_case_identifiers: bool,
    char_set: bool,
}

impl SqlDialect {
    pub fn ansi() -> SqlDialect {
        SqlDialect {
            product: DatabaseProduct::Unknown,
            quote_start: "`",
            quote_end: "`",
            escaped_quote: "``",
            upper_case_identifiers: false,
            char_set: true,
        }
    }

    pub fn postgresql() -> SqlDialect {
        SqlDialect {
            product: DatabaseProduct::Postgresql,
            quote_start: "\"",
            quote_end: "\"",
            escaped_quote: "\"\"",
            upper_case_identifiers: false,
            char_set: true,
        }
    }

    pub fn snowflake() -> SqlDialect {
        SqlDialect {
            product: DatabaseProduct::Snowflake,
            quote_start: "\"",
            quote_end: "\"",
            escaped_quote: "\"\"",
            upper_case_identifiers: false,
            char_set: true,
        }
    }

    pub fn postgresql_with_casing(upper_case: bool) -> SqlDialect {
        let mut d = SqlDialect::postgresql();
        d.upper_case_identifiers = upper_case;
        d
    }

    pub fn snowflake_with_casing(upper_case: bool) -> SqlDialect {
        let mut d = SqlDialect::snowflake();
        d.upper_case_identifiers = upper_case;
        d
    }

    #[must_use]
    pub fn without_char_set(mut self) -> SqlDialect {
        self.char_set = false;
        self
    }

    pub fn database_product(&self) -> DatabaseProduct {
        self.product
    }

    pub fn is_upper_casing(&self) -> bool {
        self.upper_case_identifiers
    }

    pub fn type_system(&self) -> RelDataTypeSystem {
        match self.product {
            DatabaseProduct::Postgresql => RelDataTypeSystem::Postgresql,
            _ => RelDataTypeSystem::Default,
        }
    }

    pub fn conformance(&self) -> Conformance {
        match self.product {
            DatabaseProduct::Unknown => Conformance::Default,
            _ => Conformance::Pragmatic2003,
        }
    }

    pub fn quote_identifier(&self, val: &str) -> String {
        if self.upper_case_identifiers {
            let upper = val.to_uppercase().replace('"', "\"\"");
            return format!("\"{upper}\"");
        }
        let mut out = String::new();
        out.push_str(self.quote_start);
        out.push_str(&val.replace(self.quote_end, self.escaped_quote));
        out.push_str(self.quote_end);
        out
    }

    pub fn requires_alias_for_from_items(&self) -> bool {
        self.product == DatabaseProduct::Postgresql
    }

    pub fn supports_nested_aggregations(&self) -> bool {
        self.product != DatabaseProduct::Postgresql
    }

    pub fn supports_group_by_literal(&self) -> bool {
        self.product != DatabaseProduct::Postgresql
    }

    pub fn supports_approx_count_distinct(&self) -> bool {
        self.product == DatabaseProduct::Snowflake
    }

    pub fn uses_limit_for_fetch(&self) -> bool {
        self.product == DatabaseProduct::Snowflake
    }

    pub fn supports_implicit_type_coercion(
        &self,
        cast_type: &RelDataType,
        operand_type: &RelDataType,
        operand_is_literal: bool,
    ) -> bool {
        let base = operand_type.sql_type_name().is_character();
        match self.product {
            DatabaseProduct::Postgresql => {
                if base && cast_type.sql_type_name().is_numeric() {
                    return false;
                }
                base && operand_is_literal
            }
            _ => base,
        }
    }

    pub fn cast_spec(&self, ty: &RelDataType) -> Option<SqlDataTypeSpec> {
        let tn = ty.sql_type_name();
        if self.product == DatabaseProduct::Postgresql {
            let alien = match tn {
                SqlTypeName::Tinyint => Some("smallint"),
                SqlTypeName::Double => Some("double precision"),
                _ => None,
            };
            if let Some(a) = alien {
                return Some(SqlDataTypeSpec {
                    type_name: a.to_string(),
                    precision: None,
                    scale: None,
                    charset: None,
                    alien: true,
                    collection: None,
                });
            }
        }
        if tn == SqlTypeName::Null {
            return None;
        }
        let system = self.type_system();
        let clamp = |value: Option<i32>, max: Option<i32>| match (value, max) {
            (Some(v), Some(m)) if m > 0 && v > m => Some(m),
            _ => value,
        };
        let (precision, scale) = match tn {
            SqlTypeName::Decimal => (
                clamp(ty.precision(), system.max_precision(tn)),
                clamp(ty.scale(), system.max_scale(tn)),
            ),
            SqlTypeName::Char | SqlTypeName::Varchar => {
                (clamp(ty.precision(), system.max_precision(tn)), ty.scale())
            }
            _ => (ty.precision(), ty.scale()),
        };
        let precision = precision.filter(|_| tn.allows_prec());
        let scale = scale.filter(|_| tn.allows_scale());
        let charset = if tn.is_character() && self.supports_char_set() {
            Some("UTF-8".to_string())
        } else {
            None
        };
        let type_name = match tn {
            SqlTypeName::IntervalYearMonth => "INTERVAL YEAR TO MONTH".to_string(),
            SqlTypeName::IntervalDaySecond => "INTERVAL DAY TO SECOND".to_string(),
            SqlTypeName::TimeWithLocalTimeZone => "TIME WITH LOCAL TIME ZONE".to_string(),
            SqlTypeName::TimestampWithLocalTimeZone => "TIMESTAMP WITH LOCAL TIME ZONE".to_string(),
            other => other.name().to_string(),
        };
        Some(SqlDataTypeSpec {
            type_name,
            precision,
            scale,
            charset,
            alien: false,
            collection: None,
        })
    }

    pub fn supports_char_set(&self) -> bool {
        self.char_set && self.product != DatabaseProduct::Postgresql
    }
}
