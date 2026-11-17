use std::fmt;

use super::sql_type_name::SqlTypeName;

pub const PRECISION_NOT_SPECIFIED: i32 = -1;
pub const SCALE_NOT_SPECIFIED: i32 = i32::MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelDataTypeSystem {
    Default,
    Postgresql,
}

impl RelDataTypeSystem {
    pub fn max_precision(self, type_name: SqlTypeName) -> i32 {
        match type_name {
            SqlTypeName::Decimal => 19,
            SqlTypeName::Varchar if self == RelDataTypeSystem::Postgresql => 10 * 1024 * 1024,
            SqlTypeName::Varchar
            | SqlTypeName::Char
            | SqlTypeName::Varbinary
            | SqlTypeName::Binary => 65536,
            SqlTypeName::Time
            | SqlTypeName::TimeWithLocalTimeZone
            | SqlTypeName::Timestamp
            | SqlTypeName::TimestampWithLocalTimeZone => 3,
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => 10,
            other => self.default_precision(other),
        }
    }

    pub fn max_scale(self, type_name: SqlTypeName) -> i32 {
        match type_name {
            SqlTypeName::Decimal => 19,
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => 9,
            _ => SCALE_NOT_SPECIFIED,
        }
    }

    pub fn default_precision(self, type_name: SqlTypeName) -> i32 {
        match type_name {
            SqlTypeName::Char | SqlTypeName::Binary | SqlTypeName::Boolean => 1,
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => 2,
            SqlTypeName::Tinyint => 3,
            SqlTypeName::Smallint => 5,
            SqlTypeName::Integer => 10,
            SqlTypeName::Decimal | SqlTypeName::Bigint => 19,
            SqlTypeName::Real => 7,
            SqlTypeName::Float | SqlTypeName::Double => 15,
            SqlTypeName::Time
            | SqlTypeName::TimeWithLocalTimeZone
            | SqlTypeName::Date
            | SqlTypeName::Timestamp
            | SqlTypeName::TimestampWithLocalTimeZone => 0,
            _ => PRECISION_NOT_SPECIFIED,
        }
    }

    pub fn default_scale(self, type_name: SqlTypeName) -> i32 {
        match type_name {
            SqlTypeName::Decimal => 0,
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => 6,
            _ => SCALE_NOT_SPECIFIED,
        }
    }
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

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_index(&self) -> usize {
        self.index
    }

