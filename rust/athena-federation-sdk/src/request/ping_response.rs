use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResponse {
    catalog_name: String,
    query_id: String,
    source_type: Option<String>,
    capabilities: i32,
    ser_de_version: i32,
}

impl PingResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        query_id: impl Into<String>,
        source_type: Option<String>,
        capabilities: i32,
        ser_de_version: i32,
    ) -> PingResponse {
        PingResponse {
            catalog_name: catalog_name.into(),
            query_id: query_id.into(),
            source_type,
            capabilities,
            ser_de_version: if ser_de_version != 0 { ser_de_version } else { 1 },
        }
    }

    pub fn get_catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn get_query_id(&self) -> &str {
        &self.query_id
    }

    pub fn get_source_type(&self) -> Option<&str> {
        self.source_type.as_deref()
    }

    pub fn get_capabilities(&self) -> i32 {
        self.capabilities
    }

    pub fn get_ser_de_version(&self) -> i32 {
        self.ser_de_version
    }
}

impl fmt::Display for PingResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PingRequest{{catalogName='{}', queryId='{}', sourceType='{}', capabilities='{}', serDeVersion='{}'}}",
            self.catalog_name,
            self.query_id,
            self.source_type.as_deref().unwrap_or("null"),
            self.capabilities,
            self.ser_de_version
        )
    }
}
