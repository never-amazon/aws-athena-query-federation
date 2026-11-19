use std::fmt;

use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::error::display::DisplayErrorContext;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;

use crate::exceptions::{
    AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode,
};

const SDK_CLIENT_EXCEPTION: &str = "software.amazon.awssdk.core.exception.SdkClientException";
const REQUEST_ID_HEADERS: [&str; 2] = ["x-amzn-requestid", "x-amz-request-id"];
const EXTENDED_REQUEST_ID_HEADER: &str = "x-amz-id-2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AwsService {
    S3,
    Kms,
    SecretsManager,
    Glue,
    Athena,
    Sts,
}

impl AwsService {
    pub fn service_name(&self) -> &'static str {
        match self {
            AwsService::S3 => "S3",
            AwsService::Kms => "Kms",
            AwsService::SecretsManager => "SecretsManager",
            AwsService::Glue => "Glue",
            AwsService::Athena => "Athena",
            AwsService::Sts => "Sts",
        }
    }

    fn java_package(self) -> &'static str {
        match self {
            AwsService::S3 => "s3",
            AwsService::Kms => "kms",
            AwsService::SecretsManager => "secretsmanager",
            AwsService::Glue => "glue",
            AwsService::Athena => "athena",
            AwsService::Sts => "sts",
        }
    }

    fn modeled_error_codes(self) -> &'static [&'static str] {
        match self {
            AwsService::S3 => S3_MODELED,
            AwsService::Kms => KMS_MODELED,
            AwsService::SecretsManager => SECRETSMANAGER_MODELED,
            AwsService::Glue => GLUE_MODELED,
            AwsService::Athena => ATHENA_MODELED,
            AwsService::Sts => STS_MODELED,
        }
    }

    pub fn java_exception_class(&self, error_code: Option<&str>) -> String {
        let simple_name = match error_code.filter(|c| self.modeled_error_codes().contains(c)) {
            Some(code) if code.ends_with("Exception") => code.to_string(),
            Some(code) => format!("{code}Exception"),
            None => format!("{}Exception", self.service_name()),
        };
        format!(
            "software.amazon.awssdk.services.{}.model.{}",
            self.java_package(),
            simple_name
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AwsErrorKind {
    Service {
        service: AwsService,
        error_code: Option<String>,
        status_code: Option<u16>,
        request_id: Option<String>,
        extended_request_id: Option<String>,
        error_details: bool,
    },
    Client,
    Other {
        error_code: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsServiceError {
    kind: AwsErrorKind,
    message: Option<String>,
    num_attempts: Option<u32>,
}

impl Default for AwsServiceError {
    fn default() -> AwsServiceError {
        AwsServiceError::other(None, None)
    }
}

impl AwsServiceError {
    pub const IO_ERROR_CODE: &'static str = "IOException";

    fn other(error_code: Option<String>, message: Option<String>) -> AwsServiceError {
        AwsServiceError {
            kind: AwsErrorKind::Other { error_code },
            message,
            num_attempts: None,
        }
    }

    pub fn new(message: impl Into<String>) -> AwsServiceError {
        AwsServiceError::other(None, Some(message.into()))
    }

    pub fn with_code(error_code: impl Into<String>, message: impl Into<String>) -> AwsServiceError {
        AwsServiceError::other(Some(error_code.into()), Some(message.into()))
    }

    pub fn service(
        service: AwsService,
        error_code: Option<String>,
        message: Option<String>,
    ) -> AwsServiceError {
        AwsServiceError {
            kind: AwsErrorKind::Service {
                service,
                error_code,
                status_code: None,
                request_id: None,
                extended_request_id: None,
                error_details: false,
            },
            message,
            num_attempts: None,
        }
    }

    pub fn client(message: impl Into<String>) -> AwsServiceError {
        AwsServiceError {
            kind: AwsErrorKind::Client,
            message: Some(message.into()),
            num_attempts: None,
        }
    }

    pub fn io(message: impl Into<String>) -> AwsServiceError {
        AwsServiceError::with_code(Self::IO_ERROR_CODE, message)
    }

    #[must_use]
    pub fn with_status_code(mut self, status: u16) -> AwsServiceError {
        if let AwsErrorKind::Service { status_code, .. } = &mut self.kind {
            *status_code = Some(status);
        }
        self
    }

    #[must_use]
    pub fn with_error_details(mut self) -> AwsServiceError {
        if let AwsErrorKind::Service { error_details, .. } = &mut self.kind {
            *error_details = true;
        }
        self
    }

    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> AwsServiceError {
        if let AwsErrorKind::Service { request_id, .. } = &mut self.kind {
            *request_id = Some(id.into());
        }
        self
    }

    #[must_use]
    pub fn with_extended_request_id(mut self, id: impl Into<String>) -> AwsServiceError {
        if let AwsErrorKind::Service {
            extended_request_id,
            ..
        } = &mut self.kind
        {
            *extended_request_id = Some(id.into());
        }
        self
    }

    #[must_use]
    pub fn with_num_attempts(mut self, num_attempts: u32) -> AwsServiceError {
        self.num_attempts = Some(num_attempts);
        self
    }

    pub fn error_code(&self) -> Option<&str> {
        match &self.kind {
            AwsErrorKind::Service { error_code, .. } | AwsErrorKind::Other { error_code } => {
                error_code.as_deref()
            }
            AwsErrorKind::Client => None,
        }
    }

    pub fn message(&self) -> &str {
        self.message.as_deref().unwrap_or("")
    }

    pub fn status_code(&self) -> Option<u16> {
        match &self.kind {
            AwsErrorKind::Service { status_code, .. } => *status_code,
            AwsErrorKind::Client | AwsErrorKind::Other { .. } => None,
        }
    }

    pub fn get_service(&self) -> Option<AwsService> {
        match &self.kind {
            AwsErrorKind::Service { service, .. } => Some(*service),
            AwsErrorKind::Client | AwsErrorKind::Other { .. } => None,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match &self.kind {
            AwsErrorKind::Service { request_id, .. } => request_id.as_deref(),
            AwsErrorKind::Client | AwsErrorKind::Other { .. } => None,
        }
    }

    pub fn extended_request_id(&self) -> Option<&str> {
        match &self.kind {
            AwsErrorKind::Service {
                extended_request_id,
                ..
            } => extended_request_id.as_deref(),
            AwsErrorKind::Client | AwsErrorKind::Other { .. } => None,
        }
    }

    pub fn num_attempts(&self) -> Option<u32> {
        self.num_attempts
    }

    pub fn is_client_error(&self) -> bool {
        self.kind == AwsErrorKind::Client
    }

    pub fn is_service_exception_of(&self, service: AwsService) -> bool {
        self.get_service() == Some(service)
    }

    pub fn is_error_code(&self, code: &str) -> bool {
        self.error_code() == Some(code)
    }

    pub fn java_class_name(&self) -> Option<String> {
        match &self.kind {
            AwsErrorKind::Client => Some(SDK_CLIENT_EXCEPTION.to_string()),
            AwsErrorKind::Service {
                service,
                error_code,
                ..
            } => Some(service.java_exception_class(error_code.as_deref())),
            AwsErrorKind::Other { .. } => None,
        }
    }

    pub fn get_message(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(message) = &self.message {
            parts.push(message.clone());
        }
        if let AwsErrorKind::Service {
            service,
            status_code,
            request_id,
            extended_request_id,
            error_details: true,
            ..
        } = &self.kind
        {
            let mut details = vec![
                format!("Service: {}", service.service_name()),
                format!("Status Code: {}", status_code.unwrap_or(0)),
                format!("Request ID: {}", request_id.as_deref().unwrap_or("null")),
            ];
            if let Some(extended) = extended_request_id {
                details.push(format!("Extended Request ID: {extended}"));
            }
            parts.push(format!("({})", details.join(", ")));
        }
        if let Some(attempts) = self.num_attempts {
            parts.push(format!("(SDK Attempt Count: {attempts})"));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    pub fn from_sdk_error<E>(
        service: AwsService,
        err: &SdkError<E, HttpResponse>,
    ) -> AwsServiceError
    where
        E: ProvideErrorMetadata + std::error::Error + 'static,
    {
        let Some(service_error) = err.as_service_error() else {
            return AwsServiceError::client(DisplayErrorContext(err).to_string());
        };
        let raw = err.raw_response();
        let header = |name: &str| raw.and_then(|r| r.headers().get(name)).map(str::to_string);
        AwsServiceError {
            kind: AwsErrorKind::Service {
                service,
                error_code: service_error.code().map(str::to_string),
                status_code: raw.map(|r| r.status().as_u16()),
                request_id: REQUEST_ID_HEADERS.iter().find_map(|h| header(h)),
                extended_request_id: header(EXTENDED_REQUEST_ID_HEADER),
                error_details: true,
            },
            message: service_error.message().map(str::to_string),
            num_attempts: None,
        }
    }
}

impl fmt::Display for AwsServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = self.get_message();
        match (self.java_class_name(), message) {
            (Some(class), Some(message)) => write!(f, "{class}: {message}"),
            (Some(class), None) => f.write_str(&class),
            (None, message) => f.write_str(message.as_deref().unwrap_or("")),
        }
    }
}

impl std::error::Error for AwsServiceError {}

impl From<AwsServiceError> for FederationError {
    fn from(e: AwsServiceError) -> Self {
        if e.is_error_code(AwsServiceError::IO_ERROR_CODE) {
            let message = e.message().to_string();
            return FederationError::AthenaConnector(AthenaConnectorException::with_cause(
                message,
                std::sync::Arc::new(e),
                ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
            ));
        }
        FederationError::AwsService(e)
    }
}

impl From<FederationError> for AwsServiceError {
    fn from(e: FederationError) -> Self {
        match e {
            FederationError::AwsService(inner) => inner,
            other => AwsServiceError::other(
                other.error_code().map(|c| c.as_str().to_string()),
                Some(other.to_string()),
            ),
        }
    }
}

const S3_MODELED: &[&str] = &[
    "AccessDenied",
    "AnnotationLimitExceeded",
    "AnnotationNameTooLong",
    "BucketAlreadyExists",
    "BucketAlreadyOwnedByYou",
    "EncryptionTypeMismatch",
    "IdempotencyParameterMismatch",
    "InvalidAnnotationName",
    "InvalidObjectState",
    "InvalidPrefix",
    "InvalidRequest",
    "InvalidWriteOffset",
    "NoSuchAnnotation",
    "NoSuchBucket",
    "NoSuchKey",
    "NoSuchUpload",
    "ObjectAlreadyInActiveTierError",
    "ObjectNotInActiveTierError",
    "TooManyParts",
    "UnsupportedMediaType",
];

const KMS_MODELED: &[&str] = &[
    "AlreadyExistsException",
    "CloudHsmClusterInUseException",
    "CloudHsmClusterInvalidConfigurationException",
    "CloudHsmClusterNotActiveException",
    "CloudHsmClusterNotFoundException",
    "CloudHsmClusterNotRelatedException",
    "ConflictException",
    "CustomKeyStoreHasCMKsException",
    "CustomKeyStoreInvalidStateException",
    "CustomKeyStoreNameInUseException",
    "CustomKeyStoreNotFoundException",
    "DependencyTimeoutException",
    "DisabledException",
    "DryRunOperationException",
    "ExpiredImportTokenException",
    "IncorrectKeyException",
    "IncorrectKeyMaterialException",
    "IncorrectTrustAnchorException",
    "InvalidAliasNameException",
    "InvalidArnException",
    "InvalidCiphertextException",
    "InvalidGrantIdException",
    "InvalidGrantTokenException",
    "InvalidImportTokenException",
    "InvalidKeyUsageException",
    "InvalidMarkerException",
    "KeyUnavailableException",
    "KMSInternalException",
    "KMSInvalidMacException",
    "KMSInvalidSignatureException",
    "KMSInvalidStateException",
    "LimitExceededException",
    "MalformedPolicyDocumentException",
    "NotFoundException",
    "TagException",
    "UnsupportedOperationException",
    "XksKeyAlreadyInUseException",
    "XksKeyInvalidConfigurationException",
    "XksKeyNotFoundException",
    "XksProxyIncorrectAuthenticationCredentialException",
    "XksProxyInvalidConfigurationException",
    "XksProxyInvalidResponseException",
    "XksProxyUriEndpointInUseException",
    "XksProxyUriInUseException",
    "XksProxyUriUnreachableException",
    "XksProxyVpcEndpointServiceInUseException",
    "XksProxyVpcEndpointServiceInvalidConfigurationException",
    "XksProxyVpcEndpointServiceNotFoundException",
];

const SECRETSMANAGER_MODELED: &[&str] = &[
    "DecryptionFailure",
    "EncryptionFailure",
    "InternalServiceError",
    "InvalidNextTokenException",
    "InvalidParameterException",
    "InvalidRequestException",
    "LimitExceededException",
    "MalformedPolicyDocumentException",
    "PreconditionNotMetException",
    "PublicPolicyException",
    "ResourceExistsException",
    "ResourceNotFoundException",
];

const GLUE_MODELED: &[&str] = &[
    "AccessDeniedException",
    "AlreadyExistsException",
    "ColumnStatisticsTaskNotRunningException",
    "ColumnStatisticsTaskRunningException",
    "ColumnStatisticsTaskStoppingException",
    "ConcurrentModificationException",
    "ConcurrentRunsExceededException",
    "ConditionCheckFailureException",
    "ConflictException",
    "CrawlerNotRunningException",
    "CrawlerRunningException",
    "CrawlerStoppingException",
    "EntityNotFoundException",
    "FederatedResourceAlreadyExistsException",
    "FederationSourceException",
    "FederationSourceRetryableException",
    "GlueEncryptionException",
    "IdempotentParameterMismatchException",
    "IllegalBlueprintStateException",
    "IllegalSessionStateException",
    "IllegalWorkflowStateException",
    "IntegrationConflictOperationFault",
    "IntegrationNotFoundFault",
    "IntegrationQuotaExceededFault",
    "InternalServerException",
    "InternalServiceException",
    "InvalidInputException",
    "InvalidIntegrationStateFault",
    "InvalidStateException",
    "KMSKeyNotAccessibleFault",
    "MaterializedViewRefreshTaskNotRunningException",
    "MaterializedViewRefreshTaskRunningException",
    "MaterializedViewRefreshTaskStoppingException",
    "MLTransformNotReadyException",
    "NoScheduleException",
    "OperationNotSupportedException",
    "OperationTimeoutException",
    "PermissionTypeMismatchException",
    "ResourceNotFoundException",
    "ResourceNotReadyException",
    "ResourceNumberLimitExceededException",
    "SchedulerNotRunningException",
    "SchedulerRunningException",
    "SchedulerTransitioningException",
    "SessionBusyException",
    "TargetResourceNotFound",
    "ThrottlingException",
    "ValidationException",
    "VersionMismatchException",
];

const ATHENA_MODELED: &[&str] = &[
    "InternalServerException",
    "InvalidRequestException",
    "MetadataException",
    "ResourceNotFoundException",
    "SessionAlreadyExistsException",
    "TooManyRequestsException",
];

const STS_MODELED: &[&str] = &[
    "ExpiredTokenException",
    "ExpiredTradeInTokenException",
    "IDPCommunicationError",
    "IDPRejectedClaim",
    "InvalidAuthorizationMessageException",
    "InvalidIdentityToken",
    "JWTPayloadSizeExceededException",
    "MalformedPolicyDocument",
    "OutboundWebIdentityFederationDisabledException",
    "PackedPolicyTooLarge",
    "RegionDisabledException",
    "SessionDurationEscalationException",
];
