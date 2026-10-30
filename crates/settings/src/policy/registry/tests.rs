use super::{RegValue, SOURCE, parse, parse_with, raw_values};
use crate::leaf::Leaf;
use crate::policy::Kind;
use crate::schema::{AgentLaunch, FontSize, Theme};
use crate::testing::abs_json;
use crate::warning::{Origin, WarningKind};
use serde_json::{Value, json};

fn kinds(key: &str) -> Option<Kind> {
    match key {
        "x.flag" => Some(Kind::Boolean),
        "x.count" => Some(Kind::Number),
        "x.name" => Some(Kind::String),
        "x.spec" => Some(Kind::Object),
        "x.list" => Some(Kind::StringArray),
        _ => None,
    }
}

/// The JSON values, or the warnings, of one registry value.
fn convert(key: &str, value: RegValue) -> Result<Value, WarningKind> {
    let (mut raws, refused, mut warnings) = raw_values(vec![(key.to_owned(), value)], kinds);
    match (raws.pop(), warnings.pop()) {
        (Some(raw), None) => {
            assert_eq!(refused, Vec::<String>::new());
            assert_eq!(raw.key, key);
            assert_eq!(
                raw.origin,
                Origin::Store {
                    name: SOURCE.to_owned()
                }
            );
            Ok(raw.value)
        }
        (None, Some(warning)) => {
            // A known key whose value is refused is locked at its default; an unknown one is not.
            let expected: Vec<String> = match &warning.kind {
                WarningKind::InvalidValue { .. } => vec![key.to_owned()],
                _ => Vec::new(),
            };
            assert_eq!(refused, expected);
            assert_eq!(
                warning.origin,
                Origin::Store {
                    name: SOURCE.to_owned()
                }
            );
            Err(warning.kind)
        }
        other => panic!("{other:?}"),
    }
}

fn invalid(key: &str, message: &str) -> Result<Value, WarningKind> {
    Err(WarningKind::InvalidValue {
        key: key.to_owned(),
        message: message.to_owned(),
    })
}

#[test]
fn dword_as_bool() {
    assert_eq!(convert("x.flag", RegValue::Dword(0)), Ok(json!(false)));
    assert_eq!(convert("x.flag", RegValue::Dword(1)), Ok(json!(true)));
    assert_eq!(
        convert("x.flag", RegValue::Dword(2)),
        invalid("x.flag", "a boolean is the REG_DWORD 0 or 1, not 2")
    );
}

#[test]
fn dword_as_number() {
    assert_eq!(
        convert("x.count", RegValue::Dword(4_000_000_000)),
        Ok(json!(4_000_000_000_u32))
    );
}

#[test]
fn sz_as_string() {
    assert_eq!(
        convert("x.name", RegValue::Sz("dark".to_owned())),
        Ok(json!("dark"))
    );
}

