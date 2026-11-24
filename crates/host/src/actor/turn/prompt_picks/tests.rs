//! `choose`: the model and effort a turn runs with, from the thread's options and its picks.

use clauseo_protocol::{
    AgentChoiceId, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind, ConfigPicks,
    NamedChoice,
};

use super::choose;
use crate::actor::agent_options::tests::probed;

fn select(
    id: &str,
    category: ConfigCategory,
    current: &str,
    values: &[(&str, &str)],
) -> ConfigOption {
    ConfigOption {
        id: id.into(),
        name: id.into(),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: current.into(),
            options: values
                .iter()
                .map(|(value, name)| ConfigChoice {
                    value: (*value).into(),
                    name: (*name).into(),
                    description: None,
                    group: None,
                    takes_effort: true,
                })
                .collect(),
        },
    }
}

fn options(model: &str, effort: &str) -> Vec<ConfigOption> {
    vec![
        select(
            "model",
            ConfigCategory::Model,
            model,
            &[
                ("opus", "Opus 5.5"),
                ("fable", "Fable 5.2"),
                ("haiku", "Haiku 5.5"),
            ],
        ),
        select(
            "effort",
            ConfigCategory::ThoughtLevel,
            effort,
            &[
                ("low", "Low"),
                ("medium", "Medium"),
                ("high", "High"),
                ("max", "Max"),
            ],
        ),
    ]
}

fn picks(pairs: &[(&str, &str)]) -> ConfigPicks {
    let map = pairs
        .iter()
        .map(|(o, v)| {
            (
                AgentChoiceId::new(o).unwrap(),
                AgentChoiceId::new(v).unwrap(),
            )
        })
        .collect();
    ConfigPicks::new(map).unwrap()
}

fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

#[test]
fn without_picks_the_options_current_values() {
    let chosen = choose(&options("opus", "medium"), &ConfigPicks::default(), None);
    assert_eq!(
        chosen,
        (
            Some(named("opus", "Opus 5.5")),
            Some(named("medium", "Medium"))
        )
    );
}

#[test]
fn an_effort_pick_names_its_level() {
    let chosen = choose(
        &options("opus", "medium"),
        &picks(&[("effort", "max")]),
        None,
    );
    assert_eq!(
        chosen,
        (Some(named("opus", "Opus 5.5")), Some(named("max", "Max")))
    );
}

#[test]
fn a_model_pick_takes_the_new_models_levels() {
    let models = probed();
    // Fable takes the current medium.
    let chosen = choose(
        &options("opus", "medium"),
        &picks(&[("model", "fable")]),
        Some(&models),
    );
    assert_eq!(
        chosen,
        (
            Some(named("fable", "Fable 5.2")),
            Some(named("medium", "Medium"))
        )
    );
    // Fable has no max: its default instead.
    let chosen = choose(
        &options("opus", "max"),
        &picks(&[("model", "fable")]),
        Some(&models),
    );
    assert_eq!(chosen.1, Some(named("medium", "Medium")));
    // Haiku takes no effort.
    let chosen = choose(
        &options("opus", "medium"),
        &picks(&[("model", "haiku")]),
        Some(&models),
    );
    assert_eq!(chosen, (Some(named("haiku", "Haiku 5.5")), None));
}

#[test]
fn a_pick_the_options_do_not_offer_is_not_named() {
    let chosen = choose(
        &options("opus", "medium"),
        &picks(&[("model", "gone")]),
        None,
    );
    assert_eq!(chosen.0, Some(named("opus", "Opus 5.5")));
    assert_eq!(choose(&[], &ConfigPicks::default(), None), (None, None));
}
