use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    AscNullsFirst,
    AscNullsLast,
    DescNullsFirst,
    DescNullsLast,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::AscNullsFirst,
        Direction::AscNullsLast,
        Direction::DescNullsFirst,
        Direction::DescNullsLast,
    ];

    pub fn is_ascending(&self) -> bool {
        matches!(self, Direction::AscNullsFirst | Direction::AscNullsLast)
    }

    pub fn is_nulls_first(&self) -> bool {
        matches!(self, Direction::AscNullsFirst | Direction::DescNullsFirst)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::AscNullsFirst => "ASC_NULLS_FIRST",
            Direction::AscNullsLast => "ASC_NULLS_LAST",
            Direction::DescNullsFirst => "DESC_NULLS_FIRST",
            Direction::DescNullsLast => "DESC_NULLS_LAST",
        }
    }

    pub fn from_name(name: &str) -> Option<Direction> {
        Direction::ALL.into_iter().find(|d| d.as_str() == name)
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            if self.is_ascending() { "ASC" } else { "DESC" },
            if self.is_nulls_first() {
                "NULLS FIRST"
            } else {
                "NULLS LAST"
            }
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

    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }
}

impl fmt::Display for OrderByField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OrderByField[columnName={}, direction={}]",
            self.column_name,
            self.direction.as_str()
        )
    }
}
