use clauseo_protocol::{
    AgentChoiceId, CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
};

use super::{choice_name, model_before, notice_text, with_mode};

fn select(
    id: &str,
    category: ConfigCategory,
    current: &str,
    values: &[(&str, &str)],
) -> ConfigOption {
    ConfigOption {
        id: CompactString::from(id),
        name: CompactString::from(id),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: CompactString::from(current),
            options: values
                .iter()
                .map(|(value, name)| ConfigChoice {
                    value: CompactString::from(*value),
                    name: CompactString::from(*name),
                    description: None,
                    group: None,
                })
                .collect(),
        },
    }
}

fn options(mode: &str, model: &str) -> Vec<ConfigOption> {
    vec![
        select(
            "mode",
            ConfigCategory::Mode,
            mode,
            &[("auto", "Auto"), ("acceptEdits", "Accept edits")],
        ),
        select(
            "model",
            ConfigCategory::Model,
            model,
            &[("opus", "Opus"), ("haiku", "Haiku")],
        ),
        select(
            "effort",
            ConfigCategory::ThoughtLevel,
            "high",
            &[("high", "High")],
        ),
    ]
}

fn id(text: &str) -> AgentChoiceId {
    AgentChoiceId::new(text).unwrap()
}

#[test]
fn only_a_switch_of_the_model_is_watched_and_it_keeps_the_old_value() {
    let logged = options("auto", "opus");
    assert_eq!(
        model_before(&logged, &id("model")),
        Some((id("model"), id("opus")))
    );
    assert_eq!(model_before(&logged, &id("effort")), None);
    assert_eq!(model_before(&logged, &id("mode")), None);
    assert_eq!(model_before(&logged, &id("nope")), None);
}

#[test]
fn the_notice_names_both_models_by_their_names() {
    let moved = options("acceptEdits", "haiku");
    assert_eq!(choice_name(&moved, &id("model"), "haiku"), "Haiku");
    assert_eq!(choice_name(&moved, &id("model"), "opus"), "Opus");
    // A value the list does not hold is named by its id.
    assert_eq!(choice_name(&moved, &id("model"), "sonnet"), "sonnet");
    assert_eq!(
        notice_text("Haiku", "Opus"),
        "Haiku can't run in Claude's automatic mode, so Claude stays on Opus. To change the \
         model, pick another one."
    );
}

#[test]
fn the_restored_options_show_auto_and_leave_the_rest_alone() {
    let restored = with_mode(options("acceptEdits", "opus"), "auto");
    assert_eq!(restored, options("auto", "opus"));
}
