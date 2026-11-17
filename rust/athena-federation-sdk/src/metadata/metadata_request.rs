use super::MetadataRequestType;
use crate::request::FederatedIdentity;

pub trait MetadataRequest {
    fn identity(&self) -> Option<&FederatedIdentity>;

    fn request_type(&self) -> MetadataRequestType;

    fn catalog_name(&self) -> &str;

    fn query_id(&self) -> Option<&str>;
}

macro_rules! impl_metadata_request {
    ($t:ty, $kind:expr) => {
        impl $crate::metadata::MetadataRequest for $t {
            fn identity(&self) -> Option<&$crate::request::FederatedIdentity> {
                self.identity.as_ref()
            }

            fn request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            fn catalog_name(&self) -> &str {
                &self.catalog_name
            }

            fn query_id(&self) -> Option<&str> {
                self.query_id.as_deref()
            }
        }

        impl $t {
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
        }
    };
}

pub(crate) use impl_metadata_request;
