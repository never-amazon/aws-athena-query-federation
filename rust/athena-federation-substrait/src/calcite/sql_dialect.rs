use super::rel_data_type::{RelDataType, RelDataTypeSystem, PRECISION_NOT_SPECIFIED, SCALE_NOT_SPECIFIED};
use super::rex::{Direction, NullDirection};
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
    pub fn is_sort_by_ordinal(self) -> bool {
        true
    }

    pub fn is_sort_by_alias(self) -> bool {
        self == Conformance::Default
    }

    pub fn is_having_alias(self) -> bool {
        false
    }

    pub fn allow_explicit_row_value_constructor(self) -> bool {
        self == Conformance::Default
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SqlDialect {
    product: DatabaseProduct,
    quote_start: String,
    quote_end: String,
    escaped_quote: String,
    upper_case_identifiers: bool,
}

impl SqlDialect {
    pub fn ansi() -> SqlDialect {
        SqlDialect {
            product: DatabaseProduct::Unknown,
            quote_start: "`".to_string(),
            quote_end: "`".to_string(),
            escaped_quote: "``".to_string(),
            upper_case_identifiers: false,
        }
    }

    pub fn postgresql() -> SqlDialect {
        SqlDialect {
            product: DatabaseProduct::Postgresql,
            quote_start: "\"".to_string(),
            quote_end: "\"".to_string(),
            escaped_quote: "\"\"".to_string(),
            upper_case_identifiers: false,
        }
    }

    pub fn snowflake() -> SqlDialect {
        SqlDialect {
            product: DatabaseProduct::Snowflake,
            quote_start: "\"".to_string(),
            quote_end: "\"".to_string(),
            escaped_quote: "\"\"".to_string(),
            upper_case_identifiers: false,
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

    pub fn get_database_product(&self) -> DatabaseProduct {
        self.product
    }

    pub fn is_upper_casing(&self) -> bool {
        self.upper_case_identifiers
    }

    pub fn get_type_system(&self) -> RelDataTypeSystem {
        match self.product {
            DatabaseProduct::Postgresql => RelDataTypeSystem::Postgresql,
            _ => RelDataTypeSystem::Default,
        }
    }

    pub fn get_conformance(&self) -> Conformance {
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
        out.push_str(&self.quote_start);
        out.push_str(&val.replace(&self.quote_end, &self.escaped_quote));
        out.push_str(&self.quote_end);
        out
    }

    pub fn quote_string_literal(&self, val: &str) -> String {
        format!("'{}'", val.replace('\'', "''"))
    }

    pub fn has_implicit_table_alias(&self) -> bool {
        true
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

    pub fn supports_aggregate_function_filter(&self) -> bool {
        true
    }

    pub fn supports_approx_count_distinct(&self) -> bool {
        self.product == DatabaseProduct::Snowflake
    }

    pub fn supports_boolean_case_when(&self) -> bool {
        true
    }

    pub fn uses_limit_for_fetch(&self) -> bool {
        self.product == DatabaseProduct::Snowflake
    }

    pub fn default_null_direction(&self, direction: Direction) -> NullDirection {
        match direction {
            Direction::Ascending => NullDirection::Last,
            Direction::Descending => NullDirection::First,
        }
    }

    pub fn supports_implicit_type_coercion(&self, cast_type: &RelDataType, operand_type: &RelDataType, operand_is_literal: bool) -> bool {
        let base = operand_type.get_sql_type_name().is_character();
        match self.product {
            DatabaseProduct::Postgresql => {
                if base && cast_type.get_sql_type_name().is_numeric() {
                    return false;
                }
                base && operand_is_literal
            }
            _ => base,
        }
    }

    pub fn get_cast_spec(&self, ty: &RelDataType) -> Option<SqlDataTypeSpec> {
        let tn = ty.get_sql_type_name();
        if self.product == DatabaseProduct::Postgresql {
            let alien = match tn {
                SqlTypeName::Tinyint => Some("smallint"),
                SqlTypeName::Double => Some("double precision"),
                _ => None,
            };
            if let Some(a) = alien {
                return Some(SqlDataTypeSpec {
                    type_name: a.to_string(),
                    precision: PRECISION_NOT_SPECIFIED,
                    scale: SCALE_NOT_SPECIFIED,
                    charset: None,
                    alien: true,
                    collection: None,
                });
            }
        }
        if tn == SqlTypeName::Null {
            return None;
        }
        let system = self.get_type_system();
        let mut precision = ty.get_precision();
        let mut scale = ty.get_scale();
        match tn {
            SqlTypeName::Decimal => {
                let max_scale = system.max_scale(tn);
                let max_precision = system.max_precision(tn);
                if max_precision > 0 && precision > max_precision {
                    precision = max_precision;
                }
                if max_scale > 0 && scale > max_scale {
                    scale = max_scale;
                }
            }
            SqlTypeName::Char | SqlTypeName::Varchar => {
                let max_precision = system.max_precision(tn);
                if max_precision > 0 && precision > max_precision {
                    precision = max_precision;
                }
            }
            _ => {}
        }
        if !tn.allows_prec() {
            precision = PRECISION_NOT_SPECIFIED;
        }
        if !tn.allows_scale() {
            scale = SCALE_NOT_SPECIFIED;
        }
        let charset = if tn.is_character() && self.supports_char_set() {
            Some("ISO-8859-1".to_string())
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
        self.product != DatabaseProduct::Postgresql
    }
}
