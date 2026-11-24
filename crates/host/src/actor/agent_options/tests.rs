//! `agent-options.json`: options persist across a new `AgentOptions`, the same options write
//! nothing, a damaged, oversized or other-version file counts as none, and an agent nothing has
//! reached gives no options and no models (there is no seed). A probe's models are kept beside
//! the options, and become the model and effort options.

use clauseo_protocol::{
    AgentModels, ConfigCategory, ConfigChoice, ConfigOption, ConfigOptionKind, ModelEntry,
    NamedChoice,
};

use super::{AgentOptions, CAP, FILE, merged};
use crate::pipeline::tests::support::TempDir;

fn model(current: &str) -> ConfigOption {
    let choice = |value: &str| ConfigChoice {
        value: value.into(),
        name: value.into(),
        description: None,
        group: None,
        takes_effort: true,
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
    assert_eq!(kept.models_of("claude-acp"), None);
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

fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

/// Claude's models as a probe finds them: Opus 5.5 at Medium, Fable 5.2, Haiku with no effort.
pub(crate) fn probed() -> AgentModels {
    let efforts = vec![
        named("low", "Low"),
        named("medium", "Medium"),
        named("high", "High"),
    ];
    let model = |id: &str, name: &str, efforts: Vec<NamedChoice>| ModelEntry {
        id: id.into(),
        name: name.into(),
        description: Some(format!("{name} described")),
        default_effort: (!efforts.is_empty()).then(|| "medium".into()),
        efforts,
    };
    AgentModels {
        model_option: "model".into(),
        effort_option: Some("effort".into()),
        models: vec![
            model("opus", "Opus 5.5", efforts.clone()),
            model("fable", "Fable 5.2", efforts),
            model("haiku", "Haiku 5.5", Vec::new()),
        ],
        current_model: "opus".into(),
        current_effort: Some("medium".into()),
    }
}

#[test]
fn a_probe_is_kept_as_models_and_as_the_model_and_effort_options() {
    let dir = TempDir::new("agent-options-probed");
    let data = dir.path();
    let mut kept = AgentOptions::new(data);
    assert!(kept.keep_probed("claude-acp", probed()).unwrap());
    assert!(
        !kept.keep_probed("claude-acp", probed()).unwrap(),
        "the same writes nothing"
    );
    let mut again = AgentOptions::new(data);
    assert_eq!(again.models_of("claude-acp"), Some(probed()));
    let options = again.of("claude-acp").to_vec();
    let shape: Vec<_> = options
        .iter()
        .map(|o| (o.id.as_str(), o.category))
        .collect();
    assert_eq!(
        shape,
        [
            ("model", ConfigCategory::Model),
            ("effort", ConfigCategory::ThoughtLevel)
        ]
    );
    let ConfigOptionKind::Select {
        current_value,
        options: choices,
    } = &options[0].kind
    else {
        panic!("no model select");
    };
    assert_eq!(current_value, "opus");
    assert_eq!(choices[0].name, "Opus 5.5");
    assert!(!choices[2].takes_effort, "Haiku takes no effort");
}

#[test]
fn a_session_report_keeps_each_models_probed_efforts() {
    let dir = TempDir::new("agent-options-probed-session");
    let mut kept = AgentOptions::new(dir.path());
    kept.keep_probed("claude-acp", probed()).unwrap();
    // A session started on Fable at High: the options say so, the efforts stay the probe's.
    let mut options = kept.of("claude-acp").to_vec();
    if let ConfigOptionKind::Select { current_value, .. } = &mut options[0].kind {
        *current_value = "fable".into();
    }
    if let ConfigOptionKind::Select { current_value, .. } = &mut options[1].kind {
        *current_value = "high".into();
    }
    kept.keep("claude-acp", &options).unwrap();
    let models = kept.models_of("claude-acp").unwrap();
    assert_eq!(models.current_model, "fable");
    assert_eq!(models.current_effort.as_deref(), Some("high"));
    assert_eq!(models.model("opus").unwrap().efforts.len(), 3);
    assert!(models.model("haiku").unwrap().efforts.is_empty());
}

#[test]
fn a_kept_value_wins_over_a_later_report_that_offers_it() {
    assert_eq!(merged(&[], &[model("opus")]), [model("opus")]);
    assert_eq!(
        merged(&[model("default")], &[model("opus")]),
        [model("default")]
    );
}
