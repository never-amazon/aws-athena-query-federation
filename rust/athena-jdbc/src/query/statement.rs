use std::fmt;

use athena_federation_sdk::data::{arrow_type_to_string, ArrowType, Value};
use athena_federation_sdk::{FederationError, Result};

use crate::sql::{JdbcConnection, PreparedStatement};
use crate::validate::{not_blank, null_pointer, runtime_with_cause};

const PARAMETERS_MESSAGE: &str = "parameters must not be null";
const QUERY_MESSAGE: &str = "query must not be blank";
const CONNECTION_MESSAGE: &str = "connection must not be null";

#[derive(Default)]
pub struct PreparedStatementBuilder<'a> {
    query: Option<String>,
    parameters: Option<Vec<String>>,
    connection: Option<&'a dyn JdbcConnection>,
}

impl fmt::Debug for PreparedStatementBuilder<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedStatementBuilder")
            .field("query", &self.query)
            .field("parameters", &self.parameters)
            .finish_non_exhaustive()
    }
}

fn not_empty(parameters: Option<&[String]>) -> Result<&[String]> {
    match parameters {
        None => Err(FederationError::null_pointer(PARAMETERS_MESSAGE)),
        Some([]) => Err(FederationError::IllegalArgument(
            PARAMETERS_MESSAGE.to_string(),
        )),
        Some(p) => Ok(p),
    }
}

impl<'a> PreparedStatementBuilder<'a> {
    pub fn new() -> PreparedStatementBuilder<'a> {
        PreparedStatementBuilder::default()
    }

    pub fn with_parameters(
        mut self,
        parameters: Vec<String>,
    ) -> Result<PreparedStatementBuilder<'a>> {
        not_empty(Some(parameters.as_slice()))?;
        self.parameters = Some(parameters);
        Ok(self)
    }

    pub fn with_query(mut self, query: &str) -> Result<PreparedStatementBuilder<'a>> {
        self.query = Some(not_blank(Some(query), QUERY_MESSAGE)?.to_string());
        Ok(self)
    }

    #[must_use]
    pub fn with_connection(
        mut self,
        connection: &'a dyn JdbcConnection,
    ) -> PreparedStatementBuilder<'a> {
        self.connection = Some(connection);
        self
    }

    pub async fn build(&self) -> Result<PreparedStatement> {
        let parameters = not_empty(self.parameters.as_deref())?;
        let query = not_blank(self.query.as_deref(), QUERY_MESSAGE)?;
        let connection = self
            .connection
            .ok_or_else(|| FederationError::null_pointer(CONNECTION_MESSAGE))?;
        tracing::info!("Running query {}", query);
        tracing::info!("Parameters {:?}", parameters);
        let prepared = async {
            let mut statement = connection.prepare_statement(query).await?;
            for parameter in parameters {
                statement.bind(parameter.as_str());
            }
            Ok(statement)
        }
        .await;
        prepared.map_err(|e: crate::sql::SqlError| {
            let message = format!("{}: {}", e.error_code(), e.message());
            runtime_with_cause(message, FederationError::from(e))
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAndValue {
    arrow_type: ArrowType,
    value: Value,
}

impl TypeAndValue {
    pub fn new(arrow_type: ArrowType, value: Value) -> Result<TypeAndValue> {
        if value.is_null() {
            return Err(null_pointer("value is null"));
        }
        Ok(TypeAndValue { arrow_type, value })
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl fmt::Display for TypeAndValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TypeAndValue{{type={}, value={}}}",
            arrow_type_to_string(&self.arrow_type),
            self.value.java_to_string()
        )
    }
}
