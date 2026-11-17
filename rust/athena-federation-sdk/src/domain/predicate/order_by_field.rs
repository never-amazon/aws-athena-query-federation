use std::fmt;

#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    ASC_NULLS_FIRST,
    ASC_NULLS_LAST,
    DESC_NULLS_FIRST,
    DESC_NULLS_LAST,
}

impl Direction {
    pub fn is_ascending(&self) -> bool {
        matches!(self, Direction::ASC_NULLS_FIRST | Direction::ASC_NULLS_LAST)
    }

    pub fn is_nulls_first(&self) -> bool {
        matches!(self, Direction::ASC_NULLS_FIRST | Direction::DESC_NULLS_FIRST)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Direction::ASC_NULLS_FIRST => "ASC_NULLS_FIRST",
            Direction::ASC_NULLS_LAST => "ASC_NULLS_LAST",
            Direction::DESC_NULLS_FIRST => "DESC_NULLS_FIRST",
            Direction::DESC_NULLS_LAST => "DESC_NULLS_LAST",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            if self.is_ascending() { "ASC" } else { "DESC" },
            if self.is_nulls_first() { "NULLS FIRST" } else { "NULLS LAST" }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderByField {
    column_name: String,
    direction: Direction,
}

impl OrderByField {
    pub fn new(column_name: impl Into<String>, direction: Direction) -> OrderByField {
        OrderByField {
            column_name: column_name.into(),
            direction,
        }
    }

    pub fn get_column_name(&self) -> &str {
        &self.column_name
    }

    pub fn get_direction(&self) -> Direction {
        self.direction
    }
}

impl fmt::Display for OrderByField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OrderByField[columnName={}, direction={}]",
            self.column_name,
            self.direction.name()
        )
    }
}
