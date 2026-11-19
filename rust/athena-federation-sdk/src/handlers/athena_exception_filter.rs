use crate::security::{AwsService, AwsServiceError};

const TOO_MANY_REQUESTS_EXCEPTION: &str = "TooManyRequestsException";

pub fn is_athena_throttle(ex: &AwsServiceError) -> bool {
    ex.is_service_exception_of(AwsService::Athena) && ex.is_error_code(TOO_MANY_REQUESTS_EXCEPTION)
}
