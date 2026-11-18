use std::fmt;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;

use super::federation_capabilities::CAPABILITIES;
use super::metadata_handler::java_class_of_request;
use crate::data::projectors::ArrowValueProjector;
use crate::data::writers::extractors::Extractor;
use crate::data::writers::fieldwriters::{FieldWriter, FieldWriterFactory};
use crate::data::writers::GeneratedRowWriter;
use crate::data::{
    local_date_time_to_epoch_milli_utc, local_date_to_epoch_day, Block, BlockAllocator,
    BlockAllocatorImpl, BlockUtils, DefaultFieldResolver, FieldVector, MinorType, Value,
};
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::lambda::LambdaContext;
use crate::request::{FederationRequest, PingRequest, PingResponse};
use crate::serde::{ObjectMapper, VersionedObjectMapperFactory, SERDE_VERSION};
use crate::udf::{
    UserDefinedFunctionRequest, UserDefinedFunctionResponse, UserDefinedFunctionType,
};

const RETURN_COLUMN_COUNT: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JavaType {
    Byte,
    Short,
    Character,
    Integer,
    Long,
    Float,
    Double,
    BigDecimal,
    String,
    ByteArray,
    Boolean,
    LocalDate,
    LocalDateTime,
    ZonedDateTime,
    List,
    Map,
}

impl TryFrom<MinorType> for JavaType {
    type Error = FederationError;

    fn try_from(minor_type: MinorType) -> Result<JavaType> {
        Ok(match minor_type {
            MinorType::TIMESTAMPMICROTZ | MinorType::TIMESTAMPMILLITZ => JavaType::ZonedDateTime,
            MinorType::DATEMILLI => JavaType::LocalDateTime,
            MinorType::TINYINT | MinorType::UINT1 => JavaType::Byte,
            MinorType::SMALLINT => JavaType::Short,
            MinorType::UINT2 => JavaType::Character,
            MinorType::DATEDAY => JavaType::LocalDate,
            MinorType::INT | MinorType::UINT4 => JavaType::Integer,
            MinorType::UINT8 | MinorType::BIGINT => JavaType::Long,
            MinorType::DECIMAL => JavaType::BigDecimal,
            MinorType::FLOAT4 => JavaType::Float,
            MinorType::FLOAT8 => JavaType::Double,
            MinorType::VARCHAR => JavaType::String,
            MinorType::VARBINARY => JavaType::ByteArray,
            MinorType::BIT => JavaType::Boolean,
            MinorType::LIST => JavaType::List,
            MinorType::STRUCT => JavaType::Map,
            other => {
                return Err(FederationError::invalid_input(format!(
                    "Unknown type {other}"
                )))
            }
        })
    }
}

impl JavaType {
    pub fn class_name(&self) -> &'static str {
        match self {
            JavaType::Byte => "java.lang.Byte",
            JavaType::Short => "java.lang.Short",
            JavaType::Character => "java.lang.Character",
            JavaType::Integer => "java.lang.Integer",
            JavaType::Long => "java.lang.Long",
            JavaType::Float => "java.lang.Float",
            JavaType::Double => "java.lang.Double",
            JavaType::BigDecimal => "java.math.BigDecimal",
            JavaType::String => "java.lang.String",
            JavaType::ByteArray => "[B",
            JavaType::Boolean => "java.lang.Boolean",
            JavaType::LocalDate => "java.time.LocalDate",
            JavaType::LocalDateTime => "java.time.LocalDateTime",
            JavaType::ZonedDateTime => "java.time.ZonedDateTime",
            JavaType::List => "java.util.List",
            JavaType::Map => "java.util.Map",
        }
    }

    fn is_interface(self) -> bool {
        matches!(self, JavaType::List | JavaType::Map)
    }
}

impl fmt::Display for JavaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.is_interface() {
            "interface"
        } else {
            "class"
        };
        write!(f, "{kind} {}", self.class_name())
    }
}

pub type UdfFunction = Arc<dyn Fn(&[Value]) -> Result<Value> + Send + Sync>;

#[derive(Clone)]
pub struct UdfMethod {
    name: String,
    parameter_types: Vec<JavaType>,
    return_type: JavaType,
    function: UdfFunction,
}

