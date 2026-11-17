use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SqlKind {
    Select,
    Identifier,
    Literal,
    DynamicParam,
    InputRef,
    As,
    And,
    Or,
    Not,
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    IsDistinctFrom,
    IsNotDistinctFrom,
    Plus,
    Minus,
    Times,
    Divide,
    Mod,
    MinusPrefix,
    PlusPrefix,
    IsNull,
    IsNotNull,
    IsTrue,
    IsNotTrue,
    IsFalse,
    IsNotFalse,
    Like,
    In,
    NotIn,
    Search,
    Between,
    Case,
    Cast,
    SafeCast,
    Extract,
    Coalesce,
    Trim,
    Ceil,
    Floor,
    CharLength,
    Descending,
    NullsFirst,
    NullsLast,
    Over,
    Window,
    Count,
    Sum,
    Sum0,
    Min,
    Max,
    Avg,
    RowNumber,
    Rank,
    DenseRank,
    PercentRank,
    CumeDist,
    Ntile,
    Lag,
    Lead,
    FirstValue,
    LastValue,
    NthValue,
    ArrayValueConstructor,
    MapValueConstructor,
    Row,
    Values,
    Exists,
    Unique,
    ScalarQuery,
    FieldAccess,
    Join,
    Union,
    Intersect,
    Except,
    Rollup,
    Cube,
    GroupingSets,
    IntervalQualifier,
    DataType,
    NodeList,
    OtherFunction,
    Other,
}

impl SqlKind {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            SqlKind::Equals
                | SqlKind::NotEquals
                | SqlKind::GreaterThan
                | SqlKind::GreaterThanOrEqual
                | SqlKind::LessThan
                | SqlKind::LessThanOrEqual
                | SqlKind::IsDistinctFrom
                | SqlKind::IsNotDistinctFrom
                | SqlKind::In
                | SqlKind::NotIn
        )
    }

    pub fn is_binary_comparison(self) -> bool {
        matches!(
            self,
            SqlKind::Equals
                | SqlKind::NotEquals
                | SqlKind::GreaterThan
                | SqlKind::GreaterThanOrEqual
                | SqlKind::LessThan
                | SqlKind::LessThanOrEqual
        )
    }

    #[must_use]
    pub fn reverse(self) -> SqlKind {
        match self {
            SqlKind::GreaterThan => SqlKind::LessThan,
            SqlKind::GreaterThanOrEqual => SqlKind::LessThanOrEqual,
            SqlKind::LessThan => SqlKind::GreaterThan,
            SqlKind::LessThanOrEqual => SqlKind::GreaterThanOrEqual,
            other => other,
        }
    }

    #[must_use]
    pub fn negate_null_safe(self) -> SqlKind {
        match self {
            SqlKind::Equals => SqlKind::NotEquals,
            SqlKind::NotEquals => SqlKind::Equals,
            SqlKind::LessThan => SqlKind::GreaterThanOrEqual,
            SqlKind::GreaterThan => SqlKind::LessThanOrEqual,
            SqlKind::LessThanOrEqual => SqlKind::GreaterThan,
            SqlKind::GreaterThanOrEqual => SqlKind::LessThan,
            SqlKind::IsTrue => SqlKind::IsNotTrue,
            SqlKind::IsFalse => SqlKind::IsNotFalse,
            SqlKind::IsNull => SqlKind::IsNotNull,
            SqlKind::IsNotTrue => SqlKind::IsTrue,
            SqlKind::IsNotFalse => SqlKind::IsFalse,
            SqlKind::IsNotNull => SqlKind::IsNull,
            SqlKind::IsDistinctFrom => SqlKind::IsNotDistinctFrom,
            SqlKind::IsNotDistinctFrom => SqlKind::IsDistinctFrom,
            SqlKind::In => SqlKind::NotIn,
            SqlKind::NotIn => SqlKind::In,
            other => other,
        }
    }

    pub fn is_unparse_comparison(self) -> bool {
        matches!(
            self,
            SqlKind::In
                | SqlKind::NotIn
                | SqlKind::Equals
                | SqlKind::NotEquals
                | SqlKind::LessThan
                | SqlKind::GreaterThan
                | SqlKind::GreaterThanOrEqual
                | SqlKind::LessThanOrEqual
        )
    }

    pub fn is_aggregate(self) -> bool {
        matches!(
            self,
            SqlKind::Count
                | SqlKind::Sum
                | SqlKind::Sum0
                | SqlKind::Min
                | SqlKind::Max
                | SqlKind::Avg
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlSyntax {
    Binary,
    Prefix,
    Postfix,
    Function,
    FunctionStar,
    FunctionId,
    Special,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SqlOperator {
    pub name: &'static str,
    pub kind: SqlKind,
    pub left_prec: i32,
    pub right_prec: i32,
    pub syntax: SqlSyntax,
    pub aggregate: bool,
    pub negated: bool,
}

impl fmt::Display for SqlOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

const fn binary(name: &'static str, kind: SqlKind, prec: i32, left_assoc: bool) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: if left_assoc { prec } else { prec + 1 },
        right_prec: if left_assoc { prec + 1 } else { prec },
        syntax: SqlSyntax::Binary,
        aggregate: false,
        negated: false,
    }
}

const fn prefix(name: &'static str, kind: SqlKind, prec: i32) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: prec,
        right_prec: prec + 1,
        syntax: SqlSyntax::Prefix,
        aggregate: false,
        negated: false,
    }
}

const fn postfix(name: &'static str, kind: SqlKind, prec: i32) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: prec,
        right_prec: prec + 1,
        syntax: SqlSyntax::Postfix,
        aggregate: false,
        negated: false,
    }
}

