use super::RecordRequestType;
use crate::request::FederatedIdentity;

pub trait RecordRequest {
    fn identity(&self) -> Option<&FederatedIdentity>;

    fn request_type(&self) -> RecordRequestType;

    fn catalog_name(&self) -> &str;

    fn query_id(&self) -> &str;
}
