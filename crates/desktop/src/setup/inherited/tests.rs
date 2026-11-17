//! A custom endpoint from the app's own environment or the agent's entry, with no plan: its host,
//! and when it is none.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::Clock;
use clauseo_protocol::{AgentId, ErrorCode, SetupEvent, SignIn};
use clauseo_settings::{Plan, PlanProvider};

use super::{Inherited, inherited_host};
use crate::bridge::lock;
use crate::setup::SetupEvents;
use crate::setup::compose::Compose;

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn env(vars: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    vars.iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

fn shown(host: &str) -> Inherited {
    Inherited {
        host: Some(host.to_owned()),
        setting: None,
    }
}

/// An endpoint with no host, its address in `setting`.
fn unnamed(setting: Option<&'static str>) -> Inherited {
    Inherited {
        host: None,
        setting,
    }
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
    let host = Some(shown("proxy.example"));
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
        Some(shown("entry.example"))
    );
    assert_eq!(
        inherited_host(&claude(), None, Some(&entry), &[]),
        Some(shown("entry.example"))
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

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// The log's lines, for a test to read.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An address that does not parse, or parses with no host, is still a custom endpoint, but no
/// part of it reaches the card or the log: it may hold a path, a query or a token; the card
/// names its variable. The API's own address variable counts as the base URL does.
#[test]
fn an_unreadable_address_shows_no_part_of_itself() {
    let api = env(&[("CLAUDE_CODE_API_BASE_URL", "https://api.firm.example/v1")]);
    assert_eq!(
        inherited_host(&claude(), None, None, &api),
        Some(shown("api.firm.example"))
    );
    let parts = ["gw.firm.example", "token", "sk-ant-x123", "gw.sock", "/p"];
    for value in [
        "gw.firm.example/p?token=sk-ant-x123",
        "unix:gw.sock?token=sk-ant-x123",
        "https://?token=sk-ant-x123",
    ] {
        let vars = env(&[("ANTHROPIC_BASE_URL", value)]);
        let endpoint = inherited_host(&claude(), None, None, &vars);
        assert_eq!(
            endpoint,
            Some(unnamed(Some("ANTHROPIC_BASE_URL"))),
            "{value}"
        );
        let events = Arc::new(SetupEvents::default());
        let mut feed = events.follow();
        let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            compose.set_inherited(&claude(), endpoint);
        });
        let sent: Vec<Arc<str>> = std::iter::from_fn(|| feed.try_recv().ok()).collect();
        assert_eq!(sent.len(), 1, "{value}");
        let value_of: serde_json::Value = serde_json::from_str(&sent[0]).unwrap();
        let event: SetupEvent = serde_json::from_value(value_of["setup"].clone()).unwrap();
        assert!(
            matches!(
                event,
                SetupEvent::SignInChanged {
                    state: SignIn::Failed {
                        code: ErrorCode::AgentEndpointKeyNeeded,
                        host: None,
                        setting: Some(ref setting),
                        ..
                    },
                    ..
                } if setting == "ANTHROPIC_BASE_URL"
            ),
            "{value}"
        );
        let log = String::from_utf8(lock(&lines.0).clone()).unwrap();
        assert!(log.contains("sign-in state changed"), "{log}");
        for part in parts {
            assert!(!sent[0].contains(part), "{part} reached the card");
            assert!(!log.contains(part), "{part} reached the log");
        }
    }
}

/// The card names the variable whose address does not parse: `ANTHROPIC_BASE_URL`, or
/// `CLAUDE_CODE_API_BASE_URL` when that one holds it. A socket alone is an endpoint with neither
/// a host nor a setting, so the card asks for a key; beside a base URL that does not parse, the
/// base URL is named.
/// A case: the variables set, and the setting the card names.
type Case<'a> = (&'a [(&'static str, &'static str)], Option<&'static str>);

#[test]
fn an_unreadable_address_names_its_variable_and_a_socket_alone_names_none() {
    let socket = ("ANTHROPIC_UNIX_SOCKET", "gw.sock");
    let default = ("ANTHROPIC_BASE_URL", "https://api.anthropic.com");
    let cases: [Case<'_>; 5] = [
        (
            &[("ANTHROPIC_BASE_URL", "gw.firm.example/p")],
            Some("ANTHROPIC_BASE_URL"),
        ),
        (
            &[("CLAUDE_CODE_API_BASE_URL", "unix:x")],
            Some("CLAUDE_CODE_API_BASE_URL"),
        ),
        (
            &[default, ("CLAUDE_CODE_API_BASE_URL", "gw.firm.example/p")],
            Some("CLAUDE_CODE_API_BASE_URL"),
        ),
        (&[socket], None),
        (
            &[socket, ("ANTHROPIC_BASE_URL", "gw.firm.example/p")],
            Some("ANTHROPIC_BASE_URL"),
        ),
    ];
    for (vars, setting) in cases {
        let endpoint = inherited_host(&claude(), None, None, &env(vars));
        assert_eq!(endpoint, Some(unnamed(setting)), "{vars:?}");
    }
    let keyed = env(&[socket, ("ANTHROPIC_API_KEY", "k")]);
    assert_eq!(
        inherited_host(&claude(), None, None, &keyed),
        Some(unnamed(None))
    );
}
