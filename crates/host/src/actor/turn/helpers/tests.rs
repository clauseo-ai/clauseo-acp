//! The model a helper names, as the conversation's model menu lists it (`listed_model`).

use clauseo_protocol::{
    CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
};

use super::listed_model;

fn choice(value: &str) -> ConfigChoice {
    ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
        group: None,
        takes_effort: true,
    }
}

/// A model menu listing `values`, and a thought-level menu listing `haiku` that must not count.
fn menu(values: &[&str]) -> Vec<ConfigOption> {
    let option = |id: &str, category, values: &[&str]| ConfigOption {
        id: id.into(),
        name: id.into(),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: values.first().copied().unwrap_or_default().into(),
            options: values.iter().copied().map(choice).collect(),
        },
    };
    vec![
        option("effort", ConfigCategory::ThoughtLevel, &["haiku-ish"]),
        option("model", ConfigCategory::Model, values),
    ]
}

fn listed(model: &str, values: &[&str]) -> CompactString {
    listed_model(model.into(), &menu(values))
}

#[test]
fn an_alias_takes_the_first_listed_id_of_its_family() {
    let values = [
        "default",
        "claude-opus-5-5",
        "claude-haiku-5-5",
        "claude-haiku-4-5",
    ];
    assert_eq!(listed("haiku", &values), "claude-haiku-5-5");
    assert_eq!(
        listed("Sonnet", &["claude-sonnet-5-5"]),
        "claude-sonnet-5-5"
    );
}

#[test]
fn an_alias_no_listed_id_holds_stays_as_it_came() {
    assert_eq!(
        listed("haiku", &["claude-opus-5-5", "claude-sonnet-5-5"]),
        "haiku"
    );
    assert_eq!(listed_model("haiku".into(), &[]), "haiku");
    // A word inside another word is no match.
    assert_eq!(listed("son", &["claude-sonnet-5-5"]), "son");
}

#[test]
fn a_listed_id_stays_as_it_is() {
    let values = ["claude-haiku-5-5", "claude-haiku-4-5"];
    assert_eq!(listed("claude-haiku-4-5", &values), "claude-haiku-4-5");
    assert_eq!(
        listed("default", &["default", "claude-default-1"]),
        "default"
    );
}
