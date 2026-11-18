use std::any::{type_name, Any};
use std::io::Write;
use std::sync::Arc;

use arrow::datatypes::Schema;

use crate::data::{Block, BlockAllocator};
use crate::domain::predicate::Marker;
use crate::exceptions::{FederationError, Result};
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use crate::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::request::{FederationRequest, FederationResponse, PingRequest, PingResponse};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::v2::lambda_function_exception_serde::{self, LambdaException};
use crate::serde::{v1, v2, SerDeContext, SerdeVersion};
use crate::udf::{UserDefinedFunctionRequest, UserDefinedFunctionResponse};

#[derive(Clone)]
pub struct ObjectMapper {
    version: SerdeVersion,
    allocator: Arc<dyn BlockAllocator>,
    indent_output: bool,
}

enum Writable {
    Request(Box<FederationRequest>),
    Response(Box<FederationResponse>),
    Block(Block),
    Schema(Schema),
    Marker(Marker),
}

fn to_request(value: &dyn Any) -> Option<FederationRequest> {
    if let Some(r) = value.downcast_ref::<FederationRequest>() {
        return Some(r.clone());
    }
    macro_rules! try_request {
        ($t:ty) => {
            if let Some(r) = value.downcast_ref::<$t>() {
                return Some(FederationRequest::from(r.clone()));
            }
        };
    }
    try_request!(PingRequest);
    try_request!(ListSchemasRequest);
    try_request!(ListTablesRequest);
    try_request!(GetTableRequest);
    try_request!(GetTableLayoutRequest);
    try_request!(GetSplitsRequest);
    try_request!(ReadRecordsRequest);
    try_request!(UserDefinedFunctionRequest);
    try_request!(GetDataSourceCapabilitiesRequest);
    None
}

fn to_response(value: &dyn Any) -> Option<FederationResponse> {
    if let Some(r) = value.downcast_ref::<FederationResponse>() {
        return Some(r.clone());
    }
    macro_rules! try_response {
        ($t:ty) => {
            if let Some(r) = value.downcast_ref::<$t>() {
                return Some(FederationResponse::from(r.clone()));
            }
        };
    }
    try_response!(PingResponse);
    try_response!(ListSchemasResponse);
    try_response!(ListTablesResponse);
    try_response!(GetTableResponse);
    try_response!(GetTableLayoutResponse);
    try_response!(GetSplitsResponse);
    try_response!(ReadRecordsResponse);
    try_response!(RemoteReadRecordsResponse);
    try_response!(UserDefinedFunctionResponse);
    try_response!(GetDataSourceCapabilitiesResponse);
    None
}

fn simple_type(name: &str) -> String {
    format!("[simple type, class {name}]")
}

fn wrap_as_io(e: FederationError) -> FederationError {
    match e {
        FederationError::Io(_) => e,
        other => FederationError::Io(other.to_string()),
    }
}

impl ObjectMapper {
    pub fn new(version: SerdeVersion, allocator: Arc<dyn BlockAllocator>) -> ObjectMapper {
        tracing::debug!("Athena SDK SerDe Version: {}", version);
        ObjectMapper {
            version,
            allocator,
            indent_output: false,
        }
    }

    #[must_use]
    pub fn indented(mut self) -> ObjectMapper {
        self.indent_output = true;
        self
    }

    pub fn version(&self) -> SerdeVersion {
        self.version
    }

    pub fn read_request_with_version_fallback(
        allocator: &Arc<dyn BlockAllocator>,
        input: &[u8],
    ) -> Result<(ObjectMapper, FederationRequest)> {
        let mut candidate = Some(SerdeVersion::CURRENT);
        while let Some(version) = candidate {
            let mapper = ObjectMapper::new(version, Arc::clone(allocator));
            match mapper.read_request(input) {
                Ok(request) => {
                    tracing::info!(
                        "Parsing request with resolvedSerDeVersion: '{}', connector SerDeVersion: '{}'",
                        version,
                        SerdeVersion::CURRENT
                    );
                    return Ok((mapper, request));
                }
                Err(FederationError::IllegalState(_)) => {
                    candidate = version.lower();
                    tracing::debug!(
                        "Client's SerDe mis-matched with connector version:, attempt with lower version: '{}'",
                        version.as_i32() - 1
                    );
                }
                Err(e) => return Err(e),
            }
        }
        Err(FederationError::invalid_input(
            "FederationRequest/ObjectMapper is null with SerDeVersion: '0'",
        ))
    }

