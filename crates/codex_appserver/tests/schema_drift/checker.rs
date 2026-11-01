//! The drift checker: compares a mirrored type with its definition in a JSON schema, and checks a
//! fixture against a definition. Every finding is one line that names the type and the field or
//! value.
//!
//! A struct's field names come from serde itself (the names its derived `Deserialize` asks
//! for), so they are the names on the wire. Whether a field is required comes from decoding: a
//! sample built from the schema with every property present must decode; with one property
//! removed it must fail exactly when the schema lists that property as required; and with a
//! property that may be `null` set to `null`, it must still decode.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// The Codex tag the vendored schema comes from, and so the folder under `schema/` the drift
/// test reads. A pin bump changes it here (`schema/README.md`, "Refreshing on a pin bump").
pub(crate) const SCHEMA_TAG: &str = "rust-v0.162.1";

/// The folder of the vendored files.
pub(crate) fn vendored_folder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(SCHEMA_TAG)
}

/// One schema file.
pub(crate) struct Schema {
    name: String,
    root: Value,
}

impl Schema {
    /// A vendored file, by its path under `schema/<SCHEMA_TAG>/`.
    pub(crate) fn vendored(path: &str) -> Self {
        let full = vendored_folder().join(path);
        let text = std::fs::read_to_string(&full)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", full.display()));
        Self::new(path, serde_json::from_str(&text).unwrap())
    }

    pub(crate) fn new(name: &str, root: Value) -> Self {
        Self {
            name: name.to_owned(),
            root,
        }
    }

    /// A definition by name; `""` is the file's root.
    pub(crate) fn def(&self, name: &str) -> &Value {
        if name.is_empty() {
            return &self.root;
        }
        self.root
            .get("definitions")
            .and_then(|definitions| definitions.get(name))
            .unwrap_or_else(|| panic!("{}: no definition `{name}`", self.name))
    }

    /// The `oneOf` entry of definition `name` whose `title` is `title`.
    pub(crate) fn variant(&self, name: &str, title: &str) -> &Value {
        one_of(self.def(name))
            .iter()
            .find(|entry| entry.get("title").and_then(Value::as_str) == Some(title))
            .unwrap_or_else(|| panic!("{}: `{name}` has no variant `{title}`", self.name))
    }

