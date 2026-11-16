//! What a host.log line about picks writes of a model or effort value (`model_and_effort`: the
//! value, `held` for a pick its option held already, else `-`; and
//! `pick_value` for a pick the agent failed).

use clauseo_protocol::{
    AgentChoiceId, CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
};

use super::{Pick, model_and_effort, pick_value};

const ARN: &str = "arn:aws:bedrock:us-east-1:123456789012:inference-profile/x";

fn select(id: &str, category: ConfigCategory, values: &[&str]) -> ConfigOption {
    ConfigOption {
        id: CompactString::from(id),
        name: CompactString::from(id),
        description: None,
        category,
        kind: ConfigOptionKind::Select {
            current_value: CompactString::from(values[0]),
            options: values
                .iter()
                .map(|value| ConfigChoice {
                    value: CompactString::from(*value),
                    name: CompactString::from(*value),
                    description: None,
                    group: None,
                })
                .collect(),
        },
    }
}

fn pick(option: &str, value: &str) -> Pick {
    (
        AgentChoiceId::new(option).unwrap(),
        AgentChoiceId::new(value).unwrap(),
    )
}

#[test]
fn a_listed_short_id_logs_as_itself() {
    let options = [
        select(
            "model",
            ConfigCategory::Model,
            &["opus", "claude-sonnet-5-5"],
        ),
        select("effort", ConfigCategory::ThoughtLevel, &["low", "high"]),
    ];
    let picks = [pick("model", "claude-sonnet-5-5"), pick("effort", "low")];
    assert_eq!(
        model_and_effort(&options, &picks, &[]),
        ("claude-sonnet-5-5", "low")
    );
}

#[test]
fn an_arn_or_an_unlisted_value_logs_as_a_dash() {
    // Listed, but not a short plain id.
    let options = [
        select("model", ConfigCategory::Model, &["opus", ARN]),
        select("effort", ConfigCategory::ThoughtLevel, &["low", "high"]),
    ];
    let picks = [pick("model", ARN), pick("effort", "max")];
    assert_eq!(model_and_effort(&options, &picks, &[]), ("-", "-"));
    // A short id the option does not list.
    let picks = [pick("model", "sonnet"), pick("effort", "high")];
    assert_eq!(model_and_effort(&options, &picks, &[]), ("-", "high"));
    // Too long, though listed and plain.
    let long = "a".repeat(41);
    let options = [select("model", ConfigCategory::Model, &[long.as_str()])];
    assert_eq!(
        model_and_effort(&options, &[pick("model", &long)], &[]),
        ("-", "-")
    );
}

/// "a config pick failed; kept" writes the pick's value as `model_and_effort` would: an ARN the
/// option lists, or a pick of an option the session lacks, logs `value=-`.
#[test]
fn a_failed_pick_logs_its_value_only_when_it_is_a_listed_short_id() {
    let options = [
        select("model", ConfigCategory::Model, &["opus", ARN]),
        select("effort", ConfigCategory::ThoughtLevel, &["low", "high"]),
    ];
    assert_eq!(pick_value(&options, &pick("model", ARN)), "-");
    assert_eq!(pick_value(&options, &pick("model", "opus")), "opus");
    assert_eq!(pick_value(&options, &pick("effort", "max")), "-");
    assert_eq!(pick_value(&options, &pick("speed", "fast")), "-");
}

/// "picks sent before first prompt": (a) a pick left out because the session held its value
/// already logs `held`; (b) an unlisted or ARN-like value logs `-`, and so does a slot with no
/// pick at all; (c) a sent listed short value logs as itself.
#[test]
fn a_held_pick_logs_held_and_a_missing_one_a_dash() {
    let options = [
        select("model", ConfigCategory::Model, &["opus", "haiku", ARN]),
        select("effort", ConfigCategory::ThoughtLevel, &["low", "high"]),
    ];
    // (a) and (c): effort sent, model held at the session's current value.
    let sent = [pick("effort", "high")];
    let held = [pick("model", "opus")];
    assert_eq!(model_and_effort(&options, &sent, &held), ("held", "high"));
    // (b): a sent ARN is not loggable, however it was held or not; no effort pick at all.
    let sent = [pick("model", ARN)];
    assert_eq!(model_and_effort(&options, &sent, &[]), ("-", "-"));
    let sent = [pick("model", "sonnet")];
    assert_eq!(model_and_effort(&options, &sent, &[]), ("-", "-"));
    // A sent value wins over a held one of the same option.
    let sent = [pick("model", "haiku")];
    assert_eq!(
        model_and_effort(&options, &sent, &[pick("model", "opus")]),
        ("haiku", "-")
    );
}
