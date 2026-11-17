use std::sync::Arc;

use arrow::datatypes::SchemaRef;

use crate::data::{Block, BlockAllocator, RecordBatchSerDe};
use crate::exceptions::{AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode, Result};

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

fn wrap_io(err: FederationError) -> FederationError {
    match err {
        FederationError::Io(message) | FederationError::Arrow(message) => {
            let cause = Arc::new(std::io::Error::other(message.clone()));
            FederationError::AthenaConnector(AthenaConnectorException::with_cause(
                message,
                cause,
                ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
            ))
        }
        other => other,
    }
}

pub(crate) fn serialize_block(block: &Block) -> Result<Vec<u8>> {
    let batch = block.get_arrow_record_batch().map_err(wrap_io)?;
    let mut out = Vec::new();
    batch.serialize(&mut out).map_err(wrap_io)?;
    Ok(out)
}

pub(crate) fn deserialize_block(allocator: &dyn BlockAllocator, bytes: &[u8], schema: SchemaRef) -> Result<Block> {
    let mut result_block = allocator.create_block(schema);
    let batch = RecordBatchSerDe::new(allocator).deserialize(bytes).map_err(wrap_io)?;
    result_block.load_arrow_record_batch(&batch).map_err(wrap_io)?;
    Ok(result_block)
}
