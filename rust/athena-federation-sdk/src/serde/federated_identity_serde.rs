use std::collections::HashMap;

use crate::exceptions::{FederationError, Result};
use crate::request::FederatedIdentity;
use crate::serde::base::{
    get_next_string_array, get_next_string_field, get_next_string_map, nullable_map,
    read_string_map_entries, serialize_nullable, validate_object_start,
    validate_object_start_token, write_string_array, write_string_map,
};
use crate::serde::json::{token_name, JsonGenerator, JsonParser, JsonToken};

const ID_FIELD: &str = "id";
const COMPATIBILITY_ID: &str = "UNKNOWN";
const PRINCIPAL_FIELD: &str = "principal";
const COMPATIBILITY_PRINCIPAL: &str = "UNKNOWN";
const ACCOUNT_FIELD: &str = "account";
const ARN_FIELD: &str = "arn";
const TAGS_FIELD: &str = "tags";
const GROUPS_FIELD: &str = "groups";
const CONFIG_OPTIONS_FIELD: &str = "configOptions";

pub fn serialize(identity: Option<&FederatedIdentity>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, identity, do_serialize)
}

pub fn do_serialize(identity: &FederatedIdentity, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(ID_FIELD, Some(COMPATIBILITY_ID))?;
    jgen.write_string_field(PRINCIPAL_FIELD, Some(COMPATIBILITY_PRINCIPAL))?;
    jgen.write_string_field(ACCOUNT_FIELD, identity.get_account())?;
    jgen.write_string_field(ARN_FIELD, identity.get_arn())?;
    write_string_map(jgen, TAGS_FIELD, identity.get_principal_tags())?;
    write_string_array(jgen, GROUPS_FIELD, identity.get_iam_groups())?;
    write_string_map(jgen, CONFIG_OPTIONS_FIELD, identity.get_config_options())
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<FederatedIdentity>> {
    if jparser.next_token()? == Some(JsonToken::ValueNull) {
        Ok(None)
    } else {
        validate_object_start_token(jparser.current_token())?;
        Ok(Some(do_deserialize(jparser)?))
    }
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<FederatedIdentity> {
    get_next_string_field(jparser, ID_FIELD)?;
    get_next_string_field(jparser, PRINCIPAL_FIELD)?;
    let account = get_next_string_field(jparser, ACCOUNT_FIELD)?;
    let arn = get_next_string_field(jparser, ARN_FIELD)?;
    let principal_tags = nullable_map(get_next_string_map(jparser, TAGS_FIELD)?);
    let groups = get_next_string_array(jparser, GROUPS_FIELD)?;
    let mut config_options: HashMap<String, Option<String>> = HashMap::new();
    let next_token = jparser.next_token()?;
    if next_token == Some(JsonToken::FieldName)
        && jparser.current_name().as_deref() == Some(CONFIG_OPTIONS_FIELD)
    {
        validate_object_start(jparser)?;
        config_options = nullable_map(read_string_map_entries(jparser)?);
        if jparser.next_token()? != Some(JsonToken::EndObject) {
            return Err(FederationError::IllegalState(
                "Expected END_OBJECT after configOptions field".into(),
            ));
        }
    } else if next_token != Some(JsonToken::EndObject) {
        return Err(FederationError::IllegalState(format!(
            "Expected END_OBJECT or configOptions field, got: {}",
            token_name(next_token)
        )));
    }
    Ok(FederatedIdentity::new_nullable(
        arn,
        account,
        principal_tags,
        groups,
        config_options,
    ))
}