impl UdfMethod {
    pub fn new<F>(
        name: impl Into<String>,
        parameter_types: Vec<JavaType>,
        return_type: JavaType,
        function: F,
    ) -> UdfMethod
    where
        F: Fn(&[Value]) -> Result<Value> + Send + Sync + 'static,
    {
        UdfMethod {
            name: name.into(),
            parameter_types,
            return_type,
            function: Arc::new(function),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parameter_types(&self) -> &[JavaType] {
        &self.parameter_types
    }

    pub fn return_type(&self) -> JavaType {
        self.return_type
    }

    pub fn invoke(&self, arguments: &[Value]) -> Result<Value> {
        (self.function)(arguments).map_err(|e| {
            FederationError::connector(
                FederationSourceErrorCode::InternalServiceException,
                e.to_string(),
            )
        })
    }
}

impl fmt::Debug for UdfMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UdfMethod")
            .field("name", &self.name)
            .field("parameter_types", &self.parameter_types)
            .field("return_type", &self.return_type)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default)]
pub struct UdfMethods {
    class_name: String,
    methods: Vec<UdfMethod>,
}

impl UdfMethods {
    pub fn new(class_name: impl Into<String>) -> UdfMethods {
        UdfMethods {
            class_name: class_name.into(),
            methods: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_method(mut self, method: UdfMethod) -> UdfMethods {
        self.methods.push(method);
        self
    }

    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    pub fn methods(&self) -> &[UdfMethod] {
        &self.methods
    }

    pub fn get_method(&self, name: &str, parameter_types: &[JavaType]) -> Result<&UdfMethod> {
        self.methods
            .iter()
            .find(|m| m.name == name && m.parameter_types == parameter_types)
            .ok_or_else(|| {
                let types: Vec<&str> = parameter_types.iter().map(JavaType::class_name).collect();
                let signature = if types.is_empty() {
                    "()".to_string()
                } else {
                    format!("({})", types.join(","))
                };
                FederationError::not_supported(format!(
                    "Failed to find UDF method. {}.{name}{signature} Please make sure the method name contains only lowercase and the method signature (name and argument types) in Lambda matches the function signature defined in SQL.",
                    self.class_name
                ))
            })
    }
}

#[async_trait]
pub trait UserDefinedFunctionHandler: Send + Sync {
    fn source_type(&self) -> &str;

    fn udf_methods(&self) -> &UdfMethods;

    async fn on_ping(&self, _request: &PingRequest) -> Result<()> {
        Ok(())
    }

    fn process_rows(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        udf_method: &UdfMethod,
        input_records: &Block,
        output_schema: &Schema,
    ) -> Result<Block> {
        let row_count = input_records.row_count();
        let mut value_projectors: Vec<ArrowValueProjector<'_>> = Vec::new();
        for field in input_records.schema().fields() {
            let field_reader = input_records
                .get_field_reader(field.name())
                .ok_or_else(|| {
                    FederationError::null_pointer(format!("No field reader for {}", field.name()))
                })?;
            value_projectors.push(ArrowValueProjector::new(field_reader)?);
        }
        let output_field = output_schema.fields().first().ok_or_else(|| {
            FederationError::IndexOutOfBounds("Index 0 out of bounds for length 0".into())
        })?;
        let mut output_row_writer =
            create_output_row_writer(output_field.name(), output_field.data_type())?;
        let mut output_records = allocator.create_block(Arc::new(output_schema.clone()));
        output_records.set_row_count(row_count);
        let mut arguments = Vec::with_capacity(value_projectors.len());
        for row_num in 0..row_count {
            arguments.clear();
            for projector in &value_projectors {
                arguments.push(projector.project(row_num)?);
            }
            let result = udf_method.invoke(&arguments)?;
            output_row_writer.write_row(&mut output_records, row_num, &result)?;
        }
        Ok(output_records)
    }
}

#[async_trait]
pub trait UserDefinedFunctionHandlerExt: UserDefinedFunctionHandler {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>>;

    async fn do_handle_request(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: &UserDefinedFunctionRequest,
    ) -> Result<Vec<u8>>;

    fn process_function(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &UserDefinedFunctionRequest,
    ) -> Result<UserDefinedFunctionResponse>;
}

#[async_trait]
impl<T: UserDefinedFunctionHandler + ?Sized> UserDefinedFunctionHandlerExt for T {
    async fn handle_request(
        &self,
        input: &[u8],
        _context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>> {
        let block_allocator = Arc::new(BlockAllocatorImpl::new());
        let allocator: Arc<dyn BlockAllocator> = block_allocator.clone();
        let result = async {
            let object_mapper = VersionedObjectMapperFactory::create(Arc::clone(&allocator));
            let raw_request = object_mapper.read_request(input)?;
            match &raw_request {
                FederationRequest::Ping(ping) => {
                    let response = do_ping(self, ping).await;
                    object_mapper.write_value_as_bytes(&response)
                }
                FederationRequest::UserDefinedFunction(request) => {
                    self.do_handle_request(&allocator, &object_mapper, request)
                        .await
                }
                other => Err(FederationError::invalid_input(format!(
                    "Expected a UserDefinedFunctionRequest but found {}",
                    java_class_of_request(other)
                ))),
            }
        }
        .await;
        block_allocator.close();
        result.map_err(FederationError::into_runtime_exception)
    }

    async fn do_handle_request(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        object_mapper: &ObjectMapper,
        request: &UserDefinedFunctionRequest,
    ) -> Result<Vec<u8>> {
        tracing::info!("doHandleRequest: request[{}]", request);
        let response = self.process_function(allocator, request)?;
        tracing::info!("doHandleRequest: response[{}]", response);
        object_mapper.write_value_as_bytes(&response)
    }

    fn process_function(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &UserDefinedFunctionRequest,
    ) -> Result<UserDefinedFunctionResponse> {
        match request.function_type() {
            UserDefinedFunctionType::SCALAR => {
                let udf_method = extract_scalar_function_method(self.udf_methods(), request)?;
                let output_records = self.process_rows(
                    allocator,
                    udf_method,
                    request.input_records(),
                    request.output_schema(),
                )?;
                Ok(UserDefinedFunctionResponse::new(
                    output_records,
                    udf_method.name(),
                ))
            }
        }
    }
}

async fn do_ping<H: UserDefinedFunctionHandler + ?Sized>(
    handler: &H,
    request: &PingRequest,
) -> PingResponse {
    let response = PingResponse::new(
        request.catalog_name(),
        request.query_id(),
        Some(handler.source_type().to_string()),
        CAPABILITIES,
        SERDE_VERSION,
    );
    if let Err(ex) = handler.on_ping(request).await {
        tracing::warn!(
            "doPing: encountered an exception while delegating onPing. {}",
            ex
        );
    }
    response
}

fn extract_java_types(schema: &Schema) -> Result<Vec<JavaType>> {
    schema
        .fields()
        .iter()
        .map(|f| JavaType::try_from(MinorType::get_minor_type_for_arrow_type(f.data_type())))
        .collect()
}

fn extract_scalar_function_method<'a>(
    methods: &'a UdfMethods,
    request: &UserDefinedFunctionRequest,
) -> Result<&'a UdfMethod> {
    let method_name = request.method_name();
    let argument_types = extract_java_types(request.input_records().schema())?;
    let return_types = extract_java_types(request.output_schema())?;
    if return_types.len() != RETURN_COLUMN_COUNT {
        return Err(FederationError::IllegalState(format!(
            "Expecting {RETURN_COLUMN_COUNT} return columns, found {} in method signature.",
            return_types.len()
        )));
    }
    let return_type = return_types[0];
    let udf_method = methods.get_method(method_name, &argument_types)?;
    let names: Vec<&str> = argument_types.iter().map(JavaType::class_name).collect();
    tracing::info!(
        "Found UDF method {} with input types [{}] and output types [{}]",
        method_name,
        names.join(", "),
        return_type.class_name()
    );
    if return_type != udf_method.return_type() {
        return Err(FederationError::invalid_input(format!(
            "signature return type {return_type} does not match udf implementation return type {}",
            udf_method.return_type()
        )));
    }
    Ok(udf_method)
}

fn class_cast(value: &Value, target: &str) -> FederationError {
    FederationError::class_cast(value.java_class_name(), target)
}

fn create_output_row_writer(
    field_name: &str,
    arrow_type: &crate::data::ArrowType,
) -> Result<GeneratedRowWriter<Value>> {
    let builder = GeneratedRowWriter::new_builder();
    let minor_type = MinorType::get_minor_type_for_arrow_type(arrow_type);
    Ok(match make_extractor(minor_type) {
        Some(extractor) => builder.with_extractor(field_name, extractor),
        None => builder.with_field_writer_factory(field_name, make_factory(minor_type)?),
    }
    .build())
}

macro_rules! extractor {
    ($variant:ident, |$value:ident| $convert:expr) => {{
        Extractor::$variant(Arc::new(move |result: &Value| {
            Ok(match result {
                Value::Null => None,
                $value => Some($convert),
            })
        }))
    }};
}

fn make_extractor(minor_type: MinorType) -> Option<Extractor<Value>> {
    Some(match minor_type {
        MinorType::INT => extractor!(Int, |v| match v {
            Value::Int(i) => *i,
            other => return Err(class_cast(other, "java.lang.Integer")),
        }),
        MinorType::DATEMILLI => {
            extractor!(DateMilli, |v| match v {
                Value::LocalDateTime(dt) => local_date_time_to_epoch_milli_utc(dt),
                other => return Err(class_cast(other, "java.time.LocalDateTime")),
            })
        }
        MinorType::DATEDAY => extractor!(DateDay, |v| match v {
            Value::LocalDate(d) => i32::try_from(local_date_to_epoch_day(d)).unwrap_or(i32::MIN),
            other => return Err(class_cast(other, "java.time.LocalDate")),
        }),
        MinorType::TINYINT => extractor!(TinyInt, |v| match v {
            Value::Byte(b) => *b,
            other => return Err(class_cast(other, "java.lang.Byte")),
        }),
        MinorType::SMALLINT => {
            extractor!(SmallInt, |v| match v {
                Value::Short(s) => *s,
                other => return Err(class_cast(other, "java.lang.Short")),
            })
        }
        MinorType::FLOAT4 => extractor!(Float4, |v| match v {
            Value::Float(f) => *f,
            other => return Err(class_cast(other, "java.lang.Float")),
        }),
        MinorType::FLOAT8 => extractor!(Float8, |v| match v {
            Value::Double(d) => *d,
            other => return Err(class_cast(other, "java.lang.Double")),
        }),
        MinorType::DECIMAL => extractor!(Decimal, |v| match v {
            Value::Decimal(d) => d.clone(),
            other => return Err(class_cast(other, "java.math.BigDecimal")),
        }),
        MinorType::BIT => extractor!(Bit, |v| match v {
            Value::Boolean(b) => i32::from(*b),
            other => return Err(class_cast(other, "java.lang.Boolean")),
        }),
        MinorType::BIGINT => extractor!(BigInt, |v| match v {
            Value::Long(l) => *l,
            other => return Err(class_cast(other, "java.lang.Long")),
        }),
        MinorType::VARCHAR => extractor!(VarChar, |v| match v {
            Value::String(s) => s.clone(),
            other => return Err(class_cast(other, "java.lang.String")),
        }),
        MinorType::VARBINARY => {
            extractor!(VarBinary, |v| match v {
                Value::Bytes(b) => b.clone(),
                other => return Err(class_cast(other, "[B")),
            })
        }
        _ => return None,
    })
}

struct ComplexValueWriter;

impl FieldWriter<Value> for ComplexValueWriter {
    fn write(&mut self, vector: &mut FieldVector, value: &Value, row_num: usize) -> Result<bool> {
        BlockUtils::set_complex_value(vector, row_num, &DefaultFieldResolver, value)?;
        Ok(true)
    }
}

fn make_factory(minor_type: MinorType) -> Result<FieldWriterFactory<Value>> {
    match minor_type {
        MinorType::LIST | MinorType::STRUCT => Ok(Arc::new(
            |_: &FieldVector,
             _: Option<&Extractor<Value>>,
             _: Option<ConstraintProjector>|
             -> Result<Box<dyn FieldWriter<Value>>> {
                Ok(Box::new(ComplexValueWriter))
            },
        )),
        other => Err(FederationError::invalid_input(format!(
            "Unsupported type {other}"
        ))),
    }
}
