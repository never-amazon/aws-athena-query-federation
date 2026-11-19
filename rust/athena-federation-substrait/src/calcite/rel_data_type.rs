use std::fmt;

use super::sql_type_name::SqlTypeName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelDataTypeSystem {
    Default,
    Postgresql,
}

impl RelDataTypeSystem {
    pub fn max_precision(self, type_name: SqlTypeName) -> Option<i32> {
        match type_name {
            SqlTypeName::Decimal => Some(19),
            SqlTypeName::Varchar if self == RelDataTypeSystem::Postgresql => Some(10 * 1024 * 1024),
            SqlTypeName::Varchar
            | SqlTypeName::Char
            | SqlTypeName::Varbinary
            | SqlTypeName::Binary => Some(65536),
            SqlTypeName::Time
            | SqlTypeName::TimeWithLocalTimeZone
            | SqlTypeName::Timestamp
            | SqlTypeName::TimestampWithLocalTimeZone => Some(3),
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => Some(10),
            other => self.default_precision(other),
        }
    }

    pub fn max_scale(self, type_name: SqlTypeName) -> Option<i32> {
        match type_name {
            SqlTypeName::Decimal => Some(19),
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => Some(9),
            _ => None,
        }
    }

    pub fn default_precision(self, type_name: SqlTypeName) -> Option<i32> {
        match type_name {
            SqlTypeName::Char | SqlTypeName::Binary | SqlTypeName::Boolean => Some(1),
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => Some(2),
            SqlTypeName::Tinyint => Some(3),
            SqlTypeName::Smallint => Some(5),
            SqlTypeName::Integer => Some(10),
            SqlTypeName::Decimal | SqlTypeName::Bigint => Some(19),
            SqlTypeName::Real => Some(7),
            SqlTypeName::Float | SqlTypeName::Double => Some(15),
            SqlTypeName::Time
            | SqlTypeName::TimeWithLocalTimeZone
            | SqlTypeName::Date
            | SqlTypeName::Timestamp
            | SqlTypeName::TimestampWithLocalTimeZone => Some(0),
            _ => None,
        }
    }

    pub fn default_scale(self, type_name: SqlTypeName) -> Option<i32> {
        match type_name {
            SqlTypeName::Decimal => Some(0),
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => Some(6),
            _ => None,
        }
    }
}

fn specified_precision(precision: i32) -> Option<i32> {
    (precision != -1).then_some(precision)
}

