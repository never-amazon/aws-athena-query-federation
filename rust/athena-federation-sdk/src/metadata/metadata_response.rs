use super::MetadataRequestType;

pub trait MetadataResponse {
    fn request_type(&self) -> MetadataRequestType;

    fn catalog_name(&self) -> &str;
}

macro_rules! impl_metadata_response {
    ($t:ty, $kind:expr) => {
        impl $crate::metadata::MetadataResponse for $t {
            fn request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            fn catalog_name(&self) -> &str {
                &self.catalog_name
            }
        }

        impl $t {
            pub fn request_type(&self) -> $crate::metadata::MetadataRequestType {
                $kind
            }

            pub fn catalog_name(&self) -> &str {
                &self.catalog_name
            }
        }
    };
}

pub(crate) use impl_metadata_response;
