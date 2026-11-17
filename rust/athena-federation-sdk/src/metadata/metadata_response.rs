use super::MetadataRequestType;

pub trait MetadataResponse {
    fn get_request_type(&self) -> MetadataRequestType;

    fn get_catalog_name(&self) -> &str;
}

macro_rules! impl_metadata_response {
    ($t:ty, $kind:expr) => {
        impl $crate::metadata::MetadataResponse for $t {
            fn get_request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            fn get_catalog_name(&self) -> &str {
                &self.catalog_name
            }
        }

        impl $t {
            pub fn get_request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            pub fn get_catalog_name(&self) -> &str {
                &self.catalog_name
            }
        }
    };
}

pub(crate) use impl_metadata_response;