    pub fn context(&self) -> SerDeContext<'_> {
        SerDeContext::new(self.version, self.allocator.as_ref())
    }

    fn resolve_writable<T: Any>(&self, value: &T) -> Result<Writable> {
        let any: &dyn Any = value;
        if let Some(r) = to_request(any) {
            return Ok(Writable::Request(Box::new(r)));
        }
        if let Some(r) = to_response(any) {
            return Ok(Writable::Response(Box::new(r)));
        }
        if !self.version.is_strict() {
            if let Some(b) = any.downcast_ref::<Block>() {
                return Ok(Writable::Block(b.clone()));
            }
            if let Some(s) = any.downcast_ref::<Schema>() {
                return Ok(Writable::Schema(s.clone()));
            }
            if let Some(m) = any.downcast_ref::<Marker>() {
                return Ok(Writable::Marker(m.clone()));
            }
        }
        Err(FederationError::invalid_input(format!(
            "No explicitly configured serializer for {}",
            simple_type(type_name::<T>())
        )))
    }

    fn write_writable(&self, writable: &Writable, jgen: &mut JsonGenerator) -> Result<()> {
        if self.version.is_strict() {
            match writable {
                Writable::Request(r) => {
                    v2::federation_request_serde::serialize(r, jgen, self.version)
                }
                Writable::Response(r) => {
                    v2::federation_response_serde::serialize(r, jgen, self.version)
                }
                _ => Err(FederationError::invalid_input(
                    "No explicitly configured serializer",
                )),
            }
        } else {
            match writable {
                Writable::Request(r) => v1::bean_serde::serialize_request(r, jgen),
                Writable::Response(r) => v1::bean_serde::serialize_response(r, jgen),
                Writable::Block(b) => v1::bean_serde::serialize_block(Some(b), jgen),
                Writable::Schema(s) => v1::bean_serde::serialize_schema(s, jgen),
                Writable::Marker(m) => v1::bean_serde::serialize_marker(m, jgen),
            }
        }
    }

    pub fn write_value_as_string<T: Any>(&self, value: &T) -> Result<String> {
        let writable = self.resolve_writable(value)?;
        let mut jgen = JsonGenerator::new(self.indent_output);
        self.write_writable(&writable, &mut jgen)
            .map_err(wrap_as_io)?;
        Ok(jgen.into_string())
    }

    pub fn write_value_as_bytes<T: Any>(&self, value: &T) -> Result<Vec<u8>> {
        Ok(self.write_value_as_string(value)?.into_bytes())
    }

    pub fn write_value<T: Any>(&self, out: &mut dyn Write, value: &T) -> Result<()> {
        let text = self.write_value_as_string(value)?;
        out.write_all(text.as_bytes())?;
        Ok(())
    }

    pub fn write_request(&self, request: &FederationRequest) -> Result<String> {
        self.write_value_as_string(request)
    }

    pub fn write_response(&self, response: &FederationResponse) -> Result<String> {
        self.write_value_as_string(response)
    }

    fn init_for_reading<'a>(input: &'a [u8], type_label: &str) -> Result<JsonParser<'a>> {
        let mut jparser = JsonParser::new(input);
        match jparser.next_token()? {
            None => Err(FederationError::Io(format!(
                "No content to map due to end-of-input for {type_label}"
            ))),
            Some(JsonToken::ValueNull) => Err(FederationError::null_pointer(format!(
                "readValue returned null for {type_label}"
            ))),
            Some(_) => Ok(jparser),
        }
    }

    pub fn read_request(&self, input: &[u8]) -> Result<FederationRequest> {
        let mut jparser = Self::init_for_reading(input, "FederationRequest")?;
        let ctx = self.context();
        if self.version.is_strict() {
            v2::federation_request_serde::do_deserialize(&mut jparser, &ctx)
        } else {
            v1::bean_serde::deserialize_request(&mut jparser, &ctx)
        }
    }

    pub fn read_response(&self, input: &[u8]) -> Result<FederationResponse> {
        let mut jparser = Self::init_for_reading(input, "FederationResponse")?;
        let ctx = self.context();
        if self.version.is_strict() {
            v2::federation_response_serde::do_deserialize(&mut jparser, &ctx)
        } else {
            v1::bean_serde::deserialize_response(&mut jparser, &ctx)
        }
    }

    pub fn read_lambda_exception(&self, input: &[u8]) -> Result<LambdaException> {
        if !self.version.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "Cannot construct instance of {}",
                simple_type(type_name::<LambdaException>())
            )));
        }
        let mut jparser = Self::init_for_reading(input, "LambdaException")?;
        lambda_function_exception_serde::deserialize(&mut jparser)
    }

    pub fn read_block(&self, input: &[u8]) -> Result<Block> {
        if self.version.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<Block>())
            )));
        }
        let mut jparser = Self::init_for_reading(input, "Block")?;
        v1::bean_serde::deserialize_block_tree(&mut jparser, &self.context())
    }

    pub fn read_schema(&self, input: &[u8]) -> Result<Schema> {
        if self.version.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<Schema>())
            )));
        }
        let mut jparser = Self::init_for_reading(input, "Schema")?;
        v1::bean_serde::deserialize_schema_tree(&mut jparser)
    }

    pub fn read_marker(&self, input: &[u8]) -> Result<Marker> {
        if self.version.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<Marker>())
            )));
        }
        let mut jparser = Self::init_for_reading(input, "Marker")?;
        let tree = jparser.read_tree()?;
        v1::bean_serde::marker_from_tree(&tree, &self.context())
    }

    pub fn read_value<T: Any>(&self, input: &[u8]) -> Result<T> {
        let wanted = std::any::TypeId::of::<T>();
        let boxed: Box<dyn Any> = if wanted == std::any::TypeId::of::<FederationRequest>() {
            Box::new(self.read_request(input)?)
        } else if wanted == std::any::TypeId::of::<FederationResponse>() {
            Box::new(self.read_response(input)?)
        } else if wanted == std::any::TypeId::of::<LambdaException>() {
            Box::new(self.read_lambda_exception(input)?)
        } else if wanted == std::any::TypeId::of::<Block>() {
            Box::new(self.read_block(input)?)
        } else if wanted == std::any::TypeId::of::<Schema>() {
            Box::new(self.read_schema(input)?)
        } else if wanted == std::any::TypeId::of::<Marker>() {
            Box::new(self.read_marker(input)?)
        } else {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<T>())
            )));
        };
        boxed
            .downcast::<T>()
            .map(|b| *b)
            .map_err(|_| FederationError::ClassCast(format!("Cannot cast to {}", type_name::<T>())))
    }

    pub fn read_value_from_str<T: Any>(&self, input: &str) -> Result<T> {
        self.read_value(input.as_bytes())
    }
}
