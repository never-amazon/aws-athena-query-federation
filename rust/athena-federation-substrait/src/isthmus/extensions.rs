use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use crate::error::{Result, SubstraitError};
use crate::proto::extensions::simple_extension_declaration::MappingType;
use crate::proto::Plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FunctionKind {
    Scalar,
    Aggregate,
    Window,
}

impl FunctionKind {
    fn parse(kind: &str) -> Option<FunctionKind> {
        match kind {
            "scalar" => Some(FunctionKind::Scalar),
            "aggregate" => Some(FunctionKind::Aggregate),
            "window" => Some(FunctionKind::Window),
            _ => None,
        }
    }

    fn missing_key(self, key: &str, namespace: &str) -> String {
        match self {
            FunctionKind::Scalar => format!(
                "Unexpected scalar function with key {key}. The namespace {namespace} is loaded but no scalar function with this key found."
            ),
            FunctionKind::Aggregate => format!(
                "Unexpected aggregate function with key {key}. The namespace {namespace} is loaded but no aggregate function with this key was found."
            ),
            FunctionKind::Window => format!(
                "Unexpected window aggregate function with key {key}. The namespace {namespace} is loaded but no window aggregate function with this key was found."
            ),
        }
    }
}

struct DefaultCatalog {
    namespaces: HashSet<&'static str>,
    functions: HashSet<(FunctionKind, &'static str, &'static str)>,
}

static DEFAULT_CATALOG: LazyLock<DefaultCatalog> = LazyLock::new(|| {
    let mut namespaces = HashSet::new();
    let mut functions = HashSet::new();
    for line in include_str!("default_extensions.tsv").lines() {
        let mut parts = line.split('\t');
        if let (Some(kind), Some(namespace), Some(key)) = (
            parts.next().and_then(FunctionKind::parse),
            parts.next(),
            parts.next(),
        ) {
            namespaces.insert(namespace);
            functions.insert((kind, namespace, key));
        }
    }
    DefaultCatalog {
        namespaces,
        functions,
    }
});

struct Anchor {
    namespace: String,
    name: String,
}

pub(crate) struct Extensions {
    functions: HashMap<u32, Anchor>,
    types: HashMap<u32, Anchor>,
}

fn missing_uri(reference: u32) -> SubstraitError {
    SubstraitError::IllegalState(format!("Could not find extension URI of {reference}"))
}

fn not_loaded(namespace: &str) -> SubstraitError {
    SubstraitError::IllegalArgument(format!(
        "Received a reference for extension {namespace} but that extension is not currently loaded."
    ))
}

impl Extensions {
    #[allow(
        deprecated,
        reason = "Isthmus 0.65 reads extension URIs from the deprecated fields"
    )]
    pub(crate) fn from_plan(plan: &Plan) -> Result<Extensions> {
        let uris: HashMap<u32, &str> = plan
            .extension_uris
            .iter()
            .map(|u| (u.extension_uri_anchor, u.uri.as_str()))
            .collect();
        let anchor = |reference: u32, name: &str| -> Result<Anchor> {
            let namespace = uris.get(&reference).ok_or_else(|| missing_uri(reference))?;
            Ok(Anchor {
                namespace: (*namespace).to_string(),
                name: name.to_string(),
            })
        };
        let mut functions = HashMap::new();
        let mut types = HashMap::new();
        for extension in &plan.extensions {
            match &extension.mapping_type {
                Some(MappingType::ExtensionFunction(f)) => {
                    functions.insert(
                        f.function_anchor,
                        anchor(f.extension_uri_reference, &f.name)?,
                    );
                }
                Some(MappingType::ExtensionType(t)) => {
                    types.insert(t.type_anchor, anchor(t.extension_uri_reference, &t.name)?);
                }
                _ => {}
            }
        }
        Ok(Extensions { functions, types })
    }

    pub(crate) fn check_function(&self, kind: FunctionKind, reference: u32) -> Result<()> {
        let anchor = self.functions.get(&reference).ok_or_else(|| {
            SubstraitError::IllegalArgument("Unknown function id. Make sure that the function id provided was shared in the extensions section of the plan.".into())
        })?;
        let catalog = &*DEFAULT_CATALOG;
        let namespace = anchor.namespace.as_str();
        if catalog
            .functions
            .contains(&(kind, namespace, anchor.name.as_str()))
        {
            return Ok(());
        }
        if catalog.namespaces.contains(namespace) {
            return Err(SubstraitError::IllegalArgument(
                kind.missing_key(&anchor.name, namespace),
            ));
        }
        Err(not_loaded(namespace))
    }

    pub(crate) fn user_defined_type_error(&self, reference: u32) -> SubstraitError {
        let Some(anchor) = self.types.get(&reference) else {
            return SubstraitError::IllegalArgument("Unknown type id. Make sure that the type id provided was shared in the extensions section of the plan.".into());
        };
        if DEFAULT_CATALOG
            .namespaces
            .contains(anchor.namespace.as_str())
        {
            return SubstraitError::IllegalArgument(format!(
                "Unexpected type with name {}. The namespace {} is loaded but no type with this name found.",
                anchor.name, anchor.namespace
            ));
        }
        not_loaded(&anchor.namespace)
    }
}
