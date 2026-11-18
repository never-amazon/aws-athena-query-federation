use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;

use athena_federation_sdk::data::spill_config::SpillConfig;
use athena_federation_sdk::data::writers::extractors::Extractor;
use athena_federation_sdk::data::writers::fieldwriters::{FieldWriter, FieldWriterFactory};
use athena_federation_sdk::data::writers::GeneratedRowWriter;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockSpiller, BlockUtils, DateTimeFormatterUtil, DefaultFieldResolver,
    FieldVector, MinorType, PutObjectRequest, S3BlockSpiller, S3Client, Value,
};
use athena_federation_sdk::domain::predicate::{
    ConstraintEvaluator, ConstraintProjector, Constraints,
};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{default_do_read_records, RecordHandler, RecordHandlerBase};
use athena_federation_sdk::metadata::optimizations::querypassthrough::QueryPassthroughSignature;
use athena_federation_sdk::records::{ReadRecordsRequest, RemoteReadRecordsResponse};
use athena_federation_sdk::request::{FederatedIdentity, FederationResponse};
use athena_federation_sdk::security::{BlockCrypto, EncryptionKey};
use athena_federation_sdk::serde::{schema_to_bytes, IpcFormat};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_federation_substrait::{SqlDialect, SubstraitSqlUtils};

use super::jdbc_to_arrow_utils::jdbc_to_arrow_schema;
use crate::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use crate::qpt::{JdbcQueryPassthrough, QUERY};
use crate::sql::{JdbcConnection, PreparedStatement, ResultSetMetaData, Row, SqlValue, Types};
use crate::validate::{close_after, null_pointer};

const CLICKHOUSE_DB: &str = "clickhouse";
pub const EXPORT_SCHEMA: &str = "ExportSchema";
const SCHEMA_SUFFIX: &str = ".schema";

pub struct JdbcRecordHandlerBase {
    record_base: RecordHandlerBase,
    jdbc_connection_factory: Option<Arc<dyn JdbcConnectionFactory>>,
    database_connection_config: Option<DatabaseConnectionConfig>,
    query_passthrough: JdbcQueryPassthrough,
}

impl JdbcRecordHandlerBase {
    pub fn new(
        record_base: RecordHandlerBase,
        database_connection_config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
    ) -> JdbcRecordHandlerBase {
        JdbcRecordHandlerBase {
            record_base,
            jdbc_connection_factory: Some(jdbc_connection_factory),
            database_connection_config: Some(database_connection_config),
            query_passthrough: JdbcQueryPassthrough,
        }
    }

    pub fn new_without_connection(record_base: RecordHandlerBase) -> JdbcRecordHandlerBase {
        JdbcRecordHandlerBase {
            record_base,
            jdbc_connection_factory: None,
            database_connection_config: None,
            query_passthrough: JdbcQueryPassthrough,
        }
    }

    pub fn record_base(&self) -> &RecordHandlerBase {
        &self.record_base
    }

    pub fn jdbc_connection_factory(&self) -> Option<&Arc<dyn JdbcConnectionFactory>> {
        self.jdbc_connection_factory.as_ref()
    }

    pub fn database_connection_config(&self) -> Option<&DatabaseConnectionConfig> {
        self.database_connection_config.as_ref()
    }

    pub fn query_passthrough(&self) -> &JdbcQueryPassthrough {
        &self.query_passthrough
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        self.record_base.config_options()
    }
}

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

