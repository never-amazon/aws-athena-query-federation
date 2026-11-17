use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{Array, ArrayRef, Date64Array, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockSpiller, FieldVector, S3BlockSpiller,
};
use athena_federation_sdk::domain::predicate::{ConstraintEvaluator, Constraints};
use athena_federation_sdk::domain::spill::SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{
    AwsClients, FederationRequestHandler, RecordHandler, RecordHandlerBase,
};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, CachableSecretsManager, KmsEncryptionProvider,
};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{
    default_export_result_set_to_spill, jdbc_to_arrow_schema, JdbcRecordHandler,
    JdbcRecordHandlerBase, JdbcRecordHandlerExt, JdbcSplitQueryBuilder, JdbcUtil,
};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement, ResultSetMetaData, SqlException};

use crate::s3_export::{construct_s3_uri, ExportReader, S3ParquetExportReader};
use crate::snowflake_constants::{is_blank, SnowflakeConstants};
use crate::snowflake_credentials_provider::SnowflakeCredentialsProvider;
use crate::snowflake_federation_expression_parser::SnowflakeFederationExpressionParser;
use crate::snowflake_metadata_handler::SnowflakeMetadataHandler;
use crate::snowflake_query_string_builder::SnowflakeQueryStringBuilder;
use crate::utils::SnowflakeArrowTypeConverter;

const FETCH_SIZE: i32 = 1000;
const UTC: &str = "UTC";

pub struct SnowflakeRecordHandler {
    base: JdbcRecordHandlerBase,
    jdbc_split_query_builder: Arc<dyn JdbcSplitQueryBuilder>,
    export_reader: Arc<dyn ExportReader>,
}

fn java_text(error: &FederationError) -> String {
    error.java_message().unwrap_or_else(|| "null".to_string())
}

fn internal_service_error(error: FederationError) -> FederationError {
    match SqlException::from_federation_error(&error) {
        Some(sql) => FederationError::internal(sql.message()),
        None => error,
    }
}

pub fn convert_timestamp_tz_milli_to_date_milli_fast(
    ts_vector: &TimestampMillisecondArray,
) -> Result<Date64Array> {
    let utc = ts_vector
        .timezone()
        .is_some_and(|zone| zone.eq_ignore_ascii_case(UTC));
    if !utc {
        return Err(FederationError::illegal_argument(
            "Athena S3 Export only support Timezone with UTC",
        ));
    }
    Ok(Date64Array::new(
        ts_vector.values().clone(),
        ts_vector.nulls().cloned(),
    ))
}

fn convert_column(field: &Field, column: &ArrayRef) -> Result<(Arc<Field>, ArrayRef)> {
    if let DataType::Timestamp(TimeUnit::Millisecond, Some(_)) = field.data_type() {
        if let Some(ts_vector) = column.as_any().downcast_ref::<TimestampMillisecondArray>() {
            let converted = convert_timestamp_tz_milli_to_date_milli_fast(ts_vector)?;
            return Ok((
                Arc::new(Field::new(
                    field.name(),
                    DataType::Date64,
                    field.is_nullable(),
                )),
                Arc::new(converted),
            ));
        }
    }
    Ok((Arc::new(field.clone()), Arc::clone(column)))
}

fn append_batch(
    block: &mut Block,
    start_row_num: usize,
    batch: &RecordBatch,
    request_schema: Option<&Schema>,
) -> Result<usize> {
    let batch_schema = batch.schema();
    for (index, field) in batch_schema.fields().iter().enumerate() {
        if request_schema.is_some_and(|schema| schema.field_with_name(field.name()).is_err()) {
            continue;
        }
        let (source_field, column) = if request_schema.is_some() {
            convert_column(field, batch.column(index))?
        } else {
            (Arc::clone(field), Arc::clone(batch.column(index)))
        };
        let source = FieldVector::from_array(source_field, column.as_ref())?;
        let target = block.get_field_vector_mut(field.name()).ok_or_else(|| {
            FederationError::null_pointer(
                "Cannot invoke \"org.apache.arrow.vector.FieldVector.getField()\" because \"targetVector\" is null",
            )
        })?;
        for row in 0..batch.num_rows() {
            target.set(start_row_num + row, &source.get_raw(row))?;
        }
    }
    Ok(batch.num_rows())
}

impl SnowflakeRecordHandler {
    pub fn default_query_builder() -> Arc<dyn JdbcSplitQueryBuilder> {
        Arc::new(SnowflakeQueryStringBuilder::new(
            SnowflakeConstants::DOUBLE_QUOTE_CHAR,
            Arc::new(SnowflakeFederationExpressionParser::new(
                SnowflakeConstants::DOUBLE_QUOTE_CHAR,
            )),
        ))
    }

    pub fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeRecordHandler> {
        let config = JdbcUtil::get_single_database_config_from_env(
            SnowflakeConstants::SNOWFLAKE_NAME,
            &config_options,
        )?;
        SnowflakeRecordHandler::new(clients, config, config_options)
    }