fn specified_scale(scale: i32) -> Option<i32> {
    (scale != i32::MIN).then_some(scale)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelDataTypeField {
    name: String,
    index: usize,
    data_type: RelDataType,
}

impl RelDataTypeField {
    pub fn new(name: impl Into<String>, index: usize, data_type: RelDataType) -> RelDataTypeField {
        RelDataTypeField {
            name: name.into(),
            index,
            data_type,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn ty(&self) -> &RelDataType {
        &self.data_type
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Scalar {
        precision: Option<i32>,
        scale: Option<i32>,
    },
    Interval {
        fractional_second_precision: Option<i32>,
    },
    Array(Box<RelDataType>),
    Map(Box<RelDataType>, Box<RelDataType>),
    Row(Vec<RelDataTypeField>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelDataType {
    type_name: SqlTypeName,
    nullable: bool,
    shape: Shape,
}

impl RelDataType {
    pub fn basic(
        type_name: SqlTypeName,
        precision: Option<i32>,
        scale: Option<i32>,
        nullable: bool,
    ) -> RelDataType {
        RelDataType {
            type_name,
            nullable,
            shape: Shape::Scalar { precision, scale },
        }
    }

    fn sql_type(
        system: RelDataTypeSystem,
        type_name: SqlTypeName,
        precision: Option<i32>,
    ) -> RelDataType {
        if type_name.allows_scale() {
            return RelDataType::sql_type_with_scale(
                system,
                type_name,
                precision,
                system.default_scale(type_name),
            );
        }
        RelDataType::basic(
            type_name,
            precision.map(|p| clamp_precision(system, type_name, p)),
            None,
            false,
        )
    }

    fn sql_type_with_scale(
        system: RelDataTypeSystem,
        type_name: SqlTypeName,
        precision: Option<i32>,
        scale: Option<i32>,
    ) -> RelDataType {
        RelDataType::basic(
            type_name,
            precision.map(|p| clamp_precision(system, type_name, p)),
            scale,
            false,
        )
    }

    pub fn create_sql_type(system: RelDataTypeSystem, type_name: SqlTypeName) -> RelDataType {
        if type_name.allows_prec() {
            return RelDataType::sql_type(system, type_name, system.default_precision(type_name));
        }
        RelDataType::basic(type_name, None, None, false)
    }

    pub fn create_sql_type_with_precision(
        system: RelDataTypeSystem,
        type_name: SqlTypeName,
        precision: i32,
    ) -> RelDataType {
        RelDataType::sql_type(system, type_name, specified_precision(precision))
    }

    pub fn create_sql_type_with_scale(
        system: RelDataTypeSystem,
        type_name: SqlTypeName,
        precision: i32,
        scale: i32,
    ) -> RelDataType {
        RelDataType::sql_type_with_scale(
            system,
            type_name,
            specified_precision(precision),
            specified_scale(scale),
        )
    }

    pub fn interval_year_month() -> RelDataType {
        RelDataType {
            type_name: SqlTypeName::IntervalYearMonth,
            nullable: false,
            shape: Shape::Interval {
                fractional_second_precision: None,
            },
        }
    }

    pub fn interval_day_second(fractional_second_precision: i32) -> RelDataType {
        RelDataType {
            type_name: SqlTypeName::IntervalDaySecond,
            nullable: false,
            shape: Shape::Interval {
                fractional_second_precision: specified_precision(fractional_second_precision),
            },
        }
    }

    pub fn interval_qualifier(&self) -> String {
        match (self.type_name, &self.shape) {
            (SqlTypeName::IntervalYearMonth, _) => "YEAR TO MONTH".to_string(),
            (
                _,
                Shape::Interval {
                    fractional_second_precision: Some(p),
                },
            ) => format!("DAY TO SECOND({p})"),
            _ => "DAY TO SECOND".to_string(),
        }
    }

    pub fn array(component: RelDataType, nullable: bool) -> RelDataType {
        RelDataType {
            type_name: SqlTypeName::Array,
            nullable,
            shape: Shape::Array(Box::new(component)),
        }
    }

    pub fn map(key: RelDataType, value: RelDataType, nullable: bool) -> RelDataType {
        RelDataType {
            type_name: SqlTypeName::Map,
            nullable,
            shape: Shape::Map(Box::new(key), Box::new(value)),
        }
    }

    pub fn row(fields: Vec<(String, RelDataType)>, nullable: bool) -> RelDataType {
        let fields = fields
            .into_iter()
            .enumerate()
            .map(|(i, (n, t))| RelDataTypeField::new(n, i, t))
            .collect();
        RelDataType {
            type_name: SqlTypeName::Row,
            nullable,
            shape: Shape::Row(fields),
        }
    }

    #[must_use]
    pub fn with_nullability(&self, nullable: bool) -> RelDataType {
        RelDataType {
            nullable,
            ..self.clone()
        }
    }

    pub fn sql_type_name(&self) -> SqlTypeName {
        self.type_name
    }

    pub fn precision(&self) -> Option<i32> {
        match self.shape {
            Shape::Scalar { precision, .. } => precision,
            _ => None,
        }
    }

    pub fn scale(&self) -> Option<i32> {
        match self.shape {
            Shape::Scalar { scale, .. } => scale,
            _ => None,
        }
    }

    pub fn precision_exceeds(&self, other: &RelDataType) -> bool {
        match (self.precision(), other.precision()) {
            (Some(a), Some(b)) => a > b,
            (Some(a), None) => a >= 0,
            (None, Some(b)) => b < -1,
            (None, None) => false,
        }
    }

    pub fn is_nullable(&self) -> bool {
        self.nullable
    }

    pub fn is_struct(&self) -> bool {
        self.type_name == SqlTypeName::Row
    }

    pub fn field_list(&self) -> &[RelDataTypeField] {
        match &self.shape {
            Shape::Row(fields) => fields,
            _ => &[],
        }
    }

    pub fn field_count(&self) -> usize {
        self.field_list().len()
    }

    pub fn field_names(&self) -> Vec<String> {
        self.field_list().iter().map(|f| f.name.clone()).collect()
    }

    pub fn component_type(&self) -> Option<&RelDataType> {
        match &self.shape {
            Shape::Array(component) => Some(component),
            _ => None,
        }
    }

    pub fn key_type(&self) -> Option<&RelDataType> {
        match &self.shape {
            Shape::Map(key, _) => Some(key),
            _ => None,
        }
    }

    pub fn value_type(&self) -> Option<&RelDataType> {
        match &self.shape {
            Shape::Map(_, value) => Some(value),
            _ => None,
        }
    }

    pub fn field(
        &self,
        name: &str,
        case_sensitive: bool,
        elide_record: bool,
    ) -> Option<&RelDataTypeField> {
        let fields = self.field_list();
        for f in fields {
            let matches = if case_sensitive {
                f.name == name
            } else {
                f.name.eq_ignore_ascii_case(name)
            };
            if matches {
                return Some(f);
            }
        }
        if elide_record {
            for f in fields {
                if f.data_type.is_struct() {
                    if let Some(inner) = f.data_type.field(name, case_sensitive, elide_record) {
                        return Some(inner);
                    }
                }
            }
        }
        None
    }

    pub fn equals_sans_nullability(&self, other: &RelDataType) -> bool {
        self.type_name == other.type_name && self.shape == other.shape
    }

    pub fn full_type_string(&self) -> String {
        let mut s = self.type_string();
        if self.type_name.is_character() {
            s.push_str(" CHARACTER SET \"UTF-8\"");
        }
        if !self.nullable {
            s.push_str(" NOT NULL");
        }
        s
    }

    fn type_string(&self) -> String {
        match (&self.shape, self.type_name) {
            (_, SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond) => {
                format!("INTERVAL {}", self.interval_qualifier())
            }
            (Shape::Array(component), _) => format!("{} ARRAY", component.full_type_string()),
            (Shape::Map(key, value), _) => format!(
                "({}, {}) MAP",
                key.full_type_string(),
                value.full_type_string()
            ),
            (Shape::Row(fields), _) => {
                let inner: Vec<String> = fields
                    .iter()
                    .map(|f| format!("{} {}", f.data_type.full_type_string(), f.name))
                    .collect();
                format!("RecordType({})", inner.join(", "))
            }
            (
                Shape::Scalar {
                    precision: Some(p),
                    scale,
                },
                t,
            ) if t.allows_prec() => match scale {
                Some(s) if t.allows_scale() => format!("{}({p}, {s})", t.name()),
                _ => format!("{}({p})", t.name()),
            },
            (_, t) => t.name().to_string(),
        }
    }
}

fn clamp_precision(system: RelDataTypeSystem, type_name: SqlTypeName, precision: i32) -> i32 {
    match system.max_precision(type_name) {
        Some(max) if max >= 0 && precision > max => max,
        _ => precision,
    }
}

impl fmt::Display for RelDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.type_string())
    }
}

#[derive(Debug, Default)]
pub struct RelDataTypeBuilder {
    fields: Vec<(String, RelDataType)>,
}

impl RelDataTypeBuilder {
    pub fn new() -> RelDataTypeBuilder {
        RelDataTypeBuilder { fields: Vec::new() }
    }

    pub fn add(&mut self, name: impl Into<String>, data_type: RelDataType) -> &mut Self {
        self.fields.push((name.into(), data_type));
        self
    }

    pub fn add_all(&mut self, fields: &[RelDataTypeField]) -> &mut Self {
        for f in fields {
            self.fields.push((f.name().to_string(), f.ty().clone()));
        }
        self
    }

    pub fn build(&self) -> RelDataType {
        RelDataType::row(self.fields.clone(), false)
    }
}