const fn function(name: &'static str, kind: SqlKind) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: 100,
        right_prec: 100,
        syntax: SqlSyntax::Function,
        aggregate: false,
        negated: false,
    }
}

const fn special(name: &'static str, kind: SqlKind, prec: i32) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: prec,
        right_prec: prec + 1,
        syntax: SqlSyntax::Special,
        aggregate: false,
        negated: false,
    }
}

const fn agg(name: &'static str, kind: SqlKind) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: 100,
        right_prec: 100,
        syntax: SqlSyntax::Function,
        aggregate: true,
        negated: false,
    }
}

const fn agg_star(name: &'static str, kind: SqlKind) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: 100,
        right_prec: 100,
        syntax: SqlSyntax::FunctionStar,
        aggregate: true,
        negated: false,
    }
}

pub static AND: SqlOperator = binary("AND", SqlKind::And, 24, true);
pub static OR: SqlOperator = binary("OR", SqlKind::Or, 22, true);
pub static NOT: SqlOperator = prefix("NOT", SqlKind::Not, 26);
pub static EQUALS: SqlOperator = binary("=", SqlKind::Equals, 30, true);
pub static NOT_EQUALS: SqlOperator = binary("<>", SqlKind::NotEquals, 30, true);
pub static GREATER_THAN: SqlOperator = binary(">", SqlKind::GreaterThan, 30, true);
pub static GREATER_THAN_OR_EQUAL: SqlOperator = binary(">=", SqlKind::GreaterThanOrEqual, 30, true);
pub static LESS_THAN: SqlOperator = binary("<", SqlKind::LessThan, 30, true);
pub static LESS_THAN_OR_EQUAL: SqlOperator = binary("<=", SqlKind::LessThanOrEqual, 30, true);
pub static IS_DISTINCT_FROM: SqlOperator =
    binary("IS DISTINCT FROM", SqlKind::IsDistinctFrom, 30, true);
pub static IS_NOT_DISTINCT_FROM: SqlOperator =
    binary("IS NOT DISTINCT FROM", SqlKind::IsNotDistinctFrom, 30, true);
