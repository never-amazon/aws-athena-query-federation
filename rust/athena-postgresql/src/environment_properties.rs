use std::collections::HashMap;
use std::hash::BuildHasher;

use athena_jdbc::connection::ConnectionStringFormat;

pub const POSTGRESQL_CONNECTION_STRING: ConnectionStringFormat =
    ConnectionStringFormat::new("postgres://jdbc:postgresql://");

pub fn connection_properties_to_environment<S: BuildHasher>(
    connection_properties: &HashMap<String, String, S>,
) -> HashMap<String, String> {
    POSTGRESQL_CONNECTION_STRING.connection_properties_to_environment(connection_properties)
}
