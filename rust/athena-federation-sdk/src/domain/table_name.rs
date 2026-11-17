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

    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }

    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    pub fn java_hash_code(&self) -> i32 {
        let schema = crate::data::java_format::java_string_hash(&self.schema_name);
        let table = crate::data::java_format::java_string_hash(&self.table_name);
        31i32
            .wrapping_add(schema)
            .wrapping_mul(31)
            .wrapping_add(table)
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
