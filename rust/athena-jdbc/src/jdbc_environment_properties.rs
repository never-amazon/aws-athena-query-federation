use std::collections::HashMap;

use async_trait::async_trait;

use athena_federation_sdk::connection::{environment_constants, EnvironmentProperties};
use athena_federation_sdk::Result;

fn java_get<'a>(properties: &'a HashMap<String, String>, key: &str) -> &'a str {
    properties.get(key).map_or("null", String::as_str)
}

#[async_trait]
pub trait JdbcEnvironmentProperties: Send + Sync {
    fn environment_properties(&self) -> &EnvironmentProperties;

    fn get_connection_string_prefix(
        &self,
        connection_properties: &HashMap<String, String>,
    ) -> String;

    fn connection_properties_to_environment(
        &self,
        connection_properties: HashMap<String, String>,
    ) -> HashMap<String, String> {
        let connection_string = format!(
            "{}{}:{}{}{}",
            self.get_connection_string_prefix(&connection_properties),
            java_get(&connection_properties, environment_constants::HOST),
            java_get(&connection_properties, environment_constants::PORT),
            self.get_database(&connection_properties),
            self.get_jdbc_parameters(&connection_properties)
        );
        HashMap::from([(
            environment_constants::DEFAULT.to_string(),
            connection_string,
        )])
    }

    fn get_database(&self, connection_properties: &HashMap<String, String>) -> String {
        format!(
            "{}{}",
            self.get_database_separator(),
            java_get(connection_properties, environment_constants::DATABASE)
        )
    }

    fn get_jdbc_parameters(&self, connection_properties: &HashMap<String, String>) -> String {
        let mut params = format!(
            "{}{}",
            self.get_jdbc_parameters_separator(),
            connection_properties
                .get(environment_constants::JDBC_PARAMS)
                .map_or("", String::as_str)
        );
        if let Some(secret_name) = connection_properties.get(environment_constants::SECRET_NAME) {
            if connection_properties.contains_key(environment_constants::JDBC_PARAMS) {
                params.push_str(&self.get_delimiter());
            }
            params.push_str("${");
            params.push_str(secret_name);
            params.push('}');
        }
        params
    }

    fn get_database_separator(&self) -> String {
        "/".to_string()
    }

    fn get_jdbc_parameters_separator(&self) -> String {
        "?".to_string()
    }

    fn get_delimiter(&self) -> String {
        "&".to_string()
    }

    async fn create_environment(&self) -> Result<HashMap<String, String>> {
        self.environment_properties()
            .create_environment_with(|properties| {
                self.connection_properties_to_environment(properties)
            })
            .await
    }
}
