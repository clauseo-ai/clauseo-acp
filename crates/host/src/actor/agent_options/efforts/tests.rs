use clauseo_protocol::{ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind};

use super::{carry, mark};

fn select(id: &str, category: ConfigCategory, current: &str, values: &[&str]) -> ConfigOption {
    ConfigOption {
        id: id.into(),
        name: id.into(),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: current.into(),
            options: values
                .iter()
                .map(|value| ConfigChoice {
                    value: (*value).into(),
                    name: (*value).into(),
                    description: None,
                    group: None,
                    takes_effort: true,
                })
                .collect(),
        },
    }
}

/// Each model choice's value and mark, in order.
fn marks(options: &[ConfigOption]) -> Vec<(String, bool)> {
    options
        .iter()
        .filter(|option| option.category == ConfigCategory::Model)
        .flat_map(|option| {
            let ConfigOptionKind::Select { options, .. } = &option.kind else {
                return Vec::new();
            };
            options
                .iter()
                .map(|choice| (choice.value.to_string(), choice.takes_effort))
                .collect()
        })
        .collect()
}

fn models(current: &str) -> ConfigOption {
    select(
        "model",
        ConfigCategory::Model,
        current,
        &["opus", "haiku", "sonnet"],
    )
}

fn effort() -> ConfigOption {
    select(
        "effort",
        ConfigCategory::ThoughtLevel,
        "high",
        &["low", "high"],
    )
}

fn own(pairs: &[(&str, bool)]) -> Vec<(String, bool)> {
    pairs
        .iter()
        .map(|(value, takes)| ((*value).to_owned(), *takes))
        .collect()
}

/// A current model whose report holds no effort option takes none; the others, unknown, do.
#[test]
fn a_model_with_no_effort_option_takes_no_effort() {
    let mut options = vec![models("haiku")];
    mark(&mut options, &[]);
    assert_eq!(
        marks(&options),
        own(&[("opus", true), ("haiku", false), ("sonnet", true)])
    );
}

/// A current model offered with an effort option takes one.
#[test]
fn a_model_with_an_effort_option_takes_effort() {
    let mut options = vec![models("opus"), effort()];
    mark(&mut options, &[]);
    assert_eq!(
        marks(&options),
        own(&[("opus", true), ("haiku", true), ("sonnet", true)])
    );
}

/// What an earlier report learned about a model that is not current holds, and a model that
/// becomes current again is marked by the report itself.
#[test]
fn an_earlier_report_keeps_what_it_learned() {
    let mut before = vec![models("haiku")];
    mark(&mut before, &[]);
    let mut now = vec![models("opus"), effort()];
    mark(&mut now, &[&before]);
    assert_eq!(
        marks(&now),
        own(&[("opus", true), ("haiku", false), ("sonnet", true)])
    );
    let mut later = vec![models("haiku"), effort()];
    mark(&mut later, &[&now]);
    assert_eq!(
        marks(&later),
        own(&[("opus", true), ("haiku", true), ("sonnet", true)])
    );
}

/// Kept options take the reported marks of the models both list, and keep their own for the rest.
#[test]
fn kept_options_take_the_reported_marks() {
    let mut reported = vec![models("haiku")];
    mark(&mut reported, &[]);
    let mut kept = vec![select(
        "model",
        ConfigCategory::Model,
        "opus",
        &["opus", "haiku", "fable"],
    )];
    carry(&mut kept, &reported);
    assert_eq!(
        marks(&kept),
        own(&[("opus", true), ("haiku", false), ("fable", true)])
    );
}
