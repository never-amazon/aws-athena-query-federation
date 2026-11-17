use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LambdaContext {
    aws_request_id: String,
    invoked_function_arn: Option<String>,
    function_name: String,
    function_version: String,
    memory_limit_in_mb: i32,
    log_group_name: String,
    log_stream_name: String,
    deadline_ms: u64,
}

impl LambdaContext {
    pub fn new() -> LambdaContext {
        LambdaContext::default()
    }

    #[must_use]
    pub fn with_aws_request_id(mut self, aws_request_id: impl Into<String>) -> LambdaContext {
        self.aws_request_id = aws_request_id.into();
        self
    }

    #[must_use]
    pub fn with_invoked_function_arn(
        mut self,
        invoked_function_arn: Option<String>,
    ) -> LambdaContext {
        self.invoked_function_arn = invoked_function_arn;
        self
    }

    #[must_use]
    pub fn with_function_name(mut self, function_name: impl Into<String>) -> LambdaContext {
        self.function_name = function_name.into();
        self
    }

    #[must_use]
    pub fn with_deadline_ms(mut self, deadline_ms: u64) -> LambdaContext {
        self.deadline_ms = deadline_ms;
        self
    }

    pub fn aws_request_id(&self) -> &str {
        &self.aws_request_id
    }

    pub fn invoked_function_arn(&self) -> Option<&str> {
        self.invoked_function_arn.as_deref()
    }

    pub fn function_name(&self) -> &str {
        &self.function_name
    }

    pub fn function_version(&self) -> &str {
        &self.function_version
    }

    pub fn memory_limit_in_mb(&self) -> i32 {
        self.memory_limit_in_mb
    }

    pub fn log_group_name(&self) -> &str {
        &self.log_group_name
    }

    pub fn log_stream_name(&self) -> &str {
        &self.log_stream_name
    }

    pub fn get_remaining_time_in_millis(&self) -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        if self.deadline_ms >= now {
            i64::try_from(self.deadline_ms - now).unwrap_or(i64::MAX)
        } else {
            -i64::try_from(now - self.deadline_ms).unwrap_or(i64::MAX)
        }
    }
}

impl From<&lambda_runtime::Context> for LambdaContext {
    fn from(context: &lambda_runtime::Context) -> Self {
        LambdaContext {
            aws_request_id: context.request_id.clone(),
            invoked_function_arn: Some(context.invoked_function_arn.clone()),
            function_name: context.env_config.function_name.clone(),
            function_version: context.env_config.version.clone(),
            memory_limit_in_mb: context.env_config.memory,
            log_group_name: context.env_config.log_group.clone(),
            log_stream_name: context.env_config.log_stream.clone(),
            deadline_ms: context.deadline,
        }
    }
}
