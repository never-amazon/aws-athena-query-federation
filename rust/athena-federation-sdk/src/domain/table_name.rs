use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TableName {
    schema_name: String,
    table_name: String,
}

impl TableName {
    pub fn new(schema_name: impl Into<String>, table_name: impl Into<String>) -> TableName {
        TableName {
            schema_name: schema_name.into(),
            table_name: table_name.into(),
        }
    }

    pub fn get_schema_name(&self) -> &str {
        &self.schema_name
    }

    pub fn get_table_name(&self) -> &str {
        &self.table_name
    }

    pub fn get_qualified_table_name(&self) -> String {
        format!("{}.{}", self.schema_name, self.table_name)
    }
}

impl fmt::Display for TableName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TableName{{schemaName={}, tableName={}}}",
            self.schema_name, self.table_name
        )
    }
}
