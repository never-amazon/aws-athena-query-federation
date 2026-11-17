use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::domain::predicate::functions::FunctionName;
use athena_federation_sdk::{FederationError, Result};

use super::federation_expression_parser::FederationExpressionParser;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DefaultJdbcFederationExpressionParser;

impl FederationExpressionParser for DefaultJdbcFederationExpressionParser {
    fn map_function_to_data_source_syntax(
        &self,
        _function_name: &FunctionName,
        _arrow_type: &ArrowType,
        _arguments: &[String],
    ) -> Result<String> {
        Err(FederationError::not_supported(
            "Subclass does not yet support complex expressions.",
        ))
    }
}
