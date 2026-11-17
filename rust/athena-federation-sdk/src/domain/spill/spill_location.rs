use std::fmt;

use super::s3_spill_location::S3SpillLocation;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpillLocation {
    S3(S3SpillLocation),
}

impl SpillLocation {
    pub fn type_name(&self) -> &'static str {
        match self {
            SpillLocation::S3(_) => "S3SpillLocation",
        }
    }

    pub fn as_s3(&self) -> Option<&S3SpillLocation> {
        match self {
            SpillLocation::S3(s) => Some(s),
        }
    }
}

impl From<S3SpillLocation> for SpillLocation {
    fn from(s: S3SpillLocation) -> Self {
        SpillLocation::S3(s)
    }
}

impl fmt::Display for SpillLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpillLocation::S3(s) => write!(f, "{s}"),
        }
    }
}
