use std::fmt;

use super::metadata_response::impl_metadata_response;
use super::{opt_str, MetadataRequestType};
use crate::domain::split::Split;

#[derive(Debug, Clone)]
pub struct GetSplitsResponse {
    catalog_name: String,
    splits: Vec<Split>,
    continuation_token: Option<String>,
}

impl GetSplitsResponse {
    pub fn new<I>(catalog_name: impl Into<String>, splits: I, continuation_token: Option<String>) -> GetSplitsResponse
    where
        I: IntoIterator<Item = Split>,
    {
        let mut unique: Vec<Split> = Vec::new();
        for split in splits {
            if !unique.contains(&split) {
                unique.push(split);
            }
        }
        GetSplitsResponse {
            catalog_name: catalog_name.into(),
            splits: unique,
            continuation_token,
        }
    }

    pub fn of_split(catalog_name: impl Into<String>, split: Split) -> GetSplitsResponse {
        GetSplitsResponse::new(catalog_name, vec![split], None)
    }

    pub fn get_splits(&self) -> &[Split] {
        &self.splits
    }

    pub fn get_continuation_token(&self) -> Option<&str> {
        self.continuation_token.as_deref()
    }
}

impl_metadata_response!(GetSplitsResponse, MetadataRequestType::GetSplits);

impl PartialEq for GetSplitsResponse {
    fn eq(&self, other: &Self) -> bool {
        self.splits.len() == other.splits.len()
            && self.splits.iter().all(|s| other.splits.contains(s))
            && self.continuation_token == other.continuation_token
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetSplitsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GetSplitsResponse{{splitSize={}, continuationToken='{}'}}",
            self.splits.len(),
            opt_str(self.continuation_token.as_deref())
        )
    }
}
