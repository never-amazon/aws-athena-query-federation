use super::RecordRequestType;

pub trait RecordResponse {
    fn get_request_type(&self) -> RecordRequestType;

    fn get_catalog_name(&self) -> &str;
}
