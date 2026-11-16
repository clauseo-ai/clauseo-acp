//! `${VAR}` and `${VAR:-default}` in the text of a Claude Code config.

use super::Problem;

/// `text` with each `${VAR}` replaced by the variable's value and each `${VAR:-default}` by the
/// value, or the default when the variable is unset or empty. A variable with no value and no
/// default is kept as written and gives [`Problem::UnresolvedVariable`]. A `${...}` whose name is
/// not a variable name, or that is never closed, is kept as written.
pub(crate) fn expand(
    text: &str,
    vars: &dyn Fn(&str) -> Option<String>,
    problems: &mut Vec<Problem>,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let Some(close) = tail.find('}') else {
            rest = tail;
            break;
        };
        let literal = &tail[..=close];
        let inner = &literal[2..literal.len() - 1];
        rest = &tail[close + 1..];
        match resolve(inner, vars) {
            Ok(value) => out.push_str(&value),
            Err(unresolved) => {
                if let Some(name) = unresolved {
                    problems.push(Problem::UnresolvedVariable { name });
                }
                out.push_str(literal);
            }
        }
    }
    out.push_str(rest);
    out
}

/// The value of `inner` (the text between `${` and `}`). `Err(Some(name))` is an unset variable
/// with no default; `Err(None)` is text that is not a reference at all.
fn resolve(inner: &str, vars: &dyn Fn(&str) -> Option<String>) -> Result<String, Option<String>> {
    let (name, default) = match inner.split_once(":-") {
        Some((name, default)) => (name, Some(default)),
        None => (inner, None),
    };
    if !valid_name(name) {
        return Err(None);
    }
    match (vars(name), default) {
        (Some(value), None) => Ok(value),
        (None, None) => Err(Some(name.to_owned())),
        (Some(value), Some(_)) if !value.is_empty() => Ok(value),
        (_, Some(default)) => Ok(default.to_owned()),
    }
}

/// Whether `name` is a variable name: ASCII letters, digits and `_`, not starting with a digit.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}
