use std::fmt;

use crate::exceptions::Result;
use crate::serde::base::validate_object_start_token;
use crate::serde::json::JsonParser;

const ERROR_TYPE_FIELD: &str = "errorType";
const ERROR_MESSAGE_FIELD: &str = "errorMessage";
const CAUSE_FIELD: &str = "cause";
const STACK_TRACE_FIELD: &str = "stackTrace";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaException {
    message: Option<String>,
    cause: Option<Box<LambdaException>>,
}

impl LambdaException {
    pub fn new(message: Option<String>, cause: Option<LambdaException>) -> LambdaException {
        LambdaException {
            message,
            cause: cause.map(Box::new),
        }
    }

    pub fn get_message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn cause(&self) -> Option<&LambdaException> {
        self.cause.as_deref()
    }
}

impl fmt::Display for LambdaException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LambdaException: {}",
            self.message.as_deref().unwrap_or("null")
        )
    }
}

impl std::error::Error for LambdaException {}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<LambdaException> {
    validate_object_start_token(jparser.current_token())?;
    do_deserialize(jparser)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<LambdaException> {
    let root = jparser.read_tree()?;
    Ok(recursive_parse(&root))
}

fn to_text(node: &serde_json::Value) -> String {
    match node {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => String::new(),
    }
}

fn get_nullable_string_value(parent: &serde_json::Value, field: &str) -> Option<String> {
    parent.get(field).map(to_text)
}

fn recursive_parse(root: &serde_json::Value) -> LambdaException {
    let error_type = get_nullable_string_value(root, ERROR_TYPE_FIELD);
    let error_message = get_nullable_string_value(root, ERROR_MESSAGE_FIELD);
    let cause = root.get(CAUSE_FIELD).map(recursive_parse);
    let mut stack_traces: Vec<Vec<String>> = Vec::new();
    if let Some(serde_json::Value::Array(elements)) = root.get(STACK_TRACE_FIELD) {
        for element in elements {
            let inner = match element {
                serde_json::Value::Array(inner) => inner.iter().map(to_text).collect(),
                other => vec![to_text(other)],
            };
            stack_traces.push(inner);
        }
    }
    let message = format!(
        "{}\nErrorType: {}",
        append_stack_trace(error_message.as_deref(), &stack_traces),
        error_type.as_deref().unwrap_or("null")
    );
    LambdaException::new(Some(message), cause)
}

fn append_stack_trace(error_message: Option<&str>, stack_traces: &[Vec<String>]) -> String {
    let rendered: Vec<String> = stack_traces
        .iter()
        .map(|inner| format!("[{}]", inner.join(", ")))
        .collect();
    format!(
        "{}. Stack trace: [{}]",
        error_message.unwrap_or("null"),
        rendered.join(", ")
    )
}
