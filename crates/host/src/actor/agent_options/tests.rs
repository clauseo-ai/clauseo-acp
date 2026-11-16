//! `agent-options.json`: options persist across a new `AgentOptions`, the same options write
//! nothing, and a damaged, oversized or other-version file counts as none.

use clauseo_protocol::{ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind};

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
    assert_eq!(kept.of("claude-acp"), []);
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
    assert_eq!(again.of("codex-acp"), []);
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
        assert_eq!(AgentOptions::new(data).of("claude-acp"), []);
    }
    // A damaged file is replaced whole by the next keep.
    let mut kept = AgentOptions::new(data);
    assert!(kept.keep("claude-acp", &[model("default")]).unwrap());
    assert_eq!(AgentOptions::new(data).of("claude-acp"), [model("default")]);
}
