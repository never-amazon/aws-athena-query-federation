use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SqlTypeName {
    Boolean,
    Tinyint,
    Smallint,
    Integer,
    Bigint,
    Decimal,
    Float,
    Real,
    Double,
    Date,
    Time,
    TimeWithLocalTimeZone,
    Timestamp,
    TimestampWithLocalTimeZone,
    IntervalYearMonth,
    IntervalDaySecond,
    Char,
    Varchar,
    Binary,
    Varbinary,
    Null,
    Any,
    Symbol,
    Array,
    Map,
    Row,
    Sarg,
    Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlTypeFamily {
    Boolean,
    Numeric,
    Date,
    Time,
    Timestamp,
    IntervalYearMonth,
    IntervalDayTime,
    Character,
    Binary,
    Null,
    Any,
    Array,
    Map,
    Other,
}

impl SqlTypeName {
    pub fn name(self) -> &'static str {
        match self {
            SqlTypeName::Boolean => "BOOLEAN",
            SqlTypeName::Tinyint => "TINYINT",
            SqlTypeName::Smallint => "SMALLINT",
            SqlTypeName::Integer => "INTEGER",
            SqlTypeName::Bigint => "BIGINT",
            SqlTypeName::Decimal => "DECIMAL",
            SqlTypeName::Float => "FLOAT",
            SqlTypeName::Real => "REAL",
            SqlTypeName::Double => "DOUBLE",
            SqlTypeName::Date => "DATE",
            SqlTypeName::Time => "TIME",
            SqlTypeName::TimeWithLocalTimeZone => "TIME_WITH_LOCAL_TIME_ZONE",
            SqlTypeName::Timestamp => "TIMESTAMP",
            SqlTypeName::TimestampWithLocalTimeZone => "TIMESTAMP_WITH_LOCAL_TIME_ZONE",
            SqlTypeName::IntervalYearMonth => "INTERVAL_YEAR_MONTH",
            SqlTypeName::IntervalDaySecond => "INTERVAL_DAY_SECOND",
            SqlTypeName::Char => "CHAR",
            SqlTypeName::Varchar => "VARCHAR",
            SqlTypeName::Binary => "BINARY",
            SqlTypeName::Varbinary => "VARBINARY",
            SqlTypeName::Null => "NULL",
            SqlTypeName::Any => "ANY",
            SqlTypeName::Symbol => "SYMBOL",
            SqlTypeName::Array => "ARRAY",
            SqlTypeName::Map => "MAP",
            SqlTypeName::Row => "ROW",
            SqlTypeName::Sarg => "SARG",
            SqlTypeName::Uuid => "UUID",
        }
    }

    pub fn value_of(name: &str) -> Option<SqlTypeName> {
        ALL.iter().copied().find(|t| t.name() == name)
    }

    pub fn family(self) -> SqlTypeFamily {
        match self {
            SqlTypeName::Boolean => SqlTypeFamily::Boolean,
            SqlTypeName::Tinyint
            | SqlTypeName::Smallint
            | SqlTypeName::Integer
            | SqlTypeName::Bigint
            | SqlTypeName::Decimal
            | SqlTypeName::Float
            | SqlTypeName::Real
            | SqlTypeName::Double => SqlTypeFamily::Numeric,
            SqlTypeName::Date => SqlTypeFamily::Date,
            SqlTypeName::Time | SqlTypeName::TimeWithLocalTimeZone => SqlTypeFamily::Time,
            SqlTypeName::Timestamp | SqlTypeName::TimestampWithLocalTimeZone => {
                SqlTypeFamily::Timestamp
            }
            SqlTypeName::IntervalYearMonth => SqlTypeFamily::IntervalYearMonth,
            SqlTypeName::IntervalDaySecond => SqlTypeFamily::IntervalDayTime,
            SqlTypeName::Char | SqlTypeName::Varchar => SqlTypeFamily::Character,
            SqlTypeName::Binary | SqlTypeName::Varbinary => SqlTypeFamily::Binary,
            SqlTypeName::Null => SqlTypeFamily::Null,
            SqlTypeName::Any => SqlTypeFamily::Any,
            SqlTypeName::Array => SqlTypeFamily::Array,
            SqlTypeName::Map => SqlTypeFamily::Map,
            SqlTypeName::Symbol | SqlTypeName::Row | SqlTypeName::Sarg | SqlTypeName::Uuid => {
                SqlTypeFamily::Other
            }
        }
    }

    pub fn allows_prec(self) -> bool {
        matches!(
            self,
            SqlTypeName::Decimal
                | SqlTypeName::Time
                | SqlTypeName::TimeWithLocalTimeZone
                | SqlTypeName::Timestamp
                | SqlTypeName::TimestampWithLocalTimeZone
                | SqlTypeName::IntervalDaySecond
                | SqlTypeName::Char
                | SqlTypeName::Varchar
                | SqlTypeName::Binary
                | SqlTypeName::Varbinary
                | SqlTypeName::Any
        )
    }

    pub fn allows_scale(self) -> bool {
        matches!(
            self,
            SqlTypeName::Decimal | SqlTypeName::IntervalDaySecond | SqlTypeName::Any
        )
    }

    pub fn is_approx(self) -> bool {
        matches!(
            self,
            SqlTypeName::Float | SqlTypeName::Real | SqlTypeName::Double
        )
    }

    pub fn is_exact_numeric(self) -> bool {
        matches!(
            self,
            SqlTypeName::Tinyint
                | SqlTypeName::Smallint
                | SqlTypeName::Integer
                | SqlTypeName::Bigint
                | SqlTypeName::Decimal
        )
    }

    pub fn is_numeric(self) -> bool {
        self.family() == SqlTypeFamily::Numeric
    }

    pub fn is_character(self) -> bool {
        matches!(self, SqlTypeName::Char | SqlTypeName::Varchar)
    }
}

const ALL: [SqlTypeName; 28] = [
    SqlTypeName::Boolean,
    SqlTypeName::Tinyint,
    SqlTypeName::Smallint,
    SqlTypeName::Integer,
    SqlTypeName::Bigint,
    SqlTypeName::Decimal,
    SqlTypeName::Float,
    SqlTypeName::Real,
    SqlTypeName::Double,
    SqlTypeName::Date,
    SqlTypeName::Time,
    SqlTypeName::TimeWithLocalTimeZone,
    SqlTypeName::Timestamp,
    SqlTypeName::TimestampWithLocalTimeZone,
    SqlTypeName::IntervalYearMonth,
    SqlTypeName::IntervalDaySecond,
    SqlTypeName::Char,
    SqlTypeName::Varchar,
    SqlTypeName::Binary,
    SqlTypeName::Varbinary,
    SqlTypeName::Null,
    SqlTypeName::Any,
    SqlTypeName::Symbol,
    SqlTypeName::Array,
    SqlTypeName::Map,
    SqlTypeName::Row,
    SqlTypeName::Sarg,
    SqlTypeName::Uuid,
];

impl fmt::Display for SqlTypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