pub(crate) fn make_extractor_with_column_name_remapping(
    field: &Field,
    metadata: &ResultSetMetaData,
    partition_values: &HashMap<String, String>,
    column_name_remapping: &HashMap<String, String>,
) -> Result<Extractor<Row>> {
    let field_type = MinorType::get_minor_type_for_arrow_type(field.data_type());
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
    if let Ok(value) = row.get_double(index) {
        return Ok(value);
    }
    let text = row.get_string(index)?.ok_or_else(|| {
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
        MinorType::BIT => Extractor::Bit(Arc::new(move |row: &Row| {
            Ok(row.get_boolean(index)?.map(i32::from))
        })),
        MinorType::TINYINT => {
            Extractor::TinyInt(Arc::new(move |row: &Row| Ok(row.get_byte(index)?)))
        }
        MinorType::SMALLINT => {
            Extractor::SmallInt(Arc::new(move |row: &Row| Ok(row.get_short(index)?)))
        }
        MinorType::INT => Extractor::Int(Arc::new(move |row: &Row| Ok(row.get_int(index)?))),
        MinorType::BIGINT => Extractor::BigInt(Arc::new(move |row: &Row| Ok(row.get_long(index)?))),
        MinorType::FLOAT4 => {
            Extractor::Float4(Arc::new(move |row: &Row| Ok(row.get_float(index)?)))
        }
        MinorType::FLOAT8 => {
            Extractor::Float8(Arc::new(move |row: &Row| extract_float8(row, index)))
        }
        MinorType::DECIMAL => {
            Extractor::Decimal(Arc::new(move |row: &Row| Ok(row.get_big_decimal(index)?)))
        }
        MinorType::DATEDAY => Extractor::DateDay(Arc::new(move |row: &Row| {
            Ok(row.get_object(index)?.to_epoch_day()?.map(date_day))
        })),
        MinorType::DATEMILLI => Extractor::DateMilli(Arc::new(move |row: &Row| {
            if row.metadata().column_type(index)? == Types::DATE {
                Ok(row
                    .get_object(index)?
                    .to_epoch_day()?
                    .map(|day| day.saturating_mul(MILLIS_PER_DAY)))
            } else {
                Ok(row.get_object(index)?.to_epoch_millis()?)
            }
        })),
        MinorType::VARCHAR => {
            Extractor::VarChar(Arc::new(move |row: &Row| Ok(row.get_string(index)?)))
        }
        MinorType::VARBINARY => {
            Extractor::VarBinary(Arc::new(move |row: &Row| Ok(row.get_bytes(index)?)))
        }
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
            MinorType::DATEMILLI | MinorType::TIMESTAMPMILLITZ,
        ) => {
            if let Some(millis) = value.to_epoch_millis()? {
                return Ok(Value::Long(millis));
            }
        }
        (_, MinorType::DATEDAY) if value.is_date_time() => {
            if let Some(day) = value.to_epoch_day()? {
                return Ok(Value::Int(date_day(day)));
            }
        }
        (SqlValue::Date(_), MinorType::DATEMILLI) => {
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
        (_, MinorType::DATEMILLI | MinorType::DATEDAY) => calendar_value(value, element_type),
        _ => Ok(value.to_value()),
    }
}

struct ArrayFieldWriter {
    field_name: String,
    element_type: MinorType,
}

impl FieldWriter<Row> for ArrayFieldWriter {
    fn write(&mut self, vector: &mut FieldVector, row: &Row, row_num: usize) -> Result<bool> {
        if let Some(values) = row.get_array(self.field_name.as_str())? {
            let list = Value::List(
                values
                    .iter()
                    .map(|value| array_element_value(value, self.element_type))
                    .collect::<Result<_>>()?,
            );
            BlockUtils::set_complex_value(vector, row_num, &DefaultFieldResolver, &list)?;
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

#[async_trait]
pub trait JdbcRecordHandler: RecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase;

    async fn export_result_set_to_spill(
        &self,
        request: &ReadRecordsRequest,
        allocator: &Arc<dyn BlockAllocator>,
        spill_locations: &mut Vec<SpillLocation>,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<Schema> {
        default_export_result_set_to_spill(
            self,
            request,
            allocator,
            spill_locations,
            query_status_checker,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the signature mirrors JdbcRecordHandler.buildSplitSql"
    )]
    async fn build_split_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        catalog_name: &str,
        table_name: &TableName,
        schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> Result<PreparedStatement>;

    async fn build_query_passthrough_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        constraints: &Constraints,
    ) -> Result<PreparedStatement> {
        let base = self.jdbc_record_base();
        base.query_passthrough()
            .verify(constraints.query_passthrough_arguments())?;
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

    async fn prepare_export_statement(
        &self,
        connection: &dyn JdbcConnection,
        query: &str,
    ) -> Result<PreparedStatement> {
        Ok(connection.prepare_statement(query).await?)
    }

    fn derive_export_schema(&self, metadata: &ResultSetMetaData) -> Result<Schema> {
        jdbc_to_arrow_schema(metadata)
    }

    fn build_export_spill_config(&self, request: &ReadRecordsRequest) -> Result<SpillConfig> {
        let mut builder = SpillConfig::new_builder();
        if let Some(location) = request.split().spill_location() {
            builder = builder.with_spill_location(location.clone());
        }
        builder
            .with_max_block_bytes(self.get_spill_config(request)?.max_block_bytes())
            .with_max_inline_block_bytes(0)
            .with_request_id(request.query_id())
            .with_encryption_key(request.split().encryption_key().cloned())
            .with_num_spill_threads(2)
            .build()
    }

    async fn resolve_scoped_s3_client(
        &self,
        request: &ReadRecordsRequest,
    ) -> Result<Arc<dyn S3Client>> {
        let override_config = self
            .get_request_override_config_with_config_options(
                request.identity().map(FederatedIdentity::config_options),
            )
            .await?;
        Ok(self.get_s3_client(override_config.as_ref(), self.record_base().amazon_s3()))
    }

    async fn write_schema_sidecar(
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

    fn make_factory(&self, field: &Field) -> FieldWriterFactory<Row> {
        let field_name = field.name().clone();
        let element_type = match field.data_type() {
            DataType::List(child) => MinorType::get_minor_type_for_arrow_type(child.data_type()),
            other => MinorType::get_minor_type_for_arrow_type(other),
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

    fn make_extractor(
        &self,
        field: &Field,
        metadata: &ResultSetMetaData,
        partition_values: &HashMap<String, String>,
    ) -> Result<Extractor<Row>> {
        make_extractor_with_column_name_remapping(
            field,
            metadata,
            partition_values,
            &HashMap::new(),
        )
    }

    fn get_column_name_remapping(
        &self,
        request: &ReadRecordsRequest,
    ) -> Result<HashMap<String, String>> {
        let Some(plan) = request.constraints().query_plan() else {
            return Ok(HashMap::new());
        };
        let renamed_to_original = SubstraitSqlUtils::get_column_remapping(
            plan.substrait_plan(),
            &self.get_sql_dialect(),
        )?;
        Ok(renamed_to_original
            .into_iter()
            .filter_map(|(renamed, original)| original.map(|original| (original, renamed)))
            .collect())
    }

    fn get_sql_dialect(&self) -> SqlDialect {
        SqlDialect::ansi()
    }

    async fn close(&self) -> Result<()> {
        match self.jdbc_record_base().jdbc_connection_factory() {
            Some(factory) => factory.close().await,
            None => Ok(()),
        }
    }
}

pub async fn default_export_result_set_to_spill<H: JdbcRecordHandler + ?Sized>(
    handler: &H,
    request: &ReadRecordsRequest,
    allocator: &Arc<dyn BlockAllocator>,
    spill_locations: &mut Vec<SpillLocation>,
    query_status_checker: &QueryStatusChecker,
) -> Result<Schema> {
    let direct_query = request
        .constraints()
        .query_passthrough_arguments()
        .get(QUERY)
        .cloned()
        .flatten()
        .unwrap_or_default();
    write_pass_through_query_using_result_schema(
        handler,
        request,
        &direct_query,
        allocator,
        spill_locations,
        query_status_checker,
    )
    .await
}

async fn write_pass_through_query_using_result_schema<H: JdbcRecordHandler + ?Sized>(
    handler: &H,
    request: &ReadRecordsRequest,
    pass_through_query: &str,
    allocator: &Arc<dyn BlockAllocator>,
    spill_locations: &mut Vec<SpillLocation>,
    query_status_checker: &QueryStatusChecker,
) -> Result<Schema> {
    let spill_location = spill_location_of(request)?;
    let spill_config = handler.build_export_spill_config(request)?;
    let s3_client = handler.resolve_scoped_s3_client(request).await?;
    tracing::info!(
        "writePassThroughQueryUsingResultSchema: executing export query; writing records via spiller to s3://{}/{}.N and schema to {}{}",
        spill_location.bucket(),
        spill_location.key(),
        spill_location.key(),
        SCHEMA_SUFFIX
    );

    let connection = handler.jdbc_get_connection(request.identity()).await?;
    let result = async {
        connection.set_auto_commit(false).await?;
        let prepared_statement = handler
            .prepare_export_statement(connection.as_ref(), pass_through_query)
            .await?;
        let mut result_set = connection.execute_query(&prepared_statement).await?;
        let derived_schema = handler.derive_export_schema(result_set.metadata())?;
        handler
            .write_schema_sidecar(
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
            handler.jdbc_record_base().config_options().clone(),
        )?;
        let written = async {
            let column_count = derived_schema.fields().len();
            let mut rows_returned_from_database: u64 = 0;
            while result_set.next().await? && query_status_checker.is_query_running() {
                let row = result_set.take_current_row()?;
                spiller
                    .write_rows(&mut |block: &mut Block, row_num: usize| -> Result<usize> {
                        DateTimeFormatterUtil::override_timezone_packing(false);
                        let written = write_export_row(block, row_num, &row, column_count);
                        DateTimeFormatterUtil::clear_timezone_packing_override();
                        written
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

fn write_export_row(
    block: &mut Block,
    row_num: usize,
    row: &Row,
    column_count: usize,
) -> Result<usize> {
    let vectors = block.field_vectors_mut();
    for (i, vector) in vectors.iter_mut().enumerate().take(column_count) {
        let value = calendar_value(row.get_object(i + 1)?, vector.minor_type())?;
        BlockUtils::set_value(vector, row_num, &value)?;
    }
    Ok(1)
}

#[async_trait]
pub trait JdbcRecordHandlerExt: JdbcRecordHandler {
    fn jdbc_database_connection_secret(&self) -> Option<&str>;

    async fn jdbc_get_connection(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Box<dyn JdbcConnection>>;

    async fn jdbc_do_read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse>;

    async fn jdbc_read_with_constraint(
        &self,
        block_spiller: &dyn BlockSpiller,
        read_records_request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()>;
}

#[async_trait]
impl<T: JdbcRecordHandler + ?Sized> JdbcRecordHandlerExt for T {
    fn jdbc_database_connection_secret(&self) -> Option<&str> {
        self.jdbc_record_base()
            .database_connection_config()
            .and_then(DatabaseConnectionConfig::secret)
    }

    async fn jdbc_get_connection(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let factory = self.jdbc_record_base().jdbc_connection_factory().ok_or_else(|| {
            null_pointer("Cannot invoke \"com.amazonaws.athena.connectors.jdbc.connection.JdbcConnectionFactory.getConnection(com.amazonaws.athena.connector.credentials.CredentialsProvider)\" because \"this.jdbcConnectionFactory\" is null")
        })?;
        let override_config = self
            .get_request_override_config_with_config_options(
                identity.map(FederatedIdentity::config_options),
            )
            .await?;
        let credentials = self
            .get_credential_provider_with_request_override_configuration(override_config.as_ref())
            .await?;
        factory.get_connection(credentials.as_deref()).await
    }

    async fn jdbc_do_read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        if !request
            .constraints()
            .query_passthrough_arguments()
            .contains_key(EXPORT_SCHEMA)
        {
            return default_do_read_records(self, allocator, request).await;
        }

        let mut spill_locations = Vec::new();
        let override_config = self
            .get_request_override_config_with_config_options(
                request.identity().map(FederatedIdentity::config_options),
            )
            .await?;
        let base = self.record_base();
        let athena = self.get_athena_client(override_config.as_ref(), base.athena());
        let query_status_checker = QueryStatusChecker::new(
            athena,
            Arc::clone(base.athena_invoker()),
            request.query_id(),
        );
        let derived_schema = self
            .export_result_set_to_spill(
                request,
                allocator,
                &mut spill_locations,
                &query_status_checker,
            )
            .await;
        query_status_checker.close();
        Ok(FederationResponse::from(RemoteReadRecordsResponse::new(
            request.catalog_name(),
            derived_schema?,
            spill_locations,
            request.split().encryption_key().cloned(),
        )))
    }

    async fn jdbc_read_with_constraint(
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
        let connection = self
            .jdbc_get_connection(read_records_request.identity())
            .await?;
        let result = async {
            let database_product_name = connection.metadata().get_database_product_name()?;
            let is_clickhouse = CLICKHOUSE_DB.eq_ignore_ascii_case(&database_product_name);
            if !is_clickhouse {
                connection.set_auto_commit(false).await?;
            }
            self.enable_case_sensitively_look_up_session(connection.as_ref())
                .await?;

            let prepared_statement = self
                .build_split_sql(
                    connection.as_ref(),
                    read_records_request.catalog_name(),
                    read_records_request.table_name(),
                    read_records_request.schema(),
                    read_records_request.constraints(),
                    read_records_request.split(),
                )
                .await?;
            let mut result_set = connection.execute_query(&prepared_statement).await?;
            let partition_values: HashMap<String, String> = read_records_request
                .split()
                .properties()
                .iter()
                .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
                .collect();
            let column_name_remapping = self.get_column_name_remapping(read_records_request)?;

            let mut row_writer_builder = GeneratedRowWriter::new_builder_with_constraints(
                read_records_request.constraints(),
            );
            for next in read_records_request.schema().fields() {
                if matches!(next.data_type(), DataType::List(_)) {
                    row_writer_builder = row_writer_builder
                        .with_field_writer_factory(next.name(), self.make_factory(next));
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
            while result_set.next().await? {
                if !query_status_checker.is_query_running() {
                    return Ok(());
                }
                let row = result_set.take_current_row()?;
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
            self.disable_case_sensitively_look_up_session(connection.as_ref())
                .await?;
            Ok(())
        }
        .await;
        close_after(connection.as_ref(), result).await
    }
}
