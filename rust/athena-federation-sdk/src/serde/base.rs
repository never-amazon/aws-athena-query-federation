use std::collections::HashMap;

use indexmap::IndexMap;

use super::java_hash::java_hash_map_order;
use super::json::{token_name, JsonGenerator, JsonParser, JsonToken};
use crate::exceptions::{FederationError, Result};

pub const TYPE_FIELD: &str = "@type";

pub fn write_string_array<'s, V, I>(
    jgen: &mut JsonGenerator,
    field_name: &str,
    values: I,
) -> Result<()>
where
    V: StringMapValue + 's,
    I: IntoIterator<Item = &'s V>,
{
    jgen.write_array_field_start(field_name)?;
    for next in values {
        jgen.write_string(next.as_opt_str())?;
    }
    jgen.write_end_array()
}

pub trait StringMapValue {
    fn as_opt_str(&self) -> Option<&str>;
}

impl StringMapValue for String {
    fn as_opt_str(&self) -> Option<&str> {
        Some(self.as_str())
    }
}

impl StringMapValue for Option<String> {
    fn as_opt_str(&self) -> Option<&str> {
        self.as_deref()
    }
}

pub fn write_string_map<V: StringMapValue, S: std::hash::BuildHasher>(
    jgen: &mut JsonGenerator,
    field_name: &str,
    values: &HashMap<String, V, S>,
) -> Result<()> {
    jgen.write_object_field_start(field_name)?;
    for key in java_hash_map_order(values.keys()) {
        jgen.write_string_field(key, values.get(key).and_then(|v| v.as_opt_str()))?;
    }
    jgen.write_end_object()
}

pub fn write_ordered_string_map(
    jgen: &mut JsonGenerator,
    field_name: &str,
    values: &IndexMap<String, String>,
) -> Result<()> {
    jgen.write_object_field_start(field_name)?;
    for (k, v) in values {
        jgen.write_string_field(k, Some(v))?;
    }
    jgen.write_end_object()
}

pub fn write_type(jgen: &mut JsonGenerator, type_name: &str) -> Result<()> {
    jgen.write_string_field(TYPE_FIELD, Some(type_name))
}

pub fn assert_field_name(
    jparser: &mut JsonParser<'_>,
    expected_field_name: Option<&str>,
) -> Result<()> {
    jparser.next_token()?;
    if jparser.current_token() != Some(JsonToken::FieldName) {
        return Err(FederationError::UnexpectedToken(format!(
            "Expected field name token but got {}",
            token_name(jparser.current_token())
        )));
    }
    let field_name_token = jparser.current_name().unwrap_or_default();
    if let Some(expected) = expected_field_name {
        if expected != field_name_token {
            return Err(FederationError::UnexpectedToken(format!(
                "Unexpected field[{field_name_token}] while expecting[{expected}]"
            )));
        }
    }
    Ok(())
}

pub fn get_next_string_field(
    jparser: &mut JsonParser<'_>,
    expected_field_name: &str,
) -> Result<Option<String>> {
    assert_field_name(jparser, Some(expected_field_name))?;
    jparser.next_token()?;
    Ok(jparser.get_value_as_string())
}

pub fn get_next_bool_field(
    jparser: &mut JsonParser<'_>,
    expected_field_name: &str,
) -> Result<bool> {
    assert_field_name(jparser, Some(expected_field_name))?;
    jparser.next_token()?;
    jparser.get_value_as_boolean()
}

pub fn get_next_binary_field(
    jparser: &mut JsonParser<'_>,
    expected_field_name: &str,
) -> Result<Vec<u8>> {
    assert_field_name(jparser, Some(expected_field_name))?;
    jparser.next_token()?;
    jparser.get_binary_value()
}

pub fn get_next_int_field(jparser: &mut JsonParser<'_>, expected_field_name: &str) -> Result<i32> {
    assert_field_name(jparser, Some(expected_field_name))?;
    jparser.next_token()?;
    jparser.get_value_as_int()
}

pub fn get_next_long_field(jparser: &mut JsonParser<'_>, expected_field_name: &str) -> Result<i64> {
    assert_field_name(jparser, Some(expected_field_name))?;
    jparser.next_token()?;
    jparser.get_value_as_long()
}

pub fn get_next_string_array(
    jparser: &mut JsonParser<'_>,
    expected_field_name: &str,
) -> Result<Vec<Option<String>>> {
    assert_field_name(jparser, Some(expected_field_name))?;
    validate_array_start(jparser)?;
    let mut result = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => {
                return Err(FederationError::io(
                    "Unexpected end-of-input: expected close marker for ARRAY",
                ))
            }
            Some(_) => result.push(jparser.get_value_as_string()),
        }
    }
    Ok(result)
}

pub fn get_next_string_map(
    jparser: &mut JsonParser<'_>,
    expected_field_name: &str,
) -> Result<Vec<(String, Option<String>)>> {
    assert_field_name(jparser, Some(expected_field_name))?;
    validate_object_start(jparser)?;
    read_string_map_entries(jparser)
}

pub fn read_string_map_entries(
    jparser: &mut JsonParser<'_>,
) -> Result<Vec<(String, Option<String>)>> {
    let mut result = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndObject) => break,
            None => {
                return Err(FederationError::io(
                    "Unexpected end-of-input: expected close marker for OBJECT",
                ))
            }
            Some(_) => {
                let key = jparser.current_name().unwrap_or_default();
                result.push((key, jparser.get_value_as_string()));
            }
        }
    }
    Ok(result)
}

