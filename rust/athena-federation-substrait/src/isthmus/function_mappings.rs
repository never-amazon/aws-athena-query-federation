use crate::calcite::operators::{self, SqlOperator};
use crate::proto::r#type::Kind;

pub(crate) fn function_name(key: &str) -> &str {
    key.split_once(':').map_or(key, |(name, _)| name)
}

fn is_numeric(output: Option<&Kind>) -> bool {
    matches!(
        output,
        Some(
            Kind::I8(_)
                | Kind::I16(_)
                | Kind::I32(_)
                | Kind::I64(_)
                | Kind::Fp32(_)
                | Kind::Fp64(_)
                | Kind::Decimal(_)
        )
    )
}

pub(crate) fn scalar_operator(name: &str, output: Option<&Kind>) -> Option<&'static SqlOperator> {
    let op: &'static SqlOperator = match name {
        "add" if is_numeric(output) => &operators::PLUS,
        "add" if matches!(output, Some(Kind::Date(_) | Kind::Time(_))) => &operators::DATETIME_PLUS,
        "subtract" if is_numeric(output) => &operators::MINUS,
        "subtract" if matches!(output, Some(Kind::Date(_))) => &operators::MINUS_DATE,
        "negate" => &operators::UNARY_MINUS,
        "multiply" => &operators::MULTIPLY,
        "divide" => &operators::DIVIDE,
        "abs" => &operators::ABS,
        "modulus" => &operators::MOD,
        "power" => &operators::POWER,
        "exp" => &operators::EXP,
        "sin" => &operators::SIN,
        "cos" => &operators::COS,
        "tan" => &operators::TAN,
        "asin" => &operators::ASIN,
        "acos" => &operators::ACOS,
        "atan" => &operators::ATAN,
        "atan2" => &operators::ATAN2,
        "sign" => &operators::SIGN,
        "log10" => &operators::LOG10,
        "ln" => &operators::LN,
        "and" => &operators::AND,
        "or" => &operators::OR,
        "not" => &operators::NOT,
        "lt" => &operators::LESS_THAN,
        "lte" => &operators::LESS_THAN_OR_EQUAL,
        "gt" => &operators::GREATER_THAN,
        "gte" => &operators::GREATER_THAN_OR_EQUAL,
        "equal" => &operators::EQUALS,
        "xor" => &operators::BIT_XOR,
        "is_null" => &operators::IS_NULL,
        "is_not_null" => &operators::IS_NOT_NULL,
        "not_equal" => &operators::NOT_EQUALS,
        "extract" => &operators::EXTRACT,
        "ceil" => &operators::CEIL,
        "floor" => &operators::FLOOR,
        "round" => &operators::ROUND,
        "like" => &operators::LIKE,
        "replace" => &operators::REPLACE,
        "substring" => &operators::SUBSTRING,
        "concat" => &operators::CONCAT,
        "char_length" => &operators::CHAR_LENGTH,
        "lower" => &operators::LOWER,
        "upper" => &operators::UPPER,
        "between" => &operators::BETWEEN,
        "is_not_distinct_from" => &operators::IS_NOT_DISTINCT_FROM,
        "coalesce" => &operators::COALESCE,
        "trim" | "ltrim" | "rtrim" => &operators::TRIM,
        _ => return None,
    };
    Some(op)
}

pub(crate) fn aggregate_operator(name: &str) -> Option<&'static SqlOperator> {
    let op: &'static SqlOperator = match name {
        "min" => &operators::MIN,
        "max" => &operators::MAX,
        "sum" => &operators::SUM,
        "sum0" => &operators::SUM0,
        "count" => &operators::COUNT,
        "approx_count_distinct" => &operators::APPROX_COUNT_DISTINCT,
        "avg" => &operators::AVG,
        _ => return None,
    };
    Some(op)
}

pub(crate) fn window_operator(name: &str) -> Option<&'static SqlOperator> {
    let op: &'static SqlOperator = match name {
        "row_number" => &operators::ROW_NUMBER,
        "lag" => &operators::LAG,
        "lead" => &operators::LEAD,
        "rank" => &operators::RANK,
        "dense_rank" => &operators::DENSE_RANK,
        "percent_rank" => &operators::PERCENT_RANK,
        "cume_dist" => &operators::CUME_DIST,
        "ntile" => &operators::NTILE,
        "first_value" => &operators::FIRST_VALUE,
        "last_value" => &operators::LAST_VALUE,
        "nth_value" => &operators::NTH_VALUE,
        other => return aggregate_operator(other),
    };
    Some(op)
}

#[cfg(test)]
mod tests {
    use super::scalar_operator;
    use crate::calcite::operators;
    use crate::proto::r#type::{self, Kind};

    #[test]
    fn add_and_subtract_resolve_on_the_output_type_like_isthmus() {
        let date = Kind::Date(r#type::Date::default());
        let time = Kind::Time(r#type::Time::default());
        let i32_kind = Kind::I32(r#type::I32::default());
        #[allow(
            deprecated,
            reason = "builds the deprecated Substrait timestamp kinds that plans can still carry"
        )]
        let timestamps = [
            Kind::Timestamp(r#type::Timestamp::default()),
            Kind::TimestampTz(r#type::TimestampTz::default()),
            Kind::PrecisionTimestamp(r#type::PrecisionTimestamp::default()),
        ];
        assert_eq!(
            Some(&operators::PLUS),
            scalar_operator("add", Some(&i32_kind))
        );
        assert_eq!(
            Some(&operators::DATETIME_PLUS),
            scalar_operator("add", Some(&date))
        );
        assert_eq!(
            Some(&operators::DATETIME_PLUS),
            scalar_operator("add", Some(&time))
        );
        assert_eq!(
            Some(&operators::MINUS),
            scalar_operator("subtract", Some(&i32_kind))
        );
        assert_eq!(
            Some(&operators::MINUS_DATE),
            scalar_operator("subtract", Some(&date))
        );
        assert_eq!(None, scalar_operator("subtract", Some(&time)));
        for ts in &timestamps {
            assert_eq!(None, scalar_operator("add", Some(ts)));
            assert_eq!(None, scalar_operator("subtract", Some(ts)));
        }
    }
}
