use std::sync::Arc;

use async_trait::async_trait;
use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::value::RawValue;

use super::lambda_context::LambdaContext;
use super::lambda_error::LambdaError;
use crate::exceptions::{FederationError, Result};

#[async_trait]
pub trait RequestStreamHandler: Send + Sync {
    async fn handle_request(
        &self,
        input: &[u8],
        context: Option<&LambdaContext>,
    ) -> Result<Vec<u8>>;
}

pub async fn run<H>(handler: H) -> Result<()>
where
    H: RequestStreamHandler + 'static,
{
    let handler = Arc::new(handler);
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Box<RawValue>>| {
        let handler = Arc::clone(&handler);
        async move { invoke(handler.as_ref(), &event).await }
    }))
    .await
    .map_err(|e| FederationError::Other(Arc::from(e)))
}

async fn invoke<H>(
    handler: &H,
    event: &LambdaEvent<Box<RawValue>>,
) -> std::result::Result<Box<RawValue>, LambdaError>
where
    H: RequestStreamHandler + ?Sized,
{
    let context = LambdaContext::from(&event.context);
    let output = handler
        .handle_request(event.payload.get().as_bytes(), Some(&context))
        .await?;
    let text = String::from_utf8(output).map_err(|e| {
        FederationError::from(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    })?;
    RawValue::from_string(text)
        .map_err(|e| LambdaError::from(FederationError::from(std::io::Error::from(e))))
}
