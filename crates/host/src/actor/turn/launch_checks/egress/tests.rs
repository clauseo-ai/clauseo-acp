//! The session proxy's config: what it allows and refuses silently, and the denial sink, which
//! only counts.

use std::sync::Arc;

use clauseo_protocol::AgentId;
use clauseo_sandbox::ports::{ListenerFor, TIER_B_PORTS};
use clauseo_sandbox::{Denial, DenialSink as _, HostRefusal, ProxyConfig, Upstream, start_proxy};

use super::{CountDenials, proxy_config};

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn config(name: &str) -> ProxyConfig {
    proxy_config(&agent(name), Arc::new(CountDenials::default()), true)
}

/// The browser listener is asked for only by a launch that offers the browser.
#[test]
fn the_browser_listener_follows_the_launch() {
    let denials = || Arc::new(CountDenials::default());
    assert!(proxy_config(&agent("claude-acp"), denials(), true).browser);
    assert!(!proxy_config(&agent("claude-acp"), denials(), false).browser);
}

/// A session's proxy is a tier-C launch's until R17 PR-E passes `ListenerFor::TierB` for a
/// tier-B one: its agent listener, and its browser listener, stay outside `TIER_B_PORTS`, where
/// every pool user's static permit reaches (S15 PR7).
#[test]
fn the_session_proxy_listens_outside_the_tier_b_ports() {
    let config = config("claude-acp");
    assert_eq!(config.listener, ListenerFor::Other);
    let proxy = start_proxy(config).unwrap();
    assert!(!TIER_B_PORTS.contains(&proxy.port()), "{}", proxy.port());
    let browser = proxy.browser_port().unwrap();
    assert!(!TIER_B_PORTS.contains(&browser), "{browser}");
}

/// Claude's session reaches its API and the seed sites; Chrome's background hosts are silent;
/// the upstream is the system's; nothing is recorded.
#[test]
fn a_claude_session_allows_its_api_and_the_seed_sites() {
    let config = config("claude-acp");
    assert!(config.allow.contains("api.anthropic.com"));
    assert!(config.allow.contains("sci.gov.in"));
    assert!(config.allow.contains("delhihighcourt.nic.in"));
    assert!(!config.allow.contains("api.openai.com"));
    assert!(!config.allow.contains("example.com"));
    assert!(config.silent.contains("update.googleapis.com"));
    assert!(!config.silent.contains("sci.gov.in"));
    assert!(matches!(config.upstream, Upstream::System));
    assert!(config.record.is_none());
}

/// Codex's session gets `OpenAI`'s hosts; an agent without a kind gets only the seed sites.
#[test]
fn each_kind_gets_its_own_hosts() {
    let codex = config("codex-acp");
    assert!(codex.allow.contains("api.openai.com"));
    assert!(!codex.allow.contains("api.anthropic.com"));
    let other = config("fake");
    assert!(!other.allow.contains("api.openai.com"));
    assert!(!other.allow.contains("api.anthropic.com"));
    assert!(other.allow.contains("sci.gov.in"));
}

/// The sink counts refusals, a rate-cap summary as its count.
#[test]
fn the_denial_sink_counts_each_refusal() {
    let sink = CountDenials::default();
    sink.denied(Denial::Host {
        host: "client-site.example".to_owned(),
        port: 443,
        why: HostRefusal::NotAllowed,
        shared_hosting: false,
    });
    sink.denied(Denial::Dropped { count: 4 });
    assert_eq!(sink.count(), 5);
}
