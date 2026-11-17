use crate::security::{AwsService, AwsServiceError};
use crate::ExceptionFilter;

const TOO_MANY_REQUESTS_EXCEPTION: &str = "TooManyRequestsException";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AthenaExceptionFilter(());

pub const ATHENA_EXCEPTION_FILTER: AthenaExceptionFilter = AthenaExceptionFilter(());

impl ExceptionFilter<AwsServiceError> for AthenaExceptionFilter {
    fn is_match(&self, ex: &AwsServiceError) -> bool {
        ex.is_service_exception_of(AwsService::Athena)
            && ex.is_error_code(TOO_MANY_REQUESTS_EXCEPTION)
    }
}
