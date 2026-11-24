//! `sets`: which of a turn's picks step 4 sends.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentChoiceId, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
};

use super::sets;

fn id(text: &str) -> AgentChoiceId {
    AgentChoiceId::new(text).unwrap()
}

/// Claude's effort option at medium, and its fast mode, a true/false option.
fn claude() -> Vec<ConfigOption> {
    let choice = |value: &str| ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
        group: None,
        takes_effort: true,
    };
    vec![
        ConfigOption {
            id: "effort".into(),
            name: "Effort".into(),
            description: None,
            category: ConfigCategory::ThoughtLevel,
            kind: ConfigOptionKind::Select {
                current_value: "medium".into(),
                options: vec![choice("low"), choice("medium"), choice("high")],
            },
        },
        ConfigOption {
            id: "fast".into(),
            name: "Fast mode".into(),
            description: None,
            category: ConfigCategory::Other,
            kind: ConfigOptionKind::Boolean {
                current_value: false,
            },
        },
    ]
}

#[test]
fn a_true_false_pick_after_the_effort_pick_keeps_the_effort() {
    // `fast` sorts after `effort`, so it comes second.
    let wanted: BTreeMap<_, _> = [(id("effort"), id("high")), (id("fast"), id("true"))]
        .into_iter()
        .collect();
    let picked = sets(&claude(), &wanted);
    assert_eq!(picked.effort, Some((id("effort"), id("high"))));
    assert_eq!(picked.model, None);
}

#[test]
fn an_unreported_pick_never_replaces_a_reported_effort_pick() {
    let wanted: BTreeMap<_, _> = [(id("effort"), id("high")), (id("zeal"), id("max"))]
        .into_iter()
        .collect();
    let picked = sets(&claude(), &wanted);
    assert_eq!(picked.effort, Some((id("effort"), id("high"))));
    // Alone, an unreported pick is checked as an effort pick.
    let wanted: BTreeMap<_, _> = [(id("zeal"), id("max"))].into_iter().collect();
    assert_eq!(
        sets(&claude(), &wanted).effort,
        Some((id("zeal"), id("max")))
    );
}