pub fn validate_object_start_token(token: Option<JsonToken>) -> Result<()> {
    match token {
        Some(JsonToken::StartObject) => Ok(()),
        Some(t) => Err(FederationError::UnexpectedToken(format!(
            "Expected {} found {}",
            JsonToken::StartObject,
            t.as_string().unwrap_or("null")
        ))),
        None => Err(FederationError::null_pointer("Cannot invoke \"com.fasterxml.jackson.core.JsonToken.asString()\" because \"token\" is null")),
    }
}

pub fn validate_object_end(jparser: &mut JsonParser<'_>) -> Result<()> {
    if jparser.next_token()? != Some(JsonToken::EndObject) {
        return Err(FederationError::UnexpectedToken(format!(
            "Expected {} found {}",
            JsonToken::EndObject,
            jparser.text_or_null()
        )));
    }
    Ok(())
}

pub fn validate_array_start(jparser: &mut JsonParser<'_>) -> Result<()> {
    if jparser.next_token()? != Some(JsonToken::StartArray) {
        return Err(FederationError::UnexpectedToken(format!(
            "Expected {} found {}",
            JsonToken::StartArray,
            jparser.text_or_null()
        )));
    }
    Ok(())
}

pub fn validate_object_start(jparser: &mut JsonParser<'_>) -> Result<()> {
    if jparser.next_token()? != Some(JsonToken::StartObject) {
        return Err(FederationError::UnexpectedToken(format!(
            "Expected {} found {}",
            JsonToken::StartObject,
            jparser.text_or_null()
        )));
    }
    Ok(())
}

pub fn get_type(jparser: &mut JsonParser<'_>) -> Result<Option<String>> {
    assert_field_name(jparser, Some(TYPE_FIELD))?;
    jparser.next_token()?;
    Ok(jparser.get_value_as_string())
}

pub fn ignore_rest_of_object(jparser: &mut JsonParser<'_>) -> Result<()> {
    if jparser.current_token() == Some(JsonToken::EndObject) {
        return Ok(());
    }
    let mut open = 1;
    loop {
        let t = jparser.next_token()?;
        match t {
            None => {
                return Err(FederationError::UnexpectedToken(format!(
                    "Expected {} found {}",
                    JsonToken::EndObject,
                    jparser.text_or_null()
                )))
            }
            Some(t) if t.is_struct_start() => open += 1,
            Some(JsonToken::EndObject) => {
                open -= 1;
                if open == 0 {
                    return Ok(());
                }
            }
            Some(_) => {}
        }
    }
}

pub fn deserialize_nullable<T, F>(
    jparser: &mut JsonParser<'_>,
    do_deserialize: F,
) -> Result<Option<T>>
where
    F: FnOnce(&mut JsonParser<'_>) -> Result<T>,
{
    if jparser.next_token()? == Some(JsonToken::ValueNull) {
        Ok(None)
    } else {
        validate_object_start_token(jparser.current_token())?;
        let result = do_deserialize(jparser)?;
        validate_object_end(jparser)?;
        Ok(Some(result))
    }
}

pub fn serialize_nullable<T, F>(
    jgen: &mut JsonGenerator,
    value: Option<&T>,
    do_serialize: F,
) -> Result<()>
where
    T: ?Sized,
    F: FnOnce(&T, &mut JsonGenerator) -> Result<()>,
{
    match value {
        Some(v) => {
            jgen.write_start_object()?;
            do_serialize(v, jgen)?;
            jgen.write_end_object()
        }
        None => jgen.write_null(),
    }
}

pub fn serialize_object<F>(jgen: &mut JsonGenerator, do_serialize: F) -> Result<()>
where
    F: FnOnce(&mut JsonGenerator) -> Result<()>,
{
    jgen.write_start_object()?;
    do_serialize(jgen)?;
    jgen.write_end_object()
}

pub fn require_non_null<T>(value: Option<T>, message: &str) -> Result<T> {
    value.ok_or_else(|| FederationError::null_pointer(message.to_string()))
}

pub fn no_enum_constant(enum_class: &str, name: Option<&str>) -> FederationError {
    match name {
        Some(n) => FederationError::IllegalArgument(format!("No enum constant {enum_class}.{n}")),
        None => FederationError::null_pointer("Name is null"),
    }
}

pub fn parse_java_long(value: Option<&str>) -> Result<i64> {
    match value {
        None => Err(FederationError::NumberFormat(
            "Cannot parse null string: null".into(),
        )),
        Some(s) => {
            let valid = {
                let body = s
                    .strip_prefix('-')
                    .or_else(|| s.strip_prefix('+'))
                    .unwrap_or(s);
                !body.is_empty() && body.chars().all(|c| c.is_ascii_digit())
            };
            if !valid {
                return Err(FederationError::NumberFormat(format!(
                    "For input string: \"{s}\""
                )));
            }
            s.parse::<i64>()
                .map_err(|_| FederationError::NumberFormat(format!("For input string: \"{s}\"")))
        }
    }
}

pub fn no_serde_configured(type_name: &str) -> FederationError {
    FederationError::invalid_input(format!("No SerDe configured for {type_name}"))
}

pub fn non_null_strings(values: Vec<Option<String>>, message: &str) -> Result<Vec<String>> {
    values
        .into_iter()
        .map(|v| v.ok_or_else(|| FederationError::null_pointer(message.to_string())))
        .collect()
}

pub fn nullable_map(entries: Vec<(String, Option<String>)>) -> HashMap<String, Option<String>> {
    entries.into_iter().collect()
}
