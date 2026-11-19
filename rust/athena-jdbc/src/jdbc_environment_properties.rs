use std::collections::HashMap;
use std::hash::BuildHasher;

use athena_federation_sdk::connection::environment_constants;

fn java_get<'a, S: BuildHasher>(properties: &'a HashMap<String, String, S>, key: &str) -> &'a str {
    properties.get(key).map_or("null", String::as_str)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionStringFormat {
    pub prefix: &'static str,
    pub database_separator: Option<&'static str>,
    pub params_separator: &'static str,
    pub delimiter: &'static str,
}

impl ConnectionStringFormat {
    pub const fn new(prefix: &'static str) -> ConnectionStringFormat {
        ConnectionStringFormat {
            prefix,
            database_separator: Some("/"),
            params_separator: "?",
            delimiter: "&",
        }
    }

    pub fn connection_properties_to_environment<S: BuildHasher>(
        &self,
        connection_properties: &HashMap<String, String, S>,
    ) -> HashMap<String, String> {
        let connection_string = format!(
            "{}{}:{}{}{}",
            self.prefix,
            java_get(connection_properties, environment_constants::HOST),
            java_get(connection_properties, environment_constants::PORT),
            self.database(connection_properties),
            self.jdbc_parameters(connection_properties)
        );
        HashMap::from([(
            environment_constants::DEFAULT.to_string(),
            connection_string,
        )])
    }

    pub fn database<S: BuildHasher>(
        &self,
        connection_properties: &HashMap<String, String, S>,
    ) -> String {
        self.database_separator
            .map_or_else(String::new, |separator| {
                format!(
                    "{separator}{}",
                    java_get(connection_properties, environment_constants::DATABASE)
                )
            })
    }

    pub fn jdbc_parameters<S: BuildHasher>(
        &self,
        connection_properties: &HashMap<String, String, S>,
    ) -> String {
        let mut params = format!(
            "{}{}",
            self.params_separator,
            connection_properties
                .get(environment_constants::JDBC_PARAMS)
                .map_or("", String::as_str)
        );
        if let Some(secret_name) = connection_properties.get(environment_constants::SECRET_NAME) {
            if connection_properties.contains_key(environment_constants::JDBC_PARAMS) {
                params.push_str(self.delimiter);
            }
            params.push_str("${");
            params.push_str(secret_name);
            params.push('}');
        }
        params
    }
}
