use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataRequestType {
    ListTables,
    ListSchemas,
    GetTable,
    GetTableLayout,
    GetSplits,
    GetDatasourceCapabilities,
}

impl MetadataRequestType {
    pub fn name(&self) -> &'static str {
        match self {
            MetadataRequestType::ListTables => "LIST_TABLES",
            MetadataRequestType::ListSchemas => "LIST_SCHEMAS",
            MetadataRequestType::GetTable => "GET_TABLE",
            MetadataRequestType::GetTableLayout => "GET_TABLE_LAYOUT",
            MetadataRequestType::GetSplits => "GET_SPLITS",
            MetadataRequestType::GetDatasourceCapabilities => "GET_DATASOURCE_CAPABILITIES",
        }
    }

    pub fn values() -> &'static [MetadataRequestType] {
        &[
            MetadataRequestType::ListTables,
            MetadataRequestType::ListSchemas,
            MetadataRequestType::GetTable,
            MetadataRequestType::GetTableLayout,
            MetadataRequestType::GetSplits,
            MetadataRequestType::GetDatasourceCapabilities,
        ]
    }

    pub fn value_of(name: &str) -> Option<MetadataRequestType> {
        MetadataRequestType::values()
            .iter()
            .copied()
            .find(|v| v.name() == name)
    }
}

impl fmt::Display for MetadataRequestType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
