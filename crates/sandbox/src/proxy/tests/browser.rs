//! The browser listener is bound only for a launch whose policy gives the browser reach.

use std::sync::Arc;

use super::harness::{Rig, Setup, Sink, echo, rig};
use crate::test_support::TempDir;
use crate::{Allowlist, Loopback, Policy, ProxyConfig, Upstream, start_proxy};

fn echo_rig(browser: bool) -> Rig {
    let rig = rig(Setup {
        allow: vec!["api.anthropic.com"],
        browser,
        ..Setup::default()
    });
    rig.net.name("api.anthropic.com", &["192.0.2.10"]);
    rig.net.route("192.0.2.10:443", echo());
    rig
}

#[test]
fn a_launch_without_the_browser_binds_only_the_agent_listener() {
    let rig = echo_rig(false);
    assert_eq!(rig.handle.browser_port(), None);
    let debug = format!("{:?}", rig.handle);
    assert!(debug.contains("browser_port: None"), "{debug}");
    // The agent listener still serves.
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
}

#[test]
fn a_launch_with_the_browser_binds_both_listeners() {
    let rig = echo_rig(true);
    let browser = rig.handle.browser_port().expect("a browser listener");
    assert_ne!(browser, rig.handle.port());
    assert_eq!(rig.connect("api.anthropic.com:443").1, 200);
    // Something listens on the browser port.
    drop(rig.browser());
}

#[test]
fn loopback_ports_without_the_browser_bind_one_listener() {
    // Tier B: a relay's loopback port in the policy, and no browser. The config's default
    // binds no browser listener whatever the policy's loopback says.
    let config = ProxyConfig::new(
        Allowlist::default(),
        Allowlist::default(),
        Upstream::Direct,
        Arc::new(Sink::default()),
    );
    assert!(!config.browser);
    let handle = start_proxy(config).unwrap();
    let dir = TempDir::new("proxy-relay-only");
    dir.dir("m");
    dir.dir("s");
    let relay = 61_150;
    let policy = Policy::builder(dir.path().join("m"), dir.path().join("s"))
        .home(dir.path().join("home"))
        .proxy_port(handle.port())
        .loopback(Loopback::Ports(vec![relay]))
        .build()
        .unwrap();
    assert_eq!(policy.loopback(), &Loopback::Ports(vec![relay]));
    assert_eq!(handle.browser_port(), None);
    assert_eq!(policy.proxy_ports(), &[handle.port()]);
}
