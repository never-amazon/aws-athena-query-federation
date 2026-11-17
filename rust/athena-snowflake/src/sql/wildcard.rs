use regex::Regex;

const SINGLE_CHAR: char = '_';
const MULTIPLE_CHAR: char = '%';
const ESCAPED_CHAR: char = '\\';

fn is_unescaped_wildcard(chars: &[char], index: usize) -> bool {
    let c = chars[index];
    (c == SINGLE_CHAR || c == MULTIPLE_CHAR) && (index == 0 || chars[index - 1] != ESCAPED_CHAR)
}

pub(crate) fn is_wildcard_pattern_str(input: Option<&str>) -> bool {
    let Some(input) = input else {
        return false;
    };
    let chars: Vec<char> = input.chars().collect();
    if chars.iter().any(|c| is_java_line_terminator(*c)) {
        return false;
    }
    (0..chars.len()).any(|index| is_unescaped_wildcard(&chars, index))
}

fn is_java_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn strip_escaped_char(piece: &str) -> String {
    let mut out = String::with_capacity(piece.len());
    let mut escaped = false;
    for c in piece.chars() {
        if c == ESCAPED_CHAR && !escaped {
            escaped = true;
        } else {
            out.push(c);
            escaped = false;
        }
    }
    out
}

fn wildcard_regex(c: char) -> &'static str {
    if c == SINGLE_CHAR {
        JAVA_DOT
    } else {
        JAVA_DOT_STAR
    }
}

const JAVA_DOT: &str = "[^\\n\\r\\x{85}\\x{2028}\\x{2029}]";
const JAVA_DOT_STAR: &str = "[^\\n\\r\\x{85}\\x{2028}\\x{2029}]*";

fn java_split(chars: &[char]) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    for index in 0..chars.len() {
        if is_unescaped_wildcard(chars, index) {
            pieces.push(std::mem::take(&mut current));
        } else {
            current.push(chars[index]);
        }
    }
    pieces.push(current);
    let only_input = pieces.len() == 1;
    if !only_input {
        while pieces.last().is_some_and(String::is_empty) {
            pieces.pop();
        }
    }
    pieces
}

pub(crate) fn to_regex_str(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut regex = String::new();
    let mut position = 0usize;
    for piece in java_split(&chars) {
        let length = piece.chars().count();
        if length > 0 {
            regex.push_str(&regex::escape(&strip_escaped_char(&piece)));
        }
        position += length;
        if position < chars.len() {
            regex.push_str(wildcard_regex(chars[position]));
            position += 1;
        }
    }
    while position < chars.len() {
        regex.push_str(wildcard_regex(chars[position]));
        position += 1;
    }
    regex
}

pub(crate) fn to_regex_pattern(pattern: Option<&str>) -> Option<Regex> {
    let pattern = pattern?;
    Regex::new(&format!("^(?:{})$", to_regex_str(pattern))).ok()
}

pub(crate) fn matches(pattern: Option<&Regex>, value: &str) -> bool {
    pattern.is_none_or(|regex| regex.is_match(value))
}

pub(crate) fn unescape_chars(escaped: &str) -> String {
    escape_sql_quotes(
        &escaped
            .replace("\\_", "_")
            .replace("\\%", "%")
            .replace("\\\\", "\\"),
    )
}

pub(crate) fn escape_sql_quotes(original: &str) -> String {
    original.replace('"', "\"\"")
}

pub(crate) fn escape_single_quote_for_like_command(arg: &str) -> String {
    let mut arg = arg.to_string();
    let mut i = 0usize;
    let mut index = find_from(&arg, '\'', i);
    while let Some(found) = index {
        let previous = arg[..found].chars().next_back();
        if found == 0 || previous != Some('\\') {
            arg = arg.replace('\'', "\\'");
            i = found + 2;
        } else {
            i = found + 1;
        }
        index = if i < arg.len() {
            find_from(&arg, '\'', i)
        } else {
            None
        };
    }
    arg
}

fn find_from(text: &str, needle: char, from: usize) -> Option<usize> {
    text.get(from..)
        .and_then(|rest| rest.find(needle))
        .map(|offset| offset + from)
}
