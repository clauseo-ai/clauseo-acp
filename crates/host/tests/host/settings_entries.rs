//! Desktop's two Rust-only settings writes through `HostHandle` (decision record 0048):
//! `set_agent_launch` writes an installed agent's `agents.<id>` entry, `set_plan` a firm's
//! `plans.<id>` (`set_plan_endpoint` its endpoint alone). Each sends `SettingsChanged` after a
//! write and nothing after a refusal; a locked id is `settings.locked`, a value that breaks a
//! rule `settings.refused`, whose message names the key and never the value; the lawyer's own
//! entry is left alone.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentId, EnvName, ErrorCode, HostError, HostEvent, PlanProvider, PlanView, SettingsView,
};
use clauseo_settings::{AgentLaunch, EntryWrite, Plan};

use crate::settings::{ready, settings, subscribe_app};
use crate::support::{Dir, Running, Scenario, wait};

const CLAUDE: &str = "claude-acp";

fn claude() -> AgentId {
    AgentId::new(CLAUDE).unwrap()
}

/// Claude's install as desktop would write it: the adapter under `<data>/agents/claude-acp/`,
/// with `env` added.
fn install(dir: &Dir, env: &[(&str, &str)]) -> AgentLaunch {
    let adapter = dir
        .data()
        .join("agents")
        .join(CLAUDE)
        .join("bin")
        .join("claude-agent-acp");
    AgentLaunch {
        command: adapter.display().to_string(),
        args: Vec::new(),
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

fn set_launch(host: &Running, launch: AgentLaunch) -> Result<EntryWrite, HostError> {
    wait(
        "set_agent_launch",
        host.handle.set_agent_launch(claude(), launch),
    )
}

fn set_plan(host: &Running, plan: Option<Plan>) -> Result<(), HostError> {
    wait("set_plan", host.handle.set_plan(claude(), plan))
}

/// The one `SettingsChanged` in `events`.
fn changed(events: &[HostEvent]) -> &SettingsView {
    match events {
        [HostEvent::SettingsChanged(view)] => view,
        other => panic!("expected one SettingsChanged: {other:?}"),
    }
}

/// `error` is `code`, not retryable, names `key` and holds none of `values`.
fn assert_refused(error: &HostError, code: ErrorCode, key: &str, values: &[&str]) {
    assert_eq!(error.code(), code, "{error}");
    assert!(!error.retryable(), "{error}");
    assert!(error.message().contains(key), "{error}");
    for value in values {
        assert!(!error.message().contains(value), "{error} holds a value");
    }
}

#[test]
fn set_agent_launch_writes_an_install_and_tells_every_window() {
    let dir = Dir::new("entries_launch_write", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let launch = install(&dir, &[("ANTHROPIC_API_KEY", "")]);
    assert_eq!(
        set_launch(&host, launch.clone()).unwrap(),
        EntryWrite::Written
    );
    host.settle();
    changed(&ready(&mut sub));
    let file: serde_json::Value = serde_json::from_str(&dir.settings_json()).unwrap();
    assert_eq!(file["agents"][CLAUDE]["command"], launch.command.as_str());
    // Written again, it is still Clauseo's own, so it is written again.
    assert_eq!(set_launch(&host, launch).unwrap(), EntryWrite::Written);
    host.shutdown();
}

#[test]
fn set_agent_launch_refuses_a_secret_or_a_program_not_clauseos_without_the_value() {
    let dir = Dir::new("entries_launch_refused", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let before = dir.settings_json();
    let secret = "sk-ant-not-a-real-key-7f3a";
    let error = set_launch(&host, install(&dir, &[("ANTHROPIC_API_KEY", secret)])).unwrap_err();
    assert_refused(
        &error,
        ErrorCode::SettingsRefused,
        "agents.claude-acp",
        &[secret],
    );
    let elsewhere = AgentLaunch {
        command: "/opt/elsewhere/claude-agent-acp".to_owned(),
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    let error = set_launch(&host, elsewhere).unwrap_err();
    assert_refused(
        &error,
        ErrorCode::SettingsRefused,
        "agents.claude-acp",
        &["/opt/elsewhere"],
    );
    assert_eq!(dir.settings_json(), before);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn set_agent_launch_refuses_an_id_policy_sets() {
    let dir = Dir::new("entries_launch_locked", &[]);
    let policy = r#"{"agents": {"claude-acp": {"command": "/opt/firm/claude-agent-acp"}}}"#;
    let host = dir.start_with_policy(0, policy);
    let (mut sub, _first) = subscribe_app(&host);
    let error = set_launch(&host, install(&dir, &[])).unwrap_err();
    assert_refused(&error, ErrorCode::SettingsLocked, "agents.claude-acp", &[]);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

/// The lawyer's own entry (here the tests' `fake_agent`, outside `<data>/agents/`) is never
/// replaced: the answer says so, and nothing is written or sent.
#[test]
fn set_agent_launch_leaves_the_lawyers_own_entry_alone() {
    let dir = Dir::new(
        "entries_launch_left_alone",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let before = dir.settings_json();
    assert_eq!(
        set_launch(&host, install(&dir, &[])).unwrap(),
        EntryWrite::LeftAlone
    );
    assert_eq!(dir.settings_json(), before);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn set_plan_writes_and_removes_a_plan_and_the_view_has_names_only() {
    let dir = Dir::new("entries_plan_write", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let plan = Plan {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        provider: Some(clauseo_settings::PlanProvider::Anthropic),
        env: BTreeMap::from([("AWS_REGION".to_owned(), "eu-west-1".to_owned())]),
    };
    set_plan(&host, Some(plan)).unwrap();
    host.settle();
    let expected = PlanView {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        provider: Some(PlanProvider::Anthropic),
        env_names: vec![EnvName::new("AWS_REGION").unwrap()],
    };
    let events = ready(&mut sub);
    let view = changed(&events);
    assert_eq!(view.plans, BTreeMap::from([(claude(), expected)]));
    assert_eq!(settings(&host).plans, view.plans);
    assert!(!serde_json::to_string(view).unwrap().contains("eu-west-1"));
    assert!(dir.settings_json().contains("gateway.firm.example"));

    set_plan(&host, None).unwrap();
    host.settle();
    assert_eq!(changed(&ready(&mut sub)).plans, BTreeMap::new());
    assert!(!dir.settings_json().contains("gateway.firm.example"));
    host.shutdown();
}

#[test]
fn set_plan_refuses_a_bad_address_or_variable_without_the_value() {
    let dir = Dir::new("entries_plan_refused", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let before = dir.settings_json();
    let address = Plan {
        endpoint: Some("http://gateway.firm.example/?token=hunter2".to_owned()),
        ..Plan::default()
    };
    let error = set_plan(&host, Some(address)).unwrap_err();
    assert_refused(
        &error,
        ErrorCode::SettingsRefused,
        "plans.claude-acp.endpoint",
        &["hunter2", "gateway.firm.example"],
    );
    let variable = Plan {
        env: BTreeMap::from([("SECRET_THING".to_owned(), "value-9d2c".to_owned())]),
        ..Plan::default()
    };
    let error = set_plan(&host, Some(variable)).unwrap_err();
    assert_refused(
        &error,
        ErrorCode::SettingsRefused,
        "plans.claude-acp",
        &["value-9d2c"],
    );
    assert_eq!(dir.settings_json(), before);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn set_plan_refuses_a_plan_policy_sets() {
    let dir = Dir::new("entries_plan_locked", &[]);
    let host = dir.start_with_policy(0, r#"{"plans": {"claude-acp": {"provider": "bedrock"}}}"#);
    let (mut sub, _first) = subscribe_app(&host);
    let error = set_plan(&host, None).unwrap_err();
    assert_refused(&error, ErrorCode::SettingsLocked, "plans.claude-acp", &[]);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    host.shutdown();
}

#[test]
fn set_plan_endpoint_keeps_the_provider_and_the_variables() {
    let dir = Dir::new("entries_plan_endpoint", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    let plan = Plan {
        endpoint: None,
        provider: Some(clauseo_settings::PlanProvider::Anthropic),
        env: BTreeMap::from([("AWS_REGION".to_owned(), "eu-west-1".to_owned())]),
    };
    set_plan(&host, Some(plan)).unwrap();
    host.settle();
    let _written = ready(&mut sub);
    let endpoint = |url: Option<&str>| {
        wait(
            "set_plan_endpoint",
            host.handle
                .set_plan_endpoint(claude(), url.map(str::to_owned)),
        )
    };
    endpoint(Some("https://gateway.firm.example")).unwrap();
    host.settle();
    let view = |endpoint: Option<&str>| PlanView {
        endpoint: endpoint.map(str::to_owned),
        provider: Some(PlanProvider::Anthropic),
        env_names: vec![EnvName::new("AWS_REGION").unwrap()],
    };
    assert_eq!(
        changed(&ready(&mut sub)).plans,
        BTreeMap::from([(claude(), view(Some("https://gateway.firm.example")))])
    );
    assert!(
        dir.settings_json().contains("eu-west-1"),
        "the variable's value kept"
    );
    endpoint(None).unwrap();
    host.settle();
    assert_eq!(
        changed(&ready(&mut sub)).plans,
        BTreeMap::from([(claude(), view(None))]),
        "removing the endpoint keeps the rest"
    );
    host.shutdown();
}

/// `settings.json`'s text, or nothing when no write made the file.
fn settings_text(dir: &Dir) -> String {
    std::fs::read_to_string(dir.data().join("settings.json")).unwrap_or_default()
}

fn set_endpoint(host: &Running, url: Option<&str>) -> Result<(), HostError> {
    wait(
        "set_plan_endpoint",
        host.handle
            .set_plan_endpoint(claude(), url.map(str::to_owned)),
    )
}

#[test]
fn clearing_the_endpoint_of_no_plan_writes_and_sends_nothing() {
    let dir = Dir::new("entries_plan_endpoint_none", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    set_endpoint(&host, None).unwrap();
    host.settle();
    assert_eq!(ready(&mut sub), []);
    assert!(!settings_text(&dir).contains("plans"), "no plan written");
    host.shutdown();
}

#[test]
fn clearing_a_plans_only_field_removes_the_plan() {
    let dir = Dir::new("entries_plan_endpoint_last", &[]);
    let host = dir.start(0);
    let (mut sub, _first) = subscribe_app(&host);
    set_endpoint(&host, Some("https://gateway.firm.example")).unwrap();
    host.settle();
    let _written = ready(&mut sub);
    set_endpoint(&host, None).unwrap();
    host.settle();
    assert_eq!(changed(&ready(&mut sub)).plans, BTreeMap::new());
    assert!(
        !settings_text(&dir).contains("claude-acp"),
        "no empty plan left"
    );
    host.shutdown();
}

#[test]
fn set_plan_endpoint_refuses_a_plan_policy_sets() {
    let dir = Dir::new("entries_plan_endpoint_locked", &[]);
    let host = dir.start_with_policy(0, r#"{"plans": {"claude-acp": {"provider": "bedrock"}}}"#);
    let (mut sub, _first) = subscribe_app(&host);
    let error = set_endpoint(&host, Some("https://gateway.firm.example")).unwrap_err();
    assert_refused(&error, ErrorCode::SettingsLocked, "plans.claude-acp", &[]);
    host.settle();
    assert_eq!(ready(&mut sub), []);
    assert!(
        !settings_text(&dir).contains("bedrock"),
        "the firm's plan is not copied"
    );
    host.shutdown();
}
