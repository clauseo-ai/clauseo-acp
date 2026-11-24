use clauseo_protocol::{ConfigOption, ConfigOptionKind};
use serde_json::json;

use super::*;
use crate::event::{AcpEvent, AgentUpdate};
use crate::map::tests::map_update;

fn line(name: &str, description: &str) -> Option<String> {
    model_line(name, Some(description.to_owned()))
}

#[test]
fn claude_rows_drop_the_name_and_the_usage_cost() {
    assert_eq!(
        line(
            "Opus 5.5",
            "Opus 5.5 · Best for everyday, complex tasks · ~2× usage vs Sonnet"
        ),
        Some("Best for everyday, complex tasks".to_owned())
    );
    assert_eq!(
        line(
            "Fable 5.1",
            "Fable 5.1 · Deepest reasoning for hard problems · Requires usage credits"
        ),
        Some("Deepest reasoning for hard problems".to_owned())
    );
}

#[test]
fn separator_with_or_without_spaces() {
    assert_eq!(
        line(
            "Opus 5.5",
            "Best for everyday,complex tasks·~2× usage vs Sonnet"
        ),
        Some("Best for everyday,complex tasks".to_owned())
    );
    assert_eq!(
        line("Opus 5.5", "  Quick  ·   Careful  ·  Deep  "),
        Some("Quick · Careful · Deep".to_owned())
    );
}

#[test]
fn only_cost_wording_gives_none() {
    assert_eq!(line("Opus 5.5", "~2× usage vs Sonnet"), None);
    assert_eq!(line("Fable 5.1", "Requires usage credits"), None);
    assert_eq!(line("Opus 5.5", "Opus 5.5 · ~2× usage vs Sonnet"), None);
}

#[test]
fn description_without_a_dot_stays() {
    assert_eq!(
        line("Opus 5.5", "Latest frontier agentic coding model."),
        Some("Latest frontier agentic coding model.".to_owned())
    );
}

#[test]
fn vendor_prefixed_name_repeat_is_dropped() {
    assert_eq!(
        line("6.1 Sol", "GPT-6.1 Sol · Fastest for quick edits"),
        Some("Fastest for quick edits".to_owned())
    );
    assert_eq!(line("6.1 Sol", "GPT-6.1 Sol"), None);
    assert_eq!(
        line("Opus 5.5", "Claude Opus 5.5 · Keeps the name out"),
        Some("Keeps the name out".to_owned())
    );
}

#[test]
fn name_match_is_case_insensitive_and_ends_the_part() {
    assert_eq!(
        line("Opus 5.5", "claude OPUS 5.5 · Best for work"),
        Some("Best for work".to_owned())
    );
    // The name inside a longer part is not a repeat of the name.
    assert_eq!(
        line("Opus 5.5", "Opus 5.5 Pro · Best for work"),
        Some("Opus 5.5 Pro · Best for work".to_owned())
    );
}

#[test]
fn user_is_not_usage() {
    assert_eq!(
        line("Opus 5.5", "Built for user-facing drafting · Users like it"),
        Some("Built for user-facing drafting · Users like it".to_owned())
    );
}

#[test]
fn usage_words_match_whole_and_plural_only() {
    assert_eq!(
        line("Opus 5.5", "Heavy usages · Light"),
        Some("Light".to_owned())
    );
    assert_eq!(
        line("Opus 5.5", "Priced per token · Billed monthly · Fast"),
        Some("Fast".to_owned())
    );
    assert_eq!(
        line("Opus 5.5", "Costs more · Credited to you · Costume ideas"),
        Some("Credited to you · Costume ideas".to_owned())
    );
}

#[test]
fn multipliers_are_dropped_and_other_numbers_are_not() {
    assert_eq!(line("Opus 5.5", "Up to 3x faster"), None);
    assert_eq!(line("Opus 5.5", "About 2X the speed"), None);
    assert_eq!(line("Opus 5.5", "~1.5× the cost"), None);
    assert_eq!(
        line("Opus 5.5", "Version 2 · 32k context"),
        Some("Version 2 · 32k context".to_owned())
    );
}

#[test]
fn none_and_blank_give_none() {
    assert_eq!(model_line("Opus 5.5", None), None);
    assert_eq!(line("Opus 5.5", ""), None);
    assert_eq!(line("Opus 5.5", "   "), None);
    assert_eq!(line("Opus 5.5", " · · "), None);
}

#[test]
fn a_blank_name_drops_no_part() {
    assert_eq!(line("", "A · B"), Some("A · B".to_owned()));
}

/// A model row's description goes into the menu without the usage cost or the repeated name.
#[test]
fn config_choice_description_is_cleaned_with_its_name() {
    let update = json!({"sessionUpdate": "config_option_update", "configOptions": [{
        "id": "model", "name": "Model", "category": "model", "type": "select",
        "currentValue": "opus",
        "options": [
            {"value": "opus", "name": "Opus 5.5",
             "description": "Opus 5.5 · Best for everyday, complex tasks · ~2× usage vs Sonnet"},
            {"value": "fable", "name": "Fable 5.1",
             "description": "Fable 5.1 · Requires usage credits"},
        ],
    }]});
    let [
        AcpEvent::Update {
            update: AgentUpdate::ConfigOptions(options),
            ..
        },
    ] = <[AcpEvent; 1]>::try_from(map_update(update, None)).unwrap()
    else {
        panic!("expected config options");
    };
    let [option] = <[ConfigOption; 1]>::try_from(options).unwrap();
    let ConfigOptionKind::Select { options, .. } = option.kind else {
        panic!("expected a select option");
    };
    let descriptions: Vec<_> = options
        .into_iter()
        .map(|choice| (choice.name.to_string(), choice.description))
        .collect();
    assert_eq!(
        descriptions,
        [
            (
                "Opus 5.5".to_owned(),
                Some("Best for everyday, complex tasks".to_owned())
            ),
            ("Fable 5.1".to_owned(), None),
        ]
    );
}
