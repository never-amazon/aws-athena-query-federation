use arrow::datatypes::FieldRef;
use athena_federation_sdk::data::Value;
use athena_federation_sdk::data::{minor_type_of, MinorType};

#[derive(Debug, Clone)]
pub struct CustomFieldVector {
    pub obj_list: Vec<Value>,
    pub field: FieldRef,
}

impl CustomFieldVector {
    pub fn new(field: FieldRef) -> CustomFieldVector {
        CustomFieldVector {
            obj_list: Vec::new(),
            field,
        }
    }

    pub fn add_value(&mut self, value: Value) {
        self.obj_list.push(value);
    }

    pub fn add(&mut self, child: Option<CustomFieldVector>) {
        let Some(child) = child else {
            self.obj_list.push(Value::Null);
            return;
        };
        match minor_type_of(self.field.data_type()) {
            MinorType::STRUCT => {
                let child_name = child.field.name().clone();
                for (i, v) in child.obj_list.into_iter().enumerate() {
                    if i >= self.obj_list.len() {
                        self.obj_list.push(Value::Map(Vec::new()));
                    }
                    if !v.is_null() {
                        if let Value::Map(entries) = &mut self.obj_list[i] {
                            entries.push((Value::String(child_name.clone()), v));
                        }
                    }
                }
            }
            _ => self.obj_list.push(Value::List(child.obj_list)),
        }
    }

    pub fn add_map(&mut self, keys: CustomFieldVector, values: CustomFieldVector) {
        assert_eq!(
            keys.obj_list.len(),
            values.obj_list.len(),
            "Mismatched key/value object sizes."
        );
        let mut entries: Vec<(Value, Value)> = Vec::new();
        for (k, v) in keys.obj_list.into_iter().zip(values.obj_list) {
            if let Some(slot) = entries.iter_mut().find(|(ek, _)| *ek == k) {
                slot.1 = v;
            } else {
                entries.push((k, v));
            }
        }
        self.obj_list.push(Value::Map(entries));
    }
}
