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

fn connector_with_response(response: FederationError) -> FederationError {
    let message = response.to_string();
    FederationError::AthenaConnector(AthenaConnectorException::with_response(
        Arc::new(response),
        message,
        ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
    ))
}

fn wrap_io(err: FederationError) -> FederationError {
    match err {
        FederationError::Io(_) | FederationError::IoError(_) => connector_with_response(err),
        FederationError::Arrow(e) => {
            connector_with_response(FederationError::from(std::io::Error::other(e)))
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

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn arrow_error_becomes_connector_exception_with_io_exception_response() {
        let err = wrap_io(FederationError::from(arrow::error::ArrowError::IpcError(
            "bad ipc".into(),
        )));
        let exception = err.as_connector_exception().unwrap();
        assert_eq!(
            Some(FederationSourceErrorCode::InternalServiceException),
            exception.error_code()
        );
        assert_eq!("Ipc error: bad ipc", exception.message());
        assert!(exception.source().is_none());
        let response = exception
            .response()
            .and_then(|r| r.downcast_ref::<FederationError>())
            .unwrap();
        assert_eq!("java.io.IOException", response.java_class_name());
    }
}
