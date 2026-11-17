use arrow::datatypes::{FieldRef, Metadata, Schema};

pub trait SchemaAware {
    fn internal_get_schema(&self) -> &Schema;

    fn get_fields(&self) -> Vec<FieldRef> {
        self.internal_get_schema().fields().iter().cloned().collect()
    }

    fn get_meta_data(&self, key: &str) -> Option<&str> {
        self.internal_get_schema().metadata().get(key).map(|s| s.as_str())
    }

    fn get_meta_data_map(&self) -> &Metadata {
        self.internal_get_schema().metadata()
    }
}