#[test]
fn sz_json_for_agent() {
    assert_eq!(
        convert(
            "x.spec",
            RegValue::Sz(r#"{"command": "/a", "args": ["b"]}"#.to_owned())
        ),
        Ok(json!({"command": "/a", "args": ["b"]}))
    );
    assert_eq!(
        convert("x.spec", RegValue::Sz("{not json".to_owned())),
        invalid(
            "x.spec",
            "the REG_SZ is not JSON: key must be a string at line 1 column 2"
        )
    );
    // Through the real kinds, an agent's REG_SZ becomes its launch spec.
    let command = abs_json("bin/agent");
    let values = parse(vec![(
        "agents.claude".to_owned(),
        RegValue::Sz(format!(r#"{{"command": "{command}"}}"#)),
    )]);
    assert_eq!(values.keys().collect::<Vec<_>>(), ["agents.claude"]);
    let (leaves, warnings) = values.into_parts();
    assert_eq!(warnings, []);
    assert!(matches!(&leaves[0], Leaf::Agent(id, AgentLaunch { .. }) if id == "claude"));
}

#[test]
fn multi_sz_array() {
    assert_eq!(
        convert(
            "x.list",
            RegValue::MultiSz(vec!["a".to_owned(), "b c".to_owned()])
        ),
        Ok(json!(["a", "b c"]))
    );
    assert_eq!(
        convert("x.list", RegValue::MultiSz(Vec::new())),
        Ok(json!([]))
    );
}

#[test]
fn a_value_of_the_wrong_registry_type_names_both_types() {
    let cases = [
        (
            "x.flag",
            RegValue::Sz("1".to_owned()),
            "it takes a REG_DWORD, not a REG_SZ",
        ),
        (
            "x.count",
            RegValue::MultiSz(Vec::new()),
            "it takes a REG_DWORD, not a REG_MULTI_SZ",
        ),
        (
            "x.name",
            RegValue::Dword(1),
            "it takes a REG_SZ, not a REG_DWORD",
        ),
        (
            "x.spec",
            RegValue::Other(3),
            "it takes a REG_SZ holding JSON, not registry type 3",
        ),
        (
            "x.list",
            RegValue::Sz("a".to_owned()),
            "it takes a REG_MULTI_SZ, not a REG_SZ",
        ),
    ];
    for (key, value, message) in cases {
        assert_eq!(convert(key, value), invalid(key, message), "{key}");
    }
}

#[test]
fn an_unknown_value_name_is_a_warning() {
    assert_eq!(
        convert("update.mode", RegValue::Dword(1)),
        Err(WarningKind::UnknownKey {
            key: "update.mode".to_owned()
        })
    );
}

#[test]
fn parse_types_with_the_settings_kinds() {
    let values = parse(vec![
        ("ui.theme".to_owned(), RegValue::Sz("dark".to_owned())),
        ("ui.font_size".to_owned(), RegValue::Dword(18)),
        ("ui.density".to_owned(), RegValue::Sz("roomy".to_owned())),
        ("ui.colour".to_owned(), RegValue::Sz("red".to_owned())),
    ]);
    let (leaves, warnings) = values.into_parts();
    // The density does not type, so it is locked at its default; the unknown colour is not.
    assert_eq!(
        leaves,
        [
            Leaf::Theme(Theme::Dark),
            Leaf::FontSize(FontSize::try_from(18).unwrap()),
            Leaf::Refused("ui.density".to_owned()),
        ]
    );
    let kinds: Vec<_> = warnings.into_iter().map(|warning| warning.kind).collect();
    assert!(matches!(&kinds[0], WarningKind::UnknownKey { key } if key == "ui.colour"));
    assert!(matches!(&kinds[1], WarningKind::InvalidValue { key, .. } if key == "ui.density"));
    assert_eq!(kinds.len(), 2);
}

#[test]
fn parse_with_keeps_raw_order() {
    let values = parse_with(
        vec![
            ("x.flag".to_owned(), RegValue::Dword(1)),
            ("x.count".to_owned(), RegValue::Dword(2)),
        ],
        kinds,
    );
    // Neither key is a setting, so typing leaves both out, in order.
    let keys: Vec<_> = values
        .warnings()
        .iter()
        .map(|warning| match &warning.kind {
            WarningKind::UnknownKey { key } => key.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(keys, ["x.flag", "x.count"]);
}

#[test]
fn value_names_ignore_case_but_agent_ids_keep_theirs() {
    let spec = format!(r#"{{"command": "{}"}}"#, abs_json("bin/claude"));
    let values = parse(vec![
        ("UI.Theme".to_owned(), RegValue::Sz("dark".to_owned())),
        ("Agents.Claude".to_owned(), RegValue::Sz(spec)),
        ("UI.Colour".to_owned(), RegValue::Sz("red".to_owned())),
    ]);
    assert_eq!(
        values.keys().collect::<Vec<_>>(),
        ["ui.theme", "agents.Claude"]
    );
    // An unknown name keeps its spelling in the warning.
    assert_eq!(
        values
            .warnings()
            .iter()
            .map(|warning| warning.kind.clone())
            .collect::<Vec<_>>(),
        [WarningKind::UnknownKey {
            key: "UI.Colour".to_owned()
        }]
    );
}
