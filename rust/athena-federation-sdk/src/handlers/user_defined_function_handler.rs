use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;

use super::federation_capabilities::CAPABILITIES;
use super::metadata_handler::java_class_of_request;
use crate::data::projectors::{ArrowValueProjector, ProjectorUtils};
use crate::data::writers::extractors::Extractor;
use crate::data::writers::fieldwriters::{FieldWriter, FieldWriterFactory};
use crate::data::writers::holders::{
    NullableBigIntHolder, NullableBitHolder, NullableDateDayHolder, NullableDateMilliHolder,
    NullableDecimalHolder, NullableFloat4Holder, NullableFloat8Holder, NullableIntHolder,
    NullableSmallIntHolder, NullableTinyIntHolder, NullableVarBinaryHolder, NullableVarCharHolder,
};
use crate::data::writers::GeneratedRowWriter;
use crate::data::{
    local_date_time_to_epoch_milli_utc, local_date_to_epoch_day, Block, BlockAllocator,
    BlockAllocatorImpl, BlockUtils, DefaultFieldResolver, FieldVector, MinorType, Value,
};
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::lambda::LambdaContext;
use crate::request::{FederationRequest, PingRequest, PingResponse};
use crate::serde::{ObjectMapper, SerdeVersion, SERDE_VERSION};
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

