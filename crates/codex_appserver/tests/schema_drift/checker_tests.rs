//! The checker on a small hand-made schema: clean, then with one planted mismatch each.

use serde::Deserialize;
use serde_json::json;

use crate::checker::{Schema, check_enum, check_struct, validate};

/// A hand-made schema in the vendored files' shape.
fn schema() -> Schema {
    Schema::new(
        "hand-made",
        json!({
            "definitions": {
                "Colour": {"enum": ["red", "green"], "type": "string"},
                "Window": {
                    "properties": {
                        "usedPercent": {"type": "integer"},
                        "resetsAt": {"type": ["integer", "null"]},
                        "colour": {"anyOf": [{"$ref": "#/definitions/Colour"}, {"type": "null"}]}
                    },
                    "required": ["usedPercent"],
                    "type": "object"
                }
            }
        }),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    used_percent: i32,
    #[serde(default)]
    resets_at: Option<i64>,
    #[serde(default)]
    colour: Option<String>,
}

#[test]
fn a_matching_type_has_no_findings() {
    let window: Window =
        serde_json::from_value(json!({"usedPercent": 5, "resetsAt": 9, "colour": "red"})).unwrap();
    assert_eq!(
        (
            window.used_percent,
            window.resets_at,
            window.colour.as_deref()
        ),
        (5, Some(9), Some("red"))
    );
    let schema = schema();
    assert_eq!(
        check_struct::<Window>(&schema, schema.def("Window"), &[]),
        Vec::<String>::new()
    );
    assert_eq!(
        check_enum("Colour", &["red", "green"], schema.def("Colour"), None),
        Vec::<String>::new()
    );
}

#[test]
fn a_field_required_in_the_schema_but_optional_in_the_type_is_named() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Window {
        #[serde(default)]
        used_percent: Option<i32>,
        #[serde(default)]
        resets_at: Option<i64>,
        #[serde(default)]
        colour: Option<String>,
    }
    let window: Window = serde_json::from_value(json!({"usedPercent": 5})).unwrap();
    assert_eq!(
        (window.used_percent, window.resets_at, window.colour),
        (Some(5), None, None)
    );
    let schema = schema();
    assert_eq!(
        check_struct::<Window>(&schema, schema.def("Window"), &[]),
        ["Window: field `usedPercent` is required in the schema but optional in the type"]
    );
}

#[test]
fn a_misnamed_field_is_named_both_ways() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Window {
        used_percent: i32,
        #[serde(default)]
        resets_at: Option<i64>,
        #[serde(default)]
        color: Option<String>,
    }
    let window: Window = serde_json::from_value(json!({"usedPercent": 5})).unwrap();
    assert_eq!(
        (window.used_percent, window.resets_at, window.color),
        (5, None, None)
    );
    let schema = schema();
    assert_eq!(
        check_struct::<Window>(&schema, schema.def("Window"), &[]),
        [
            "Window: field `colour` is in the schema but not the type",
            "Window: field `color` is in the type but not the schema",
        ]
    );
}

#[test]
fn a_value_with_no_variant_is_named() {
    let schema = schema();
    assert_eq!(
        check_enum("Colour", &["red"], schema.def("Colour"), None),
        ["Colour: value `green` is in the schema but has no variant"]
    );
}

#[test]
fn a_nullable_field_that_refuses_null_is_named() {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Window {
        used_percent: i32,
        #[serde(default)]
        resets_at: i64,
        #[serde(default)]
        colour: Option<String>,
    }
    let window: Window = serde_json::from_value(json!({"usedPercent": 5})).unwrap();
    assert_eq!(
        (window.used_percent, window.resets_at, window.colour),
        (5, 0, None)
    );
    let schema = schema();
    let findings = check_struct::<Window>(&schema, schema.def("Window"), &[]);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].starts_with("Window: field `resetsAt` may be null in the schema"),
        "{findings:?}"
    );
}

#[test]
fn validate_names_a_bad_fixture_field() {
    let schema = schema();
    let fixture = json!({"usedPercent": 1, "colour": "blue", "extra": true});
    assert_eq!(
        validate(&schema, schema.def("Window"), &fixture, "window.json"),
        [
            "window.json.colour: 0 alternatives of its anyOf match",
            "window.json.extra: not in the schema",
        ]
    );
    assert_eq!(
        validate(
            &schema,
            schema.def("Window"),
            &json!({"resetsAt": null}),
            "window.json"
        ),
        ["window.json: required field `usedPercent` is missing"]
    );
}
