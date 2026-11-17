#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubTypeProperties {
    property_name: String,
    property_values: Vec<String>,
}

impl SubTypeProperties {
    pub fn new(
        property_name: impl Into<String>,
        property_values: Vec<String>,
    ) -> SubTypeProperties {
        SubTypeProperties {
            property_name: property_name.into(),
            property_values,
        }
    }

    pub fn get_sub_type(&self) -> &str {
        &self.property_name
    }

    pub fn get_properties(&self) -> &[String] {
        &self.property_values
    }
}
