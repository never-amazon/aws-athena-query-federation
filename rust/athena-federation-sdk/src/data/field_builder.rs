use std::sync::Arc;

use arrow::datatypes::{DataType, Field, FieldRef};
use indexmap::IndexMap;

use super::types::{decimal, new_field, nullable_field, with_children, ArrowType};

#[derive(Debug, Clone)]
pub struct FieldBuilder {
    name: String,
    arrow_type: ArrowType,
    nullable: bool,
    children: IndexMap<String, FieldRef>,
}

impl FieldBuilder {
    pub fn new_builder(name: impl Into<String>, arrow_type: ArrowType) -> FieldBuilder {
        FieldBuilder::new_builder_nullable(name, arrow_type, true)
    }

    pub fn new_builder_nullable(
        name: impl Into<String>,
        arrow_type: ArrowType,
        nullable: bool,
    ) -> FieldBuilder {
        FieldBuilder {
            name: name.into(),
            arrow_type,
            nullable,
            children: IndexMap::new(),
        }
    }

    pub fn add_field_with_type(
        &mut self,
        field_name: &str,
        arrow_type: ArrowType,
        children: Option<Vec<FieldRef>>,
    ) -> &mut Self {
        self.children.insert(
            field_name.to_string(),
            Arc::new(nullable_field(field_name, arrow_type, children)),
        );
        self
    }

    pub fn add_field_with_nullable(
        &mut self,
        field_name: &str,
        arrow_type: ArrowType,
        nullable: bool,
        children: Option<Vec<FieldRef>>,
    ) -> &mut Self {
        self.children.insert(
            field_name.to_string(),
            Arc::new(new_field(field_name, nullable, arrow_type, children)),
        );
        self
    }

    pub fn add_field(&mut self, child: impl Into<FieldRef>) -> &mut Self {
        let child = child.into();
        self.children.insert(child.name().clone(), child);
        self
    }

    fn put(&mut self, field_name: &str, arrow_type: ArrowType) -> &mut Self {
        self.children.insert(
            field_name.to_string(),
            Arc::new(Field::new(field_name, arrow_type, true)),
        );
        self
    }

    pub fn add_string_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Utf8)
    }

    pub fn add_list_field(&mut self, field_name: &str, arrow_type: ArrowType) -> &mut Self {
        let base = Field::new("", arrow_type, true);
        let field = Field::new(field_name, DataType::List(Arc::new(base)), true);
        self.children
            .insert(field_name.to_string(), Arc::new(field));
        self
    }

    pub fn add_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int32)
    }

    pub fn add_float8_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Float64)
    }

    pub fn add_big_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int64)
    }

    pub fn add_bit_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Boolean)
    }

    pub fn add_tiny_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int8)
    }

    pub fn add_small_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int16)
    }

    pub fn add_float4_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Float32)
    }

    pub fn add_decimal_field(&mut self, field_name: &str, precision: u8, scale: i8) -> &mut Self {
        self.put(field_name, decimal(precision, scale))
    }

    pub fn add_date_day_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Date32)
    }

    pub fn add_date_milli_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Date64)
    }

    pub fn get_child(&self, field_name: &str) -> Option<&FieldRef> {
        self.children.get(field_name)
    }

    pub fn build(&self) -> Field {
        let children: Vec<FieldRef> = self.children.values().cloned().collect();
        let dt = if children.is_empty() {
            self.arrow_type.clone()
        } else {
            with_children(&self.arrow_type, children)
        };
        Field::new(self.name.clone(), dt, self.nullable)
    }
}
