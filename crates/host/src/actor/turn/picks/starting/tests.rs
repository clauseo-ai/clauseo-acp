//! The starting effort's first pick, with a test-only value through `first_pick`'s parameter,
//! and the values each agent kind ships with.

use clauseo_protocol::{
    AgentChoiceId, AgentKind, CompactString, ConfigCategory, ConfigChoice, ConfigOption,
    ConfigOptionKind,
};

use super::{Pick, first_pick, of};

fn select(id: &str, category: ConfigCategory, current: &str, values: &[&str]) -> ConfigOption {
    ConfigOption {
        id: CompactString::from(id),
        name: CompactString::from(id),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: CompactString::from(current),
            options: values
                .iter()
                .map(|value| ConfigChoice {
                    value: CompactString::from(*value),
                    name: CompactString::from(*value),
                    description: None,
                    group: None,
                    takes_effort: true,
                })
                .collect(),
        },
    }
}

/// Codex's start: gpt-6.2-sol at low, with its efforts.
fn start() -> Vec<ConfigOption> {
    vec![
        select(
            "model",
            ConfigCategory::Model,
            "gpt-6.2-sol",
            &["gpt-6.2-sol", "gpt-6-luna"],
        ),
        select(
            "reasoning_effort",
            ConfigCategory::ThoughtLevel,
            "low",
            &["low", "medium", "high"],
        ),
    ]
}

fn pick(option: &str, value: &str) -> Pick {
    (
        AgentChoiceId::new(option).unwrap(),
        AgentChoiceId::new(value).unwrap(),
    )
}

#[test]
fn it_is_the_first_pick_on_a_fresh_agent() {
    assert_eq!(
        first_pick(Some("medium"), true, &[], &start()),
        Some(pick("reasoning_effort", "medium"))
    );
    // With a model pick that keeps the starting model, too.
    let same = [pick("model", "gpt-6.2-sol")];
    assert_eq!(
        first_pick(Some("medium"), true, &same, &start()),
        Some(pick("reasoning_effort", "medium"))
    );
}

#[test]
fn a_lawyers_pick_wins_and_stays() {
    // An effort the lawyer chose (config, matter prefs, a waiting set), even one the start holds.
    for chosen in ["high", "low"] {
        let picks = [pick("reasoning_effort", chosen)];
        assert_eq!(first_pick(Some("medium"), true, &picks, &start()), None);
    }
    // A reopened conversation keeps the values it ran with.
    assert_eq!(first_pick(Some("medium"), false, &[], &start()), None);
}

#[test]
fn not_when_the_model_lacks_the_effort_or_a_pick_changes_the_model() {
    assert_eq!(first_pick(Some("ultra"), true, &[], &start()), None);
    let other = [pick("model", "gpt-6-luna")];
    assert_eq!(first_pick(Some("medium"), true, &other, &start()), None);
    // A model with no effort option at all.
    let bare = vec![start().remove(0)];
    assert_eq!(first_pick(Some("medium"), true, &[], &bare), None);
    // Held already: nothing to set.
    assert_eq!(first_pick(Some("low"), true, &[], &start()), None);
}

#[test]
fn with_none_nothing_changes() {
    assert_eq!(first_pick(None, true, &[], &start()), None);
}

#[test]
fn claude_starts_at_medium_and_codex_at_low() {
    assert_eq!(of(AgentKind::Claude), Some("medium"));
    assert_eq!(of(AgentKind::Codex), Some("low"));
}
