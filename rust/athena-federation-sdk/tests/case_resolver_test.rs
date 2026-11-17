use std::collections::HashMap;

use athena_federation_sdk::resolver::{
    CaseResolver, FederationSdkCasingMode, CASING_MODE_CONFIGURATION_KEY,
};
use athena_federation_sdk::FederationError;

const DEFAULT_GLUE_CONNECTION: &str = "default_glue_connection";

fn test_case_resolver() -> CaseResolver {
    CaseResolver::new(
        "TestSource",
        FederationSdkCasingMode::Lower,
        FederationSdkCasingMode::Upper,
    )
}

#[test]
fn test_get_casing_mode_no_config_key_defaults_to_non_glue_based() {
    let case_resolver = test_case_resolver();
    let config_options = HashMap::new();
    assert_eq!(
        FederationSdkCasingMode::Lower,
        case_resolver.get_casing_mode(&config_options).unwrap()
    );
}

#[test]
fn test_get_casing_mode_with_glue_connection_defaults_to_glue_based() {
    let case_resolver = test_case_resolver();
    let mut config_options = HashMap::new();
    config_options.insert(DEFAULT_GLUE_CONNECTION.to_string(), "true".to_string());
    assert_eq!(
        FederationSdkCasingMode::Lower,
        case_resolver.get_casing_mode(&config_options).unwrap()
    );
}

#[test]
fn test_get_casing_mode_valid_config() {
    let case_resolver = test_case_resolver();
    let mut config_options = HashMap::new();
    config_options.insert(
        CASING_MODE_CONFIGURATION_KEY.to_string(),
        "ANNOTATION".to_string(),
    );
    assert_eq!(
        FederationSdkCasingMode::Annotation,
        case_resolver.get_casing_mode(&config_options).unwrap()
    );
}

#[test]
fn test_get_casing_mode_invalid_config() {
    let case_resolver = test_case_resolver();
    let mut config_options = HashMap::new();
    config_options.insert(
        CASING_MODE_CONFIGURATION_KEY.to_string(),
        "INVALID_MODE".to_string(),
    );
    let err = case_resolver.get_casing_mode(&config_options).unwrap_err();
    assert!(matches!(err, FederationError::IllegalArgument(_)));
    assert!(err.to_string().contains("INVALID_MODE"));
}
