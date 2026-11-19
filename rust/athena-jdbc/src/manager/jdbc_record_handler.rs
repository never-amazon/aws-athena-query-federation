use std::collections::HashMap;
use std::hash::BuildHasher;
use std::num::NonZeroUsize;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;

use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::data::spill_config::SpillConfig;
use athena_federation_sdk::data::writers::extractors::Extractor;
use athena_federation_sdk::data::writers::fieldwriters::{FieldWriter, FieldWriterFactory};
use athena_federation_sdk::data::writers::GeneratedRowWriter;
use athena_federation_sdk::data::{
    block_utils, minor_type_of, Block, BlockAllocator, BlockSpiller, DefaultFieldResolver,
    FieldVector, MinorType, PutObjectRequest, S3BlockSpiller, S3Client, TzEncoding, Value,
};
use athena_federation_sdk::domain::predicate::{
    ConstraintEvaluator, ConstraintProjector, Constraints,
};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::handlers::{read_records, HandlerContext, RecordHandler};
use athena_federation_sdk::records::{ReadRecordsRequest, RemoteReadRecordsResponse};
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, BlockCrypto, EncryptionKey, FederatedIdentity,
};
use athena_federation_sdk::serde::{schema_to_bytes, IpcFormat};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_federation_substrait::{plan, SqlDialect};

use super::jdbc_split_query_builder::SplitQuery;
use super::jdbc_to_arrow_utils::jdbc_to_arrow_schema;
use crate::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use crate::qpt::{JDBC_QUERY_PASSTHROUGH, QUERY};
use crate::sql::{JdbcConnection, JdbcType, PreparedStatement, ResultSetMetaData, Row, SqlValue};
use crate::validate::{close_after, null_pointer};

const CLICKHOUSE_DB: &str = "clickhouse";
pub const EXPORT_SCHEMA: &str = "ExportSchema";
const SCHEMA_SUFFIX: &str = ".schema";

fn null_extractor(field_type: MinorType) -> Result<Extractor<Row>> {
    Extractor::null(field_type)
        .ok_or_else(|| FederationError::not_supported(format!("Unhandled type {field_type}")))
}

fn parse_java_double(text: &str) -> Result<f64> {
    let trimmed = text.trim_matches(|c: char| c <= ' ');
    let number = trimmed
        .strip_suffix(['d', 'D', 'f', 'F'])
        .unwrap_or(trimmed);
    let parsed = match number {
        "NaN" => Some(f64::NAN),
        "Infinity" | "+Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        other
            if other.bytes().any(|b| b.is_ascii_digit())
                && other.bytes().all(|b| {
                    b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')
                }) =>
        {
            other.parse::<f64>().ok()
        }
        _ => None,
    };
    parsed.ok_or_else(|| {
        if trimmed.is_empty() {
            FederationError::NumberFormat("empty String".to_string())
        } else {
            FederationError::NumberFormat(format!("For input string: \"{text}\""))
        }
    })
}

pub(crate) fn make_extractor_with_column_name_remapping<S: BuildHasher>(
    field: &Field,
    metadata: &ResultSetMetaData,
    partition_values: &HashMap<String, String, S>,
    column_name_remapping: &HashMap<String, String>,
) -> Result<Extractor<Row>> {
    let field_type = minor_type_of(field.data_type());
    let field_name = column_name_remapping
        .get(field.name())
        .cloned()
        .unwrap_or_else(|| field.name().clone());

    if let Some(partition_value) = partition_values.get(&field_name) {
        let partition_value = partition_value.clone();
        return Ok(Extractor::VarChar(Arc::new(move |_: &Row| {
            Ok(Some(partition_value.clone()))
        })));
    }

    let Ok(index) = metadata.find_column(&field_name) else {
        tracing::debug!(
            "Column {} not found in ResultSet, returning null extractor",
            field_name
        );
        return null_extractor(field_type);
    };

    typed_extractor(field_type, index)
}