    pub fn get_type(&self) -> &RelDataType {
        &self.data_type
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelDataType {
    type_name: SqlTypeName,
    precision: i32,
    scale: i32,
    nullable: bool,
    fields: Vec<RelDataTypeField>,
    component: Option<Box<RelDataType>>,
    value: Option<Box<RelDataType>>,
}

impl RelDataType {
    pub fn basic(
        type_name: SqlTypeName,
        precision: i32,
        scale: i32,
        nullable: bool,
    ) -> RelDataType {
        RelDataType {
            type_name,
            precision,
            scale,
            nullable,
            fields: Vec::new(),
            component: None,
            value: None,
        }
    }

    pub fn create_sql_type(system: RelDataTypeSystem, type_name: SqlTypeName) -> RelDataType {
        if type_name.allows_prec() {
            return RelDataType::create_sql_type_with_precision(
                system,
                type_name,
                system.default_precision(type_name),
            );
        }
        RelDataType::basic(
            type_name,
            PRECISION_NOT_SPECIFIED,
            SCALE_NOT_SPECIFIED,
            false,
        )
    }

    pub fn create_sql_type_with_precision(
        system: RelDataTypeSystem,
        type_name: SqlTypeName,
        precision: i32,
    ) -> RelDataType {
        if type_name.allows_scale() {
            return RelDataType::create_sql_type_with_scale(
                system,
                type_name,
                precision,
                system.default_scale(type_name),
            );
        }
        let max = system.max_precision(type_name);
        let precision = if max >= 0 && precision > max {
            max
        } else {
            precision
        };
        RelDataType::basic(type_name, precision, SCALE_NOT_SPECIFIED, false)
    }

    pub fn create_sql_type_with_scale(
        system: RelDataTypeSystem,
        type_name: SqlTypeName,
        precision: i32,
        scale: i32,
    ) -> RelDataType {
        let mut precision = precision;
        if precision != PRECISION_NOT_SPECIFIED {
            let max = system.max_precision(type_name);
            if max >= 0 && precision > max {
                precision = max;
            }
        }
        RelDataType::basic(type_name, precision, scale, false)
    }

    pub fn interval(type_name: SqlTypeName, fractional_second_precision: i32) -> RelDataType {
        RelDataType::basic(
            type_name,
            PRECISION_NOT_SPECIFIED,
            fractional_second_precision,
            false,
        )
    }

    pub fn interval_qualifier(&self) -> String {
        match self.type_name {
            SqlTypeName::IntervalYearMonth => "YEAR TO MONTH".to_string(),
            _ if self.scale == PRECISION_NOT_SPECIFIED => "DAY TO SECOND".to_string(),
            _ => format!("DAY TO SECOND({})", self.scale),
        }
    }

    pub fn array(component: RelDataType, nullable: bool) -> RelDataType {
        RelDataType {
            type_name: SqlTypeName::Array,
            precision: PRECISION_NOT_SPECIFIED,
            scale: SCALE_NOT_SPECIFIED,
            nullable,
            fields: Vec::new(),
            component: Some(Box::new(component)),
            value: None,
        }
    }

    pub fn map(key: RelDataType, value: RelDataType, nullable: bool) -> RelDataType {
        RelDataType {
            type_name: SqlTypeName::Map,
            precision: PRECISION_NOT_SPECIFIED,
            scale: SCALE_NOT_SPECIFIED,
            nullable,
            fields: Vec::new(),
            component: Some(Box::new(key)),
            value: Some(Box::new(value)),
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
            precision: PRECISION_NOT_SPECIFIED,
            scale: SCALE_NOT_SPECIFIED,
            nullable,
            fields,
            component: None,
            value: None,
        }
    }

    #[must_use]
    pub fn with_nullability(&self, nullable: bool) -> RelDataType {
        let mut t = self.clone();
        t.nullable = nullable;
        t
    }

    pub fn get_sql_type_name(&self) -> SqlTypeName {
        self.type_name
    }

    pub fn get_precision(&self) -> i32 {
        self.precision
    }

    pub fn get_scale(&self) -> i32 {
        self.scale
    }

    pub fn is_nullable(&self) -> bool {
        self.nullable
    }

    pub fn is_struct(&self) -> bool {
        self.type_name == SqlTypeName::Row
    }

    pub fn get_field_list(&self) -> &[RelDataTypeField] {
        &self.fields
    }

    pub fn get_field_count(&self) -> usize {
        self.fields.len()
    }

    pub fn get_field_names(&self) -> Vec<String> {
        self.fields.iter().map(|f| f.name.clone()).collect()
    }

    pub fn get_component_type(&self) -> Option<&RelDataType> {
        if self.type_name == SqlTypeName::Array {
            self.component.as_deref()
        } else {
            None
        }
    }

    pub fn get_key_type(&self) -> Option<&RelDataType> {
        if self.type_name == SqlTypeName::Map {
            self.component.as_deref()
        } else {
            None
        }
    }

    pub fn get_value_type(&self) -> Option<&RelDataType> {
        self.value.as_deref()
    }

    pub fn get_field(
        &self,
        name: &str,
        case_sensitive: bool,
        elide_record: bool,
    ) -> Option<&RelDataTypeField> {
        for f in &self.fields {
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
            for f in &self.fields {
                if f.data_type.is_struct() {
                    if let Some(inner) = f.data_type.get_field(name, case_sensitive, elide_record) {
                        return Some(inner);
                    }
                }
            }
        }
        None
    }

    pub fn equals_sans_nullability(&self, other: &RelDataType) -> bool {
        self.with_nullability(false) == other.with_nullability(false)
    }

    pub fn get_full_type_string(&self) -> String {
        let mut s = self.type_string();
        if !self.nullable {
            s.push_str(" NOT NULL");
        }
        s
    }

    fn type_string(&self) -> String {
        match self.type_name {
            SqlTypeName::Array => format!(
                "{} ARRAY",
                self.component
                    .as_ref()
                    .map(|c| c.get_full_type_string())
                    .unwrap_or_default()
            ),
            SqlTypeName::Map => format!(
                "({}, {}) MAP",
                self.component
                    .as_ref()
                    .map(|c| c.get_full_type_string())
                    .unwrap_or_default(),
                self.value
                    .as_ref()
                    .map(|c| c.get_full_type_string())
                    .unwrap_or_default()
            ),
            SqlTypeName::Row => {
                let inner: Vec<String> = self
                    .fields
                    .iter()
                    .map(|f| format!("{} {}", f.data_type.get_full_type_string(), f.name))
                    .collect();
                format!("RecordType({})", inner.join(", "))
            }
            SqlTypeName::IntervalYearMonth | SqlTypeName::IntervalDaySecond => {
                format!("INTERVAL {}", self.interval_qualifier())
            }
            t if self.precision == PRECISION_NOT_SPECIFIED || !t.allows_prec() => {
                t.name().to_string()
            }
            t if self.scale != SCALE_NOT_SPECIFIED && t.allows_scale() => {
                format!("{}({}, {})", t.name(), self.precision, self.scale)
            }
            t => format!("{}({})", t.name(), self.precision),
        }
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
            self.fields
                .push((f.get_name().to_string(), f.get_type().clone()));
        }
        self
    }

    pub fn build(&self) -> RelDataType {
        RelDataType::row(self.fields.clone(), false)
    }
}
