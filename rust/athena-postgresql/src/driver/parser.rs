#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeSql {
    pub(crate) sql: String,
    pub(crate) parameter_count: usize,
}

fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn dollar_tag_end(chars: &[char], start: usize) -> Option<usize> {
    let first = *chars.get(start + 1)?;
    if first == '$' {
        return Some(start + 1);
    }
    if !(first.is_alphabetic() || first == '_') {
        return None;
    }
    let mut i = start + 2;
    while let Some(c) = chars.get(i) {
        if *c == '$' {
            return Some(i);
        }
        if !is_identifier_char(*c) || *c == '$' {
            return None;
        }
        i += 1;
    }
    None
}

fn quoted_end(chars: &[char], i: usize) -> usize {
    let escape =
        i > 0 && matches!(chars[i - 1], 'e' | 'E') && (i < 2 || !is_identifier_char(chars[i - 2]));
    let mut j = i + 1;
    while j < chars.len() {
        if escape && chars[j] == '\\' {
            j += 2;
            continue;
        }
        if chars[j] == '\'' {
            if chars.get(j + 1) == Some(&'\'') {
                j += 2;
                continue;
            }
            break;
        }
        j += 1;
    }
    (j + 1).min(chars.len())
}

fn identifier_end(chars: &[char], i: usize) -> usize {
    let mut j = i + 1;
    while j < chars.len() && chars[j] != '"' {
        j += 1;
    }
    (j + 1).min(chars.len())
}

fn line_comment_end(chars: &[char], i: usize) -> usize {
    let mut j = i;
    while j < chars.len() && chars[j] != '\n' && chars[j] != '\r' {
        j += 1;
    }
    j
}

fn block_comment_end(chars: &[char], i: usize) -> usize {
    let mut depth = 1;
    let mut j = i + 2;
    while j < chars.len() && depth > 0 {
        if chars[j] == '/' && chars.get(j + 1) == Some(&'*') {
            depth += 1;
            j += 2;
        } else if chars[j] == '*' && chars.get(j + 1) == Some(&'/') {
            depth -= 1;
            j += 2;
        } else {
            j += 1;
        }
    }
    j.min(chars.len())
}

fn dollar_quoted_end(chars: &[char], i: usize) -> Option<usize> {
    let tag_end = dollar_tag_end(chars, i)?;
    let tag = &chars[i..=tag_end];
    let mut j = tag_end + 1;
    while j + tag.len() <= chars.len() {
        if &chars[j..j + tag.len()] == tag {
            return Some(j + tag.len());
        }
        j += 1;
    }
    Some(chars.len())
}

fn literal_end(chars: &[char], i: usize) -> Option<usize> {
    match chars[i] {
        '\'' => Some(quoted_end(chars, i)),
        '"' => Some(identifier_end(chars, i)),
        '-' if chars.get(i + 1) == Some(&'-') => Some(line_comment_end(chars, i)),
        '/' if chars.get(i + 1) == Some(&'*') => Some(block_comment_end(chars, i)),
        '$' if i == 0 || !is_identifier_char(chars[i - 1]) => dollar_quoted_end(chars, i),
        _ => None,
    }
}

pub(crate) fn to_native_sql(jdbc_sql: &str) -> NativeSql {
    let chars: Vec<char> = jdbc_sql.chars().collect();
    let mut out = String::with_capacity(jdbc_sql.len() + 8);
    let mut count = 0usize;
    let mut i = 0;
    while i < chars.len() {
        if let Some(end) = literal_end(&chars, i) {
            out.extend(&chars[i..end]);
            i = end;
        } else if chars[i] == '?' {
            if chars.get(i + 1) == Some(&'?') {
                out.push('?');
                i += 2;
            } else {
                count += 1;
                out.push('$');
                out.push_str(&count.to_string());
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    NativeSql {
        sql: out,
        parameter_count: count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_placeholders_outside_quotes_and_comments() {
        let native = to_native_sql(
            "SELECT '?', \"?\", $$?$$, $tag$ ? $tag$ /* ? */ FROM t WHERE a = ? AND b = ? -- ?\n AND c ?? d",
        );
        assert_eq!(
            native.sql,
            "SELECT '?', \"?\", $$?$$, $tag$ ? $tag$ /* ? */ FROM t WHERE a = $1 AND b = $2 -- ?\n AND c ? d"
        );
        assert_eq!(native.parameter_count, 2);
    }

    #[test]
    fn escape_strings_honour_backslashes() {
        let native = to_native_sql("SELECT E'it\\'s ?', 'a''?' , ?");
        assert_eq!(native.sql, "SELECT E'it\\'s ?', 'a''?' , $1");
        assert_eq!(native.parameter_count, 1);
    }

    #[test]
    fn existing_numbered_parameters_are_left_alone() {
        let native = to_native_sql("SELECT $1");
        assert_eq!(native.sql, "SELECT $1");
        assert_eq!(native.parameter_count, 0);
    }
}