    pub fn new(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeRecordHandler> {
        let factory = Arc::new(SnowflakeMetadataHandler::connection_factory(
            &config,
            &config_options,
        )?);
        SnowflakeRecordHandler::with_query_builder(
            clients,
            config,
            factory,
            SnowflakeRecordHandler::default_query_builder(),
            config_options,
        )
    }

    pub fn with_query_builder(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        jdbc_split_query_builder: Arc<dyn JdbcSplitQueryBuilder>,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeRecordHandler> {
        let export_reader = Arc::new(S3ParquetExportReader::new(Arc::clone(&clients.s3)));
        let record_base = RecordHandlerBase::new(config.engine(), clients, config_options)?;
        Ok(SnowflakeRecordHandler {
            base: JdbcRecordHandlerBase::new(record_base, config, jdbc_connection_factory),
            jdbc_split_query_builder,
            export_reader,
        })
    }

    #[must_use]
    pub fn with_export_reader(
        mut self,
        export_reader: Arc<dyn ExportReader>,
    ) -> SnowflakeRecordHandler {
        self.export_reader = export_reader;
        self
    }

    fn s3_export_enabled(&self) -> bool {
        SnowflakeConstants::is_s3_export_enabled(self.base.config_options())
    }

    pub fn create_snowflake_credentials_provider(
        &self,
        secret_name: Option<&str>,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Option<SnowflakeCredentialsProvider> {
        let secret_name = secret_name.filter(|name| !is_blank(Some(name)))?;
        Some(SnowflakeCredentialsProvider::new(
            secret_name,
            Arc::clone(self.cachable_secrets_manager().secrets_manager()),
            request_override_configuration.cloned(),
        ))
    }

    pub async fn handle_s3_export_read(
        &self,
        spiller: &dyn BlockSpiller,
        records_request: &ReadRecordsRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        tracing::info!(
            "handleS3ExportRead: schema[{:?}] tableName[{}]",
            records_request.schema(),
            records_request.table_name()
        );
        let split = records_request.split();
        let export_bucket = split
            .get_property(SnowflakeConstants::SNOWFLAKE_SPLIT_EXPORT_BUCKET)
            .unwrap_or("null");
        let s3_object_key = split
            .get_property(SnowflakeConstants::SNOWFLAKE_SPLIT_OBJECT_KEY)
            .ok_or_else(|| {
                FederationError::null_pointer(
                    "Cannot invoke \"String.isEmpty()\" because \"s3ObjectKey\" is null",
                )
            })?;
        if s3_object_key.is_empty() {
            tracing::debug!("S3 object key is empty from request, skip read from S3");
            return Ok(());
        }
        let s3_path = construct_s3_uri(export_bucket, s3_object_key);
        let read = async {
            let reader = self
                .export_reader
                .construct_arrow_reader(&s3_path, records_request.schema())
                .await?;
            for batch in reader {
                let batch = batch?;
                spiller
                    .write_rows(&mut |block: &mut Block, start_row_num: usize| {
                        append_batch(block, start_row_num, &batch, Some(records_request.schema()))
                    })
                    .await?;
            }
            Ok::<(), FederationError>(())
        }
        .await;
        read.map_err(|e| {
            FederationError::connector(
                FederationSourceErrorCode::InternalServiceException,
                format!(
                    "Error in object content for object : {s3_path} {}",
                    java_text(&e)
                ),
            )
        })
    }

    async fn respill_exported_parquet(
        &self,
        request: &ReadRecordsRequest,
        allocator: &Arc<dyn BlockAllocator>,
        spill_locations: &mut Vec<SpillLocation>,
        query_status_checker: &QueryStatusChecker,
        export_bucket: &str,
        s3_object_key: &str,
    ) -> Result<Schema> {
        let split = request.split();
        let Some(SpillLocation::S3(spill_location)) = split.spill_location() else {
            return Err(FederationError::null_pointer(
                "Cannot invoke \"com.amazonaws.athena.connector.lambda.domain.spill.S3SpillLocation.getKey()\" because \"spillLocation\" is null",
            ));
        };
        let s3_client = self.resolve_scoped_s3_client(request).await?;
        let spill_config = self.build_export_spill_config(request)?;
        let s3_path = construct_s3_uri(export_bucket, s3_object_key);
        tracing::info!(
            "exportResultSetToSpill(S3 export): reading exported Parquet {} and re-spilling as Arrow",
            s3_path
        );
        let derived_schema = self.export_reader.inspect_parquet_schema(&s3_path).await?;
        let reader = self
            .export_reader
            .construct_arrow_reader(&s3_path, &derived_schema)
            .await?;
        let evaluator = Arc::new(ConstraintEvaluator::new(
            Some(allocator.as_ref()),
            &derived_schema,
            request.constraints().clone(),
        ));
        let spiller = S3BlockSpiller::new(
            Arc::clone(&s3_client),
            spill_config,
            Arc::clone(allocator),
            Arc::new(derived_schema.clone()),
            evaluator,
            self.base.config_options().clone(),
        )?;
        let written = async {
            self.write_schema_sidecar(
                s3_client.as_ref(),
                spill_location,
                split.encryption_key(),
                &derived_schema,
                allocator,
            )
            .await?;
            for batch in reader {
                let batch = batch?;
                if !query_status_checker.is_query_running() {
                    break;
                }
                spiller
                    .write_rows(&mut |block: &mut Block, start_row_num: usize| {
                        append_batch(block, start_row_num, &batch, None)
                    })
                    .await?;
            }
            if spiller.spilled().await? {
                spill_locations.extend(spiller.get_spill_locations().await?);
            }
            Ok::<(), FederationError>(())
        }
        .await;
        spiller.close().await;
        written.map(|()| derived_schema)
    }
}

#[async_trait]
impl FederationRequestHandler for SnowflakeRecordHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.record_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.record_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }

    async fn get_credential_provider_with_request_override_configuration(
        &self,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        Ok(self
            .create_snowflake_credentials_provider(
                self.database_connection_secret(),
                request_override_configuration,
            )
            .map(|provider| Box::new(provider) as Box<dyn CredentialsProvider>))
    }

    async fn create_credentials_provider(
        &self,
        secret_name: &str,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Box<dyn CredentialsProvider>> {
        self.create_snowflake_credentials_provider(
            Some(secret_name),
            request_override_configuration,
        )
        .map(|provider| Box::new(provider) as Box<dyn CredentialsProvider>)
        .ok_or_else(|| {
            FederationError::null_pointer("credentials provider is null for a blank secret name")
        })
    }
}

#[async_trait]
impl RecordHandler for SnowflakeRecordHandler {
    fn record_base(&self) -> &RecordHandlerBase {
        self.base.record_base()
    }

    async fn do_read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        self.jdbc_do_read_records(allocator, request).await
    }

    async fn read_with_constraint(
        &self,
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        if self.s3_export_enabled() {
            self.handle_s3_export_read(spiller, request, query_status_checker)
                .await
        } else {
            self.jdbc_read_with_constraint(spiller, request, query_status_checker)
                .await
        }
    }
}

#[async_trait]
impl JdbcRecordHandler for SnowflakeRecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase {
        &self.base
    }

