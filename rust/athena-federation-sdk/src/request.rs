use std::fmt;

use crate::connection::environment_constants;
use crate::lambda::LambdaContext;
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use crate::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::security::FederatedIdentity;
use crate::udf::{UserDefinedFunctionRequest, UserDefinedFunctionResponse};

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

    pub fn is_federated(&self) -> bool {
        self.identity()
            .and_then(|identity| {
                identity
                    .config_options()
                    .get(environment_constants::FAS_TOKEN)
            })
            .is_some_and(Option::is_some)
    }

    #[must_use]
    pub fn with_context(mut self, context: Option<LambdaContext>) -> FederationRequest {
        match &mut self {
            FederationRequest::ListSchemas(r) => r.set_context(context),
            FederationRequest::ListTables(r) => r.set_context(context),
            FederationRequest::GetTable(r) => r.set_context(context),
            FederationRequest::GetTableLayout(r) => r.set_context(context),
            FederationRequest::GetSplits(r) => r.set_context(context),
            FederationRequest::GetDataSourceCapabilities(r) => r.set_context(context),
            FederationRequest::Ping(_)
            | FederationRequest::ReadRecords(_)
            | FederationRequest::UserDefinedFunction(_) => {}
        }
        self
    }

    pub fn is_metadata_request(&self) -> bool {
        matches!(
            self,
            FederationRequest::ListSchemas(_)
                | FederationRequest::ListTables(_)
                | FederationRequest::GetTable(_)
                | FederationRequest::GetTableLayout(_)
                | FederationRequest::GetSplits(_)
                | FederationRequest::GetDataSourceCapabilities(_)
        )
    }

    pub fn is_record_request(&self) -> bool {
        matches!(self, FederationRequest::ReadRecords(_))
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

#[derive(Debug, Clone)]
pub struct PingRequest {
    identity: Option<FederatedIdentity>,
    catalog_name: String,
    query_id: String,
}

impl PingRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        catalog_name: impl Into<String>,
        query_id: impl Into<String>,
    ) -> PingRequest {
        PingRequest {
            identity,
            catalog_name: catalog_name.into(),
            query_id: query_id.into(),
        }
    }

    pub fn identity(&self) -> Option<&FederatedIdentity> {
        self.identity.as_ref()
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn query_id(&self) -> &str {
        &self.query_id
    }
}

impl PartialEq for PingRequest {
    fn eq(&self, other: &Self) -> bool {
        self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for PingRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PingRequest{{catalogName='{}', queryId='{}'}}",
            self.catalog_name, self.query_id
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResponse {
    catalog_name: String,
    query_id: String,
    source_type: Option<String>,
    capabilities: i32,
    ser_de_version: i32,
}

impl PingResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        query_id: impl Into<String>,
        source_type: Option<String>,
        capabilities: i32,
        ser_de_version: i32,
    ) -> PingResponse {
        PingResponse {
            catalog_name: catalog_name.into(),
            query_id: query_id.into(),
            source_type,
            capabilities,
            ser_de_version: if ser_de_version != 0 {
                ser_de_version
            } else {
                1
            },
        }
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn query_id(&self) -> &str {
        &self.query_id
    }

    pub fn source_type(&self) -> Option<&str> {
        self.source_type.as_deref()
    }

    pub fn capabilities(&self) -> i32 {
        self.capabilities
    }

    pub fn ser_de_version(&self) -> i32 {
        self.ser_de_version
    }
}

impl fmt::Display for PingResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PingRequest{{catalogName='{}', queryId='{}', sourceType='{}', capabilities='{}', serDeVersion='{}'}}",
            self.catalog_name,
            self.query_id,
            self.source_type.as_deref().unwrap_or("null"),
            self.capabilities,
            self.ser_de_version
        )
    }
}
