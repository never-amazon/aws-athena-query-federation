use athena_federation_sdk::Result;

use crate::validate::not_blank;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DatabaseConnectionInfo {
    driver_class_name: String,
    default_port: i32,
}

impl DatabaseConnectionInfo {
    pub fn new(driver_class_name: &str, default_port: i32) -> Result<DatabaseConnectionInfo> {
        Ok(DatabaseConnectionInfo {
            driver_class_name: not_blank(
                Some(driver_class_name),
                "driverClassName must not be blank",
            )?
            .to_string(),
            default_port,
        })
    }

    pub fn driver_class_name(&self) -> &str {
        &self.driver_class_name
    }

    pub fn default_port(&self) -> i32 {
        self.default_port
    }
}