fn extract_float8(row: &Row, index: usize) -> Result<Option<f64>> {
    if let Ok(value) = row.value(index).and_then(SqlValue::to_double) {
        return Ok(value);
    }
    let text = row.value(index)?.to_java_string().ok_or_else(|| {
        null_pointer("Cannot invoke \"String.replaceAll(String, String)\" because the return value of \"java.sql.ResultSet.getString(String)\" is null")
    })?;
    parse_java_double(&text.replace([',', '$'], "")).map(Some)
}

const MILLIS_PER_DAY: i64 = 86_400_000;
fn date_day(epoch_day: i64) -> i32 {
    i32::try_from(epoch_day).unwrap_or(if epoch_day < 0 { i32::MIN } else { i32::MAX })
}

fn typed_extractor(field_type: MinorType, index: usize) -> Result<Extractor<Row>> {
    let extractor = match field_type {
        MinorType::Bit => Extractor::Bit(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_boolean()?.map(i32::from))
        })),
        MinorType::TinyInt => Extractor::TinyInt(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_byte()?)
        })),
        MinorType::SmallInt => Extractor::SmallInt(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_short()?)
        })),
        MinorType::Int => {
            Extractor::Int(Arc::new(move |row: &Row| Ok(row.value(index)?.to_int()?)))
        }
        MinorType::BigInt => Extractor::BigInt(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_long()?)
        })),
        MinorType::Float4 => Extractor::Float4(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_float()?)
        })),
        MinorType::Float8 => {
            Extractor::Float8(Arc::new(move |row: &Row| extract_float8(row, index)))
        }
        MinorType::Decimal => Extractor::Decimal(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_big_decimal()?)
        })),
        MinorType::DateDay => Extractor::DateDay(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_epoch_day()?.map(date_day))
        })),
        MinorType::DateMilli => Extractor::DateMilli(Arc::new(move |row: &Row| {
            if row.metadata().column(index)?.jdbc_type() == JdbcType::Date {
                Ok(row
                    .value(index)?
                    .to_epoch_day()?
                    .map(|day| day.saturating_mul(MILLIS_PER_DAY)))
            } else {
                Ok(row.value(index)?.to_epoch_millis()?)
            }
        })),
        MinorType::VarChar => Extractor::VarChar(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_java_string())
        })),
        MinorType::VarBinary => Extractor::VarBinary(Arc::new(move |row: &Row| {
            Ok(row.value(index)?.to_bytes()?)
        })),
        other => {
            return Err(FederationError::not_supported(format!(
                "Unhandled type {other}"
            )))
        }
    };
    Ok(extractor)
}

fn calendar_value(value: &SqlValue, minor_type: MinorType) -> Result<Value> {
    let inner = match value {
        SqlValue::Rendered { value, .. } => value.as_ref(),
        other => other,
    };
    match (inner, minor_type) {
        (
            SqlValue::Timestamp(_) | SqlValue::TimestampTz(_),
            MinorType::DateMilli | MinorType::TimestampMilliTz,
        ) => {
            if let Some(millis) = value.to_epoch_millis()? {
                return Ok(Value::Long(millis));
            }
        }
        (_, MinorType::DateDay) if value.is_date_time() => {
            if let Some(day) = value.to_epoch_day()? {
                return Ok(Value::Int(date_day(day)));
            }
        }
        (SqlValue::Date(_), MinorType::DateMilli) => {
            if let Some(day) = value.to_epoch_day()? {
                return Ok(Value::Long(day.saturating_mul(MILLIS_PER_DAY)));
            }
        }
        _ => {}
    }
    Ok(value.to_value())
}

fn array_element_value(value: &SqlValue, element_type: MinorType) -> Result<Value> {
    match (value, element_type) {
        (SqlValue::Array(_), _) => Ok(value.to_value()),
        (_, MinorType::DateMilli | MinorType::DateDay) => calendar_value(value, element_type),
        _ => Ok(value.to_value()),
    }
}

struct ArrayFieldWriter {
    field_name: String,
    element_type: MinorType,
}

