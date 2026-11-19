use std::fmt;

use super::PingResponse;
use crate::metadata::{
    GetDataSourceCapabilitiesResponse, GetSplitsResponse, GetTableLayoutResponse, GetTableResponse,
    ListSchemasResponse, ListTablesResponse,
};
use crate::records::{ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::udf::UserDefinedFunctionResponse;

#[derive(Debug, Clone, PartialEq)]
pub enum FederationResponse {
    Ping(PingResponse),
    ListSchemas(ListSchemasResponse),
    ListTables(ListTablesResponse),
    GetTable(GetTableResponse),
    GetTableLayout(GetTableLayoutResponse),
    GetSplits(GetSplitsResponse),
    ReadRecords(ReadRecordsResponse),
    RemoteReadRecords(RemoteReadRecordsResponse),
    UserDefinedFunction(UserDefinedFunctionResponse),
    GetDataSourceCapabilities(GetDataSourceCapabilitiesResponse),
}

impl FederationResponse {
    pub fn type_name(&self) -> &'static str {
        match self {
            FederationResponse::Ping(_) => "PingResponse",
            FederationResponse::ListSchemas(_) => "ListSchemasResponse",
            FederationResponse::ListTables(_) => "ListTablesResponse",
            FederationResponse::GetTable(_) => "GetTableResponse",
            FederationResponse::GetTableLayout(_) => "GetTableLayoutResponse",
            FederationResponse::GetSplits(_) => "GetSplitsResponse",
            FederationResponse::ReadRecords(_) => "ReadRecordsResponse",
            FederationResponse::RemoteReadRecords(_) => "RemoteReadRecordsResponse",
            FederationResponse::UserDefinedFunction(_) => "UserDefinedFunctionResponse",
            FederationResponse::GetDataSourceCapabilities(_) => "GetDataSourceCapabilitiesResponse",
        }
    }
}

macro_rules! from_response {
    ($variant:ident, $t:ty) => {
        impl From<$t> for FederationResponse {
            fn from(value: $t) -> Self {
                FederationResponse::$variant(value)
            }
        }
    };
}

from_response!(Ping, PingResponse);
from_response!(ListSchemas, ListSchemasResponse);
from_response!(ListTables, ListTablesResponse);
from_response!(GetTable, GetTableResponse);
from_response!(GetTableLayout, GetTableLayoutResponse);
from_response!(GetSplits, GetSplitsResponse);
from_response!(ReadRecords, ReadRecordsResponse);
from_response!(RemoteReadRecords, RemoteReadRecordsResponse);
from_response!(UserDefinedFunction, UserDefinedFunctionResponse);
from_response!(GetDataSourceCapabilities, GetDataSourceCapabilitiesResponse);

impl fmt::Display for FederationResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FederationResponse::Ping(r) => fmt::Display::fmt(r, f),
            FederationResponse::ListSchemas(r) => fmt::Display::fmt(r, f),
            FederationResponse::ListTables(r) => fmt::Display::fmt(r, f),
            FederationResponse::GetTable(r) => fmt::Display::fmt(r, f),
            FederationResponse::GetTableLayout(r) => fmt::Display::fmt(r, f),
            FederationResponse::GetSplits(r) => fmt::Display::fmt(r, f),
            FederationResponse::ReadRecords(r) => fmt::Display::fmt(r, f),
            FederationResponse::RemoteReadRecords(r) => fmt::Display::fmt(r, f),
            FederationResponse::UserDefinedFunction(r) => fmt::Display::fmt(r, f),
            FederationResponse::GetDataSourceCapabilities(r) => fmt::Display::fmt(r, f),
        }
    }
}
