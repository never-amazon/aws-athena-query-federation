use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, FieldRef, Schema};
use indexmap::IndexMap;

use crate::data::types::{decimal, new_field, nullable_field, with_children, ArrowType};

#[derive(Debug, Clone, Default)]
pub struct SchemaBuilder {
    fields: IndexMap<String, FieldRef>,
    metadata: IndexMap<String, String>,
    nested_field_builder_map: IndexMap<String, FieldBuilder>,
}

impl SchemaBuilder {
    pub fn new() -> SchemaBuilder {
        SchemaBuilder::default()
    }

    pub fn new_builder() -> SchemaBuilder {
        SchemaBuilder::default()
    }

    pub fn add_field(&mut self, field: impl Into<FieldRef>) -> &mut Self {
        let field = field.into();
        self.fields.insert(field.name().clone(), field);
        self
    }

    pub fn add_field_with_type(&mut self, field_name: &str, arrow_type: ArrowType) -> &mut Self {
        self.fields.insert(
            field_name.to_string(),
            Arc::new(Field::new(field_name, arrow_type, true)),
        );
        self
    }

    pub fn add_field_with_children(
        &mut self,
        field_name: &str,
        arrow_type: ArrowType,
        children: Vec<FieldRef>,
    ) -> &mut Self {
        self.fields.insert(
            field_name.to_string(),
            Arc::new(nullable_field(field_name, arrow_type, Some(children))),
        );
        self
    }

    pub fn add_struct_field(&mut self, field_name: &str) -> &mut Self {
        self.nested_field_builder_map.insert(
            field_name.to_string(),
            FieldBuilder::new_builder(
                field_name,
                DataType::Struct(arrow::datatypes::Fields::default()),
            ),
        );
        self
    }

    pub fn add_list_field(&mut self, field_name: &str, arrow_type: ArrowType) -> &mut Self {
        let child = Field::new("", arrow_type, true);
        self.fields.insert(
            field_name.to_string(),
            Arc::new(Field::new(
                field_name,
                DataType::List(Arc::new(child)),
                true,
            )),
        );
        self
    }

    pub fn add_child_field(
        &mut self,
        parent: &str,
        child: &str,
        arrow_type: ArrowType,
    ) -> &mut Self {
        if let Some(b) = self.nested_field_builder_map.get_mut(parent) {
            b.add_field_with_type(child, arrow_type, None);
        }
        self
    }

    pub fn add_child(&mut self, parent: &str, child: impl Into<FieldRef>) -> &mut Self {
        let child = child.into();
        if let Some(b) = self.nested_field_builder_map.get_mut(parent) {
            b.add_field(child);
        }
        self
    }

    fn put(&mut self, field_name: &str, arrow_type: ArrowType) -> &mut Self {
        self.fields.insert(
            field_name.to_string(),
            Arc::new(Field::new(field_name, arrow_type, true)),
        );
        self
    }

    pub fn add_string_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Utf8)
    }

    pub fn add_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int32)
    }

    pub fn add_tiny_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int8)
    }

    pub fn add_small_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int16)
    }

    pub fn add_float8_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Float64)
    }

    pub fn add_float4_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Float32)
    }

    pub fn add_big_int_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Int64)
    }

    pub fn add_bit_field(&mut self, field_name: &str) -> &mut Self {
        self.put(field_name, DataType::Boolean)
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

    pub fn add_metadata(&mut self, key: &str, value: &str) -> &mut Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    pub fn get_field(&self, name: &str) -> Option<&FieldRef> {
        self.fields.get(name)
    }

    pub fn get_nested_field(&self, name: &str) -> Option<&FieldBuilder> {
        self.nested_field_builder_map.get(name)
    }

    pub fn build(&self) -> Schema {
        let mut fields = self.fields.clone();
        for (name, builder) in &self.nested_field_builder_map {
            fields.insert(name.clone(), Arc::new(builder.build()));
        }
        let metadata: HashMap<String, String> = self.metadata.clone().into_iter().collect();
        Schema::new_with_metadata(fields.into_values().collect::<Vec<_>>(), metadata)
    }
}

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
