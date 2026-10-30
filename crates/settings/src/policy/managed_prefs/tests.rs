use super::{AGENTS_KEY, PlistValue, SOURCE, parse};
use crate::leaf::Leaf;
use crate::policy::{PolicyRead, PolicyValues};
use crate::schema::{Density, FontSize, Theme};
use crate::testing::abs_str;
use crate::warning::{Origin, WarningKind};

fn read(values: Vec<(&str, PlistValue, bool)>) -> PolicyRead {
    parse(
        values
            .into_iter()
            .map(|(key, value, forced)| (key.to_owned(), value, forced))
            .collect(),
    )
}

fn values(values: Vec<(&str, PlistValue, bool)>) -> (Vec<Leaf>, Vec<WarningKind>) {
    match read(values) {
        PolicyRead::Read(values) => {
            let (leaves, warnings) = PolicyValues::into_parts(values);
            let kinds = warnings
                .into_iter()
                .inspect(|warning| {
                    assert_eq!(
                        warning.origin,
                        Origin::Store {
                            name: SOURCE.to_owned()
                        }
                    );
                })
                .map(|warning| warning.kind)
                .collect();
            (leaves, kinds)
        }
        other => panic!("{other:?}"),
    }
}

fn string(text: &str) -> PlistValue {
    PlistValue::String(text.to_owned())
}

#[test]
fn unforced_value_is_not_policy() {
    // A value the user wrote with `defaults write` is not forced.
    assert!(matches!(
        read(vec![("ui.theme", string("dark"), false)]),
        PolicyRead::Absent
    ));
    let (leaves, warnings) = values(vec![
        ("ui.theme", string("dark"), false),
        ("ui.density", string("compact"), true),
    ]);
    assert_eq!(leaves, [Leaf::Density(Density::Compact)]);
    assert_eq!(warnings, []);
}

#[test]
fn nothing_read_is_absent() {
    assert!(matches!(read(Vec::new()), PolicyRead::Absent));
}

#[test]
fn forced_values_are_typed() {
    let (leaves, warnings) = values(vec![
        ("ui.theme", string("light"), true),
        ("ui.font_size", PlistValue::Integer(13), true),
    ]);
    assert_eq!(
        leaves,
        [
            Leaf::Theme(Theme::Light),
            Leaf::FontSize(FontSize::try_from(13).unwrap())
        ]
    );
    assert_eq!(warnings, []);
}

#[test]
fn the_agents_dictionary_holds_one_launch_spec_per_id() {
    let launch = |command: String| {
        PlistValue::Dict(vec![
            ("command".to_owned(), PlistValue::String(command)),
            ("args".to_owned(), PlistValue::Array(vec![string("--acp")])),
        ])
    };
    let agents = PlistValue::Dict(vec![
        ("claude".to_owned(), launch(abs_str("bin/claude"))),
        ("codex".to_owned(), launch("codex".to_owned())),
    ]);
    let (leaves, warnings) = values(vec![(AGENTS_KEY, agents, true)]);
    assert_eq!(leaves.len(), 2);
    assert!(
        matches!(&leaves[0], Leaf::Agent(id, launch) if id == "claude" && launch.args == ["--acp"])
    );
    // A relative command locks the agent with no entry.
    assert_eq!(leaves[1], Leaf::Refused("agents.codex".to_owned()));
    assert_eq!(
        warnings,
        [WarningKind::RelativeAgentCommand {
            id: "codex".to_owned(),
            command: "codex".to_owned()
        }]
    );
}

#[test]
fn agents_that_is_no_dictionary_and_unknown_keys_are_warnings() {
    let (leaves, warnings) = values(vec![
        (AGENTS_KEY, string("claude"), true),
        ("telemetry.on", PlistValue::Bool(true), true),
    ]);
    assert_eq!(leaves, []);
    assert_eq!(
        warnings,
        [
            WarningKind::InvalidValue {
                key: AGENTS_KEY.to_owned(),
                message: "it must be a dictionary from agent id to launch spec".to_owned(),
            },
            WarningKind::UnknownKey {
                key: "telemetry.on".to_owned()
            },
        ]
    );
}

#[test]
fn each_plist_type_becomes_its_json_value() {
    // `matters.default_location` takes a string, so every other type is refused when typed;
    // the message shows the JSON value it became.
    let cases = [
        (
            PlistValue::Bool(true),
            "invalid type: boolean `true`, expected path string",
        ),
        (
            PlistValue::Integer(-3),
            "invalid type: integer `-3`, expected path string",
        ),
        (
            PlistValue::Real(1.5),
            "invalid type: floating point `1.5`, expected path string",
        ),
        (
            PlistValue::Array(vec![string("a")]),
            "invalid type: sequence, expected path string",
        ),
        (
            PlistValue::Dict(vec![("k".to_owned(), PlistValue::Bool(false))]),
            "invalid type: map, expected path string",
        ),
    ];
    for (value, message) in cases {
        let (_, warnings) = values(vec![("matters.default_location", value, true)]);
        assert_eq!(
            warnings,
            [WarningKind::InvalidValue {
                key: "matters.default_location".to_owned(),
                message: message.to_owned(),
            }]
        );
    }
}

#[test]
fn a_date_or_a_non_finite_number_has_no_json_value() {
    let cases = [
        (PlistValue::Other("date"), "no setting takes a date"),
        (PlistValue::Real(f64::NAN), "NaN is not a finite number"),
        (
            PlistValue::Array(vec![PlistValue::Other("data")]),
            "no setting takes a data",
        ),
        (
            PlistValue::Dict(vec![("k".to_owned(), PlistValue::Real(f64::INFINITY))]),
            "inf is not a finite number",
        ),
    ];
    for (value, message) in cases {
        let (leaves, warnings) = values(vec![("ui.font_size", value, true)]);
        assert_eq!(leaves, [Leaf::Refused("ui.font_size".to_owned())]);
        assert_eq!(
            warnings,
            [WarningKind::InvalidValue {
                key: "ui.font_size".to_owned(),
                message: message.to_owned(),
            }]
        );
    }
}

#[test]
fn an_agent_key_on_its_own_is_read_too() {
    let launch = PlistValue::Dict(vec![(
        "command".to_owned(),
        PlistValue::String(abs_str("bin/claude")),
    )]);
    let (leaves, warnings) = values(vec![("agents.claude", launch, true)]);
    assert!(matches!(&leaves[..], [Leaf::Agent(id, _)] if id == "claude"));
    assert_eq!(warnings, []);
}
