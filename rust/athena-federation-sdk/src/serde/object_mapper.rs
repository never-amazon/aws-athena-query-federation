use std::any::type_name;
use std::sync::Arc;

use arrow::datatypes::Schema;

use crate::data::{Block, BlockAllocator};
use crate::domain::predicate::Marker;
use crate::exceptions::{FederationError, Result};
use crate::request::{FederationRequest, FederationResponse};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::v2::lambda_function_exception_serde::{self, LambdaException};
use crate::serde::{v1, v2, SerDeContext, SerdeVersion};

#[derive(Clone)]
pub struct ObjectMapper {
    version: SerdeVersion,
    allocator: Arc<dyn BlockAllocator>,
    indent_output: bool,
}

fn simple_type<T>() -> String {
    format!("[simple type, class {}]", type_name::<T>())
}

fn wrap_as_io(e: FederationError) -> FederationError {
    match e {
        FederationError::Io { .. } => e,
        other => FederationError::io(other.to_string()),
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
                Err(FederationError::UnexpectedToken(_)) => {
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

    fn write_with(&self, write: impl FnOnce(&mut JsonGenerator) -> Result<()>) -> Result<String> {
        let mut jgen = JsonGenerator::new(self.indent_output);
        write(&mut jgen).map_err(wrap_as_io)?;
        Ok(jgen.into_string())
    }

    fn write_bean<T>(
        &self,
        write: impl FnOnce(&mut JsonGenerator) -> Result<()>,
    ) -> Result<String> {
        if self.version.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured serializer for {}",
                simple_type::<T>()
            )));
        }
        self.write_with(write)
    }

    fn reject_bean_read_in_strict<T>(&self) -> Result<()> {
        if self.version.is_strict() {
            return Err(FederationError::invalid_input(format!(
                "No explicitly configured deserializer for {}",
                simple_type::<T>()
            )));
        }
        Ok(())
    }

    pub fn write_request(&self, request: &FederationRequest) -> Result<String> {
        self.write_with(|jgen| {
            if self.version.is_strict() {
                v2::federation_request_serde::serialize(request, jgen, self.version)
            } else {
                v1::bean_serde::serialize_request(request, jgen)
            }
        })
    }

    pub fn write_response(&self, response: &FederationResponse) -> Result<String> {
        self.write_with(|jgen| {
            if self.version.is_strict() {
                v2::federation_response_serde::serialize(response, jgen, self.version)
            } else {
                v1::bean_serde::serialize_response(response, jgen)
            }
        })
    }

    pub fn write_block(&self, block: &Block) -> Result<String> {
        self.write_bean::<Block>(|jgen| v1::bean_serde::serialize_block(Some(block), jgen))
    }

    pub fn write_schema(&self, schema: &Schema) -> Result<String> {
        self.write_bean::<Schema>(|jgen| v1::bean_serde::serialize_schema(schema, jgen))
    }

    pub fn write_marker(&self, marker: &Marker) -> Result<String> {
        self.write_bean::<Marker>(|jgen| v1::bean_serde::serialize_marker(marker, jgen))
    }

    fn init_for_reading<'a>(input: &'a [u8], type_label: &str) -> Result<JsonParser<'a>> {
        let mut jparser = JsonParser::new(input);
        match jparser.next_token()? {
            None => Err(FederationError::io(format!(
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
                simple_type::<LambdaException>()
            )));
        }
        let mut jparser = Self::init_for_reading(input, "LambdaException")?;
        lambda_function_exception_serde::deserialize(&mut jparser)
    }

    pub fn read_block(&self, input: &[u8]) -> Result<Block> {
        self.reject_bean_read_in_strict::<Block>()?;
        let mut jparser = Self::init_for_reading(input, "Block")?;
        v1::bean_serde::deserialize_block_tree(&mut jparser, &self.context())
    }

    pub fn read_schema(&self, input: &[u8]) -> Result<Schema> {
        self.reject_bean_read_in_strict::<Schema>()?;
        let mut jparser = Self::init_for_reading(input, "Schema")?;
        v1::bean_serde::deserialize_schema_tree(&mut jparser)
    }

    pub fn read_marker(&self, input: &[u8]) -> Result<Marker> {
        self.reject_bean_read_in_strict::<Marker>()?;
        let mut jparser = Self::init_for_reading(input, "Marker")?;
        let tree = jparser.read_tree()?;
        v1::bean_serde::marker_from_tree(&tree, &self.context())
    }
}