    async fn export_result_set_to_spill(
        &self,
        request: &ReadRecordsRequest,
        allocator: &Arc<dyn BlockAllocator>,
        spill_locations: &mut Vec<SpillLocation>,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<Schema> {
        let split = request.split();
        let export_bucket = split.get_property(SnowflakeConstants::SNOWFLAKE_SPLIT_EXPORT_BUCKET);
        let s3_object_key = split.get_property(SnowflakeConstants::SNOWFLAKE_SPLIT_OBJECT_KEY);
        match s3_object_key {
            Some(key) if self.s3_export_enabled() && !key.is_empty() => {
                self.respill_exported_parquet(
                    request,
                    allocator,
                    spill_locations,
                    query_status_checker,
                    export_bucket.unwrap_or("null"),
                    key,
                )
                .await
            }
            _ => {
                default_export_result_set_to_spill(
                    self,
                    request,
                    allocator,
                    spill_locations,
                    query_status_checker,
                )
                .await
            }
        }
    }

    async fn build_split_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        _catalog_name: &str,
        table_name: &TableName,
        schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> Result<PreparedStatement> {
        let result = async {
            let mut prepared_statement = if constraints.is_query_pass_through() {
                self.build_query_passthrough_sql(jdbc_connection, constraints)
                    .await?
            } else {
                self.jdbc_split_query_builder
                    .build_sql(
                        jdbc_connection,
                        "",
                        table_name.schema_name(),
                        table_name.table_name(),
                        schema,
                        constraints,
                        split,
                    )
                    .await?
            };
            prepared_statement.set_fetch_size(FETCH_SIZE)?;
            Ok(prepared_statement)
        }
        .await;
        result.map_err(internal_service_error)
    }

    fn derive_export_schema(&self, metadata: &ResultSetMetaData) -> Result<Schema> {
        let base_schema = jdbc_to_arrow_schema(metadata)?;
        let mut fields = Vec::with_capacity(base_schema.fields().len());
        for (i, base_field) in base_schema.fields().iter().enumerate() {
            let column = i + 1;
            let is_arrow_integer = matches!(
                base_field.data_type(),
                DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64
            );
            if is_arrow_integer && metadata.scale(column)? == 0 {
                let precision = metadata.precision(column)?;
                if (1..=SnowflakeArrowTypeConverter::DEFAULT_PRECISION).contains(&precision) {
                    let precision = u8::try_from(precision).unwrap_or(u8::MAX);
                    fields.push(Arc::new(
                        Field::new(
                            base_field.name(),
                            DataType::Decimal128(precision, 0),
                            base_field.is_nullable(),
                        )
                        .with_metadata(base_field.metadata().clone()),
                    ));
                    continue;
                }
                tracing::debug!(
                    "deriveExportSchema: column {} precision {} outside Arrow decimal range; keeping {:?}",
                    base_field.name(),
                    precision,
                    base_field.data_type()
                );
            }
            fields.push(Arc::clone(base_field));
        }
        Ok(Schema::new_with_metadata(
            fields,
            base_schema.metadata().clone(),
        ))
    }
}
