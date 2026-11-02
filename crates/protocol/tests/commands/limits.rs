//! `refresh_limits`: its wire form, the optional agent, the closed `why`, and that it passes
//! `validate()`. What a refresh does is the host's and `limits`' (`crates/host`, `crates/limits`).

use clauseo_protocol::{AgentKind, CommandEnvelope, HostCommand, RefreshWhy};

use super::Validate;
use super::fixtures::{command_id, round_trips};

#[test]
fn a_refresh_for_every_agent_leaves_the_agent_out() {
    let command = HostCommand::RefreshLimits {
        agent: None,
        why: RefreshWhy::TryAgain,
    };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"refresh_limits":{"why":"try_again"}}"#
    );
    round_trips(&command);
    assert!(command.validate().is_ok());
}

#[test]
fn a_refresh_for_one_agent_names_it() {
    let command = HostCommand::RefreshLimits {
        agent: Some(AgentKind::Codex),
        why: RefreshWhy::Shown,
    };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"refresh_limits":{"agent":"codex","why":"shown"}}"#
    );
    round_trips(&command);
    assert!(command.validate().is_ok());
}

#[test]
fn a_missing_agent_means_every_agent() {
    let json = patch_command(r#"{"why":"shown"}"#);
    let envelope: CommandEnvelope = serde_json::from_str(&json).unwrap();
    assert_eq!(
        envelope.command,
        HostCommand::RefreshLimits {
            agent: None,
            why: RefreshWhy::Shown,
        }
    );
}

#[test]
fn a_refresh_refuses_an_unknown_why_agent_or_field() {
    for body in [
        r#"{"why":"sometimes"}"#,
        r#"{"why":"shown","agent":"fake-agent"}"#,
        r#"{"why":"shown","agent":null,"force":true}"#,
        r"{}",
    ] {
        let json = patch_command(body);
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{body} decoded"
        );
    }
}

#[test]
fn every_why_has_its_snake_case_name() {
    assert_eq!(
        serde_json::to_string(&[RefreshWhy::Shown, RefreshWhy::TryAgain]).unwrap(),
        r#"["shown","try_again"]"#
    );
}

/// A `refresh_limits` command's JSON around `body`.
fn patch_command(body: &str) -> String {
    format!(
        r#"{{"command_id":"{}","command":{{"refresh_limits":{body}}}}}"#,
        command_id(1)
    )
}
