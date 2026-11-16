//! What a host.log line about picks writes of a model or effort value (`model_and_effort`).

use clauseo_protocol::{
    AgentChoiceId, CompactString, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind,
};

use super::{Pick, model_and_effort};

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
        model_and_effort(&options, &picks),
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
    assert_eq!(model_and_effort(&options, &picks), ("-", "-"));
    // A short id the option does not list.
    let picks = [pick("model", "sonnet"), pick("effort", "high")];
    assert_eq!(model_and_effort(&options, &picks), ("-", "high"));
    // Too long, though listed and plain.
    let long = "a".repeat(41);
    let options = [select("model", ConfigCategory::Model, &[long.as_str()])];
    assert_eq!(
        model_and_effort(&options, &[pick("model", &long)]),
        ("-", "-")
    );
}
