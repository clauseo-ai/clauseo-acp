//! The line reader for `budgets.toml`: one line at a time, into a key and a value, or into
//! nothing when the line is not a shape the file uses.

/// A value as written: a basic string, or a number.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    Text(String),
    Number(f64),
}

impl Value {
    pub(super) fn text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            Value::Number(_) => None,
        }
    }
}

/// One line, read on its own.
pub(super) enum Line<'a> {
    Blank,
    Header,
    Pair { key: &'a str, value: Value },
    Unreadable,
}

pub(super) fn read_line(raw: &str) -> Line<'_> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return Line::Blank;
    }
    if let Some(rest) = line.strip_prefix("[[budget]]") {
        return if is_comment_or_empty(rest) {
            Line::Header
        } else {
            Line::Unreadable
        };
    }
    let Some((key, rest)) = line.split_once('=') else {
        return Line::Unreadable;
    };
    let key = key.trim_end();
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return Line::Unreadable;
    }
    match read_value(rest.trim_start()) {
        Some(value) => Line::Pair { key, value },
        None => Line::Unreadable,
    }
}

/// A basic string (`"..."`, with `\"` and `\\` as the only escapes), or a number, each followed by
/// nothing or a comment.
pub(super) fn read_value(text: &str) -> Option<Value> {
    if let Some(body) = text.strip_prefix('"') {
        return read_string(body);
    }
    let end = text
        .find(|c: char| c.is_whitespace() || c == '#')
        .unwrap_or(text.len());
    let (token, tail) = text.split_at(end);
    if !is_comment_or_empty(tail) || !is_number(token) {
        return None;
    }
    token.parse::<f64>().ok().map(Value::Number)
}

/// The rest of a string after its opening quote.
fn read_string(body: &str) -> Option<Value> {
    let mut chars = body.chars();
    let mut text = String::new();
    loop {
        match chars.next()? {
            '"' => {
                return is_comment_or_empty(chars.as_str()).then_some(Value::Text(text));
            }
            '\\' => match chars.next()? {
                '"' => text.push('"'),
                '\\' => text.push('\\'),
                _ => return None,
            },
            other => text.push(other),
        }
    }
}

fn is_comment_or_empty(rest: &str) -> bool {
    let rest = rest.trim();
    rest.is_empty() || rest.starts_with('#')
}

/// `-?digits(.digits)?`, with no exponent and no bare dot.
pub(super) fn is_number(token: &str) -> bool {
    let digits = token.strip_prefix('-').unwrap_or(token);
    let (whole, fraction) = match digits.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (digits, None),
    };
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    all_digits(whole) && fraction.is_none_or(all_digits)
}
