use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token {
    value: String,
    marker: Option<char>,
    pos: usize,
}

impl Token {
    pub fn new(value: impl Into<String>, marker: Option<char>, pos: usize) -> Token {
        Token {
            value: value.into(),
            marker,
            pos,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn marker(&self) -> Option<char> {
        self.marker
    }

    pub fn pos(&self) -> usize {
        self.pos
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Token{{value='{}', marker=", self.value)?;
        match self.marker {
            Some(marker) => write!(f, "{marker}")?,
            None => f.write_str("null")?,
        }
        write!(f, ", pos={}}}", self.pos)
    }
}

#[derive(Debug, Clone)]
pub struct GlueTypeParser {
    input: Vec<char>,
    pos: usize,
    current: Option<Token>,
}

impl GlueTypeParser {
    pub const FIELD_START: char = '<';
    pub const FIELD_END: char = '>';
    pub const FIELD_DIV: char = ':';
    pub const FIELD_SEP: char = ',';
    pub const ESCAPE_CHAR: char = '\\';
    pub const FIELD_PARAM_START: char = '(';
    pub const FIELD_PARAM_END: char = ')';

    pub fn new(input: &str) -> GlueTypeParser {
        GlueTypeParser {
            input: input.chars().collect(),
            pos: 0,
            current: None,
        }
    }

    fn is_token(c: char) -> bool {
        matches!(
            c,
            GlueTypeParser::FIELD_START
                | GlueTypeParser::FIELD_END
                | GlueTypeParser::FIELD_DIV
                | GlueTypeParser::FIELD_SEP
        )
    }

    pub fn has_next(&self) -> bool {
        self.pos < self.input.len()
    }

    #[allow(
        clippy::should_implement_trait,
        reason = "mirrors GlueTypeParser.next, which keeps returning tokens past the end"
    )]
    pub fn next(&mut self) -> Token {
        let mut value = String::new();
        let mut read_pos = self.pos;
        let mut last_escape_char: Option<usize> = None;
        let mut field_params_context = false;
        while read_pos < self.input.len() {
            let last = self.input[read_pos];
            read_pos += 1;
            if last == GlueTypeParser::ESCAPE_CHAR {
                last_escape_char = Some(read_pos - 1);
            } else if last == ' ' {
            } else if field_params_context {
                if last == GlueTypeParser::FIELD_PARAM_END {
                    field_params_context = false;
                }
                value.push(last);
            } else if !GlueTypeParser::is_token(last)
                || (read_pos >= 2 && last_escape_char == Some(read_pos - 2))
            {
                if last == GlueTypeParser::FIELD_PARAM_START {
                    field_params_context = true;
                }
                value.push(last);
            } else {
                self.pos = read_pos;
                let token = Token::new(value, Some(last), read_pos);
                tracing::debug!("next: {}", token);
                self.current = Some(token.clone());
                return token;
            }
        }
        self.pos = read_pos;
        let token = Token::new(value, None, read_pos);
        tracing::debug!("next: {}", token);
        self.current = Some(token.clone());
        token
    }

    pub fn current_token(&self) -> Option<&Token> {
        self.current.as_ref()
    }
}
