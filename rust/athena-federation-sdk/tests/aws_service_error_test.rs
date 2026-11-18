use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::connection::{
    environment_constants, EnvironmentProperties, GlueConnectionClient,
};
use athena_federation_sdk::data::{Block, BlockSpiller, RowWriter};
use athena_federation_sdk::domain::predicate::ConstraintEvaluator;
use athena_federation_sdk::domain::spill::SpillLocation;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsService, AwsServiceError, EncryptionKeyFactory, KmsClient,
    KmsKeyFactory,
};
use athena_federation_sdk::{exceptions::FederationSourceErrorCode, FederationError};
use athena_federation_sdk::{ThrottleConfig, ThrottlingInvoker};
use aws_sdk_glue::types::Connection;
use aws_sdk_kms::types::DataKeySpec;
use serde_json::Value as Json;

fn fixture(name: &str) -> Json {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/aws_errors");
    path.push(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    serde_json::from_str(&text).expect("fixture is JSON")
}

fn text(case: &Json, field: &str) -> Option<String> {
    case.get(field).and_then(Json::as_str).map(str::to_string)
}

fn service_of(name: &str) -> AwsService {
    match name {
        "S3" => AwsService::S3,
        "Kms" => AwsService::Kms,
        "SecretsManager" => AwsService::SecretsManager,
        "Glue" => AwsService::Glue,
        "Athena" => AwsService::Athena,
        "Sts" => AwsService::Sts,
        other => panic!("unknown service {other}"),
    }
}

fn error_from_case(case: &Json) -> AwsServiceError {
    let mut error = if case["clientError"].as_bool() == Some(true) {
        AwsServiceError::client(text(case, "message").expect("client error message"))
    } else {
        let service = service_of(&text(case, "service").expect("service"));
        let mut error =
            AwsServiceError::service(service, text(case, "errorCode"), text(case, "message"));
        if case["errorDetails"].as_bool() == Some(true) {
            error = error.with_error_details();
        }
        error
    };
    if let Some(status) = case["statusCode"].as_u64() {
        error = error.with_status_code(u16::try_from(status).unwrap());
    }
    if let Some(request_id) = text(case, "requestId") {
        error = error.with_request_id(request_id);
    }
    if let Some(extended) = text(case, "extendedRequestId") {
        error = error.with_extended_request_id(extended);
    }
    if let Some(attempts) = case["numAttempts"].as_u64() {
        error = error.with_num_attempts(u32::try_from(attempts).unwrap());
    }
    error
}

fn case(name: &str) -> Json {
    fixture("aws_service_errors.json")
        .as_array()
        .expect("array")
        .iter()
        .find(|c| c["name"].as_str() == Some(name))
        .cloned()
        .unwrap_or_else(|| panic!("no case {name}"))
}

fn site_message(name: &str) -> String {
    text(&fixture("aws_error_site_messages.json"), name).expect("site message")
}

#[test]
fn java_message_and_to_string_match_aws_sdk_v2() {
    let cases = fixture("aws_service_errors.json");
    for case in cases.as_array().expect("array") {
        let name = case["name"].as_str().unwrap_or_default();
        let error = error_from_case(case);
        assert_eq!(
            text(case, "javaClass"),
            error.java_class_name(),
            "class for {name}"
        );
        assert_eq!(
            text(case, "getMessage"),
            error.get_message(),
            "getMessage for {name}"
        );
        assert_eq!(
            text(case, "toString"),
            Some(error.to_string()),
            "toString for {name}"
        );
        assert_eq!(
            text(case, "getMessage").unwrap_or_default(),
            FederationError::from(error).to_string(),
            "FederationError message for {name}"
        );
    }
}

struct NotSpilled;

#[async_trait]
impl BlockSpiller for NotSpilled {
    async fn write_rows(
        &self,
        _row_writer: &mut (dyn RowWriter + Send),
    ) -> athena_federation_sdk::Result<()> {
        Ok(())
    }

    fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
        Arc::new(ConstraintEvaluator::empty_evaluator())
    }

    async fn spilled(&self) -> athena_federation_sdk::Result<bool> {
        Ok(false)
    }

    async fn get_block(&self) -> athena_federation_sdk::Result<Block> {
        Err(FederationError::internal("not used"))
    }

    async fn get_spill_locations(&self) -> athena_federation_sdk::Result<Vec<SpillLocation>> {
        Ok(Vec::new())
    }

    async fn close(&self) {}
}

#[tokio::test]
async fn throttling_invoker_message_uses_java_to_string() {
    let throttle = error_from_case(&case("athena_unmodeled_throttling"));
    let invoker: ThrottlingInvoker<AwsServiceError> = ThrottlingInvoker::new(
        ThrottleConfig {
            initial_delay_ms: 10,
            max_delay_ms: 2_000,
            decrease: 0.5,
            increase_ms: 10,
        },
        |ex: &AwsServiceError| ex.is_service_exception_of(AwsService::Athena),
        Some(Arc::new(NotSpilled)),
    )
    .unwrap();
    let ex = invoker
        .invoke_with_timeout_millis(
            || async { Err::<i32, AwsServiceError>(throttle.clone()) },
            2_000,
        )
        .await
        .unwrap_err();
    assert_eq!(
        Some(FederationSourceErrorCode::ThrottlingException.as_str()),
        ex.error_code()
    );
    assert_eq!(site_message("throttlingInvokerMessage"), ex.message());
}

