use base64::Engine;

use crate::exceptions::{FederationError, Result};
use crate::java_cast::{double_to_int, double_to_long, long_to_double};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsonToken {
    StartObject,
    EndObject,
    StartArray,
    EndArray,
    FieldName,
    ValueEmbeddedObject,
    ValueString,
    ValueNumberInt,
    ValueNumberFloat,
    ValueTrue,
    ValueFalse,
    ValueNull,
}

impl JsonToken {
    pub fn as_str(self) -> &'static str {
        match self {
            JsonToken::StartObject => "START_OBJECT",
            JsonToken::EndObject => "END_OBJECT",
            JsonToken::StartArray => "START_ARRAY",
            JsonToken::EndArray => "END_ARRAY",
            JsonToken::FieldName => "FIELD_NAME",
            JsonToken::ValueEmbeddedObject => "VALUE_EMBEDDED_OBJECT",
            JsonToken::ValueString => "VALUE_STRING",
            JsonToken::ValueNumberInt => "VALUE_NUMBER_INT",
            JsonToken::ValueNumberFloat => "VALUE_NUMBER_FLOAT",
            JsonToken::ValueTrue => "VALUE_TRUE",
            JsonToken::ValueFalse => "VALUE_FALSE",
            JsonToken::ValueNull => "VALUE_NULL",
        }
    }

    pub fn as_string(&self) -> Option<&'static str> {
        match self {
            JsonToken::StartObject => Some("{"),
            JsonToken::EndObject => Some("}"),
            JsonToken::StartArray => Some("["),
            JsonToken::EndArray => Some("]"),
            JsonToken::ValueTrue => Some("true"),
            JsonToken::ValueFalse => Some("false"),
            JsonToken::ValueNull => Some("null"),
            _ => None,
        }
    }

    pub fn is_struct_start(&self) -> bool {
        matches!(self, JsonToken::StartObject | JsonToken::StartArray)
    }

    pub fn is_struct_end(&self) -> bool {
        matches!(self, JsonToken::EndObject | JsonToken::EndArray)
    }

    pub fn is_scalar_value(&self) -> bool {
        matches!(
            self,
            JsonToken::ValueEmbeddedObject
                | JsonToken::ValueString
                | JsonToken::ValueNumberInt
                | JsonToken::ValueNumberFloat
                | JsonToken::ValueTrue
                | JsonToken::ValueFalse
                | JsonToken::ValueNull
        )
    }
}

impl std::fmt::Display for JsonToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn token_name(token: Option<JsonToken>) -> &'static str {
    token.map_or("null", |t| t.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectState {
    FirstName,
    Colon,
    CommaOrEnd,
}

#[derive(Debug, Clone)]
enum ParseContext {
    Root {
        values: usize,
    },
    Array {
        values: usize,
    },
    Object {
        state: ObjectState,
        name: Option<String>,
    },
}

pub struct JsonParser<'a> {
    input: &'a [u8],
    pos: usize,
    contexts: Vec<ParseContext>,
    current: Option<JsonToken>,
    text: String,
    closed_name: Option<String>,
}

fn parse_error(message: impl Into<String>) -> FederationError {
    FederationError::io(message.into())
}

