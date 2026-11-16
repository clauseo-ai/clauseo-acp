//! The model and effort pickers' fields: `CreateThread::config`, `UpdateMatter::agent_prefs`
//! and `AgentSummary::config_options`.

use clauseo_protocol::{
    AgentId, AgentKind, AgentPrefs, AgentSummary, ConfigCategory, ConfigChoice, ConfigOption,
    ConfigOptionKind, ConfigPicks, FieldChange, HostCommand, MAX_CONFIG_PICKS,
};

use super::Validate;
use super::fixtures::{agent, choice, matter_id, round_trips, short};

fn picks(pairs: &[(&str, &str)]) -> ConfigPicks {
    ConfigPicks::new(pairs.iter().map(|(o, v)| (choice(o), choice(v))).collect()).unwrap()
}

#[test]
fn create_thread_config_is_missing_empty_or_picks() {
    let matter = matter_id(1);
    let decode = |fields: &str| -> HostCommand {
        let json =
            format!(r#"{{"create_thread":{{"matter":"{matter}","agent":"claude-acp"{fields}}}}}"#);
        serde_json::from_str(&json).unwrap()
    };
    let with = |config| HostCommand::CreateThread {
        matter,
        agent: agent("claude-acp"),
        config,
    };
    assert_eq!(decode(""), with(None));
    assert_eq!(
        decode(r#","config":{}"#),
        with(Some(ConfigPicks::default()))
    );
    let picked = decode(r#","config":{"model":"opus","effort":"high"}"#);
    assert_eq!(
        picked,
        with(Some(picks(&[("model", "opus"), ("effort", "high")])))
    );
    for command in [decode(""), decode(r#","config":{}"#), picked] {
        round_trips(&command);
        assert!(command.validate().is_ok());
    }
    let missing = serde_json::to_string(&with(None)).unwrap();
    assert!(!missing.contains("config"), "{missing}");

    let too_many: String = (0..=MAX_CONFIG_PICKS)
        .map(|n| format!(r#""o{n}":"v""#))
        .collect::<Vec<_>>()
        .join(",");
    for bad in [
        format!(r#","config":{{{too_many}}}"#),
        r#","config":{"model":true}"#.to_owned(),
        r#","config":{"model":""}"#.to_owned(),
        r#","config":["model"]"#.to_owned(),
    ] {
        let json = format!(r#"{{"create_thread":{{"matter":"{matter}","agent":"x"{bad}}}}}"#);
        assert!(serde_json::from_str::<HostCommand>(&json).is_err(), "{bad}");
    }
}

#[test]
fn update_matter_agent_prefs_keeps_clears_and_sets() {
    let matter = matter_id(1);
    let prefs = |fields: &str| -> FieldChange<AgentPrefs> {
        let json = format!(r#"{{"update_matter":{{"matter":"{matter}"{fields}}}}}"#);
        let HostCommand::UpdateMatter { agent_prefs, .. } = serde_json::from_str(&json).unwrap()
        else {
            panic!("not an update");
        };
        agent_prefs
    };
    assert_eq!(prefs(""), FieldChange::Keep);
    assert_eq!(prefs(r#","agent_prefs":null"#), FieldChange::Clear);
    let claude = picks(&[("model", "opus")]);
    let set = AgentPrefs(std::collections::BTreeMap::from([(
        AgentKind::Claude,
        claude,
    )]));
    assert_eq!(
        prefs(r#","agent_prefs":{"claude":{"model":"opus"}}"#),
        FieldChange::Set(set.clone())
    );
    let command = HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: FieldChange::Set(set),
    };
    round_trips(&command);
    assert!(command.validate().is_ok());
    // A kind this version does not know is refused, as an unknown field is.
    let json =
        format!(r#"{{"update_matter":{{"matter":"{matter}","agent_prefs":{{"gemini":{{}}}}}}}}"#);
    assert!(serde_json::from_str::<HostCommand>(&json).is_err());
}

#[test]
fn an_agent_summary_carries_its_cached_options() {
    let option = ConfigOption {
        id: short("effort"),
        name: short("Effort"),
        description: None,
        category: ConfigCategory::ThoughtLevel,
        kind: ConfigOptionKind::Select {
            current_value: short("default"),
            options: vec![ConfigChoice {
                value: short("default"),
                name: short("Default"),
                description: None,
                group: None,
            }],
        },
    };
    let summary = AgentSummary {
        id: AgentId::new("claude-acp").unwrap(),
        kind: Some(AgentKind::Claude),
        config_options: vec![option],
    };
    let json = serde_json::to_string(&summary).unwrap();
    assert!(
        json.contains(r#""config_options":[{"id":"effort""#),
        "{json}"
    );
    assert!(json.contains(r#""category":"thought_level""#), "{json}");
    round_trips(&summary);
    // None cached yet: left out, and a missing field reads as none.
    let bare: AgentSummary = serde_json::from_str(r#"{"id":"claude-acp"}"#).unwrap();
    assert_eq!(bare.config_options, []);
    assert!(
        !serde_json::to_string(&bare)
            .unwrap()
            .contains("config_options")
    );
}
