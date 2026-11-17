use std::collections::HashMap;

use athena_federation_sdk::connection::EnvironmentProperties;
use athena_jdbc::JdbcEnvironmentProperties;

const CONNECTION_STRING_PREFIX: &str = "postgres://jdbc:postgresql://";

#[derive(Clone, Default)]
pub struct PostGreSqlEnvironmentProperties {
    environment_properties: EnvironmentProperties,
}

impl PostGreSqlEnvironmentProperties {
    pub fn new() -> PostGreSqlEnvironmentProperties {
        PostGreSqlEnvironmentProperties::default()
    }

    pub fn with_environment_properties(
        environment_properties: EnvironmentProperties,
    ) -> PostGreSqlEnvironmentProperties {
        PostGreSqlEnvironmentProperties {
            environment_properties,
        }
    }
}

impl JdbcEnvironmentProperties for PostGreSqlEnvironmentProperties {
    fn environment_properties(&self) -> &EnvironmentProperties {
        &self.environment_properties
    }

    fn get_connection_string_prefix(
        &self,
        _connection_properties: &HashMap<String, String>,
    ) -> String {
        CONNECTION_STRING_PREFIX.to_string()
    }
}
