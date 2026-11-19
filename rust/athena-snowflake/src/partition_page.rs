use std::fmt;

use athena_federation_sdk::domain::Split;

use crate::snowflake_constants;

const PRIMARY_MARKER: &str = snowflake_constants::PARTITION_PRIMARY_MARKER;
const LIMIT_MARKER: &str = snowflake_constants::PARTITION_LIMIT_MARKER;
const OFFSET_MARKER: &str = snowflake_constants::PARTITION_OFFSET_MARKER;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PartitionPage<'a> {
    pub(crate) primary_key: &'a str,
    pub(crate) limit: &'a str,
    pub(crate) offset: &'a str,
}

impl<'a> PartitionPage<'a> {
    pub(crate) fn from_split(split: &'a Split) -> Option<PartitionPage<'a>> {
        PartitionPage::parse(split.get_property(snowflake_constants::BLOCK_PARTITION_COLUMN_NAME)?)
    }

    fn parse(partition_val: &'a str) -> Option<PartitionPage<'a>> {
        let (Some(primary_index), Some(limit_marker), Some(offset_marker)) = (
            partition_val.find(PRIMARY_MARKER),
            partition_val.rfind(LIMIT_MARKER),
            partition_val.rfind(OFFSET_MARKER),
        ) else {
            return None;
        };
        let primary_start = primary_index + PRIMARY_MARKER.len();
        if primary_start > limit_marker || limit_marker > offset_marker {
            tracing::warn!("Malformed partition value: {partition_val}");
            return None;
        }
        let primary_key = &partition_val[primary_start..limit_marker];
        let limit = &partition_val[limit_marker + LIMIT_MARKER.len()..offset_marker];
        let offset = &partition_val[offset_marker + OFFSET_MARKER.len()..];
        if primary_key.is_empty() {
            return None;
        }
        if !is_ascii_digits(limit) || !is_ascii_digits(offset) {
            tracing::warn!(
                "Non-numeric limit/offset in partition value: limit={limit}, offset={offset}"
            );
            return None;
        }
        Some(PartitionPage {
            primary_key,
            limit,
            offset,
        })
    }

    pub(crate) fn limit_within(&self, constraint_limit: Option<u64>) -> String {
        let Some(constraint_limit) = constraint_limit else {
            return self.limit.to_string();
        };
        match self.limit.parse::<i64>() {
            Ok(limit) if !u64::try_from(limit).is_ok_and(|l| l >= constraint_limit) => {
                limit.to_string()
            }
            _ => constraint_limit.to_string(),
        }
    }
}

impl fmt::Display for PartitionPage<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{PRIMARY_MARKER}{}{LIMIT_MARKER}{}{OFFSET_MARKER}{}",
            snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
            self.primary_key,
            self.limit,
            self.offset
        )
    }
}

fn is_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}