struct FailingKms {
    error: AwsServiceError,
}

#[async_trait]
impl KmsClient for FailingKms {
    async fn generate_data_key(
        &self,
        _key_id: &str,
        _key_spec: DataKeySpec,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError> {
        Err(self.error.clone())
    }

    async fn generate_random(&self, _number_of_bytes: i32) -> Result<Vec<u8>, AwsServiceError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn kms_key_factory_message_uses_java_get_message() {
    let factory = KmsKeyFactory::new(
        Arc::new(FailingKms {
            error: error_from_case(&case("kms_not_found")),
        }),
        "masterKey",
    );
    let e = factory.create().await.unwrap_err();
    assert_eq!(
        Some(FederationSourceErrorCode::EntityNotFoundException),
        e.error_code()
    );
    assert_eq!(site_message("kmsKeyFactoryMessage"), e.to_string());
}

struct FailingGlue {
    error: AwsServiceError,
}

#[async_trait]
impl GlueConnectionClient for FailingGlue {
    async fn get_connection(&self, _name: &str) -> Result<Connection, AwsServiceError> {
        Err(self.error.clone())
    }
}

#[tokio::test]
async fn environment_properties_message_uses_java_to_string() {
    let failure = case("glue_entity_not_found");
    let props = EnvironmentProperties::new()
        .with_env_map(HashMap::from([(
            environment_constants::DEFAULT_GLUE_CONNECTION.to_string(),
            "my-glue-conn".to_string(),
        )]))
        .with_glue_client(Arc::new(FailingGlue {
            error: error_from_case(&failure),
        }));
    let e = props.create_environment().await.unwrap_err();
    assert!(matches!(e, FederationError::Runtime(_)), "{e:?}");
    assert_eq!(text(&failure, "toString").expect("toString"), e.to_string());
}

fn raw_response(
    status: u16,
    headers: &[(&'static str, &'static str)],
) -> aws_smithy_runtime_api::client::orchestrator::HttpResponse {
    let mut response = aws_smithy_runtime_api::client::orchestrator::HttpResponse::new(
        aws_smithy_runtime_api::http::StatusCode::try_from(status).unwrap(),
        aws_smithy_types::body::SdkBody::from(""),
    );
    for (name, value) in headers {
        response.headers_mut().insert(*name, *value);
    }
    response
}

#[test]
fn from_sdk_error_reads_service_error_details() {
    use aws_sdk_secretsmanager::operation::put_secret_value::PutSecretValueError;
    use aws_smithy_runtime_api::client::result::SdkError;
    use aws_smithy_types::error::ErrorMetadata;

    let expected = case("secrets_manager_unmodeled_code");
    let error = PutSecretValueError::generic(
        ErrorMetadata::builder()
            .code("ThrottlingException")
            .message("Rate exceeded")
            .build(),
    );
    let sdk_error =
        SdkError::service_error(error, raw_response(400, &[("x-amzn-RequestId", "req-1")]));
    let converted = AwsServiceError::from_sdk_error(AwsService::SecretsManager, &sdk_error);
    assert!(!converted.is_client_error());
    assert_eq!(text(&expected, "toString"), Some(converted.to_string()));

    let expected = case("s3_access_denied");
    let error = aws_sdk_s3::operation::put_object::PutObjectError::generic(
        ErrorMetadata::builder()
            .code("AccessDenied")
            .message("Access Denied")
            .build(),
    );
    let sdk_error = SdkError::service_error(
        error,
        raw_response(
            403,
            &[("x-amz-request-id", "req-6"), ("x-amz-id-2", "ext-6")],
        ),
    );
    let converted = AwsServiceError::from_sdk_error(AwsService::S3, &sdk_error);
    assert_eq!(text(&expected, "toString"), Some(converted.to_string()));
}

#[test]
fn from_sdk_error_maps_failures_without_a_response_to_client_errors() {
    use aws_sdk_secretsmanager::operation::put_secret_value::PutSecretValueError;
    use aws_smithy_runtime_api::client::result::SdkError;

    let sdk_error: SdkError<
        PutSecretValueError,
        aws_smithy_runtime_api::client::orchestrator::HttpResponse,
    > = SdkError::timeout_error("connect timed out");
    let converted = AwsServiceError::from_sdk_error(AwsService::SecretsManager, &sdk_error);
    assert!(converted.is_client_error());
    assert!(!converted.is_service_exception_of(AwsService::SecretsManager));
    assert_eq!(None, converted.status_code());
    assert_eq!(
        Some("software.amazon.awssdk.core.exception.SdkClientException".to_string()),
        converted.java_class_name()
    );
}
