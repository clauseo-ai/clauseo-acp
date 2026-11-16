//! A custom endpoint from the app's own environment or the agent's entry, with no plan: its host,
//! and when it is none.

use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_protocol::AgentId;
use clauseo_settings::{Plan, PlanProvider};

use super::inherited_host;

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn env(vars: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    vars.iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn plan(endpoint: Option<&str>, provider: Option<PlanProvider>) -> Plan {
    Plan {
        endpoint: endpoint.map(str::to_owned),
        provider,
        env: BTreeMap::new(),
    }
}

/// The app's own base URL, no plan: its host, with or without a key in the app's env (the hook
/// sets the key from the keys), and from the entry over the app's env.
#[test]
fn an_inherited_base_url_with_no_plan_names_its_host() {
    let url = env(&[("ANTHROPIC_BASE_URL", "https://proxy.example:8443/v1")]);
    let host = Some("proxy.example".to_owned());
    assert_eq!(inherited_host(&claude(), None, None, &url), host);
    let keyed = env(&[
        ("ANTHROPIC_BASE_URL", "https://proxy.example:8443/v1"),
        ("ANTHROPIC_API_KEY", "sk-of-the-app"),
    ]);
    assert_eq!(inherited_host(&claude(), None, None, &keyed), host);
    let anthropic = plan(None, Some(PlanProvider::Anthropic));
    assert_eq!(
        inherited_host(&claude(), Some(&anthropic), None, &url),
        host
    );
    let entry = BTreeMap::from([(
        "ANTHROPIC_BASE_URL".to_owned(),
        "https://entry.example".to_owned(),
    )]);
    assert_eq!(
        inherited_host(&claude(), None, Some(&entry), &url),
        Some("entry.example".to_owned())
    );
    assert_eq!(
        inherited_host(&claude(), None, Some(&entry), &[]),
        Some("entry.example".to_owned())
    );
}

/// None when a plan sets the endpoint or a cloud provider, for another agent, with a token or
/// a provider switch in the env, with the default address, or with no base URL.
#[test]
fn no_inherited_endpoint_under_a_plan_a_switch_or_the_default_address() {
    let url = env(&[("ANTHROPIC_BASE_URL", "https://proxy.example")]);
    let firm = plan(Some("https://gateway.firm.example"), None);
    assert_eq!(inherited_host(&claude(), Some(&firm), None, &url), None);
    let bedrock = plan(None, Some(PlanProvider::Bedrock));
    assert_eq!(inherited_host(&claude(), Some(&bedrock), None, &url), None);
    let codex = AgentId::new("codex-acp").unwrap();
    assert_eq!(inherited_host(&codex, None, None, &url), None);
    for off in [
        ("ANTHROPIC_AUTH_TOKEN", "t"),
        ("CLAUDE_CODE_USE_VERTEX", "1"),
    ] {
        let vars = env(&[("ANTHROPIC_BASE_URL", "https://proxy.example"), off]);
        assert_eq!(
            inherited_host(&claude(), None, None, &vars),
            None,
            "{off:?}"
        );
    }
    let default = env(&[("ANTHROPIC_BASE_URL", "https://api.anthropic.com/")]);
    assert_eq!(inherited_host(&claude(), None, None, &default), None);
    assert_eq!(
        inherited_host(&claude(), None, None, &env(&[("PATH", "x")])),
        None
    );
}