    /// The names under `definitions`.
    pub(crate) fn definition_names(&self) -> Vec<String> {
        self.root
            .get("definitions")
            .and_then(Value::as_object)
            .map(|definitions| definitions.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Follows a `$ref` within this file.
    fn resolve<'a>(&'a self, node: &'a Value) -> &'a Value {
        match node.get("$ref").and_then(Value::as_str) {
            Some(reference) => {
                let name = reference
                    .strip_prefix("#/definitions/")
                    .unwrap_or_else(|| panic!("{}: unsupported $ref {reference}", self.name));
                self.resolve(self.def(name))
            }
            None => node,
        }
    }

    /// A value the node accepts: every property present, the first enum value, the first
    /// alternative that is not `null`, one entry in a map or list.
    pub(crate) fn sample(&self, node: &Value) -> Value {
        let node = self.resolve(node);
        if let Some(values) = node.get("enum").and_then(Value::as_array) {
            return values.first().cloned().unwrap_or(Value::Null);
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            if let Some(alternatives) = node.get(key).and_then(Value::as_array) {
                let chosen = alternatives
                    .iter()
                    .find(|alternative| !is_null_type(alternative))
                    .or(alternatives.first());
                return chosen.map_or(Value::Null, |alternative| self.sample(alternative));
            }
        }
        match first_type(node) {
            Some("object") => {
                let mut object = Map::new();
                for (name, property) in properties(node) {
                    object.insert(name.clone(), self.sample(property));
                }
                if let Some(entry) = node.get("additionalProperties").filter(|v| v.is_object()) {
                    object.insert("sample".to_owned(), self.sample(entry));
                }
                Value::Object(object)
            }
            Some("array") => Value::Array(
                node.get("items")
                    .map(|items| self.sample(items))
                    .into_iter()
                    .collect(),
            ),
            Some("string") => Value::String("sample".to_owned()),
            Some("integer" | "number") => Value::from(0),
            Some("boolean") => Value::Bool(false),
            _ => Value::Null,
        }
    }

    /// Whether the node accepts `null`.
    fn nullable(&self, node: &Value) -> bool {
        if node.as_bool() == Some(true) || node.as_object().is_some_and(Map::is_empty) {
            return true;
        }
        let node = self.resolve(node);
        let typed = node.get("type");
        let any = ["allOf", "anyOf", "oneOf", "enum", "properties", "$ref"]
            .iter()
            .any(|key| node.get(*key).is_some());
        if typed.is_none() && !any {
            return true;
        }
        type_names(node).contains(&"null")
            || ["anyOf", "oneOf"].iter().any(|key| {
                node.get(*key)
                    .and_then(Value::as_array)
                    .is_some_and(|alternatives| alternatives.iter().any(|a| self.nullable(a)))
            })
    }
}

/// Checks struct `T` against `node`, ignoring the schema properties in `skip` (a tag the
/// enum around the struct reads).
pub(crate) fn check_struct<T: DeserializeOwned>(
    schema: &Schema,
    node: &Value,
    skip: &[&str],
) -> Vec<String> {
    let (name, fields) = struct_fields::<T>();
    let node = schema.resolve(node);
    let mut findings = compare_fields(name, fields, node, skip);
    let Value::Object(full) = schema.sample(node) else {
        return vec![format!("{name}: the schema node is not an object")];
    };
    if let Err(error) = serde_json::from_value::<T>(Value::Object(full.clone())) {
        findings.push(format!(
            "{name}: a sample with every field does not decode: {error}"
        ));
        return findings;
    }
    let required = required(node);
    for (field, property) in properties(node) {
        if skip.contains(&field.as_str()) || !fields.contains(&field.as_str()) {
            continue;
        }
        let mut without = full.clone();
        without.remove(field);
        let decodes = serde_json::from_value::<T>(Value::Object(without)).is_ok();
        match (required.contains(&field.as_str()), decodes) {
            (true, true) => findings.push(format!(
                "{name}: field `{field}` is required in the schema but optional in the type"
            )),
            (false, false) => findings.push(format!(
                "{name}: field `{field}` is optional in the schema but required in the type"
            )),
            _ => {}
        }
        if schema.nullable(property) {
            let mut null = full.clone();
            null.insert(field.clone(), Value::Null);
            if let Err(error) = serde_json::from_value::<T>(Value::Object(null)) {
                findings.push(format!(
                    "{name}: field `{field}` may be null in the schema, but null does not \
                     decode: {error}"
                ));
            }
        }
    }
    findings
}

/// Checks a struct-like `name` with these wire `fields` against `node`'s properties.
pub(crate) fn compare_fields(
    name: &str,
    fields: &[&str],
    node: &Value,
    skip: &[&str],
) -> Vec<String> {
    let mut findings = Vec::new();
    let schema_fields: Vec<&str> = properties(node)
        .map(|(field, _)| field.as_str())
        .filter(|field| !skip.contains(field))
        .collect();
    for field in &schema_fields {
        if !fields.contains(field) {
            findings.push(format!(
                "{name}: field `{field}` is in the schema but not the type"
            ));
        }
    }
    for field in fields {
        if !schema_fields.contains(field) {
            findings.push(format!(
                "{name}: field `{field}` is in the type but not the schema"
            ));
        }
    }
    findings
}

/// Checks an enum's named `values` against `node`: a string `enum`, or a `oneOf` whose entries
/// each fix a `tag` property to one value.
pub(crate) fn check_enum(
    name: &str,
    values: &[&str],
    node: &Value,
    tag: Option<&str>,
) -> Vec<String> {
    let listed: Vec<&str> = match tag {
        None => node
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default(),
        Some(tag) => one_of(node)
            .iter()
            .filter_map(|entry| {
                entry
                    .get("properties")?
                    .get(tag)?
                    .get("enum")?
                    .get(0)?
                    .as_str()
            })
            .collect(),
    };
    if listed.is_empty() {
        return vec![format!("{name}: the schema node lists no values")];
    }
    let mut findings = Vec::new();
    for value in &listed {
        if !values.contains(value) {
            findings.push(format!(
                "{name}: value `{value}` is in the schema but has no variant"
            ));
        }
    }
    for value in values {
        if !listed.contains(value) {
            findings.push(format!(
                "{name}: value `{value}` has a variant but is not in the schema"
            ));
        }
    }
    findings
}

/// Checks a value (a fixture) against `node`: types, required properties, no property the
/// schema does not name, enum values, and the alternatives of `anyOf`, `oneOf` and `allOf`.
pub(crate) fn validate(schema: &Schema, node: &Value, value: &Value, at: &str) -> Vec<String> {
    if node.as_bool() == Some(true) {
        return Vec::new();
    }
    let node = schema.resolve(node);
    let mut findings = Vec::new();
    if let Some(alternatives) = node.get("allOf").and_then(Value::as_array) {
        for alternative in alternatives {
            findings.extend(validate(schema, alternative, value, at));
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(alternatives) = node.get(key).and_then(Value::as_array) {
            let matching = alternatives
                .iter()
                .filter(|alternative| validate(schema, alternative, value, at).is_empty())
                .count();
            let fits = if key == "oneOf" {
                matching == 1
            } else {
                matching >= 1
            };
            if !fits {
                findings.push(format!("{at}: {matching} alternatives of its {key} match"));
            }
        }
    }
    if let Some(values) = node.get("enum").and_then(Value::as_array)
        && !values.contains(value)
    {
        findings.push(format!("{at}: {value} is not one of the schema's values"));
    }
    let types = type_names(node);
    if !types.is_empty() && !types.iter().any(|kind| has_type(value, kind)) {
        findings.push(format!("{at}: {value} is not of type {types:?}"));
    }
    if let Value::Object(object) = value {
        for field in required(node) {
            if !object.contains_key(field) {
                findings.push(format!("{at}: required field `{field}` is missing"));
            }
        }
        let extra = node.get("additionalProperties");
        for (field, item) in object {
            let path = format!("{at}.{field}");
            match node
                .get("properties")
                .and_then(|properties| properties.get(field))
            {
                Some(property) => findings.extend(validate(schema, property, item, &path)),
                None => match extra {
                    Some(entry) if entry.is_object() => {
                        findings.extend(validate(schema, entry, item, &path));
                    }
                    Some(Value::Bool(true)) => {}
                    _ if node.get("properties").is_some() => {
                        findings.push(format!("{path}: not in the schema"));
                    }
                    _ => {}
                },
            }
        }
    }
    if let (Value::Array(items), Some(item_node)) = (value, node.get("items")) {
        for (index, item) in items.iter().enumerate() {
            findings.extend(validate(schema, item_node, item, &format!("{at}[{index}]")));
        }
    }
    findings
}

fn has_type(value: &Value, kind: &str) -> bool {
    match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    }
}

fn one_of(node: &Value) -> &[Value] {
    node.get("oneOf")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn properties(node: &Value) -> impl Iterator<Item = (&String, &Value)> {
    node.get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
}

fn required(node: &Value) -> Vec<&str> {
    node.get("required")
        .and_then(Value::as_array)
        .map(|fields| fields.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn type_names(node: &Value) -> Vec<&str> {
    match node.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn first_type(node: &Value) -> Option<&str> {
    type_names(node).into_iter().find(|kind| *kind != "null")
}

fn is_null_type(node: &Value) -> bool {
    type_names(node) == ["null"]
}

/// The struct name and wire field names `T`'s derived `Deserialize` asks for.
pub(crate) fn struct_fields<T: DeserializeOwned>() -> (&'static str, &'static [&'static str]) {
    let seen = RefCell::new(None);
    let outcome = T::deserialize(Probe(&seen));
    assert!(outcome.is_err(), "the probe never yields a value");
    seen.into_inner()
        .unwrap_or_else(|| panic!("{} is not a derived struct", std::any::type_name::<T>()))
}

/// A deserializer that records what `deserialize_struct` is asked for, then stops.
struct Probe<'a>(&'a RefCell<Option<(&'static str, &'static [&'static str])>>);

#[derive(Debug)]
struct Stop(String);

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Stop {}

impl serde::de::Error for Stop {
    fn custom<M: fmt::Display>(msg: M) -> Self {
        Self(msg.to_string())
    }
}

impl<'de> serde::Deserializer<'de> for Probe<'_> {
    type Error = Stop;

    fn deserialize_any<V: serde::de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Stop> {
        drop(visitor);
        Err(Stop("not a struct".to_owned()))
    }

    fn deserialize_struct<V: serde::de::Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Stop> {
        drop(visitor);
        *self.0.borrow_mut() = Some((name, fields));
        Err(Stop("probed".to_owned()))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
        ignored_any
    }
}
