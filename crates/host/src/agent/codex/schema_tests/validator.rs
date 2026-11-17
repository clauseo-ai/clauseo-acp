//! The JSON-schema validator the schema tests use: the subset Codex 0.162.1's config schema
//! uses, refusing any keyword it does not handle.

use serde_json::{Map, Value, json};

/// What in `doc` breaks `schema`, one line each; empty when it validates.
pub(super) fn errors(schema: &Value, doc: &Value) -> Vec<String> {
    let mut out = Vec::new();
    check(schema, schema, doc, "", &mut out);
    out
}

/// Whether `value` validates against `node`.
fn passes(root: &Value, node: &Value, value: &Value, at: &str) -> bool {
    let mut out = Vec::new();
    check(root, node, value, at, &mut out);
    out.is_empty()
}

/// The JSON-schema subset the Codex schema uses: `$ref` (to `#/definitions/…`), `allOf`,
/// `anyOf`, `oneOf`, `not`, `type`, `enum`, `properties`, `additionalProperties`, `required`,
/// `items`, `minimum`, `maximum`, `exclusiveMinimum`, `minLength` and `maxLength`; `format`,
/// `default`, `description` and `title` are annotations. Any other keyword it meets is an error,
/// so it never passes a document by skipping a rule.
fn check(root: &Value, node: &Value, value: &Value, at: &str, out: &mut Vec<String>) {
    let node = match node {
        Value::Object(node) => node,
        Value::Bool(true) => return,
        Value::Bool(false) => {
            out.push(format!("{at}: not allowed here"));
            return;
        }
        other @ (Value::Null | Value::Number(_) | Value::String(_) | Value::Array(_)) => {
            out.push(format!("{at}: schema node is not a schema: {other}"));
            return;
        }
    };
    for (keyword, arg) in node {
        let here = Here {
            root,
            node,
            value,
            at,
        };
        match keyword.as_str() {
            "$ref" => match arg
                .as_str()
                .and_then(|path| path.strip_prefix("#/definitions/"))
                .and_then(|name| root["definitions"].get(name))
            {
                Some(target) => check(root, target, value, at, out),
                None => out.push(format!("{at}: unresolved $ref {arg}")),
            },
            "allOf" => {
                for sub in array(arg) {
                    check(root, sub, value, at, out);
                }
            }
            "anyOf" | "oneOf" => branches(here, keyword, arg, out),
            "not" => {
                if passes(root, arg, value, at) {
                    out.push(format!("{at}: matches a `not` schema"));
                }
            }
            "type" => {
                let names: Vec<&str> = match arg {
                    Value::String(name) => vec![name.as_str()],
                    Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
                    Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {
                        Vec::new()
                    }
                };
                if !names.iter().any(|name| has_type(value, name)) {
                    out.push(format!("{at}: {value} is not of type {arg}"));
                }
            }
            "enum" => {
                if !array(arg).contains(value) {
                    out.push(format!("{at}: {value} is not one of {arg}"));
                }
            }
            "properties" | "additionalProperties" | "required" | "items" => {
                containers(here, keyword, arg, out);
            }
            "minimum" | "maximum" | "exclusiveMinimum" | "minLength" | "maxLength" => {
                bounds(keyword, arg, value, at, out);
            }
            "format" | "default" | "description" | "title" | "$schema" | "definitions" => {}
            other => out.push(format!(
                "{at}: the test's validator does not handle `{other}`"
            )),
        }
    }
}

/// Where [`check`] is: the schema root, the node, the value and its path.
#[derive(Clone, Copy)]
struct Here<'a> {
    root: &'a Value,
    node: &'a Map<String, Value>,
    value: &'a Value,
    at: &'a str,
}

/// `anyOf` (at least one branch passes) and `oneOf` (exactly one does).
fn branches(here: Here<'_>, keyword: &str, arg: &Value, out: &mut Vec<String>) {
    let Here {
        root, value, at, ..
    } = here;
    let mut errors = Vec::new();
    let mut passed = 0;
    for sub in array(arg) {
        let mut branch = Vec::new();
        check(root, sub, value, at, &mut branch);
        if branch.is_empty() {
            passed += 1;
        }
        errors.extend(branch);
    }
    let fine = if keyword == "anyOf" {
        passed > 0
    } else {
        passed == 1
    };
    if !fine {
        out.push(format!(
            "{at}: {passed} branches of {keyword} match: {}",
            errors.join("; ")
        ));
    }
}