pub static PLUS: SqlOperator = binary("+", SqlKind::Plus, 40, true);
pub static MINUS: SqlOperator = binary("-", SqlKind::Minus, 40, true);
pub static DATETIME_PLUS: SqlOperator = SqlOperator {
    syntax: SqlSyntax::Special,
    ..binary("+", SqlKind::Plus, 40, true)
};
pub static MINUS_DATE: SqlOperator = SqlOperator {
    syntax: SqlSyntax::Special,
    ..binary("-", SqlKind::Minus, 40, true)
};
pub static MULTIPLY: SqlOperator = binary("*", SqlKind::Times, 60, true);
pub static DIVIDE: SqlOperator = binary("/", SqlKind::Divide, 60, true);
pub static CONCAT: SqlOperator = binary("||", SqlKind::Other, 60, true);
pub static UNARY_MINUS: SqlOperator = prefix("-", SqlKind::MinusPrefix, 80);
pub static UNARY_PLUS: SqlOperator = prefix("+", SqlKind::PlusPrefix, 80);
pub static IS_NULL: SqlOperator = postfix("IS NULL", SqlKind::IsNull, 28);
pub static IS_NOT_NULL: SqlOperator = postfix("IS NOT NULL", SqlKind::IsNotNull, 28);
pub static IS_TRUE: SqlOperator = postfix("IS TRUE", SqlKind::IsTrue, 28);
pub static IS_NOT_TRUE: SqlOperator = postfix("IS NOT TRUE", SqlKind::IsNotTrue, 28);
pub static IS_FALSE: SqlOperator = postfix("IS FALSE", SqlKind::IsFalse, 28);
pub static IS_NOT_FALSE: SqlOperator = postfix("IS NOT FALSE", SqlKind::IsNotFalse, 28);
pub static DESC: SqlOperator = postfix("DESC", SqlKind::Descending, 20);
pub static NULLS_FIRST: SqlOperator = postfix("NULLS FIRST", SqlKind::NullsFirst, 18);
pub static NULLS_LAST: SqlOperator = postfix("NULLS LAST", SqlKind::NullsLast, 18);
pub static IN: SqlOperator = binary("IN", SqlKind::In, 32, true);
pub static EXISTS: SqlOperator = prefix("EXISTS", SqlKind::Exists, 40);
pub static UNIQUE: SqlOperator = prefix("UNIQUE", SqlKind::Unique, 40);
pub static SCALAR_QUERY: SqlOperator = SqlOperator {
    name: "$SCALAR_QUERY",
    kind: SqlKind::ScalarQuery,
    left_prec: 1,
    right_prec: 0,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static UNION: SqlOperator = binary("UNION", SqlKind::Union, 12, true);
pub static UNION_ALL: SqlOperator = binary("UNION ALL", SqlKind::Union, 12, true);
pub static INTERSECT: SqlOperator = binary("INTERSECT", SqlKind::Intersect, 14, true);
pub static INTERSECT_ALL: SqlOperator = binary("INTERSECT ALL", SqlKind::Intersect, 14, true);
pub static EXCEPT: SqlOperator = binary("EXCEPT", SqlKind::Except, 12, true);
pub static EXCEPT_ALL: SqlOperator = binary("EXCEPT ALL", SqlKind::Except, 12, true);
const fn join(name: &'static str) -> SqlOperator {
    SqlOperator {
        name,
        kind: SqlKind::Join,
        left_prec: 16,
        right_prec: 17,
        syntax: SqlSyntax::Special,
        aggregate: false,
        negated: false,
    }
}
pub static INNER_JOIN: SqlOperator = join("INNER JOIN");
pub static LEFT_JOIN: SqlOperator = join("LEFT JOIN");
pub static RIGHT_JOIN: SqlOperator = join("RIGHT JOIN");
pub static FULL_JOIN: SqlOperator = join("FULL JOIN");
pub static CROSS_JOIN: SqlOperator = join("CROSS JOIN");
pub static COMMA_JOIN: SqlOperator = join(",");
const fn grouping(name: &'static str, kind: SqlKind) -> SqlOperator {
    SqlOperator {
        name,
        kind,
        left_prec: 4,
        right_prec: 5,
        syntax: SqlSyntax::Special,
        aggregate: false,
        negated: false,
    }
}
pub static ROLLUP: SqlOperator = grouping("ROLLUP", SqlKind::Rollup);
pub static CUBE: SqlOperator = grouping("CUBE", SqlKind::Cube);
pub static GROUPING_SETS: SqlOperator = grouping("GROUPING SETS", SqlKind::GroupingSets);
pub static NOT_IN: SqlOperator = binary("NOT IN", SqlKind::NotIn, 32, true);
pub static LIKE: SqlOperator = SqlOperator {
    name: "LIKE",
    kind: SqlKind::Like,
    left_prec: 33,
    right_prec: 32,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static NOT_LIKE: SqlOperator = SqlOperator {
    name: "NOT LIKE",
    kind: SqlKind::Like,
    left_prec: 33,
    right_prec: 32,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: true,
};
pub static BETWEEN: SqlOperator = special("BETWEEN", SqlKind::Between, 32);
pub static NOT_BETWEEN: SqlOperator = SqlOperator {
    name: "NOT BETWEEN",
    kind: SqlKind::Between,
    left_prec: 32,
    right_prec: 33,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: true,
};
pub static SEARCH: SqlOperator = function("SEARCH", SqlKind::Search);
pub static AS: SqlOperator = special("AS", SqlKind::As, 20);
pub static CASE: SqlOperator = special("CASE", SqlKind::Case, 200);
pub static CAST: SqlOperator = SqlOperator {
    name: "CAST",
    kind: SqlKind::Cast,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static SAFE_CAST: SqlOperator = SqlOperator {
    name: "SAFE_CAST",
    kind: SqlKind::SafeCast,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static EXTRACT: SqlOperator = SqlOperator {
    name: "EXTRACT",
    kind: SqlKind::Extract,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static OVER: SqlOperator = SqlOperator {
    name: "OVER",
    kind: SqlKind::Over,
    left_prec: 20,
    right_prec: 21,
    syntax: SqlSyntax::Binary,
    aggregate: false,
    negated: false,
};
pub static PRECEDING: SqlOperator = postfix("PRECEDING", SqlKind::Other, 20);
pub static FOLLOWING: SqlOperator = postfix("FOLLOWING", SqlKind::Other, 20);
pub static TRIM: SqlOperator = SqlOperator {
    name: "TRIM",
    kind: SqlKind::Trim,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static BIT_XOR: SqlOperator = function("BITXOR", SqlKind::OtherFunction);
pub static ARRAY_VALUE_CONSTRUCTOR: SqlOperator = SqlOperator {
    name: "ARRAY",
    kind: SqlKind::ArrayValueConstructor,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static ROW: SqlOperator = SqlOperator {
    name: "ROW",
    kind: SqlKind::Row,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static COALESCE: SqlOperator = function("COALESCE", SqlKind::Coalesce);
pub static UPPER: SqlOperator = function("UPPER", SqlKind::OtherFunction);
pub static LOWER: SqlOperator = function("LOWER", SqlKind::OtherFunction);
pub static CHAR_LENGTH: SqlOperator = function("CHAR_LENGTH", SqlKind::CharLength);
pub static LENGTH: SqlOperator = function("LENGTH", SqlKind::OtherFunction);
pub static SUBSTRING: SqlOperator = SqlOperator {
    name: "SUBSTRING",
    kind: SqlKind::OtherFunction,
    left_prec: 100,
    right_prec: 100,
    syntax: SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
pub static REPLACE: SqlOperator = function("REPLACE", SqlKind::OtherFunction);
pub static ABS: SqlOperator = function("ABS", SqlKind::OtherFunction);
pub static MOD: SqlOperator = function("MOD", SqlKind::Mod);
pub static POWER: SqlOperator = function("POWER", SqlKind::OtherFunction);
pub static EXP: SqlOperator = function("EXP", SqlKind::OtherFunction);
pub static SIN: SqlOperator = function("SIN", SqlKind::OtherFunction);
pub static COS: SqlOperator = function("COS", SqlKind::OtherFunction);
pub static TAN: SqlOperator = function("TAN", SqlKind::OtherFunction);
pub static ASIN: SqlOperator = function("ASIN", SqlKind::OtherFunction);
pub static ACOS: SqlOperator = function("ACOS", SqlKind::OtherFunction);
pub static ATAN: SqlOperator = function("ATAN", SqlKind::OtherFunction);
pub static ATAN2: SqlOperator = function("ATAN2", SqlKind::OtherFunction);
pub static SIGN: SqlOperator = function("SIGN", SqlKind::OtherFunction);
pub static LOG10: SqlOperator = function("LOG10", SqlKind::OtherFunction);
pub static LN: SqlOperator = function("LN", SqlKind::OtherFunction);
pub static ROUND: SqlOperator = function("ROUND", SqlKind::OtherFunction);
pub static CEIL: SqlOperator = function("CEIL", SqlKind::Ceil);
pub static FLOOR: SqlOperator = function("FLOOR", SqlKind::Floor);
pub static COUNT: SqlOperator = agg_star("COUNT", SqlKind::Count);
pub static SUM: SqlOperator = agg("SUM", SqlKind::Sum);
pub static SUM0: SqlOperator = agg("$SUM0", SqlKind::Sum0);
pub static MIN: SqlOperator = agg("MIN", SqlKind::Min);
pub static MAX: SqlOperator = agg("MAX", SqlKind::Max);
pub static AVG: SqlOperator = agg("AVG", SqlKind::Avg);
pub static APPROX_COUNT_DISTINCT: SqlOperator =
    agg("APPROX_COUNT_DISTINCT", SqlKind::OtherFunction);
pub static BOOL_OR: SqlOperator = agg("BOOL_OR", SqlKind::OtherFunction);
pub static BOOL_AND: SqlOperator = agg("BOOL_AND", SqlKind::OtherFunction);
pub static BOOLOR_AGG: SqlOperator = agg("BOOLOR_AGG", SqlKind::OtherFunction);
pub static BOOLAND_AGG: SqlOperator = agg("BOOLAND_AGG", SqlKind::OtherFunction);
pub static ROW_NUMBER: SqlOperator = agg("ROW_NUMBER", SqlKind::RowNumber);
pub static RANK: SqlOperator = agg("RANK", SqlKind::Rank);
pub static DENSE_RANK: SqlOperator = agg("DENSE_RANK", SqlKind::DenseRank);
pub static PERCENT_RANK: SqlOperator = agg("PERCENT_RANK", SqlKind::PercentRank);
pub static CUME_DIST: SqlOperator = agg("CUME_DIST", SqlKind::CumeDist);
pub static NTILE: SqlOperator = agg("NTILE", SqlKind::Ntile);
pub static LAG: SqlOperator = agg("LAG", SqlKind::Lag);
pub static LEAD: SqlOperator = agg("LEAD", SqlKind::Lead);
pub static FIRST_VALUE: SqlOperator = agg("FIRST_VALUE", SqlKind::FirstValue);
pub static LAST_VALUE: SqlOperator = agg("LAST_VALUE", SqlKind::LastValue);
pub static NTH_VALUE: SqlOperator = agg("NTH_VALUE", SqlKind::NthValue);

impl SqlOperator {
    pub fn not(&self) -> Option<&'static SqlOperator> {
        match (self.kind, self.negated) {
            (SqlKind::In, _) => Some(&NOT_IN),
            (SqlKind::NotIn, _) => Some(&IN),
            (SqlKind::Like, false) => Some(&NOT_LIKE),
            (SqlKind::Like, true) => Some(&LIKE),
            (SqlKind::Between, false) => Some(&NOT_BETWEEN),
            (SqlKind::Between, true) => Some(&BETWEEN),
            _ => None,
        }
    }

    pub fn is_window_rank(&self) -> bool {
        matches!(
            self.kind,
            SqlKind::RowNumber
                | SqlKind::Rank
                | SqlKind::DenseRank
                | SqlKind::PercentRank
                | SqlKind::CumeDist
                | SqlKind::Ntile
        )
    }

    pub fn is_symmetrical(&self) -> bool {
        matches!(
            self.kind,
            SqlKind::Equals
                | SqlKind::NotEquals
                | SqlKind::IsDistinctFrom
                | SqlKind::IsNotDistinctFrom
        )
    }
}

pub fn of_comparison_kind(kind: SqlKind) -> Option<&'static SqlOperator> {
    match kind {
        SqlKind::Equals => Some(&EQUALS),
        SqlKind::NotEquals => Some(&NOT_EQUALS),
        SqlKind::GreaterThan => Some(&GREATER_THAN),
        SqlKind::GreaterThanOrEqual => Some(&GREATER_THAN_OR_EQUAL),
        SqlKind::LessThan => Some(&LESS_THAN),
        SqlKind::LessThanOrEqual => Some(&LESS_THAN_OR_EQUAL),
        SqlKind::IsNull => Some(&IS_NULL),
        SqlKind::IsNotNull => Some(&IS_NOT_NULL),
        SqlKind::IsTrue => Some(&IS_TRUE),
        SqlKind::IsNotTrue => Some(&IS_NOT_TRUE),
        SqlKind::IsFalse => Some(&IS_FALSE),
        SqlKind::IsNotFalse => Some(&IS_NOT_FALSE),
        SqlKind::IsDistinctFrom => Some(&IS_DISTINCT_FROM),
        SqlKind::IsNotDistinctFrom => Some(&IS_NOT_DISTINCT_FROM),
        _ => None,
    }
}

impl SqlOperator {
    pub fn allows_framing(&self) -> bool {
        !matches!(
            self.kind,
            SqlKind::RowNumber
                | SqlKind::Rank
                | SqlKind::DenseRank
                | SqlKind::PercentRank
                | SqlKind::CumeDist
                | SqlKind::Ntile
                | SqlKind::Lag
                | SqlKind::Lead
        )
    }
}
