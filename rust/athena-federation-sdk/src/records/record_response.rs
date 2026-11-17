use super::RecordRequestType;

pub trait RecordResponse {
    fn request_type(&self) -> RecordRequestType;

    fn catalog_name(&self) -> &str;
}
