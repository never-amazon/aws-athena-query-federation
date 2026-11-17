use super::RecordRequestType;
use crate::request::FederatedIdentity;

pub trait RecordRequest {
    fn get_identity(&self) -> Option<&FederatedIdentity>;

    fn get_request_type(&self) -> RecordRequestType;

    fn get_catalog_name(&self) -> &str;

    fn get_query_id(&self) -> &str;
}
