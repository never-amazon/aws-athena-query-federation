use std::collections::HashMap;
use std::fmt;

use crate::domain::spill::SpillLocation;
use crate::exceptions::{FederationError, Result};
use crate::security::EncryptionKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    spill_location: Option<SpillLocation>,
    encryption_key: Option<EncryptionKey>,
    properties: HashMap<String, String>,
}

fn missing(key: &str) -> FederationError {
    FederationError::Runtime(format!("Cannot parse null string: property {} not found", key))
}

impl Split {
    pub fn new(
        spill_location: Option<SpillLocation>,
        encryption_key: Option<EncryptionKey>,
        properties: HashMap<String, String>,
    ) -> Split {
        Split {
            spill_location,
            encryption_key,
            properties,
        }
    }

    pub fn get_property(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(|s| s.as_str())
    }

    pub fn get_property_as_int(&self, key: &str) -> Result<i32> {
        let v = self.get_property(key).ok_or_else(|| missing(key))?;
        v.parse::<i32>()
            .map_err(|_| FederationError::Runtime(format!("For input string: \"{}\"", v)))
    }

    pub fn get_property_as_long(&self, key: &str) -> Result<i64> {
        let v = self.get_property(key).ok_or_else(|| missing(key))?;
        v.parse::<i64>()
            .map_err(|_| FederationError::Runtime(format!("For input string: \"{}\"", v)))
    }

    pub fn get_property_as_double(&self, key: &str) -> Result<f64> {
        let v = self.get_property(key).ok_or_else(|| missing(key))?;
        v.trim()
            .parse::<f64>()
            .map_err(|_| FederationError::Runtime(format!("For input string: \"{}\"", v)))
    }

    pub fn get_properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    pub fn get_spill_location(&self) -> Option<&SpillLocation> {
        self.spill_location.as_ref()
    }

    pub fn get_encryption_key(&self) -> Option<&EncryptionKey> {
        self.encryption_key.as_ref()
    }

    pub fn new_builder(spill_location: Option<SpillLocation>, encryption_key: Option<EncryptionKey>) -> SplitBuilder {
        SplitBuilder::default()
            .with_spill_location(spill_location)
            .with_encryption_key(encryption_key)
    }
}

impl fmt::Display for Split {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = self
            .spill_location
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "null".to_string());
        let mut props: Vec<String> = self.properties.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        props.sort();
        write!(
            f,
            "Split{{spillLocation={}, encryptionKey=<hidden>, properties={{{}}}}}",
            loc,
            props.join(", ")
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct SplitBuilder {
    properties: HashMap<String, String>,
    spill_location: Option<SpillLocation>,
    encryption_key: Option<EncryptionKey>,
}

impl SplitBuilder {
    pub fn add(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }

    pub fn with_spill_location(mut self, val: Option<SpillLocation>) -> Self {
        self.spill_location = val;
        self
    }

    pub fn with_encryption_key(mut self, key: Option<EncryptionKey>) -> Self {
        self.encryption_key = key;
        self
    }

    pub fn apply_properties(mut self, input_properties: &HashMap<String, String>) -> Self {
        self.properties
            .extend(input_properties.iter().map(|(k, v)| (k.clone(), v.clone())));
        self
    }

    pub fn build(self) -> Split {
        Split {
            spill_location: self.spill_location,
            encryption_key: self.encryption_key,
            properties: self.properties,
        }
    }
}
