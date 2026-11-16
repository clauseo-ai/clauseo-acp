//! A small JSON Schema checker, enough for the trimmed settings schema
//! (`tests/fixtures/claude_settings/settings.schema.json`): `type` (object, array, string,
//! number, integer, boolean, null), `properties`, `required`, `additionalProperties` (a boolean
//! or a schema), `items`, `enum`, `const`, `anyOf`, `oneOf`, `minLength`, `exclusiveMinimum`
//! and `$ref` to `#/definitions/…` or `#/$defs/…`. A schema holding any other keyword panics
//! ([`Schema::audit`]), so no keyword is ever skipped unread. `integer` takes only a whole
//! number written without a fraction (`10`, not `10.0`).

use serde_json::{Map, Value};

/// The keywords the checker reads or may ignore (the annotations and the definitions).
const KEYWORDS: &[&str] = &[
    "$schema",
    "$comment",
    "$id",
    "title",
    "description",
    "definitions",
    "$defs",
    "$ref",
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "enum",
    "const",
    "anyOf",
    "oneOf",
    "minLength",
    "exclusiveMinimum",
];

/// A schema document, its root and its definitions.
pub(super) struct Schema<'a> {
    root: &'a Value,
}

impl<'a> Schema<'a> {
    pub(super) fn new(root: &'a Value) -> Self {
        Self { root }
    }

    /// Every way `value` breaks the schema, each as `<path>: <why>`; empty when it fits.
    pub(super) fn errors(&self, value: &Value) -> Vec<String> {
        self.errors_at(self.root, value, "$")
    }

    /// Panics on a keyword the checker does not read, or a `$ref` that names no definition,
    /// anywhere in the document, the definitions included.
    pub(super) fn audit(&self) {
        self.audit_at(self.root, "$");
        for place in ["definitions", "$defs"] {
            if let Some(Value::Object(definitions)) = self.root.get(place) {
                for (name, schema) in definitions {
                    self.audit_at(schema, &format!("#/{place}/{name}"));
                }
            }
        }
    }

    fn audit_at(&self, schema: &Value, at: &str) {
        let schema = match schema {
            Value::Bool(_) => return,
            Value::Object(schema) => schema,
            other @ (Value::Null | Value::Number(_) | Value::String(_) | Value::Array(_)) => {
                panic!("{at}: a schema is an object or a boolean, not {other}")
            }
        };
        for key in schema.keys() {
            assert!(
                KEYWORDS.contains(&key.as_str()),
                "{at}: the checker does not read `{key}`"
            );
        }
        if let Some(reference) = schema.get("$ref") {
            self.resolve(reference, at);
        }
        if let Some(Value::Object(properties)) = schema.get("properties") {
            for (name, sub) in properties {
                self.audit_at(sub, &format!("{at}.properties.{name}"));
            }
        }
        for key in ["additionalProperties", "items"] {
            if let Some(sub) = schema.get(key) {
                self.audit_at(sub, &format!("{at}.{key}"));
            }
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(Value::Array(options)) = schema.get(key) {
                for (index, sub) in options.iter().enumerate() {
                    self.audit_at(sub, &format!("{at}.{key}[{index}]"));
                }
            }
        }
    }

    fn errors_at(&self, schema: &Value, value: &Value, at: &str) -> Vec<String> {
        let mut errors = Vec::new();
        self.walk(schema, value, at, &mut errors);
        errors
    }

