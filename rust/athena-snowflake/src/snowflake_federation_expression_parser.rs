use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::domain::predicate::expression::VariableExpression;
use athena_federation_sdk::domain::predicate::functions::FunctionName;
use athena_federation_sdk::Result;
use athena_jdbc::manager::{
    FederationExpressionParser, JdbcFederationExpressionParser, JdbcFederationExpressionParserExt,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnowflakeFederationExpressionParser {
    quote_char: String,
}

impl SnowflakeFederationExpressionParser {
    pub fn new(quote_char: impl Into<String>) -> SnowflakeFederationExpressionParser {
        SnowflakeFederationExpressionParser {
            quote_char: quote_char.into(),
        }
    }
}

impl FederationExpressionParser for SnowflakeFederationExpressionParser {
    fn map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
        arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String> {
        self.jdbc_map_function_to_data_source_syntax(function_name, arrow_type, arguments)
    }

    fn parse_variable_expression(&self, variable_expression: &VariableExpression) -> String {
        self.jdbc_parse_variable_expression(variable_expression)
    }
}

impl JdbcFederationExpressionParser for SnowflakeFederationExpressionParser {
    fn quote_char(&self) -> &str {
        &self.quote_char
    }

    fn write_array_constructor_clause(
        &self,
        _arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String> {
        Ok(arguments.join(", "))
    }
}
