use crate::calcite::operators::{self, SqlOperator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FunctionDecl {
    pub(crate) name: String,
    pub(crate) key: String,
}

impl FunctionDecl {
    pub(crate) fn from_key(key: &str) -> FunctionDecl {
        let name = match key.find(':') {
            Some(i) => &key[..i],
            None => key,
        };
        FunctionDecl {
            name: name.to_string(),
            key: key.to_string(),
        }
    }
}

const NUMERIC_OUTPUTS: [&str; 7] = ["i8", "i16", "i32", "i64", "fp32", "fp64", "dec"];

pub(crate) fn scalar_operator(name: &str, output_type: &str) -> Option<&'static SqlOperator> {
    let op: &'static SqlOperator = match name {
        "add" => {
            if NUMERIC_OUTPUTS.contains(&output_type) {
                &operators::PLUS
            } else {
                return None;
            }
        }
        "subtract" => {
            if NUMERIC_OUTPUTS.contains(&output_type) {
                &operators::MINUS
            } else {
                return None;
            }
        }
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