/// `properties`, `additionalProperties` and `required` on an object, `items` on an array.
fn containers(here: Here<'_>, keyword: &str, arg: &Value, out: &mut Vec<String>) {
    let Here {
        root,
        node,
        value,
        at,
    } = here;
    match (keyword, value) {
        ("properties", Value::Object(object)) => {
            for (key, sub) in arg.as_object().into_iter().flatten() {
                if let Some(item) = object.get(key) {
                    check(root, sub, item, &format!("{at}/{key}"), out);
                }
            }
        }
        ("additionalProperties", Value::Object(object)) => {
            let named = node.get("properties").and_then(Value::as_object);
            for (key, item) in object {
                if named.is_some_and(|named| named.contains_key(key)) {
                    continue;
                }
                let here = format!("{at}/{key}");
                if arg == &Value::Bool(false) {
                    out.push(format!("{here}: key not in Codex's schema"));
                } else {
                    check(root, arg, item, &here, out);
                }
            }
        }
        ("required", Value::Object(object)) => {
            for key in array(arg).iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    out.push(format!("{at}: lacks the required key {key}"));
                }
            }
        }
        ("items", Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                check(root, arg, item, &format!("{at}/{index}"), out);
            }
        }
        _ => {}
    }
}

/// `minimum`, `maximum` and `exclusiveMinimum` on a number, `minLength` and `maxLength` on a
/// string's length in characters.
fn bounds(keyword: &str, arg: &Value, value: &Value, at: &str, out: &mut Vec<String>) {
    let measured = if keyword.ends_with("Length") {
        value
            .as_str()
            .map(|text| u32::try_from(text.chars().count()).map_or(f64::MAX, f64::from))
    } else {
        value.as_f64()
    };
    let (Some(measured), Some(bound)) = (measured, arg.as_f64()) else {
        return;
    };
    let fine = match keyword {
        "minimum" | "minLength" => measured >= bound,
        "exclusiveMinimum" => measured > bound,
        _ => measured <= bound,
    };
    if !fine {
        out.push(format!("{at}: {measured} breaks {keyword} {bound}"));
    }
}

/// `arg` as an array, or none.
fn array(arg: &Value) -> &[Value] {
    arg.as_array().map_or(&[], Vec::as_slice)
}

/// Whether `value` is of the JSON-schema type `name`.
fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        _ => false,
    }
}

/// The validator itself: it refuses what each keyword forbids and meets no keyword it skips.
#[test]
fn schema_validator_applies_each_keyword() {
    let root = json!({
        "definitions": {"Mode": {"enum": ["a", "b"], "type": "string"}},
        "type": "object",
        "additionalProperties": false,
        "required": ["mode"],
        "properties": {
            "mode": {"$ref": "#/definitions/Mode"},
            "count": {"type": "integer", "minimum": 1},
            "list": {"type": "array", "items": {"type": "string", "minLength": 2}},
            "either": {"oneOf": [{"type": "string"}, {"type": "boolean"}]},
        },
    });
    let fine = json!({"mode": "a", "count": 2, "list": ["xy"]});
    assert_eq!(errors(&root, &fine), Vec::<String>::new());
    for bad in [
        json!({}),
        json!({"mode": "c"}),
        json!({"mode": "a", "count": 0}),
        json!({"mode": "a", "count": 1.5}),
        json!({"mode": "a", "list": ["x"]}),
        json!({"mode": "a", "either": 3}),
        json!({"mode": "a", "other": 1}),
    ] {
        assert!(!errors(&root, &bad).is_empty(), "{bad} is refused");
    }
    let unknown = json!({"pattern": "^a$"});
    assert_ne!(errors(&unknown, &json!("a")), Vec::<String>::new());
}
