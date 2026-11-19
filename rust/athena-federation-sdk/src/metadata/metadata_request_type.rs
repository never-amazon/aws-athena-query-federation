use crate::named_enum::named_enum;

named_enum! {
    pub enum MetadataRequestType {
        ListTables => "LIST_TABLES",
        ListSchemas => "LIST_SCHEMAS",
        GetTable => "GET_TABLE",
        GetTableLayout => "GET_TABLE_LAYOUT",
        GetSplits => "GET_SPLITS",
        GetDatasourceCapabilities => "GET_DATASOURCE_CAPABILITIES",
    }
}
