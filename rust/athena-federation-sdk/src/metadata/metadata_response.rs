macro_rules! impl_metadata_response {
    ($t:ty, $kind:expr) => {
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
