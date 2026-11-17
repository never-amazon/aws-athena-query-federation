use std::fmt;

use super::{FederatedIdentity, PingRequest};
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetSplitsRequest, GetTableLayoutRequest, GetTableRequest,
    ListSchemasRequest, ListTablesRequest, MetadataRequest,
};
use crate::records::{ReadRecordsRequest, RecordRequest};
use crate::udf::UserDefinedFunctionRequest;

#[derive(Debug, Clone, PartialEq)]
pub enum FederationRequest {
    Ping(PingRequest),
    ListSchemas(ListSchemasRequest),
    ListTables(ListTablesRequest),
    GetTable(GetTableRequest),
    GetTableLayout(GetTableLayoutRequest),
    GetSplits(GetSplitsRequest),
    ReadRecords(ReadRecordsRequest),
    UserDefinedFunction(UserDefinedFunctionRequest),
    GetDataSourceCapabilities(GetDataSourceCapabilitiesRequest),
}

impl FederationRequest {
    pub fn type_name(&self) -> &'static str {
        match self {
            FederationRequest::Ping(_) => "PingRequest",
            FederationRequest::ListSchemas(_) => "ListSchemasRequest",
            FederationRequest::ListTables(_) => "ListTablesRequest",
            FederationRequest::GetTable(_) => "GetTableRequest",
            FederationRequest::GetTableLayout(_) => "GetTableLayoutRequest",
            FederationRequest::GetSplits(_) => "GetSplitsRequest",
            FederationRequest::ReadRecords(_) => "ReadRecordsRequest",
            FederationRequest::UserDefinedFunction(_) => "UserDefinedFunctionRequest",
            FederationRequest::GetDataSourceCapabilities(_) => "GetDataSourceCapabilitiesRequest",
        }
    }

    pub fn identity(&self) -> Option<&FederatedIdentity> {
        match self {
            FederationRequest::Ping(r) => r.identity(),
            FederationRequest::ListSchemas(r) => r.identity(),
            FederationRequest::ListTables(r) => r.identity(),
            FederationRequest::GetTable(r) => r.identity(),
            FederationRequest::GetTableLayout(r) => r.identity(),
            FederationRequest::GetSplits(r) => r.identity(),
            FederationRequest::ReadRecords(r) => r.identity(),
            FederationRequest::UserDefinedFunction(r) => r.identity(),
            FederationRequest::GetDataSourceCapabilities(r) => r.identity(),
        }
    }

    pub fn as_metadata_request(&self) -> Option<&dyn MetadataRequest> {
        match self {
            FederationRequest::ListSchemas(r) => Some(r),
            FederationRequest::ListTables(r) => Some(r),
            FederationRequest::GetTable(r) => Some(r),
            FederationRequest::GetTableLayout(r) => Some(r),
            FederationRequest::GetSplits(r) => Some(r),
            FederationRequest::GetDataSourceCapabilities(r) => Some(r),
            _ => None,
        }
    }

    pub fn as_record_request(&self) -> Option<&dyn RecordRequest> {
        match self {
            FederationRequest::ReadRecords(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_metadata_request(&self) -> bool {
        self.as_metadata_request().is_some()
    }

    pub fn is_record_request(&self) -> bool {
        self.as_record_request().is_some()
    }
}

macro_rules! from_request {
    ($variant:ident, $t:ty) => {
        impl From<$t> for FederationRequest {
            fn from(value: $t) -> Self {
                FederationRequest::$variant(value)
            }
        }
    };
}

from_request!(Ping, PingRequest);
from_request!(ListSchemas, ListSchemasRequest);
from_request!(ListTables, ListTablesRequest);
from_request!(GetTable, GetTableRequest);
from_request!(GetTableLayout, GetTableLayoutRequest);
from_request!(GetSplits, GetSplitsRequest);
from_request!(ReadRecords, ReadRecordsRequest);
from_request!(UserDefinedFunction, UserDefinedFunctionRequest);
from_request!(GetDataSourceCapabilities, GetDataSourceCapabilitiesRequest);

impl fmt::Display for FederationRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FederationRequest::Ping(r) => fmt::Display::fmt(r, f),
            FederationRequest::ListSchemas(r) => fmt::Display::fmt(r, f),
            FederationRequest::ListTables(r) => fmt::Display::fmt(r, f),
            FederationRequest::GetTable(r) => fmt::Display::fmt(r, f),
            FederationRequest::GetTableLayout(r) => fmt::Display::fmt(r, f),
            FederationRequest::GetSplits(r) => fmt::Display::fmt(r, f),
            FederationRequest::ReadRecords(r) => fmt::Display::fmt(r, f),
            FederationRequest::UserDefinedFunction(r) => fmt::Display::fmt(r, f),
            FederationRequest::GetDataSourceCapabilities(r) => fmt::Display::fmt(r, f),
        }
    }
}
