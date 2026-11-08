//! The session proxy's config: what it allows and refuses silently, and the denial sink, which
//! only counts.

use std::sync::Arc;

use clauseo_protocol::AgentId;
use clauseo_sandbox::ports::{ListenerFor, TIER_B_PORTS};
use clauseo_sandbox::{Denial, DenialSink as _, HostRefusal, ProxyConfig, Upstream, start_proxy};

use super::{CountDenials, proxy_config};
use crate::actor::input::Input;
use crate::config::SessionKey;

fn agent(name: &str) -> AgentId {
    AgentId::new(name).unwrap()
}

fn config(name: &str) -> ProxyConfig {
    proxy_config(&agent(name), Arc::new(CountDenials::default()), None, true)
}

/// The browser listener is asked for only by a launch that offers the browser.
#[test]
fn the_browser_listener_follows_the_launch() {
    let denials = || Arc::new(CountDenials::default());
    assert!(proxy_config(&agent("claude-acp"), denials(), None, true).browser);
    assert!(!proxy_config(&agent("claude-acp"), denials(), None, false).browser);
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

/// The config refuses from the block list it is given, the very `Current` the updater swaps;
/// with none, an empty one.
#[test]
fn the_config_shares_the_block_list_it_is_given() {
    let current = Arc::new(clauseo_blocklist::Current::default());
    let denials = || Arc::new(CountDenials::default());
    let shared = proxy_config(
        &agent("claude-acp"),
        denials(),
        Some(Arc::clone(&current)),
        true,
    );
    assert!(Arc::ptr_eq(&shared.block, &current));
    let none = proxy_config(&agent("claude-acp"), denials(), None, true);
    assert!(!Arc::ptr_eq(&none.block, &current));
    assert!(none.block.get().is_unloaded());
}

/// A reporting sink posts each block-list refusal, the session's key and the host only, and
/// nothing for any other refusal; it counts them all.
#[test]
fn the_sink_posts_each_blocked_site_and_nothing_else() {
    let key = SessionKey {
        matter: clauseo_protocol::MatterId::new(uuid::Uuid::now_v7()).unwrap(),
        thread: clauseo_protocol::ThreadId::new(uuid::Uuid::now_v7()).unwrap(),
        launch: 3,
    };
    let (post, inputs) = async_channel::unbounded();
    let sink = CountDenials::reporting(key, post);
    let generated = clauseo_blocklist::Generated::parse("2026-11-07T06:00:00Z").unwrap();
    let blocked = || Denial::Blocked {
        host: "evil.example".to_owned(),
        port: 443,
        listed: "evil.example".to_owned(),
        tag: clauseo_blocklist::Tag::Phishing,
        generated,
    };
    sink.denied(blocked());
    sink.denied(Denial::Host {
        host: "client-site.example".to_owned(),
        port: 443,
        why: HostRefusal::NotAllowed,
        shared_hosting: false,
    });
    sink.denied(blocked());
    assert_eq!(sink.count(), 3);
    let mut posted = Vec::new();
    while let Ok(input) = inputs.try_recv() {
        let Input::SiteBlocked { session, site } = input else {
            panic!("{input:?}");
        };
        assert_eq!(session, key);
        assert_eq!(site.host, "evil.example");
        assert_eq!(site.tag, clauseo_blocklist::Tag::Phishing);
        assert_eq!(site.generated, generated);
        posted.push(site);
    }
    assert_eq!(posted.len(), 2, "once per refusal");
}