impl<'a> JsonParser<'a> {
    pub fn new(input: &'a [u8]) -> JsonParser<'a> {
        JsonParser {
            input,
            pos: 0,
            contexts: vec![ParseContext::Root { values: 0 }],
            current: None,
            text: String::new(),
            closed_name: None,
        }
    }

    pub fn current_token(&self) -> Option<JsonToken> {
        self.current
    }

    pub fn current_name(&self) -> Option<String> {
        match self.current {
            Some(JsonToken::FieldName) => Some(self.text.clone()),
            Some(JsonToken::StartObject | JsonToken::StartArray) => self.parent_name(),
            Some(JsonToken::EndObject | JsonToken::EndArray) => {
                self.closed_name.clone().or_else(|| self.context_name())
            }
            _ => self.context_name(),
        }
    }

    fn context_name(&self) -> Option<String> {
        match self.contexts.last() {
            Some(ParseContext::Object { name, .. }) => name.clone(),
            _ => None,
        }
    }

    fn parent_name(&self) -> Option<String> {
        if self.contexts.len() < 2 {
            return None;
        }
        match &self.contexts[self.contexts.len() - 2] {
            ParseContext::Object { name, .. } => name.clone(),
            _ => None,
        }
    }

    pub fn get_text(&self) -> Option<String> {
        match self.current {
            None => None,
            Some(
                JsonToken::FieldName
                | JsonToken::ValueString
                | JsonToken::ValueNumberInt
                | JsonToken::ValueNumberFloat,
            ) => Some(self.text.clone()),
            Some(t) => t.as_string().map(std::string::ToString::to_string),
        }
    }

    pub fn text_or_null(&self) -> String {
        self.get_text().unwrap_or_else(|| "null".to_string())
    }

    pub fn get_value_as_string(&self) -> Option<String> {
        match self.current {
            Some(JsonToken::ValueString) => Some(self.text.clone()),
            Some(JsonToken::FieldName) => self.current_name(),
            None | Some(JsonToken::ValueNull) => None,
            Some(t) if !t.is_scalar_value() => None,
            Some(_) => self.get_text(),
        }
    }

    pub fn get_value_as_int(&self) -> Result<i32> {
        match self.current {
            Some(JsonToken::ValueNumberInt) => {
                let v: i128 = self.text.parse().map_err(|_| {
                    parse_error(format!("Numeric value ({}) out of range of int", self.text))
                })?;
                i32::try_from(v).map_err(|_| {
                    parse_error(format!(
                        "Numeric value ({}) out of range of int ({} - {})",
                        self.text,
                        i32::MIN,
                        i32::MAX
                    ))
                })
            }
            Some(JsonToken::ValueNumberFloat) => {
                let d: f64 = self
                    .text
                    .parse()
                    .map_err(|_| parse_error(format!("Malformed number {}", self.text)))?;
                if d < f64::from(i32::MIN) || d > f64::from(i32::MAX) {
                    return Err(parse_error(format!(
                        "Numeric value ({}) out of range of int ({} - {})",
                        self.text,
                        i32::MIN,
                        i32::MAX
                    )));
                }
                Ok(double_to_int(d))
            }
            Some(JsonToken::ValueTrue) => Ok(1),
            Some(JsonToken::ValueString) => Ok(parse_as_int(&self.text, 0)),
            _ => Ok(0),
        }
    }

    pub fn get_value_as_long(&self) -> Result<i64> {
        match self.current {
            Some(JsonToken::ValueNumberInt) => {
                let v: i128 = self.text.parse().map_err(|_| {
                    parse_error(format!(
                        "Numeric value ({}) out of range of long",
                        self.text
                    ))
                })?;
                i64::try_from(v).map_err(|_| {
                    parse_error(format!(
                        "Numeric value ({}) out of range of long ({} - {})",
                        self.text,
                        i64::MIN,
                        i64::MAX
                    ))
                })
            }
            Some(JsonToken::ValueNumberFloat) => {
                let d: f64 = self
                    .text
                    .parse()
                    .map_err(|_| parse_error(format!("Malformed number {}", self.text)))?;
                if d < long_to_double(i64::MIN) || d > long_to_double(i64::MAX) {
                    return Err(parse_error(format!(
                        "Numeric value ({}) out of range of long ({} - {})",
                        self.text,
                        i64::MIN,
                        i64::MAX
                    )));
                }
                Ok(double_to_long(d))
            }
            Some(JsonToken::ValueTrue) => Ok(1),
            Some(JsonToken::ValueString) => Ok(parse_as_long(&self.text, 0)),
            _ => Ok(0),
        }
    }

    pub fn get_value_as_boolean(&self) -> Result<bool> {
        match self.current {
            Some(JsonToken::ValueNumberInt) => Ok(self.get_value_as_int()? != 0),
            Some(JsonToken::ValueTrue) => Ok(true),
            Some(JsonToken::ValueString) => {
                let s = self.text.trim();
                Ok(s == "true")
            }
            _ => Ok(false),
        }
    }

    pub fn get_binary_value(&self) -> Result<Vec<u8>> {
        if self.current != Some(JsonToken::ValueString) {
            return Err(parse_error(format!(
                "Current token ({}) not VALUE_STRING or VALUE_EMBEDDED_OBJECT, can not access as binary",
                token_name(self.current)
            )));
        }
        decode_base64(&self.text)
    }

    pub fn next_token(&mut self) -> Result<Option<JsonToken>> {
        self.closed_name = None;
        let token = self.advance()?;
        self.current = token;
        Ok(token)
    }

    fn advance(&mut self) -> Result<Option<JsonToken>> {
        let ctx_index = self.contexts.len() - 1;
        let ctx = self.contexts[ctx_index].clone();
        match ctx {
            ParseContext::Root { values } => {
                self.skip_ws();
                if self.pos >= self.input.len() {
                    return Ok(None);
                }
                self.contexts[ctx_index] = ParseContext::Root { values: values + 1 };
                self.parse_value(None).map(Some)
            }
            ParseContext::Array { values } => {
                self.skip_ws();
                let c = self.peek().ok_or_else(|| Self::eof_error("ARRAY"))?;
                if c == b']' {
                    self.pos += 1;
                    self.contexts.pop();
                    self.closed_name = self.context_name();
                    return Ok(Some(JsonToken::EndArray));
                }
                if values > 0 {
                    if c != b',' {
                        return Err(Self::unexpected_char(
                            c,
                            "was expecting comma to separate Array entries",
                        ));
                    }
                    self.pos += 1;
                    self.skip_ws();
                }
                self.contexts[ctx_index] = ParseContext::Array { values: values + 1 };
                self.parse_value(None).map(Some)
            }
            ParseContext::Object { state, name } => match state {
                ObjectState::Colon => {
                    self.skip_ws();
                    let c = self.peek().ok_or_else(|| Self::eof_error("OBJECT"))?;
                    if c != b':' {
                        return Err(Self::unexpected_char(
                            c,
                            "was expecting a colon to separate field name and value",
                        ));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    self.contexts[ctx_index] = ParseContext::Object {
                        state: ObjectState::CommaOrEnd,
                        name: name.clone(),
                    };
                    self.parse_value(name).map(Some)
                }
                ObjectState::FirstName | ObjectState::CommaOrEnd => {
                    self.skip_ws();
                    let c = self.peek().ok_or_else(|| Self::eof_error("OBJECT"))?;
                    if c == b'}' {
                        self.pos += 1;
                        self.contexts.pop();
                        self.closed_name = self.context_name();
                        return Ok(Some(JsonToken::EndObject));
                    }
                    if state == ObjectState::CommaOrEnd {
                        if c != b',' {
                            return Err(Self::unexpected_char(
                                c,
                                "was expecting comma to separate Object entries",
                            ));
                        }
                        self.pos += 1;
                        self.skip_ws();
                    }
                    let c = self.peek().ok_or_else(|| Self::eof_error("OBJECT"))?;
                    if c != b'"' {
                        return Err(Self::unexpected_char(
                            c,
                            "was expecting double-quote to start field name",
                        ));
                    }
                    self.pos += 1;
                    let field = self.parse_string_body()?;
                    self.text.clone_from(&field);
                    self.contexts[ctx_index] = ParseContext::Object {
                        state: ObjectState::Colon,
                        name: Some(field),
                    };
                    Ok(Some(JsonToken::FieldName))
                }
            },
        }
    }

    fn parse_value(&mut self, _name: Option<String>) -> Result<JsonToken> {
        let c = self.peek().ok_or_else(|| Self::eof_error("VALUE"))?;
        match c {
            b'{' => {
                self.pos += 1;
                self.contexts.push(ParseContext::Object {
                    state: ObjectState::FirstName,
                    name: None,
                });
                self.text = "{".to_string();
                Ok(JsonToken::StartObject)
            }
            b'[' => {
                self.pos += 1;
                self.contexts.push(ParseContext::Array { values: 0 });
                self.text = "[".to_string();
                Ok(JsonToken::StartArray)
            }
            b'"' => {
                self.pos += 1;
                self.text = self.parse_string_body()?;
                Ok(JsonToken::ValueString)
            }
            b't' => self.parse_literal("true", JsonToken::ValueTrue),
            b'f' => self.parse_literal("false", JsonToken::ValueFalse),
            b'n' => self.parse_literal("null", JsonToken::ValueNull),
            b'-' | b'0'..=b'9' => self.parse_number(),
            other => Err(Self::unexpected_char(
                other,
                "expected a valid value (JSON String, Number, Array, Object or token 'null', 'true' or 'false')",
            )),
        }
    }

    fn parse_literal(&mut self, literal: &str, token: JsonToken) -> Result<JsonToken> {
        let end = self.pos + literal.len();
        if end > self.input.len() || &self.input[self.pos..end] != literal.as_bytes() {
            let snippet: String =
                String::from_utf8_lossy(&self.input[self.pos..self.input.len().min(self.pos + 32)])
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect();
            return Err(parse_error(format!(
                "Unrecognized token '{snippet}': was expecting (JSON String, Number, Array, Object or token 'null', 'true' or 'false')"
            )));
        }
        if let Some(&next) = self.input.get(end) {
            if next.is_ascii_alphanumeric() || next == b'_' {
                return Err(parse_error(format!(
                    "Unrecognized token '{}{}': was expecting (JSON String, Number, Array, Object or token 'null', 'true' or 'false')",
                    literal, next as char
                )));
            }
        }
        self.pos = end;
        self.text = literal.to_string();
        Ok(token)
    }

    fn parse_number(&mut self) -> Result<JsonToken> {
        let start = self.pos;
        let mut is_float = false;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let int_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        let int_len = self.pos - int_start;
        if int_len == 0 {
            return Err(parse_error(
                "Unexpected character: expected digit after minus sign",
            ));
        }
        if int_len > 1 && self.input[int_start] == b'0' {
            return Err(parse_error(
                "Invalid numeric value: Leading zeroes not allowed",
            ));
        }
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            let frac_start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.pos == frac_start {
                return Err(parse_error(
                    "Unexpected character: Decimal point not followed by a digit",
                ));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if self.pos == exp_start {
                return Err(parse_error(
                    "Unexpected character: Exponent indicator not followed by a digit",
                ));
            }
        }
        self.text = String::from_utf8_lossy(&self.input[start..self.pos]).into_owned();
        Ok(if is_float {
            JsonToken::ValueNumberFloat
        } else {
            JsonToken::ValueNumberInt
        })
    }

    fn parse_string_body(&mut self) -> Result<String> {
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(parse_error(
                    "Unexpected end-of-input: was expecting closing quote for a string value",
                ));
            };
            self.pos += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self
                        .peek()
                        .ok_or_else(|| parse_error("Unexpected end-of-input in character escape sequence"))?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let first = self.parse_hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&first) {
                                if self.peek() == Some(b'\\') && self.input.get(self.pos + 1) == Some(&b'u') {
                                    self.pos += 2;
                                    let second = self.parse_hex4()?;
                                    if (0xDC00..0xE000).contains(&second) {
                                        let combined = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                                        char::from_u32(combined).unwrap_or('\u{FFFD}')
                                    } else {
                                        '\u{FFFD}'
                                    }
                                } else {
                                    '\u{FFFD}'
                                }
                            } else {
                                char::from_u32(first).unwrap_or('\u{FFFD}')
                            };
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => {
                            return Err(parse_error(format!(
                                "Unrecognized character escape '{}' (code {})",
                                other as char, other
                            )))
                        }
                    }
                }
                c if c < 0x20 => {
                    return Err(parse_error(format!(
                        "Illegal unquoted character ((CTRL-CHAR, code {c})): has to be escaped using backslash to be included in string value"
                    )))
                }
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|e| parse_error(format!("Invalid UTF-8 middle byte: {e}")))
    }

    fn parse_hex4(&mut self) -> Result<u32> {
        let end = self.pos + 4;
        if end > self.input.len() {
            return Err(parse_error(
                "Unexpected end-of-input in character escape sequence",
            ));
        }
        let s = std::str::from_utf8(&self.input[self.pos..end])
            .map_err(|_| parse_error("Unexpected character in hex escape"))?;
        let v = u32::from_str_radix(s, 16).map_err(|_| {
            parse_error("Unexpected character: expected a hex-digit for character escape sequence")
        })?;
        self.pos = end;
        Ok(v)
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\n' || c == b'\r' || c == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn eof_error(what: &str) -> FederationError {
        parse_error(format!(
            "Unexpected end-of-input: expected close marker for {what}"
        ))
    }

    fn unexpected_char(c: u8, detail: &str) -> FederationError {
        parse_error(format!(
            "Unexpected character ('{}' (code {})): {}",
            c as char, c, detail
        ))
    }

    pub fn read_tree(&mut self) -> Result<serde_json::Value> {
        match self.current {
            Some(JsonToken::StartObject) => {
                let mut map = serde_json::Map::new();
                loop {
                    match self.next_token()? {
                        Some(JsonToken::EndObject) => break,
                        Some(JsonToken::FieldName) => {
                            let key = self.text.clone();
                            self.next_token()?;
                            let value = self.read_tree()?;
                            map.insert(key, value);
                        }
                        other => {
                            return Err(parse_error(format!(
                                "Unexpected token {} in object",
                                token_name(other)
                            )))
                        }
                    }
                }
                Ok(serde_json::Value::Object(map))
            }
            Some(JsonToken::StartArray) => {
                let mut items = Vec::new();
                loop {
                    match self.next_token()? {
                        Some(JsonToken::EndArray) => break,
                        Some(_) => items.push(self.read_tree()?),
                        None => return Err(Self::eof_error("ARRAY")),
                    }
                }
                Ok(serde_json::Value::Array(items))
            }
            Some(JsonToken::ValueString) => Ok(serde_json::Value::String(self.text.clone())),
            Some(JsonToken::ValueNumberInt | JsonToken::ValueNumberFloat) => {
                serde_json::from_str(&self.text).map_err(|e| parse_error(e.to_string()))
            }
            Some(JsonToken::ValueTrue) => Ok(serde_json::Value::Bool(true)),
            Some(JsonToken::ValueFalse) => Ok(serde_json::Value::Bool(false)),
            Some(JsonToken::ValueNull) | None => Ok(serde_json::Value::Null),
            Some(other) => Err(parse_error(format!("Unexpected token {other}"))),
        }
    }

    pub fn skip_children(&mut self) -> Result<()> {
        match self.current {
            Some(JsonToken::StartObject | JsonToken::StartArray) => {
                let mut open = 1;
                while open > 0 {
                    match self.next_token()? {
                        Some(t) if t.is_struct_start() => open += 1,
                        Some(t) if t.is_struct_end() => open -= 1,
                        Some(_) => {}
                        None => return Err(Self::eof_error("OBJECT")),
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

pub fn parse_as_int(s: &str, default: i32) -> i32 {
    let s = s.trim();
    if s.is_empty() {
        return default;
    }
    let body = s.strip_prefix('+').unwrap_or(s);
    let digits = body.strip_prefix('-').unwrap_or(body);
    if digits.chars().any(|c| !c.is_ascii_digit()) {
        return match body.parse::<f64>() {
            Ok(d) if d.is_finite() => double_to_int(d),
            _ => default,
        };
    }
    body.parse::<i32>().unwrap_or(default)
}

pub fn parse_as_long(s: &str, default: i64) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return default;
    }
    let body = s.strip_prefix('+').unwrap_or(s);
    let digits = body.strip_prefix('-').unwrap_or(body);
    if digits.chars().any(|c| !c.is_ascii_digit()) {
        return match body.parse::<f64>() {
            Ok(d) if d.is_finite() => double_to_long(d),
            _ => default,
        };
    }
    body.parse::<i64>().unwrap_or(default)
}

pub fn decode_base64(text: &str) -> Result<Vec<u8>> {
    let cleaned: String = text.chars().filter(|c| *c as u32 > 0x20).collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| {
            parse_error(format!(
                "Failed to decode VALUE_STRING as base64 (MIME-NO-LINEFEEDS): {e}"
            ))
        })
}

pub fn encode_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenKind {
    Root,
    Object,
    Array,
}

#[derive(Debug, Clone)]
struct GenContext {
    kind: GenKind,
    count: usize,
    expect_value: bool,
}

pub struct JsonGenerator {
    out: String,
    pretty: bool,
    contexts: Vec<GenContext>,
    nesting: usize,
}

impl JsonGenerator {
    pub fn new(pretty: bool) -> JsonGenerator {
        JsonGenerator {
            out: String::new(),
            pretty,
            contexts: vec![GenContext {
                kind: GenKind::Root,
                count: 0,
                expect_value: false,
            }],
            nesting: 0,
        }
    }

    pub fn into_string(self) -> String {
        self.out
    }

    pub fn as_str(&self) -> &str {
        &self.out
    }

    fn indent(&mut self) {
        if self.pretty {
            self.out.push('\n');
            for _ in 0..self.nesting {
                self.out.push_str("  ");
            }
        }
    }

    fn before_value(&mut self, what: &str) -> Result<()> {
        let pretty = self.pretty;
        let ctx = self
            .contexts
            .last_mut()
            .ok_or_else(|| FederationError::io("Generator closed"))?;
        match ctx.kind {
            GenKind::Root => {
                if ctx.count > 0 {
                    self.out.push(' ');
                }
                ctx.count += 1;
            }
            GenKind::Array => {
                if ctx.count > 0 {
                    self.out.push(',');
                }
                if pretty {
                    self.out.push(' ');
                }
                ctx.count += 1;
            }
            GenKind::Object => {
                if !ctx.expect_value {
                    return Err(FederationError::io(format!(
                        "Can not write {what}, expecting field name (context: Object)"
                    )));
                }
                ctx.expect_value = false;
            }
        }
        Ok(())
    }

    pub fn write_start_object(&mut self) -> Result<()> {
        self.before_value("a start object")?;
        self.out.push('{');
        self.nesting += 1;
        self.contexts.push(GenContext {
            kind: GenKind::Object,
            count: 0,
            expect_value: false,
        });
        Ok(())
    }

    pub fn write_end_object(&mut self) -> Result<()> {
        let ctx = self
            .contexts
            .pop()
            .ok_or_else(|| FederationError::io("Current context not Object"))?;
        if ctx.kind != GenKind::Object {
            return Err(FederationError::io("Current context not Object but Array"));
        }
        self.nesting -= 1;
        if self.pretty {
            if ctx.count > 0 {
                self.indent();
            } else {
                self.out.push(' ');
            }
        }
        self.out.push('}');
        Ok(())
    }

    pub fn write_start_array(&mut self) -> Result<()> {
        self.before_value("a start array")?;
        self.out.push('[');
        self.contexts.push(GenContext {
            kind: GenKind::Array,
            count: 0,
            expect_value: false,
        });
        Ok(())
    }

    pub fn write_end_array(&mut self) -> Result<()> {
        let ctx = self
            .contexts
            .pop()
            .ok_or_else(|| FederationError::io("Current context not Array"))?;
        if ctx.kind != GenKind::Array {
            return Err(FederationError::io("Current context not Array but Object"));
        }
        if self.pretty {
            self.out.push(' ');
        }
        self.out.push(']');
        Ok(())
    }

    pub fn write_field_name(&mut self, name: &str) -> Result<()> {
        let count = {
            let ctx = self
                .contexts
                .last_mut()
                .ok_or_else(|| FederationError::io("Generator closed"))?;
            if ctx.kind != GenKind::Object || ctx.expect_value {
                return Err(FederationError::io(
                    "Can not write a field name, expecting a value",
                ));
            }
            ctx.expect_value = true;
            ctx.count += 1;
            ctx.count
        };
        if count > 1 {
            self.out.push(',');
        }
        self.indent();
        write_quoted(&mut self.out, name);
        if self.pretty {
            self.out.push_str(" : ");
        } else {
            self.out.push(':');
        }
        Ok(())
    }

    pub fn write_string(&mut self, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => {
                self.before_value("a string")?;
                write_quoted(&mut self.out, v);
                Ok(())
            }
            None => self.write_null(),
        }
    }

    pub fn write_null(&mut self) -> Result<()> {
        self.before_value("a null")?;
        self.out.push_str("null");
        Ok(())
    }

    pub fn write_boolean(&mut self, value: bool) -> Result<()> {
        self.before_value("a boolean value")?;
        self.out.push_str(if value { "true" } else { "false" });
        Ok(())
    }

    pub fn write_number_i64(&mut self, value: i64) -> Result<()> {
        self.before_value("a number")?;
        self.out.push_str(&value.to_string());
        Ok(())
    }

    pub fn write_number_i32(&mut self, value: i32) -> Result<()> {
        self.write_number_i64(i64::from(value))
    }

    pub fn write_raw_number(&mut self, text: &str) -> Result<()> {
        self.before_value("a number")?;
        self.out.push_str(text);
        Ok(())
    }

    pub fn write_binary(&mut self, bytes: &[u8]) -> Result<()> {
        self.before_value("a binary value")?;
        self.out.push('"');
        self.out.push_str(&encode_base64(bytes));
        self.out.push('"');
        Ok(())
    }

    pub fn write_string_field(&mut self, name: &str, value: Option<&str>) -> Result<()> {
        self.write_field_name(name)?;
        self.write_string(value)
    }

    pub fn write_boolean_field(&mut self, name: &str, value: bool) -> Result<()> {
        self.write_field_name(name)?;
        self.write_boolean(value)
    }

    pub fn write_number_field_i64(&mut self, name: &str, value: i64) -> Result<()> {
        self.write_field_name(name)?;
        self.write_number_i64(value)
    }

    pub fn write_number_field_i32(&mut self, name: &str, value: i32) -> Result<()> {
        self.write_field_name(name)?;
        self.write_number_i32(value)
    }

    pub fn write_binary_field(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write_field_name(name)?;
        self.write_binary(bytes)
    }

    pub fn write_null_field(&mut self, name: &str) -> Result<()> {
        self.write_field_name(name)?;
        self.write_null()
    }

    pub fn write_array_field_start(&mut self, name: &str) -> Result<()> {
        self.write_field_name(name)?;
        self.write_start_array()
    }

    pub fn write_object_field_start(&mut self, name: &str) -> Result<()> {
        self.write_field_name(name)?;
        self.write_start_object()
    }

    pub fn write_tree(&mut self, value: &serde_json::Value) -> Result<()> {
        match value {
            serde_json::Value::Null => self.write_null(),
            serde_json::Value::Bool(b) => self.write_boolean(*b),
            serde_json::Value::Number(n) => self.write_raw_number(&n.to_string()),
            serde_json::Value::String(s) => self.write_string(Some(s)),
            serde_json::Value::Array(items) => {
                self.write_start_array()?;
                for item in items {
                    self.write_tree(item)?;
                }
                self.write_end_array()
            }
            serde_json::Value::Object(map) => {
                self.write_start_object()?;
                for (k, v) in map {
                    self.write_field_name(k)?;
                    self.write_tree(v)?;
                }
                self.write_end_object()
            }
        }
    }
}

fn write_quoted(out: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let v = c as u32;
                out.push_str("\\u00");
                out.push(HEX[((v >> 4) & 0xF) as usize] as char);
                out.push(HEX[(v & 0xF) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