impl FieldWriter<Row> for ArrayFieldWriter {
    fn write(&mut self, vector: &mut FieldVector, row: &Row, row_num: usize) -> Result<bool> {
        if let Some(values) = row.by_label(self.field_name.as_str())?.to_array()? {
            let list = Value::List(
                values
                    .iter()
                    .map(|value| array_element_value(value, self.element_type))
                    .collect::<Result<_>>()?,
            );
            block_utils::set_complex_value(vector, row_num, &DefaultFieldResolver, &list)?;
        }
        Ok(true)
    }
}

fn spill_location_of(request: &ReadRecordsRequest) -> Result<&S3SpillLocation> {
    match request.split().spill_location() {
        Some(SpillLocation::S3(location)) => Ok(location),
        None => Err(null_pointer(
            "Cannot invoke \"com.amazonaws.athena.connector.lambda.domain.spill.S3SpillLocation.getBucket()\" because \"spillLocation\" is null",
        )),
    }
}

pub(crate) fn missing_factory() -> FederationError {
    null_pointer("Cannot invoke \"com.amazonaws.athena.connectors.jdbc.connection.JdbcConnectionFactory.getConnection(com.amazonaws.athena.connector.credentials.CredentialsProvider)\" because \"this.jdbcConnectionFactory\" is null")
}

pub fn make_extractor<S: BuildHasher>(
    field: &Field,
    metadata: &ResultSetMetaData,
    partition_values: &HashMap<String, String, S>,
) -> Result<Extractor<Row>> {
    make_extractor_with_column_name_remapping(field, metadata, partition_values, &HashMap::new())
}

pub fn make_field_writer_factory(field: &Field) -> FieldWriterFactory<Row> {
    let field_name = field.name().clone();
    let element_type = match field.data_type() {
        DataType::List(child) => minor_type_of(child.data_type()),
        other => minor_type_of(other),
    };
    Arc::new(
        move |_vector: &FieldVector,
              _extractor: Option<&Extractor<Row>>,
              _constraint: Option<ConstraintProjector>|
              -> Result<Box<dyn FieldWriter<Row>>> {
            Ok(Box::new(ArrayFieldWriter {
                field_name: field_name.clone(),
                element_type,
            }))
        },
    )
}

fn column_name_remapping(request: &ReadRecordsRequest) -> Result<HashMap<String, String>> {
    let Some(plan) = request.constraints().query_plan() else {
        return Ok(HashMap::new());
    };
    let renamed_to_original = plan::column_remapping(plan.substrait_plan(), &SqlDialect::ansi())?;
    Ok(renamed_to_original
        .into_iter()
        .filter_map(|(renamed, original)| original.map(|original| (original, renamed)))
        .collect())
}

pub enum ConnectionSource {
    Direct {
        config: DatabaseConnectionConfig,
        factory: Arc<dyn JdbcConnectionFactory>,
    },
    Delegating,
}

