use super::MetadataRequestType;
use crate::lambda::LambdaContext;
use crate::request::FederatedIdentity;

#[derive(Debug, Clone, Copy)]
pub struct MetadataHeader<'a> {
    pub identity: Option<&'a FederatedIdentity>,
    pub request_type: MetadataRequestType,
    pub catalog_name: &'a str,
    pub query_id: Option<&'a str>,
    pub context: Option<&'a LambdaContext>,
}

macro_rules! impl_metadata_request {
    ($t:ty, $kind:expr) => {
        impl $t {
            pub fn header(&self) -> $crate::metadata::MetadataHeader<'_> {
                $crate::metadata::MetadataHeader {
                    identity: self.identity.as_ref(),
                    request_type: $kind,
                    catalog_name: &self.catalog_name,
                    query_id: self.query_id.as_deref(),
                    context: self.context.as_ref(),
                }
            }

            pub fn identity(&self) -> Option<&$crate::request::FederatedIdentity> {
                self.identity.as_ref()
            }

            pub fn request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            pub fn catalog_name(&self) -> &str {
                &self.catalog_name
            }

            pub fn query_id(&self) -> Option<&str> {
                self.query_id.as_deref()
            }

            pub fn context(&self) -> Option<&$crate::lambda::LambdaContext> {
                self.context.as_ref()
            }

            pub fn set_context(&mut self, context: Option<$crate::lambda::LambdaContext>) {
                self.context = context;
            }
        }
    };
}

pub(crate) use impl_metadata_request;
