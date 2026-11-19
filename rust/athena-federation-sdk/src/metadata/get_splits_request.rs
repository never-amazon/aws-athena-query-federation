use std::fmt;

use arrow::datatypes::SchemaRef;

use super::metadata_request::impl_metadata_request;
use super::{collections_equals, opt_str, MetadataRequestType};
use crate::data::Block;
use crate::domain::predicate::Constraints;
use crate::domain::table_name::TableName;
use crate::lambda::LambdaContext;
use crate::security::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct GetSplitsRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    table_name: TableName,
    partitions: Block,
    partition_cols: Vec<String>,
    constraints: Constraints,
    continuation_token: Option<String>,
    context: Option<LambdaContext>,
}

impl GetSplitsRequest {
    #[allow(clippy::too_many_arguments, reason = "mirrors the Java constructor")]
    pub fn new(
        identity: Option<FederatedIdentity>,
        query_id: Option<String>,
        catalog_name: impl Into<String>,
        table_name: TableName,
        partitions: Block,
        partition_cols: Vec<String>,
        constraints: Constraints,
        continuation_token: Option<String>,
    ) -> GetSplitsRequest {
        GetSplitsRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
            table_name,
            partitions,
            partition_cols,
            constraints,
            continuation_token,
            context: None,
        }
    }

    pub fn with_continuation_token(
        clone: &GetSplitsRequest,
        continuation_token: Option<String>,
    ) -> GetSplitsRequest {
        let mut copy = clone.clone();
        copy.continuation_token = continuation_token;
        copy
    }

    pub fn continuation_token(&self) -> Option<&str> {
        self.continuation_token.as_deref()
    }

    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn schema(&self) -> &SchemaRef {
        self.partitions.schema()
    }

    pub fn partition_cols(&self) -> &[String] {
        &self.partition_cols
    }

    pub fn partitions(&self) -> &Block {
        &self.partitions
    }

    pub fn constraints(&self) -> &Constraints {
        &self.constraints
    }

    pub fn has_continuation_token(&self) -> bool {
        self.continuation_token.is_some()
    }
}

impl_metadata_request!(GetSplitsRequest, MetadataRequestType::GetSplits);

impl PartialEq for GetSplitsRequest {
    fn eq(&self, other: &Self) -> bool {
        self.table_name == other.table_name
            && self.partitions == other.partitions
            && collections_equals(&self.partition_cols, &other.partition_cols)
            && self.continuation_token == other.continuation_token
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetSplitsRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GetSplitsRequest{{queryId={}, tableName={}, partitionCols=[{}], requestType={}, catalogName={}, partitions={}, constraints={}, continuationToken={}}}",
            opt_str(self.query_id.as_deref()),
            self.table_name,
            self.partition_cols.join(", "),
            self.request_type(),
            self.catalog_name,
            self.partitions,
            self.constraints,
            opt_str(self.continuation_token.as_deref())
        )
    }
}
