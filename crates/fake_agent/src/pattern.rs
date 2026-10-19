//! Patterns that client messages are matched against, `off` pointers, and the `{{cwd}}`
//! placeholder.

use serde_json::Value;

/// The pattern string that matches any present value.
pub(crate) const ANY: &str = "{{any}}";
/// The placeholder for the current session's `cwd`.
pub(crate) const CWD: &str = "{{cwd}}";

/// A copy of `value` with `{{cwd}}` replaced by `cwd` in every string value. Keys stay as they are.
///
/// Works on parsed JSON, so a Windows `cwd` with backslashes stays valid JSON.
pub(crate) fn substitute(value: &Value, cwd: &str) -> Value {
    match value {
        Value::String(text) if text.contains(CWD) => Value::String(text.replace(CWD, cwd)),
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, cwd)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, v)| (key.clone(), substitute(v, cwd)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Whether any string value in `value` holds `{{cwd}}`.
pub(crate) fn uses_cwd(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains(CWD),
        Value::Array(items) => items.iter().any(uses_cwd),
        Value::Object(map) => map.values().any(uses_cwd),
        _ => false,
    }
}

/// Equality that treats `1` and `1.0` as the same number, since the SDK writes floats as floats.
pub(crate) fn scalar_eq(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) if a.is_f64() || b.is_f64() => a
            .as_f64()
            .zip(b.as_f64())
            .is_some_and(|(x, y)| x.to_bits() == y.to_bits()),
        _ => left == right,
    }
}

/// Matches `actual` against `pattern`. `pointer` is the JSON pointer of both, for the message.
///
/// An object pattern matches when every key it names matches; extra keys are fine. Arrays match
/// element by element with equal length. `"{{any}}"` matches any present value. Scalars match
/// by equality. The error names the first pointer that differs.
pub(crate) fn matches(
    pattern: &Value,
    actual: Option<&Value>,
    pointer: &str,
) -> Result<(), String> {
    let at = if pointer.is_empty() { "/" } else { pointer };
    let Some(actual) = actual else {
        return Err(format!("{at}: missing, expected {}", short(pattern)));
    };
    match pattern {
        Value::String(text) if text == ANY => Ok(()),
        Value::Object(fields) => {
            let Value::Object(actual_fields) = actual else {
                return Err(format!("{at}: expected an object, got {}", short(actual)));
            };
            for (key, sub) in fields {
                matches(sub, actual_fields.get(key), &child(pointer, key))?;
            }
            Ok(())
        }
        Value::Array(items) => {
            let Value::Array(actual_items) = actual else {
                return Err(format!("{at}: expected an array, got {}", short(actual)));
            };
            if items.len() != actual_items.len() {
                return Err(format!(
                    "{at}: expected {} items, got {}",
                    items.len(),
                    actual_items.len()
                ));
            }
            for (index, (sub, got)) in items.iter().zip(actual_items).enumerate() {
                matches(sub, Some(got), &child(pointer, &index.to_string()))?;
            }
            Ok(())
        }
        scalar if scalar_eq(scalar, actual) => Ok(()),
        scalar => Err(format!(
            "{at}: expected {}, got {}",
            short(scalar),
            short(actual)
        )),
    }
}

/// Checks that every pointer in `off` is off in `actual`: missing, `null`, `false`, or an object
/// whose values are all off.
pub(crate) fn check_off(actual: Option<&Value>, off: &[String]) -> Result<(), String> {
    for pointer in off {
        let found = actual.and_then(|value| value.pointer(pointer));
        if !is_off(found) {
            let got = found.map_or_else(String::new, short);
            return Err(format!(
                "{pointer}: expected off (missing, null, false or all off), got {got}"
            ));
        }
    }
    Ok(())
}

fn is_off(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => true,
        Some(Value::Object(map)) => map.values().all(|v| is_off(Some(v))),
        Some(_) => false,
    }
}

/// `pointer` extended by `key`, escaped as RFC 6901 says.
pub(crate) fn child(pointer: &str, key: &str) -> String {
    format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Compact JSON of `value`, cut at 120 characters so a message stays one readable line.
pub(crate) fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(120) {
        Some((cut, _)) => format!("{}...", &text[..cut]),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pattern_subset_and_off() {
        let message = json!({"a": 1, "b": {"c": [1, 2], "d": "x"}, "e": null, "f": 0.5});
        assert_eq!(
            matches(&json!({"b": {"d": "x"}}), Some(&message), ""),
            Ok(())
        );
        assert_eq!(
            matches(&json!({"a": "{{any}}", "e": "{{any}}"}), Some(&message), ""),
            Ok(())
        );
        assert_eq!(
            matches(&json!({"f": 0.5, "a": 1.0}), Some(&message), ""),
            Ok(())
        );
        assert_eq!(
            matches(&json!({"b": {"d": "y"}}), Some(&message), ""),
            Err(r#"/b/d: expected "y", got "x""#.to_owned())
        );
        assert_eq!(
            matches(&json!({"b": {"c": [1]}}), Some(&message), ""),
            Err("/b/c: expected 1 items, got 2".to_owned())
        );
        assert_eq!(
            matches(&json!({"z": "{{any}}"}), Some(&message), ""),
            Err(r#"/z: missing, expected "{{any}}""#.to_owned())
        );
        assert_eq!(
            matches(&json!({"a~/": 1}), Some(&json!({})), ""),
            Err("/a~0~1: missing, expected 1".to_owned())
        );

        let caps = json!({"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false,
            "auth": {"terminal": true}, "plan": {}});
        let off = ["/fs", "/terminal", "/subagents", "/plan"].map(str::to_owned);
        assert_eq!(check_off(Some(&caps), &off), Ok(()));
        assert_eq!(
            check_off(Some(&caps), &["/auth".to_owned()]),
            Err(
                r#"/auth: expected off (missing, null, false or all off), got {"terminal":true}"#
                    .to_owned()
            )
        );
        assert!(check_off(Some(&json!({"fs": {"readTextFile": true}})), &off).is_err());
        assert!(check_off(Some(&json!({"terminal": "yes"})), &off).is_err());
        assert_eq!(check_off(None, &off), Ok(()));
    }

    #[test]
    fn cwd_is_substituted_in_values_only() {
        let cwd = r"C:\Users\a b\matter";
        let value = json!({"{{cwd}}": ["{{cwd}}/notes/a.txt", 1], "path": "x"});
        let out = substitute(&value, cwd);
        assert_eq!(
            out,
            json!({"{{cwd}}": [r"C:\Users\a b\matter/notes/a.txt", 1], "path": "x"})
        );
        assert!(serde_json::from_str::<Value>(&out.to_string()).is_ok());
        assert!(uses_cwd(&value));
        assert!(!uses_cwd(&out));
    }
}