    fn walk(&self, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
        let schema = match schema {
            Value::Bool(true) => return,
            Value::Bool(false) => {
                errors.push(format!("{at}: no value is allowed here"));
                return;
            }
            Value::Object(schema) => schema,
            other @ (Value::Null | Value::Number(_) | Value::String(_) | Value::Array(_)) => {
                panic!("{at}: a schema is an object or a boolean, not {other}")
            }
        };
        if let Some(reference) = schema.get("$ref") {
            self.walk(self.resolve(reference, at), value, at, errors);
        }
        if let Some(types) = schema.get("type") {
            let names: Vec<&str> = match types {
                Value::String(name) => vec![name.as_str()],
                Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
                other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_)) => {
                    panic!("{at}: `type` is a string or an array, not {other}")
                }
            };
            if !names.iter().any(|name| has_type(value, name)) {
                errors.push(format!("{at}: {value} is not of type {names:?}"));
                return;
            }
        }
        if let Some(Value::Array(options)) = schema.get("enum")
            && !options.contains(value)
        {
            errors.push(format!("{at}: {value} is not one of {options:?}"));
        }
        if let Some(constant) = schema.get("const")
            && constant != value
        {
            errors.push(format!("{at}: {value} is not {constant}"));
        }
        Self::check_scalar(schema, value, at, errors);
        self.check_object(schema, value, at, errors);
        if let (Value::Array(items), Some(sub)) = (value, schema.get("items")) {
            for (index, item) in items.iter().enumerate() {
                self.walk(sub, item, &format!("{at}[{index}]"), errors);
            }
        }
        self.check_choice(schema, value, at, errors);
    }

    /// `minLength` of a string and `exclusiveMinimum` of a number.
    fn check_scalar(
        schema: &Map<String, Value>,
        value: &Value,
        at: &str,
        errors: &mut Vec<String>,
    ) {
        if let (Value::String(text), Some(min)) =
            (value, schema.get("minLength").and_then(Value::as_u64))
            && u64::try_from(text.chars().count()).unwrap_or(u64::MAX) < min
        {
            errors.push(format!("{at}: {value} is shorter than {min}"));
        }
        if let (Some(number), Some(min)) = (
            value.as_f64(),
            schema.get("exclusiveMinimum").and_then(Value::as_f64),
        ) && number <= min
        {
            errors.push(format!("{at}: {value} is not above {min}"));
        }
    }

    /// `required`, `properties` and `additionalProperties` of an object.
    fn check_object(
        &self,
        schema: &Map<String, Value>,
        value: &Value,
        at: &str,
        errors: &mut Vec<String>,
    ) {
        let Value::Object(object) = value else {
            return;
        };
        if let Some(Value::Array(required)) = schema.get("required") {
            for name in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(name) {
                    errors.push(format!("{at}: `{name}` is required"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, item) in object {
            let item_at = format!("{at}.{key}");
            let known = properties.and_then(|known| known.get(key));
            match (known, schema.get("additionalProperties")) {
                (None, None | Some(Value::Bool(true))) => {}
                (None, Some(Value::Bool(false))) => {
                    errors.push(format!("{item_at}: not a key the schema allows"));
                }
                (Some(sub), _) | (None, Some(sub)) => self.walk(sub, item, &item_at, errors),
            }
        }
    }

    /// `anyOf` (at least one option fits) and `oneOf` (exactly one does).
    fn check_choice(
        &self,
        schema: &Map<String, Value>,
        value: &Value,
        at: &str,
        errors: &mut Vec<String>,
    ) {
        for key in ["anyOf", "oneOf"] {
            let Some(Value::Array(options)) = schema.get(key) else {
                continue;
            };
            let each: Vec<Vec<String>> = options
                .iter()
                .map(|option| self.errors_at(option, value, at))
                .collect();
            let fits = each.iter().filter(|errors| errors.is_empty()).count();
            let ok = if key == "oneOf" { fits == 1 } else { fits >= 1 };
            if !ok {
                errors.push(format!(
                    "{at}: {fits} of {key}'s options fit, each: {each:?}"
                ));
            }
        }
    }

    /// The definition `reference` names.
    fn resolve(&self, reference: &Value, at: &str) -> &'a Value {
        let Some(reference) = reference.as_str() else {
            panic!("{at}: `$ref` is not a string");
        };
        let found = ["definitions", "$defs"].iter().find_map(|place| {
            let name = reference.strip_prefix(&format!("#/{place}/"))?;
            self.root.get(*place)?.get(name)
        });
        found.unwrap_or_else(|| panic!("{at}: `$ref` {reference} names no definition"))
    }
}

/// Whether `value` is of the JSON Schema type `name`.
fn has_type(value: &Value, name: &str) -> bool {
    match name {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        other => panic!("unknown type `{other}`"),
    }
}
