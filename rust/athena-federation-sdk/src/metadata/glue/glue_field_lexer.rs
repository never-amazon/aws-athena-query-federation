use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields};

use super::default_glue_type::DefaultGlueType;
use super::glue_type_parser::{GlueTypeParser, Token};
use crate::data::{map_type, ArrowType, FieldBuilder};
use crate::exceptions::{FederationError, Result};

const STRUCT: &str = "struct";
const MAP: &str = "map";
const LIST_EQUIVALENTS: [&str; 2] = ["array", "set"];

pub trait BaseTypeMapper {
    fn get_type(&self, type_name: &str) -> Result<Option<ArrowType>>;

    fn get_field(&self, name: &str, type_name: &str) -> Result<Option<Field>> {
        Ok(self
            .get_type(type_name)?
            .map(|arrow_type| FieldBuilder::new_builder(name, arrow_type).build()))
    }
}

impl<F> BaseTypeMapper for F
where
    F: Fn(&str) -> Result<Option<ArrowType>>,
{
    fn get_type(&self, type_name: &str) -> Result<Option<ArrowType>> {
        self(type_name)
    }
}

struct DefaultTypeMapper;

impl BaseTypeMapper for DefaultTypeMapper {
    fn get_type(&self, type_name: &str) -> Result<Option<ArrowType>> {
        DefaultGlueType::to_arrow_type(type_name)
    }
}

pub struct GlueFieldLexer;

impl GlueFieldLexer {
    pub const MAP_DISABLED: bool = false;

    pub fn lex(name: &str, input: &str) -> Result<Option<Field>> {
        GlueFieldLexer::lex_with_mapper(name, input, &DefaultTypeMapper)
    }

    pub fn lex_with_mapper(
        name: &str,
        input: &str,
        mapper: &dyn BaseTypeMapper,
    ) -> Result<Option<Field>> {
        let mut parser = GlueTypeParser::new(input);
        lex_internal(name, &mut parser, mapper)
    }
}

fn lex_internal(
    name: &str,
    parser: &mut GlueTypeParser,
    mapper: &dyn BaseTypeMapper,
) -> Result<Option<Field>> {
    tracing::debug!("lexInternal: enter - {}", name);
    let type_token = parser.next();
    let type_token_value_lower = type_token.value().to_lowercase();
    let result = if type_token_value_lower == STRUCT {
        parse_struct(name, &type_token, parser, mapper).map(Some)
    } else if type_token_value_lower == MAP {
        parse_map(name, &type_token, parser, mapper).map(Some)
    } else if LIST_EQUIVALENTS.contains(&type_token_value_lower.as_str()) {
        parse_list(name, &type_token, parser, mapper).map(Some)
    } else {
        expect_token_marker_is_field_end(Some(&type_token))
            .and_then(|()| mapper.get_field(name, &type_token_value_lower))
    };
    tracing::debug!("lexInternal: exit - {}", name);
    result
}

fn required_child(child: Option<Field>) -> Result<Field> {
    child.ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"org.apache.arrow.vector.types.pojo.Field.getName()\" because \"child\" is null")
    })
}

fn parse_struct(
    name: &str,
    type_token: &Token,
    parser: &mut GlueTypeParser,
    mapper: &dyn BaseTypeMapper,
) -> Result<Field> {
    expect_token_marker_is_field_start(type_token)?;
    let mut field_builder = FieldBuilder::new_builder(name, DataType::Struct(Fields::empty()));
    while parser.has_next() {
        let child_name_token = parser.next();
        if child_name_token.marker() != Some(GlueTypeParser::FIELD_DIV) {
            break;
        }
        let child = lex_internal(child_name_token.value(), parser, mapper)?;
        field_builder.add_field(required_child(child)?);
    }
    expect_token_marker_is_field_end(parser.current_token())?;
    Ok(field_builder.build())
}

fn parse_list(
    name: &str,
    type_token: &Token,
    parser: &mut GlueTypeParser,
    mapper: &dyn BaseTypeMapper,
) -> Result<Field> {
    expect_token_marker_is_field_start(type_token)?;
    let child = lex_internal(name, parser, mapper)?;
    let closing_token = parser.next();
    expect_token_marker_is_field_end(Some(&closing_token))?;
    let child = required_child(child)?;
    Ok(
        FieldBuilder::new_builder(name, DataType::List(Arc::new(child.clone())))
            .add_field(child)
            .build(),
    )
}

fn parse_map(
    name: &str,
    type_token: &Token,
    parser: &mut GlueTypeParser,
    mapper: &dyn BaseTypeMapper,
) -> Result<Field> {
    if GlueFieldLexer::MAP_DISABLED {
        return Err(FederationError::not_supported(
            "Map type is currently unsupported",
        ));
    }
    expect_token_marker_is_field_start(type_token)?;
    let key_type = lex_internal("key", parser, mapper)?;
    let value_type = lex_internal("value", parser, mapper)?;
    let closing_token = parser.next();
    expect_token_marker_is_field_end(Some(&closing_token))?;
    let key_type = key_type.ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"org.apache.arrow.vector.types.pojo.Field.getType()\" because \"keyType\" is null")
    })?;
    let value_type = required_child(value_type)?;
    let key_not_nullable = key_type.with_nullable(false);
    Ok(Field::new(
        name,
        map_type(false, key_not_nullable, value_type),
        true,
    ))
}

fn expect_token_marker_is_field_start(token: &Token) -> Result<()> {
    if token.marker() == Some(GlueTypeParser::FIELD_START) {
        Ok(())
    } else {
        Err(FederationError::invalid_input(format!(
            "Expected field start: but found {token}"
        )))
    }
}

fn expect_token_marker_is_field_end(token: Option<&Token>) -> Result<()> {
    let token = token.ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"com.amazonaws.athena.connector.lambda.metadata.glue.GlueTypeParser$Token.getMarker()\" because \"token\" is null")
    })?;
    match token.marker() {
        None | Some(GlueTypeParser::FIELD_SEP | GlueTypeParser::FIELD_END) => Ok(()),
        Some(_) => Err(FederationError::invalid_input(format!(
            "Expected field ending but found: {token}"
        ))),
    }
}
