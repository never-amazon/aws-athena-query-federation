use std::sync::Arc;

use arrow::datatypes::SchemaRef;

use crate::data::{Block, BlockAllocator, RecordBatchSerDe};
use crate::exceptions::{
    AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode, Result,
};

pub(crate) fn internal_error(message: impl Into<String>) -> FederationError {
    FederationError::AthenaConnector(AthenaConnectorException::new(
        message,
        ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
    ))
}

pub(crate) fn invalid_input(message: impl Into<String>) -> FederationError {
    FederationError::AthenaConnector(AthenaConnectorException::new(
        message,
        ErrorDetails::of(FederationSourceErrorCode::InvalidInputException),
    ))
}

fn connector_with_cause(message: String, cause: crate::exceptions::Cause) -> FederationError {
    FederationError::AthenaConnector(AthenaConnectorException::with_cause(
        message,
        cause,
        ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
    ))
}

fn wrap_io(err: FederationError) -> FederationError {
    match err {
        FederationError::Io(message) => {
            let cause = Arc::new(std::io::Error::other(message.clone()));
            connector_with_cause(message, cause)
        }
        FederationError::IoError(e) => connector_with_cause(e.to_string(), e),
        FederationError::Arrow(e) => connector_with_cause(e.to_string(), e),
        other => other,
    }
}

pub(crate) fn serialize_block(block: &Block) -> Result<Vec<u8>> {
    let batch = block.get_arrow_record_batch().map_err(wrap_io)?;
    let mut out = Vec::new();
    batch.serialize(&mut out).map_err(wrap_io)?;
    Ok(out)
}

pub(crate) fn deserialize_block(
    allocator: &dyn BlockAllocator,
    bytes: &[u8],
    schema: SchemaRef,
) -> Result<Block> {
    let mut result_block = allocator.create_block(schema);
    let batch = RecordBatchSerDe::new(allocator)
        .deserialize(bytes)
        .map_err(wrap_io)?;
    result_block
        .load_arrow_record_batch(&batch)
        .map_err(wrap_io)?;
    Ok(result_block)
}
