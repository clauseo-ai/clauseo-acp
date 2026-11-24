//! `choose`: the model and effort a turn runs with, from the thread's options and its picks.

use clauseo_protocol::{
    AgentChoiceId, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind, ConfigPicks,
    NamedChoice,
};

use super::{PromptPicks, choose};
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

/// A session start that attaches without a step 4 wait (everything held) calls no
/// `session_started`, so `session_starting` alone must drop what the last start left: its start
/// options, which no `TurnStarted` took when that turn was cancelled, and its list wait.
#[test]
fn a_session_start_drops_the_last_starts_options_and_list_wait() {
    let turn = clauseo_protocol::TurnId::new(
        uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[3; 10]).into_uuid(),
    )
    .unwrap();
    let mut picks = PromptPicks {
        start: Some(options("opus", "medium")),
        kept_wait: Some(turn),
        kept_waited: true,
        kept_told: true,
        ..PromptPicks::default()
    };
    picks.session_starting();
    assert_eq!(picks.start, None);
    assert_eq!(picks.kept_wait, None);
    assert!(!picks.kept_waited && !picks.kept_told);
    assert!(picks.kept_lost.is_empty() && picks.kept_due.is_empty());
}
