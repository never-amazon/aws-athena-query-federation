use async_trait::async_trait;

use super::metadata_handler::{MetadataHandler, MetadataHandlerExt};
use super::record_handler::{RecordHandler, RecordHandlerExt};
use super::user_defined_function_handler::{
    UserDefinedFunctionHandler, UserDefinedFunctionHandlerExt,
};
use crate::exceptions::Result;
use crate::lambda::{LambdaContext, RequestStreamHandler};

macro_rules! stream_handler {
    ($name:ident, $handler:ident, $ext:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
        pub struct $name<H>(H);

        impl<H: $handler> $name<H> {
            pub fn new(handler: H) -> Self {
                $name(handler)
            }

            pub fn handler(&self) -> &H {
                &self.0
            }

            pub fn into_handler(self) -> H {
                self.0
            }
        }

        #[async_trait]
        impl<H: $handler> RequestStreamHandler for $name<H> {
            async fn handle_request(
                &self,
                input: &[u8],
                context: Option<&LambdaContext>,
            ) -> Result<Vec<u8>> {
                $ext::handle_request(&self.0, input, context).await
            }
        }
    };
}

stream_handler!(MetadataStreamHandler, MetadataHandler, MetadataHandlerExt);
stream_handler!(RecordStreamHandler, RecordHandler, RecordHandlerExt);
stream_handler!(
    UserDefinedFunctionStreamHandler,
    UserDefinedFunctionHandler,
    UserDefinedFunctionHandlerExt
);
