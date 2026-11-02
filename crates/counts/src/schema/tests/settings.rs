//! `setting_changed` carries each setting's own values, so the names here must be the settings'
//! names. `counts` may not depend on `settings`, so this test holds the two together.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::schema::{Language, LiteMode, ReducedMotion, TextSize, Theme};

/// Every string an `enum` or `const` lists under `$defs/<def>` of the settings' JSON Schema.
fn settings_names(def: &str) -> BTreeSet<String> {
    fn walk(value: &Value, names: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::String(name)) = object.get("const") {
                    names.insert(name.clone());
                }
                if let Some(Value::Array(values)) = object.get("enum") {
                    names.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
                }
                object.values().for_each(|value| walk(value, names));
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, names)),
            _ => {}
        }
    }
    let schema = clauseo_settings::json_schema();
    let mut names = BTreeSet::new();
    walk(&schema["$defs"][def], &mut names);
    assert!(!names.is_empty(), "the settings schema has no {def}");
    names
}

fn ours(all: &[&'static str]) -> BTreeSet<String> {
    all.iter().map(|&name| name.to_owned()).collect()
}

#[test]
fn setting_values_are_the_settings_own() {
    let theme: Vec<_> = Theme::ALL.iter().map(|v| v.as_str()).collect();
    let lite: Vec<_> = LiteMode::ALL.iter().map(|v| v.as_str()).collect();
    let motion: Vec<_> = ReducedMotion::ALL.iter().map(|v| v.as_str()).collect();
    let language: Vec<_> = Language::ALL.iter().map(|v| v.as_str()).collect();
    assert_eq!(ours(&theme), settings_names("Theme"));
    assert_eq!(ours(&lite), settings_names("Lite"));
    assert_eq!(ours(&motion), settings_names("Motion"));
    assert_eq!(ours(&language), settings_names("Language"));
    assert_eq!(TextSize::MIN, clauseo_settings::FontSize::MIN);
    assert_eq!(TextSize::MAX, clauseo_settings::FontSize::MAX);
}
