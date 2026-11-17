use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubstraitField {
    name: String,
    r#type: String,
    children: Vec<SubstraitField>,
}

impl SubstraitField {
    pub fn new(
        name: impl Into<String>,
        r#type: impl Into<String>,
        children: Option<Vec<SubstraitField>>,
    ) -> SubstraitField {
        SubstraitField {
            name: name.into(),
            r#type: r#type.into(),
            children: children.unwrap_or_default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn get_type(&self) -> &str {
        &self.r#type
    }

    pub fn children(&self) -> &[SubstraitField] {
        &self.children
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

impl fmt::Display for SubstraitField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SubstraitField{{name='{}', type='{}', children={}}}",
            self.name,
            self.r#type,
            self.children.len()
        )
    }
}
