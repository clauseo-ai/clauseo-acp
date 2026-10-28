use super::{FontSize, Language, SECTIONS, Settings, json_schema};
use crate::leaf::KEYS;
use crate::policy::Kind;
use serde_json::Value;

// The ACP SDK turns on serde_json's `preserve_order`, so the schema's key order depends on which
// crates are in the build. The tests sort keys.
#[test]
fn schema_snapshot() {
    insta::with_settings!({
        snapshot_path => "../snapshots",
        prepend_module_to_snapshot => false,
        omit_expression => true,
        sort_maps => true,
    }, {
        insta::assert_json_snapshot!("settings_schema", json_schema());
    });
}

/// The schema's properties, as dotted keys with the JSON Schema type of each.
fn schema_leaves() -> Vec<(String, String)> {
    let schema = json_schema();
    let defs = &schema["$defs"];
    let resolve = |value: &Value| -> Value {
        let reference = value["$ref"]
            .as_str()
            .or_else(|| value["allOf"][0]["$ref"].as_str());
        match reference.and_then(|r| r.strip_prefix("#/$defs/")) {
            Some(name) => defs[name].clone(),
            None => value.clone(),
        }
    };
    let mut leaves = Vec::new();
    for (section, section_schema) in schema["properties"].as_object().unwrap() {
        let section_schema = resolve(section_schema);
        let Some(properties) = section_schema["properties"].as_object() else {
            continue;
        };
        for (name, property) in properties {
            let property = resolve(property);
            let kind = match &property["type"] {
                Value::String(kind) => kind.clone(),
                Value::Array(kinds) => kinds[0].as_str().unwrap().to_owned(),
                _ if property.get("enum").is_some() || property.get("oneOf").is_some() => {
                    "string".to_owned()
                }
                other => panic!("{section}.{name}: {other}"),
            };
            leaves.push((format!("{section}.{name}"), kind));
        }
    }
    leaves
}

#[test]
fn the_keys_and_their_kinds_are_the_schemas() {
    let mut expected: Vec<(String, String)> = KEYS
        .iter()
        .map(|(key, kind)| {
            let kind = match kind {
                Kind::Number => "integer",
                Kind::String => "string",
                other => panic!("{key}: {other:?}"),
            };
            ((*key).to_owned(), kind.to_owned())
        })
        .collect();
    expected.sort();
    let mut actual = schema_leaves();
    actual.sort();
    assert_eq!(actual, expected);
    let schema = json_schema();
    let mut sections: Vec<&str> = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    sections.sort_unstable();
    let mut known = SECTIONS.to_vec();
    known.sort_unstable();
    assert_eq!(sections, known);
}

#[test]
fn font_sizes_run_from_12_to_20() {
    assert_eq!(FontSize::default().get(), 14);
    for size in [12, 14, 20] {
        assert_eq!(FontSize::try_from(size).map(FontSize::get), Ok(size));
        assert_eq!(u8::from(FontSize::try_from(size).unwrap()), size);
    }
    for size in [0, 11, 21, 255] {
        assert!(FontSize::try_from(size).is_err(), "{size}");
    }
}

#[test]
fn languages_are_bcp_47_tags() {
    assert_eq!(Language::default().as_str(), "en");
    for tag in ["en", "en-GB", "zh-Hant-TW", "de-CH-1901", "abcdefgh-x"] {
        let language = Language::try_from(tag.to_owned()).unwrap();
        assert_eq!(String::from(language), tag);
    }
    for tag in [
        "",
        "e",
        "1a",
        "en_GB",
        "en-",
        "-en",
        "abcdefghi",
        "en-abcdefghi",
        "é",
    ] {
        assert!(Language::try_from(tag.to_owned()).is_err(), "{tag:?}");
    }
}

#[test]
fn each_setting_has_its_default() {
    let settings = Settings::default();
    assert_eq!(settings.agents, std::collections::BTreeMap::new());
    assert_eq!(settings.matters.default_location, None);
    assert_eq!(
        serde_json::to_value(&settings.ui).unwrap(),
        serde_json::json!({"theme": "system", "font_size": 14, "density": "comfortable", "language": "en"})
    );
}
