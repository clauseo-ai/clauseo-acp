//! `agent-options.json`: options persist across a new `AgentOptions`, the same options write
//! nothing, and a damaged, oversized or other-version file counts as none, so a Claude or
//! ChatGPT agent gives its seed: its model and effort ids, each at the agent's own default alone.

use clauseo_protocol::{ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind};

use super::seed::{self, AGENT_DEFAULT};
use super::{AgentOptions, CAP, FILE};
use crate::pipeline::tests::support::TempDir;

fn model(current: &str) -> ConfigOption {
    let choice = |value: &str| ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
        group: None,
    };
    ConfigOption {
        id: "model".into(),
        name: "Model".into(),
        description: None,
        category: ConfigCategory::Model,
        kind: ConfigOptionKind::Select {
            current_value: current.into(),
            options: vec![choice("default"), choice("opus")],
        },
    }
}

#[test]
fn options_are_written_and_read_back() {
    let dir = TempDir::new("agent-options-kept");
    let data = dir.path();
    let mut kept = AgentOptions::new(data);
    assert_eq!(kept.of("claude-acp"), seed::of("claude-acp"));
    assert_eq!(kept.of("fake-agent"), []);
    assert!(kept.keep("claude-acp", &[model("default")]).unwrap());
    assert!(
        !kept.keep("claude-acp", &[model("default")]).unwrap(),
        "the same writes nothing"
    );
    assert!(kept.keep("claude-acp", &[model("opus")]).unwrap());
    let disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(data.join(FILE)).unwrap()).unwrap();
    assert_eq!(disk["v"], 1);
    assert_eq!(disk["agents"]["claude-acp"][0]["category"], "model");
    let mut again = AgentOptions::new(data);
    assert_eq!(again.of("claude-acp"), [model("opus")]);
    assert_eq!(again.of("codex-acp"), seed::of("codex-acp"));
}

#[test]
fn a_damaged_oversized_or_other_version_file_counts_as_none() {
    let dir = TempDir::new("agent-options-damaged");
    let data = dir.path();
    let oversized = format!(
        r#"{{"v":1,"pad":"{}"}}"#,
        "x".repeat(usize::try_from(CAP).unwrap())
    );
    for bytes in [
        b"{".to_vec(),
        br#"{"v":2,"agents":{"claude-acp":[]}}"#.to_vec(),
        oversized.into_bytes(),
    ] {
        crate::write::replace_private_file(&data.join(FILE), &bytes).unwrap();
        assert_eq!(
            AgentOptions::new(data).of("claude-acp"),
            seed::of("claude-acp")
        );
    }
    // A damaged file is replaced whole by the next keep.
    let mut kept = AgentOptions::new(data);
    assert!(kept.keep("claude-acp", &[model("default")]).unwrap());
    assert_eq!(AgentOptions::new(data).of("claude-acp"), [model("default")]);
}

/// The ids, categories and the one value of each seed, the shape the agent reports.
#[test]
fn each_seed_is_the_agents_model_and_effort_at_its_own_default() {
    for (agent, ids) in [
        ("claude-acp", ["model", "effort"]),
        ("codex-acp", ["model", "reasoning_effort"]),
    ] {
        let options = seed::of(agent);
        let shape: Vec<_> = options
            .iter()
            .map(|option| (option.id.as_str(), option.category))
            .collect();
        assert_eq!(
            shape,
            [
                (ids[0], ConfigCategory::Model),
                (ids[1], ConfigCategory::ThoughtLevel)
            ],
            "{agent}"
        );
        for option in options {
            let ConfigOptionKind::Select {
                current_value,
                options,
            } = &option.kind
            else {
                panic!("{agent}'s {} is no select", option.id);
            };
            assert_eq!(current_value, AGENT_DEFAULT);
            let values: Vec<_> = options.iter().map(|choice| choice.value.as_str()).collect();
            assert_eq!(values, [AGENT_DEFAULT], "{agent}: no row but the default");
        }
    }
    assert_eq!(seed::of("fake-agent"), []);
}