impl JavaType {
    pub fn for_minor_type(minor_type: MinorType) -> Result<JavaType> {
        Ok(match BlockUtils::get_java_type(minor_type)? {
            "java.lang.Byte" => JavaType::Byte,
            "java.lang.Short" => JavaType::Short,
            "java.lang.Character" => JavaType::Character,
            "java.lang.Integer" => JavaType::Integer,
            "java.lang.Long" => JavaType::Long,
            "java.lang.Float" => JavaType::Float,
            "java.lang.Double" => JavaType::Double,
            "java.math.BigDecimal" => JavaType::BigDecimal,
            "java.lang.String" => JavaType::String,
            "[B" => JavaType::ByteArray,
            "java.lang.Boolean" => JavaType::Boolean,
            "java.time.LocalDate" => JavaType::LocalDate,
            "java.time.LocalDateTime" => JavaType::LocalDateTime,
            "java.time.ZonedDateTime" => JavaType::ZonedDateTime,
            "java.util.List" => JavaType::List,
            "java.util.Map" => JavaType::Map,
            other => {
                return Err(FederationError::invalid_input(format!(
                    "Unknown type {other}"
                )))
            }
        })
    }

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
        let mut value_projectors: Vec<Box<dyn ArrowValueProjector + '_>> = Vec::new();
        for field in input_records.schema().fields() {
            let field_reader = input_records
                .get_field_reader(field.name())
                .ok_or_else(|| {
                    FederationError::null_pointer(format!("No field reader for {}", field.name()))
                })?;
            value_projectors.push(ProjectorUtils::create_arrow_value_projector(field_reader)?);
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
            let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&allocator));
            let raw_request = object_mapper.read_request(input)?;
            match &raw_request {
                FederationRequest::Ping(ping) => {
                    let response = do_ping(self, ping).await;
                    object_mapper
                        .write_response(&response.into())
                        .map(String::into_bytes)
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
        object_mapper
            .write_response(&response.into())
            .map(String::into_bytes)
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
        .map(|f| JavaType::for_minor_type(MinorType::get_minor_type_for_arrow_type(f.data_type())))
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

fn result_of(context: &dyn Any) -> Result<&Value> {
    context
        .downcast_ref::<Value>()
        .ok_or_else(|| FederationError::ClassCast("context is not a UDF result".into()))
}

fn class_cast(value: &Value, target: &str) -> FederationError {
    FederationError::class_cast(value.java_class_name(), target)
}

fn create_output_row_writer(
    field_name: &str,
    arrow_type: &crate::data::ArrowType,
) -> Result<GeneratedRowWriter> {
    let builder = GeneratedRowWriter::new_builder();
    let minor_type = MinorType::get_minor_type_for_arrow_type(arrow_type);
    Ok(match make_extractor(minor_type) {
        Some(extractor) => builder.with_extractor(field_name, extractor),
        None => builder.with_field_writer_factory(field_name, make_factory(minor_type)?),
    }
    .build())
}

macro_rules! extractor {
    ($ctor:ident, $holder:ty, |$value:ident| $convert:expr) => {{
        Extractor::$ctor(move |context: &dyn Any, dst: &mut $holder| {
            match result_of(context)? {
                Value::Null => dst.is_set = 0,
                $value => {
                    dst.is_set = 1;
                    dst.value = $convert;
                }
            }
            Ok(())
        })
    }};
}

fn make_extractor(minor_type: MinorType) -> Option<Extractor> {
    Some(match minor_type {
        MinorType::INT => extractor!(int, NullableIntHolder, |v| match v {
            Value::Int(i) => *i,
            other => return Err(class_cast(other, "java.lang.Integer")),
        }),
        MinorType::DATEMILLI => {
            extractor!(date_milli, NullableDateMilliHolder, |v| match v {
                Value::LocalDateTime(dt) => local_date_time_to_epoch_milli_utc(dt),
                other => return Err(class_cast(other, "java.time.LocalDateTime")),
            })
        }
        MinorType::DATEDAY => extractor!(date_day, NullableDateDayHolder, |v| match v {
            Value::LocalDate(d) => i32::try_from(local_date_to_epoch_day(d)).unwrap_or(i32::MIN),
            other => return Err(class_cast(other, "java.time.LocalDate")),
        }),
        MinorType::TINYINT => extractor!(tiny_int, NullableTinyIntHolder, |v| match v {
            Value::Byte(b) => *b,
            other => return Err(class_cast(other, "java.lang.Byte")),
        }),
        MinorType::SMALLINT => {
            extractor!(small_int, NullableSmallIntHolder, |v| match v {
                Value::Short(s) => *s,
                other => return Err(class_cast(other, "java.lang.Short")),
            })
        }
        MinorType::FLOAT4 => extractor!(float4, NullableFloat4Holder, |v| match v {
            Value::Float(f) => *f,
            other => return Err(class_cast(other, "java.lang.Float")),
        }),
        MinorType::FLOAT8 => extractor!(float8, NullableFloat8Holder, |v| match v {
            Value::Double(d) => *d,
            other => return Err(class_cast(other, "java.lang.Double")),
        }),
        MinorType::DECIMAL => extractor!(decimal, NullableDecimalHolder, |v| match v {
            Value::Decimal(d) => d.clone(),
            other => return Err(class_cast(other, "java.math.BigDecimal")),
        }),
        MinorType::BIT => extractor!(bit, NullableBitHolder, |v| match v {
            Value::Boolean(b) => i32::from(*b),
            other => return Err(class_cast(other, "java.lang.Boolean")),
        }),
        MinorType::BIGINT => extractor!(big_int, NullableBigIntHolder, |v| match v {
            Value::Long(l) => *l,
            other => return Err(class_cast(other, "java.lang.Long")),
        }),
        MinorType::VARCHAR => extractor!(var_char, NullableVarCharHolder, |v| match v {
            Value::String(s) => s.clone(),
            other => return Err(class_cast(other, "java.lang.String")),
        }),
        MinorType::VARBINARY => {
            extractor!(var_binary, NullableVarBinaryHolder, |v| match v {
                Value::Bytes(b) => b.clone(),
                other => return Err(class_cast(other, "[B")),
            })
        }
        _ => return None,
    })
}

struct ComplexValueWriter;

impl FieldWriter for ComplexValueWriter {
    fn write(
        &mut self,
        vector: &mut FieldVector,
        context: &dyn Any,
        row_num: usize,
    ) -> Result<bool> {
        let value = result_of(context)?;
        BlockUtils::set_complex_value(vector, row_num, &DefaultFieldResolver, value)?;
        Ok(true)
    }
}

struct ComplexValueWriterFactory;

impl FieldWriterFactory for ComplexValueWriterFactory {
    fn create(
        &self,
        _vector: &FieldVector,
        _extractor: Option<&Extractor>,
        _constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> Result<Box<dyn FieldWriter>> {
        Ok(Box::new(ComplexValueWriter))
    }
}

fn make_factory(minor_type: MinorType) -> Result<Arc<dyn FieldWriterFactory>> {
    match minor_type {
        MinorType::LIST | MinorType::STRUCT => Ok(Arc::new(ComplexValueWriterFactory)),
        other => Err(FederationError::invalid_input(format!(
            "Unsupported type {other}"
        ))),
    }
}
