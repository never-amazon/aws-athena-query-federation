use super::MetadataRequestType;
use crate::request::FederatedIdentity;

pub trait MetadataRequest {
    fn get_identity(&self) -> Option<&FederatedIdentity>;

    fn get_request_type(&self) -> MetadataRequestType;

    fn get_catalog_name(&self) -> &str;

    fn get_query_id(&self) -> Option<&str>;
}

macro_rules! impl_metadata_request {
    ($t:ty, $kind:expr) => {
        impl $crate::metadata::MetadataRequest for $t {
            fn get_identity(&self) -> Option<&$crate::request::FederatedIdentity> {
                self.identity.as_ref()
            }

            fn get_request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            fn get_catalog_name(&self) -> &str {
                &self.catalog_name
            }

            fn get_query_id(&self) -> Option<&str> {
                self.query_id.as_deref()
            }
        }

        impl $t {
            pub fn get_identity(&self) -> Option<&$crate::request::FederatedIdentity> {
                self.identity.as_ref()
            }

            pub fn get_request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            pub fn get_catalog_name(&self) -> &str {
                &self.catalog_name
            }

            pub fn get_query_id(&self) -> Option<&str> {
                self.query_id.as_deref()
            }
        }
    };
}

pub(crate) use impl_metadata_request;
