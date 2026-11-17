use std::fmt;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordRequestType {
    ReadRecords,
}

impl RecordRequestType {
    pub fn name(&self) -> &'static str {
        "READ_RECORDS"
    }

    pub fn values() -> &'static [RecordRequestType] {
        &[RecordRequestType::ReadRecords]
    }
}

impl fmt::Display for RecordRequestType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
