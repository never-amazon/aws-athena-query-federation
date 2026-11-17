use std::any::{type_name, Any};
use std::io::Write;
use std::sync::Arc;

use arrow::datatypes::Schema;

use crate::data::{Block, BlockAllocator};
use crate::domain::predicate::Marker;
use crate::exceptions::{FederationError, Result};
use crate::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest,
    GetTableLayoutResponse, GetTableRequest, GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use crate::records::{ReadRecordsRequest, ReadRecordsResponse, RemoteReadRecordsResponse};
use crate::request::{FederationRequest, FederationResponse, PingRequest, PingResponse};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::v2::lambda_function_exception_serde::{self, LambdaException};
use crate::serde::{v1, v2, SerDeContext, SERDE_VERSION_ONE};
use crate::udf::{UserDefinedFunctionRequest, UserDefinedFunctionResponse};

#[derive(Clone)]
pub struct ObjectMapper {
    version: i32,
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

fn as_request(value: &dyn Any) -> Option<FederationRequest> {
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

fn as_response(value: &dyn Any) -> Option<FederationResponse> {
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
    format!("[simple type, class {}]", name)
}

fn wrap_as_io(e: FederationError) -> FederationError {
    match e {
        FederationError::Io(_) => e,
        other => FederationError::Io(other.to_string()),
    }
}

impl ObjectMapper {
    pub fn strict(version: i32, allocator: Arc<dyn BlockAllocator>) -> ObjectMapper {
        ObjectMapper {
            version,
            allocator,
            indent_output: false,
        }
    }

    pub fn bean(allocator: Arc<dyn BlockAllocator>) -> ObjectMapper {
        ObjectMapper {
            version: SERDE_VERSION_ONE,
            allocator,
            indent_output: false,
        }
    }

    pub fn get_ser_de_version(&self) -> i32 {
        self.version
    }

    pub fn is_strict(&self) -> bool {
        self.version > SERDE_VERSION_ONE
    }

    pub fn get_allocator(&self) -> &Arc<dyn BlockAllocator> {
        &self.allocator
    }

    pub fn enable_indent_output(&mut self) -> &mut ObjectMapper {
        self.indent_output = true;
        self
    }

    pub fn disable_indent_output(&mut self) -> &mut ObjectMapper {
        self.indent_output = false;
        self
    }

    pub fn is_indent_output(&self) -> bool {
        self.indent_output
    }

    pub fn context(&self) -> SerDeContext<'_> {
        SerDeContext::new(self.version, self.allocator.as_ref())
    }

    fn resolve_writable<T: Any>(&self, value: &T) -> Result<Writable> {
        let any: &dyn Any = value;
        if let Some(r) = as_request(any) {
            return Ok(Writable::Request(Box::new(r)));
        }
        if let Some(r) = as_response(any) {
            return Ok(Writable::Response(Box::new(r)));
        }
        if !self.is_strict() {
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
        if self.is_strict() {
            match writable {
                Writable::Request(r) => v2::federation_request_serde::serialize(r, jgen, self.version),
                Writable::Response(r) => v2::federation_response_serde::serialize(r, jgen, self.version),
                _ => Err(FederationError::invalid_input("No explicitly configured serializer")),
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
        self.write_writable(&writable, &mut jgen).map_err(wrap_as_io)?;
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

    fn init_for_reading<'a>(&self, input: &'a [u8], type_label: &str) -> Result<JsonParser<'a>> {
        let mut jparser = JsonParser::new(input);
        match jparser.next_token()? {
            None => Err(FederationError::Io(format!(
                "No content to map due to end-of-input for {}",
                type_label
            ))),
            Some(JsonToken::ValueNull) => Err(FederationError::NullPointer(format!(
                "readValue returned null for {}",
                type_label
            ))),
            Some(_) => Ok(jparser),
        }
    }

    pub fn read_request(&self, input: &[u8]) -> Result<FederationRequest> {
        let mut jparser = self.init_for_reading(input, "FederationRequest")?;
        let ctx = self.context();
        if self.is_strict() {
            v2::federation_request_serde::do_deserialize(&mut jparser, &ctx)
        } else {
            v1::bean_serde::deserialize_request(&mut jparser, &ctx)
        }
    }

    pub fn read_response(&self, input: &[u8]) -> Result<FederationResponse> {
        let mut jparser = self.init_for_reading(input, "FederationResponse")?;
        let ctx = self.context();
        if self.is_strict() {
            v2::federation_response_serde::do_deserialize(&mut jparser, &ctx)
        } else {
            v1::bean_serde::deserialize_response(&mut jparser, &ctx)
        }
    }

    pub fn read_lambda_exception(&self, input: &[u8]) -> Result<LambdaException> {
        if !self.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "Cannot construct instance of {}",
                simple_type(type_name::<LambdaException>())
            )));
        }
        let mut jparser = self.init_for_reading(input, "LambdaException")?;
        lambda_function_exception_serde::deserialize(&mut jparser)
    }

    pub fn read_block(&self, input: &[u8]) -> Result<Block> {
        if self.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<Block>())
            )));
        }
        let mut jparser = self.init_for_reading(input, "Block")?;
        v1::bean_serde::deserialize_block_tree(&mut jparser, &self.context())
    }

    pub fn read_schema(&self, input: &[u8]) -> Result<Schema> {
        if self.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<Schema>())
            )));
        }
        let mut jparser = self.init_for_reading(input, "Schema")?;
        v1::bean_serde::deserialize_schema_tree(&mut jparser)
    }

    pub fn read_marker(&self, input: &[u8]) -> Result<Marker> {
        if self.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type(type_name::<Marker>())
            )));
        }
        let mut jparser = self.init_for_reading(input, "Marker")?;
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
