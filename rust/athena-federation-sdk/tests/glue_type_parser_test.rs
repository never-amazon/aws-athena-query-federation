use athena_federation_sdk::metadata::glue::{GlueTypeParser, Token};

fn assert_tokens(input: &str, expected_tokens: &[Token]) {
    let mut parser = GlueTypeParser::new(input);
    let mut pos = 0;
    while parser.has_next() {
        let next = parser.next();
        assert_eq!(expected_tokens[pos], next);
        pos += 1;
    }
    assert_eq!(expected_tokens.len(), pos);
}

#[test]
fn parse_test() {
    let input = "STRUCT <  street_address: STRUCT <    street_number: INT,    street_name: STRING,    street_type: STRING  >,  country: STRING,  postal_code: ARRAY<STRING>>";
    let expected_tokens = vec![
        Token::new("STRUCT", Some(GlueTypeParser::FIELD_START), 8),
        Token::new("street_address", Some(GlueTypeParser::FIELD_DIV), 25),
        Token::new("STRUCT", Some(GlueTypeParser::FIELD_START), 34),
        Token::new("street_number", Some(GlueTypeParser::FIELD_DIV), 52),
        Token::new("INT", Some(GlueTypeParser::FIELD_SEP), 57),
        Token::new("street_name", Some(GlueTypeParser::FIELD_DIV), 73),
        Token::new("STRING", Some(GlueTypeParser::FIELD_SEP), 81),
        Token::new("street_type", Some(GlueTypeParser::FIELD_DIV), 97),
        Token::new("STRING", Some(GlueTypeParser::FIELD_END), 107),
        Token::new("", Some(GlueTypeParser::FIELD_SEP), 108),
        Token::new("country", Some(GlueTypeParser::FIELD_DIV), 118),
        Token::new("STRING", Some(GlueTypeParser::FIELD_SEP), 126),
        Token::new("postal_code", Some(GlueTypeParser::FIELD_DIV), 140),
        Token::new("ARRAY", Some(GlueTypeParser::FIELD_START), 147),
        Token::new("STRING", Some(GlueTypeParser::FIELD_END), 154),
        Token::new("", Some(GlueTypeParser::FIELD_END), 155),
    ];
    assert_tokens(input, &expected_tokens);
}

#[test]
fn parse_test_simple() {
    assert_tokens("string", &[Token::new("string", None, 6)]);
}

#[test]
fn parse_test_complex() {
    let input = "ARRAY<STRUCT<time:timestamp, measure_value\\:\\:double:double>>";
    let expected_tokens = vec![
        Token::new("ARRAY", Some(GlueTypeParser::FIELD_START), 6),
        Token::new("STRUCT", Some(GlueTypeParser::FIELD_START), 13),
        Token::new("time", Some(GlueTypeParser::FIELD_DIV), 18),
        Token::new("timestamp", Some(GlueTypeParser::FIELD_SEP), 28),
        Token::new("measure_value::double", Some(GlueTypeParser::FIELD_DIV), 53),
        Token::new("double", Some(GlueTypeParser::FIELD_END), 60),
        Token::new("", Some(GlueTypeParser::FIELD_END), 61),
    ];
    assert_tokens(input, &expected_tokens);
}

#[test]
fn token_to_string_matches_java() {
    assert_eq!(
        "Token{value='STRUCT', marker=<, pos=8}",
        Token::new("STRUCT", Some('<'), 8).to_string()
    );
    assert_eq!(
        "Token{value='x', marker=null, pos=1}",
        Token::new("x", None, 1).to_string()
    );
}