#[async_trait]
pub trait RecordDialect: Send + Sync + Sized + 'static {
    async fn build_split_sql(
        &self,
        handler: &JdbcRecordHandler<Self>,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement>;

    async fn read_with_constraint(
        &self,
        handler: &JdbcRecordHandler<Self>,
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        handler
            .read_split(spiller, request, query_status_checker)
            .await
    }

    async fn export_result_set_to_spill(
        &self,
        handler: &JdbcRecordHandler<Self>,
        request: &ReadRecordsRequest,
        allocator: &Arc<dyn BlockAllocator>,
        spill_locations: &mut Vec<SpillLocation>,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<Schema> {
        handler
            .export_query_result(request, allocator, spill_locations, query_status_checker)
            .await
    }

    async fn prepare_export_statement(
        &self,
        connection: &dyn JdbcConnection,
        query: &str,
    ) -> Result<PreparedStatement> {
        Ok(connection.prepare_statement(query).await?)
    }

    fn derive_export_schema(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        metadata: &ResultSetMetaData,
    ) -> Result<Schema> {
        jdbc_to_arrow_schema(metadata)
    }

    async fn enable_case_sensitively_look_up_session(
        &self,
        _connection: &dyn JdbcConnection,
    ) -> Result<bool> {
        Ok(false)
    }

    async fn disable_case_sensitively_look_up_session(
        &self,
        _connection: &dyn JdbcConnection,
    ) -> Result<bool> {
        Ok(false)
    }

    async fn credentials_provider(
        &self,
        context: &HandlerContext,
        secret_name: Option<&str>,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        context
            .credentials_provider(secret_name, override_config)
            .await
    }

    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

pub struct JdbcRecordHandler<D> {
    context: HandlerContext,
    connection: ConnectionSource,
    dialect: D,
}

impl<D: RecordDialect> JdbcRecordHandler<D> {
    pub fn new(
        context: HandlerContext,
        connection: ConnectionSource,
        dialect: D,
    ) -> JdbcRecordHandler<D> {
        JdbcRecordHandler {
            context,
            connection,
            dialect,
        }
    }

    pub fn dialect(&self) -> &D {
        &self.dialect
    }

    pub fn connection(&self) -> &ConnectionSource {
        &self.connection
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        self.context.config_options()
    }

    pub async fn connect(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let ConnectionSource::Direct { config, factory } = &self.connection else {
            return Err(missing_factory());
        };
        let override_config = self.context.identity_override_config(identity).await?;
        let credentials = self
            .dialect
            .credentials_provider(&self.context, config.secret(), override_config.as_ref())
            .await?;
        factory.get_connection(credentials.as_deref()).await
    }

    pub async fn close(&self) -> Result<()> {
        let mut first_failure = self.dialect.close().await.err();
        if let ConnectionSource::Direct { factory, .. } = &self.connection {
            if let Err(e) = factory.close().await {
                first_failure.get_or_insert(e);
            }
        }
        first_failure.map_or(Ok(()), Err)
    }

    pub async fn build_split_sql(
        &self,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        self.dialect.build_split_sql(self, connection, query).await
    }

    pub async fn build_query_passthrough_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        constraints: &Constraints,
    ) -> Result<PreparedStatement> {
        JDBC_QUERY_PASSTHROUGH.verify(constraints.query_passthrough_arguments())?;
        let client_pass_query = constraints
            .query_passthrough_arguments()
            .get(QUERY)
            .cloned()
            .flatten()
            .unwrap_or_default();
        Ok(jdbc_connection
            .prepare_statement(&client_pass_query)
            .await?)
    }

    pub fn build_export_spill_config(&self, request: &ReadRecordsRequest) -> Result<SpillConfig> {
        Ok(SpillConfig {
            max_inline_block_bytes: 0,
            spill_threads: NonZeroUsize::new(2),
            ..self.context.spill_config(request)?
        })
    }

    pub async fn resolve_scoped_s3_client(
        &self,
        request: &ReadRecordsRequest,
    ) -> Result<Arc<dyn S3Client>> {
        let override_config = self
            .context
            .identity_override_config(request.identity())
            .await?;
        Ok(self.context.s3_client(override_config.as_ref()))
    }

    pub async fn write_schema_sidecar(
        &self,
        s3_client: &dyn S3Client,
        spill_location: &S3SpillLocation,
        encryption_key: Option<&EncryptionKey>,
        derived_schema: &Schema,
    ) -> Result<()> {
        let schema_bytes = BlockCrypto::for_key(encryption_key.cloned())
            .encrypt_with_bytes(&schema_to_bytes(derived_schema, IpcFormat::ArrowDefault)?)?;
        let key = format!("{}{SCHEMA_SUFFIX}", spill_location.key());
        let length = i64::try_from(schema_bytes.len())
            .map_err(|e| FederationError::IllegalArgument(e.to_string()))?;
        let put_request = PutObjectRequest::builder()
            .bucket(spill_location.bucket())
            .key(&key)
            .content_length(length)
            .build();
        s3_client
            .put_object(put_request, schema_bytes)
            .await
            .map_err(FederationError::AwsService)?;
        tracing::info!(
            "putSchema: wrote {} bytes to s3://{}/{}",
            length,
            spill_location.bucket(),
            key
        );
        Ok(())
    }

    pub async fn export_query_result(
        &self,
        request: &ReadRecordsRequest,
        allocator: &Arc<dyn BlockAllocator>,
        spill_locations: &mut Vec<SpillLocation>,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<Schema> {
        let pass_through_query = request
            .constraints()
            .query_passthrough_arguments()
            .get(QUERY)
            .cloned()
            .flatten()
            .unwrap_or_default();
        let spill_location = spill_location_of(request)?;
        let spill_config = self.build_export_spill_config(request)?;
        let s3_client = self.resolve_scoped_s3_client(request).await?;
        tracing::info!(
            "writePassThroughQueryUsingResultSchema: executing export query; writing records via spiller to s3://{}/{}.N and schema to {}{}",
            spill_location.bucket(),
            spill_location.key(),
            spill_location.key(),
            SCHEMA_SUFFIX
        );

        let connection = self.connect(request.identity()).await?;
        let result = async {
            connection.set_auto_commit(false).await?;
            let prepared_statement = self
                .dialect
                .prepare_export_statement(connection.as_ref(), &pass_through_query)
                .await?;
            let mut result_set = connection.execute_query(&prepared_statement).await?;
            let derived_schema = self.dialect.derive_export_schema(self, result_set.metadata())?;
            self.write_schema_sidecar(
                s3_client.as_ref(),
                spill_location,
                request.split().encryption_key(),
                &derived_schema,
            )
            .await?;

            let evaluator = Arc::new(ConstraintEvaluator::new(
                &derived_schema,
                request.constraints().clone(),
            ));
            let spiller = S3BlockSpiller::new(
                Arc::clone(&s3_client),
                spill_config,
                Arc::clone(allocator),
                Arc::new(derived_schema.clone()),
                evaluator,
                self.config_options(),
            )?;
            let written = async {
                let column_count = derived_schema.fields().len();
                let mut rows_returned_from_database: u64 = 0;
                while let Some(row) = result_set.next_row().await? {
                    if !query_status_checker.is_query_running() {
                        break;
                    }
                    spiller
                        .write_rows(&mut |block: &mut Block, row_num: usize| -> Result<usize> {
                            block.set_tz_encoding(TzEncoding::Unpacked);
                            write_export_row(block, row_num, &row, column_count)
                        })
                        .await?;
                    rows_returned_from_database += 1;
                }
                connection.commit().await?;
                if spiller.spilled().await? {
                    spill_locations.extend(spiller.get_spill_locations().await?);
                }
                tracing::info!(
                    "writePassThroughQueryUsingResultSchema: wrote {} row(s) across {} spill object(s) + 1 schema sidecar under s3://{}/{}",
                    rows_returned_from_database,
                    spill_locations.len(),
                    spill_location.bucket(),
                    spill_location.key()
                );
                Ok::<(), FederationError>(())
            }
            .await;
            spiller.close().await;
            written.map(|()| derived_schema)
        }
        .await;
        close_after(connection.as_ref(), result).await
    }

    pub async fn read_split(
        &self,
        block_spiller: &dyn BlockSpiller,
        read_records_request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        tracing::info!(
            "{}: Catalog: {}, table {}, splits {}",
            read_records_request.query_id(),
            read_records_request.catalog_name(),
            read_records_request.table_name(),
            read_records_request.split()
        );
        let connection = self.connect(read_records_request.identity()).await?;
        let result = async {
            let database_product_name = connection.metadata().database_product_name();
            let is_clickhouse = CLICKHOUSE_DB.eq_ignore_ascii_case(&database_product_name);
            if !is_clickhouse {
                connection.set_auto_commit(false).await?;
            }
            self.dialect
                .enable_case_sensitively_look_up_session(connection.as_ref())
                .await?;

            let prepared_statement = self
                .build_split_sql(connection.as_ref(), &SplitQuery::of(read_records_request))
                .await?;
            let mut result_set = connection.execute_query(&prepared_statement).await?;
            let partition_values: HashMap<String, String> = read_records_request
                .split()
                .properties()
                .iter()
                .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
                .collect();
            let column_name_remapping = column_name_remapping(read_records_request)?;

            let mut row_writer_builder = GeneratedRowWriter::new_builder_with_constraints(
                read_records_request.constraints(),
            );
            for next in read_records_request.schema().fields() {
                if matches!(next.data_type(), DataType::List(_)) {
                    row_writer_builder = row_writer_builder
                        .with_field_writer_factory(next.name(), make_field_writer_factory(next));
                } else {
                    row_writer_builder = row_writer_builder.with_extractor(
                        next.name(),
                        make_extractor_with_column_name_remapping(
                            next,
                            result_set.metadata(),
                            &partition_values,
                            &column_name_remapping,
                        )?,
                    );
                }
            }
            let mut row_writer = row_writer_builder.build();
            let mut rows_returned_from_database: u64 = 0;
            while let Some(row) = result_set.next_row().await? {
                if !query_status_checker.is_query_running() {
                    return Ok(());
                }
                block_spiller
                    .write_rows(&mut |block: &mut Block, row_num: usize| -> Result<usize> {
                        Ok(usize::from(row_writer.write_row(block, row_num, &row)?))
                    })
                    .await?;
                rows_returned_from_database += 1;
            }
            tracing::info!("{} rows returned by database.", rows_returned_from_database);
            if !is_clickhouse {
                connection.commit().await?;
            }
            self.dialect
                .disable_case_sensitively_look_up_session(connection.as_ref())
                .await?;
            Ok(())
        }
        .await;
        close_after(connection.as_ref(), result).await
    }
}

fn write_export_row(
    block: &mut Block,
    row_num: usize,
    row: &Row,
    column_count: usize,
) -> Result<usize> {
    let vectors = block.field_vectors_mut();
    for (i, vector) in vectors.iter_mut().enumerate().take(column_count) {
        let value = calendar_value(row.value(i)?, vector.minor_type())?;
        block_utils::set_value(vector, row_num, &value)?;
    }
    Ok(1)
}

#[async_trait]
impl<D: RecordDialect> RecordHandler for JdbcRecordHandler<D> {
    fn context(&self) -> &HandlerContext {
        &self.context
    }

    async fn read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        if !request
            .constraints()
            .query_passthrough_arguments()
            .contains_key(EXPORT_SCHEMA)
        {
            return read_records(self, allocator, request).await;
        }

        let mut spill_locations = Vec::new();
        let override_config = self
            .context
            .identity_override_config(request.identity())
            .await?;
        let query_status_checker = self
            .context
            .query_status_checker(override_config.as_ref(), request.query_id());
        let derived_schema = self
            .dialect
            .export_result_set_to_spill(
                self,
                request,
                allocator,
                &mut spill_locations,
                &query_status_checker,
            )
            .await;
        drop(query_status_checker);
        Ok(FederationResponse::from(RemoteReadRecordsResponse::new(
            request.catalog_name(),
            derived_schema?,
            spill_locations,
            request.split().encryption_key().cloned(),
        )))
    }

    async fn read_with_constraint(
        &self,
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.dialect
            .read_with_constraint(self, spiller, request, query_status_checker)
            .await
    }
}

#[async_trait]
pub trait RecordDelegate: Send + Sync {
    async fn build_split_sql(
        &self,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement>;

    async fn close(&self) -> Result<()>;
}

#[async_trait]
impl<D: RecordDialect> RecordDelegate for JdbcRecordHandler<D> {
    async fn build_split_sql(
        &self,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        JdbcRecordHandler::build_split_sql(self, connection, query).await
    }

    async fn close(&self) -> Result<()> {
        JdbcRecordHandler::close(self).await
    }
}
